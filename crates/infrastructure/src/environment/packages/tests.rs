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
