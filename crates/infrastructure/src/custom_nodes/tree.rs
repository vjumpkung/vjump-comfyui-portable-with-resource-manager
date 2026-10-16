//! Tree helpers: link-free validation, deterministic digests and no-replace directory renames.
//! Nothing here follows links or executes node code.
use application::custom_nodes::NodeAppError;
use sha2::{Digest, Sha256};
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

/// Renames a directory or file without replacing an existing destination.
pub fn rename_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::MoveFileExW;
        let wide =
            |path: &Path| -> Vec<u16> { path.as_os_str().encode_wide().chain(Some(0)).collect() };
        // Without MOVEFILE_REPLACE_EXISTING the call fails when the destination exists.
        if unsafe { MoveFileExW(wide(from).as_ptr(), wide(to).as_ptr(), 0) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::ffi::OsStrExt;
        let a = std::ffi::CString::new(from.as_os_str().as_bytes())?;
        let b = std::ffi::CString::new(to.as_os_str().as_bytes())?;
        let result = unsafe {
            libc::renameat2(
                libc::AT_FDCWD,
                a.as_ptr(),
                libc::AT_FDCWD,
                b.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        if result < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
    #[cfg(all(unix, not(target_os = "linux")))]
    {
        if to.symlink_metadata().is_ok() {
            return Err(io::ErrorKind::AlreadyExists.into());
        }
        fs::rename(from, to)
    }
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

    #[test]
    fn rename_never_replaces_an_existing_destination() {
        let directory = tempfile::tempdir().unwrap();
        let (a, b, c) = (
            directory.path().join("a"),
            directory.path().join("b"),
            directory.path().join("c"),
        );
        fs::create_dir(&a).unwrap();
        fs::create_dir(&b).unwrap();
        assert!(rename_no_replace(&a, &b).is_err());
        assert!(a.exists() && b.exists());
        rename_no_replace(&a, &c).unwrap();
        assert!(!a.exists() && c.exists());
    }

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
