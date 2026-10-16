//! Binding drift check.
//!
//! These are integration tests because the build script embeds the Windows Common Controls v6
//! manifest into test executables only for integration test targets.

use std::path::{Path, PathBuf};

use desktop::ipc::bindings;
use specta_typescript::Typescript;

const REGENERATE: &str = "cargo test -p desktop --test bindings -- --ignored export_bindings";

fn checked_in_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/src/shared/ipc/bindings.ts")
}

fn export_to(path: &Path) {
    bindings::builder::<tauri::Wry>()
        .export(Typescript::default(), path)
        .expect("failed to export TypeScript bindings");
}

fn normalize(text: &str) -> String {
    text.replace("\r\n", "\n")
}

#[test]
fn bindings_up_to_date() {
    let temp = tempfile::tempdir().unwrap();
    let generated_path = temp.path().join("bindings.ts");
    export_to(&generated_path);
    let generated = normalize(&std::fs::read_to_string(&generated_path).unwrap());

    let checked_in = std::fs::read_to_string(checked_in_path()).unwrap_or_else(|_| {
        panic!("ui/src/shared/ipc/bindings.ts is missing. Regenerate it with: {REGENERATE}")
    });

    assert!(
        normalize(&checked_in) == generated,
        "ui/src/shared/ipc/bindings.ts is out of date with the Rust IPC types. \
         Regenerate it with: {REGENERATE}"
    );
}

#[test]
#[ignore = "writes ui/src/shared/ipc/bindings.ts; run explicitly to regenerate"]
fn export_bindings() {
    let path = checked_in_path();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    export_to(&path);
}
