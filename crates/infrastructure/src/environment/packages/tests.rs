use domain::environment::DistName;

use super::*;

fn strings(args: Vec<OsString>) -> Vec<String> {
    args.into_iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect()
}

#[test]
fn install_arguments_enforce_wheels_the_index_and_the_constraints() {
    let python = Path::new("/envs/e/bin/python");
    let spec = InstallSpec {
        requirements: vec!["torch==2.14.1".into(), "torchvision==0.29.1".into()],
        index_url: Some("https://download.pytorch.org/whl/cu130".into()),
        ..InstallSpec::default()
    };
    assert_eq!(
        strings(install_args(python, &spec, false)),
        [
            "pip",
            "install",
            "--python",
            "/envs/e/bin/python",
            "--only-binary",
            ":all:",
            "--index-url",
            "https://download.pytorch.org/whl/cu130",
            "torch==2.14.1",
            "torchvision==0.29.1"
        ]
    );

    let spec = InstallSpec {
        requirement_files: vec!["/c/ComfyUI/requirements.txt".into()],
        constraint_file: Some("/c/constraints.txt".into()),
        upgrade: true,
        allow_source_builds: true,
        ..InstallSpec::default()
    };
    let args = strings(install_args(python, &spec, true));
    assert_eq!(
        args,
        [
            "pip",
            "install",
            "--python",
            "/envs/e/bin/python",
            "--dry-run",
            "-c",
            "/c/constraints.txt",
            "--upgrade",
            "-r",
            "/c/ComfyUI/requirements.txt"
        ],
        "source builds are only allowed with explicit consent, and the preview never mutates"
    );
    assert!(!args.contains(&"--only-binary".to_owned()));
}

#[test]
fn a_guarded_requirement_file_is_fed_on_stdin_from_the_guarded_directory() {
    use application::environment::{GuardedRequirements, Retained};
    use std::sync::Arc;
    let python = Path::new("/envs/e/bin/python");
    let file = PathBuf::from("/c/custom_nodes/pack/requirements.txt");
    let retained = Retained::new(Arc::new(()));
    let spec = InstallSpec {
        requirement_files: vec!["/c/ComfyUI/requirements.txt".into(), file.clone()],
        guarded: Some(GuardedRequirements {
            path: file,
            directory: "/c/custom_nodes/pack".into(),
            text: Arc::from(
                &b"numpy
"[..],
            ),
            retained: retained.clone(),
        }),
        ..InstallSpec::default()
    };
    assert_eq!(
        strings(install_args(python, &spec, false)),
        [
            "pip",
            "install",
            "--python",
            "/envs/e/bin/python",
            "--only-binary",
            ":all:",
            "-r",
            "/c/ComfyUI/requirements.txt",
            "-r",
            "-"
        ],
        "only the guarded file is replaced by standard input"
    );
    let command = with_guarded(CommandSpec::new("uv"), &spec);
    assert_eq!(
        command.cwd.as_deref(),
        Some(Path::new("/c/custom_nodes/pack"))
    );
    assert_eq!(
        command.stdin.as_deref(),
        Some(
            &b"numpy
"[..]
        )
    );
    assert_eq!(command.retained, retained);
    // A spec which does not list the guarded file leaves the command and its arguments alone.
    let unlisted = InstallSpec {
        requirement_files: vec![],
        ..spec
    };
    assert!(!strings(install_args(python, &unlisted, false)).contains(&"-".to_owned()));
    let command = with_guarded(CommandSpec::new("uv"), &unlisted);
    assert!(command.cwd.is_none() && command.stdin.is_none() && command.retained.is_empty());
}

