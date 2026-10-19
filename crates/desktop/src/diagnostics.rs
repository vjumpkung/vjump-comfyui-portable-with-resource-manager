//! The diagnostics report as the user receives it: rendered by the application layer, shown
//! relative to the install root, redacted as a whole, and handed to the system clipboard.

use std::{path::Path, sync::Arc};

use application::settings::diagnostics::DiagnosticsSnapshot;
use infrastructure::shared::logging::redact;
use tauri::{AppHandle, Runtime};
use tauri_plugin_clipboard_manager::ClipboardExt;

use crate::state::AppState;

/// Writes text to the system clipboard. Failure is expected (no clipboard service, a locked
/// clipboard) and only means the UI must offer the text for manual selection.
pub trait Clipboard: Send + Sync {
    fn write_text(&self, text: &str) -> Result<(), ClipboardError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipboardError;

/// The clipboard plugin, used from Rust only; no webview holds its permissions.
pub struct TauriClipboard<R: Runtime>(AppHandle<R>);

impl<R: Runtime> TauriClipboard<R> {
    pub fn new(app: AppHandle<R>) -> Self {
        Self(app)
    }
}

impl<R: Runtime> Clipboard for TauriClipboard<R> {
    fn write_text(&self, text: &str) -> Result<(), ClipboardError> {
        self.0
            .clipboard()
            .write_text(text.to_owned())
            .map_err(|_| ClipboardError)
    }
}

/// Stands in until the application handle exists.
pub struct UnavailableClipboard;

impl Clipboard for UnavailableClipboard {
    fn write_text(&self, _: &str) -> Result<(), ClipboardError> {
        Err(ClipboardError)
    }
}

/// The finished text of the report and whether it was copied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopyOutcome {
    pub text: String,
    pub copied: bool,
}

/// Presents the install root as `<root>`, then redacts the entire rendered report. This is the
/// last step before the text is returned or copied, so nothing a section contains can bypass it.
pub fn finalize(rendered: &str, root: &Path) -> String {
    redact(&replace_root(rendered, root)).into_owned()
}

fn replace_root(text: &str, root: &Path) -> String {
    let root = root.to_string_lossy();
    let root = root.strip_prefix(r"\\?\").unwrap_or(&root);
    let root = root.trim_end_matches(['/', '\\']);
    if root.is_empty() {
        return text.to_owned();
    }
    // The root may be printed with either separator, or in the Windows verbatim form.
    let mut variants = vec![
        root.to_owned(),
        root.replace('\\', "/"),
        root.replace('/', "\\"),
        format!(r"\\?\{}", root.replace('/', "\\")),
    ];
    // Longest first, so the verbatim form is replaced before its shorter plain spelling.
    variants.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
    variants.dedup();
    variants.iter().fold(text.to_owned(), |text, variant| {
        replace_all(&text, variant, "<root>", cfg!(windows))
    })
}

fn replace_all(haystack: &str, needle: &str, with: &str, ignore_ascii_case: bool) -> String {
    if !ignore_ascii_case {
        return haystack.replace(needle, with);
    }
    // ASCII lowercasing keeps byte offsets, and a valid UTF-8 needle only matches at boundaries.
    let lower = haystack.to_ascii_lowercase();
    let needle_lower = needle.to_ascii_lowercase();
    let mut out = String::with_capacity(haystack.len());
    let mut cursor = 0;
    for (start, _) in lower.match_indices(&needle_lower) {
        out.push_str(&haystack[cursor..start]);
        out.push_str(with);
        cursor = start + needle.len();
    }
    out.push_str(&haystack[cursor..]);
    out
}

/// Collects the report and finalizes it. A start without a usable install root still reports
/// what is known, with the recovery reason as its availability warning.
pub(crate) async fn collect_report(state: &AppState) -> String {
    match state.services() {
        Ok(services) => {
            let snapshot = services.diagnostics.execute().await;
            finalize(&snapshot.render(), &services.root.root)
        }
        Err(_) => {
            let mut snapshot = DiagnosticsSnapshot {
                app_version: env!("CARGO_PKG_VERSION").to_owned(),
                os: std::env::consts::OS.to_owned(),
                ..DiagnosticsSnapshot::default()
            };
            if let Some(recovery) = state.recovery() {
                snapshot.warnings.push(format!(
                    "The app started without a usable install root ({}): {}",
                    recovery.code, recovery.message
                ));
            }
            finalize(&snapshot.render(), Path::new(""))
        }
    }
}

/// Puts the report on the clipboard on a blocking thread. The returned text is exactly the text
/// that was offered to the clipboard.
pub(crate) async fn copy_report(clipboard: Arc<dyn Clipboard>, text: String) -> CopyOutcome {
    let payload = text.clone();
    let copied =
        tauri::async_runtime::spawn_blocking(move || clipboard.write_text(&payload).is_ok())
            .await
            .unwrap_or(false);
    CopyOutcome { text, copied }
}

#[cfg(test)]
mod tests;
