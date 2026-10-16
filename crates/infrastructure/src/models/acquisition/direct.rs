use super::{
    http::{ArtifactHttp, status},
    sources::SourceIngress,
};
use application::models::acquisition::{ResolvedFile, Result};
use domain::{models::acquisition::*, shared::model_values::SafeFileName};
use percent_encoding::percent_decode_str;
use reqwest::{
    Method,
    header::{CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_RANGE, HeaderMap},
};
use sha2::{Digest, Sha256};
use url::Url;

pub async fn resolve(
    http: &ArtifactHttp,
    ingress: &SourceIngress,
    source: &SourceRef,
) -> Result<ResolvedFile> {
    let url = ingress.executable(source)?;
    let confirmed = matches!(
        source,
        SourceRef::Direct {
            insecure_http_confirmed: true,
            ..
        }
    );
    let mut response = http
        .send(
            Method::HEAD,
            url.clone(),
            None,
            HeaderMap::new(),
            confirmed,
            true,
        )
        .await?
        .response;
    if matches!(response.status().as_u16(), 405 | 501) {
        let mut headers = HeaderMap::new();
        headers.insert(
            reqwest::header::RANGE,
            reqwest::header::HeaderValue::from_static("bytes=0-0"),
        );
        response = http
            .send(Method::GET, url.clone(), None, headers, confirmed, true)
            .await?
            .response;
    }
    status(response.status().as_u16())?;
    let size = if response.status().as_u16() == 206 {
        response
            .headers()
            .get(CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.rsplit('/').next())
            .and_then(|s| s.parse().ok())
    } else {
        response
            .headers()
            .get(CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok())
    };
    let filename = response
        .headers()
        .get(CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok())
        .and_then(content_disposition)
        .or_else(|| url_filename(&url))
        .ok_or(DownloadErrorCode::SelectionRequired)?;
    let identity = ProviderIdentity(format!(
        "url:{}",
        crate::shared::model_paths::hex(&Sha256::digest(url.as_str().as_bytes()))
    ));
    Ok(ResolvedFile {
        source: source.clone(),
        identity,
        filename,
        size: size.map_or(DownloadSize::Unknown, DownloadSize::Exact),
        sha256: None,
        suggested_category: None,
        primary: true,
    })
}
pub fn url_filename(url: &Url) -> Option<SafeFileName> {
    let last = url.path_segments()?.next_back()?;
    SafeFileName::new(percent_decode_str(last).decode_utf8().ok()?.as_ref()).ok()
}
/// Bounded RFC 6266 parameter scanner: semicolons inside quoted strings are data.
pub fn content_disposition(value: &str) -> Option<SafeFileName> {
    if value.len() > 8192 {
        return None;
    }
    let mut fields = Vec::new();
    let mut start = 0;
    let mut quoted = false;
    let mut escaped = false;
    for (i, c) in value.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match c {
            '\\' if quoted => escaped = true,
            '"' => quoted = !quoted,
            ';' if !quoted => {
                fields.push(&value[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    if quoted || escaped {
        return None;
    }
    fields.push(&value[start..]);
    let mut regular = None;
    let mut extended = None;
    for field in fields.into_iter().skip(1) {
        let Some((key, raw)) = field.trim().split_once('=') else {
            continue;
        };
        let raw = raw.trim();
        if key.eq_ignore_ascii_case("filename*") {
            let mut parts = raw.splitn(3, '\'');
            let charset = parts.next()?;
            let _language = parts.next()?;
            let encoded = parts.next()?;
            let bytes: Vec<u8> = percent_decode_str(encoded).collect();
            let decoded = if charset.eq_ignore_ascii_case("utf-8") {
                String::from_utf8(bytes).ok()
            } else if charset.eq_ignore_ascii_case("iso-8859-1") {
                Some(bytes.into_iter().map(char::from).collect())
            } else {
                None
            };
            extended = decoded.and_then(|s| SafeFileName::new(s).ok());
        } else if key.eq_ignore_ascii_case("filename") {
            let decoded = if raw.starts_with('"') && raw.ends_with('"') {
                let mut out = String::new();
                let mut escape = false;
                for c in raw[1..raw.len() - 1].chars() {
                    if escape {
                        out.push(c);
                        escape = false;
                    } else if c == '\\' {
                        escape = true;
                    } else {
                        out.push(c);
                    }
                }
                out
            } else {
                raw.to_owned()
            };
            regular = SafeFileName::new(decoded).ok();
        }
    }
    extended.or(regular)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn disposition_handles_quotes_and_encodings() {
        assert_eq!(
            content_disposition("attachment; filename=\"a;b.bin\"")
                .unwrap()
                .as_str(),
            "a;b.bin"
        );
        assert_eq!(
            content_disposition("attachment; filename=old.bin; filename*=UTF-8''new%20file.bin")
                .unwrap()
                .as_str(),
            "new file.bin"
        );
        assert!(content_disposition("attachment; filename=\"../bad\"").is_none());
    }
}
