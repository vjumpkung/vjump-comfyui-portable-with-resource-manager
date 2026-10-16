//! ACL enforcement tests against the real compiled manifest and capability.
//!
//! These are integration tests because the build script embeds the Windows Common Controls v6
//! manifest into test executables only for integration test targets.

mod support;

use desktop::{MANAGER_WINDOW, comfy_window::COMFYUI_WINDOW};
use serde_json::{Value, json};
use support::*;
use tauri::WebviewWindow;
use tauri::test::{MockRuntime, get_ipc_response};

/// What a command is expected to do when the ACL lets it through.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Expect {
    Success,
    /// The handler runs and answers with a sanitized `IpcError` (the test state has no
    /// environment, run or plan to act on).
    HandlerError,
}

/// Every command with a valid body.
fn commands(app: &TestApp) -> Vec<(&'static str, Value, Expect)> {
    use Expect::*;
    let new_root = app.root.parent().unwrap().join("chosen root");
    vec![
        ("app_info", json!({}), Success),
        ("settings_get", json!({}), Success),
        (
            "settings_set",
            json!({
                "input": {
                    "downloadConcurrency": 4,
                    "hfEngine": "native",
                    "closeBehavior": "ask",
                    "theme": "dark",
                    "registryCacheTtl": 6,
                    "bandwidthCapKibS": null
                }
            }),
            Success,
        ),
        (
            "secret_set",
            json!({ "input": { "kind": "civitAi", "value": "acl-test-placeholder-value" } }),
            Success,
        ),
        ("secret_clear", json!({ "kind": "civitAi" }), Success),
        ("secret_status", json!({ "kind": "huggingFace" }), Success),
        ("setup_status", json!({}), Success),
        ("setup_detect_hardware", json!({}), Success),
        ("setup_list_profiles", json!({}), Success),
        (
            "setup_validate_python",
            json!({ "source": { "kind": "managed" } }),
            Success,
        ),
        (
            "setup_select_root",
            json!({ "path": new_root.display().to_string(), "draft": "{}" }),
            Success,
        ),
        (
            "setup_run",
            json!({
                "action": {
                    "kind": "new",
                    "request": {
                        "mode": "first_run",
                        "pythonSource": { "kind": "managed" },
                        "profileId": host_profile(),
                        "comfyuiRef": { "kind": "latest" },
                        "installManager": false,
                        "cleanCache": false,
                        "confirmAdoptedChanges": false,
                        "allowSourceBuilds": false
                    }
                },
                "onEvent": "__CHANNEL__:1"
            }),
            Success,
        ),
        (
            "setup_cancel",
            json!({ "runId": "run-unknown" }),
            HandlerError,
        ),
        ("env_info", json!({}), HandlerError),
        ("env_packages", json!({}), HandlerError),
        (
            "env_pip_preview",
            json!({ "operation": { "kind": "install", "specs": ["requests"] } }),
            HandlerError,
        ),
        (
            "env_pip_apply",
            json!({ "input": { "planId": "plan-unknown", "confirmedChanges": [] } }),
            HandlerError,
        ),
        (
            "env_delete",
            json!({ "envId": "env-unknown" }),
            HandlerError,
        ),
        ("comfy_status", json!({}), HandlerError),
        ("comfy_start", json!({}), HandlerError),
        ("comfy_stop", json!({}), HandlerError),
        ("comfy_restart", json!({}), HandlerError),
        (
            "comfy_subscribe",
            json!({ "afterCursor": null, "onEvent": "__CHANNEL__:2" }),
            HandlerError,
        ),
        (
            "comfy_unsubscribe",
            json!({ "subscriptionId": 4_000_000 }),
            HandlerError,
        ),
        ("comfy_get_launch_options", json!({}), HandlerError),
        (
            "comfy_set_launch_options",
            json!({
                "options": {
                    "port": 8188,
                    "vram": "auto",
                    "attention": "default",
                    "preview": "auto",
                    "enableManager": false,
                    "listenLan": false,
                    "extraArgs": []
                }
            }),
            HandlerError,
        ),
        ("comfy_list_versions", json!({}), HandlerError),
        (
            "comfy_update",
            json!({
                "input": {
                    "target": { "kind": "latest_release" },
                    "stopRunning": false,
                    "retry": null
                }
            }),
            HandlerError,
        ),
        ("comfy_open_window", json!({}), HandlerError),
        ("comfy_open_browser", json!({}), HandlerError),
        ("comfy_diagnostics", json!({}), HandlerError),
        ("comfy_read_extra_model_paths", json!({}), HandlerError),
        (
            "app_close_decide",
            json!({ "requestId": "close-unknown", "choice": "cancel" }),
            HandlerError,
        ),
        ("secret_test", json!({ "kind": "civitAi" }), HandlerError),
        (
            "dl_resolve",
            json!({ "input": { "url": "not a url", "httpConfirmed": false } }),
            HandlerError,
        ),
        (
            "dl_enqueue",
            json!({
                "items": [{
                    "resolutionId": 1,
                    "fileIndex": 0,
                    "category": "checkpoints",
                    "subdirectory": "",
                    "filename": null,
                    "conflict": null,
                    "force": false
                }]
            }),
            HandlerError,
        ),
        ("dl_pause", json!({ "jobId": "job-unknown" }), HandlerError),
        (
            "dl_resume",
            json!({ "jobId": "job-unknown", "decision": null }),
            HandlerError,
        ),
        ("dl_cancel", json!({ "jobId": "job-unknown" }), HandlerError),
        ("dl_retry", json!({ "jobId": "job-unknown" }), HandlerError),
        (
            "dl_list",
            json!({ "before": null, "limit": 50 }),
            HandlerError,
        ),
        (
            "dl_subscribe",
            json!({ "afterCursor": null, "onEvent": "__CHANNEL__:3" }),
            HandlerError,
        ),
        (
            "dl_unsubscribe",
            json!({ "subscriptionId": 4_000_000 }),
            HandlerError,
        ),
        (
            "dl_import_pack",
            json!({ "input": { "source": "json", "value": "not json" } }),
            HandlerError,
        ),
        (
            "dl_export",
            json!({ "input": { "jobIds": null } }),
            HandlerError,
        ),
        ("model_list", json!({}), HandlerError),
        ("model_rescan", json!({}), HandlerError),
        (
            "model_details",
            json!({ "location": { "rootId": "models", "path": "checkpoints/none.safetensors" } }),
            HandlerError,
        ),
        (
            "model_rename",
            json!({
                "input": {
                    "location": { "rootId": "models", "path": "checkpoints/none.safetensors" },
                    "newName": "other.safetensors",
                    "allowExtensionChange": false
                }
            }),
            HandlerError,
        ),
        (
            "model_move",
            json!({
                "input": {
                    "location": { "rootId": "models", "path": "checkpoints/none.safetensors" },
                    "targetRootId": "models",
                    "targetDirectory": "loras"
                }
            }),
            HandlerError,
        ),
        (
            "model_delete_plan",
            json!({ "locations": [{ "rootId": "models", "path": "checkpoints/none.safetensors" }] }),
            HandlerError,
        ),
        (
            "model_delete",
            json!({ "planId": "plan-unknown" }),
            HandlerError,
        ),
        (
            "model_delete_permanently",
            json!({ "planId": "plan-unknown" }),
            HandlerError,
        ),
        (
            "model_identify",
            json!({
                "input": {
                    "location": { "rootId": "models", "path": "checkpoints/none.safetensors" },
                    "replaceExisting": false
                }
            }),
            HandlerError,
        ),
        ("model_operation_status", json!({}), HandlerError),
        (
            "model_subscribe",
            json!({ "onEvent": "__CHANNEL__:4" }),
            HandlerError,
        ),
        (
            "model_unsubscribe",
            json!({ "subscriptionId": 4_000_000 }),
            HandlerError,
        ),
        (
            "node_registry_search",
            json!({ "input": { "search": "", "page": 1, "limit": 50, "refresh": false } }),
            HandlerError,
        ),
        ("node_list", json!({}), HandlerError),
        ("node_rescan", json!({}), HandlerError),
        (
            "node_prepare_registry",
            json!({ "input": { "requestId": "req-1", "id": "some-node", "version": null } }),
            HandlerError,
        ),
        (
            "node_prepare_git",
            json!({
                "input": {
                    "requestId": "req-2",
                    "repository": "https://github.com/example/node",
                    "reference": null
                }
            }),
            HandlerError,
        ),
        (
            "node_prepare_update",
            json!({ "input": { "requestId": "req-3", "nodeId": "node-unknown", "reference": null } }),
            HandlerError,
        ),
        (
            "node_prepare_retry_deps",
            json!({ "input": { "requestId": "req-4", "nodeId": "node-unknown" } }),
            HandlerError,
        ),
        (
            "node_commit",
            json!({
                "input": {
                    "planId": "plan-unknown",
                    "binding": "binding",
                    "executeNodeCode": true,
                    "runInstallPy": false,
                    "acknowledgeWarnings": false,
                    "rememberSource": false,
                    "rememberInstallPy": false,
                    "confirmedChanges": []
                }
            }),
            HandlerError,
        ),
        (
            "node_set_enabled",
            json!({ "input": { "nodeId": "node-unknown", "enabled": false } }),
            HandlerError,
        ),
        (
            "node_uninstall",
            json!({ "input": { "nodeId": "node-unknown" } }),
            HandlerError,
        ),
        (
            "node_update_all",
            json!({
                "input": {
                    "items": [{
                        "planId": "plan-unknown",
                        "binding": "binding",
                        "executeNodeCode": true,
                        "runInstallPy": false,
                        "acknowledgeWarnings": false,
                        "rememberSource": false,
                        "rememberInstallPy": false,
                        "confirmedChanges": []
                    }]
                }
            }),
            HandlerError,
        ),
        ("node_cancel", json!({ "key": "plan-unknown" }), HandlerError),
        (
            "node_operation_status",
            json!({ "operationId": "node-unknown" }),
            HandlerError,
        ),
        (
            "node_subscribe",
            json!({ "onEvent": "__CHANNEL__:5" }),
            HandlerError,
        ),
        (
            "node_unsubscribe",
            json!({ "subscriptionId": 4_000_000 }),
            HandlerError,
        ),
        ("pick_folder", json!({ "title": null }), Success),
        (
            "pick_file",
            json!({ "title": null, "extensions": ["exe"] }),
            Success,
        ),
    ]
}

