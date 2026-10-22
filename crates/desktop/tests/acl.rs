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
                    "language": "en",
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
        ("setup_list_curated_nodes", json!({}), Success),
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
                        "accelerators": [],
                        "curatedNodes": [],
                        "cleanCache": false,
                        "confirmAdoptedChanges": false,
                        "allowSourceBuilds": false,
                        "thirdPartyConsent": false
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
        ("comfy_read_extra_model_paths", json!({}), HandlerError),
        // A relative path is refused by validation before anything is written.
        (
            "comfy_generate_extra_model_paths",
            json!({ "basePath": "relative/models" }),
            HandlerError,
        ),
        ("comfy_remove_extra_model_paths", json!({}), HandlerError),
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
            "model_reveal",
            json!({ "location": { "rootId": "models", "path": "checkpoints/none.safetensors" } }),
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
            "node_open_repository",
            json!({ "repository": "https://github.com/example/node" }),
            Success,
        ),
        ("node_reveal", json!({ "id": null }), HandlerError),
        (
            "node_registry_search",
            json!({ "input": { "search": "", "page": 1, "limit": 50, "refresh": false } }),
            HandlerError,
        ),
        ("node_list", json!({}), HandlerError),
        (
            "node_git_status",
            json!({ "input": { "nodeId": "node-unknown" } }),
            HandlerError,
        ),
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
        (
            "node_cancel",
            json!({ "key": "plan-unknown" }),
            HandlerError,
        ),
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
        // The gallery adapters are real, so these never count as a fake port call; the reveal
        // commands may reach the fake shell or answer with a sanitized error.
        (
            "gallery_list",
            json!({ "directory": { "area": "output", "path": "" } }),
            HandlerError,
        ),
        (
            "gallery_rename",
            json!({
                "input": {
                    "entry": {
                        "location": { "area": "output", "path": "none.png" },
                        "revision": "revision-unknown"
                    },
                    "newName": "other.png"
                }
            }),
            HandlerError,
        ),
        (
            "gallery_move",
            json!({
                "input": {
                    "items": [{
                        "location": { "area": "output", "path": "none.png" },
                        "revision": "revision-unknown"
                    }],
                    "destination": { "area": "output", "path": "folder" }
                }
            }),
            HandlerError,
        ),
        (
            "gallery_copy",
            json!({
                "input": {
                    "items": [{
                        "location": { "area": "output", "path": "none.png" },
                        "revision": "revision-unknown"
                    }],
                    "destination": { "area": "output", "path": "folder" }
                }
            }),
            HandlerError,
        ),
        (
            "gallery_delete_plan",
            json!({
                "items": [{
                    "location": { "area": "output", "path": "none.png" },
                    "revision": "revision-unknown"
                }]
            }),
            HandlerError,
        ),
        (
            "gallery_delete",
            json!({ "planId": "plan-unknown" }),
            HandlerError,
        ),
        (
            "gallery_reveal",
            json!({ "location": { "area": "output", "path": "none.png" } }),
            HandlerError,
        ),
        // The test app has no updater configuration: the handlers answer with
        // `updater_not_configured` without any network access.
        ("app_update_status", json!({}), HandlerError),
        ("app_update_check", json!({}), HandlerError),
        (
            "app_update_install",
            json!({ "updateId": "update-unknown", "onEvent": "__CHANNEL__:6" }),
            HandlerError,
        ),
        ("app_copy_diagnostics", json!({}), Success),
        ("pick_folder", json!({ "title": null }), Success),
        (
            "pick_file",
            json!({ "title": null, "extensions": ["exe"] }),
            Success,
        ),
        // These quiesce the workers before requesting a restart, so exercise them last.
        (
            "setup_open_install",
            json!({ "path": app.root.display().to_string() }),
            Success,
        ),
        (
            "setup_new_install",
            json!({ "path": new_root.display().to_string() }),
            Success,
        ),
        ("setup_restart", json!({}), Success),
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
fn settings_language_round_trips_through_ipc() {
    let app = build_app();
    let window = manager_window(&app);
    let call = |cmd: &str, body: Value| get_ipc_response(&window, request(cmd, local_url(), body));

    for language in ["th", "en"] {
        let response = call(
            "settings_set",
            json!({
                "input": {
                    "downloadConcurrency": 4,
                    "hfEngine": "native",
                    "closeBehavior": "ask",
                    "theme": "dark",
                    "language": language,
                    "registryCacheTtl": 6,
                    "bandwidthCapKibS": null
                }
            }),
        )
        .unwrap();
        let saved: Value = response.deserialize().unwrap();
        assert_eq!(saved["language"], language);
        let response = call("settings_get", json!({})).unwrap();
        let loaded: Value = response.deserialize().unwrap();
        assert_eq!(loaded, saved);
    }
}

