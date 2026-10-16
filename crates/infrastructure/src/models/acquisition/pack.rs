use super::{
    http::{ArtifactHttp, bounded, status},
    huggingface,
    sources::SourceIngress,
};
use application::models::acquisition::*;
use async_trait::async_trait;
use domain::{
    models::{acquisition::*, library::ModelCategory},
    shared::model_values::JobId,
};
use serde_json::{Value, json};
use std::sync::Arc;
use url::Url;

pub struct ResourcePacks {
    pub http: ArtifactHttp,
    pub ingress: Arc<SourceIngress>,
}
impl ResourcePacks {
    async fn fetch(&self, url: &str) -> Result<Value> {
        let url = Url::parse(url).map_err(|_| DownloadErrorCode::InvalidSource)?;
        if url.scheme() != "https" {
            return Err(DownloadErrorCode::InvalidSource.into());
        }
        let response = self
            .http
            .send(
                reqwest::Method::GET,
                url,
                None,
                reqwest::header::HeaderMap::new(),
                false,
                true,
            )
            .await?
            .response;
        status(response.status().as_u16())?;
        serde_json::from_slice(&bounded(response, 8 << 20).await?)
            .map_err(|_| DownloadErrorCode::InvalidMetadata.into())
    }
    fn rows(&self, value: &Value) -> Result<Vec<PackRow>> {
        let array = value.as_array().ok_or(DownloadErrorCode::InvalidMetadata)?;
        if array.len() > 10_000 {
            return Err(DownloadErrorCode::InvalidMetadata.into());
        }
        array
            .iter()
            .map(|row| {
                let name = row
                    .get("name")
                    .and_then(Value::as_str)
                    .filter(|s| s.len() <= 4096)
                    .ok_or(DownloadErrorCode::InvalidMetadata)?;
                let category = row
                    .get("type")
                    .or_else(|| row.get("model_type"))
                    .and_then(Value::as_str)
                    .ok_or(DownloadErrorCode::InvalidMetadata)?;
                if row
                    .get("type")
                    .zip(row.get("model_type"))
                    .is_some_and(|(a, b)| a != b)
                {
                    return Err(DownloadErrorCode::InvalidMetadata.into());
                }
                let category =
                    ModelCategory::new(category).map_err(|_| DownloadErrorCode::InvalidSource)?;
                let url = row
                    .get("url")
                    .and_then(Value::as_str)
                    .ok_or(DownloadErrorCode::InvalidMetadata)?;
                Ok(PackRow {
                    name: name.into(),
                    source: self.ingress.stage(url, false)?,
                    category,
                })
            })
            .collect()
    }
}
#[async_trait]
impl ModelPacks for ResourcePacks {
    async fn parse(&self, input: &str, remote: bool) -> Result<Vec<PackRow>> {
        if input.len() > 8 << 20 {
            return Err(DownloadErrorCode::InvalidMetadata.into());
        }
        let value = if remote {
            self.fetch(input).await?
        } else {
            serde_json::from_str(input).map_err(|_| DownloadErrorCode::InvalidMetadata)?
        };
        if value.is_array() {
            return self.rows(&value);
        }
        let index = value
            .as_object()
            .ok_or(DownloadErrorCode::InvalidMetadata)?;
        let mut result = Vec::new();
        let mut lists = 0;
        for entries in index.values() {
            for entry in entries
                .as_array()
                .ok_or(DownloadErrorCode::InvalidMetadata)?
            {
                lists += 1;
                if lists > 10_000 {
                    return Err(DownloadErrorCode::InvalidMetadata.into());
                }
                let url = entry
                    .get("url")
                    .and_then(Value::as_str)
                    .ok_or(DownloadErrorCode::InvalidMetadata)?;
                let list = self.fetch(url).await?;
                // Exactly index -> resource_list; an index nested here is invalid.
                result.extend(self.rows(&list)?);
                if result.len() > 10_000 {
                    return Err(DownloadErrorCode::InvalidMetadata.into());
                }
            }
        }
        Ok(result)
    }
    fn export(&self, jobs: &[DownloadJob]) -> Result<PackExport> {
        let mut rows = Vec::new();
        let mut excluded: Vec<JobId> = Vec::new();
        for job in jobs {
            let url = match &job.source {
                SourceRef::CivitAi {
                    version_id: Some(v),
                    file_id: Some(f),
                    ..
                } => Some(super::civitai::transfer_url(*v, *f).to_string()),
                SourceRef::HuggingFace {
                    repo_type,
                    repo,
                    revision,
                    path: Some(path),
                } => Some(huggingface::transfer_url(*repo_type, repo, revision, path).to_string()),
                SourceRef::GoogleDrive { file_id } => {
                    Some(format!("https://drive.google.com/file/d/{file_id}/view"))
                }
                SourceRef::Direct {
                    locator,
                    requires_source_refresh: false,
                    ..
                } => Some(locator.clone()),
                _ => None,
            };
            if let Some(url) = url {
                rows.push(json!({"name":job.destination.filename.as_str(),"url":url,"type":job.destination.category.as_str()}));
            } else {
                excluded.push(job.id.clone());
            }
        }
        Ok(PackExport {
            json: serde_json::to_string_pretty(&rows)
                .map_err(|_| DownloadErrorCode::InvalidMetadata)?,
            excluded,
        })
    }
}
