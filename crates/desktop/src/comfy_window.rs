use std::sync::{Arc, RwLock};

use tauri::{Manager, Runtime, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tauri_plugin_opener::OpenerExt;

/// The label of the top-level window that hosts the ComfyUI web UI. No capability
/// names this label, so it can never invoke app commands.
pub const COMFYUI_WINDOW: &str = "comfyui";

struct ComfyOrigin(Arc<RwLock<Url>>);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Navigation {
    Embedded,
    Browser,
    Denied,
}

pub fn browser_url_allowed(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https")
        && url.username().is_empty()
        && url.password().is_none()
        && url.host_str().is_some()
}

pub fn navigation(origin: &Url, target: &Url) -> Navigation {
    if !browser_url_allowed(target) {
        Navigation::Denied
    } else if target.origin() == origin.origin() {
        Navigation::Embedded
    } else {
        Navigation::Browser
    }
}

pub fn open_in_browser<R: Runtime>(app: &tauri::AppHandle<R>, url: Url) {
    if browser_url_allowed(&url) {
        let app = app.clone();
        tauri::async_runtime::spawn_blocking(move || {
            if app.opener().open_url(url.as_str(), None::<&str>).is_err() {
                tracing::warn!("the system browser could not be opened");
            }
        });
    }
}

/// Opens ComfyUI in its own top-level window with no capability attached.
pub fn open_comfyui_window<R: Runtime, M: Manager<R>>(
    app: &M,
    url: Url,
) -> tauri::Result<WebviewWindow<R>> {
    if !browser_url_allowed(&url) || url.scheme() != "http" || url.host_str() != Some("127.0.0.1") {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "invalid ComfyUI origin",
        )
        .into());
    }
    let origin = if let Some(state) = app.try_state::<ComfyOrigin>() {
        let shared = state.0.clone();
        *shared
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = url.clone();
        shared
    } else {
        let shared = Arc::new(RwLock::new(url.clone()));
        app.manage(ComfyOrigin(shared.clone()));
        shared
    };
    if let Some(window) = app.get_webview_window(COMFYUI_WINDOW) {
        window.navigate(url)?;
        window.show()?;
        window.set_focus()?;
        return Ok(window);
    }
    let navigation_app = app.app_handle().clone();
    let new_window_app = app.app_handle().clone();
    let state = app.state::<crate::AppState>();
    let data_directory = state.data_dir.join("webview/comfyui");
    WebviewWindowBuilder::new(app, COMFYUI_WINDOW, WebviewUrl::External(url))
        .title("ComfyUI")
        .data_directory(data_directory)
        .on_navigation(move |target| {
            let origin = origin
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            match navigation(&origin, target) {
                Navigation::Embedded => true,
                Navigation::Browser => {
                    open_in_browser(&navigation_app, target.clone());
                    false
                }
                Navigation::Denied => false,
            }
        })
        .on_new_window(move |target, _| {
            open_in_browser(&new_window_app, target);
            tauri::webview::NewWindowResponse::Deny
        })
        .build()
}
