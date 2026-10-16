//! Native file and folder pickers behind a small trait, so the commands stay testable and
//! the dialog plugin never needs a webview-facing permission (the manager only calls
//! the app's own `pick_folder` and `pick_file` commands).

use std::path::PathBuf;

use async_trait::async_trait;
use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_dialog::DialogExt;

use crate::MANAGER_WINDOW;

#[async_trait]
pub trait FilePicker: Send + Sync {
    async fn pick_folder(&self, title: Option<String>) -> Option<PathBuf>;
    /// `extensions` are file extensions without the dot; an empty list shows every file.
    async fn pick_file(&self, title: Option<String>, extensions: Vec<String>) -> Option<PathBuf>;
}

/// The production picker, backed by `tauri-plugin-dialog`.
pub struct DialogPicker<R: Runtime> {
    app: AppHandle<R>,
}

impl<R: Runtime> DialogPicker<R> {
    pub fn new(app: AppHandle<R>) -> Self {
        Self { app }
    }

    fn builder(&self, title: Option<String>) -> tauri_plugin_dialog::FileDialogBuilder<R> {
        let mut builder = self.app.dialog().file();
        if let Some(title) = title {
            builder = builder.set_title(title);
        }
        if let Some(window) = self.app.get_webview_window(MANAGER_WINDOW) {
            builder = builder.set_parent(&window);
        }
        builder
    }
}

#[async_trait]
impl<R: Runtime> FilePicker for DialogPicker<R> {
    async fn pick_folder(&self, title: Option<String>) -> Option<PathBuf> {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        self.builder(title).pick_folder(move |path| {
            let _ = sender.send(path);
        });
        receiver
            .await
            .ok()
            .flatten()
            .and_then(|path| path.into_path().ok())
    }

    async fn pick_file(&self, title: Option<String>, extensions: Vec<String>) -> Option<PathBuf> {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let mut builder = self.builder(title);
        if !extensions.is_empty() {
            let extensions: Vec<&str> = extensions.iter().map(String::as_str).collect();
            builder = builder.add_filter("Files", &extensions);
        }
        builder.pick_file(move |path| {
            let _ = sender.send(path);
        });
        receiver
            .await
            .ok()
            .flatten()
            .and_then(|path| path.into_path().ok())
    }
}