/// A handler answer is either success or the sanitized `IpcError` object. An ACL denial is a
/// plain string and never reaches the handler.
fn reached_handler(response: &Result<tauri::ipc::InvokeResponseBody, Value>) -> bool {
    match response {
        Ok(_) => true,
        Err(Value::Object(error)) => error.contains_key("code") && error.contains_key("message"),
        Err(_) => false,
    }
}

fn assert_denied(app: &TestApp, window: &WebviewWindow<MockRuntime>, url: tauri::Url, case: &str) {
    for (cmd, body, _) in commands(app) {
        let before = app.calls.count();
        let response = get_ipc_response(window, request(cmd, url.clone(), body));
        assert!(response.is_err(), "{case}: `{cmd}` must be denied");
        assert!(
            !reached_handler(&response),
            "{case}: `{cmd}` must be denied by the ACL, not by the handler: {response:?}"
        );
        assert_eq!(
            app.calls.count(),
            before,
            "{case}: denied `{cmd}` must not reach its handler"
        );
    }
}

#[test]
fn every_command_is_covered_by_the_acl_test() {
    // The bindings builder, the build script's app manifest and this test must list the same
    // commands, so a command added without a permission or an ACL test fails here.
    let app = build_app();
    let tested: Vec<&str> = commands(&app).iter().map(|(cmd, _, _)| *cmd).collect();
    let manifest =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/build.rs")).unwrap();
    let capability = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/capabilities/manager.json"
    ))
    .unwrap();
    for cmd in &tested {
        assert!(
            manifest.contains(&format!("\"{cmd}\"")),
            "{cmd} is missing in build.rs"
        );
        let permission = format!("\"allow-{}\"", cmd.replace('_', "-"));
        assert!(
            capability.contains(&permission),
            "{permission} is missing in manager.json"
        );
    }
    // The generated bindings list every registered command exactly once.
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("bindings.ts");
    desktop::ipc::bindings::builder::<MockRuntime>()
        .export(specta_typescript::Typescript::default(), &path)
        .unwrap();
    let generated = std::fs::read_to_string(path).unwrap();
    for cmd in &tested {
        assert_eq!(
            generated
                .matches(&format!("TAURI_INVOKE(\"{cmd}\""))
                .count(),
            1,
            "{cmd} must be registered with the bindings builder"
        );
    }
    assert_eq!(
        generated.matches("TAURI_INVOKE(").count(),
        tested.len(),
        "a registered command is not covered by the ACL test"
    );
}

