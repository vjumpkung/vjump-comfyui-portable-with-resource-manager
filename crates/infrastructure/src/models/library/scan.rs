//! Blocking directory traversal of one model root. It never follows links, never hashes, and
//! only opens files whose size or modification time differ from the catalog.
use super::roots::RootSpec;
use crate::shared::model_paths;
use application::models::library::{ScanWarning, ScanWarningCode};
use domain::{
    models::{
        acquisition::FileIdentity,
        library::{ModelCategory, SidecarSet, is_sidecar_among, sidecar_parts},
    },
    shared::model_values::SafeFileName,
};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};
use walkdir::WalkDir;

/// What the catalog already knows about a path: size and nanosecond mtime.
pub type KnownFiles = HashMap<String, (u64, u128)>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observed {
    pub path: String,
    pub category: ModelCategory,
    pub size: u64,
    pub modified_ns: u128,
    pub sidecars: SidecarSet,
    /// Present for new or changed files only.
    pub identity: Option<FileIdentity>,
}
#[derive(Debug)]
pub struct RootScan {
    pub root_id: String,
    /// False when the traversal was interrupted; rows of an incomplete root are never deleted.
    pub complete: bool,
    pub files: Vec<Observed>,
    pub warnings: Vec<ScanWarning>,
}

fn is_reparse(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        let _ = metadata;
        false
    }
}
fn modified_ns(metadata: &fs::Metadata) -> u128 {
    metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos())
}
fn ignored_name(name: &str) -> bool {
    name.starts_with('.')
        || name.to_ascii_lowercase().ends_with(".part")
        || (name.starts_with("put_") && name.ends_with("_here"))
}

struct Candidate {
    relative: Vec<SafeFileName>,
    size: u64,
    modified_ns: u128,
    absolute: PathBuf,
}

pub fn scan_root(spec: &RootSpec, known: &KnownFiles) -> RootScan {
    let mut scan = RootScan {
        root_id: spec.id.clone(),
        complete: true,
        files: Vec::new(),
        warnings: Vec::new(),
    };
    let warn = |scan: &mut RootScan, code, detail: String| {
        scan.warnings.push(ScanWarning {
            root_id: Some(spec.id.clone()),
            code,
            detail,
        });
    };
    let mut candidates: Vec<Candidate> = Vec::new();
    let mut walker = WalkDir::new(&spec.path)
        .follow_links(false)
        .follow_root_links(false)
        .min_depth(1)
        .sort_by_file_name()
        .into_iter();
    while let Some(entry) = walker.next() {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                scan.complete = false;
                let detail = error
                    .path()
                    .map_or_else(String::new, |p| relative_text(&spec.path, p));
                warn(&mut scan, ScanWarningCode::Unreadable, detail);
                continue;
            }
        };
        let relative_path = relative_text(&spec.path, entry.path());
        let name = entry.file_name().to_string_lossy().into_owned();
        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(_) => {
                scan.complete = false;
                warn(&mut scan, ScanWarningCode::Unreadable, relative_path);
                continue;
            }
        };
        if entry.path_is_symlink() || is_reparse(&metadata) {
            // Links and junctions are never traversed; add the target as its own root instead.
            warn(&mut scan, ScanWarningCode::UnsafeLink, relative_path);
            if entry.file_type().is_dir() {
                walker.skip_current_dir();
            }
            continue;
        }
        if entry.file_type().is_dir() {
            if name.starts_with('.') {
                walker.skip_current_dir();
            } else if SafeFileName::new(name.as_str()).is_err() {
                warn(&mut scan, ScanWarningCode::InvalidName, relative_path);
                walker.skip_current_dir();
            }
            continue;
        }
        if !entry.file_type().is_file() || ignored_name(&name) {
            continue;
        }
        let Some(relative) = components(&spec.path, entry.path()) else {
            warn(&mut scan, ScanWarningCode::InvalidName, relative_path);
            continue;
        };
        candidates.push(Candidate {
            relative,
            size: metadata.len(),
            modified_ns: modified_ns(&metadata),
            absolute: entry.path().to_owned(),
        });
    }

    // Group by directory so sidecars are recognized only beside a same-stem model.
    let mut by_directory: HashMap<Vec<&str>, Vec<usize>> = HashMap::new();
    for (index, candidate) in candidates.iter().enumerate() {
        let directory: Vec<&str> = candidate.relative[..candidate.relative.len() - 1]
            .iter()
            .map(SafeFileName::as_str)
            .collect();
        by_directory.entry(directory).or_default().push(index);
    }
    let mut sidecar_flags: HashMap<usize, SidecarSet> = HashMap::new();
    let mut hidden: Vec<bool> = vec![false; candidates.len()];
    for indexes in by_directory.values() {
        let names: Vec<&str> = indexes
            .iter()
            .map(|&i| candidates[i].relative.last().expect("file name").as_str())
            .collect();
        for &index in indexes {
            let name = candidates[index]
                .relative
                .last()
                .expect("file name")
                .as_str();
            if is_sidecar_among(name, names.iter().copied()) {
                hidden[index] = true;
                // Attribute the sidecar to the models that share its stem.
                let (stem, suffix) = sidecar_parts(name).expect("recognized sidecar");
                for &other in indexes {
                    let other_name = candidates[other]
                        .relative
                        .last()
                        .expect("file name")
                        .as_str();
                    if other != index
                        && sidecar_parts(other_name).is_none()
                        && domain::models::library::stem_of(other_name) == stem
                    {
                        let flags = sidecar_flags.entry(other).or_default();
                        match suffix {
                            ".preview.png" => flags.preview = true,
                            ".civitai.info" => flags.civitai_info = true,
                            _ => flags.json = true,
                        }
                    }
                }
            }
        }
    }

    for (index, candidate) in candidates.into_iter().enumerate() {
        if hidden[index] {
            continue;
        }
        // The default root's first component is the category; loose files there are ignored.
        let category = match &spec.category {
            Some(category) => category.clone(),
            None => {
                if candidate.relative.len() < 2 {
                    continue;
                }
                match ModelCategory::new(candidate.relative[0].as_str()) {
                    Ok(category) => category,
                    Err(_) => continue,
                }
            }
        };
        let path = candidate
            .relative
            .iter()
            .map(SafeFileName::as_str)
            .collect::<Vec<_>>()
            .join("/");
        let unchanged = known.get(&path).is_some_and(|&(size, modified)| {
            size == candidate.size && modified == candidate.modified_ns
        });
        let identity = if unchanged {
            None
        } else {
            Some(identity_of(&candidate))
        };
        scan.files.push(Observed {
            path,
            category,
            size: candidate.size,
            modified_ns: candidate.modified_ns,
            sidecars: sidecar_flags.remove(&index).unwrap_or_default(),
            identity,
        });
    }
    scan
}

