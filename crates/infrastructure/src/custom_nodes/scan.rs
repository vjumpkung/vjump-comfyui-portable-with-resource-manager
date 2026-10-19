//! Reads installed nodes from `custom_nodes/` and `custom_nodes/.disabled/` using only file
//! reads. Nothing is executed or imported. Provenance read here is an observation.
use super::{
    git::is_object_id,
    paths::safe_name,
    provenance::{observed_identity, read_manager_registry, read_marker},
    tree::{is_link, read_text},
};
use crate::{
    environment::source::valid_ref_name,
    shared::{download::sha256_hex, model_paths::PinnedDirectory},
};
use domain::{custom_nodes::*, shared::model_values::SafeFileName};
use std::{fs, sync::Arc};

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
            let value = value.trim();
            let value = {
                let quoted = value
                    .strip_prefix('"')
                    .and_then(|v| v.strip_suffix('"'))
                    .unwrap_or(value);
                let mut decoded = String::new();
                let mut chars = quoted.chars();
                while let Some(c) = chars.next() {
                    decoded.push(if c == '\\' {
                        match chars.next()? {
                            'n' => '\n',
                            't' => '\t',
                            'b' => '\u{8}',
                            '\\' => '\\',
                            '"' => '"',
                            _ => return None,
                        }
                    } else {
                        c
                    });
                }
                decoded
            };
            if name == "origin" && origin.is_none() {
                origin = Some(value.clone());
            }
            first.get_or_insert(value);
        }
    }
    origin.or(first)
}

