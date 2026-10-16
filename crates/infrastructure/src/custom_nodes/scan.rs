//! Reads installed nodes from `custom_nodes/` and `custom_nodes/.disabled/` using only file
//! reads. Nothing is executed or imported. Provenance read here is an observation.
use super::{
    git::is_object_id,
    provenance::{observed_identity, read_manager_registry, read_marker},
    tree::{is_link, read_text},
};
use crate::{
    environment::source::valid_ref_name, shared::download::sha256_hex,
};
use domain::{
    custom_nodes::*,
    shared::model_values::SafeFileName,
};
use std::{fs, path::Path};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitMeta {
    pub repository: Option<String>,
    pub commit: Option<String>,
    pub branch: Option<String>,
}

/// A stable identifier for a node found on disk, derived from where it lives.
pub fn scan_id(name: &str, single_file: bool) -> NodeId {
    let digest = sha256_hex(format!("{single_file}\0{name}").as_bytes());
    NodeId::new(format!("scan-{}", &digest[..16])).expect("hex digits are valid")
}

fn remote_url(config: &str) -> Option<String> {
    let mut section: Option<String> = None;
    let mut first: Option<String> = None;
    let mut origin: Option<String> = None;
    for line in config.lines() {
        let line = line.trim();
        if let Some(header) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = header
                .strip_prefix("remote \"")
                .and_then(|r| r.strip_suffix('"'))
                .map(str::to_owned);
        } else if let Some(name) = &section
            && let Some((key, value)) = line.split_once('=')
            && key.trim().eq_ignore_ascii_case("url")
        {
            let value = value.trim().to_owned();
            if name == "origin" && origin.is_none() {
                origin = Some(value.clone());
            }
            first.get_or_insert(value);
        }
    }
    origin.or(first)
}

/// Reads HEAD, the branch ref and the remote URL from the `.git` directory as plain files.
pub fn read_git(dir: &Path) -> Option<GitMeta> {
    let git = dir.join(".git");
    let meta = git.symlink_metadata().ok()?;
    if is_link(&meta) || !meta.is_dir() {
        return None;
    }
    let head = read_text(&git.join("HEAD"), 1024)?;
    let head = head.trim();
    let (branch, commit) = if let Some(reference) = head.strip_prefix("ref: refs/heads/") {
        let reference = reference.trim();
        if !valid_ref_name(reference) {
            return None;
        }
        let loose = read_text(&git.join("refs").join("heads").join(reference), 256);
        let commit = loose.map(|t| t.trim().to_owned()).or_else(|| {
            let packed = read_text(&git.join("packed-refs"), 8 << 20)?;
            let wanted = format!("refs/heads/{reference}");
            packed.lines().find_map(|line| {
                let (sha, name) = line.split_once(' ')?;
                (name.trim() == wanted).then(|| sha.to_owned())
            })
        });
        (Some(reference.to_owned()), commit)
    } else {
        (None, Some(head.to_owned()))
    };
    let repository = read_text(&git.join("config"), 256 * 1024)
        .and_then(|c| remote_url(&c))
        .map(|raw| observed_identity(&raw));
    Some(GitMeta {
        repository,
        commit: commit.filter(|c| is_object_id(c)),
        branch,
    })
}

fn classify(dir: &Path) -> (NodeSource, bool) {
    let marker = read_marker(dir);
    if let Some(meta) = read_git(dir) {
        let source = NodeSource::Git {
            repository: meta.repository.clone().unwrap_or_default(),
            commit: meta.commit.unwrap_or_default(),
            branch: meta.branch,
        };
        let conflict = match &marker {
            Some(NodeSource::Git { repository, .. }) => {
                meta.repository.as_deref() != Some(repository.as_str())
            }
            Some(_) => true,
            None => false,
        };
        return (source, conflict);
    }
    if let Some(source) = marker {
        return (source, false);
    }
    if let Some(source) = read_manager_registry(dir) {
        return (source, false);
    }
    (NodeSource::Manual, false)
}

