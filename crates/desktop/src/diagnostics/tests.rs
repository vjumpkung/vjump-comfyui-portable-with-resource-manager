use std::{path::PathBuf, sync::Mutex};

use application::settings::diagnostics::{
    ComfySection, DiagnosticsSnapshot, EnvironmentSection, ManifestSection, PinnedTool,
};
use infrastructure::shared::logging::register_secret;

use super::*;

fn root() -> PathBuf {
    PathBuf::from(if cfg!(windows) {
        r"C:\Users\someone\vjump install ไทย"
    } else {
        "/home/someone/vjump install ไทย"
    })
}

/// A secret-bearing value for `section`, in a form the shared redactor must recognise.
fn leak(section: &str) -> String {
    format!("https://host.test/file?safe=1&X-Amz-Signature=sec-{section}&token=sec-{section}-t")
}

/// Every part of the report carries a secret, an authorization header or a path in the root.
fn snapshot_with_a_secret_everywhere() -> DiagnosticsSnapshot {
    let root = root();
    let inside = |tail: &str| format!("{}/{tail}", root.display());
    DiagnosticsSnapshot {
        app_version: format!("1.0.0 {}", leak("version")),
        os: format!("Windows {}", leak("os")),
        gpus: vec![format!("GPU {}", leak("gpu"))],
        driver_version: Some(format!("591 {}", leak("driver"))),
        driver_cuda: Some(format!("13.1 {}", leak("cuda"))),
        environment: Some(EnvironmentSection {
            id: inside("envs/main"),
            ownership: format!("app_owned {}", leak("ownership")),
            state: format!("verified {}", leak("state")),
            python: format!("3.13.16 {}", leak("python")),
            python_source: format!("managed {}", leak("source")),
            profile_id: format!("profile {}", leak("profile")),
            profile_revision: 1,
            profile_label: Some(format!("label {}", leak("label"))),
            cuda_variant: Some(format!("cu130 {}", leak("variant"))),
            torch: Some(format!("2.14.1 {}", leak("torch"))),
            triton: Some(format!("3.8.0 {}", leak("triton"))),
            protected_pins: vec![("torch".into(), format!("2.14.1 {}", leak("pin")))],
            accelerators: vec![format!("sageattention failed {}", leak("accelerator"))],
        }),
        manifest: Some(ManifestSection {
            profile_id: format!("manifest {}", leak("manifest")),
            profile_revision: 2,
        }),
        comfyui: ComfySection {
            revision: Some(format!("abc123 {}", leak("revision"))),
            status: Some(format!("stopped {}", leak("status"))),
        },
        tools: vec![PinnedTool {
            name: "uv".into(),
            version: format!("0.13.1 {}", leak("tool")),
        }],
        app_log: vec![
            format!("opened {}", inside("data/app.db")),
            format!(
                "Authorization: Bearer sec-app-log-bearer {}",
                leak("app-log")
            ),
        ],
        runtime_log: vec![
            format!("loading {}", inside("models/a.safetensors")),
            format!("hf_{} {}", "a".repeat(30), leak("runtime-log")),
        ],
        warnings: vec![format!("warning {}", leak("warning"))],
    }
}

fn report() -> String {
    finalize(&snapshot_with_a_secret_everywhere().render(), &root())
}

#[derive(Default)]
struct Recording {
    written: Mutex<Vec<String>>,
}

impl Clipboard for Recording {
    fn write_text(&self, text: &str) -> Result<(), ClipboardError> {
        self.written.lock().unwrap().push(text.to_owned());
        Ok(())
    }
}

struct Failing;

impl Clipboard for Failing {
    fn write_text(&self, _: &str) -> Result<(), ClipboardError> {
        Err(ClipboardError)
    }
}

fn assert_clean(text: &str) {
    for leaked in ["sec-", "hf_aaaa"] {
        assert!(!text.contains(leaked), "`{leaked}` leaked:\n{text}");
    }
    let root = root();
    let root = root.display().to_string();
    assert!(!text.contains(&root), "the install root leaked:\n{text}");
    assert!(!text.contains(&root.replace('\\', "/")));
}

#[test]
fn diagnostics_redacts_every_section_and_clipboard_payload() {
    register_secret("registered-session-secret-value");
    let mut snapshot = snapshot_with_a_secret_everywhere();
    snapshot
        .warnings
        .push("also registered-session-secret-value".into());
    let text = finalize(&snapshot.render(), &root());

    assert_clean(&text);
    // Safe context survives: only the credentials and the root are replaced.
    assert!(text.contains("safe=1"), "{text}");
    assert!(text.contains("<root>/data/app.db"), "{text}");
    assert!(text.contains("<root>/models/a.safetensors"), "{text}");
    assert!(text.contains("ID: <root>/envs/main"), "{text}");
    assert!(!text.contains("registered-session-secret-value"));
    for header in [
        "App version",
        "== Environment ==",
        "== Install manifest ==",
        "== ComfyUI ==",
        "== Tool versions ==",
        "== Availability warnings ==",
        "== Application log",
        "== ComfyUI runtime log",
    ] {
        assert!(text.contains(header), "{header} missing:\n{text}");
    }

    // The clipboard receives exactly the redacted text that is returned.
    let clipboard = Arc::new(Recording::default());
    let outcome = tauri::async_runtime::block_on(copy_report(clipboard.clone(), text.clone()));
    assert!(outcome.copied);
    assert_eq!(outcome.text, text);
    assert_eq!(*clipboard.written.lock().unwrap(), [text]);
}

#[test]
fn clipboard_failure_returns_selectable_redacted_text() {
    let text = report();
    assert_clean(&text);

    let outcome = tauri::async_runtime::block_on(copy_report(Arc::new(Failing), text.clone()));
    assert!(!outcome.copied);
    assert_eq!(outcome.text, text);
    assert_clean(&outcome.text);
    assert!(
        UnavailableClipboard.write_text(&text).is_err(),
        "the fallback clipboard never claims success"
    );
}

#[test]
fn install_root_is_presented_as_root_in_every_spelling() {
    let root = root();
    let shown = root.display().to_string();
    let forward = shown.replace('\\', "/");
    let backward = shown.replace('/', "\\");
    let text = format!(
        "a {shown}\\x b {forward}/y c \\\\?\\{backward}\\z d {shown} e {}-suffix",
        shown.to_uppercase()
    );
    let replaced = replace_root(&text, &root);
    assert!(replaced.starts_with("a <root>\\x b <root>/y c <root>\\z d <root> e "));
    if cfg!(windows) {
        assert!(replaced.ends_with("e <root>-suffix"), "{replaced}");
    }
    assert!(!replaced.contains(&shown));
    assert!(!replaced.contains(&forward));

    // Nothing is rewritten when no root is known.
    assert_eq!(
        replace_root("keep / and \\", Path::new("")),
        "keep / and \\"
    );
    assert_eq!(
        replace_root("keep / and \\", Path::new("/")),
        "keep / and \\"
    );
}
