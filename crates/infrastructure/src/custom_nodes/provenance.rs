//! Source identity: HTTPS remote validation and normalization, safe folder names, the recovery
//! marker and the Manager-compatible `.tracking` inventory. Metadata found on disk is an
//! observation; none of it authorizes anything.
use super::{
    sqlite::{source_from_json, source_to_json},
    tree::{is_link, read_text},
};
use domain::{custom_nodes::NodeSource, shared::model_values::SafeFileName};
use percent_encoding::percent_decode_str;
use serde_json::{Value, json};
use std::{fs, io, path::Path};
use url::Url;
use walkdir::WalkDir;

pub const MARKER: &str = ".vjump-node.json";
pub const TRACKING: &str = ".tracking";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remote {
    /// The identity used for trust and comparison: `https://host/path` without `.git` or slash.
    pub identity: String,
    /// What Git is given.
    pub clone_url: String,
}

/// Accepts HTTPS URLs without userinfo, query or fragment. `allow_local` (tests only) also
/// accepts a local path that exists.
pub fn validate_remote(input: &str, allow_local: bool) -> Option<Remote> {
    let input = input.trim();
    if allow_local && !input.contains("://") && Path::new(input).exists() {
        return Some(Remote {
            identity: input.to_owned(),
            clone_url: input.to_owned(),
        });
    }
    if input.len() > 500 || input.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return None;
    }
    let url = Url::parse(input).ok()?;
    if url.scheme() != "https"
        || url.host_str().is_none_or(str::is_empty)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.port().is_some_and(|p| p != 443)
    {
        return None;
    }
    let path = url.path().trim_end_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    if path.is_empty() || path == "/" || path.contains("//") {
        return None;
    }
    let identity = format!("https://{}{path}", url.host_str()?.to_ascii_lowercase());
    Some(Remote {
        clone_url: identity.clone(),
        identity,
    })
}

/// Normalizes a remote read from disk for comparison. Non-HTTPS remotes keep their spelling.
pub fn observed_identity(raw: &str) -> String {
    validate_remote(raw, false).map_or_else(|| raw.trim().to_owned(), |r| r.identity)
}

/// The folder name for a repository: its decoded last path segment, validated.
pub fn repository_folder(identity: &str) -> Option<SafeFileName> {
    let last = identity
        .trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()?;
    let decoded = percent_decode_str(last).decode_utf8().ok()?;
    let name = decoded.strip_suffix(".git").unwrap_or(&decoded);
    safe_folder(name)
}

/// A node folder name the app may create: valid file name, no leading dot, no spaces at the
/// edges. A leading dot would collide with `.disabled` and the app's `.vjump-*` names.
pub fn safe_folder(name: &str) -> Option<SafeFileName> {
    if name.starts_with('.') || name.trim() != name || name == "__pycache__" {
        return None;
    }
    SafeFileName::new(name).ok()
}

pub fn write_marker(
    node_dir: &Path,
    name: &str,
    source: &NodeSource,
    artifact_digest: Option<&str>,
) -> io::Result<()> {
    let document = json!({
        "schema": 1,
        "name": name,
        "source": source_to_json(source),
        "artifact_digest": artifact_digest,
    });
    fs::write(node_dir.join(MARKER), serde_json::to_vec_pretty(&document)?)
}

/// Marker schema 1, or `None` for anything else (including a marker that is a link).
pub fn read_marker(node_dir: &Path) -> Option<NodeSource> {
    let value: Value = serde_json::from_str(&read_text(&node_dir.join(MARKER), 64 * 1024)?).ok()?;
    if value.get("schema")?.as_u64()? != 1 {
        return None;
    }
    source_from_json(value.get("source")?)
}