#[test]
fn setup_restart_requires_a_saved_root_change() {
    let app = build_app();
    let window = manager_window(&app);
    let call = |cmd: &str, body: Value| get_ipc_response(&window, request(cmd, local_url(), body));

    let before = app.calls.count();
    let error = call("setup_restart", json!({})).unwrap_err();
    assert_eq!(error["code"], "restart_not_required");
    assert_eq!(app.calls.count(), before, "no restart without a change");

    let new_root = app.root.parent().unwrap().join("chosen root");
    call(
        "setup_select_root",
        json!({ "path": new_root.display().to_string(), "draft": "{}" }),
    )
    .unwrap();

    let before = app.calls.count();
    call("setup_restart", json!({})).unwrap();
    assert_eq!(app.calls.count(), before + 1, "exactly one restart request");
}

#[test]
fn repository_browser_urls_reject_empty_authority_and_userinfo() {
    let app = build_app();
    let window = manager_window(&app);
    for repository in [
        "",
        "not a URL",
        "https:////@github.com/example/node",
        "https:///github.com/x/y",
        "https:////github.com/x/y",
        "https://@github.com/x/y",
        "https://github.com@evil.com/x",
        r"https://\github.com/x/y",
        r"https:\\github.com/x/y",
        "http://github.com/example/node",
        "file:///C:/Windows/win.ini",
        "javascript:alert(1)",
        "git@github.com:example/node.git",
        "https://user:secret@github.com/example/node",
        "https://user@github.com/example/node",
        "https://github.com/example/node?token=secret",
        "https://github.com/example/node#secret",
        "https://github.com:444/example/node",
        "https://github.com/",
        "https://github.com/example/\nnode",
    ] {
        let before = app.calls.count();
        let response = get_ipc_response(
            &window,
            request(
                "node_open_repository",
                local_url(),
                json!({ "repository": repository }),
            ),
        );
        assert_eq!(
            response.unwrap_err(),
            json!({
                "code": "validation", "message": "The repository URL is not valid."
            })
        );
        assert_eq!(app.calls.count(), before, "shell must not be invoked");
    }
}