#[test]
fn a_dry_run_lists_installs_and_removals() {
    let lines: Vec<String> = [
        "Using Python 3.13.16 environment at: venv1",
        "Resolved 6 packages in 321ms",
        "Would download 6 packages",
        "Would uninstall 1 package",
        "Would install 6 packages",
        " + certifi==2026.7.22",
        " + requests==2.34.2",
        " - six==1.16.0",
        " + six==1.17.0",
        " ~ pip==26.2.1",
        " + torch==2.15.0+cu130",
        "warning: something + else == odd",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    let preview = parse_dry_run(&lines);
    let names: Vec<_> = preview
        .installs
        .iter()
        .map(|p| format!("{}=={}", p.name, p.version))
        .collect();
    assert_eq!(
        names,
        [
            "certifi==2026.7.22",
            "requests==2.34.2",
            "six==1.17.0",
            "torch==2.15.0+cu130"
        ]
    );
    assert_eq!(preview.removals.len(), 1);
    assert_eq!(preview.removals[0].name, "six");
    assert_eq!(preview.removals[0].version, "1.16.0");
}

#[test]
fn the_package_list_is_parsed_from_json_even_with_leading_noise() {
    let packages = parse_list("Using Python 3.13.16 environment at: venv1\n[{\"name\":\"six\",\"version\":\"1.16.0\"},{\"name\":\"torch\",\"version\":\"2.14.1+cu130\"}]\n").unwrap();
    assert_eq!(packages.len(), 2);
    assert_eq!(packages[1].version, "2.14.1+cu130");
    assert!(parse_list("not json").is_err());
    assert_eq!(parse_list("[]").unwrap(), Vec::new());
}

#[test]
fn constraints_are_exact_sorted_and_never_use_wildcards() {
    let mut pins = PinSet::new();
    pins.insert(DistName::new("triton_windows").unwrap(), "3.8.0.post29");
    pins.insert(DistName::new("torch").unwrap(), "2.14.1+cu130");
    pins.insert(DistName::new("torchvision").unwrap(), "0.29.1+cu130");
    let text = render_constraints(&pins, &["numpy<2.3".into(), "  pillow<12  ".into()]);
    let lines: Vec<&str> = text.lines().filter(|line| !line.starts_with('#')).collect();
    assert_eq!(
        lines,
        [
            "torch==2.14.1+cu130",
            "torchvision==0.29.1+cu130",
            "triton-windows==3.8.0.post29",
            "numpy<2.3",
            "pillow<12"
        ]
    );
    assert!(!text.contains('*'));
}

#[tokio::test]
async fn constraints_are_written_atomically_under_the_cache() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("install ไทย");
    let mut pins = PinSet::new();
    pins.insert(DistName::new("torch").unwrap(), "2.14.1+cu130");
    let env = EnvId::new("env-1").unwrap();
    let path = UvPackages::new()
        .write_constraints(&root, &env, &pins, &[])
        .await
        .unwrap();
    assert_eq!(path, root.join("data/cache/constraints-env-1.txt"));
    assert!(
        std::fs::read_to_string(&path)
            .unwrap()
            .contains("torch==2.14.1+cu130")
    );
    // Rewriting replaces the file and leaves no temporary file behind.
    pins.insert(DistName::new("torchvision").unwrap(), "0.29.1+cu130");
    UvPackages::new()
        .write_constraints(&root, &env, &pins, &[])
        .await
        .unwrap();
    assert!(
        std::fs::read_to_string(&path)
            .unwrap()
            .contains("torchvision")
    );
    assert_eq!(
        std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
        1
    );
}

#[test]
fn preview_preflight_accepts_binary_requirements_and_refuses_source_syntax() {
    use application::environment::GuardedRequirements;
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("local-1.0-py3-none-any.whl"),
        b"wheel",
    )
    .unwrap();
    std::fs::write(
        directory.path().join("more.txt"),
        "numpy>=1.25\n-r deeper.txt\n",
    )
    .unwrap();
    std::fs::write(
        directory.path().join("deeper.txt"),
        "--editable ../source\n",
    )
    .unwrap();
    std::fs::write(directory.path().join("ok.txt"), "-r ok.txt\npillow\n").unwrap();
    std::fs::create_dir(directory.path().join("source")).unwrap();
    let check = |text: &str| {
        let path = directory.path().join("requirements.txt");
        preflight_requirements(&InstallSpec {
            requirement_files: vec![path.clone()],
            guarded: Some(GuardedRequirements {
                path,
                directory: directory.path().to_owned(),
                text: std::sync::Arc::from(text.as_bytes()),
                retained: Default::default(),
            }),
            ..InstallSpec::default()
        })
    };
    for accepted in [
        "numpy>=1.25,<3\n# comment\n\ntorch==2.7.0 # pinned\n",
        "pywin32>=306 ; sys_platform == 'win32'\n",
        "opencv-python[headless]~=4.8; python_version >= \"3.9\" and platform_machine != 'arm64'\n",
        "./local-1.0-py3-none-any.whl\n",
        "local @ ./local-1.0-py3-none-any.whl\n",
        "wheel @ https://example.invalid/wheel-1.0-py3-none-any.whl\n",
        // A file reached twice is read once, and an include cycle terminates.
        "-r ok.txt\n-r ok.txt\n",
    ] {
        assert!(check(accepted).is_ok(), "{accepted:?}");
    }
    for refused in [
        "-e ../source\n",
        "--editable=../source\n",
        "../source\n",
        "source\n",
        ".\n",
        "pkg @ ../source\n",
        "pkg @ file:///c/source\n",
        "git+https://example.invalid/pkg.git\n",
        "https://example.invalid/pkg-1.0.tar.gz\n",
        "pkg==1.0 ; python_version > '3' @ ../source\n",
        "pkg ; \n",
        "--no-binary pkg\n",
        "--unknown ../source\n",
        "pkg \\n==1.0\n",
        "-r more.txt\n",
        "-r missing.txt\n",
    ] {
        assert!(check(refused).is_err(), "{refused:?}");
    }
}

#[test]
fn no_deps_is_explicit_and_keeps_binary_only_constraints() {
    let spec = InstallSpec {
        requirements: vec!["/cache/verified.whl".into()],
        constraint_file: Some("/cache/pins.txt".into()),
        no_deps: true,
        ..InstallSpec::default()
    };
    let args = strings(install_args(Path::new("/env/python"), &spec, false));
    assert!(args.windows(2).any(|p| p == ["--only-binary", ":all:"]));
    assert!(args.windows(2).any(|p| p == ["-c", "/cache/pins.txt"]));
    assert!(args.iter().any(|arg| arg == "--no-deps"));
    assert!(
        !strings(install_args(
            Path::new("/env/python"),
            &InstallSpec::default(),
            false
        ))
        .iter()
        .any(|arg| arg == "--no-deps")
    );
}
