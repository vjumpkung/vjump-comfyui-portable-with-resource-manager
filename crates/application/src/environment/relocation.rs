use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RebuildReason {
    Moved,
    InterpreterBroken,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallationHealth {
    Healthy,
    RebuildRequired(RebuildReason),
}

/// Pure component normalization; filesystem adapters canonicalize existing locations first.
pub fn normalized_path(path: &Path) -> PathBuf {
    let normalized = component_path(path);
    if cfg!(windows) {
        PathBuf::from(normalized.to_string_lossy().to_lowercase())
    } else {
        normalized
    }
}

fn component_path(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    let text = if cfg!(windows)
        && text
            .get(..8)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(r"\\?\UNC\"))
    {
        format!(r"\\{}", &text[8..])
    } else {
        text.strip_prefix(r"\\?\").unwrap_or(&text).to_owned()
    };
    let mut normalized = PathBuf::new();
    for component in Path::new(&text).components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

/// Rebase only a whole component prefix, retaining paths outside the recorded root.
pub fn rebase_path(path: &Path, old: &Path, new: &Path) -> Option<PathBuf> {
    let normalized = normalized_path(path);
    let old = normalized_path(old);
    if !normalized.is_absolute() || !old.is_absolute() {
        return None;
    }
    normalized.strip_prefix(&old).ok()?;
    let tail: PathBuf = component_path(path)
        .components()
        .skip(old.components().count())
        .collect();
    Some(new.join(tail))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rebasing_uses_whole_components_and_preserves_tail_spelling() {
        let root = std::env::temp_dir().join("Original");
        let new = std::env::temp_dir().join("Moved");
        assert_eq!(
            rebase_path(&root.join("envs/CaseSensitiveName"), &root, &new),
            Some(new.join("envs/CaseSensitiveName"))
        );
        assert_eq!(
            rebase_path(
                &root.with_file_name("Original-sibling").join("env"),
                &root,
                &new
            ),
            None
        );
        assert_eq!(
            rebase_path(&root.join("../external/env"), &root, &new),
            None
        );
    }

    #[cfg(windows)]
    #[test]
    fn verbatim_unc_paths_keep_absolute_component_identity() {
        let old = Path::new(r"\\server\share\Old");
        let new = Path::new(r"\\server\share\New");
        assert!(same_location_for_test(
            Path::new(r"\\?\UNC\server\share\OLD\."),
            old
        ));
        assert_eq!(
            rebase_path(
                Path::new(r"\\?\UNC\server\share\Old\envs\CaseName"),
                old,
                new
            ),
            Some(new.join(r"envs\CaseName"))
        );
    }

    #[cfg(windows)]
    fn same_location_for_test(left: &Path, right: &Path) -> bool {
        normalized_path(left) == normalized_path(right)
    }
}
