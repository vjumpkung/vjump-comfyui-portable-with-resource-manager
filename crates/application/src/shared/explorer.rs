use std::path::PathBuf;

/// Where the system file manager should open. Never serialized; desktop hands it to the shell.
/// Adapters build the paths from pinned directories, never from caller strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExplorerTarget {
    /// Show the contents of this folder.
    OpenFolder(PathBuf),
    /// Show the parent folder with this item selected.
    RevealItem(PathBuf),
}
