use std::sync::Arc;

use application::environment::{NullSink, RunId, StepId};
use domain::environment::{EnvId, NewEnvironment, Ownership, PythonSourceKind};

use super::*;

fn probe(version: [u8; 3]) -> Probe {
    Probe {
        version,
        bits: 64,
        implementation: "cpython".into(),
        base_prefix: "/base".into(),
        header: true,
        import_library: true,
        pip: true,
    }
}

fn check(checks: &[ValidationCheck], id: CheckId) -> &ValidationCheck {
    checks.iter().find(|check| check.id == id).unwrap()
}

#[test]
fn a_healthy_interpreter_passes_every_check_on_both_oses() {
    for os in [OsKind::Windows, OsKind::Linux] {
        let checks = checks_from_probe(&probe([3, 13, 16]), os, Some(true));
        assert!(
            checks.iter().all(|check| check.passed),
            "{os:?}: {checks:?}"
        );
        assert_eq!(
            checks.iter().any(|check| check.id == CheckId::MsvcRuntime),
            os == OsKind::Windows,
            "the MSVC check only exists on Windows"
        );
    }
}

#[test]
fn unsupported_versions_and_32_bit_block_setup() {
    for version in [[3, 11, 9], [3, 15, 0], [2, 7, 18]] {
        let checks = checks_from_probe(&probe(version), OsKind::Linux, None);
        let version_check = check(&checks, CheckId::VersionSupported);
        assert!(!version_check.passed, "{version:?}");
        assert_eq!(version_check.blocks, [BlockedFeature::Setup]);
    }
    for version in [[3, 12, 0], [3, 13, 16], [3, 14, 8]] {
        assert!(
            check(
                &checks_from_probe(&probe(version), OsKind::Linux, None),
                CheckId::VersionSupported
            )
            .passed
        );
    }
    let mut pypy = probe([3, 13, 0]);
    pypy.implementation = "pypy".into();
    assert!(
        !check(
            &checks_from_probe(&pypy, OsKind::Linux, None),
            CheckId::VersionSupported
        )
        .passed
    );
    let mut narrow = probe([3, 13, 0]);
    narrow.bits = 32;
    let checks = checks_from_probe(&narrow, OsKind::Linux, None);
    assert_eq!(
        check(&checks, CheckId::Is64Bit).blocks,
        [BlockedFeature::Setup]
    );
}

#[test]
fn missing_headers_block_only_triton_and_are_judged_per_os() {
    let mut no_header = probe([3, 13, 16]);
    no_header.header = false;
    for os in [OsKind::Windows, OsKind::Linux] {
        let checks = checks_from_probe(&no_header, os, Some(true));
        let development = check(&checks, CheckId::DevelopmentFiles);
        assert!(!development.passed);
        assert_eq!(development.blocks, [BlockedFeature::Triton]);
    }
    // Linux needs Python.h only; Windows also needs the import library.
    let mut no_library = probe([3, 13, 16]);
    no_library.import_library = false;
    assert!(
        check(
            &checks_from_probe(&no_library, OsKind::Linux, None),
            CheckId::DevelopmentFiles
        )
        .passed
    );
    assert!(
        !check(
            &checks_from_probe(&no_library, OsKind::Windows, Some(true)),
            CheckId::DevelopmentFiles
        )
        .passed
    );
}

#[test]
fn msvc_runtime_failure_blocks_setup_on_windows() {
    let checks = checks_from_probe(&probe([3, 13, 16]), OsKind::Windows, Some(false));
    let msvc = check(&checks, CheckId::MsvcRuntime);
    assert!(!msvc.passed);
    assert_eq!(msvc.blocks, [BlockedFeature::Setup]);
}

#[test]
fn probe_output_is_parsed_from_the_marker_line() {
    let line = r#"noise
VJ-PROBE:{"version":[3,13,16],"bits":64,"implementation":"cpython","base_prefix":"C:\\Python","header":true,"import_library":false,"pip":true}
trailing"#;
    let parsed = parse_probe(line).unwrap();
    assert_eq!(parsed.version, [3, 13, 16]);
    assert!(!parsed.import_library);
    assert_eq!(parse_probe("no marker here"), None);
    assert_eq!(parse_probe("VJ-PROBE:{broken"), None);
}