fn identity_of(candidate: &Candidate) -> FileIdentity {
    let fallback = FileIdentity {
        size: candidate.size,
        modified_ns: candidate.modified_ns,
        platform_id: String::new(),
        digest: None,
    };
    match fs::File::open(&candidate.absolute) {
        Ok(file) => model_paths::identity(&file).unwrap_or(fallback),
        Err(_) => fallback,
    }
}

fn relative_text(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}
fn components(root: &Path, path: &Path) -> Option<Vec<SafeFileName>> {
    path.strip_prefix(root)
        .ok()?
        .components()
        .map(|c| SafeFileName::new(c.as_os_str().to_str()?).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn spec(path: &Path, category: Option<&str>) -> RootSpec {
        RootSpec {
            id: "models".into(),
            label: "test".into(),
            path: fs::canonicalize(path).unwrap(),
            category: category.map(|c| ModelCategory::new(c).unwrap()),
        }
    }
    fn write(path: &Path, bytes: &[u8]) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    #[test]
    fn sidecars_partials_hidden_files_and_loose_files_are_not_models() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        write(&root.join("checkpoints/foo.safetensors"), b"model");
        write(&root.join("checkpoints/foo.preview.png"), b"png");
        write(&root.join("checkpoints/foo.civitai.info"), b"{}");
        write(&root.join("checkpoints/foo.json"), b"{}");
        write(&root.join("checkpoints/config.json"), b"{}");
        write(&root.join("checkpoints/foo.metadata.json"), b"{}");
        write(&root.join("checkpoints/partial.safetensors.part"), b"x");
        write(&root.join("checkpoints/.download-1.backup"), b"x");
        write(&root.join("checkpoints/put_checkpoints_here"), b"");
        write(&root.join("loras/sub/ไทย model.pt"), b"thai");
        write(&root.join("loose.bin"), b"ignored");
        let scan = scan_root(&spec(root, None), &KnownFiles::new());
        assert!(scan.complete);
        let mut paths: Vec<_> = scan.files.iter().map(|f| f.path.as_str()).collect();
        paths.sort();
        assert_eq!(
            paths,
            [
                "checkpoints/config.json",
                "checkpoints/foo.metadata.json",
                "checkpoints/foo.safetensors",
                "loras/sub/ไทย model.pt"
            ]
        );
        let foo = scan
            .files
            .iter()
            .find(|f| f.path == "checkpoints/foo.safetensors")
            .unwrap();
        assert_eq!(
            foo.sidecars,
            SidecarSet {
                preview: true,
                civitai_info: true,
                json: true
            }
        );
        assert_eq!(foo.category.as_str(), "checkpoints");
    }

    #[test]
    fn an_extra_root_has_a_fixed_category_and_invalid_names_are_reported() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        write(&root.join("nested/a.safetensors"), b"a");
        write(&root.join("bad name .safetensors"), b"b");
        write(&root.join("odd..name.pt"), b"c");
        let scan = scan_root(&spec(root, Some("loras")), &KnownFiles::new());
        let paths: Vec<_> = scan.files.iter().map(|f| f.path.as_str()).collect();
        assert!(paths.contains(&"nested/a.safetensors"));
        assert!(paths.contains(&"bad name .safetensors"));
        assert!(scan.files.iter().all(|f| f.category.as_str() == "loras"));
        assert!(
            scan.warnings
                .iter()
                .any(|w| w.code == ScanWarningCode::InvalidName && w.detail == "odd..name.pt")
        );
    }

    #[test]
    fn unchanged_files_are_not_opened_and_changed_files_get_an_identity() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        write(&root.join("checkpoints/a.pt"), b"aaaa");
        write(&root.join("checkpoints/b.pt"), b"bbbb");
        let first = scan_root(&spec(root, None), &KnownFiles::new());
        assert!(first.files.iter().all(|f| f.identity.is_some()));
        let mut known = KnownFiles::new();
        for file in &first.files {
            known.insert(file.path.clone(), (file.size, file.modified_ns));
        }
        write(&root.join("checkpoints/b.pt"), b"bbbbbbbb");
        let second = scan_root(&spec(root, None), &known);
        let a = second
            .files
            .iter()
            .find(|f| f.path.ends_with("a.pt"))
            .unwrap();
        let b = second
            .files
            .iter()
            .find(|f| f.path.ends_with("b.pt"))
            .unwrap();
        assert!(a.identity.is_none());
        assert!(b.identity.is_some());
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_folders_are_reported_and_never_traversed() {
        let directory = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        write(&outside.path().join("secret.safetensors"), b"x");
        let root = directory.path();
        fs::create_dir_all(root.join("checkpoints")).unwrap();
        std::os::unix::fs::symlink(outside.path(), root.join("checkpoints/linked")).unwrap();
        let scan = scan_root(&spec(root, None), &KnownFiles::new());
        assert!(scan.files.is_empty());
        assert!(
            scan.warnings
                .iter()
                .any(|w| w.code == ScanWarningCode::UnsafeLink)
        );
    }

    #[cfg(windows)]
    #[test]
    fn junctions_are_reported_and_never_traversed() {
        let directory = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        write(&outside.path().join("secret.safetensors"), b"x");
        let root = directory.path();
        fs::create_dir_all(root.join("checkpoints")).unwrap();
        let junction = root.join("checkpoints").join("linked");
        let status = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&junction)
            .arg(outside.path())
            .output()
            .unwrap();
        if !status.status.success() {
            eprintln!("junction creation is unavailable; skipping");
            return;
        }
        let scan = scan_root(&spec(root, None), &KnownFiles::new());
        assert!(scan.files.is_empty(), "{:?}", scan.files);
        assert!(
            scan.warnings
                .iter()
                .any(|w| w.code == ScanWarningCode::UnsafeLink)
        );
    }

    #[test]
    fn two_thousand_cached_files_scan_in_under_two_seconds_without_hashing() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        for n in 0..2000 {
            write(
                &root.join(format!("checkpoints/group{}/model-{n}.safetensors", n % 20)),
                b"x",
            );
        }
        let spec = spec(root, None);
        let first = scan_root(&spec, &KnownFiles::new());
        assert_eq!(first.files.len(), 2000);
        let mut known = KnownFiles::new();
        for file in &first.files {
            known.insert(file.path.clone(), (file.size, file.modified_ns));
        }
        let started = Instant::now();
        let second = scan_root(&spec, &known);
        let elapsed = started.elapsed();
        assert_eq!(second.files.len(), 2000);
        assert!(second.files.iter().all(|f| f.identity.is_none()));
        assert!(elapsed.as_secs_f64() < 2.0, "cached scan took {elapsed:?}");
    }
}
