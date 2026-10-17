//! Tree helpers: link-free validation, deterministic digests and pinned archive extraction.
//! Nothing here follows links or executes node code.
use super::paths::filesystem;
use crate::shared::{
    archive::{ExtractLimits, validate_entry_name},
    model_paths::PinnedDirectory,
};
use application::custom_nodes::NodeAppError;
use domain::shared::model_values::SafeFileName;
use sha2::{Digest, Sha256};
use std::{collections::HashMap, io::Write, sync::Arc};
use std::{
    fs::{self, File},
    io::{self, Read},
    path::Path,
};
use walkdir::WalkDir;

const MAX_ENTRIES: usize = 200_000;
const MAX_BYTES: u64 = 8 << 30;

/// Names that belong to the app, not to the node, and so are excluded from digests.
fn app_owned(depth: usize, name: &str) -> bool {
    depth == 1 && matches!(name, ".vjump-node.json" | ".tracking")
}

pub(crate) fn is_link(meta: &fs::Metadata) -> bool {
    if meta.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            return true;
        }
    }
    false
}

fn walker(root: &Path) -> impl Iterator<Item = walkdir::Result<walkdir::DirEntry>> {
    WalkDir::new(root)
        .follow_links(false)
        .follow_root_links(false)
        .sort_by_file_name()
        .into_iter()
        .filter_entry(|entry| !(entry.depth() == 1 && entry.file_name() == ".git"))
}

/// Rejects links, reparse points, devices and oversized trees. The top-level `.git` directory is
/// not walked: Git owns it and a clone cannot place links there.
pub fn validate_tree(root: &Path) -> Result<(), NodeAppError> {
    let mut entries = 0usize;
    let mut bytes = 0u64;
    for entry in walker(root) {
        let entry = entry.map_err(|_| NodeAppError::UnsafeSource)?;
        entries += 1;
        let meta = entry
            .path()
            .symlink_metadata()
            .map_err(|_| NodeAppError::UnsafeSource)?;
        if is_link(&meta) || !(meta.is_file() || meta.is_dir()) {
            return Err(NodeAppError::UnsafeSource);
        }
        bytes = bytes.saturating_add(meta.len());
        if entries > MAX_ENTRIES || bytes > MAX_BYTES {
            return Err(NodeAppError::UnsafeSource);
        }
    }
    Ok(())
}

pub fn sha256_file(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 256 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(crate::shared::download::hex(&hasher.finalize()))
}

/// A digest of the node's files and their relative names. Order-stable and link-free.
pub fn tree_digest(root: &Path) -> Result<String, NodeAppError> {
    let mut hasher = Sha256::new();
    let mut entries = 0usize;
    for entry in walker(root) {
        let entry = entry.map_err(|_| NodeAppError::Filesystem)?;
        if entry.depth() == 0 {
            continue;
        }
        let name = entry.file_name().to_string_lossy();
        if app_owned(entry.depth(), &name) || name == "__pycache__" {
            continue;
        }
        let meta = entry
            .path()
            .symlink_metadata()
            .map_err(|_| NodeAppError::Filesystem)?;
        if is_link(&meta) {
            return Err(NodeAppError::UnsafeSource);
        }
        if !meta.is_file() {
            continue;
        }
        if entry
            .path()
            .components()
            .any(|c| c.as_os_str() == "__pycache__")
        {
            continue;
        }
        entries += 1;
        if entries > MAX_ENTRIES {
            return Err(NodeAppError::UnsafeSource);
        }
        let relative = entry
            .path()
            .strip_prefix(root)
            .map_err(|_| NodeAppError::Filesystem)?
            .to_string_lossy()
            .replace('\\', "/");
        let file = sha256_file(entry.path()).map_err(|_| NodeAppError::Filesystem)?;
        hasher.update(relative.as_bytes());
        hasher.update([0]);
        hasher.update(file.as_bytes());
        hasher.update(*b"\n");
    }
    Ok(format!(
        "tree:{}",
        crate::shared::download::hex(&hasher.finalize())
    ))
}

/// Reads a small text file without following a link.
pub fn read_text(path: &Path, limit: u64) -> Option<String> {
    let meta = path.symlink_metadata().ok()?;
    if is_link(&meta) || !meta.is_file() || meta.len() > limit {
        return None;
    }
    fs::read_to_string(path).ok()
}

