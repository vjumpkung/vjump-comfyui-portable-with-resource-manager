//! CivitAI identification by SHA-256 and the curated sidecar documents. The credential vault and
//! redirect-safe HTTP client are the acquisition context's; this adapter adds the by-hash lookup,
//! bounded preview decoding and the reader of the details pane.
use crate::models::acquisition::http::{ArtifactHttp, bounded};
use application::models::library::*;
use async_trait::async_trait;
use domain::{
    models::library::{ModelLocation, sidecar_names},
    settings::CredentialKind,
    shared::model_values::Sha256,
};
use image::{ImageDecoder, ImageFormat, ImageReader, Limits};
use reqwest::{Method, header::HeaderMap};
use serde_json::{Value, json};
use std::io::Cursor;
use url::Url;

const INFO_SCHEMA: u64 = 1;
const MAX_LOOKUP_BYTES: usize = 8 << 20;
const MAX_PREVIEW_BYTES: usize = 10 << 20;
const MAX_PIXELS: u64 = 16_000_000;
const MAX_DECODE_ALLOC: u64 = 128 << 20;

pub struct CivitaiClient {
    http: ArtifactHttp,
    base: Url,
}
impl CivitaiClient {
    pub fn new(http: ArtifactHttp) -> Self {
        Self {
            http,
            base: Url::parse("https://civitai.com").expect("constant provider URL"),
        }
    }
    /// A different origin, for offline tests with a local fixture server.
    pub fn with_base(http: ArtifactHttp, base: Url) -> Self {
        Self { http, base }
    }
}

fn map_status(code: u16) -> LibraryError {
    LibraryError(match code {
        401 | 403 => LibraryErrorCode::CredentialRequired,
        500..=599 => LibraryErrorCode::Network,
        _ => LibraryErrorCode::InvalidResponse,
    })
}

/// Builds the version and the curated sidecar document from a by-hash response.
pub fn parse_version(value: &Value) -> Option<CivitaiMatch> {
    let version_id = value.get("id")?.as_u64()?;
    let model_id = value.get("modelId")?.as_u64()?;
    let text = |v: Option<&Value>| {
        v.and_then(Value::as_str)
            .map(|s| s.chars().take(300).collect::<String>())
    };
    let model_name = text(value.get("model").and_then(|m| m.get("name")))?;
    let version_name = text(value.get("name")).unwrap_or_default();
    let base_model = text(value.get("baseModel"));
    let trigger_words: Vec<String> = value
        .get("trainedWords")
        .and_then(Value::as_array)
        .map(|words| {
            words
                .iter()
                .filter_map(|w| text(Some(w)))
                .take(100)
                .collect()
        })
        .unwrap_or_default();
    let image_url = value
        .get("images")
        .and_then(Value::as_array)
        .and_then(|images| {
            images.iter().find_map(|image| {
                // Still images only; videos are never previews.
                let kind = image.get("type").and_then(Value::as_str).unwrap_or("image");
                let url = image.get("url").and_then(Value::as_str)?;
                (kind == "image"
                    && Url::parse(url).is_ok_and(|u| u.scheme() == "https" || u.scheme() == "http"))
                .then(|| url.to_owned())
            })
        });
    let version = CivitaiVersion {
        version_id,
        model_id,
        model_name,
        version_name,
        base_model,
        trigger_words,
        image_url,
    };
    let info_json = serde_json::to_vec_pretty(&json!({
        "schema": INFO_SCHEMA,
        "versionId": version.version_id,
        "modelId": version.model_id,
        "modelName": version.model_name,
        "versionName": version.version_name,
        "baseModel": version.base_model,
        "trainedWords": version.trigger_words,
    }))
    .ok()?;
    Some(CivitaiMatch { version, info_json })
}

