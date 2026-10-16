use crate::settings::secrets::CredentialVault;
use application::models::acquisition::{DownloadError, Result};
use domain::{models::acquisition::DownloadErrorCode as E, settings::CredentialKind};
use reqwest::{
    Client, Method, Response,
    header::{ACCEPT_ENCODING, AUTHORIZATION, HeaderMap, LOCATION},
};
use std::{sync::Arc, time::Duration};
use url::Url;

#[derive(Clone)]
pub struct ArtifactHttp {
    pub(crate) client: Client,
    pub(crate) vault: Arc<dyn CredentialVault>,
}
pub struct ArtifactResponse {
    pub response: Response,
    pub signed_hop: bool,
}
impl ArtifactHttp {
    pub fn new(vault: Arc<dyn CredentialVault>) -> Result<Self> {
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_gzip()
            .no_brotli()
            .no_zstd()
            .no_deflate()
            .connect_timeout(Duration::from_secs(15))
            .read_timeout(Duration::from_secs(60))
            .user_agent("vjumpkung-comfyui/0.1")
            .build()
            .map_err(|_| E::Network)?;
        Ok(Self { client, vault })
    }
    pub async fn send(
        &self,
        method: Method,
        mut url: Url,
        kind: Option<CredentialKind>,
        mut headers: HeaderMap,
        http_confirmed: bool,
        metadata: bool,
    ) -> Result<ArtifactResponse> {
        headers.insert(
            ACCEPT_ENCODING,
            reqwest::header::HeaderValue::from_static("identity"),
        );
        headers.remove(AUTHORIZATION);
        let allowed = kind.is_some_and(|kind| provider_origin(&url, kind));
        if allowed {
            let snapshot = self
                .vault
                .read(kind.expect("provider kind"))
                .await
                .map_err(|_| E::TokenRequired)?;
            if let Some(secret) = snapshot.value {
                let mut value =
                    reqwest::header::HeaderValue::from_str(&format!("Bearer {}", secret.expose()))
                        .map_err(|_| E::InvalidToken)?;
                value.set_sensitive(true);
                headers.insert(AUTHORIZATION, value);
            }
        }
        let mut signed_hop = false;
        for redirects in 0..=10 {
            validate_url(&url, http_confirmed)?;
            let mut request = self
                .client
                .request(method.clone(), url.clone())
                .headers(headers.clone());
            if metadata {
                request = request.timeout(Duration::from_secs(30));
            }
            let response = request.send().await.map_err(|_| E::Network)?;
            if !response.status().is_redirection() {
                return Ok(ArtifactResponse {
                    response,
                    signed_hop,
                });
            }
            if redirects == 10 {
                return Err(E::InvalidMetadata.into());
            }
            let location = response
                .headers()
                .get(LOCATION)
                .and_then(|v| v.to_str().ok())
                .ok_or(E::InvalidMetadata)?;
            let next = url.join(location).map_err(|_| E::InvalidMetadata)?;
            validate_url(&next, http_confirmed)?;
            if url.origin() != next.origin() {
                headers.remove(AUTHORIZATION);
                signed_hop = true;
            }
            url = next;
        }
        Err(E::InvalidMetadata.into())
    }
    pub async fn json(&self, url: Url, kind: Option<CredentialKind>) -> Result<serde_json::Value> {
        let response = self
            .send(Method::GET, url, kind, HeaderMap::new(), false, true)
            .await?
            .response;
        if kind == Some(CredentialKind::CivitAi) && matches!(response.status().as_u16(), 401 | 403)
        {
            return Err(E::TokenRequired.into());
        }
        status(response.status().as_u16())?;
        serde_json::from_slice(&bounded(response, 8 << 20).await?)
            .map_err(|_| E::InvalidMetadata.into())
    }
}
pub fn provider_origin(url: &Url, kind: CredentialKind) -> bool {
    url.scheme() == "https"
        && url.port_or_known_default() == Some(443)
        && url.host_str()
            == Some(match kind {
                CredentialKind::CivitAi => "civitai.com",
                CredentialKind::HuggingFace => "huggingface.co",
            })
}
pub fn validate_url(url: &Url, http_confirmed: bool) -> Result<()> {
    if !url.username().is_empty() || url.password().is_some() || url.host_str().is_none() {
        return Err(E::InvalidSource.into());
    }
    match url.scheme() {
        "https" => Ok(()),
        "http" if http_confirmed => Ok(()),
        "http" => Err(E::HttpConfirmationRequired.into()),
        _ => Err(E::InvalidSource.into()),
    }
}
pub fn status(code: u16) -> Result<()> {
    match code {
        200..=299 => Ok(()),
        401 => Err(E::TokenRequired.into()),
        403 => Err(E::HttpClient.into()),
        404 => Err(E::NotFound.into()),
        500..=599 => Err(E::Service.into()),
        _ => Err(E::HttpClient.into()),
    }
}
pub async fn bounded(mut response: Response, limit: usize) -> Result<Vec<u8>> {
    if response.content_length().is_some_and(|n| n > limit as u64) {
        return Err(E::InvalidMetadata.into());
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| DownloadError(E::Network))?
    {
        if body.len().saturating_add(chunk.len()) > limit {
            return Err(E::InvalidMetadata.into());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}
