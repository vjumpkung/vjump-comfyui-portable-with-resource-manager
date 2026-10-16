use std::{process::Command, sync::Arc};

use application::{
    environment::{NullSink, RunId, StepId},
    shared::CancelToken,
};

use super::*;
use crate::shared::test_http::{Route, TestServer};

/// The system Git, found on the test process's PATH.
fn locate_git() -> PathBuf {
    let names: &[&str] = if cfg!(windows) {
        &["git.exe"]
    } else {
        &["git"]
    };
    std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .flat_map(|dir| names.iter().map(move |name| dir.join(name)))
        .find(|candidate| candidate.is_file())
        .expect("git must be installed to run the source tests")
}

fn git(directory: &Path, args: &[&str]) -> String {
    let output = Command::new(locate_git())
        .current_dir(directory)
        .args([
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "tag.gpgsign=false",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

/// A local "remote": main with two commits, a lightweight tag, an annotated tag and a branch.
struct Remote {
    _directory: tempfile::TempDir,
    path: PathBuf,
    first: String,
    second: String,
}

impl Remote {
    fn new(with_requirements: bool) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("remote repo ไทย");
        fs::create_dir_all(&path).unwrap();
        git(&path, &["init", "-b", "main"]);
        if with_requirements {
            fs::write(path.join("requirements.txt"), "numpy\n").unwrap();
        }
        fs::write(path.join("README.md"), "one").unwrap();
        git(&path, &["add", "-A"]);
        git(&path, &["commit", "-m", "first"]);
        let first = git(&path, &["rev-parse", "HEAD"]);
        git(&path, &["tag", "v1.0.0"]);
        fs::write(path.join("README.md"), "two").unwrap();
        git(&path, &["commit", "-am", "second"]);
        let second = git(&path, &["rev-parse", "HEAD"]);
        git(&path, &["tag", "-a", "v1.1.0", "-m", "release 1.1.0"]);
        git(&path, &["branch", "dev"]);
        Self {
            _directory: directory,
            path,
            first,
            second,
        }
    }

    fn url(&self) -> String {
        self.path.to_string_lossy().into_owned()
    }
}

struct Fixture {
    remote: Remote,
    server: TestServer,
    root: PathBuf,
    _directory: tempfile::TempDir,
}

impl Fixture {
    async fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        Self {
            remote: Remote::new(true),
            server: TestServer::start().await,
            root: directory.path().join("install root ไทย"),
            _directory: directory,
        }
    }

    fn source(&self) -> GitSource {
        GitSource::with(
            Downloader::with_loopback_http(),
            Endpoints {
                comfyui_url: self.remote.url(),
                manager_url: self.remote.url(),
                releases_api: self.server.url("/latest"),
            },
            Some(locate_git()),
        )
    }
}

fn ctx() -> StepContext {
    StepContext {
        run_id: RunId::new("test").unwrap(),
        step: StepId::CloneComfyUi,
        sink: Arc::new(NullSink),
        cancel: CancelToken::new(),
        allow_source_builds: false,
    }
}

#[test]
fn ref_names_cannot_inject_options_or_revision_syntax() {
    for good in ["v0.40.0", "release/1.2", "feature_x-1", "master"] {
        assert!(valid_ref_name(good), "{good}");
    }
    for bad in [
        "",
        "-x",
        "--upload-pack=evil",
        "/abs",
        ".hidden",
        "a..b",
        "a@{1}",
        "a b",
        "tag^{}",
        "a//b",
        "end/",
        "end.",
        "x.lock",
        "tab\tname",
        "semi;colon",
        "ünï",
    ] {
        assert!(!valid_ref_name(bad), "{bad:?}");
    }
    assert!(!valid_ref_name(&"a".repeat(201)));
}

#[test]
fn ls_remote_output_is_parsed() {
    let sha = "a".repeat(40);
    let text = format!(
        "{sha}\trefs/tags/v1\n{sha}\trefs/tags/v1^{{}}\nwarning: noise\nnot-a-sha\trefs/x\n"
    );
    let refs = parse_ls_remote(&text);
    assert_eq!(refs.len(), 2);
    assert_eq!(refs[1].1, "refs/tags/v1^{}");
}

#[tokio::test]
async fn tags_resolve_to_the_peeled_commit() {
    let fixture = Fixture::new().await;
    let source = fixture.source();
    let annotated = source
        .resolve(&fixture.root, &ComfyRef::Tag("v1.1.0".into()))
        .await
        .unwrap();
    assert_eq!(
        annotated.commit, fixture.remote.second,
        "the annotated tag is peeled to its commit"
    );
    assert_eq!(annotated.reference, "v1.1.0");
    assert!(annotated.branch.is_none());
    let lightweight = source
        .resolve(&fixture.root, &ComfyRef::Tag("v1.0.0".into()))
        .await
        .unwrap();
    assert_eq!(lightweight.commit, fixture.remote.first);

    let missing = source
        .resolve(&fixture.root, &ComfyRef::Tag("v9.9.9".into()))
        .await
        .unwrap_err();
    assert!(matches!(missing, ProvisionError::Failed(message) if message.contains("not found")));
    for bad in ["--upload-pack=evil", "a..b", ""] {
        assert!(
            source
                .resolve(&fixture.root, &ComfyRef::Tag(bad.into()))
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn branches_and_the_default_head_resolve() {
    let fixture = Fixture::new().await;
    let source = fixture.source();
    let dev = source
        .resolve(&fixture.root, &ComfyRef::Branch("dev".into()))
        .await
        .unwrap();
    assert_eq!(dev.commit, fixture.remote.second);
    assert_eq!(dev.branch.as_deref(), Some("dev"));
    assert!(
        source
            .resolve(&fixture.root, &ComfyRef::Branch("nope".into()))
            .await
            .is_err()
    );
    let manager = source.resolve_manager(&fixture.root).await.unwrap();
    assert_eq!(manager.commit, fixture.remote.second);
    assert!(
        source
            .resolve(&fixture.root, &ComfyRef::KeepCurrent)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn latest_uses_the_release_api_not_the_highest_tag() {
    let fixture = Fixture::new().await;
    // The newest *published release* is the older tag; semver-max would pick v1.1.0.
    fixture.server.route(
        "/latest",
        Route::Body {
            status: 200,
            headers: vec![("Content-Type".into(), "application/json".into())],
            body: br#"{"tag_name":"v1.0.0","name":"Release"}"#.to_vec(),
        },
    );
    let resolved = fixture
        .source()
        .resolve(&fixture.root, &ComfyRef::Latest)
        .await
        .unwrap();
    assert_eq!(resolved.reference, "v1.0.0");
    assert_eq!(resolved.commit, fixture.remote.first);
    let request = fixture.server.requests.lock().unwrap()[0].to_lowercase();
    assert!(request.contains("accept: application/vnd.github+json"));
    assert!(request.contains("user-agent: vjumpkung-comfyui-desktop/"));
}

#[tokio::test]
async fn release_api_failures_are_classified_without_changing_the_request() {
    let fixture = Fixture::new().await;
    let source = fixture.source();
    let resolve = || source.resolve(&fixture.root, &ComfyRef::Latest);

    fixture.server.route(
        "/latest",
        Route::Body {
            status: 403,
            headers: vec![("X-RateLimit-Remaining".into(), "0".into())],
            body: b"{\"message\":\"API rate limit exceeded\"}".to_vec(),
        },
    );
    assert!(
        matches!(resolve().await, Err(ProvisionError::Retryable(message)) if message.contains("rate limit"))
    );

    fixture
        .server
        .route("/latest", Route::status(429, "slow down"));
    assert!(matches!(resolve().await, Err(ProvisionError::Retryable(_))));
    fixture.server.route("/latest", Route::status(503, "later"));
    assert!(matches!(resolve().await, Err(ProvisionError::Retryable(_))));
    // A plain 403 or 404 is not a transient condition.
    fixture
        .server
        .route("/latest", Route::status(403, "forbidden"));
    assert!(matches!(resolve().await, Err(ProvisionError::Failed(_))));
    fixture.server.route("/latest", Route::status(404, "none"));
    assert!(matches!(resolve().await, Err(ProvisionError::Failed(_))));
    fixture
        .server
        .route("/latest", Route::ok(b"not json".to_vec()));
    assert!(matches!(resolve().await, Err(ProvisionError::Failed(_))));
    // A release whose tag does not exist in the repository is a hard failure.
    fixture
        .server
        .route("/latest", Route::ok(br#"{"tag_name":"v7.7.7"}"#.to_vec()));
    assert!(matches!(resolve().await, Err(ProvisionError::Failed(_))));

    // An unreachable API is retryable.
    let dead = GitSource::with(
        Downloader::with_loopback_http(),
        Endpoints {
            comfyui_url: fixture.remote.url(),
            manager_url: fixture.remote.url(),
            releases_api: "http://127.0.0.1:1/latest".into(),
        },
        Some(locate_git()),
    );
    assert!(matches!(
        dead.resolve(&fixture.root, &ComfyRef::Latest).await,
        Err(ProvisionError::Retryable(_))
    ));
    // The production source refuses a non-HTTPS endpoint.
    let insecure = GitSource::with(
        Downloader::new(),
        Endpoints {
            comfyui_url: fixture.remote.url(),
            manager_url: fixture.remote.url(),
            releases_api: fixture.server.url("/latest"),
        },
        Some(locate_git()),
    );
    assert!(matches!(
        insecure.resolve(&fixture.root, &ComfyRef::Latest).await,
        Err(ProvisionError::Failed(_))
    ));
}

#[tokio::test]
async fn checkouts_are_inspected_without_being_altered() {
    let fixture = Fixture::new().await;
    let source = fixture.source();
    assert_eq!(
        source.inspect_checkout(&fixture.root).await.unwrap(),
        Checkout::Absent
    );
    let dir = fixture.root.join("ComfyUI");
    fs::create_dir_all(&dir).unwrap();
    assert_eq!(
        source.inspect_checkout(&fixture.root).await.unwrap(),
        Checkout::Absent,
        "an empty folder"
    );
    fs::write(dir.join("notes.txt"), b"user data").unwrap();
    assert_eq!(
        source.inspect_checkout(&fixture.root).await.unwrap(),
        Checkout::NotGit
    );
    assert_eq!(fs::read(dir.join("notes.txt")).unwrap(), b"user data");
    // A broken .git folder is not a checkout we may alter.
    fs::create_dir_all(dir.join(".git")).unwrap();
    assert_eq!(
        source.inspect_checkout(&fixture.root).await.unwrap(),
        Checkout::NotGit
    );
}

#[tokio::test]
async fn a_tag_is_cloned_staged_checked_out_and_published() {
    let fixture = Fixture::new().await;
    let source = fixture.source();
    let resolved = source
        .resolve(&fixture.root, &ComfyRef::Tag("v1.0.0".into()))
        .await
        .unwrap();
    source
        .clone_comfyui(&fixture.root, &resolved, &ctx())
        .await
        .unwrap();
    let dir = fixture.root.join("ComfyUI");
    assert_eq!(
        fs::read_to_string(dir.join("README.md")).unwrap(),
        "one",
        "the tagged commit is checked out"
    );
    assert_eq!(
        source.inspect_checkout(&fixture.root).await.unwrap(),
        Checkout::Repo {
            commit: fixture.remote.first.clone()
        }
    );
    let leftovers: Vec<_> = fs::read_dir(&fixture.root)
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(leftovers, ["ComfyUI"], "no staging directory remains");
    let files = source.requirement_files(&fixture.root).await.unwrap();
    assert_eq!(files.comfyui, dir.join("requirements.txt"));
    assert!(files.nodes.is_empty());
}

#[tokio::test]
async fn a_branch_clone_tracks_the_branch() {
    let fixture = Fixture::new().await;
    let source = fixture.source();
    let resolved = source
        .resolve(&fixture.root, &ComfyRef::Branch("dev".into()))
        .await
        .unwrap();
    source
        .clone_comfyui(&fixture.root, &resolved, &ctx())
        .await
        .unwrap();
    let dir = fixture.root.join("ComfyUI");
    assert_eq!(git(&dir, &["rev-parse", "--abbrev-ref", "HEAD"]), "dev");
    assert_eq!(
        git(&dir, &["rev-parse", "--abbrev-ref", "dev@{upstream}"]),
        "origin/dev"
    );
}

#[tokio::test]
async fn failed_and_cancelled_clones_leave_nothing_and_an_existing_folder_is_never_replaced() {
    let fixture = Fixture::new().await;
    let source = fixture.source();
    let resolved = source
        .resolve(&fixture.root, &ComfyRef::Tag("v1.0.0".into()))
        .await
        .unwrap();

    let broken = ResolvedGit {
        url: fixture
            .remote
            .path
            .join("does-not-exist")
            .to_string_lossy()
            .into_owned(),
        ..resolved.clone()
    };
    assert!(
        source
            .clone_comfyui(&fixture.root, &broken, &ctx())
            .await
            .is_err()
    );
    let names = |root: &Path| -> Vec<String> {
        fs::read_dir(root)
            .map(|entries| {
                entries
                    .flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default()
    };
    assert!(
        names(&fixture.root).is_empty(),
        "{:?}",
        names(&fixture.root)
    );

    let cancelled = ctx();
    cancelled.cancel.cancel();
    assert_eq!(
        source
            .clone_comfyui(&fixture.root, &resolved, &cancelled)
            .await
            .unwrap_err(),
        ProvisionError::Cancelled
    );
    assert!(
        names(&fixture.root).is_empty(),
        "{:?}",
        names(&fixture.root)
    );

    // A wrong pinned commit is detected and discarded.
    let wrong = ResolvedGit {
        commit: "0".repeat(40),
        ..resolved.clone()
    };
    assert!(
        source
            .clone_comfyui(&fixture.root, &wrong, &ctx())
            .await
            .is_err()
    );
    assert!(names(&fixture.root).is_empty());

    // An existing non-empty folder is left exactly as it was.
    let dir = fixture.root.join("ComfyUI");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("user.txt"), b"mine").unwrap();
    assert!(
        source
            .clone_comfyui(&fixture.root, &resolved, &ctx())
            .await
            .is_err()
    );
    assert_eq!(fs::read(dir.join("user.txt")).unwrap(), b"mine");
    assert_eq!(names(&fixture.root), ["ComfyUI"]);
}

#[tokio::test]
async fn requirement_files_cover_enabled_and_disabled_custom_nodes_only() {
    let fixture = Fixture::new().await;
    let source = fixture.source();
    let comfy = fixture.root.join("ComfyUI");
    fs::create_dir_all(comfy.join("custom_nodes/good node ไทย")).unwrap();
    fs::create_dir_all(comfy.join("custom_nodes/no-reqs")).unwrap();
    fs::create_dir_all(comfy.join("custom_nodes/.disabled/off")).unwrap();
    fs::create_dir_all(comfy.join("custom_nodes/.disabled/.vjump-staging-x")).unwrap();
    fs::write(
        comfy.join("custom_nodes/.disabled/.vjump-staging-x/requirements.txt"),
        "never installed",
    )
    .unwrap();
    fs::create_dir_all(comfy.join("custom_nodes/__pycache__")).unwrap();
    fs::create_dir_all(comfy.join("custom_nodes/another")).unwrap();
    fs::write(
        comfy.join("custom_nodes/good node ไทย/requirements.txt"),
        "x",
    )
    .unwrap();
    fs::write(comfy.join("custom_nodes/another/requirements.txt"), "y").unwrap();
    fs::write(
        comfy.join("custom_nodes/.disabled/off/requirements.txt"),
        "z",
    )
    .unwrap();
    fs::write(comfy.join("custom_nodes/loose_file.py"), "").unwrap();
    assert!(
        source.requirement_files(&fixture.root).await.is_err(),
        "ComfyUI requirements are mandatory"
    );
    fs::write(comfy.join("requirements.txt"), "numpy").unwrap();
    let files = source.requirement_files(&fixture.root).await.unwrap();
    let names: Vec<_> = files
        .nodes
        .iter()
        .map(|path| {
            path.parent()
                .unwrap()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert_eq!(names, ["off", "another", "good node ไทย"]);
}

#[tokio::test]
async fn the_manager_is_installed_at_the_approved_commit_and_existing_folders_are_respected() {
    let fixture = Fixture::new().await;
    let source = fixture.source();
    let resolved = source.resolve_manager(&fixture.root).await.unwrap();
    let files = source
        .install_manager(&fixture.root, &resolved, &ctx())
        .await
        .unwrap();
    let manager = fixture.root.join("ComfyUI/custom_nodes/ComfyUI-Manager");
    assert_eq!(files, [manager.join("requirements.txt")]);
    assert_eq!(git(&manager, &["rev-parse", "HEAD"]), resolved.commit);

    // The same approved commit is reused without cloning again.
    assert_eq!(
        source
            .install_manager(&fixture.root, &resolved, &ctx())
            .await
            .unwrap()
            .len(),
        1
    );
    // Another commit than the approved one is refused and nothing changes.
    let other = ResolvedGit {
        commit: fixture.remote.first.clone(),
        ..resolved.clone()
    };
    assert!(
        source
            .install_manager(&fixture.root, &other, &ctx())
            .await
            .is_err()
    );
    assert_eq!(git(&manager, &["rev-parse", "HEAD"]), resolved.commit);

    // A non-git folder is never touched.
    fs::remove_dir_all(&manager).unwrap();
    fs::create_dir_all(&manager).unwrap();
    fs::write(manager.join("custom.txt"), b"mine").unwrap();
    assert!(
        source
            .install_manager(&fixture.root, &resolved, &ctx())
            .await
            .is_err()
    );
    assert_eq!(fs::read(manager.join("custom.txt")).unwrap(), b"mine");
}