#[test]
fn node_list_exposes_recent_failures_and_correct_launch_blocking() {
    use application::custom_nodes::{NodeOperation, NodeRepository, OperationKind, OperationPhase};
    use desktop::ipc::node_dto::NodeListDto;
    use domain::{
        custom_nodes::{DependencyState, InstalledNode, NodeId, NodeSource, ProvenanceOrigin},
        environment::{EnvId, PinSet},
        shared::model_values::SafeFileName,
    };
    use infrastructure::{custom_nodes::sqlite::SqliteNodes, shared::sqlite::Db};

    let app = build_app();
    let window = manager_window(&app);
    tauri::async_runtime::block_on(async {
        let store = SqliteNodes {
            db: Db::open(app.root.join("data/app.db")).await.unwrap(),
        };
        let node = InstalledNode {
            id: NodeId::new("node-1").unwrap(),
            name: SafeFileName::new("pack").unwrap(),
            source: NodeSource::Manual,
            origin: ProvenanceOrigin::App,
            enabled: true,
            single_file: false,
            tree_digest: "tree".into(),
            dependencies: DependencyState::NotRun,
            conflict: false,
            dirty: None,
        };
        store.save_node(&node).await.unwrap();
        for index in 0..23 {
            let phase = match index {
                20 => OperationPhase::CleanupPending,
                21 => OperationPhase::Published,
                22 => OperationPhase::Prepared,
                n if n % 2 == 0 => OperationPhase::Completed,
                _ => OperationPhase::Failed,
            };
            let id = format!("op-{index:02}");
            let op = NodeOperation {
                id: NodeId::new(&id).unwrap(),
                kind: OperationKind::Install,
                phase,
                node: node.clone(),
                prior: None,
                environment: EnvId::new("env-1").unwrap(),
                pins: PinSet::new(),
                binding: "binding".into(),
                artifact_digest: None,
                requirements_digest: None,
                hook_digest: None,
                consent_identity: None,
                failure: (phase == OperationPhase::Failed).then(|| "Installation failed".into()),
                staging: format!(".disabled/.vjump-staging-{id}"),
                backup: format!(".disabled/.vjump-backup-{id}"),
                dependency_requirements: Vec::new(),
            };
            store.save_operation(&op).await.unwrap();
        }
    });
    let response = get_ipc_response(&window, request("node_list", local_url(), json!({}))).unwrap();
    let list: NodeListDto = response.deserialize().unwrap();
    assert_eq!(list.nodes[0].id, "node-1");
    assert!(list.pending_reviews.is_empty());
    assert_eq!(list.recent.len(), 16);
    assert_eq!(list.recent[0].id, "op-19");
    assert_eq!(list.recent[15].id, "op-04");
    assert_eq!(
        list.recent[0].failure.as_deref(),
        Some("Installation failed")
    );
    assert!(
        list.recent
            .iter()
            .all(|op| op.node_id == "node-1" && !op.blocks_start)
    );
    assert_eq!(list.unresolved.len(), 2);
    assert!(
        list.unresolved
            .iter()
            .any(|op| op.phase == "cleanup_pending" && !op.blocks_start)
    );
    assert!(
        list.unresolved
            .iter()
            .any(|op| op.phase == "published" && op.blocks_start)
    );
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

#[test]
fn the_updater_and_clipboard_plugin_commands_are_not_granted_to_any_window() {
    // Updates and the clipboard are used from Rust only, through the app's own commands.
    let capability = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/capabilities/manager.json"
    ))
    .unwrap();
    assert!(!capability.contains("updater:"));
    assert!(!capability.contains("clipboard-manager:"));
}

#[test]
fn update_and_diagnostics_commands_answer_without_configuration() {
    let app = build_app();
    let window = manager_window(&app);
    let call = |cmd: &str, body: Value| get_ipc_response(&window, request(cmd, local_url(), body));

    let status = call("app_update_status", json!({})).unwrap();
    let status = serde_json::from_slice::<Value>(&body_bytes(&status)).unwrap();
    assert_eq!(status["configured"], false);
    assert_eq!(status["currentVersion"], env!("CARGO_PKG_VERSION"));

    for (cmd, body) in [
        ("app_update_check", json!({})),
        (
            "app_update_install",
            json!({ "updateId": "update-unknown", "onEvent": "__CHANNEL__:6" }),
        ),
    ] {
        let error = call(cmd, body).unwrap_err();
        assert_eq!(error["code"], "updater_not_configured", "{cmd}");
    }

    // The clipboard is not available in the test app: the redacted text comes back to select.
    let copied = call("app_copy_diagnostics", json!({})).unwrap();
    let copied = serde_json::from_slice::<Value>(&body_bytes(&copied)).unwrap();
    assert_eq!(copied["copied"], false);
    let text = copied["text"].as_str().unwrap();
    assert!(text.contains("== Availability warnings =="));
    assert!(!text.contains(&app.root.display().to_string()));
}

