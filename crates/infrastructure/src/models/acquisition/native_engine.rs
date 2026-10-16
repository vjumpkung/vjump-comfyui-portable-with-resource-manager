use super::{finalize::StagingArea, http, resolver::ProviderResolver};
use application::{models::acquisition::*, shared::CancelToken};
use async_trait::async_trait;
use domain::models::acquisition::*;
use reqwest::{
    Method,
    header::{CONTENT_LENGTH, CONTENT_RANGE, ETAG, HeaderMap, HeaderValue, IF_RANGE, RANGE},
};
use sha2::{Digest, Sha256};
use std::{sync::Arc, time::Instant};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

pub struct NativeDownloader {
    pub resolver: Arc<ProviderResolver>,
    pub staging: Arc<StagingArea>,
    pub repository: Arc<dyn JobRepository>,
}
pub(crate) async fn cancelled(cancel: &CancelToken) {
    while !cancel.is_cancelled() {
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
}
pub fn strong_etag(value: &str) -> bool {
    value.len() >= 2
        && value.len() <= 512
        && value.starts_with('"')
        && value.ends_with('"')
        && !value.starts_with("W/")
        && !value.chars().any(char::is_control)
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContentRange {
    pub start: u64,
    pub end: u64,
    pub total: u64,
}
pub fn content_range(value: &str) -> Option<ContentRange> {
    let range = value.strip_prefix("bytes ")?;
    let (bounds, total) = range.split_once('/')?;
    let (start, end) = bounds.split_once('-')?;
    let result = ContentRange {
        start: start.parse().ok()?,
        end: end.parse().ok()?,
        total: total.parse().ok()?,
    };
    (result.start <= result.end && result.end < result.total).then_some(result)
}
#[async_trait]
impl Downloader for NativeDownloader {
    async fn transfer(
        &self,
        job: &DownloadJob,
        sink: Arc<dyn ProgressSink>,
        cancel: CancelToken,
    ) -> Result<TransferResult> {
        let key = StagingArea::part_key(job);
        let staging = self.staging.clone();
        let open_key = key.clone();
        let file = tokio::task::spawn_blocking(move || {
            staging
                .open_key(&open_key, true, true)
                .map(|(_, _, file)| file)
        })
        .await
        .map_err(|_| DownloadErrorCode::Disk)?
        .map_err(|_| DownloadErrorCode::Disk)?;
        let mut file = tokio::fs::File::from_std(file);
        let mut offset = file
            .metadata()
            .await
            .map_err(|_| DownloadErrorCode::Disk)?
            .len();
        let validator = job
            .validator
            .as_ref()
            .and_then(|ResumeValidator::StrongEtag(v)| strong_etag(v).then_some(v));
        if validator.is_none() {
            offset = 0;
            file.set_len(0).await.map_err(|_| DownloadErrorCode::Disk)?;
        }
        let mut hash = Sha256::new();
        file.seek(std::io::SeekFrom::Start(0))
            .await
            .map_err(|_| DownloadErrorCode::Disk)?;
        if offset > 0 {
            let mut left = offset;
            let mut buffer = vec![0; 1024 * 1024];
            while left > 0 {
                if cancel.is_cancelled() {
                    return Err(DownloadErrorCode::Cancelled.into());
                }
                let n = file
                    .read(&mut buffer)
                    .await
                    .map_err(|_| DownloadErrorCode::Disk)?;
                if n == 0 {
                    return Err(DownloadErrorCode::LengthMismatch.into());
                }
                hash.update(&buffer[..n]);
                left = left.saturating_sub(n as u64);
            }
        }
        let mut headers = HeaderMap::new();
        if offset > 0 {
            headers.insert(
                RANGE,
                HeaderValue::from_str(&format!("bytes={offset}-"))
                    .map_err(|_| DownloadErrorCode::InvalidRange)?,
            );
            headers.insert(
                IF_RANGE,
                HeaderValue::from_str(validator.expect("resume validator"))
                    .map_err(|_| DownloadErrorCode::InvalidRange)?,
            );
        }
        let (url, kind, http_confirmed) = tokio::select! {biased;_=cancelled(&cancel)=>return Err(DownloadErrorCode::Cancelled.into()),r=self.resolver.transfer_url(&job.source)=>r?};
        let result = tokio::select! {biased;_=cancelled(&cancel)=>return Err(DownloadErrorCode::Cancelled.into()),r=self.resolver.http.send(Method::GET,url,kind,headers,http_confirmed,false)=>r?};
        let mut response = result.response;
        let status = response.status().as_u16();
        if status == 403 && result.signed_hop {
            return Err(DownloadErrorCode::SignedUrlExpired.into());
        }
        if matches!(job.source, SourceRef::HuggingFace { .. }) && matches!(status, 401 | 403 | 404)
        {
            return Err(super::huggingface::classify(status, response.headers()).into());
        }
        http::status(status)?;
        if response
            .headers()
            .get("content-encoding")
            .is_some_and(|v| v.as_bytes() != b"identity")
        {
            return Err(DownloadErrorCode::InvalidMetadata.into());
        }
        if response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|s| s.starts_with("text/html"))
        {
            return Err(if matches!(job.source, SourceRef::HuggingFace { .. }) {
                DownloadErrorCode::NativeUnsupportedUseHf
            } else {
                DownloadErrorCode::UnexpectedConfirmationPage
            }
            .into());
        }
        let content_length = response
            .headers()
            .get(CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok());
        let etag = response
            .headers()
            .get(ETAG)
            .and_then(|v| v.to_str().ok())
            .filter(|s| strong_etag(s))
            .map(str::to_owned);
        let expected_body;
        let total;
        if status == 206 {
            let range = response
                .headers()
                .get(CONTENT_RANGE)
                .and_then(|v| v.to_str().ok())
                .and_then(content_range)
                .ok_or(DownloadErrorCode::InvalidRange)?;
            if range.start != offset
                || content_length.is_some_and(|n| n != range.end - range.start + 1)
                || validator
                    .zip(etag.as_ref())
                    .is_some_and(|(old, new)| old != new)
                || matches!(job.size,DownloadSize::Exact(n) if n!=range.total)
            {
                return Err(DownloadErrorCode::InvalidRange.into());
            }
            expected_body = Some(range.end - range.start + 1);
            total = Some(range.total);
        } else if status == 200 {
            offset = 0;
            file.set_len(0).await.map_err(|_| DownloadErrorCode::Disk)?;
            file.seek(std::io::SeekFrom::Start(0))
                .await
                .map_err(|_| DownloadErrorCode::Disk)?;
            hash = Sha256::new();
            expected_body = content_length;
            total = content_length;
            if let (DownloadSize::Exact(expected), Some(actual)) = (job.size, total)
                && expected != actual
            {
                return Err(DownloadErrorCode::LengthMismatch.into());
            }
        } else {
            return Err(DownloadErrorCode::InvalidRange.into());
        }
        let validator = etag.map(ResumeValidator::StrongEtag);
        let mut checkpoint = job.clone();
        checkpoint.validator = validator.clone();
        checkpoint.bytes_done = offset;
        checkpoint.state = DownloadState::Downloading;
        if let Some(n) = total {
            checkpoint.size = DownloadSize::Exact(n);
        }
        self.repository.save(&checkpoint, job.generation).await?;
        let mut body_bytes = 0u64;
        let started = Instant::now();
        let mut sampled = started;
        let mut sampled_bytes = offset;
        let mut speed = 0.0;
        loop {
            let chunk = tokio::select! {biased;_=cancelled(&cancel)=>{file.sync_all().await.map_err(|_|DownloadErrorCode::Disk)?;return Err(DownloadErrorCode::Cancelled.into());},r=response.chunk()=>r.map_err(|_|DownloadErrorCode::Network)?};
            let Some(chunk) = chunk else { break };
            body_bytes = body_bytes
                .checked_add(chunk.len() as u64)
                .ok_or(DownloadErrorCode::LengthMismatch)?;
            if expected_body.is_some_and(|n| body_bytes > n) {
                return Err(DownloadErrorCode::LengthMismatch.into());
            }
            file.write_all(&chunk)
                .await
                .map_err(|_| DownloadErrorCode::Disk)?;
            hash.update(&chunk);
            let now = Instant::now();
            if now.duration_since(sampled) >= PROGRESS_INTERVAL {
                let done = offset + body_bytes;
                let instantaneous =
                    (done - sampled_bytes) as f64 / now.duration_since(sampled).as_secs_f64();
                speed = if speed == 0.0 {
                    instantaneous
                } else {
                    0.3 * instantaneous + 0.7 * speed
                };
                sink.sample(
                    &job.id,
                    job.generation,
                    TransferProgress {
                        bytes: done,
                        total: total.map_or(DownloadSize::Unknown, DownloadSize::Exact),
                        bytes_per_second: speed,
                        eta_seconds: total.filter(|n| *n >= done).and_then(|n| {
                            (speed > 0.0).then_some(((n - done) as f64 / speed) as u64)
                        }),
                        estimated: false,
                    },
                );
                sampled = now;
                sampled_bytes = done;
            }
        }
        file.sync_all().await.map_err(|_| DownloadErrorCode::Disk)?;
        let done = offset + body_bytes;
        if expected_body.is_some_and(|n| n != body_bytes)
            || total.is_some_and(|n| n != done)
            || matches!(job.size,DownloadSize::Exact(n) if n!=done)
        {
            return Err(DownloadErrorCode::LengthMismatch.into());
        }
        if let Some(expected) = &job.expected_sha256
            && crate::shared::model_paths::hex(&hash.finalize()) != expected.as_str()
        {
            return Err(DownloadErrorCode::HashMismatch.into());
        }
        Ok(TransferResult {
            staged_key: key,
            validator,
            size: done,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_malformed_ranges_and_weak_validators() {
        for s in [
            "bytes */10",
            "bytes 2-1/10",
            "bytes 0-10/10",
            "items 0-1/10",
            "bytes 0-1/*",
            "bytes 0-1/2,4-5/8",
        ] {
            assert!(content_range(s).is_none(), "{s}");
        }
        assert!(!strong_etag("W/\"a\""));
        assert!(strong_etag("\"a\""));
    }
}
