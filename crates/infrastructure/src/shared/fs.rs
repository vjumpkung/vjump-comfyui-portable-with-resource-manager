use std::{
    io::{self, Write},
    path::Path,
};

/// Writes `bytes` to a temporary file beside `path`, flushes it to disk, then atomically
/// replaces `path` (and flushes the directory on Unix, where that is meaningful).
pub fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no parent directory"))?;
    // An exclusive random sibling avoids following an attacker-supplied `.tmp`
    // symlink and keeps concurrent writers from truncating each other's data.
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    #[cfg(unix)]
    if let Ok(directory) = std::fs::File::open(parent) {
        let _ = directory.sync_all();
    }
    Ok(())
}

/// The nearest ancestor of `path` (including itself) that exists.
pub fn nearest_existing(path: &Path) -> Option<&Path> {
    path.ancestors().find(|candidate| candidate.exists())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn replaces_existing_content_and_leaves_no_temporary_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("install ไทย.json");
        atomic_write(&path, b"one").unwrap();
        atomic_write(&path, b"two").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"two");
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn nearest_existing_walks_up_to_a_real_ancestor() {
        let directory = tempfile::tempdir().unwrap();
        let deep = directory.path().join("a").join("b").join("c");
        assert_eq!(nearest_existing(&deep), Some(directory.path()));
        assert_eq!(nearest_existing(directory.path()), Some(directory.path()));
    }

    #[test]
    fn a_preexisting_temporary_sibling_is_never_overwritten() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("install.json");
        let unrelated = directory.path().join("install.json.tmp");
        fs::write(&unrelated, b"unrelated file").unwrap();
        atomic_write(&path, b"new manifest").unwrap();
        assert_eq!(fs::read(path).unwrap(), b"new manifest");
        assert_eq!(fs::read(unrelated).unwrap(), b"unrelated file");
    }
}