#[test]
fn pyvenv_cfg_is_parsed_for_uv_and_stdlib_venvs() {
    let uv = "home = C:\\Python\\cpython-3.13.16\nimplementation = CPython\nuv = 0.13.1\nversion_info = 3.13.16\ninclude-system-site-packages = false\n";
    let config = parse_pyvenv_cfg(uv).unwrap();
    assert_eq!(config.version(), Some(PythonVersion::new(3, 13, 16)));
    assert_eq!(config.home, PathBuf::from("C:\\Python\\cpython-3.13.16"));
    let stdlib = "home = /usr/bin\ninclude-system-site-packages = false\nversion = 3.12.4\nexecutable = /usr/bin/python3.12\n";
    assert_eq!(
        parse_pyvenv_cfg(stdlib).unwrap().version(),
        Some(PythonVersion::new(3, 12, 4))
    );
    let release = "home = /x\nversion_info = 3.14.8.final.0\n";
    assert_eq!(
        parse_pyvenv_cfg(release).unwrap().version(),
        Some(PythonVersion::new(3, 14, 8))
    );
    // uv records only major.minor for a venv made from a minor-version link; the exact patch is
    // then read from the venv's own Python.
    let minor_only = parse_pyvenv_cfg("home = /x/cpython-3.13\nversion_info = 3.13\n").unwrap();
    assert_eq!(
        (minor_only.major, minor_only.minor, minor_only.patch),
        (3, 13, None)
    );
    assert_eq!(minor_only.version(), None);
    assert_eq!(
        parse_pyvenv_cfg("version = 3.13.1\n"),
        None,
        "home is required"
    );
    assert_eq!(
        parse_pyvenv_cfg("home = /x\n"),
        None,
        "a version is required"
    );
}

#[test]
fn normalize_handles_missing_tails_unicode_and_the_verbatim_prefix() {
    let directory = tempfile::tempdir().unwrap();
    let missing = directory.path().join("venvs ไทย").join("new env");
    let normalized = normalize_path(&missing);
    assert!(normalized.ends_with(PathBuf::from("venvs ไทย").join("new env")));
    assert!(!normalized.to_string_lossy().starts_with(r"\\?\"));
    assert!(normalized.is_absolute());
    // An existing path normalizes to the same place as a missing child of it.
    assert_eq!(
        normalize_path(directory.path()),
        normalize_path(&directory.path().join("."))
            .components()
            .collect::<PathBuf>()
    );
}

fn ctx() -> StepContext {
    StepContext {
        run_id: RunId::new("test").unwrap(),
        step: StepId::PythonVenv,
        sink: Arc::new(NullSink),
        cancel: CancelToken::new(),
        allow_source_builds: false,
    }
}

#[tokio::test]
async fn validation_classifies_custom_venv_locations_without_running_python() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("install");
    let adapter = PythonAdapter::new();
    let validate = |path: PathBuf| {
        let root = root.clone();
        async move {
            adapter
                .validate(&root, &PythonSourceInput::CustomVenv(path))
                .await
                .unwrap()
        }
    };

    // A folder that does not exist yet, and an empty one, become app-owned venvs.
    let new = directory.path().join("brand new ไทย");
    let report = validate(new.clone()).await;
    assert_eq!(report.target, ValidationTarget::EmptyFolder);
    assert!(report.usable());
    assert!(!new.exists(), "validation never creates anything");
    let empty = directory.path().join("empty");
    fs::create_dir_all(&empty).unwrap();
    assert_eq!(validate(empty).await.target, ValidationTarget::EmptyFolder);

    // A folder with unrelated content is rejected and left alone.
    let busy = directory.path().join("busy");
    fs::create_dir_all(&busy).unwrap();
    fs::write(busy.join("notes.txt"), b"keep me").unwrap();
    let report = validate(busy.clone()).await;
    assert_eq!(report.target, ValidationTarget::Rejected);
    assert!(!report.usable());
    assert_eq!(fs::read(busy.join("notes.txt")).unwrap(), b"keep me");

    // A file, and a venv without an interpreter, are rejected.
    let file = directory.path().join("file.txt");
    fs::write(&file, b"x").unwrap();
    assert_eq!(validate(file).await.target, ValidationTarget::Rejected);
    let broken = directory.path().join("broken venv");
    fs::create_dir_all(&broken).unwrap();
    fs::write(broken.join("pyvenv.cfg"), b"home = /x\nversion = 3.13.1\n").unwrap();
    assert_eq!(validate(broken).await.target, ValidationTarget::Rejected);

    // A missing interpreter is rejected.
    let report = adapter
        .validate(
            &root,
            &PythonSourceInput::CustomInterpreter(directory.path().join("no python.exe")),
        )
        .await
        .unwrap();
    assert_eq!(report.target, ValidationTarget::Rejected);
}

#[tokio::test]
async fn create_venv_keeps_a_matching_venv_and_refuses_unrelated_folders() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("install");
    let adapter = PythonAdapter::new();
    let interpreter = directory.path().join("python");

    let existing = directory.path().join("envs").join("env-1");
    fs::create_dir_all(&existing).unwrap();
    fs::write(
        existing.join("pyvenv.cfg"),
        b"home = /x\nversion_info = 3.13.16\n",
    )
    .unwrap();
    let info = adapter
        .create_venv(&root, &interpreter, &existing, &ctx())
        .await
        .unwrap();
    assert_eq!(info.version, PythonVersion::new(3, 13, 16));

    let unrelated = directory.path().join("documents ไทย");
    fs::create_dir_all(&unrelated).unwrap();
    fs::write(unrelated.join("important.docx"), b"precious").unwrap();
    let error = adapter
        .create_venv(&root, &interpreter, &unrelated, &ctx())
        .await
        .unwrap_err();
    assert!(matches!(error, ProvisionError::Failed(message) if message.contains("left unchanged")));
    assert_eq!(
        fs::read(unrelated.join("important.docx")).unwrap(),
        b"precious"
    );

    let file = directory.path().join("a file");
    fs::write(&file, b"x").unwrap();
    assert!(
        adapter
            .create_venv(&root, &interpreter, &file, &ctx())
            .await
            .is_err()
    );
}

