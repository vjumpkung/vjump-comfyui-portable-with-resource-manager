//! Files owned by the ComfyUI checkout, cached against the Git index content.
use crate::{
    custom_nodes::git::tracked_paths,
    environment::{
        layout::{Layout, Tool, command},
        tools::git_program,
    },
    shared::process::{self, RunLimits},
};
use application::shared::CancelToken;
use sha2::{Digest, Sha256};
use std::{
    collections::{HashSet, VecDeque},
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    sync::Mutex,
    time::Duration,
};

const MAX_INDEX_BYTES: u64 = 64 * 1024 * 1024;
const CACHE_CAPACITY: usize = 8;

struct CachedInventory {
    comfy: PathBuf,
    fingerprint: [u8; 32],
    paths: HashSet<String>,
}

static CACHE: Mutex<VecDeque<CachedInventory>> = Mutex::new(VecDeque::new());

fn index_fingerprint(comfy: &Path) -> Option<[u8; 32]> {
    let git = comfy.join(".git");
    if !std::fs::symlink_metadata(&git).ok()?.is_dir() {
        return None;
    }
    let index = git.join("index");
    if !std::fs::symlink_metadata(&index).ok()?.is_file() {
        return None;
    }
    let file = File::open(index).ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() || metadata.len() > MAX_INDEX_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(MAX_INDEX_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > MAX_INDEX_BYTES {
        return None;
    }
    Some(Sha256::digest(&bytes).into())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrackedFiles {
    NotRepository,
    Known(HashSet<String>),
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("ComfyUI repository file is protected")]
pub struct Protected;

fn comparable(path: &str) -> std::borrow::Cow<'_, str> {
    #[cfg(windows)]
    {
        path.to_lowercase().into()
    }
    #[cfg(not(windows))]
    {
        path.into()
    }
}

impl TrackedFiles {
    pub fn hides(&self, comfy_rel: &str) -> bool {
        match self {
            Self::Known(paths) => {
                let wanted = comparable(comfy_rel);
                paths.iter().any(|path| comparable(path) == wanted)
            }
            _ => false,
        }
    }

    pub fn guard(&self, comfy_rel: &str) -> Result<(), Protected> {
        match self {
            Self::NotRepository | Self::Unavailable => Ok(()),
            Self::Known(paths) => {
                let wanted = comparable(comfy_rel);
                let prefix = format!("{wanted}/");
                if paths.iter().any(|path| {
                    let path = comparable(path);
                    path == wanted || path.starts_with(&prefix)
                }) {
                    Err(Protected)
                } else {
                    Ok(())
                }
            }
        }
    }
}

pub async fn comfy_tracked_files(root: &Path) -> TrackedFiles {
    comfy_tracked_files_with(root, query).await
}

async fn comfy_tracked_files_with<F>(root: &Path, query: impl FnOnce(Layout) -> F) -> TrackedFiles
where
    F: std::future::Future<Output = TrackedFiles>,
{
    let layout = Layout::new(root);
    match std::fs::symlink_metadata(layout.comfyui_dir().join(".git")) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return TrackedFiles::NotRepository;
        }
        Err(_) => {
            tracing::warn!("ComfyUI repository file protection could not inspect .git");
            return TrackedFiles::Unavailable;
        }
        Ok(_) => {}
    }
    let comfy = layout.comfyui_dir();
    let before = index_fingerprint(&comfy);
    {
        let mut cache = CACHE.lock().unwrap_or_else(|error| error.into_inner());
        if let Some(entry) = cache.iter().find(|entry| entry.comfy == comfy)
            && Some(entry.fingerprint) == before
        {
            return TrackedFiles::Known(entry.paths.clone());
        }
        cache.retain(|entry| entry.comfy != comfy);
    }
    let tracked = query(layout).await;
    let after = index_fingerprint(&comfy);
    if let (Some(before), TrackedFiles::Known(paths)) = (before, &tracked)
        && Some(before) == after
    {
        let mut cache = CACHE.lock().unwrap_or_else(|error| error.into_inner());
        cache.retain(|entry| entry.comfy != comfy);
        cache.push_back(CachedInventory {
            comfy,
            fingerprint: before,
            paths: paths.clone(),
        });
        while cache.len() > CACHE_CAPACITY {
            cache.pop_front();
        }
    }
    tracked
}

async fn query(layout: Layout) -> TrackedFiles {
    let Some(git) = git_program(&layout) else {
        tracing::warn!("ComfyUI repository file protection requires Git");
        return TrackedFiles::Unavailable;
    };
    let spec = command(&layout, Tool::Git, git)
        .args([
            "-c",
            "core.longpaths=true",
            "-c",
            "advice.detachedHead=false",
            "-c",
            "core.autocrlf=input",
            "-c",
            "core.eol=lf",
            "-c",
            "core.quotepath=off",
        ])
        .arg("-C")
        .arg(layout.comfyui_dir())
        .args(["ls-files", "-z", "--", "models", "input", "output"]);
    let limits = RunLimits {
        timeout: Some(Duration::from_secs(30)),
        max_stdout_bytes: 8 * 1024 * 1024,
        ..Default::default()
    };
    match process::run_raw_stdout(&spec, &CancelToken::new(), &limits).await {
        Ok(bytes) => match tracked_paths(&bytes) {
            Ok(paths) => TrackedFiles::Known(paths),
            Err(_) => {
                tracing::warn!("ComfyUI repository file protection could not parse tracked paths");
                TrackedFiles::Unavailable
            }
        },
        Err(_) => {
            tracing::warn!("ComfyUI repository file protection could not list tracked paths");
            TrackedFiles::Unavailable
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
