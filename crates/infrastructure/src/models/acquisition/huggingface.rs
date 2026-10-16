use super::http::{ArtifactHttp, bounded, provider_origin};
use application::models::acquisition::{ResolvedFile, Result};
use domain::{
    models::acquisition::*,
    settings::CredentialKind,
    shared::model_values::{RelativeModelPath, SafeFileName, Sha256},
};
use reqwest::{
    Method,
    header::{HeaderMap, LINK},
};
use serde_json::Value;
use url::Url;

fn encoded(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes())
        .collect::<String>()
        .replace('+', "%20")
}
pub fn transfer_url(
    repo_type: RepoType,
    repo: &str,
    commit: &str,
    path: &RelativeModelPath,
) -> Url {
    let prefix = match repo_type {
        RepoType::Model => "",
        RepoType::Dataset => "datasets/",
        RepoType::Space => "spaces/",
    };
    let path = path
        .components()
        .iter()
        .map(|p| encoded(p.as_str()))
        .collect::<Vec<_>>()
        .join("/");
    Url::parse(&format!(
        "https://huggingface.co/{prefix}{repo}/resolve/{}/{path}",
        encoded(commit)
    ))
    .expect("validated HF source")
}
pub fn classify(status: u16, headers: &HeaderMap) -> DownloadErrorCode {
    let code = headers
        .get("x-error-code")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    match code {
        "GatedRepo" => DownloadErrorCode::GatedLicense,
        "EntryNotFound" => DownloadErrorCode::FileNotFound,
        "InvalidToken" => DownloadErrorCode::InvalidToken,
        "RevisionNotFound" => DownloadErrorCode::RevisionNotFound,
        _ => match status {
            401 | 404 => DownloadErrorCode::RepositoryUnavailable,
            403 => DownloadErrorCode::HttpClient,
            500..=599 => DownloadErrorCode::Service,
            _ => DownloadErrorCode::HttpClient,
        },
    }
}
async fn raw(http: &ArtifactHttp, url: Url, method: Method) -> Result<reqwest::Response> {
    if !provider_origin(&url, CredentialKind::HuggingFace) {
        return Err(DownloadErrorCode::InvalidMetadata.into());
    }
    let snapshot = http
        .vault
        .read(CredentialKind::HuggingFace)
        .await
        .map_err(|_| DownloadErrorCode::TokenRequired)?;
    let mut request = http
        .client
        .request(method, url)
        .header("Accept-Encoding", "identity")
        .timeout(std::time::Duration::from_secs(30));
    if let Some(value) = snapshot.value {
        request = request.bearer_auth(value.expose());
    }
    let response = request
        .send()
        .await
        .map_err(|_| DownloadErrorCode::Network)?;
    if response.status().is_client_error() || response.status().is_server_error() {
        return Err(classify(response.status().as_u16(), response.headers()).into());
    }
    Ok(response)
}
pub async fn resolve(http: &ArtifactHttp, source: &SourceRef) -> Result<Vec<ResolvedFile>> {
    let SourceRef::HuggingFace {
        repo_type,
        repo,
        revision,
        path,
    } = source
    else {
        return Err(DownloadErrorCode::InvalidSource.into());
    };
    let info_url = Url::parse(&format!(
        "https://huggingface.co/api/{}/{repo}/revision/{}",
        repo_type.plural(),
        encoded(revision)
    ))
    .map_err(|_| DownloadErrorCode::InvalidSource)?;
    let info: Value =
        serde_json::from_slice(&bounded(raw(http, info_url, Method::GET).await?, 8 << 20).await?)
            .map_err(|_| DownloadErrorCode::InvalidMetadata)?;
    let commit = info
        .get("sha")
        .and_then(Value::as_str)
        .filter(|s| s.len() == 40 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or(DownloadErrorCode::InvalidMetadata)?
        .to_ascii_lowercase();
    let canonical_repo = info.get("id").and_then(Value::as_str).unwrap_or(repo);
    if canonical_repo.split('/').count() != 2
        || canonical_repo
            .split('/')
            .any(|s| SafeFileName::new(s).is_err())
    {
        return Err(DownloadErrorCode::InvalidMetadata.into());
    }
    let mut entries = Vec::new();
    if path.is_none() {
        let mut next=Some(Url::parse(&format!("https://huggingface.co/api/{}/{canonical_repo}/tree/{commit}?recursive=true&expand=false",repo_type.plural())).unwrap());
        let prefix = format!("/api/{}/{canonical_repo}/tree/{commit}", repo_type.plural());
        let mut visited = std::collections::HashSet::new();
        while let Some(url) = next {
            if !provider_origin(&url, CredentialKind::HuggingFace)
                || url.path() != prefix
                || !visited.insert(url.to_string())
                || visited.len() > 1000
            {
                return Err(DownloadErrorCode::InvalidMetadata.into());
            }
            let response = raw(http, url.clone(), Method::GET).await?;
            next = response
                .headers()
                .get(LINK)
                .and_then(|v| v.to_str().ok())
                .and_then(next_link)
                .map(|s| url.join(&s))
                .transpose()
                .map_err(|_| DownloadErrorCode::InvalidMetadata)?;
            let page: Vec<Value> = serde_json::from_slice(&bounded(response, 8 << 20).await?)
                .map_err(|_| DownloadErrorCode::InvalidMetadata)?;
            entries.extend(
                page.into_iter()
                    .filter(|v| v.get("type").and_then(Value::as_str) == Some("file")),
            );
            if entries.len() > 10_000 {
                return Err(DownloadErrorCode::InvalidMetadata.into());
            }
        }
    } else {
        entries.push(serde_json::json!({"path":path.as_ref().unwrap().as_string()}));
    }
    let mut files = Vec::new();
    for entry in entries {
        let raw_path = entry
            .get("path")
            .and_then(Value::as_str)
            .ok_or(DownloadErrorCode::InvalidMetadata)?;
        let path =
            RelativeModelPath::new(raw_path).map_err(|_| DownloadErrorCode::InvalidMetadata)?;
        let filename = path
            .components()
            .last()
            .cloned()
            .ok_or(DownloadErrorCode::InvalidMetadata)?;
        let tree_hash = entry
            .get("lfs")
            .and_then(|v| v.get("oid"))
            .and_then(Value::as_str)
            .map(Sha256::new)
            .transpose()
            .map_err(|_| DownloadErrorCode::InvalidMetadata)?;
        let tree_size = entry
            .get("lfs")
            .and_then(|v| v.get("size"))
            .or_else(|| entry.get("size"))
            .and_then(Value::as_u64);
        let response = raw(
            http,
            transfer_url(*repo_type, canonical_repo, &commit, &path),
            Method::HEAD,
        )
        .await?;
        let (hash, size) = metadata(
            response.headers(),
            response.status().is_redirection(),
            tree_hash,
            tree_size,
        )?;
        files.push(ResolvedFile {
            identity: ProviderIdentity(format!(
                "hf:{}:{canonical_repo}@{commit}:{raw_path}",
                repo_type.singular()
            )),
            source: SourceRef::HuggingFace {
                repo_type: *repo_type,
                repo: canonical_repo.to_owned(),
                revision: commit.clone(),
                path: Some(path),
            },
            filename,
            size: size.map_or(DownloadSize::Unknown, DownloadSize::Exact),
            sha256: hash,
            suggested_category: None,
            primary: false,
        });
    }
    if files.is_empty() {
        return Err(DownloadErrorCode::FileNotFound.into());
    }
    Ok(files)
}
pub fn metadata(
    headers: &HeaderMap,
    redirect: bool,
    tree_hash: Option<Sha256>,
    tree_size: Option<u64>,
) -> Result<(Option<Sha256>, Option<u64>)> {
    let linked = headers
        .get("x-linked-etag")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| Sha256::new(s.trim_matches('"')).ok());
    if linked
        .as_ref()
        .zip(tree_hash.as_ref())
        .is_some_and(|(a, b)| a != b)
    {
        return Err(DownloadErrorCode::InvalidMetadata.into());
    }
    let size = headers
        .get("x-linked-size")
        .or_else(|| (!redirect).then(|| headers.get("content-length")).flatten())
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok());
    if size.zip(tree_size).is_some_and(|(a, b)| a != b) {
        return Err(DownloadErrorCode::InvalidMetadata.into());
    }
    Ok((linked.or(tree_hash), size.or(tree_size)))
}
fn next_link(link: &str) -> Option<String> {
    link.split(',').find_map(|part| {
        let (uri, params) = part.trim().split_once('>')?;
        (params
            .split(';')
            .any(|s| matches!(s.trim(), "rel=\"next\"" | "rel=next")))
        .then(|| uri.trim_start_matches('<').to_owned())
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn never_uses_redirect_length_or_xet_hash_as_sha() {
        let mut h = HeaderMap::new();
        h.insert("content-length", "123".parse().unwrap());
        h.insert("x-xet-hash", "a".repeat(64).parse().unwrap());
        assert_eq!(metadata(&h, true, None, None).unwrap(), (None, None));
        h.insert(
            "x-linked-etag",
            format!("\"{}\"", "b".repeat(64)).parse().unwrap(),
        );
        assert!(metadata(&h, true, Some(Sha256::new(&"a".repeat(64)).unwrap()), None).is_err());
    }
}
