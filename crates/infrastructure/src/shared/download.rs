//! Verified downloads for pinned tools: HTTPS only, streamed to a `.part` file, SHA-256 checked
//! before the file is published. A failed or interrupted download leaves no final file.

use std::{
    fmt::Write as _,
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};

use application::shared::CancelToken;
use sha2::{Digest, Sha256};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DownloadError {
    #[error("the download URL is not allowed (HTTPS is required)")]
    InsecureUrl,
    #[error("the server answered with HTTP {0}")]
    Status(u16),
    #[error("the network connection failed or was interrupted")]
    Network,
    #[error("the download ended before the announced size")]
    Truncated,
    #[error("the download is larger than allowed")]
    TooLarge,
    #[error("the downloaded file does not match its pinned SHA-256")]
    HashMismatch,
    #[error("the file could not be written: {0}")]
    Io(String),
    #[error("the download was cancelled")]
    Cancelled,
}

impl DownloadError {
    /// Transient failures that are worth a retry.
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::Network | Self::Truncated)
            || matches!(self, Self::Status(code) if *code >= 500 || *code == 429)
    }
}

#[derive(Debug, Clone)]
pub struct Downloader {
    client: reqwest::Client,
    allow_loopback_http: bool,
}

pub fn is_loopback_http(url: &str) -> bool {
    ["http://127.0.0.1", "http://localhost", "http://[::1]"]
        .iter()
        .any(|prefix| {
            url.strip_prefix(prefix)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with([':', '/']))
        })
}

impl Downloader {
    pub fn new() -> Self {
        Self::build(false)
    }

    /// Allows `http://127.0.0.1`; only used by tests against local fixture servers.
    pub fn with_loopback_http() -> Self {
        Self::build(true)
    }

    fn build(allow_loopback_http: bool) -> Self {
        let policy = reqwest::redirect::Policy::custom(move |attempt| {
            let url = attempt.url().as_str().to_owned();
            if attempt.previous().len() >= 8 {
                attempt.error("too many redirects")
            } else if url.starts_with("https://") || (allow_loopback_http && is_loopback_http(&url))
            {
                attempt.follow()
            } else {
                attempt.error("redirect to an insecure URL")
            }
        });
        let client = reqwest::Client::builder()
            .user_agent(concat!(
                "vjumpkung-comfyui-desktop/",
                env!("CARGO_PKG_VERSION")
            ))
            .connect_timeout(Duration::from_secs(20))
            .redirect(policy)
            .build()
            .expect("the HTTP client configuration is valid");
        Self {
            client,
            allow_loopback_http,
        }
    }

    pub fn client(&self) -> &reqwest::Client {
        &self.client
    }

    pub fn url_allowed(&self, url: &str) -> bool {
        url.starts_with("https://") || (self.allow_loopback_http && is_loopback_http(url))
    }

    /// Downloads `url` to `destination` after verifying its SHA-256 (lowercase hex).
    pub async fn fetch(
        &self,
        url: &str,
        destination: &Path,
        sha256: &str,
        max_bytes: u64,
        cancel: &CancelToken,
        progress: &(dyn Fn(u64, Option<u64>) + Send + Sync),
    ) -> Result<(), DownloadError> {
        if !self.url_allowed(url) {
            return Err(DownloadError::InsecureUrl);
        }
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| DownloadError::Io(error.kind().to_string()))?;
        }
        let part = part_path(destination);
        let result = self
            .fetch_to_part(url, &part, sha256, max_bytes, cancel, progress)
            .await;
        match result {
            Ok(()) => fs::rename(&part, destination).map_err(|error| {
                let _ = fs::remove_file(&part);
                DownloadError::Io(error.kind().to_string())
            }),
            Err(error) => {
                let _ = fs::remove_file(&part);
                Err(error)
            }
        }
    }

    async fn fetch_to_part(
        &self,
        url: &str,
        part: &Path,
        sha256: &str,
        max_bytes: u64,
        cancel: &CancelToken,
        progress: &(dyn Fn(u64, Option<u64>) + Send + Sync),
    ) -> Result<(), DownloadError> {
        let mut response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|_| DownloadError::Network)?;
        if !response.status().is_success() {
            return Err(DownloadError::Status(response.status().as_u16()));
        }
        let expected_len = response.content_length();
        if expected_len.is_some_and(|length| length > max_bytes) {
            return Err(DownloadError::TooLarge);
        }
        let mut file =
            File::create(part).map_err(|error| DownloadError::Io(error.kind().to_string()))?;
        let mut hasher = Sha256::new();
        let mut received = 0u64;
        let mut last_report = 0u64;
        loop {
            if cancel.is_cancelled() {
                return Err(DownloadError::Cancelled);
            }
            let chunk = tokio::time::timeout(Duration::from_secs(60), response.chunk())
                .await
                .map_err(|_| DownloadError::Network)?
                .map_err(|_| DownloadError::Network)?;
            let Some(chunk) = chunk else { break };
            received += chunk.len() as u64;
            if received > max_bytes {
                return Err(DownloadError::TooLarge);
            }
            hasher.update(&chunk);
            file.write_all(&chunk)
                .map_err(|error| DownloadError::Io(error.kind().to_string()))?;
            if received - last_report >= 1 << 20 {
                last_report = received;
                progress(received, expected_len);
            }
        }
        file.sync_all()
            .map_err(|error| DownloadError::Io(error.kind().to_string()))?;
        drop(file);
        progress(received, expected_len);
        if expected_len.is_some_and(|length| length != received) {
            return Err(DownloadError::Truncated);
        }
        if !hex(&hasher.finalize()).eq_ignore_ascii_case(sha256) {
            return Err(DownloadError::HashMismatch);
        }
        Ok(())
    }
}

impl Default for Downloader {
    fn default() -> Self {
        Self::new()
    }
}

fn part_path(destination: &Path) -> PathBuf {
    let mut name = destination
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_default();
    name.push(".part");
    destination.with_file_name(name)
}

pub fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(text, "{byte:02x}");
    }
    text
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

/// SHA-256 of a file, streamed.
pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    use std::io::Read;
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex(&hasher.finalize()))
}

#[cfg(test)]
mod tests;
