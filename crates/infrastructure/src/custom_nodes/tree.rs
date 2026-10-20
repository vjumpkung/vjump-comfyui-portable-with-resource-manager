//! Tree helpers: link-free validation, deterministic digests and pinned archive extraction.
//! Nothing here follows links or executes node code.
use super::paths::filesystem;
use crate::shared::{
    archive::{ExtractLimits, validate_entry_name},
    model_paths::PinnedDirectory,
};
use application::custom_nodes::{NodeAppError, NodePreparePolicy};
use domain::shared::model_values::SafeFileName;
use sha2::{Digest, Sha256};
use std::{collections::HashMap, io::Write, sync::Arc};
use std::{
    fs::{self, File},
    io::{self, Read},
};

pub(crate) const MAX_ENTRIES: usize = 200_000;
const MAX_BYTES: u64 = 8 << 30;
pub(crate) const MAX_NODE_PATH_COMPONENTS: usize = 64;
pub(crate) const MAX_NODE_RELATIVE_PATH_BYTES: usize = 1024;
pub(crate) const MAX_ARCHIVE_NAME_BYTES: usize = 1024;
pub(crate) const MAX_ARCHIVE_PLANNED_NAME_BYTES: usize = 8 * 1024 * 1024;
pub(crate) const ROOT_NATIVE_MESSAGE: &str = "This node has programs or libraries (.dll, .exe and similar) in its top-level folder. Tools could load them before you approve the node, so it cannot be installed.";
pub(crate) const CACHE_SETUP_WARNING: &str =
    "This repository ships __pycache__ content; consented first-run setup allows it.";
pub(crate) const NATIVE_SETUP_WARNING: &str = "This node has top-level native-image entries that tools may load; consented first-run setup allows them.";

pub(crate) fn is_native_image_name(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, extension)| {
        matches!(
            extension.to_uppercase().as_str(),
            "DLL" | "EXE" | "COM" | "CPL" | "DRV" | "OCX" | "SCR" | "SYS"
        )
    })
}

pub(crate) fn refuse_root_native(dir: &Arc<PinnedDirectory>) -> Result<(), NodeAppError> {
    if has_root_native(dir)? {
        return Err(NodeAppError::Failed(ROOT_NATIVE_MESSAGE.to_owned()));
    }
    Ok(())
}