/// The Manager `.tracking` file: every installed file as a relative path with forward slashes.
pub fn write_tracking(node_dir: &Path) -> io::Result<()> {
    let mut lines = Vec::new();
    for entry in WalkDir::new(node_dir)
        .follow_links(false)
        .sort_by_file_name()
    {
        let entry = entry.map_err(io::Error::other)?;
        if entry.depth() == 0 || !entry.file_type().is_file() {
            continue;
        }
        let meta = entry.path().symlink_metadata()?;
        if is_link(&meta) {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(node_dir)
            .map_err(io::Error::other)?
            .to_string_lossy()
            .replace('\\', "/");
        if relative == MARKER || relative == TRACKING {
            continue;
        }
        lines.push(relative);
    }
    fs::write(node_dir.join(TRACKING), lines.join("\n") + "\n")
}

/// Manager registry provenance: `.tracking` together with `pyproject.toml`'s name and version.
pub fn read_manager_registry(node_dir: &Path) -> Option<NodeSource> {
    read_text(&node_dir.join(TRACKING), 4 << 20)?;
    let pyproject = read_text(&node_dir.join("pyproject.toml"), 256 * 1024)?;
    let document: toml::Value = toml::from_str(&pyproject).ok()?;
    let project = document.get("project")?;
    let id = project.get("name")?.as_str()?.trim().to_owned();
    let version = project.get("version")?.as_str()?.trim().to_owned();
    if id.is_empty() || version.is_empty() {
        return None;
    }
    let repository = project
        .get("urls")
        .and_then(|u| u.get("Repository").or_else(|| u.get("repository")))
        .and_then(|v| v.as_str())
        .map(str::to_owned);
    let publisher = document
        .get("tool")
        .and_then(|t| t.get("comfy"))
        .and_then(|c| c.get("PublisherId"))
        .and_then(|v| v.as_str())
        .map(str::to_owned);
    Some(NodeSource::Registry {
        id,
        publisher,
        version,
        repository,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remotes_are_https_only_and_normalized() {
        let a = validate_remote("https://GitHub.com/Owner/Repo.git/", false).unwrap();
        assert_eq!(a.identity, "https://github.com/Owner/Repo");
        assert_eq!(
            a,
            validate_remote("https://github.com/Owner/Repo", false).unwrap()
        );
        for bad in [
            "http://github.com/o/r",
            "git@github.com:o/r.git",
            "ssh://github.com/o/r",
            "file:///etc",
            "https://user:pw@github.com/o/r",
            "https://user@github.com/o/r",
            "https://github.com/o/r?x=1",
            "https://github.com/o/r#frag",
            "https://github.com:8443/o/r",
            "https://github.com/",
            "https://github.com/o/r with space",
            "-c core.sshCommand=evil",
        ] {
            assert!(validate_remote(bad, false).is_none(), "{bad}");
        }
    }

    #[test]
    fn folder_names_come_from_the_decoded_repository_name() {
        assert_eq!(
            repository_folder("https://h/o/ComfyUI-Foo")
                .unwrap()
                .as_str(),
            "ComfyUI-Foo"
        );
        assert_eq!(
            repository_folder("https://h/o/My%20Nodes.git")
                .unwrap()
                .as_str(),
            "My Nodes"
        );
        for bad in [
            "https://h/o/..%2f..%2fevil",
            "https://h/o/%2e%2e",
            "https://h/o/.hidden",
            "https://h/o/CON",
            "https://h/o/a%3Ab",
            "https://h/o/x%5Cy",
        ] {
            assert!(repository_folder(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn marker_round_trips_and_rejects_other_schemas() {
        let directory = tempfile::tempdir().unwrap();
        let source = NodeSource::Registry {
            id: "n".into(),
            publisher: Some("p".into()),
            version: "1.0.0".into(),
            repository: None,
        };
        write_marker(directory.path(), "n", &source, Some("abc")).unwrap();
        assert_eq!(read_marker(directory.path()), Some(source));
        fs::write(
            directory.path().join(MARKER),
            r#"{"schema":2,"source":{"kind":"manual"}}"#,
        )
        .unwrap();
        assert_eq!(read_marker(directory.path()), None);
    }

    #[test]
    fn manager_registry_needs_tracking_and_pyproject_together() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(
            directory.path().join("pyproject.toml"),
            "[project]\nname = \"foo-nodes\"\nversion = \"1.2.3\"\n[project.urls]\nRepository = \"https://github.com/o/foo\"\n[tool.comfy]\nPublisherId = \"o\"\n",
        )
        .unwrap();
        assert_eq!(read_manager_registry(directory.path()), None);
        fs::write(directory.path().join(TRACKING), "a.py\n").unwrap();
        assert_eq!(
            read_manager_registry(directory.path()),
            Some(NodeSource::Registry {
                id: "foo-nodes".into(),
                publisher: Some("o".into()),
                version: "1.2.3".into(),
                repository: Some("https://github.com/o/foo".into()),
            })
        );
        fs::write(directory.path().join("pyproject.toml"), "not = [toml").unwrap();
        assert_eq!(read_manager_registry(directory.path()), None);
    }

    #[test]
    fn tracking_lists_files_but_not_app_metadata() {
        let directory = tempfile::tempdir().unwrap();
        fs::create_dir_all(directory.path().join("sub")).unwrap();
        fs::write(directory.path().join("a.py"), "x").unwrap();
        fs::write(directory.path().join("sub/b.py"), "y").unwrap();
        fs::write(directory.path().join(MARKER), "{}").unwrap();
        write_tracking(directory.path()).unwrap();
        assert_eq!(
            fs::read_to_string(directory.path().join(TRACKING)).unwrap(),
            "a.py\nsub/b.py\n"
        );
    }
}