#[test]
fn manager_window_with_local_origin_can_invoke_all_commands() {
    let app = build_app();
    let window = manager_window(&app);
    for (cmd, body, expect) in commands(&app) {
        let before = app.calls.count();
        let response = get_ipc_response(&window, request(cmd, local_url(), body));
        match expect {
            Expect::Success => assert!(
                response.is_ok(),
                "manager/local: `{cmd}` must be allowed and succeed: {response:?}"
            ),
            Expect::HandlerError => assert!(
                reached_handler(&response),
                "manager/local: `{cmd}` must reach its handler: {response:?}"
            ),
        }
        assert!(
            app.calls.count() > before || matches!(expect, Expect::HandlerError),
            "manager/local: `{cmd}` must reach an application port"
        );
    }
}

#[test]
fn comfyui_window_with_local_origin_is_denied() {
    let app = build_app();
    let _manager = manager_window(&app);
    let comfyui = comfyui_window(&app);
    assert_eq!(comfyui.label(), COMFYUI_WINDOW);
    assert_denied(&app, &comfyui, local_url(), "comfyui/local");
}

#[test]
fn comfyui_window_with_comfyui_origin_is_denied() {
    let app = build_app();
    let _manager = manager_window(&app);
    let comfyui = comfyui_window(&app);
    assert_denied(
        &app,
        &comfyui,
        "http://127.0.0.1:8188".parse().unwrap(),
        "comfyui/http://127.0.0.1:8188",
    );
}

#[test]
fn manager_window_with_remote_origin_is_denied() {
    let app = build_app();
    let manager = manager_window(&app);
    assert_denied(
        &app,
        &manager,
        "https://example.com".parse().unwrap(),
        "manager/remote",
    );
}

#[test]
fn the_dialog_plugin_commands_are_not_granted_to_any_window() {
    // The pickers are reached through the app's own commands; the plugin's commands carry
    // no permission in any capability.
    let capability = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/capabilities/manager.json"
    ))
    .unwrap();
    assert!(!capability.contains("dialog:"));
    let _ = MANAGER_WINDOW;
}