/// Lists directory nodes and top-level `.py` nodes in both locations. App staging, backups and
/// every other dot-prefixed name are never nodes.
pub fn scan(custom_nodes: &Path) -> Vec<InstalledNode> {
    let mut nodes = Vec::new();
    for (location, enabled) in [(custom_nodes.to_path_buf(), true), (custom_nodes.join(".disabled"), false)] {
        let Ok(entries) = fs::read_dir(&location) else {
            continue;
        };
        for entry in entries.flatten() {
            let file_name = entry.file_name();
            let Some(name) = file_name.to_str() else {
                continue;
            };
            if name.starts_with('.') || name == "__pycache__" {
                continue;
            }
            let Ok(safe) = SafeFileName::new(name) else {
                continue;
            };
            let path = entry.path();
            let Ok(meta) = path.symlink_metadata() else {
                continue;
            };
            let linked = is_link(&meta);
            let (single_file, source, conflict) = if meta.is_dir() || (linked && path.is_dir()) {
                if linked {
                    (false, NodeSource::Manual, false)
                } else {
                    let (source, conflict) = classify(&path);
                    (false, source, conflict)
                }
            } else if (meta.is_file() || linked) && name.to_ascii_lowercase().ends_with(".py") {
                (true, NodeSource::Manual, false)
            } else {
                continue;
            };
            nodes.push(InstalledNode {
                id: scan_id(name, single_file),
                name: safe,
                source,
                origin: ProvenanceOrigin::ExternallyObserved,
                enabled,
                single_file,
                tree_digest: String::new(),
                dependencies: DependencyState::NotRun,
                conflict,
                dirty: None,
            });
        }
    }
    nodes.sort_by(|a, b| a.name.as_str().to_lowercase().cmp(&b.name.as_str().to_lowercase()));
    nodes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::custom_nodes::provenance::{MARKER, TRACKING, write_marker};

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    #[test]
    fn scanning_reads_files_only_and_classifies_each_provenance() {
        let directory = tempfile::tempdir().unwrap();
        let cn = directory.path();
        // A manual single-file node and a manual directory node, one disabled.
        write(&cn.join("solo.py"), "raise SystemExit('must never run')");
        write(&cn.join("pack/__init__.py"), "raise SystemExit('must never run')");
        write(&cn.join(".disabled/off/__init__.py"), "x");
        // A Manager registry node.
        write(&cn.join("reg/pyproject.toml"), "[project]\nname = \"reg\"\nversion = \"2.0.0\"\n");
        write(&cn.join("reg").join(TRACKING), "pyproject.toml\n");
        // A Git node with a branch and an HTTPS remote.
        let commit = "a".repeat(40);
        write(&cn.join("gitnode/.git/HEAD"), "ref: refs/heads/main\n");
        write(&cn.join("gitnode/.git/refs/heads/main"), &format!("{commit}\n"));
        write(
            &cn.join("gitnode/.git/config"),
            "[core]\n\tbare = false\n[remote \"origin\"]\n\turl = https://GitHub.com/o/r.git\n",
        );
        // App staging, backups and other dot names are never nodes.
        write(&cn.join(".disabled/.vjump-staging-x/__init__.py"), "x");
        write(&cn.join(".disabled/.vjump-backup-x/__init__.py"), "x");
        write(&cn.join(".hidden/__init__.py"), "x");
        write(&cn.join("__pycache__/x.pyc"), "x");
        write(&cn.join("readme.txt"), "x");

        let nodes = scan(cn);
        let by_name = |n: &str| nodes.iter().find(|node| node.name.as_str() == n).unwrap();
        assert_eq!(nodes.len(), 5, "{nodes:?}");
        assert!(by_name("solo.py").single_file);
        assert_eq!(by_name("pack").source, NodeSource::Manual);
        assert!(!by_name("off").enabled);
        assert_eq!(
            by_name("reg").source,
            NodeSource::Registry { id: "reg".into(), publisher: None, version: "2.0.0".into(), repository: None }
        );
        assert_eq!(
            by_name("gitnode").source,
            NodeSource::Git { repository: "https://github.com/o/r".into(), commit, branch: Some("main".into()) }
        );
        assert!(nodes.iter().all(|n| n.origin == ProvenanceOrigin::ExternallyObserved));
        assert_eq!(by_name("pack").id, scan_id("pack", false));
    }

    #[test]
    fn a_marker_that_disagrees_with_git_metadata_is_a_conflict() {
        let directory = tempfile::tempdir().unwrap();
        let node = directory.path().join("n");
        write(&node.join(".git/HEAD"), &format!("{}\n", "b".repeat(40)));
        write(&node.join(".git/config"), "[remote \"origin\"]\n\turl = https://github.com/o/real\n");
        write_marker(
            &node,
            "n",
            &NodeSource::Git { repository: "https://github.com/o/claimed".into(), commit: "c".repeat(40), branch: None },
            None,
        )
        .unwrap();
        let nodes = scan(directory.path());
        assert!(nodes[0].conflict);
        assert_eq!(
            nodes[0].source,
            NodeSource::Git { repository: "https://github.com/o/real".into(), commit: "b".repeat(40), branch: None }
        );
        assert!(node.join(MARKER).exists());
    }

    #[test]
    fn unsafe_branch_names_in_head_are_not_followed() {
        let directory = tempfile::tempdir().unwrap();
        let node = directory.path().join("n");
        write(&node.join(".git/HEAD"), "ref: refs/heads/../../../secret\n");
        write(&directory.path().join("secret"), "x");
        assert!(read_git(&node).is_none());
    }
}
