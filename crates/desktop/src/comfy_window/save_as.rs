//! Asks where to save each download the ComfyUI page starts (for example Export workflow).
//! Wry's own DownloadStarting handler keeps the WebView2 default path, so this second handler
//! takes a deferral, shows a Save As dialog and then sets the chosen path or cancels;
//! closing the window cancels any download still waiting.

use std::{
    cell::RefCell,
    collections::HashMap,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

use tauri::{Manager, Runtime, WebviewWindow, webview::PlatformWebview};
use tauri_plugin_dialog::DialogExt;
use webview2_com::{
    DownloadStartingEventHandler,
    Microsoft::Web::WebView2::Win32::{
        ICoreWebView2_4, ICoreWebView2Deferral, ICoreWebView2DownloadStartingEventArgs,
    },
    take_pwstr,
};
use windows_core::{HSTRING, Interface, PWSTR};

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

thread_local! {
    // Downloads waiting for the dialog, keyed by id. The COM objects stay on the UI thread;
    // only the id travels through the dialog worker thread.
    static PENDING: RefCell<HashMap<u64, (ICoreWebView2DownloadStartingEventArgs, ICoreWebView2Deferral)>> = RefCell::default();
}

pub(super) fn install<R: Runtime>(window: &WebviewWindow<R>) {
    let target = window.clone();
    let scheduled = window.with_webview(move |webview| {
        if let Err(error) = register(target, webview) {
            tracing::warn!(%error, "the ComfyUI download prompt could not be installed");
        }
    });
    if scheduled.is_err() {
        tracing::warn!("the ComfyUI download prompt could not be installed");
    }
    window.on_window_event(|event| {
        if matches!(event, tauri::WindowEvent::Destroyed) {
            cancel_pending();
        }
    });
}

fn register<R: Runtime>(
    window: WebviewWindow<R>,
    webview: PlatformWebview,
) -> windows_core::Result<()> {
    let core = unsafe { webview.controller().CoreWebView2()? }.cast::<ICoreWebView2_4>()?;
    let handler = DownloadStartingEventHandler::create(Box::new(move |_, args| {
        if let Some(args) = args {
            prompt(&window, args)?;
        }
        Ok(())
    }));
    let mut token = 0;
    unsafe { core.add_DownloadStarting(&handler, &mut token) }
}

// Runs inside the WebView2 event on the UI thread: never block here. save_file only queues
// the dialog on its own thread.
fn prompt<R: Runtime>(
    window: &WebviewWindow<R>,
    args: ICoreWebView2DownloadStartingEventArgs,
) -> windows_core::Result<()> {
    let deferral = unsafe { args.GetDeferral()? };
    let mut suggested = PWSTR::null();
    let suggested = unsafe { args.ResultFilePath(&mut suggested) }
        .map(|()| PathBuf::from(take_pwstr(suggested)))
        .ok();
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    PENDING.with_borrow_mut(|pending| pending.insert(id, (args, deferral)));
    let mut dialog = window.dialog().file().set_parent(window);
    if let Some(suggested) = &suggested {
        if let Some(name) = suggested.file_name() {
            dialog = dialog.set_file_name(name.to_string_lossy());
        }
        if let Some(folder) = suggested.parent() {
            dialog = dialog.set_directory(folder);
        }
    }
    let app = window.app_handle().clone();
    dialog.save_file(move |choice| {
        let path = choice
            .and_then(|path| path.into_path().ok())
            .filter(|path| path.is_absolute());
        if app.run_on_main_thread(move || finish(id, path)).is_err() {
            tracing::warn!("a ComfyUI download could not be completed");
        }
    });
    Ok(())
}

// UI thread: the window is gone, so any download still waiting for its dialog is cancelled.
fn cancel_pending() {
    for (args, deferral) in PENDING.take().into_values() {
        let _ = unsafe { args.SetCancel(true) };
        let _ = unsafe { deferral.Complete() };
    }
}

// UI thread: sets the chosen path, or cancels, and completes the deferral exactly once.
fn finish(id: u64, path: Option<PathBuf>) {
    let Some((args, deferral)) = PENDING.with_borrow_mut(|pending| pending.remove(&id)) else {
        return;
    };
    let saved = path.is_some_and(|path| {
        unsafe { args.SetResultFilePath(&HSTRING::from(path.as_path())) }.is_ok()
    });
    if !saved {
        let _ = unsafe { args.SetCancel(true) };
    }
    if let Err(error) = unsafe { deferral.Complete() } {
        tracing::warn!(%error, "a ComfyUI download could not be completed");
    }
}