/// Reads HEAD, the branch ref and the remote URL from the `.git` directory as plain files.
pub fn read_git(dir: &Arc<PinnedDirectory>) -> Option<GitMeta> {
    let git = dir.child(&safe_name(".git"), false).ok()?;
    let head = read_text(&git, &safe_name("HEAD"), 1024)?;
    let head = head.trim();
    let (branch, commit) = if let Some(reference) = head.strip_prefix("ref: refs/heads/") {
        let reference = reference.trim();
        if !valid_ref_name(reference) {
            return None;
        }
        let components = format!("refs/heads/{reference}")
            .split('/')
            .map(str::to_owned)
            .collect::<Vec<_>>();
        super::tree::validate_components(&components).ok()?;
        let (name, parents) = components.split_last()?;
        let loose = (|| {
            let mut parent = git.clone();
            for component in parents {
                parent = parent
                    .child(&SafeFileName::new(component).ok()?, false)
                    .ok()?;
            }
            read_text(&parent, &SafeFileName::new(name).ok()?, 256)
        })();
        let commit = loose.map(|t| t.trim().to_owned()).or_else(|| {
            let packed = read_text(&git, &safe_name("packed-refs"), 8 << 20)?;
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
    let repository = read_text(&git, &safe_name("config"), 256 * 1024)
        .and_then(|c| remote_url(&c))
        .map(|raw| observed_identity(&raw));
    Some(GitMeta {
        repository,
        commit: commit.filter(|c| is_object_id(c)),
        branch,
    })
}

fn classify(dir: &Arc<PinnedDirectory>) -> (NodeSource, bool) {
    let marker = read_marker(dir).or_else(|| read_manager_registry(dir));
    let meta = read_git(dir);
    let git_present = dir.open_entry(&safe_name(".git")).is_ok();
    if let Some(source) = marker {
        return match (&source, meta) {
            (NodeSource::Git { repository, .. }, Some(meta))
                if meta.repository.as_deref() == Some(repository.as_str()) =>
            {
                (
                    NodeSource::Git {
                        repository: repository.clone(),
                        commit: meta.commit.unwrap_or_default(),
                        branch: meta.branch,
                    },
                    false,
                )
            }
            (NodeSource::Git { .. }, _) => (source, true),
            _ => (source, git_present),
        };
    }
    if let Some(meta) = meta {
        return (
            NodeSource::Git {
                repository: meta.repository.unwrap_or_default(),
                commit: meta.commit.unwrap_or_default(),
                branch: meta.branch,
            },
            false,
        );
    }
    (NodeSource::Manual, false)
}

/// Lists directory nodes and top-level `.py` nodes in both locations. App staging, backups and
/// every other dot-prefixed name are never nodes.
pub fn scan(custom_nodes: &Arc<PinnedDirectory>) -> Vec<InstalledNode> {
    let mut nodes = Vec::new();
    for (location, enabled) in std::iter::once((custom_nodes.clone(), true)).chain(
        custom_nodes
            .child(&safe_name(".disabled"), false)
            .ok()
            .map(|dir| (dir, false)),
    ) {
        let Ok(entries) = fs::read_dir(location.anchored_path()) else {
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
                    let Ok(directory) = location.child(&safe, false) else {
                        continue;
                    };
                    let (source, conflict) = classify(&directory);
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
    nodes.sort_by(|a, b| {
        a.name
            .as_str()
            .to_lowercase()
            .cmp(&b.name.as_str().to_lowercase())
    });
    nodes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::custom_nodes::provenance::{MARKER, TRACKING, write_marker};
    use crate::shared::model_paths::pin_absolute;
    use std::path::Path;

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
        write(
            &cn.join("pack/__init__.py"),
            "raise SystemExit('must never run')",
        );
        write(&cn.join(".disabled/off/__init__.py"), "x");
        // A Manager registry node.
        write(
            &cn.join("reg/pyproject.toml"),
            "[project]\nname = \"reg\"\nversion = \"2.0.0\"\n",
        );
        write(&cn.join("reg").join(TRACKING), "pyproject.toml\n");
        // A Git node with a branch and an HTTPS remote.
        let commit = "a".repeat(40);
        write(&cn.join("gitnode/.git/HEAD"), "ref: refs/heads/main\n");
        write(
            &cn.join("gitnode/.git/refs/heads/main"),
            &format!("{commit}\n"),
        );
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

        let nodes = scan(&pin_absolute(cn).unwrap());
        let by_name = |n: &str| nodes.iter().find(|node| node.name.as_str() == n).unwrap();
        assert_eq!(nodes.len(), 5, "{nodes:?}");
        assert!(by_name("solo.py").single_file);
        assert_eq!(by_name("pack").source, NodeSource::Manual);
        assert!(!by_name("off").enabled);
        assert_eq!(
            by_name("reg").source,
            NodeSource::Registry {
                id: "reg".into(),
                publisher: None,
                version: "2.0.0".into(),
                repository: None
            }
        );
        assert_eq!(
            by_name("gitnode").source,
            NodeSource::Git {
                repository: "https://github.com/o/r".into(),
                commit,
                branch: Some("main".into())
            }
        );
        assert!(
            nodes
                .iter()
                .all(|n| n.origin == ProvenanceOrigin::ExternallyObserved)
        );
        assert_eq!(by_name("pack").id, scan_id("pack", false));
    }

    #[test]
    fn a_marker_that_disagrees_with_git_metadata_is_a_conflict() {
        let directory = tempfile::tempdir().unwrap();
        let node = directory.path().join("n");
        write(&node.join(".git/HEAD"), &format!("{}\n", "b".repeat(40)));
        write(
            &node.join(".git/config"),
            "[remote \"origin\"]\n\turl = https://github.com/o/real\n",
        );
        write_marker(
            &node,
            "n",
            &NodeSource::Git {
                repository: "https://github.com/o/claimed".into(),
                commit: "c".repeat(40),
                branch: None,
            },
            None,
        )
        .unwrap();
        let nodes = scan(&pin_absolute(directory.path()).unwrap());
        assert!(nodes[0].conflict);
        assert_eq!(
            nodes[0].source,
            NodeSource::Git {
                repository: "https://github.com/o/claimed".into(),
                commit: "c".repeat(40),
                branch: None
            }
        );
        assert!(node.join(MARKER).exists());
    }

    #[test]
    fn unsafe_branch_names_in_head_are_not_followed() {
        let directory = tempfile::tempdir().unwrap();
        let node = directory.path().join("n");
        write(&node.join(".git/HEAD"), "ref: refs/heads/../../../secret\n");
        write(&directory.path().join("secret"), "x");
        assert!(read_git(&pin_absolute(&node).unwrap()).is_none());
    }

    #[test]
    fn multi_component_git_refs_are_read_through_pinned_parents() {
        let directory = tempfile::tempdir().unwrap();
        let node = directory.path().join("node");
        let commit = "c".repeat(40);
        write(&node.join(".git/HEAD"), "ref: refs/heads/feature/topic\n");
        write(&node.join(".git/refs/heads/feature/topic"), &commit);
        let pinned = pin_absolute(&node).unwrap();
        let meta = read_git(&pinned).unwrap();
        assert_eq!(meta.branch.as_deref(), Some("feature/topic"));
        assert_eq!(meta.commit.as_deref(), Some(commit.as_str()));
    }
}
