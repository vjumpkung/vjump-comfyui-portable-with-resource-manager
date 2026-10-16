use super::http::validate_url;
use application::models::acquisition::Result;
use domain::{
    models::acquisition::*,
    shared::model_values::{RelativeModelPath, SafeFileName},
};
use percent_encoding::percent_decode_str;
use std::{
    collections::HashMap,
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
};
use url::Url;

pub struct SourceIngress {
    next: AtomicU64,
    urls: Mutex<HashMap<u64, Url>>,
}
impl Default for SourceIngress {
    fn default() -> Self {
        // Process-local references must not accidentally resolve to a different URL after restart.
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64
            & 0x7fff_ffff_ffff_0000;
        Self {
            next: AtomicU64::new(seed),
            urls: Mutex::new(HashMap::new()),
        }
    }
}
impl SourceIngress {
    pub fn stage(&self, raw: &str, http_confirmed: bool) -> Result<SourceRef> {
        let url = Url::parse(raw).map_err(|_| DownloadErrorCode::InvalidSource)?;
        validate_url(&url, http_confirmed)?;
        if url.query_pairs().any(|(key, _)| {
            matches!(
                key.to_ascii_lowercase().as_str(),
                "token" | "access_token" | "api_key" | "hf_token"
            )
        }) {
            return Err(DownloadErrorCode::InvalidSource.into());
        }
        // Check original path before Url's dot-segment normalization.
        let tail = raw
            .split_once("://")
            .ok_or(DownloadErrorCode::InvalidSource)?
            .1;
        let path = tail
            .find('/')
            .map(|i| &tail[i..])
            .unwrap_or("")
            .split(['?', '#'])
            .next()
            .unwrap_or("");
        let mut decoded = Vec::new();
        for component in path.trim_start_matches('/').split('/') {
            if component.is_empty() {
                continue;
            }
            let component = percent_decode_str(component)
                .decode_utf8()
                .map_err(|_| DownloadErrorCode::InvalidSource)?
                .into_owned();
            if component == "."
                || component == ".."
                || component.contains('\\')
                || component.contains(':')
                || component.contains('\0')
            {
                return Err(DownloadErrorCode::InvalidSource.into());
            }
            decoded.push(component);
        }
        let host = url.host_str().unwrap_or("");
        if matches!(host, "civitai.com" | "civitai.green" | "civitai.red") {
            let query = |name: &str| {
                url.query_pairs()
                    .find(|(k, _)| k == name)
                    .map(|(_, v)| v.into_owned())
            };
            let number = |s: &str| s.parse::<u64>().ok().filter(|n| *n > 0);
            let (model_id, version_id) = match decoded.as_slice() {
                [a, id, ..] if a == "models" => (
                    Some(number(id).ok_or(DownloadErrorCode::InvalidSource)?),
                    query("modelVersionId")
                        .map(|s| number(&s).ok_or(DownloadErrorCode::InvalidSource))
                        .transpose()?,
                ),
                [a, b, id] if a == "api" && b == "model-versions" => (
                    None,
                    Some(number(id).ok_or(DownloadErrorCode::InvalidSource)?),
                ),
                [a, b, c, id]
                    if a == "api"
                        && ((b == "download" && c == "models")
                            || (b == "v1" && c == "model-versions")) =>
                {
                    (
                        None,
                        Some(number(id).ok_or(DownloadErrorCode::InvalidSource)?),
                    )
                }
                _ => return Err(DownloadErrorCode::InvalidSource.into()),
            };
            return Ok(SourceRef::CivitAi {
                model_id,
                version_id,
                file_id: query("fileId")
                    .map(|s| number(&s).ok_or(DownloadErrorCode::InvalidSource))
                    .transpose()?,
                filters: CivitFilters {
                    file_type: query("type"),
                    format: query("format"),
                    size: query("size"),
                    fp: query("fp"),
                },
            });
        }
        if host == "huggingface.co" {
            let (repo_type, start) = match decoded.first().map(String::as_str) {
                Some("datasets") => (RepoType::Dataset, 1),
                Some("spaces") => (RepoType::Space, 1),
                _ => (RepoType::Model, 0),
            };
            if decoded.len() < start + 2 {
                return Err(DownloadErrorCode::InvalidSource.into());
            }
            for s in &decoded[start..start + 2] {
                SafeFileName::new(s).map_err(|_| DownloadErrorCode::InvalidSource)?;
            }
            let repo = format!("{}/{}", decoded[start], decoded[start + 1]);
            let (revision, path) = if decoded.len() == start + 2 {
                ("main".into(), None)
            } else if matches!(
                decoded.get(start + 2).map(String::as_str),
                Some("resolve" | "blob" | "tree")
            ) && decoded.len() >= start + 4
            {
                let revision = decoded[start + 3].clone();
                if revision
                    .split('/')
                    .any(|s| s.is_empty() || s == "." || s == "..")
                {
                    return Err(DownloadErrorCode::InvalidSource.into());
                }
                // A decoded slash is allowed only in the revision; all file segments are local names.
                for s in &decoded[start + 4..] {
                    SafeFileName::new(s).map_err(|_| DownloadErrorCode::InvalidSource)?;
                }
                let path = if decoded.len() == start + 4 {
                    None
                } else {
                    Some(
                        RelativeModelPath::new(&decoded[start + 4..].join("/"))
                            .map_err(|_| DownloadErrorCode::InvalidSource)?,
                    )
                };
                (revision, path)
            } else {
                return Err(DownloadErrorCode::InvalidSource.into());
            };
            return Ok(SourceRef::HuggingFace {
                repo_type,
                repo,
                revision,
                path,
            });
        }
        if host == "drive.google.com" || host == "drive.usercontent.google.com" {
            if decoded.iter().any(|s| s == "folders") {
                return Err(DownloadErrorCode::DriveFolderUnsupported.into());
            }
            let id = match decoded.as_slice() {
                [a, b, id, ..] if a == "file" && b == "d" => Some(id.clone()),
                _ => url
                    .query_pairs()
                    .find(|(k, _)| k == "id")
                    .map(|(_, v)| v.into_owned()),
            }
            .ok_or(DownloadErrorCode::InvalidSource)?;
            if id.is_empty()
                || id.len() > 200
                || !id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            {
                return Err(DownloadErrorCode::InvalidSource.into());
            }
            return Ok(SourceRef::GoogleDrive { file_id: id });
        }
        for s in &decoded {
            SafeFileName::new(s).map_err(|_| DownloadErrorCode::InvalidSource)?;
        }
        let ephemeral = url.query().is_some();
        let mut locator = url.clone();
        locator.set_query(None);
        locator.set_fragment(None);
        let ingress_id = if ephemeral {
            let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
            crate::shared::logging::register_secret(raw);
            self.urls
                .lock()
                .map_err(|_| DownloadErrorCode::Storage)?
                .insert(id, url);
            Some(id)
        } else {
            None
        };
        Ok(SourceRef::Direct {
            locator: locator.to_string(),
            ingress_id,
            requires_source_refresh: ephemeral,
            insecure_http_confirmed: http_confirmed,
        })
    }
    pub(crate) fn executable(&self, source: &SourceRef) -> Result<Url> {
        if let SourceRef::Direct {
            locator,
            ingress_id,
            requires_source_refresh,
            ..
        } = source
        {
            if let Some(id) = ingress_id {
                return self
                    .urls
                    .lock()
                    .map_err(|_| DownloadErrorCode::Storage)?
                    .get(id)
                    .cloned()
                    .ok_or(DownloadErrorCode::SourceRefreshRequired.into());
            }
            if *requires_source_refresh {
                return Err(DownloadErrorCode::SourceRefreshRequired.into());
            }
            return Url::parse(locator).map_err(|_| DownloadErrorCode::InvalidSource.into());
        }
        Err(DownloadErrorCode::InvalidSource.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_sources_before_url_normalization() {
        let ingress = SourceIngress::default();
        for url in [
            "https://huggingface.co/a/b/resolve/main/%2e%2e/x",
            "https://huggingface.co/a/b/resolve/main/a%2fb",
            "https://x/a/../b",
            "https://u:p@x/a",
            "https://civitai.com/models/1?token=secret",
        ] {
            assert!(ingress.stage(url, false).is_err(), "{url}");
        }
        assert!(
            matches!(ingress.stage("https://huggingface.co/datasets/a/b/blob/refs%2Fpr%2F1/x.bin",false).unwrap(),SourceRef::HuggingFace { repo_type:RepoType::Dataset,revision,.. } if revision=="refs/pr/1")
        );
        assert!(matches!(
            ingress
                .stage("https://civitai.green/models/123?modelVersionId=456", false)
                .unwrap(),
            SourceRef::CivitAi {
                version_id: Some(456),
                ..
            }
        ));
        let signed = ingress
            .stage("https://x/file.bin?signature=secret", false)
            .unwrap();
        assert!(!format!("{signed:?}").contains("secret"));
    }
}
