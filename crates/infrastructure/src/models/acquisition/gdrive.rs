use super::{
    direct::content_disposition,
    http::{ArtifactHttp, bounded, status},
};
use application::models::acquisition::{ResolvedFile, Result};
use domain::{models::acquisition::*, shared::model_values::SafeFileName};
use reqwest::{Method, header::HeaderMap};
use scraper::{Html, Selector};
use url::Url;

pub fn download_url(id: &str) -> Url {
    let mut u = Url::parse("https://drive.usercontent.google.com/download").unwrap();
    u.query_pairs_mut()
        .append_pair("id", id)
        .append_pair("export", "download");
    u
}
pub async fn locate(http: &ArtifactHttp, id: &str) -> Result<(Url, HeaderMap)> {
    let mut url = download_url(id);
    for transition in 0..=2 {
        let response = http
            .send(Method::GET, url, None, HeaderMap::new(), false, true)
            .await?
            .response;
        if response.status().as_u16() == 403 {
            return Err(DownloadErrorCode::DrivePermission.into());
        }
        status(response.status().as_u16())?;
        url = response.url().clone();
        if !response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|s| s.to_ascii_lowercase().contains("text/html"))
        {
            return Ok((url, response.headers().clone()));
        }
        let body = bounded(response, 1 << 20).await?;
        let html = String::from_utf8_lossy(&body);
        url = confirmation(&html, &url)?;
        if transition == 2 {
            return Err(DownloadErrorCode::UnexpectedConfirmationPage.into());
        }
    }
    Err(DownloadErrorCode::UnexpectedConfirmationPage.into())
}
pub fn confirmation(html: &str, base: &Url) -> Result<Url> {
    if html.len() > 1 << 20 {
        return Err(DownloadErrorCode::UnexpectedConfirmationPage.into());
    }
    let lower = html.to_ascii_lowercase();
    if lower.contains("download quota") || lower.contains("too many users have viewed") {
        return Err(DownloadErrorCode::DriveQuota.into());
    }
    if lower.contains("request access")
        || lower.contains("you need access")
        || lower.contains("access denied")
    {
        return Err(DownloadErrorCode::DrivePermission.into());
    }
    let document = Html::parse_document(html);
    let form_selector = Selector::parse("form#download-form").unwrap();
    let input_selector = Selector::parse("input[type=hidden]").unwrap();
    let form = document
        .select(&form_selector)
        .next()
        .ok_or(DownloadErrorCode::UnexpectedConfirmationPage)?;
    if form
        .value()
        .attr("method")
        .is_some_and(|s| !s.eq_ignore_ascii_case("get"))
    {
        return Err(DownloadErrorCode::UnexpectedConfirmationPage.into());
    }
    let mut action = base
        .join(
            form.value()
                .attr("action")
                .ok_or(DownloadErrorCode::UnexpectedConfirmationPage)?,
        )
        .map_err(|_| DownloadErrorCode::UnexpectedConfirmationPage)?;
    if action.scheme() != "https"
        || action.port_or_known_default() != Some(443)
        || !matches!(
            action.host_str(),
            Some("drive.usercontent.google.com" | "drive.google.com")
        )
        || !action.username().is_empty()
        || action.password().is_some()
    {
        return Err(DownloadErrorCode::UnexpectedConfirmationPage.into());
    }
    if !matches!(action.path(), "/download" | "/uc") {
        return Err(DownloadErrorCode::UnexpectedConfirmationPage.into());
    }
    let mut has_confirm = false;
    for input in form.select(&input_selector).take(50) {
        if let (Some(name), Some(value)) = (input.value().attr("name"), input.value().attr("value"))
        {
            if name == "confirm" {
                has_confirm = true;
            }
            action.query_pairs_mut().append_pair(name, value);
        }
    }
    if !has_confirm {
        return Err(DownloadErrorCode::UnexpectedConfirmationPage.into());
    }
    Ok(action)
}
pub async fn resolve(http: &ArtifactHttp, source: &SourceRef) -> Result<ResolvedFile> {
    let SourceRef::GoogleDrive { file_id } = source else {
        return Err(DownloadErrorCode::InvalidSource.into());
    };
    let (_, headers) = locate(http, file_id).await?;
    let filename = headers
        .get("content-disposition")
        .and_then(|v| v.to_str().ok())
        .and_then(content_disposition)
        .or_else(|| SafeFileName::new(format!("{file_id}.bin")).ok())
        .ok_or(DownloadErrorCode::InvalidMetadata)?;
    Ok(ResolvedFile {
        source: source.clone(),
        identity: ProviderIdentity(format!("gdrive:{file_id}")),
        filename,
        size: headers
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse().ok())
            .map_or(DownloadSize::Unknown, DownloadSize::Exact),
        sha256: None,
        suggested_category: None,
        primary: true,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn confirmation_is_bounded_and_allowlisted() {
        let base = download_url("abc");
        let form = "<form id='download-form' action='https://drive.usercontent.google.com/download'><input type='hidden' name='confirm' value='x y&amp;z'></form>";
        assert!(
            confirmation(form, &base)
                .unwrap()
                .as_str()
                .contains("confirm=x+y%26z")
        );
        assert!(
            confirmation(
                &form.replace("drive.usercontent.google.com", "evil.test"),
                &base
            )
            .is_err()
        );
        assert!(
            matches!(confirmation("download quota exceeded",&base),Err(e) if e.0==DownloadErrorCode::DriveQuota)
        );
    }
}
