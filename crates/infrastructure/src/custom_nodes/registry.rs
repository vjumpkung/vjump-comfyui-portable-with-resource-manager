//! The Comfy registry client (`api.comfy.org`): paginated browse and search with a durable cache,
//! exact-version resolution and the release artifact download. Optional metadata that the
//! registry omits stays unknown; nothing is substituted for it.
use super::{
    compat::{Host, evaluate},
    sqlite::SqliteNodes,
};
use crate::shared::{
    clock::rfc3339_utc,
    download::{Downloader, hex},
};
use application::{custom_nodes::*, shared::CancelToken};
use async_trait::async_trait;
use domain::custom_nodes::{Compatibility, CompatibilityMetadata};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    io::Write,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// Unreserved characters stay readable; everything else in a path or query value is encoded.
const COMPONENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

pub const REGISTRY_BASE: &str = "https://api.comfy.org";
/// Bump when the stored normalized shape changes.
const CACHE_SCHEMA: u32 = 2;
const MAX_RESPONSE_BYTES: usize = 8 << 20;
const MAX_ARTIFACT_BYTES: u64 = 512 << 20;

pub type HostProbe = Arc<dyn Fn() -> Host + Send + Sync>;

pub struct RegistryHttp {
    downloader: Downloader,
    base: String,
    cache: Arc<SqliteNodes>,
    host: HostProbe,
}

/// An exact release with everything the installer needs. The transfer URL is never persisted.
#[derive(Debug, Clone)]
pub struct ReleaseInfo {
    pub release: RegistryRelease,
    pub download_url: Option<String>,
    pub metadata: CompatibilityMetadata,
}

/// The identity of a downloaded and hashed artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artifact {
    pub sha256: String,
    pub bytes: u64,
}

impl RegistryHttp {
    pub fn new(
        downloader: Downloader,
        base: impl Into<String>,
        cache: Arc<SqliteNodes>,
        host: HostProbe,
    ) -> Self {
        Self {
            downloader,
            base: base.into().trim_end_matches('/').to_owned(),
            cache,
            host,
        }
    }

    /// What this installation looks like to a node; also used to evaluate staged releases.
    pub fn host(&self) -> Host {
        (self.host)()
    }

    fn url(&self, path: &str) -> Result<String, NodeAppError> {
        let url = format!("{}{path}", self.base);
        if self.downloader.url_allowed(&url) {
            Ok(url)
        } else {
            Err(NodeAppError::InvalidRegistry)
        }
    }