/// Reads a curated `.civitai.info` document back.
pub fn parse_info(bytes: &[u8]) -> Option<CivitaiVersion> {
    let value: Value = serde_json::from_slice(bytes).ok()?;
    if value.get("schema")?.as_u64()? != INFO_SCHEMA {
        return None;
    }
    Some(CivitaiVersion {
        version_id: value.get("versionId")?.as_u64()?,
        model_id: value.get("modelId")?.as_u64()?,
        model_name: value.get("modelName")?.as_str()?.to_owned(),
        version_name: value
            .get("versionName")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        base_model: value
            .get("baseModel")
            .and_then(Value::as_str)
            .map(str::to_owned),
        trigger_words: value
            .get("trainedWords")
            .and_then(Value::as_array)
            .map(|w| {
                w.iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default(),
        image_url: None,
    })
}

/// Decodes with limits and re-encodes as a real PNG (never trusts the provider's format).
pub fn transcode_to_png(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let mut limits = Limits::default();
    limits.max_alloc = Some(MAX_DECODE_ALLOC);
    limits.max_image_width = Some(16_384);
    limits.max_image_height = Some(16_384);
    reader.limits(limits);
    let decoder = reader.into_decoder().ok()?;
    let (width, height) = decoder.dimensions();
    if u64::from(width) * u64::from(height) > MAX_PIXELS {
        return None;
    }
    let image = image::DynamicImage::from_decoder(decoder).ok()?;
    let mut out = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
        .ok()?;
    Some(out)
}

#[async_trait]
impl CivitaiLookup for CivitaiClient {
    async fn by_hash(&self, sha256: &Sha256) -> Result<Option<CivitaiMatch>> {
        let url = self
            .base
            .join(&format!(
                "api/v1/model-versions/by-hash/{}",
                sha256.as_str()
            ))
            .map_err(|_| LibraryError(LibraryErrorCode::InvalidRequest))?;
        let http_confirmed = url.scheme() == "http";
        let response = self
            .http
            .send(
                Method::GET,
                url,
                Some(CredentialKind::CivitAi),
                HeaderMap::new(),
                http_confirmed,
                true,
            )
            .await
            .map_err(|_| LibraryError(LibraryErrorCode::Network))?
            .response;
        match response.status().as_u16() {
            404 => return Ok(None),
            200..=299 => {}
            other => return Err(map_status(other)),
        }
        let body = bounded(response, MAX_LOOKUP_BYTES)
            .await
            .map_err(|_| LibraryError(LibraryErrorCode::InvalidResponse))?;
        let value: Value = serde_json::from_slice(&body)
            .map_err(|_| LibraryError(LibraryErrorCode::InvalidResponse))?;
        parse_version(&value)
            .map(Some)
            .ok_or(LibraryError(LibraryErrorCode::InvalidResponse))
    }

    async fn preview_png(&self, version: &CivitaiVersion) -> Result<Option<Vec<u8>>> {
        let Some(url) = version
            .image_url
            .as_deref()
            .and_then(|u| Url::parse(u).ok())
        else {
            return Ok(None);
        };
        let http_confirmed = url.scheme() == "http" && self.base.scheme() == "http";
        let response = self
            .http
            .send(
                Method::GET,
                url,
                Some(CredentialKind::CivitAi),
                HeaderMap::new(),
                http_confirmed,
                false,
            )
            .await
            .map_err(|_| LibraryError(LibraryErrorCode::Network))?
            .response;
        if !response.status().is_success() {
            return Ok(None);
        }
        let Ok(bytes) = bounded(response, MAX_PREVIEW_BYTES).await else {
            return Ok(None);
        };
        // Decoding is CPU-bound; a malformed or oversized image is reported as "no preview".
        Ok(
            tokio::task::spawn_blocking(move || transcode_to_png(&bytes))
                .await
                .ok()
                .flatten(),
        )
    }
}

/// Reads the sidecars for the details pane.
pub struct SidecarDetails {
    pub files: std::sync::Arc<dyn ModelFiles>,
}
#[async_trait]
impl ModelDetailsReader for SidecarDetails {
    async fn read(&self, location: &ModelLocation) -> Result<ModelDetails> {
        let name = location
            .file_name()
            .ok_or(LibraryError(LibraryErrorCode::InvalidRequest))?;
        let sidecars = sidecar_names(name);
        let preview = self
            .files
            .read_sidecar(&location.sibling(sidecars[0].clone()), MAX_PREVIEW_BYTES)
            .await
            .unwrap_or(None);
        let civitai = self
            .files
            .read_sidecar(&location.sibling(sidecars[1].clone()), 1 << 20)
            .await
            .ok()
            .flatten()
            .and_then(|bytes| parse_info(&bytes));
        Ok(ModelDetails {
            civitai,
            preview_png: preview,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        settings::secrets::{CredentialSnapshot, CredentialVault},
        shared::test_http::{Route, TestServer},
    };
    use domain::settings::CredentialKind;
    use std::sync::Arc;

    struct NoCredentials;
    #[async_trait]
    impl CredentialVault for NoCredentials {
        async fn read(
            &self,
            _: CredentialKind,
        ) -> std::result::Result<CredentialSnapshot, application::settings::CredentialStoreError>
        {
            Ok(CredentialSnapshot {
                generation: 0,
                value: None,
            })
        }
        async fn generation(
            &self,
            _: CredentialKind,
        ) -> std::result::Result<u64, application::settings::CredentialStoreError> {
            Ok(0)
        }
    }

    fn png(width: u32, height: u32) -> Vec<u8> {
        let image = image::RgbImage::from_pixel(width, height, image::Rgb([200, 30, 30]));
        let mut out = Vec::new();
        image
            .write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
            .unwrap();
        out
    }
    fn jpeg() -> Vec<u8> {
        let image = image::RgbImage::from_pixel(8, 8, image::Rgb([10, 200, 30]));
        let mut out = Vec::new();
        image
            .write_to(&mut Cursor::new(&mut out), ImageFormat::Jpeg)
            .unwrap();
        out
    }

    fn version_body(server: &str) -> Vec<u8> {
        json!({
            "id": 77, "modelId": 7, "name": "v1.0", "baseModel": "SDXL 1.0",
            "trainedWords": ["trigger one", "trigger two"],
            "model": {"name": "Cool Model", "type": "LORA"},
            "files": [{"url": "https://civitai.com/api/download/models/77?token=secret"}],
            "images": [
                {"type": "video", "url": format!("{server}/video.mp4")},
                {"type": "image", "url": format!("{server}/preview.jpg")}
            ]
        })
        .to_string()
        .into_bytes()
    }

    #[test]
    fn the_curated_document_has_ids_names_base_model_and_trigger_words_only() {
        let found =
            parse_version(&serde_json::from_slice(&version_body("http://x")).unwrap()).unwrap();
        assert_eq!(found.version.version_id, 77);
        assert_eq!(
            found.version.image_url.as_deref(),
            Some("http://x/preview.jpg")
        );
        let text = String::from_utf8(found.info_json.clone()).unwrap();
        assert!(!text.contains("secret"), "{text}");
        assert!(
            !text.contains("http"),
            "no transfer URLs are persisted: {text}"
        );
        let round = parse_info(&found.info_json).unwrap();
        assert_eq!(round.trigger_words, ["trigger one", "trigger two"]);
        assert_eq!(round.base_model.as_deref(), Some("SDXL 1.0"));
        assert!(parse_info(b"{\"schema\":9}").is_none());
        assert!(parse_info(b"not json").is_none());
    }

    #[test]
    fn previews_are_transcoded_to_real_png_and_bombs_are_refused() {
        let out = transcode_to_png(&jpeg()).unwrap();
        assert_eq!(&out[..8], b"\x89PNG\r\n\x1a\n");
        assert!(transcode_to_png(b"not an image").is_none());
        assert!(transcode_to_png(&png(2, 2)).is_some());
        // 5000 x 5000 = 25 megapixels exceeds the 16 megapixel limit.
        assert!(transcode_to_png(&png(5000, 5000)).is_none());
    }

    #[tokio::test]
    async fn lookup_by_hash_parses_the_response_and_a_404_is_not_identified() {
        let server = TestServer::start().await;
        let hash = Sha256::new(&"ab".repeat(32)).unwrap();
        let unknown = Sha256::new(&"cd".repeat(32)).unwrap();
        server.route(
            &format!("/api/v1/model-versions/by-hash/{}", hash.as_str()),
            Route::ok(version_body(&server.base_url)),
        );
        server.route("/preview.jpg", Route::ok(jpeg()));
        let client = CivitaiClient::with_base(
            ArtifactHttp::new(Arc::new(NoCredentials)).unwrap(),
            Url::parse(&server.base_url).unwrap(),
        );
        let found = client.by_hash(&hash).await.unwrap().unwrap();
        assert_eq!(found.version.model_name, "Cool Model");
        let preview = client.preview_png(&found.version).await.unwrap().unwrap();
        assert_eq!(&preview[..4], b"\x89PNG");
        assert!(client.by_hash(&unknown).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn oversized_malformed_or_failing_previews_are_reported_as_missing() {
        let server = TestServer::start().await;
        server.route("/big", Route::ok(vec![0u8; MAX_PREVIEW_BYTES + 1]));
        server.route("/junk", Route::ok(b"<html>not an image</html>".to_vec()));
        server.route("/gone", Route::status(500, "x"));
        let client = CivitaiClient::with_base(
            ArtifactHttp::new(Arc::new(NoCredentials)).unwrap(),
            Url::parse(&server.base_url).unwrap(),
        );
        for path in ["big", "junk", "gone"] {
            let version = CivitaiVersion {
                version_id: 1,
                model_id: 1,
                model_name: "m".into(),
                version_name: String::new(),
                base_model: None,
                trigger_words: Vec::new(),
                image_url: Some(format!("{}/{path}", server.base_url)),
            };
            assert!(
                client.preview_png(&version).await.unwrap().is_none(),
                "{path}"
            );
        }
    }

    #[tokio::test]
    async fn a_malformed_lookup_response_is_an_error_not_a_match() {
        let server = TestServer::start().await;
        let hash = Sha256::new(&"ef".repeat(32)).unwrap();
        server.route(
            &format!("/api/v1/model-versions/by-hash/{}", hash.as_str()),
            Route::ok(b"{\"id\":\"nope\"}".to_vec()),
        );
        let client = CivitaiClient::with_base(
            ArtifactHttp::new(Arc::new(NoCredentials)).unwrap(),
            Url::parse(&server.base_url).unwrap(),
        );
        assert_eq!(
            client.by_hash(&hash).await.unwrap_err().0,
            LibraryErrorCode::InvalidResponse
        );
    }
}