fn has_root_native(dir: &Arc<PinnedDirectory>) -> Result<bool, NodeAppError> {
    if cfg!(windows) {
        let mut entries = 0;
        for name in frame(dir.clone(), String::new(), 0, false, &mut entries)?.names {
            if is_native_image_name(name.as_str()) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

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

pub(crate) fn validate_components(components: &[String]) -> Result<(), NodeAppError> {
    let bytes =
        components.iter().map(String::len).sum::<usize>() + components.len().saturating_sub(1);
    if components.is_empty()
        || components.len() > MAX_NODE_PATH_COMPONENTS
        || bytes > MAX_NODE_RELATIVE_PATH_BYTES
        || components
            .iter()
            .any(|name| SafeFileName::new(name).is_err())
    {
        return Err(NodeAppError::UnsafeSource);
    }
    Ok(())
}

struct Frame {
    directory: Arc<PinnedDirectory>,
    names: std::vec::IntoIter<SafeFileName>,
    relative: String,
    depth: usize,
    cache: bool,
}

fn frame(
    directory: Arc<PinnedDirectory>,
    relative: String,
    depth: usize,
    cache: bool,
    entries: &mut usize,
) -> Result<Frame, NodeAppError> {
    let mut names = Vec::new();
    for entry in fs::read_dir(directory.anchored_path()).map_err(filesystem)? {
        *entries += 1;
        if *entries > MAX_ENTRIES {
            return Err(NodeAppError::UnsafeSource);
        }
        let name = entry
            .map_err(filesystem)?
            .file_name()
            .into_string()
            .map_err(|_| NodeAppError::UnsafeSource)?;
        names.push(SafeFileName::new(name).map_err(|_| NodeAppError::UnsafeSource)?);
    }
    names.sort();
    Ok(Frame {
        directory,
        names: names.into_iter(),
        relative,
        depth,
        cache,
    })
}

/// A single bounded traversal shared by validation, inventory and hashing. Only the
/// active ancestor chain is pinned; every leaf is opened relative to its pinned parent.
pub(crate) fn walk_files(
    directory: &Arc<PinnedDirectory>,
    git_node: bool,
    reject_cache: bool,
    visit: impl FnMut(&Arc<PinnedDirectory>, &SafeFileName, &str, bool) -> Result<(), NodeAppError>,
) -> Result<(), NodeAppError> {
    walk_files_inner(directory, git_node, reject_cache, visit).map(|_| ())
}

fn walk_files_inner(
    directory: &Arc<PinnedDirectory>,
    git_node: bool,
    reject_cache: bool,
    mut visit: impl FnMut(&Arc<PinnedDirectory>, &SafeFileName, &str, bool) -> Result<(), NodeAppError>,
) -> Result<bool, NodeAppError> {
    let mut entries = 0;
    let mut bytes = 0u64;
    let mut shipped_cache = false;
    let mut stack = vec![frame(
        directory.clone(),
        String::new(),
        0,
        false,
        &mut entries,
    )?];
    while let Some(current) = stack.last_mut() {
        let Some(name) = current.names.next() else {
            stack.pop();
            continue;
        };
        let depth = current.depth + 1;
        let relative = if current.relative.is_empty() {
            name.as_str().to_owned()
        } else {
            format!("{}/{}", current.relative, name.as_str())
        };
        if depth > MAX_NODE_PATH_COMPONENTS || relative.len() > MAX_NODE_RELATIVE_PATH_BYTES {
            return Err(NodeAppError::UnsafeSource);
        }
        let cache = current.cache || name.as_str().eq_ignore_ascii_case("__pycache__");
        shipped_cache |= cache;
        if cache && reject_cache {
            return Err(NodeAppError::UnsafeSource);
        }
        #[cfg(test)]
        read_boundary();
        let child = match current.directory.child(&name, false) {
            Ok(child) => Some(child),
            Err(e) if e.kind() == io::ErrorKind::NotADirectory => None,
            Err(e) => return Err(filesystem(e)),
        };
        let leaf = if child.is_none() {
            Some(current.directory.open_read_pin(&name).map_err(filesystem)?)
        } else {
            None
        };
        let meta = match &child {
            Some(child) => child.file.metadata(),
            None => leaf.as_ref().unwrap().metadata(),
        }
        .map_err(filesystem)?;
        bytes = bytes.saturating_add(meta.len());
        if bytes > MAX_BYTES {
            return Err(NodeAppError::UnsafeSource);
        }
        if name.as_str().eq_ignore_ascii_case(".git") {
            if !git_node || depth != 1 || child.is_none() {
                return Err(NodeAppError::UnsafeSource);
            }
            // The directory handle itself was validated before excluding Git's contents.
            continue;
        }
        if let Some(child) = child {
            stack.push(frame(child, relative, depth, cache, &mut entries)?);
        } else {
            visit(
                &current.directory,
                &name,
                &relative,
                cache || app_owned(depth, name.as_str()),
            )?;
        }
    }
    Ok(shipped_cache)
}

pub fn validate_tree(directory: &Arc<PinnedDirectory>, git_node: bool) -> Result<(), NodeAppError> {
    walk_files(directory, git_node, false, |_, _, _, _| Ok(()))
}

pub(crate) fn validate_fresh_git(
    directory: &Arc<PinnedDirectory>,
    policy: NodePreparePolicy,
) -> Result<Vec<String>, NodeAppError> {
    let strict = policy == NodePreparePolicy::Strict;
    let shipped_cache = walk_files_inner(directory, true, strict, |_, _, _, _| Ok(()))?;
    let root_native = has_root_native(directory)?;
    if strict && root_native {
        return Err(NodeAppError::Failed(ROOT_NATIVE_MESSAGE.to_owned()));
    }
    let mut warnings = Vec::new();
    if shipped_cache {
        warnings.push(CACHE_SETUP_WARNING.to_owned());
    }
    if root_native {
        warnings.push(NATIVE_SETUP_WARNING.to_owned());
    }
    Ok(warnings)
}

pub fn sha256_file(parent: &PinnedDirectory, name: &SafeFileName) -> io::Result<String> {
    #[cfg(test)]
    read_boundary();
    let mut file = parent.open_read_pin(name)?;
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
pub fn tree_digest(directory: &Arc<PinnedDirectory>) -> Result<String, NodeAppError> {
    digest_tree(directory, true)
}

pub(crate) fn digest_tree(
    directory: &Arc<PinnedDirectory>,
    git_node: bool,
) -> Result<String, NodeAppError> {
    let mut hasher = Sha256::new();
    walk_files(
        directory,
        git_node,
        false,
        |parent, name, relative, excluded| {
            if excluded {
                return Ok(());
            }
            let file = sha256_file(parent, name).map_err(filesystem)?;
            hasher.update(relative.as_bytes());
            hasher.update([0]);
            hasher.update(file.as_bytes());
            hasher.update(*b"\n");
            Ok(())
        },
    )?;
    Ok(format!(
        "tree:{}",
        crate::shared::download::hex(&hasher.finalize())
    ))
}

/// Reads a small text file without following a link.
pub fn read_text(parent: &PinnedDirectory, name: &SafeFileName, limit: u64) -> Option<String> {
    #[cfg(test)]
    read_boundary();
    let file = parent.open_read_pin(name).ok()?;
    let mut text = String::new();
    file.take(limit + 1).read_to_string(&mut text).ok()?;
    (text.len() as u64 <= limit).then_some(text)
}

#[cfg(test)]
thread_local! {
    static READ_BOUNDARY: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(crate) fn before_read(boundary: impl FnOnce() + 'static) {
    READ_BOUNDARY.with(|slot| *slot.borrow_mut() = Some(Box::new(boundary)));
}

#[cfg(test)]
fn read_boundary() {
    READ_BOUNDARY.with(|slot| {
        if let Some(boundary) = slot.borrow_mut().take() {
            boundary();
        }
    });
}

#[derive(Default)]
struct ComponentTrie(HashMap<(usize, String), TrieEntry>);

struct TrieEntry {
    spelling: String,
    id: usize,
    is_dir: bool,
    explicit: bool,
}

impl ComponentTrie {
    fn add(&mut self, components: &[String], is_dir: bool) -> Result<(), NodeAppError> {
        let mut parent = 0;
        for (index, component) in components.iter().enumerate() {
            let explicit = index + 1 == components.len();
            let wanted_dir = !explicit || is_dir;
            let next_id = self.0.len() + 1;
            let entry = self
                .0
                .entry((parent, component.to_lowercase()))
                .or_insert_with(|| TrieEntry {
                    spelling: component.clone(),
                    id: next_id,
                    is_dir: wanted_dir,
                    explicit: false,
                });
            if entry.spelling != *component
                || entry.is_dir != wanted_dir
                || (explicit && entry.explicit)
            {
                return Err(NodeAppError::UnsafeSource);
            }
            entry.explicit |= explicit;
            parent = entry.id;
        }
        Ok(())
    }
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
    let mut seen = ComponentTrie::default();
    let mut planned = Vec::new();
    let mut total = 0u64;
    let mut compressed = 0u64;
    let mut planned_name_bytes = 0usize;
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
        let decoded = entry.name().map_err(|_| unsafe_source())?;
        planned_name_bytes = planned_name_bytes.saturating_add(decoded.len());
        if decoded.len() > MAX_ARCHIVE_NAME_BYTES
            || planned_name_bytes > MAX_ARCHIVE_PLANNED_NAME_BYTES
        {
            return Err(unsafe_source());
        }
        let components = validate_entry_name(&decoded).map_err(|_| unsafe_source())?;
        validate_components(&components)?;
        if components.iter().any(|name| {
            name.eq_ignore_ascii_case(".git") || name.eq_ignore_ascii_case("__pycache__")
        }) {
            return Err(unsafe_source());
        }
        seen.add(&components, entry.is_dir())?;
        let mut names = Vec::new();
        for name in &components {
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
    use crate::shared::model_paths::pin_absolute;

    #[test]
    fn root_native_names_match_pe_extensions_case_insensitively() {
        for name in ["a.dll", "A.DLL", "c.Exe", "x.ſcr", ".dll"] {
            assert!(is_native_image_name(name), "{name}");
        }
        for name in [
            "x.pyd",
            "x.so",
            "x.bat",
            "check.bat",
            "install.bat",
            "x.dll.txt",
            "dll",
            "x.whl",
        ] {
            assert!(!is_native_image_name(name), "{name}");
        }
    }

    #[test]
    fn digest_ignores_app_files_and_changes_with_content() {
        let directory = tempfile::tempdir().unwrap();
        let node = directory.path().join("node");
        fs::create_dir_all(node.join("sub")).unwrap();
        fs::write(node.join("a.py"), "one").unwrap();
        fs::write(node.join("sub/b.py"), "two").unwrap();
        let first = tree_digest(&pin_absolute(&node).unwrap()).unwrap();
        fs::write(node.join(".vjump-node.json"), "{}").unwrap();
        fs::write(node.join(".tracking"), "a.py\n").unwrap();
        fs::create_dir_all(node.join("__pycache__")).unwrap();
        fs::write(node.join("__pycache__/x.pyc"), "cache").unwrap();
        assert_eq!(first, tree_digest(&pin_absolute(&node).unwrap()).unwrap());
        fs::write(node.join("a.py"), "changed").unwrap();
        assert_ne!(first, tree_digest(&pin_absolute(&node).unwrap()).unwrap());
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
            assert!(validate_tree(&pin_absolute(&node).unwrap(), false).is_err());
            assert!(tree_digest(&pin_absolute(&node).unwrap()).is_err());
        }
    }
}