fn body_bytes(body: &tauri::ipc::InvokeResponseBody) -> Vec<u8> {
    match body {
        tauri::ipc::InvokeResponseBody::Json(text) => text.clone().into_bytes(),
        tauri::ipc::InvokeResponseBody::Raw(bytes) => bytes.clone(),
    }
}

#[test]
fn recovery_handlers_keep_root_selection_diagnostics_and_pickers_available() {
    use application::environment::{RootInfo, RootSource};
    use desktop::RecoveryInfo;
    for code in ["root_missing", "root_inaccessible", "rebuild_required"] {
        // setup_status retains the specific reason; normal-service commands use
        // IpcError::recovery, which maps inaccessible roots to recovery_required.
        let recovery_code = if code == "root_inaccessible" {
            "recovery_required"
        } else {
            code
        };
        let recorded = std::env::temp_dir().join("recorded-missing-install");
        let app = build_app_with(Options {
            recovery: Some(RecoveryInfo {
                code: code.into(),
                message: "Open or rebuild the installation.".into(),
                root: Some(RootInfo {
                    root: recorded.clone(),
                    source: RootSource::Bootstrap,
                }),
            }),
            ..Default::default()
        });
        let window = manager_window(&app);
        for (cmd, body) in [
            (
                "setup_open_install",
                json!({ "path": app.root.display().to_string() }),
            ),
            (
                "setup_new_install",
                json!({ "path": app.root.join("new").display().to_string() }),
            ),
            ("app_copy_diagnostics", json!({})),
            ("pick_folder", json!({ "title": null })),
            ("pick_file", json!({ "title": null, "extensions": [] })),
        ] {
            let before = app.calls.count();
            let response = get_ipc_response(&window, request(cmd, local_url(), body));
            assert!(response.is_ok(), "{code}/{cmd}: {response:?}");
            if cmd.starts_with("setup_") {
                assert!(
                    app.calls.count() >= before + 2,
                    "opener and controlled restart both ran"
                );
            }
        }
        let updater = get_ipc_response(
            &window,
            request("app_update_status", local_url(), json!({})),
        )
        .unwrap();
        let updater: Value = serde_json::from_slice(&body_bytes(&updater)).unwrap();
        assert_eq!(updater["configured"], false);
        for (cmd, body, expected_code) in [
            ("app_update_check", json!({}), "updater_not_configured"),
            (
                "app_update_install",
                json!({ "updateId": "update-unknown", "onEvent": "__CHANNEL__:6" }),
                recovery_code,
            ),
            ("setup_list_curated_nodes", json!({}), recovery_code),
        ] {
            let error = get_ipc_response(&window, request(cmd, local_url(), body)).unwrap_err();
            assert_eq!(error["code"], expected_code, "{code}/{cmd}");
        }
        let status =
            get_ipc_response(&window, request("setup_status", local_url(), json!({}))).unwrap();
        let status: Value = serde_json::from_slice(&body_bytes(&status)).unwrap();
        assert_eq!(status["recovery"]["code"], code);
        assert_eq!(status["installRoot"], recorded.display().to_string());
        let error =
            get_ipc_response(&window, request("settings_get", local_url(), json!({}))).unwrap_err();
        assert_eq!(error["code"], recovery_code);
        for label in [COMFYUI_WINDOW, "untrusted"] {
            let other =
                tauri::WebviewWindowBuilder::new(&app.app, label, tauri::WebviewUrl::default())
                    .build()
                    .unwrap();
            assert_denied(&app, &other, local_url(), label);
        }
        assert_denied(
            &app,
            &window,
            "https://example.invalid".parse().unwrap(),
            "recovery remote origin",
        );
    }
}