    async fn get_json(&self, path: &str) -> Result<Value, NodeAppError> {
        let url = self.url(path)?;
        let mut response = self
            .downloader
            .client()
            .get(&url)
            .header("Accept", "application/json")
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .map_err(|_| NodeAppError::Offline)?;
        let status = response.status();
        if !status.is_success() {
            return Err(match status.as_u16() {
                429 | 500..=599 => NodeAppError::Offline,
                code => NodeAppError::RegistryStatus(code),
            });
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
        {
            return Err(NodeAppError::InvalidRegistry);
        }
        let mut body = Vec::new();
        while let Some(chunk) = tokio::time::timeout(Duration::from_secs(30), response.chunk())
            .await
            .map_err(|_| NodeAppError::Offline)?
            .map_err(|_| NodeAppError::Offline)?
        {
            body.extend_from_slice(&chunk);
            if body.len() > MAX_RESPONSE_BYTES {
                return Err(NodeAppError::InvalidRegistry);
            }
        }
        serde_json::from_slice(&body).map_err(|_| NodeAppError::InvalidRegistry)
    }

    fn encode(id: &str) -> Result<String, NodeAppError> {
        let valid = !id.is_empty()
            && id.len() <= 100
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'));
        if !valid {
            return Err(NodeAppError::InvalidRequest);
        }
        Ok(utf8_percent_encode(id, COMPONENT).to_string())
    }

    /// Resolves an exact release. `None` asks for the latest release once; callers then pin the
    /// returned version for every later request.
    pub async fn release(
        &self,
        id: &str,
        exact: Option<&str>,
    ) -> Result<ReleaseInfo, NodeAppError> {
        let encoded = Self::encode(id)?;
        let path = match exact {
            Some(version) => format!(
                "/nodes/{encoded}/install?version={}",
                utf8_percent_encode(version, COMPONENT)
            ),
            None => format!("/nodes/{encoded}/install"),
        };
        let value = self.get_json(&path).await?;
        let info = parse_release(&value).ok_or(NodeAppError::InvalidRegistry)?;
        if info.release.node_id != id || exact.is_some_and(|v| v != info.release.version) {
            return Err(NodeAppError::InvalidRegistry);
        }
        Ok(info)
    }

    /// Downloads the artifact of an already resolved release into `destination`.
    pub async fn download(
        &self,
        url: &str,
        destination: &mut std::fs::File,
        cancel: &CancelToken,
    ) -> Result<Artifact, NodeAppError> {
        if !self.downloader.url_allowed(url) {
            return Err(NodeAppError::UnsafeSource);
        }
        let mut response = self
            .downloader
            .client()
            .get(url)
            .send()
            .await
            .map_err(|_| NodeAppError::Offline)?;
        if !response.status().is_success() {
            return Err(NodeAppError::RegistryStatus(response.status().as_u16()));
        }
        let announced = response.content_length();
        if announced.is_some_and(|length| length > MAX_ARTIFACT_BYTES) {
            return Err(NodeAppError::UnsafeSource);
        }
        let mut hasher = Sha256::new();
        let mut received = 0u64;
        loop {
            if cancel.is_cancelled() {
                return Err(NodeAppError::Cancelled);
            }
            let chunk = tokio::time::timeout(Duration::from_secs(60), response.chunk())
                .await
                .map_err(|_| NodeAppError::Offline)?
                .map_err(|_| NodeAppError::Offline)?;
            let Some(chunk) = chunk else { break };
            received += chunk.len() as u64;
            if received > MAX_ARTIFACT_BYTES {
                return Err(NodeAppError::UnsafeSource);
            }
            hasher.update(&chunk);
            destination
                .write_all(&chunk)
                .map_err(|_| NodeAppError::Filesystem)?;
        }
        destination
            .sync_all()
            .map_err(|_| NodeAppError::Filesystem)?;
        if announced.is_some_and(|length| length != received) {
            return Err(NodeAppError::Offline);
        }
        Ok(Artifact {
            sha256: hex(&hasher.finalize()),
            bytes: received,
        })
    }

    fn cache_key(query: &RegistryQuery) -> String {
        let endpoint = if query.search.trim().is_empty() {
            "nodes"
        } else {
            "nodes/search"
        };
        format!(
            "v{CACHE_SCHEMA}|{endpoint}|q={}|page={}|limit={}",
            normalize_query(&query.search),
            query.page,
            query.limit
        )
    }

    fn hydrate(&self, mut page: RegistryPage) -> RegistryPage {
        let host = (self.host)();
        for node in &mut page.nodes {
            node.evaluated = evaluate(&node.compatibility, &host);
        }
        page
    }
}

fn normalize_query(search: &str) -> String {
    search
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn now_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

fn text(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

fn strings(value: &Value, key: &str) -> Option<Vec<String>> {
    value.get(key).and_then(Value::as_array).map(|items| {
        items
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect()
    })
}

fn unknown_evaluation() -> Compatibility {
    Compatibility {
        os: domain::custom_nodes::Support::Unknown,
        accelerator: domain::custom_nodes::Support::Unknown,
        comfyui: domain::custom_nodes::Support::Unknown,
        flagged: false,
        deprecated: false,
    }
}

fn metadata_of(version: &Value, node_status: Option<String>) -> CompatibilityMetadata {
    CompatibilityMetadata {
        supported_os: strings(version, "supported_os"),
        supported_accelerators: strings(version, "supported_accelerators"),
        comfyui_expression: text(version, "supported_comfyui_version"),
        status: text(version, "status").or(node_status),
        deprecated: version.get("deprecated").and_then(Value::as_bool),
    }
}

/// Normalizes one registry node. Absent optional fields stay `None`.
pub(crate) fn parse_node(value: &Value) -> Option<RegistryNode> {
    let id = text(value, "id")?;
    let latest = value.get("latest_version");
    let node_metadata = metadata_of(value, None);
    let node_flagged = node_metadata.is_flagged();
    let node_deprecated = node_metadata.deprecated == Some(true);
    let mut metadata = latest
        .map(|v| metadata_of(v, text(value, "status")))
        .unwrap_or_else(|| node_metadata.clone());
    if node_metadata.is_flagged() {
        metadata.status = node_metadata.status;
    }
    if node_metadata.deprecated == Some(true) {
        metadata.deprecated = Some(true);
    }
    Some(RegistryNode {
        name: text(value, "name").unwrap_or_else(|| id.clone()),
        publisher: value.get("publisher").and_then(|p| text(p, "id")),
        description: text(value, "description"),
        downloads: value.get("downloads").and_then(Value::as_u64),
        github_stars: value.get("github_stars").and_then(Value::as_u64),
        latest_version: latest.and_then(|v| text(v, "version")),
        repository: text(value, "repository"),
        compatibility: metadata,
        evaluated: unknown_evaluation(),
        node_flagged,
        node_deprecated,
        id,
    })
}

pub(crate) fn parse_page(value: &Value, requested: &RegistryQuery) -> Option<RegistryPage> {
    let nodes: Vec<RegistryNode> = value
        .get("nodes")?
        .as_array()?
        .iter()
        .filter_map(parse_node)
        .collect();
    let total = value
        .get("total")
        .and_then(Value::as_u64)
        .unwrap_or(nodes.len() as u64);
    let limit = value
        .get("limit")
        .and_then(Value::as_u64)
        .map_or(requested.limit, |v| v as u32);
    Some(RegistryPage {
        page: value
            .get("page")
            .and_then(Value::as_u64)
            .map_or(requested.page, |v| v as u32),
        total_pages: value.get("totalPages").and_then(Value::as_u64).map_or_else(
            || total.div_ceil(u64::from(limit.max(1))) as u32,
            |v| v as u32,
        ),
        limit,
        total,
        nodes,
    })
}

pub(crate) fn parse_release(value: &Value) -> Option<ReleaseInfo> {
    let version = text(value, "version")?;
    Some(ReleaseInfo {
        release: RegistryRelease {
            node_id: text(value, "node_id").unwrap_or_default(),
            version,
            dependencies: strings(value, "dependencies").unwrap_or_default(),
        },
        download_url: text(value, "downloadUrl"),
        metadata: metadata_of(value, None),
    })
}

fn page_to_json(page: &RegistryPage) -> Value {
    json!({
        "page": page.page, "limit": page.limit, "total": page.total, "total_pages": page.total_pages,
        "nodes": page.nodes.iter().map(|n| json!({
            "id": n.id, "name": n.name, "publisher": n.publisher, "description": n.description,
            "downloads": n.downloads, "github_stars": n.github_stars, "latest_version": n.latest_version,
            "repository": n.repository,
            "os": n.compatibility.supported_os, "accelerators": n.compatibility.supported_accelerators,
            "comfyui": n.compatibility.comfyui_expression, "status": n.compatibility.status,
            "deprecated": n.compatibility.deprecated,
            "node_flagged": n.node_flagged, "node_deprecated": n.node_deprecated,
        })).collect::<Vec<_>>(),
    })
}

fn page_from_json(value: &Value) -> Option<RegistryPage> {
    let nodes = value
        .get("nodes")?
        .as_array()?
        .iter()
        .map(|n| {
            Some(RegistryNode {
                id: text(n, "id")?,
                name: text(n, "name")?,
                publisher: text(n, "publisher"),
                description: text(n, "description"),
                downloads: n.get("downloads").and_then(Value::as_u64),
                github_stars: n.get("github_stars").and_then(Value::as_u64),
                latest_version: text(n, "latest_version"),
                repository: text(n, "repository"),
                compatibility: CompatibilityMetadata {
                    supported_os: strings(n, "os"),
                    supported_accelerators: strings(n, "accelerators"),
                    comfyui_expression: text(n, "comfyui"),
                    status: text(n, "status"),
                    deprecated: n.get("deprecated").and_then(Value::as_bool),
                },
                evaluated: unknown_evaluation(),
                node_flagged: n.get("node_flagged")?.as_bool()?,
                node_deprecated: n.get("node_deprecated")?.as_bool()?,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(RegistryPage {
        page: value.get("page")?.as_u64()? as u32,
        limit: value.get("limit")?.as_u64()? as u32,
        total: value.get("total")?.as_u64()?,
        total_pages: value.get("total_pages")?.as_u64()? as u32,
        nodes,
    })
}

#[async_trait]
impl RegistryClient for RegistryHttp {
    async fn search(
        &self,
        query: RegistryQuery,
        refresh: bool,
        ttl_seconds: u64,
    ) -> Result<CachedPage, NodeAppError> {
        if query.page == 0 || query.limit == 0 || query.limit > 200 || query.search.len() > 200 {
            return Err(NodeAppError::InvalidRequest);
        }
        let key = Self::cache_key(&query);
        let cached = self.cache.cache_get(&key).await.and_then(|(body, at)| {
            let page = page_from_json(&serde_json::from_str(&body).ok()?)?;
            Some((page, at))
        });
        if !refresh
            && let Some((page, at)) = &cached
            && now_seconds().saturating_sub(*at) <= ttl_seconds as i64
        {
            return Ok(CachedPage {
                page: self.hydrate(page.clone()),
                fetched_at: rfc3339_utc((*at).max(0) as u64),
                stale: false,
            });
        }
        let path = if query.search.trim().is_empty() {
            format!("/nodes?page={}&limit={}", query.page, query.limit)
        } else {
            format!(
                "/nodes/search?search={}&page={}&limit={}",
                utf8_percent_encode(&normalize_query(&query.search), COMPONENT),
                query.page,
                query.limit
            )
        };
        match self.get_json(&path).await {
            Ok(value) => {
                let page = parse_page(&value, &query).ok_or(NodeAppError::InvalidRegistry)?;
                let at = now_seconds();
                self.cache
                    .cache_put(&key, &page_to_json(&page).to_string(), at)
                    .await;
                Ok(CachedPage {
                    page: self.hydrate(page),
                    fetched_at: rfc3339_utc(at.max(0) as u64),
                    stale: false,
                })
            }
            // A failed refresh never overwrites good cached data; offline use shows it as stale.
            Err(NodeAppError::Offline) => match cached {
                Some((page, at)) => Ok(CachedPage {
                    page: self.hydrate(page),
                    fetched_at: rfc3339_utc(at.max(0) as u64),
                    stale: true,
                }),
                None => Err(NodeAppError::Offline),
            },
            Err(error) => Err(error),
        }
    }

    async fn node(&self, id: &str) -> Result<RegistryNode, NodeAppError> {
        let value = self
            .get_json(&format!("/nodes/{}", Self::encode(id)?))
            .await?;
        let mut node = parse_node(&value).ok_or(NodeAppError::InvalidRegistry)?;
        if node.id != id {
            return Err(NodeAppError::InvalidRegistry);
        }
        node.evaluated = evaluate(&node.compatibility, &(self.host)());
        Ok(node)
    }

    async fn versions(&self, id: &str) -> Result<Vec<RegistryRelease>, NodeAppError> {
        let value = self
            .get_json(&format!("/nodes/{}/versions", Self::encode(id)?))
            .await?;
        let items = value.as_array().ok_or(NodeAppError::InvalidRegistry)?;
        let mut releases = Vec::new();
        for item in items {
            let mut info = parse_release(item).ok_or(NodeAppError::InvalidRegistry)?;
            if info.release.node_id.is_empty() {
                info.release.node_id = id.to_owned();
            }
            if info.release.node_id != id {
                return Err(NodeAppError::InvalidRegistry);
            }
            releases.push(info.release);
        }
        Ok(releases)
    }

    async fn install(
        &self,
        id: &str,
        exact: Option<&str>,
    ) -> Result<RegistryRelease, NodeAppError> {
        Ok(self.release(id, exact).await?.release)
    }
}
