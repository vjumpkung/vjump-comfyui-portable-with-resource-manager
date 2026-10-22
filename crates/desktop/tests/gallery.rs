//! The gallery and "show in file manager" commands through the real IPC layer: sanitized
//! validation, results free of absolute paths, and the target handed to the shell.
//!
//! These are integration tests because the build script embeds the Windows Common Controls v6
//! manifest into test executables only for integration test targets.

mod support;

use std::{fs, path::Path};

use application::shared::ExplorerTarget;
use serde_json::{Value, json};
use support::*;
use tauri::{
    WebviewWindow,
    test::{MockRuntime, get_ipc_response},
};

fn invoke(window: &WebviewWindow<MockRuntime>, cmd: &str, body: Value) -> Result<Value, Value> {
    get_ipc_response(window, request(cmd, local_url(), body))
        .map(|response| response.deserialize().unwrap())
}

fn write(app: &TestApp, relative: &str, bytes: &[u8]) {
    let path = app.root.join("ComfyUI").join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

fn explored(app: &TestApp) -> Vec<ExplorerTarget> {
    app.explored.lock().unwrap().clone()
}

fn path_of(target: &ExplorerTarget) -> &Path {
    match target {
        ExplorerTarget::OpenFolder(path) | ExplorerTarget::RevealItem(path) => path,
    }
}

fn location(path: &str) -> Value {
    json!({ "area": "output", "path": path })
}

fn entry_ref(path: &str) -> Value {
    json!({ "location": location(path), "revision": "revision" })
}

#[test]
fn gallery_reveal_selects_files_and_opens_folders() {
    let app = build_app();
    let window = manager_window(&app);
    write(&app, "output/ภาพ ไทย.png", b"png");
    write(&app, "output/folder/inner.png", b"png");

    invoke(
        &window,
        "gallery_reveal",
        json!({ "location": location("ภาพ ไทย.png") }),
    )
    .unwrap();
    invoke(
        &window,
        "gallery_reveal",
        json!({ "location": location("folder") }),
    )
    .unwrap();
    invoke(
        &window,
        "gallery_reveal",
        json!({ "location": location("") }),
    )
    .unwrap();

    let targets = explored(&app);
    assert_eq!(targets.len(), 3);
    assert!(matches!(targets[0], ExplorerTarget::RevealItem(_)));
    assert!(path_of(&targets[0]).ends_with("ComfyUI/output/ภาพ ไทย.png"));
    assert!(matches!(targets[1], ExplorerTarget::OpenFolder(_)));
    assert!(path_of(&targets[1]).ends_with("ComfyUI/output/folder"));
    assert!(matches!(targets[2], ExplorerTarget::OpenFolder(_)));
    assert!(path_of(&targets[2]).ends_with("ComfyUI/output"));
}

#[test]
fn gallery_reveal_refuses_what_is_missing_or_unaddressable() {
    let app = build_app();
    let window = manager_window(&app);
    write(&app, "output/a.png", b"png");

    let missing = invoke(
        &window,
        "gallery_reveal",
        json!({ "location": location("missing.png") }),
    )
    .unwrap_err();
    assert_eq!(missing["code"], "gallery_not_found");
    let hostile = invoke(
        &window,
        "gallery_reveal",
        json!({ "location": location("../a.png") }),
    )
    .unwrap_err();
    assert_eq!(hostile["code"], "validation");
    assert!(explored(&app).is_empty(), "the shell is never asked");
}

#[test]
fn model_reveal_opens_the_models_folder_and_refuses_unknown_models() {
    let app = build_app();
    let window = manager_window(&app);

    invoke(&window, "model_reveal", json!({ "location": null })).unwrap();
    let targets = explored(&app);
    assert_eq!(targets.len(), 1);
    assert!(matches!(targets[0], ExplorerTarget::OpenFolder(_)));
    assert!(path_of(&targets[0]).ends_with("models"));

    let missing = invoke(
        &window,
        "model_reveal",
        json!({ "location": { "rootId": "models", "path": "checkpoints/none.safetensors" } }),
    )
    .unwrap_err();
    assert_eq!(missing["code"], "model_not_found");
    let hostile = invoke(
        &window,
        "model_reveal",
        json!({ "location": { "rootId": "models", "path": "../none.safetensors" } }),
    )
    .unwrap_err();
    assert_eq!(hostile["code"], "validation");
    assert_eq!(explored(&app).len(), 1);
}

#[test]
fn node_reveal_opens_the_custom_nodes_folder_and_refuses_unknown_nodes() {
    let app = build_app();
    let window = manager_window(&app);
    fs::create_dir_all(app.root.join("ComfyUI/custom_nodes")).unwrap();

    invoke(&window, "node_reveal", json!({ "id": null })).unwrap();
    let targets = explored(&app);
    assert_eq!(targets.len(), 1);
    assert!(matches!(targets[0], ExplorerTarget::OpenFolder(_)));
    assert!(path_of(&targets[0]).ends_with("custom_nodes"));

    let unknown = invoke(&window, "node_reveal", json!({ "id": "node-unknown" })).unwrap_err();
    assert_eq!(unknown["code"], "node_not_found");
    let invalid = invoke(&window, "node_reveal", json!({ "id": "../x" })).unwrap_err();
    assert_eq!(invalid["code"], "invalid_identifier");
    assert_eq!(explored(&app).len(), 1);
}

#[test]
fn batches_and_locations_are_validated_before_anything_runs() {
    let app = build_app();
    let window = manager_window(&app);
    let items = |count: usize| -> Vec<Value> { (0..count).map(|_| entry_ref("a.png")).collect() };

    for count in [0, 501] {
        for (cmd, body) in [
            (
                "gallery_move",
                json!({ "input": { "items": items(count), "destination": location("") } }),
            ),
            (
                "gallery_copy",
                json!({ "input": { "items": items(count), "destination": location("") } }),
            ),
            ("gallery_delete_plan", json!({ "items": items(count) })),
        ] {
            let error = invoke(&window, cmd, body).unwrap_err();
            assert_eq!(error["code"], "validation", "{cmd}/{count}");
            assert_eq!(
                error["message"], "Select between 1 and 500 items.",
                "{cmd}/{count}"
            );
        }
    }
    let long_revision = "r".repeat(513);
    for (cmd, body) in [
        ("gallery_list", json!({ "directory": location("../x") })),
        ("gallery_list", json!({ "directory": location("a\\b") })),
        (
            "gallery_list",
            json!({ "directory": location(".vjump-gallery-staging") }),
        ),
        (
            "gallery_rename",
            json!({ "input": {
                "entry": { "location": location("a.png"), "revision": long_revision },
                "newName": "b.png"
            } }),
        ),
        (
            "gallery_move",
            json!({ "input": { "items": [entry_ref("a.png")], "destination": location("../x") } }),
        ),
    ] {
        let error = invoke(&window, cmd, body).unwrap_err();
        assert_eq!(error["code"], "validation", "{cmd}");
    }
    let unknown_plan = invoke(
        &window,
        "gallery_delete",
        json!({ "planId": "plan-unknown" }),
    )
    .unwrap_err();
    assert_eq!(unknown_plan["code"], "gallery_stale");
    assert_eq!(
        unknown_plan["message"],
        "The file or folder changed. Refresh and try again."
    );
}

#[test]
fn files_are_listed_renamed_copied_moved_and_planned_without_exposing_paths() {
    let app = build_app();
    let window = manager_window(&app);
    write(&app, "output/ภาพ ไทย.png", b"png");
    fs::create_dir_all(app.root.join("ComfyUI/output/archive")).unwrap();

    let listing = invoke(
        &window,
        "gallery_list",
        json!({ "directory": location("") }),
    )
    .unwrap();
    assert_eq!(listing["directory"], location(""));
    assert_eq!(listing["warnings"], json!([]));
    let entries = listing["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2);
    let file = entries
        .iter()
        .find(|entry| entry["name"] == "ภาพ ไทย.png")
        .unwrap();
    assert_eq!(file["kind"], "file");
    assert_eq!(file["media"], "image");
    assert_eq!(file["sizeBytes"], 3.0);
    assert_eq!(file["location"], location("ภาพ ไทย.png"));
    assert!(file["modifiedMs"].as_f64().unwrap() > 0.0);
    let folder = entries
        .iter()
        .find(|entry| entry["name"] == "archive")
        .unwrap();
    assert_eq!(folder["kind"], "directory");
    assert_eq!(folder["media"], "other");
    assert_eq!(folder["sizeBytes"], Value::Null);
    assert!(
        !listing
            .to_string()
            .contains(&app.root.display().to_string()),
        "a listing carries no absolute path"
    );

    let renamed = invoke(
        &window,
        "gallery_rename",
        json!({ "input": {
            "entry": { "location": file["location"], "revision": file["revision"] },
            "newName": "renamed.png"
        } }),
    )
    .unwrap();
    assert_eq!(renamed, location("renamed.png"));
    assert!(app.root.join("ComfyUI/output/renamed.png").is_file());
    assert!(!app.root.join("ComfyUI/output/ภาพ ไทย.png").exists());

    let listing = invoke(
        &window,
        "gallery_list",
        json!({ "directory": location("") }),
    )
    .unwrap();
    let renamed_entry = listing["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["name"] == "renamed.png")
        .unwrap()
        .clone();
    let reference = json!({
        "location": renamed_entry["location"],
        "revision": renamed_entry["revision"]
    });

    let copied = invoke(
        &window,
        "gallery_copy",
        json!({ "input": { "items": [reference], "destination": location("archive") } }),
    )
    .unwrap();
    assert_eq!(copied["results"][0]["outcome"], "succeeded");
    assert_eq!(copied["results"][0]["source"], location("renamed.png"));
    assert_eq!(
        copied["results"][0]["destination"],
        location("archive/renamed.png")
    );
    assert_eq!(copied["results"][0]["error"], Value::Null);
    assert!(app.root.join("ComfyUI/output/renamed.png").is_file());
    assert!(
        app.root
            .join("ComfyUI/output/archive/renamed.png")
            .is_file()
    );

    // Moving onto an existing name takes a suffix instead of replacing anything.
    let moved = invoke(
        &window,
        "gallery_move",
        json!({ "input": { "items": [reference], "destination": location("archive") } }),
    )
    .unwrap();
    assert_eq!(moved["results"][0]["outcome"], "succeeded");
    assert_eq!(
        moved["results"][0]["destination"],
        location("archive/renamed (1).png")
    );
    assert!(!app.root.join("ComfyUI/output/renamed.png").exists());
    assert!(
        app.root
            .join("ComfyUI/output/archive/renamed (1).png")
            .is_file()
    );

    // A stale reference fails that item only, with a sanitized error.
    let stale = invoke(
        &window,
        "gallery_move",
        json!({ "input": { "items": [reference], "destination": location("archive") } }),
    )
    .unwrap();
    assert_eq!(stale["results"][0]["outcome"], "failed");
    assert_eq!(stale["results"][0]["error"]["code"], "gallery_not_found");

    let archive = invoke(
        &window,
        "gallery_list",
        json!({ "directory": location("archive") }),
    )
    .unwrap();
    let in_archive = archive["entries"][0].clone();
    // Only the plan is made here: confirming it would send the file to the real Recycle Bin.
    let plan = invoke(
        &window,
        "gallery_delete_plan",
        json!({ "items": [{
            "location": in_archive["location"],
            "revision": in_archive["revision"]
        }] }),
    )
    .unwrap();
    assert!(!plan["planId"].as_str().unwrap().is_empty());
    assert_eq!(plan["totalBytes"], 3.0);
    assert_eq!(plan["items"][0]["kind"], "file");
    assert_eq!(plan["items"][0]["bytes"], 3.0);
    assert_eq!(
        plan["items"][0]["entry"]["location"],
        in_archive["location"]
    );
    assert!(
        !plan.to_string().contains(&app.root.display().to_string()),
        "a plan carries no absolute path"
    );
}