/// Node ZIP extraction keeps each output parent pinned through creation and writing.
/// Validate the complete archive before creating any entries, as the shared extractor does.
pub(crate) fn extract_zip(
    archive: File,
    staging: &Arc<PinnedDirectory>,
    limits: ExtractLimits,
) -> Result<(), NodeAppError> {
    let unsafe_source = || NodeAppError::UnsafeSource;
    if fs::read_dir(staging.anchored_path())
        .map_err(filesystem)?
        .next()
        .is_some()
    {
        return Err(NodeAppError::Filesystem);
    }
    let archive_bytes = archive.metadata().map_err(filesystem)?.len().max(1);
    let mut zip = zip::ZipArchive::new(archive).map_err(|_| unsafe_source())?;
    if zip.len() > limits.max_entries {
        return Err(unsafe_source());
    }
    let mut seen = HashMap::new();
    let mut planned = Vec::new();
    let mut total = 0u64;
    let mut compressed = 0u64;
    for index in 0..zip.len() {
        let entry = zip.by_index_raw(index).map_err(|_| unsafe_source())?;
        if entry.encrypted()
            || entry.is_symlink()
            || entry.external_attributes() & 0x400 != 0
            || entry.name_raw().contains(&0)
            || entry.size() > entry.compressed_size().saturating_mul(limits.max_ratio)
            || entry
                .unix_mode()
                .is_some_and(|mode| !matches!(mode & 0o170_000, 0 | 0o100_000 | 0o040_000))
        {
            return Err(unsafe_source());
        }
        let components = validate_entry_name(&entry.name().map_err(|_| unsafe_source())?)
            .map_err(|_| unsafe_source())?;
        let mut relative = String::new();
        let mut names = Vec::new();
        for (position, name) in components.iter().enumerate() {
            if !relative.is_empty() {
                relative.push('/');
            }
            relative.push_str(&name.to_lowercase());
            let is_dir = position + 1 < components.len() || entry.is_dir();
            if let Some(previous) = seen.insert(relative.clone(), is_dir)
                && !(previous && is_dir)
            {
                return Err(unsafe_source());
            }
            names.push(SafeFileName::new(name).map_err(|_| unsafe_source())?);
        }
        total = total.saturating_add(entry.size());
        compressed = compressed.saturating_add(entry.compressed_size());
        planned.push((
            index,
            names,
            entry.is_dir(),
            entry.size(),
            entry.unix_mode(),
        ));
    }
    if total > limits.max_total_bytes
        || total > compressed.saturating_mul(limits.max_ratio)
        || total > archive_bytes.saturating_mul(limits.max_ratio)
    {
        return Err(unsafe_source());
    }
    for (index, ..) in &planned {
        zip.by_index(*index).map_err(|_| unsafe_source())?;
    }
    let mut written_total = 0u64;
    for (index, names, is_dir, declared, mode) in planned {
        let (name, parents) = names.split_last().ok_or_else(unsafe_source)?;
        let mut parent = staging.clone();
        for component in parents {
            parent = parent.child(component, true).map_err(filesystem)?;
        }
        if is_dir {
            parent.child(name, true).map_err(filesystem)?;
            continue;
        }
        let mut output = parent.open(name, true, true).map_err(filesystem)?;
        let mut input = zip
            .by_index(index)
            .map_err(|_| unsafe_source())?
            .take(declared.saturating_add(1));
        let mut count = 0u64;
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let n = input.read(&mut buffer).map_err(|_| unsafe_source())?;
            if n == 0 {
                break;
            }
            count += n as u64;
            written_total = written_total.saturating_add(n as u64);
            if count > declared
                || written_total > limits.max_total_bytes
                || written_total > archive_bytes.saturating_mul(limits.max_ratio)
            {
                return Err(unsafe_source());
            }
            output.write_all(&buffer[..n]).map_err(filesystem)?;
        }
        output.sync_all().map_err(filesystem)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            output
                .set_permissions(fs::Permissions::from_mode(
                    if mode.is_some_and(|m| m & 0o111 != 0) {
                        0o755
                    } else {
                        0o644
                    },
                ))
                .map_err(filesystem)?;
        }
        #[cfg(windows)]
        let _ = mode;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digest_ignores_app_files_and_changes_with_content() {
        let directory = tempfile::tempdir().unwrap();
        let node = directory.path().join("node");
        fs::create_dir_all(node.join("sub")).unwrap();
        fs::write(node.join("a.py"), "one").unwrap();
        fs::write(node.join("sub/b.py"), "two").unwrap();
        let first = tree_digest(&node).unwrap();
        fs::write(node.join(".vjump-node.json"), "{}").unwrap();
        fs::write(node.join(".tracking"), "a.py\n").unwrap();
        fs::create_dir_all(node.join("__pycache__")).unwrap();
        fs::write(node.join("__pycache__/x.pyc"), "cache").unwrap();
        assert_eq!(first, tree_digest(&node).unwrap());
        fs::write(node.join("a.py"), "changed").unwrap();
        assert_ne!(first, tree_digest(&node).unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn unix_symlinks_are_rejected() {
        #[cfg(unix)]
        {
            let directory = tempfile::tempdir().unwrap();
            let node = directory.path().join("node");
            fs::create_dir(&node).unwrap();
            std::os::unix::fs::symlink("/etc/passwd", node.join("link")).unwrap();
            assert!(validate_tree(&node).is_err());
            assert!(tree_digest(&node).is_err());
        }
    }
}
