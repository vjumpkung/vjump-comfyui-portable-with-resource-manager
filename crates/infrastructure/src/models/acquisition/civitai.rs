use super::http::ArtifactHttp;
use application::models::acquisition::{ResolvedFile, Result};
use domain::{
    models::{acquisition::*, library::ModelCategory},
    settings::CredentialKind,
    shared::model_values::{SafeFileName, Sha256},
};
use serde_json::Value;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use url::Url;

pub fn transfer_url(version: u64, file: u64) -> Url {
    Url::parse(&format!(
        "https://civitai.com/api/download/models/{version}?fileId={file}"
    ))
    .expect("constant provider URL")
}
pub async fn resolve(
    http: &ArtifactHttp,
    source: &SourceRef,
    now: u64,
) -> Result<Vec<ResolvedFile>> {
    let SourceRef::CivitAi {
        model_id,
        version_id,
        file_id,
        filters,
    } = source
    else {
        return Err(DownloadErrorCode::InvalidSource.into());
    };
    let version = if let Some(id) = version_id {
        *id
    } else {
        let id = model_id.ok_or(DownloadErrorCode::InvalidSource)?;
        let model = http
            .json(
                Url::parse(&format!("https://civitai.com/api/v1/models/{id}")).unwrap(),
                Some(CredentialKind::CivitAi),
            )
            .await?;
        latest_published(
            model
                .get("modelVersions")
                .and_then(Value::as_array)
                .ok_or(DownloadErrorCode::InvalidMetadata)?,
            now,
        )
        .ok_or(DownloadErrorCode::NotFound)?
    };
    let value = http
        .json(
            Url::parse(&format!(
                "https://civitai.com/api/v1/model-versions/{version}"
            ))
            .unwrap(),
            Some(CredentialKind::CivitAi),
        )
        .await?;
    if value
        .get("earlyAccessEndsAt")
        .and_then(Value::as_str)
        .and_then(|s| OffsetDateTime::parse(s, &Rfc3339).ok())
        .is_some_and(|t| t.unix_timestamp() > now as i64)
        || value.get("paidAccess").and_then(Value::as_bool) == Some(true)
    {
        return Err(DownloadErrorCode::EarlyAccessOrPaid.into());
    }
    select_files(&value, version, *file_id, filters)
}
pub fn latest_published(versions: &[Value], now: u64) -> Option<u64> {
    versions
        .iter()
        .filter_map(|v| {
            let timestamp = OffsetDateTime::parse(v.get("publishedAt")?.as_str()?, &Rfc3339)
                .ok()?
                .unix_timestamp();
            let id = v.get("id")?.as_u64()?;
            (timestamp >= 0 && timestamp <= now as i64).then_some((timestamp, id))
        })
        .max()
        .map(|(_, id)| id)
}
pub fn select_files(
    value: &Value,
    version: u64,
    file_id: Option<u64>,
    filters: &CivitFilters,
) -> Result<Vec<ResolvedFile>> {
    let files = value
        .get("files")
        .and_then(Value::as_array)
        .ok_or(DownloadErrorCode::InvalidMetadata)?;
    let explicit = filters.file_type.is_some()
        || filters.format.is_some()
        || filters.size.is_some()
        || filters.fp.is_some();
    let mut selected = Vec::new();
    for file in files {
        let id = file
            .get("id")
            .and_then(Value::as_u64)
            .ok_or(DownloadErrorCode::InvalidMetadata)?;
        if let Some(expected) = file_id {
            if id != expected {
                continue;
            }
        } else {
            let matches = |key: &str, wanted: &Option<String>, metadata: bool| {
                wanted.as_ref().is_none_or(|w| {
                    let target = if metadata {
                        file.get("metadata").and_then(|v| v.get(key))
                    } else {
                        file.get(key)
                    };
                    target
                        .and_then(Value::as_str)
                        .is_some_and(|s| s.eq_ignore_ascii_case(w))
                })
            };
            if !matches("type", &filters.file_type, false)
                || !matches("format", &filters.format, true)
                || !matches("size", &filters.size, true)
                || !matches("fp", &filters.fp, true)
            {
                continue;
            }
            if !explicit
                && file
                    .get("type")
                    .and_then(Value::as_str)
                    .is_some_and(|s| s != "Model")
            {
                continue;
            }
        }
        let sha = file
            .get("hashes")
            .and_then(|h| h.get("SHA256"))
            .and_then(Value::as_str)
            .and_then(|s| Sha256::new(s).ok())
            .ok_or(DownloadErrorCode::InvalidMetadata)?;
        let name = file
            .get("name")
            .and_then(Value::as_str)
            .and_then(|s| SafeFileName::new(s).ok())
            .ok_or(DownloadErrorCode::InvalidMetadata)?;
        let size = file
            .get("sizeKB")
            .and_then(Value::as_f64)
            .filter(|s| s.is_finite() && *s >= 0.0)
            .map(|s| DownloadSize::Estimated((s * 1024.0) as u64))
            .unwrap_or(DownloadSize::Unknown);
        selected.push(ResolvedFile {
            source: SourceRef::CivitAi {
                model_id: None,
                version_id: Some(version),
                file_id: Some(id),
                filters: CivitFilters::default(),
            },
            identity: ProviderIdentity(format!("civitai:{id}")),
            filename: name,
            size,
            sha256: Some(sha),
            suggested_category: value
                .get("model")
                .and_then(|m| m.get("type"))
                .and_then(Value::as_str)
                .and_then(ModelCategory::suggest_civitai),
            primary: file
                .get("primary")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        });
    }
    if selected.is_empty() {
        return Err(DownloadErrorCode::SelectionNotFound.into());
    }
    Ok(selected)
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn publication_time_not_array_order_and_explicit_selection_is_strict() {
        assert_eq!(
            latest_published(
                &[
                    json!({"id":2,"publishedAt":"2020-01-01T00:00:00Z"}),
                    json!({"id":5,"publishedAt":"2019-01-01T00:00:00Z"}),
                    json!({"id":9,"publishedAt":"2099-01-01T00:00:00Z"})
                ],
                1_700_000_000
            ),
            Some(2)
        );
        let metadata = json!({"files":[{"id":1,"name":"a.bin","hashes":{"SHA256":"a".repeat(64)},"type":"Model"}]});
        assert!(select_files(&metadata, 2, Some(99), &CivitFilters::default()).is_err());
        assert!(
            select_files(
                &metadata,
                2,
                None,
                &CivitFilters {
                    format: Some("GGUF".into()),
                    ..Default::default()
                }
            )
            .is_err()
        );
    }
}