#[test]
fn only_authorized_venv_directories_are_deleted() {
    let directory = tempfile::tempdir().unwrap();
    let venv = directory.path().join("envs ไทย").join("env-1");
    fs::create_dir_all(venv.join("Lib")).unwrap();
    fs::write(venv.join("pyvenv.cfg"), b"home = /x\nversion = 3.13.1\n").unwrap();
    fs::write(venv.join("Lib").join("file"), b"x").unwrap();

    // The location must equal the authorized one.
    assert!(remove_venv_dir(&venv, &directory.path().join("elsewhere")).is_err());
    // A shallow path (a drive or home root) is never deleted.
    let shallow = if cfg!(windows) {
        Path::new(r"C:\Users")
    } else {
        Path::new("/home")
    };
    assert!(remove_venv_dir(shallow, shallow).is_err());
    assert!(venv.exists());

    // A populated folder that is not a venv is left unchanged.
    let documents = directory.path().join("my documents ไทย");
    fs::create_dir_all(&documents).unwrap();
    fs::write(documents.join("a.txt"), b"keep").unwrap();
    assert!(remove_venv_dir(&documents, &documents).is_err());
    assert!(documents.join("a.txt").exists());

    remove_venv_dir(&venv, &venv).unwrap();
    assert!(!venv.exists());
    // Deleting twice is fine.
    remove_venv_dir(&venv, &venv).unwrap();
}

#[tokio::test]
async fn adopted_environments_are_never_removed_by_the_adapter() {
    let directory = tempfile::tempdir().unwrap();
    let venv = directory.path().join("user venv").join("env");
    fs::create_dir_all(&venv).unwrap();
    fs::write(venv.join("pyvenv.cfg"), b"home = /x\nversion = 3.13.1\n").unwrap();
    let new = |ownership| {
        PythonEnvironment::provisioning(NewEnvironment {
            id: EnvId::new("env-adopted").unwrap(),
            path: venv.clone(),
            ownership,
            source: PythonSourceKind::CustomVenv,
            base_interpreter: None,
            python_version: PythonVersion::new(3, 13, 1),
            profile_id: "p".into(),
            profile_revision: 1,
            authorized_location: venv.clone(),
            now: "t".into(),
        })
    };
    let adapter = PythonAdapter::new();
    assert!(
        adapter
            .remove_environment(&new(Ownership::Adopted))
            .await
            .is_err()
    );
    assert!(venv.exists(), "an adopted venv must survive");
    adapter
        .remove_environment(&new(Ownership::AppOwned))
        .await
        .unwrap();
    assert!(!venv.exists());
}

#[test]
fn runtime_dlls_are_copied_once_and_never_overwritten() {
    let directory = tempfile::tempdir().unwrap();
    let scripts = directory.path().join("Scripts");
    let home = directory.path().join("python home ไทย");
    fs::create_dir_all(&scripts).unwrap();
    fs::create_dir_all(&home).unwrap();
    for name in [
        "msvcp140.dll",
        "MSVCP140_1.dll",
        "concrt140.dll",
        "pip.exe",
        "other.dll",
    ] {
        fs::write(scripts.join(name), name.as_bytes()).unwrap();
    }
    fs::write(home.join("msvcp140.dll"), b"original").unwrap();
    copy_runtime_dlls(&scripts, &home).unwrap();
    assert_eq!(fs::read(home.join("msvcp140.dll")).unwrap(), b"original");
    assert!(home.join("MSVCP140_1.dll").is_file());
    assert!(home.join("concrt140.dll").is_file());
    assert!(!home.join("pip.exe").exists());
    assert!(!home.join("other.dll").exists());
}

/// Runs against a real interpreter named by `VJ_TEST_PYTHON`; skipped when it is not set.
#[tokio::test]
#[ignore = "needs VJ_TEST_PYTHON pointing at a CPython 3.12 to 3.14 interpreter"]
async fn a_real_interpreter_is_probed() {
    let python = std::env::var("VJ_TEST_PYTHON").expect("VJ_TEST_PYTHON");
    let directory = tempfile::tempdir().unwrap();
    let report = PythonAdapter::new()
        .validate(
            directory.path(),
            &PythonSourceInput::CustomInterpreter(PathBuf::from(python)),
        )
        .await
        .unwrap();
    assert_eq!(report.target, ValidationTarget::Interpreter);
    assert!(report.python_version.is_some());
}
