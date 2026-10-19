//! Hash-verified accelerator artifacts, cached by digest and safe wheel filename.

use std::{
    fs,
    path::{Path, PathBuf},
};

use application::environment::{ProvisionError, StepContext, WheelFetcher};
use async_trait::async_trait;
use domain::{
    environment::{AcceleratorSource, AcceleratorSpec},
    shared::model_values::SafeFileName,
};

use super::profiles::validate_wheel_url;
use crate::shared::{
    download::{DownloadError, Downloader, sha256_file},
    model_paths::pin_absolute,
};

const UNKNOWN_WHEEL_LIMIT: u64 = 2 << 30;

#[derive(Default)]
pub struct Wheels {
    downloader: Downloader,
}

fn cache_error(_: impl std::fmt::Display) -> ProvisionError {
    failed("the wheel cache could not be accessed safely")
}

fn failed(message: &str) -> ProvisionError {
    ProvisionError::Failed(message.to_owned())
}

impl Wheels {
    pub fn new() -> Self {
        Self::default()
    }

    async fn fetch_artifact(
        &self,
        root: &Path,
        spec: &AcceleratorSpec,
        ctx: &StepContext,
    ) -> Result<PathBuf, ProvisionError> {
        let AcceleratorSource::Wheel {
            url,
            filename,
            sha256,
            size,
        } = &spec.source
        else {
            return Err(failed("the component is not a wheel artifact"));
        };
        if ctx.cancel.is_cancelled() {
            return Err(ProvisionError::Cancelled);
        }
        let canonical = fs::canonicalize(root).map_err(cache_error)?;
        let mut directory = pin_absolute(&canonical).map_err(cache_error)?;
        for part in ["data", "cache", "wheels", sha256.as_str()] {
            directory = directory
                .child(&SafeFileName::new(part).map_err(cache_error)?, true)
                .map_err(cache_error)?;
        }
        let destination = directory.path.join(filename.as_str());
        let part_name =
            SafeFileName::new(format!("{}.part", filename.as_str())).map_err(cache_error)?;
        // Refuse nonregular files/reparse points before the shared downloader opens its part.
        for name in [filename, &part_name] {
            if fs::symlink_metadata(directory.path.join(name.as_str())).is_ok() {
                let file = directory.open(name, false, false).map_err(cache_error)?;
                if !file.metadata().map_err(cache_error)?.is_file() {
                    return Err(failed("the wheel cache contains a nonregular file"));
                }
            }
        }
        let limit = size.unwrap_or(UNKNOWN_WHEEL_LIMIT);
        let reusable = destination.is_file()
            && fs::metadata(&destination).is_ok_and(|m| m.len() <= limit)
            && sha256_file(&destination).is_ok_and(|digest| digest == sha256.as_str());
        if !reusable {
            if destination.exists() {
                directory.remove_owned_file(filename).map_err(cache_error)?;
            }
            ctx.log(format!("Downloading {}", spec.name));
            self.downloader
                .fetch(
                    url,
                    &destination,
                    sha256.as_str(),
                    limit,
                    &ctx.cancel,
                    &|done, total| ctx.progress(done, total),
                )
                .await
                .map_err(|error| match error {
                    DownloadError::Cancelled => ProvisionError::Cancelled,
                    error if error.is_retryable() => ProvisionError::Retryable(error.to_string()),
                    error => ProvisionError::Failed(error.to_string()),
                })?;
        }
        if ctx.cancel.is_cancelled() {
            return Err(ProvisionError::Cancelled);
        }
        Ok(destination)
    }
}

#[async_trait]
impl WheelFetcher for Wheels {
    async fn fetch(
        &self,
        root: &Path,
        spec: &AcceleratorSpec,
        ctx: &StepContext,
    ) -> Result<PathBuf, ProvisionError> {
        spec.validate()
            .map_err(|_| failed("invalid accelerator artifact"))?;
        let AcceleratorSource::Wheel { url, .. } = &spec.source else {
            return Err(failed("the component is not a wheel artifact"));
        };
        validate_wheel_url(url).map_err(|_| failed("invalid accelerator URL"))?;
        self.fetch_artifact(root, spec, ctx).await
    }
}

#[cfg(test)]
mod tests;
