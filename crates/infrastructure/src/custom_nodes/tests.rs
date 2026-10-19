//! Offline tests: a loopback registry, zip fixtures and local Git repositories created with the
//! `git` binary (skipped with a message when it is missing).
use super::{
    files::{LocalNodeFiles, Recycler, node_limits},
    git::GitNodes,
    registry::RegistryHttp,
    scan,
    sqlite::SqliteNodes,
    tree,
};
use crate::shared::{
    archive::ExtractLimits,
    download::Downloader,
    model_paths::pin_absolute,
    sqlite::Db,
    test_http::{Route, TestServer},
};
use application::{
    custom_nodes::*,
    environment::{NullSink, RunId, StepContext, StepId},
    shared::CancelToken,
};
use domain::{
    custom_nodes::*,
    environment::{EnvId, PinSet},
    shared::model_values::SafeFileName,
};
use std::{
    fs,
    io::{Cursor, Write},
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex},
};
use zip::write::SimpleFileOptions;

fn ctx() -> StepContext {
    StepContext {
        run_id: RunId::new("node-test").unwrap(),
        step: StepId::ComfyRequirements,
        sink: Arc::new(NullSink),
        cancel: CancelToken::new(),
        allow_source_builds: false,
        retained: Default::default(),
    }
}

fn locate_git() -> Option<PathBuf> {
    let output = Command::new("git").arg("--version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let path = std::env::var_os("PATH")?;
    let names: &[&str] = if cfg!(windows) {
        &["git.exe"]
    } else {
        &["git"]
    };
    std::env::split_paths(&path)
        .flat_map(|dir| names.iter().map(move |n| dir.join(n)))
        .find(|candidate| candidate.is_file())
}

macro_rules! require_git {
    () => {
        match locate_git() {
            Some(git) => git,
            None => {
                eprintln!("skipped: the git binary is not available");
                return;
            }
        }
    };
}

fn node_id(text: &str) -> NodeId {
    NodeId::new(text).unwrap()
}

#[tokio::test]
async fn registry_download_writes_the_opened_staging_file() {
    let fixture = Fixture::new().await;
    let paths = super::paths::NodePaths::open(&fixture.root, true).unwrap();
    let entry = paths.staging_zip(&node_id("opened-download"));
    let path = super::paths::entry_path(&entry);
    let moved = fixture._directory.path().join("opened.zip");
    let mut opened = entry.0.open(&entry.1, true, true).unwrap();
    fs::rename(&path, &moved).unwrap();
    fs::write(&path, b"external sentinel").unwrap();
    fixture
        .server
        .route("/opened.zip", Route::ok(b"archive bytes"));
    let artifact = fixture
        .files
        .registry
        .download(
            &fixture.server.url("/opened.zip"),
            &mut opened,
            &CancelToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(artifact.bytes, 13);
    assert_eq!(fs::read(moved).unwrap(), b"archive bytes");
    assert_eq!(fs::read(path).unwrap(), b"external sentinel");
}

#[tokio::test]
async fn registry_archive_replacement_is_not_extracted() {
    let fixture = Fixture::new().await;
    let operation = node_id("archive-substitution");
    let paths = super::paths::NodePaths::open(&fixture.root, true).unwrap();
    let entry = paths.staging_zip(&operation);
    let archive = super::paths::entry_path(&entry);
    let moved = fixture._directory.path().join("retained.zip");
    let replacement = zip_bytes(&[("__init__.py", b"foreign"), ("foreign.txt", b"sentinel")]);
    let expected = replacement.clone();
    fixture.registry_node(
        "original",
        "1.0.0",
        zip_bytes(&[("__init__.py", b"original")]),
    );
    let changed = archive.clone();
    super::files::after_archive_open(move || {
        fs::rename(&changed, moved).unwrap();
        fs::write(&changed, replacement).unwrap();
    });
    assert!(matches!(
        fixture
            .files
            .prepare(
                &fixture.root,
                &operation,
                &PrepareRequest::Registry {
                    id: "original".into(),
                    version: Some("1.0.0".into()),
                },
                None,
                &ctx()
            )
            .await,
        Err(NodeAppError::UnsafeSource)
    ));
    let staging = super::paths::entry_path(&paths.staging(&operation));
    assert_eq!(fs::read(staging.join("__init__.py")).unwrap(), b"original");
    assert!(!staging.join("foreign.txt").exists());
    assert_eq!(fs::read(archive).unwrap(), expected);
}

#[tokio::test]
async fn registry_create_new_refusal_preserves_existing_entries() {
    for occupied in 0..3 {
        let fixture = Fixture::new().await;
        let operation = node_id("occupied-download");
        let paths = super::paths::NodePaths::open(&fixture.root, true).unwrap();
        let entries = [
            paths.staging_zip(&operation),
            paths.staging(&operation),
            paths.staging_hoist(&operation),
        ];
        let path = super::paths::entry_path(&entries[occupied]);
        fs::write(&path, b"external sentinel").unwrap();
        fixture.registry_node("occupied", "1.0.0", Fixture::good_zip());
        assert!(matches!(
            fixture
                .files
                .prepare(
                    &fixture.root,
                    &operation,
                    &PrepareRequest::Registry {
                        id: "occupied".into(),
                        version: Some("1.0.0".into()),
                    },
                    None,
                    &ctx()
                )
                .await,
            Err(NodeAppError::UnsafeSource)
        ));
        assert_eq!(fs::read(path).unwrap(), b"external sentinel");
        assert_eq!(fixture.owned().len(), 1);
    }
}

#[tokio::test]
async fn registry_download_cancellation_cleans_only_owned_entries() {
    let fixture = Fixture::new().await;
    let operation = node_id("cancel-download");
    let paths = super::paths::NodePaths::open(&fixture.root, true).unwrap();
    let foreign = super::paths::entry_path(&paths.staging_zip(&node_id("foreign")));
    fs::write(&foreign, b"external sentinel").unwrap();
    fixture.registry_node("cancelled", "1.0.0", Fixture::good_zip());
    let step = ctx();
    let cancel = step.cancel.clone();
    super::files::after_archive_open(move || cancel.cancel());
    assert!(matches!(
        fixture
            .files
            .prepare(
                &fixture.root,
                &operation,
                &PrepareRequest::Registry {
                    id: "cancelled".into(),
                    version: Some("1.0.0".into()),
                },
                None,
                &step
            )
            .await,
        Err(NodeAppError::Cancelled)
    ));
    assert_eq!(fs::read(foreign).unwrap(), b"external sentinel");
    assert_eq!(fixture.owned().len(), 1);
}

#[test]
fn tree_reads_use_pinned_nonfollowing_handles() {
    let temp = tempfile::tempdir().unwrap();
    let node = temp.path().join("node");
    let outside = temp.path().join("outside");
    let saved = temp.path().join("saved");
    fs::create_dir(&node).unwrap();
    fs::create_dir(&outside).unwrap();
    fs::write(node.join("payload.py"), b"reviewed").unwrap();
    fs::write(outside.join("payload.py"), b"outside bytes").unwrap();
    let pinned = pin_absolute(&node).unwrap();
    let before = tree::tree_digest(&pinned).unwrap();
    let (swap_node, swap_saved, swap_outside) = (node.clone(), saved.clone(), outside.clone());
    tree::before_read(move || match fs::rename(&swap_node, &swap_saved) {
        Ok(()) => assert!(directory_link(&swap_outside, &swap_node)),
        Err(error) => {
            #[cfg(windows)]
            assert_sharing_refusal(&error);
            #[cfg(unix)]
            panic!("Unix parent substitution must execute: {error}");
        }
    });
    assert_eq!(tree::tree_digest(&pinned).unwrap(), before);
    assert_eq!(
        fs::read(outside.join("payload.py")).unwrap(),
        b"outside bytes"
    );
    drop(pinned);
    if saved.exists() {
        remove_directory_link(&node);
        fs::rename(&saved, &node).unwrap();
    }
    // Replace the leaf between enumeration and opening it. Junctions exercise the
    // nonfollowing check on Windows without requiring file-symlink privileges.
    for helper in ["digest", "text", "hash"] {
        let pinned = pin_absolute(&node).unwrap();
        let (leaf, outside) = (node.join("payload.py"), outside.clone());
        tree::before_read(move || {
            fs::remove_file(&leaf).unwrap();
            assert!(directory_link(&outside, &leaf));
        });
        let name = SafeFileName::new("payload.py").unwrap();
        match helper {
            "digest" => assert_eq!(tree::tree_digest(&pinned), Err(NodeAppError::UnsafeSource)),
            "text" => assert!(tree::read_text(&pinned, &name, 1024).is_none()),
            "hash" => assert_eq!(
                super::paths::filesystem(tree::sha256_file(&pinned, &name).unwrap_err()),
                NodeAppError::UnsafeSource
            ),
            _ => unreachable!(),
        }
        remove_directory_link(&node.join("payload.py"));
        fs::write(node.join("payload.py"), b"reviewed").unwrap();
    }
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn single_file_fifo_is_refused_without_blocking() {
    use crate::shared::gate::TokioMaintenanceGate;
    use application::shared::MaintenanceGate;
    use std::{os::unix::ffi::OsStrExt, time::Duration};
    let fixture = Fixture::new().await;
    let mut single = node("fifo.py", NodeSource::Manual, true);
    single.single_file = true;
    let path = std::ffi::CString::new(fixture.cn().join("fifo.py").as_os_str().as_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
    let gate = TokioMaintenanceGate::new();
    let lease = gate.acquire().await.unwrap();
    let (sender, receiver) = std::sync::mpsc::channel();
    let root = fixture.root.clone();
    let worker = std::thread::spawn(move || {
        let result = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(
                fixture
                    .files
                    .snapshot(&root, &single, None, Default::default()),
            );
        drop(lease);
        sender.send(result).unwrap();
    });
    assert_eq!(
        receiver
            .recv_timeout(Duration::from_secs(2))
            .expect("FIFO read must return promptly")
            .unwrap_err(),
        NodeAppError::UnsafeSource
    );
    worker.join().unwrap();
    assert!(gate.try_acquire().await.is_ok());
}

fn zip_bytes(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in entries {
        if name.ends_with('/') {
            writer.add_directory(*name, stored).unwrap();
        } else {
            writer.start_file(*name, stored).unwrap();
            writer.write_all(data).unwrap();
        }
    }
    writer.finish().unwrap().into_inner()
}

fn check_archive_plan(entries: &[(&str, &[u8])], accepted: bool) {
    let temp = tempfile::tempdir().unwrap();
    let archive = temp.path().join("archive.zip");
    fs::write(&archive, zip_bytes(entries)).unwrap();
    let parent = pin_absolute(temp.path()).unwrap();
    let staging = parent
        .create_new_child(&SafeFileName::new("staging").unwrap())
        .unwrap();
    let result = tree::extract_zip(fs::File::open(archive).unwrap(), &staging, node_limits());
    if accepted {
        result.unwrap();
    } else {
        assert_eq!(result, Err(NodeAppError::UnsafeSource));
        assert_eq!(fs::read_dir(staging.anchored_path()).unwrap().count(), 0);
    }
}

#[test]
fn registry_archive_depth_is_rejected_before_writing() {
    let name = format!("{}leaf.py", "d/".repeat(64));
    check_archive_plan(
        &[("first.py", b"must not be written"), (&name, b"x")],
        false,
    );
}

#[test]
fn node_path_limits_accept_boundaries_and_reject_excess() {
    for (count, accepted) in [(64, true), (65, false)] {
        let names = vec!["d".to_owned(); count];
        assert_eq!(tree::validate_components(&names).is_ok(), accepted);
        check_archive_plan(&[(&names.join("/"), b"x")], accepted);
    }
    for (bytes, accepted) in [(255, true), (256, false)] {
        // UTF-8 bytes, rather than character count, are limited.
        let name = format!("{}{}", "é".repeat(bytes / 2), "a".repeat(bytes % 2));
        assert_eq!(
            tree::validate_components(std::slice::from_ref(&name)).is_ok(),
            accepted
        );
        check_archive_plan(&[(&name, b"x")], accepted);
    }
    for (bytes, accepted) in [(1024, true), (1025, false)] {
        let names = vec![
            "a".repeat(255),
            "b".repeat(255),
            "c".repeat(255),
            "d".repeat(254),
            "e".repeat(bytes - 1023),
        ];
        assert_eq!(names.iter().map(String::len).sum::<usize>() + 4, bytes);
        assert_eq!(tree::validate_components(&names).is_ok(), accepted);
        check_archive_plan(&[(&names.join("/"), b"x")], accepted);
    }
    let name = format!("{}x", "./".repeat(512));
    check_archive_plan(&[(&name, b"x")], false);
}

#[test]
fn archive_planning_has_a_total_name_budget() {
    let names = (0..9_300)
        .map(|index| format!("{}{index:04}.py", "./".repeat(450)))
        .collect::<Vec<_>>();
    assert!(names.iter().map(String::len).sum::<usize>() > tree::MAX_ARCHIVE_PLANNED_NAME_BYTES);
    assert!(
        names
            .iter()
            .all(|name| name.len() <= tree::MAX_ARCHIVE_NAME_BYTES)
    );
    let entries = names
        .iter()
        .map(|name| (name.as_str(), &b""[..]))
        .collect::<Vec<_>>();
    check_archive_plan(&entries[..2], true);
    check_archive_plan(&entries, false);
}

#[test]
fn archive_component_trie_rejects_case_and_kind_collisions() {
    check_archive_plan(&[("a/x.py", b"x"), ("a/y.py", b"y"), ("a/", b"")], true);
    for names in [
        vec!["a/x.py", "A/y.py"],
        vec!["a/", "./a/"],
        vec!["a.py", "./a.py"],
        vec!["a", "a/x.py"],
        vec!["a/x.py", "a"],
        vec!["a/", "a"],
    ] {
        let entries = names
            .iter()
            .map(|name| (*name, &b""[..]))
            .collect::<Vec<_>>();
        check_archive_plan(&entries, false);
    }
}

#[test]
fn registry_archives_reject_git_and_shipped_pycache() {
    for name in [
        ".git/config",
        "pack/.GiT/HEAD",
        "__pycache__/x.pyc",
        "pack/__PYCACHE__/x.pyc",
    ] {
        check_archive_plan(&[("first.py", b"x"), (name, b"forbidden")], false);
    }
}

#[test]
fn git_metadata_and_installed_caches_are_validated_before_exclusion() {
    let temp = tempfile::tempdir().unwrap();
    let parent = pin_absolute(temp.path()).unwrap();
    let directory = parent
        .create_new_child(&SafeFileName::new("node").unwrap())
        .unwrap();
    let node = directory.anchored_path();
    fs::create_dir(node.join(".git")).unwrap();
    assert_eq!(
        tree::validate_tree(&directory, false),
        Err(NodeAppError::UnsafeSource)
    );
    tree::validate_tree(&directory, true).unwrap();
    fs::create_dir_all(node.join("sub/.git")).unwrap();
    assert_eq!(
        tree::validate_tree(&directory, true),
        Err(NodeAppError::UnsafeSource)
    );
    fs::remove_dir_all(node.join("sub")).unwrap();
    fs::remove_dir(node.join(".git")).unwrap();
    fs::write(node.join(".git"), b"gitdir: outside").unwrap();
    assert_eq!(
        tree::validate_tree(&directory, true),
        Err(NodeAppError::UnsafeSource)
    );
    fs::remove_file(node.join(".git")).unwrap();
    let outside = temp.path().join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("sentinel"), b"external").unwrap();
    assert!(directory_link(&outside, &node.join(".git")));
    assert_eq!(
        tree::validate_tree(&directory, true),
        Err(NodeAppError::UnsafeSource)
    );
    remove_directory_link(&node.join(".git"));
    fs::create_dir(node.join("__pycache__")).unwrap();
    assert!(directory_link(
        &outside,
        &node.join("__pycache__").join("linked")
    ));
    assert_eq!(
        tree::tree_digest(&directory),
        Err(NodeAppError::UnsafeSource)
    );
    remove_directory_link(&node.join("__pycache__/linked"));
    assert_eq!(fs::read(outside.join("sentinel")).unwrap(), b"external");
}

#[test]
fn digest_exclusions_use_only_node_relative_components() {
    let temp = tempfile::tempdir().unwrap();
    let node = temp.path().join("__pycache__/node");
    fs::create_dir_all(node.join("sub/__pycache__")).unwrap();
    fs::write(node.join("main.py"), b"one").unwrap();
    fs::write(node.join("sub/__pycache__/cache.pyc"), b"cache one").unwrap();
    let pinned = pin_absolute(&node).unwrap();
    let before = tree::tree_digest(&pinned).unwrap();
    fs::write(node.join("main.py"), b"two").unwrap();
    let changed = tree::tree_digest(&pinned).unwrap();
    assert_ne!(before, changed);
    fs::write(node.join("sub/__pycache__/cache.pyc"), b"cache two").unwrap();
    assert_eq!(changed, tree::tree_digest(&pinned).unwrap());
}

#[test]
fn staging_create_new_refusal_preserves_foreign_directory() {
    let temp = tempfile::tempdir().unwrap();
    let parent = pin_absolute(temp.path()).unwrap();
    let name = SafeFileName::new("staging").unwrap();
    let path = temp.path().join(name.as_str());
    let foreign = path.clone();
    super::files::before_staging_create(move || {
        fs::create_dir(&foreign).unwrap();
        fs::write(foreign.join("sentinel"), b"foreign").unwrap();
    });
    assert!(matches!(
        super::files::create_staging(&(parent.clone(), name.clone())),
        Err(NodeAppError::UnsafeSource)
    ));
    assert_eq!(fs::read(path.join("sentinel")).unwrap(), b"foreign");
    assert_eq!(
        parent.create_new_child(&name).err().unwrap().kind(),
        std::io::ErrorKind::AlreadyExists
    );
}

/// Creates a legacy tree without retaining an unbounded chain of directory pins.
fn deep_tree(root: &Path, directories: usize) -> PathBuf {
    let mut leaf = root.to_path_buf();
    for _ in 0..directories {
        fs::create_dir(&leaf).unwrap();
        leaf.push("d");
    }
    leaf.pop();
    fs::write(leaf.join("sentinel"), b"deep contents").unwrap();
    leaf
}

#[test]
fn owned_tree_cleanup_uses_a_bounded_stack() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().to_owned();
    deep_tree(&path.join("owned"), 64);
    fs::write(path.join("outside"), b"external sentinel").unwrap();
    std::thread::Builder::new()
        .stack_size(128 * 1024)
        .spawn(move || {
            let parent = pin_absolute(&path).unwrap();
            parent
                .remove_owned_tree(&SafeFileName::new("owned").unwrap())
                .unwrap();
            assert!(!path.join("owned").exists());
            assert_eq!(
                fs::read(path.join("outside")).unwrap(),
                b"external sentinel"
            );
        })
        .unwrap()
        .join()
        .unwrap();
}

#[tokio::test]
async fn legacy_overdeep_staging_is_preserved_without_startup_crash() {
    let fixture = Fixture::new().await;
    let paths = super::paths::NodePaths::open(&fixture.root, true).unwrap();
    let op = operation(
        OperationKind::Install,
        "legacy-depth",
        node("pack", registry_source("pack", "1"), true),
        None,
        OperationPhase::Prepared,
    );
    let staging = super::paths::entry_path(&paths.staging(&op.id));
    let leaf = deep_tree(&staging, 65);
    fixture.store.save_operation(&op).await.unwrap();
    for _ in 0..2 {
        assert_eq!(
            fixture
                .files
                .recover(&fixture.root, &op, Default::default())
                .await
                .unwrap_err(),
            NodeAppError::UnsafeSource
        );
        assert_eq!(fs::read(leaf.join("sentinel")).unwrap(), b"deep contents");
        assert_eq!(
            fixture.store.operations().await.unwrap()[0].phase,
            OperationPhase::Prepared
        );
    }
}

#[tokio::test]
async fn git_tree_depth_is_rejected_without_recursive_cleanup() {
    let git = locate_git().expect("Git must execute for H1 regression evidence");
    let g = git_fixture(git).await;
    let name = format!("{}leaf.py", "d/".repeat(64));
    g.repo.commit(&name, "tracked deep contents", "deep tree");
    let id = node_id("deep-git");
    assert_eq!(
        g.fixture
            .files
            .prepare(&g.fixture.root, &id, &g.request(None), None, &ctx())
            .await
            .unwrap_err(),
        NodeAppError::UnsafeSource
    );
    let paths = super::paths::NodePaths::open(&g.fixture.root, true).unwrap();
    let staging = paths.staging(&id);
    assert_eq!(
        fs::read(super::paths::entry_path(&staging).join(&name)).unwrap(),
        b"tracked deep contents"
    );
    let result = std::thread::Builder::new()
        .stack_size(128 * 1024)
        .spawn(move || staging.0.remove_owned_tree(&staging.1))
        .unwrap()
        .join()
        .unwrap();
    assert_eq!(
        result.unwrap_err().kind(),
        std::io::ErrorKind::PermissionDenied
    );
}

#[tokio::test]
async fn fresh_git_checkouts_reject_shipped_pycache() {
    let git = locate_git().expect("Git must execute for H1 regression evidence");
    for update in [false, true] {
        for cache in ["__pycache__", "__PYCACHE__"] {
            let g = git_fixture(git.clone()).await;
            let installed = if update {
                Some(g.install("before-cache", None).await)
            } else {
                None
            };
            let cache_path = format!("sub/{cache}/tracked.pyc");
            g.repo.commit(&cache_path, "tracked cache", "ship cache");
            let id = node_id("cache-stage");
            let request = match &installed {
                Some(node) => PrepareRequest::Update {
                    node: node.id.clone(),
                    reference: None,
                },
                None => g.request(None),
            };
            assert_eq!(
                g.fixture
                    .files
                    .prepare(&g.fixture.root, &id, &request, installed.as_ref(), &ctx())
                    .await
                    .unwrap_err(),
                NodeAppError::UnsafeSource
            );
            let staging = g.fixture.disabled().join(".vjump-staging-cache-stage");
            assert_eq!(
                fs::read(staging.join(&cache_path)).unwrap(),
                b"tracked cache"
            );
            let checkout = Repo {
                dir: staging,
                git: g.repo.git.clone(),
            };
            assert!(
                checkout.run(&["status", "--porcelain"]).is_empty(),
                "refusal must preserve the tracked checkout"
            );
        }
    }
}

struct Fixture {
    _directory: tempfile::TempDir,
    root: PathBuf,
    server: TestServer,
    store: Arc<SqliteNodes>,
    files: LocalNodeFiles,
    recycled: Arc<Mutex<Vec<PathBuf>>>,
}

impl Fixture {
    async fn new() -> Self {
        Self::with(None, node_limits()).await
    }

    async fn with(git: Option<PathBuf>, limits: ExtractLimits) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("install root ไทย");
        fs::create_dir_all(root.join("ComfyUI/custom_nodes")).unwrap();
        fs::create_dir_all(root.join("data")).unwrap();
        let db = Db::open(root.join("data/app.db")).await.unwrap();
        let store = Arc::new(SqliteNodes { db });
        let server = TestServer::start().await;
        let registry = Arc::new(RegistryHttp::new(
            Downloader::with_loopback_http(),
            server.base_url.clone(),
            store.clone(),
            Arc::new(|| super::compat::Host {
                os: HostOs::Windows,
                cuda: true,
                comfyui: super::compat::parse_version("0.3.50"),
            }),
        ));
        let recycled: Arc<Mutex<Vec<PathBuf>>> = Arc::default();
        let log = recycled.clone();
        let recycler: Recycler = Arc::new(move |parent, name, expected| {
            let file = parent.open_entry(name)?;
            if crate::shared::model_paths::identity(&file)? != *expected {
                return Err(std::io::ErrorKind::PermissionDenied.into());
            }
            drop(file);
            log.lock().unwrap().push(parent.path.join(name.as_str()));
            parent.remove_owned_tree(name)
        });
        let git = Arc::new(match git {
            Some(program) => GitNodes::with_program(program, true),
            None => GitNodes::new(),
        });
        let files = LocalNodeFiles {
            registry,
            git,
            recycler,
            limits,
        };
        Self {
            _directory: directory,
            root,
            server,
            store,
            files,
            recycled,
        }
    }

    fn cn(&self) -> PathBuf {
        self.root.join("ComfyUI/custom_nodes")
    }

    fn disabled(&self) -> PathBuf {
        self.cn().join(".disabled")
    }

    /// Everything the app wrote under `.disabled` for operations.
    fn owned(&self) -> Vec<String> {
        fs::read_dir(self.disabled())
            .map(|entries| {
                entries
                    .flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .filter(|n| n.starts_with(".vjump-staging") || n.starts_with(".vjump-backup"))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn registry_node(&self, id: &str, version: &str, body: Vec<u8>) {
        let url = format!("{}/dl/{id}-{version}.zip", self.server.base_url);
        let release = serde_json::json!({
            "node_id": id, "version": version, "downloadUrl": url,
            "dependencies": ["requests>=2", "pillow"],
            "supported_os": ["Microsoft :: Windows"], "supported_accelerators": ["GPU :: NVIDIA CUDA"],
            "supported_comfyui_version": ">=0.3.0", "deprecated": false,
        });
        self.server.route(
            &format!("/nodes/{id}/install?version={version}"),
            Route::ok(release.to_string()),
        );
        self.server
            .route(&format!("/dl/{id}-{version}.zip"), Route::ok(body));
        self.server.route(
            &format!("/nodes/{id}"),
            Route::ok(
                serde_json::json!({
                    "id": id, "name": id, "repository": "https://github.com/pub/repo",
                    "publisher": {"id": "pub"},
                    "latest_version": {
                        "version": version,
                        "supported_os": ["Microsoft :: Windows"],
                        "supported_accelerators": ["GPU :: NVIDIA CUDA"],
                        "supported_comfyui_version": ">=0.3.0",
                    }
                })
                .to_string(),
            ),
        );
    }

    fn latest(&self, id: &str, version: &str) {
        // The unpinned latest request answers with the same release as the exact one.
        let url = format!("{}/dl/{id}-{version}.zip", self.server.base_url);
        self.server.route(
            &format!("/nodes/{id}/install"),
            Route::ok(
                serde_json::json!({"node_id": id, "version": version, "downloadUrl": url, "dependencies": []})
                    .to_string(),
            ),
        );
    }

    fn good_zip() -> Vec<u8> {
        zip_bytes(&[
            ("__init__.py", b"NODE_CLASS_MAPPINGS = {}"),
            ("requirements.txt", b"# comment\nnumpy>=1.0\n\nrequests"),
            ("install.py", b"raise SystemExit('never executed in tests')"),
            ("nodes/", b""),
            ("nodes/a.py", b"x = 1"),
        ])
    }
}

fn node(name: &str, source: NodeSource, enabled: bool) -> InstalledNode {
    InstalledNode {
        id: node_id(&format!(
            "n-{}",
            &crate::shared::download::sha256_hex(name.as_bytes())[..12]
        )),
        name: SafeFileName::new(name).unwrap(),
        source,
        origin: ProvenanceOrigin::App,
        enabled,
        single_file: false,
        tree_digest: String::new(),
        dependencies: DependencyState::NotRun,
        conflict: false,
        dirty: None,
    }
}

fn operation(
    kind: OperationKind,
    id: &str,
    node: InstalledNode,
    prior: Option<InstalledNode>,
    phase: OperationPhase,
) -> NodeOperation {
    NodeOperation {
        id: node_id(id),
        kind,
        phase,
        node,
        prior,
        environment: EnvId::new("env-1").unwrap(),
        pins: PinSet::new(),
        binding: "binding".into(),
        artifact_digest: None,
        requirements_digest: None,
        hook_digest: None,
        consent_identity: None,
        failure: None,
        staging: format!(".disabled/.vjump-staging-{id}"),
        backup: format!(".disabled/.vjump-backup-{id}"),
        dependency_requirements: Vec::new(),
    }
}

fn registry_source(id: &str, version: &str) -> NodeSource {
    NodeSource::Registry {
        id: id.into(),
        publisher: Some("pub".into()),
        version: version.into(),
        repository: Some("https://github.com/pub/repo".into()),
    }
}

// ---------------------------------------------------------------------------------------
// Registry browse, search and cache
// ---------------------------------------------------------------------------------------

fn page_body(ids: &[&str], page: u32, total: u32) -> String {
    serde_json::json!({
        "nodes": ids.iter().map(|id| serde_json::json!({
            "id": id, "name": format!("Node {id}"), "downloads": 7,
            "publisher": {"id": "pub"}, "repository": "https://github.com/pub/r",
            "latest_version": {"version": "1.2.3", "supported_os": ["OS Independent"], "supported_comfyui_version": ">=9.0"},
        })).collect::<Vec<_>>(),
        "page": page, "limit": 2, "total": total, "totalPages": total.div_ceil(2),
    })
    .to_string()
}

#[tokio::test]
async fn browse_search_pagination_and_cache_keys() {
    let fixture = Fixture::new().await;
    let registry = &fixture.files.registry;
    fixture.server.route(
        "/nodes?page=1&limit=2",
        Route::ok(page_body(&["a", "b"], 1, 5)),
    );
    fixture.server.route(
        "/nodes?page=2&limit=2",
        Route::ok(page_body(&["c", "d"], 2, 5)),
    );
    fixture.server.route(
        "/nodes/search?search=image%20tools&page=1&limit=2",
        Route::ok(page_body(&["img"], 1, 1)),
    );
    let query = |search: &str, page| RegistryQuery {
        search: search.into(),
        page,
        limit: 2,
    };
    let first = registry.search(query("", 1), false, 3600).await.unwrap();
    assert_eq!(first.page.nodes.len(), 2);
    assert_eq!(
        (first.page.total, first.page.total_pages, first.page.page),
        (5, 3, 1)
    );
    assert!(!first.stale);
    let node = &first.page.nodes[0];
    assert_eq!(node.publisher.as_deref(), Some("pub"));
    assert_eq!(node.github_stars, None, "missing stars stay unknown");
    assert_eq!(node.evaluated.os, Support::Compatible);
    assert_eq!(node.evaluated.comfyui, Support::Incompatible);
    assert_eq!(node.evaluated.accelerator, Support::Unknown);
    let requests = fixture.server.requests.lock().unwrap().len();
    // A fresh cache entry answers without a request, whatever the whitespace and case.
    registry.search(query("", 1), false, 3600).await.unwrap();
    assert_eq!(fixture.server.requests.lock().unwrap().len(), requests);
    // Another page is another cache key.
    let second = registry.search(query("", 2), false, 3600).await.unwrap();
    assert_eq!(second.page.nodes[0].id, "c");
    assert_eq!(fixture.server.requests.lock().unwrap().len(), requests + 1);
    // The search endpoint with a normalized query.
    let found = registry
        .search(query("  Image   TOOLS ", 1), false, 3600)
        .await
        .unwrap();
    assert_eq!(found.page.nodes[0].id, "img");
    let before = fixture.server.requests.lock().unwrap().len();
    registry
        .search(query("image tools", 1), false, 3600)
        .await
        .unwrap();
    assert_eq!(fixture.server.requests.lock().unwrap().len(), before);
    // Manual refresh bypasses the TTL.
    registry.search(query("", 1), true, 3600).await.unwrap();
    assert_eq!(fixture.server.requests.lock().unwrap().len(), before + 1);
}

#[tokio::test]
async fn browse_cache_preserves_top_level_flags_separately_from_latest_release_flags() {
    let fixture = Fixture::new().await;
    fixture.server.route(
        "/nodes?page=1&limit=2",
        Route::ok(
            serde_json::json!({"nodes": [
                {
                    "id": "node-flags", "status": "NodeStatusFlagged", "deprecated": true,
                    "latest_version": {
                        "version": "9.0.0", "status": "NodeVersionStatusActive",
                        "deprecated": false,
                    },
                },
                {
                    "id": "latest-flags", "status": "NodeStatusActive", "deprecated": false,
                    "latest_version": {
                        "version": "9.0.0", "status": "NodeVersionStatusFlagged",
                        "deprecated": true,
                    },
                },
            ]})
            .to_string(),
        ),
    );
    let query = RegistryQuery {
        search: String::new(),
        page: 1,
        limit: 2,
    };
    let registry = &fixture.files.registry;
    let fresh = registry.search(query.clone(), false, 3600).await.unwrap();
    let nodes = &fresh.page.nodes;
    assert!(
        nodes
            .iter()
            .all(|n| n.evaluated.flagged && n.evaluated.deprecated)
    );
    assert!(nodes[0].node_flagged && nodes[0].node_deprecated);
    assert!(!nodes[1].node_flagged && !nodes[1].node_deprecated);
    let cached = registry.search(query, false, 3600).await.unwrap();
    assert_eq!(cached.page, fresh.page);
    assert_eq!(fixture.server.requests.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn offline_results_are_stale_and_refusals_are_failures() {
    let fixture = Fixture::new().await;
    let registry = &fixture.files.registry;
    let query = RegistryQuery {
        search: String::new(),
        page: 1,
        limit: 2,
    };
    fixture
        .server
        .route("/nodes?page=1&limit=2", Route::ok(page_body(&["a"], 1, 1)));
    registry.search(query.clone(), false, 3600).await.unwrap();
    // The service becomes unavailable: the cached page is returned and flagged stale.
    fixture
        .server
        .route("/nodes?page=1&limit=2", Route::status(503, "down"));
    let stale = registry.search(query.clone(), true, 3600).await.unwrap();
    assert!(stale.stale);
    assert_eq!(stale.page.nodes[0].id, "a");
    // A refusal is a failure, not an overridable warning, and does not overwrite the cache.
    fixture
        .server
        .route("/nodes?page=1&limit=2", Route::status(403, "no"));
    assert_eq!(
        registry
            .search(query.clone(), true, 3600)
            .await
            .unwrap_err(),
        NodeAppError::RegistryStatus(403)
    );
    fixture
        .server
        .route("/nodes?page=1&limit=2", Route::status(503, "down"));
    assert_eq!(
        registry
            .search(query.clone(), true, 3600)
            .await
            .unwrap()
            .page
            .nodes[0]
            .id,
        "a"
    );
    // Nothing cached and nothing reachable.
    fixture
        .server
        .route("/nodes?page=9&limit=2", Route::status(503, "down"));
    let other = RegistryQuery {
        search: String::new(),
        page: 9,
        limit: 2,
    };
    assert_eq!(
        registry.search(other, false, 3600).await.unwrap_err(),
        NodeAppError::Offline
    );
    // Garbage and invalid requests.
    fixture
        .server
        .route("/nodes?page=3&limit=2", Route::ok("{\"unexpected\": true}"));
    let garbage = RegistryQuery {
        search: String::new(),
        page: 3,
        limit: 2,
    };
    assert_eq!(
        registry.search(garbage, true, 3600).await.unwrap_err(),
        NodeAppError::InvalidRegistry
    );
    let zero = RegistryQuery {
        search: String::new(),
        page: 0,
        limit: 2,
    };
    assert_eq!(
        registry.search(zero, true, 3600).await.unwrap_err(),
        NodeAppError::InvalidRequest
    );
}

#[tokio::test]
async fn exact_version_resolution_rejects_identity_and_version_mismatches() {
    let fixture = Fixture::new().await;
    let registry = &fixture.files.registry;
    fixture.server.route(
        "/nodes/n/install",
        Route::ok(r#"{"node_id":"n","version":"2.0.0","downloadUrl":"https://cdn.example/n.zip","dependencies":["x"]}"#),
    );
    let latest = registry.release("n", None).await.unwrap();
    assert_eq!(latest.release.version, "2.0.0");
    assert_eq!(latest.release.dependencies, ["x"]);
    // A response for another node or another version is not accepted.
    fixture.server.route(
        "/nodes/n/install?version=2.0.0",
        Route::ok(
            r#"{"node_id":"other","version":"2.0.0","downloadUrl":"https://cdn.example/n.zip"}"#,
        ),
    );
    assert_eq!(
        registry.release("n", Some("2.0.0")).await.unwrap_err(),
        NodeAppError::InvalidRegistry
    );
    fixture.server.route(
        "/nodes/n/install?version=2.0.0",
        Route::ok(r#"{"node_id":"n","version":"2.0.1","downloadUrl":"https://cdn.example/n.zip"}"#),
    );
    assert_eq!(
        registry.release("n", Some("2.0.0")).await.unwrap_err(),
        NodeAppError::InvalidRegistry
    );
    // Identifiers cannot smuggle path or query syntax into a request.
    for bad in ["../x", "a/b", "a?b", "", "a b"] {
        assert_eq!(
            registry.release(bad, None).await.unwrap_err(),
            NodeAppError::InvalidRequest,
            "{bad}"
        );
    }
}

// ---------------------------------------------------------------------------------------
// Registry install, update and hardened extraction
// ---------------------------------------------------------------------------------------

#[tokio::test]
async fn a_registry_release_is_staged_published_and_recorded_with_provenance() {
    let fixture = Fixture::new().await;
    fixture.latest("good-node", "1.0.0");
    fixture.registry_node("good-node", "1.0.0", Fixture::good_zip());
    let op = node_id("node-op1");
    let request = PrepareRequest::Registry {
        id: "good-node".into(),
        version: None,
    };
    let staged = fixture
        .files
        .prepare(&fixture.root, &op, &request, None, &ctx())
        .await
        .unwrap();
    assert_eq!(staged.name.as_str(), "good-node");
    assert_eq!(staged.source, registry_source("good-node", "1.0.0"));
    assert_eq!(staged.dependencies, ["requests>=2", "pillow"]);
    assert_eq!(staged.requirement_lines, ["numpy>=1.0", "requests"]);
    assert!(staged.hook_digest.is_some() && staged.requirements_digest.is_some());
    assert_eq!(staged.artifact_digest.as_ref().unwrap().len(), 64);
    assert!(staged.warnings.is_empty(), "{:?}", staged.warnings);
    // Staging lives under .disabled; the node is not loadable yet.
    let staging = fixture.disabled().join(".vjump-staging-node-op1");
    assert!(staging.join("__init__.py").is_file() && staging.join(".vjump-node.json").is_file());
    assert!(staging.join(".tracking").is_file());
    assert!(!fixture.cn().join("good-node").exists());
    // "Latest" was resolved once and then requested by exact version.
    let requests = fixture.server.requests.lock().unwrap().clone();
    assert!(
        requests
            .iter()
            .any(|r| r.starts_with("GET /nodes/good-node/install HTTP"))
    );
    assert!(
        requests
            .iter()
            .any(|r| r.starts_with("GET /nodes/good-node/install?version=1.0.0 HTTP"))
    );
    // The consent snapshot matches what is on disk, and publication is a no-replace move.
    let mut installed = node("good-node", staged.source.clone(), true);
    installed.tree_digest = staged.tree_digest.clone();
    let snapshot = fixture
        .files
        .snapshot(&fixture.root, &installed, Some(&op), Default::default())
        .await
        .unwrap();
    assert_eq!(snapshot.digest, staged.tree_digest);
    assert_eq!(snapshot.hook_digest, staged.hook_digest);
    let operation = operation(
        OperationKind::Install,
        "node-op1",
        installed.clone(),
        None,
        OperationPhase::PublishIntent,
    );
    fixture
        .files
        .publish(&fixture.root, &operation, Default::default())
        .await
        .unwrap();
    assert!(fixture.cn().join("good-node/__init__.py").is_file());
    assert!(!staging.exists() && fixture.owned().is_empty());
    let scanned = fixture
        .files
        .scan(&fixture.root, Default::default())
        .await
        .unwrap();
    assert_eq!(scanned.len(), 1);
    assert_eq!(
        scanned[0].source, installed.source,
        "the marker survives a database loss"
    );
    // The same folder is never overwritten.
    fixture.registry_node("good-node", "1.0.0", Fixture::good_zip());
    let again = fixture
        .files
        .prepare(&fixture.root, &node_id("node-op2"), &request, None, &ctx())
        .await;
    assert_eq!(again.unwrap_err(), NodeAppError::TargetExists);
    assert!(fixture.owned().is_empty());
}

#[tokio::test]
async fn incompatible_registry_metadata_becomes_review_warnings() {
    let fixture = Fixture::new().await;
    let release = serde_json::json!({
        "node_id": "picky", "version": "1.0.0",
        "downloadUrl": format!("{}/dl/picky.zip", fixture.server.base_url),
        "supported_os": ["POSIX :: Linux"], "supported_comfyui_version": ">=99.0", "deprecated": true,
        "status": "NodeVersionStatusFlagged",
    });
    fixture
        .server
        .route("/nodes/picky/install", Route::ok(release.to_string()));
    fixture.server.route(
        "/nodes/picky/install?version=1.0.0",
        Route::ok(release.to_string()),
    );
    fixture
        .server
        .route("/dl/picky.zip", Route::ok(Fixture::good_zip()));
    fixture.server.route(
        "/nodes/picky",
        Route::ok(serde_json::json!({"id": "picky", "name": "picky"}).to_string()),
    );
    let request = PrepareRequest::Registry {
        id: "picky".into(),
        version: None,
    };
    let staged = fixture
        .files
        .prepare(&fixture.root, &node_id("node-warn"), &request, None, &ctx())
        .await
        .unwrap();
    let text = staged.warnings.join("|");
    for needle in [
        "OS is incompatible",
        "ComfyUI is incompatible",
        "deprecated",
        "flagged",
    ] {
        assert!(text.contains(needle), "{needle} missing in {text}");
    }
    assert!(
        text.contains("accelerator compatibility is unknown"),
        "{text}"
    );
}

#[tokio::test]
async fn registry_detail_failures_abort_preparation() {
    for (route, expected) in [
        (
            Route::status(403, "refused"),
            NodeAppError::RegistryStatus(403),
        ),
        (
            Route::status(404, "missing"),
            NodeAppError::RegistryStatus(404),
        ),
        (Route::status(500, "unavailable"), NodeAppError::Offline),
        (Route::status(429, "limited"), NodeAppError::Offline),
        (Route::ok("not json"), NodeAppError::InvalidRegistry),
        (
            Route::ok(r#"{"name":"missing id"}"#),
            NodeAppError::InvalidRegistry,
        ),
        (
            Route::ok(r#"{"id":"another-node"}"#),
            NodeAppError::InvalidRegistry,
        ),
    ] {
        for update in [false, true] {
            let fixture = Fixture::new().await;
            fixture.registry_node("detail", "2.0.0", Fixture::good_zip());
            fixture.server.route("/nodes/detail", route.clone());
            let prior = node("detail", registry_source("detail", "1.0.0"), true);
            let request = if update {
                PrepareRequest::Update {
                    node: prior.id.clone(),
                    reference: Some("2.0.0".into()),
                }
            } else {
                PrepareRequest::Registry {
                    id: "detail".into(),
                    version: Some("2.0.0".into()),
                }
            };
            let result = fixture
                .files
                .prepare(
                    &fixture.root,
                    &node_id("node-detail"),
                    &request,
                    update.then_some(&prior),
                    &ctx(),
                )
                .await;
            assert_eq!(result.unwrap_err(), expected, "update={update}");
            assert!(fixture.owned().is_empty());
            assert!(!fixture.cn().join("detail").exists());
            let requests = fixture.server.requests.lock().unwrap();
            assert_eq!(requests.len(), 2, "release and detail only: {requests:?}");
            assert!(requests.iter().all(|r| !r.starts_with("GET /dl/")));
        }
    }
}

#[tokio::test]
async fn registry_node_and_release_warnings_are_bound_to_review() {
    let fixture = Fixture::new().await;
    fixture.registry_node("warn", "1.0.0", Fixture::good_zip());
    let detail = serde_json::json!({
        "id": "warn", "status": "NodeStatusFlagged", "deprecated": true,
        // A different latest version must not add compatibility warnings.
        "latest_version": {
            "version": "9.0.0", "status": "NodeVersionStatusActive", "deprecated": false,
            "supported_os": ["macOS"],
            "supported_accelerators": ["GPU :: NVIDIA CUDA"],
            "supported_comfyui_version": ">=0.3.0",
        },
    });
    fixture
        .server
        .route("/nodes/warn", Route::ok(detail.to_string()));
    let release = serde_json::json!({
        "node_id": "warn", "version": "1.0.0",
        "downloadUrl": fixture.server.url("/dl/warn-1.0.0.zip"),
        "supported_os": ["OS Independent"], "supported_accelerators": ["CPU"],
        "supported_comfyui_version": ">=99.0", "deprecated": true,
        "status": "NodeVersionStatusFlagged",
    });
    fixture.server.route(
        "/nodes/warn/install?version=1.0.0",
        Route::ok(release.to_string()),
    );
    let request = PrepareRequest::Registry {
        id: "warn".into(),
        version: Some("1.0.0".into()),
    };
    let staged = fixture
        .files
        .prepare(
            &fixture.root,
            &node_id("node-warn-both"),
            &request,
            None,
            &ctx(),
        )
        .await
        .unwrap();
    assert_eq!(
        staged.source,
        NodeSource::Registry {
            id: "warn".into(),
            publisher: None,
            version: "1.0.0".into(),
            repository: None,
        }
    );
    let expected = [
        "accelerator is incompatible",
        "ComfyUI is incompatible",
        "Registry status is flagged or unavailable",
        "This node is deprecated",
    ];
    assert_eq!(staged.warnings, expected);
    // Preparing again gives exactly the same deduplicated warning order.
    let repeated = fixture
        .files
        .prepare(
            &fixture.root,
            &node_id("node-warn-repeat"),
            &request,
            None,
            &ctx(),
        )
        .await
        .unwrap();
    assert_eq!(repeated.warnings, staged.warnings);
    fixture
        .server
        .route("/nodes/warn", Route::status(403, "now refused"));
    assert_eq!(
        staged.warnings.len(),
        4,
        "the prepared evidence remains frozen"
    );
    assert_eq!(
        fixture
            .files
            .prepare(
                &fixture.root,
                &node_id("node-warn-refused"),
                &request,
                None,
                &ctx(),
            )
            .await
            .unwrap_err(),
        NodeAppError::RegistryStatus(403)
    );

    // Release-only flags also survive active node-level metadata.
    fixture.server.route(
        "/nodes/warn",
        Route::ok(r#"{"id":"warn","status":"NodeStatusActive"}"#),
    );
    let release_only = fixture
        .files
        .prepare(
            &fixture.root,
            &node_id("node-warn-release"),
            &request,
            None,
            &ctx(),
        )
        .await
        .unwrap();
    assert_eq!(release_only.warnings, expected);

    // Node-only flags survive an exact release with both flags explicitly clear.
    fixture
        .server
        .route("/nodes/warn", Route::ok(detail.to_string()));
    let mut release = release;
    release["status"] = "NodeVersionStatusActive".into();
    release["deprecated"] = false.into();
    fixture.server.route(
        "/nodes/warn/install?version=1.0.0",
        Route::ok(release.to_string()),
    );
    let node_only = fixture
        .files
        .prepare(
            &fixture.root,
            &node_id("node-warn-node"),
            &request,
            None,
            &ctx(),
        )
        .await
        .unwrap();
    assert_eq!(node_only.warnings, expected);
    let requests = fixture.server.requests.lock().unwrap();
    assert!(
        requests
            .iter()
            .filter(|r| r.starts_with("GET /nodes/warn/install"))
            .all(|r| r.starts_with("GET /nodes/warn/install?version=1.0.0 "))
    );
}

#[tokio::test]
async fn registry_review_compatibility_is_exclusive_to_the_exact_release() {
    let compatible = serde_json::json!({
        "supported_os": ["OS Independent"],
        "supported_accelerators": ["GPU :: NVIDIA CUDA"],
        "supported_comfyui_version": ">=0.3.0",
        "status": "NodeVersionStatusActive", "deprecated": false,
    });
    let mut incompatible = compatible.clone();
    incompatible["supported_os"] = serde_json::json!(["macOS"]);
    let mut latest_flags = compatible.clone();
    latest_flags["status"] = "NodeVersionStatusFlagged".into();
    latest_flags["deprecated"] = true.into();
    let unknown = vec![
        "OS compatibility is unknown",
        "accelerator compatibility is unknown",
        "ComfyUI compatibility is unknown",
    ];
    for (label, mut release, mut latest, expected) in [
        (
            "exact OS is incompatible",
            incompatible,
            compatible.clone(),
            vec!["OS is incompatible"],
        ),
        (
            "missing exact compatibility",
            serde_json::json!({}),
            compatible.clone(),
            unknown.clone(),
        ),
        (
            "unparseable exact compatibility",
            serde_json::json!({
                "supported_os": ["Mystery OS"],
                "supported_accelerators": ["Future accelerator"],
                "supported_comfyui_version": "not a version",
            }),
            compatible.clone(),
            unknown,
        ),
        (
            "flags only on latest release",
            compatible,
            latest_flags,
            vec![],
        ),
    ] {
        let fixture = Fixture::new().await;
        fixture.registry_node("exact", "1.0.0", Fixture::good_zip());
        release["node_id"] = "exact".into();
        release["version"] = "1.0.0".into();
        release["downloadUrl"] = fixture.server.url("/dl/exact-1.0.0.zip").into();
        fixture.server.route(
            "/nodes/exact/install?version=1.0.0",
            Route::ok(release.to_string()),
        );
        latest["version"] = "9.0.0".into();
        fixture.server.route(
            "/nodes/exact",
            Route::ok(
                serde_json::json!({
                    "id": "exact", "status": "NodeStatusActive", "deprecated": false,
                    "supported_os": ["OS Independent"],
                    "supported_accelerators": ["GPU :: NVIDIA CUDA"],
                    "supported_comfyui_version": ">=0.3.0",
                    "latest_version": latest,
                })
                .to_string(),
            ),
        );
        let staged = fixture
            .files
            .prepare(
                &fixture.root,
                &node_id("node-exact"),
                &PrepareRequest::Registry {
                    id: "exact".into(),
                    version: Some("1.0.0".into()),
                },
                None,
                &ctx(),
            )
            .await
            .unwrap();
        assert_eq!(staged.warnings, expected, "{label}");
    }
}

fn malicious_corpus() -> Vec<(&'static str, Vec<u8>)> {
    let symlink = {
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        writer.add_symlink("link", "/etc/passwd", options).unwrap();
        writer.finish().unwrap().into_inner()
    };
    let bomb = {
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        writer.start_file("zeros.bin", options).unwrap();
        writer.write_all(&vec![0u8; 8 << 20]).unwrap();
        writer.finish().unwrap().into_inner()
    };
    let many: Vec<(String, Vec<u8>)> = (0..40).map(|i| (format!("f{i}.py"), Vec::new())).collect();
    let many = {
        let refs: Vec<(&str, &[u8])> = many
            .iter()
            .map(|(n, d)| (n.as_str(), d.as_slice()))
            .collect();
        zip_bytes(&refs)
    };
    vec![
        ("parent traversal", zip_bytes(&[("../evil.txt", b"x")])),
        ("nested traversal", zip_bytes(&[("a/../../evil.txt", b"x")])),
        ("internal dot-dot", zip_bytes(&[("foo/../bar.py", b"x")])),
        ("backslash traversal", zip_bytes(&[("..\\evil.txt", b"x")])),
        ("absolute path", zip_bytes(&[("/evil.txt", b"x")])),
        ("drive relative", zip_bytes(&[("C:evil.txt", b"x")])),
        ("drive absolute", zip_bytes(&[("C:\\evil.txt", b"x")])),
        ("unc path", zip_bytes(&[("//server/share/evil.txt", b"x")])),
        (
            "alternate data stream",
            zip_bytes(&[("file.py:stream", b"x")]),
        ),
        ("reserved name", zip_bytes(&[("CON.txt", b"x")])),
        ("trailing dot", zip_bytes(&[("name.", b"x")])),
        ("symbolic link entry", symlink),
        (
            "case collision",
            zip_bytes(&[("Module.py", b"1"), ("module.py", b"2")]),
        ),
        (
            "file then directory",
            zip_bytes(&[("a", b"1"), ("a/b.py", b"2")]),
        ),
        (
            "directory then file",
            zip_bytes(&[("a/b.py", b"2"), ("a", b"1")]),
        ),
        (
            "supplied marker",
            zip_bytes(&[
                ("__init__.py", b""),
                (".vjump-node.json", b"{\"schema\":1}"),
            ]),
        ),
        ("compression bomb", bomb),
        ("too many entries", many),
    ]
}

#[tokio::test]
async fn malicious_archives_write_nothing_outside_owned_staging() {
    // Entry cap 20 so the corpus exercises the count limit; every other limit is the default.
    let limits = ExtractLimits {
        max_entries: 20,
        ..node_limits()
    };
    for (index, (label, body)) in malicious_corpus().into_iter().enumerate() {
        let fixture = Fixture::with(None, limits).await;
        fixture.latest("evil", "1.0.0");
        fixture.registry_node("evil", "1.0.0", body);
        let request = PrepareRequest::Registry {
            id: "evil".into(),
            version: None,
        };
        let op = node_id(&format!("node-evil{index}"));
        let result = fixture
            .files
            .prepare(&fixture.root, &op, &request, None, &ctx())
            .await;
        assert!(result.is_err(), "{label} must be rejected");
        // No owned leftovers, no node, nothing beside the install root's tree.
        assert!(fixture.owned().is_empty(), "{label}: {:?}", fixture.owned());
        assert!(!fixture.cn().join("evil").exists(), "{label}");
        let leftovers: Vec<_> = fs::read_dir(fixture.cn())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(leftovers, [".disabled"], "{label}");
        let disabled: Vec<_> = fs::read_dir(fixture.disabled())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            disabled.iter().all(|n| n == ".vjump-no-hooks"),
            "{label}: {disabled:?}"
        );
        for escaped in [
            "evil.txt",
            "ComfyUI/evil.txt",
            "bar.py",
            "ComfyUI/custom_nodes/evil.txt",
        ] {
            assert!(!fixture.root.join(escaped).exists(), "{label}: {escaped}");
        }
        assert!(
            !fixture.root.parent().unwrap().join("evil.txt").exists(),
            "{label}: an entry escaped the install root"
        );
    }
}

#[tokio::test]
async fn oversize_downloads_and_insecure_transfer_urls_are_refused() {
    let fixture = Fixture::new().await;
    let release = |url: &str| {
        serde_json::json!({"node_id": "n", "version": "1.0.0", "downloadUrl": url}).to_string()
    };
    let request = PrepareRequest::Registry {
        id: "n".into(),
        version: None,
    };
    fixture.server.route(
        "/nodes/n",
        Route::ok(serde_json::json!({"id": "n", "name": "n"}).to_string()),
    );
    // A transfer URL that is not HTTPS (or loopback in tests) is not fetched at all.
    fixture.server.route(
        "/nodes/n/install",
        Route::ok(release("http://example.com/n.zip")),
    );
    fixture.server.route(
        "/nodes/n/install?version=1.0.0",
        Route::ok(release("http://example.com/n.zip")),
    );
    let refused = fixture
        .files
        .prepare(&fixture.root, &node_id("node-http"), &request, None, &ctx())
        .await;
    assert_eq!(refused.unwrap_err(), NodeAppError::UnsafeSource);
    // A truncated body is not accepted as an artifact.
    let url = format!("{}/dl/cut.zip", fixture.server.base_url);
    fixture
        .server
        .route("/nodes/n/install", Route::ok(release(&url)));
    fixture
        .server
        .route("/nodes/n/install?version=1.0.0", Route::ok(release(&url)));
    fixture.server.route(
        "/dl/cut.zip",
        Route::Truncated {
            body: vec![1, 2, 3],
            declared: 100,
        },
    );
    let truncated = fixture
        .files
        .prepare(&fixture.root, &node_id("node-cut"), &request, None, &ctx())
        .await;
    assert!(truncated.is_err());
    assert!(fixture.owned().is_empty());
}

#[tokio::test]
async fn a_registry_update_stages_the_new_release_and_reports_up_to_date() {
    let fixture = Fixture::new().await;
    fs::create_dir_all(fixture.cn().join("good-node")).unwrap();
    fs::write(fixture.cn().join("good-node/__init__.py"), "old").unwrap();
    let mut prior = node("good-node", registry_source("good-node", "1.0.0"), true);
    prior.tree_digest =
        tree::tree_digest(&pin_absolute(&fixture.cn().join("good-node")).unwrap()).unwrap();
    fixture.latest("good-node", "1.1.0");
    fixture.registry_node("good-node", "1.1.0", Fixture::good_zip());
    let op = node_id("node-upd");
    let request = PrepareRequest::Update {
        node: prior.id.clone(),
        reference: None,
    };
    let staged = fixture
        .files
        .prepare(&fixture.root, &op, &request, Some(&prior), &ctx())
        .await
        .unwrap();
    assert_eq!(staged.name.as_str(), "good-node");
    assert_eq!(staged.source, registry_source("good-node", "1.1.0"));
    // The old copy stays complete until publication.
    assert_eq!(
        fs::read_to_string(fixture.cn().join("good-node/__init__.py")).unwrap(),
        "old"
    );
    let mut operation = operation(
        OperationKind::Update,
        "node-upd",
        {
            let mut n = prior.clone();
            n.source = staged.source.clone();
            n.tree_digest = staged.tree_digest.clone();
            n
        },
        Some(prior.clone()),
        OperationPhase::PublishIntent,
    );
    fixture
        .files
        .backup(&fixture.root, &operation)
        .await
        .unwrap();
    operation.phase = OperationPhase::OldBackedUp;
    assert!(!fixture.cn().join("good-node").exists());
    assert!(
        fixture
            .disabled()
            .join(".vjump-backup-node-upd/__init__.py")
            .is_file()
    );
    fixture
        .files
        .publish(&fixture.root, &operation, Default::default())
        .await
        .unwrap();
    assert_eq!(
        fs::read_to_string(fixture.cn().join("good-node/__init__.py")).unwrap(),
        "NODE_CLASS_MAPPINGS = {}"
    );
    // After verification the previous version is recycled, not deleted.
    fixture
        .files
        .recycle(&fixture.root, &operation)
        .await
        .unwrap();
    assert_eq!(fixture.recycled.lock().unwrap().len(), 1);
    assert!(fixture.owned().is_empty());
    // The same version again is not an update.
    let mut current = prior.clone();
    current.source = registry_source("good-node", "1.1.0");
    let same = fixture
        .files
        .prepare(
            &fixture.root,
            &node_id("node-upd2"),
            &request,
            Some(&current),
            &ctx(),
        )
        .await;
    assert_eq!(same.unwrap_err(), NodeAppError::UpToDate);
}

#[tokio::test]
async fn a_disabled_node_keeps_its_disabled_location_through_update() {
    let fixture = Fixture::new().await;
    fs::create_dir_all(fixture.disabled().join("off-node")).unwrap();
    fs::write(fixture.disabled().join("off-node/__init__.py"), "old").unwrap();
    let mut prior = node("off-node", registry_source("off-node", "1.0.0"), false);
    prior.tree_digest =
        tree::tree_digest(&pin_absolute(&fixture.disabled().join("off-node")).unwrap()).unwrap();
    fixture.latest("off-node", "2.0.0");
    fixture.registry_node("off-node", "2.0.0", Fixture::good_zip());
    let request = PrepareRequest::Update {
        node: prior.id.clone(),
        reference: None,
    };
    let staged = fixture
        .files
        .prepare(
            &fixture.root,
            &node_id("node-off"),
            &request,
            Some(&prior),
            &ctx(),
        )
        .await
        .unwrap();
    let mut updated = prior.clone();
    updated.source = staged.source.clone();
    let operation = operation(
        OperationKind::Update,
        "node-off",
        updated,
        Some(prior),
        OperationPhase::PublishIntent,
    );
    fixture
        .files
        .backup(&fixture.root, &operation)
        .await
        .unwrap();
    fixture
        .files
        .publish(&fixture.root, &operation, Default::default())
        .await
        .unwrap();
    assert!(fixture.disabled().join("off-node/__init__.py").is_file());
    assert!(
        !fixture.cn().join("off-node").exists(),
        "an update must not enable the node"
    );
}

// ---------------------------------------------------------------------------------------
// Enable, disable and containment
// ---------------------------------------------------------------------------------------

#[tokio::test]
async fn enable_and_disable_round_trip_through_the_disabled_folder() {
    let fixture = Fixture::new().await;
    fs::create_dir_all(fixture.cn().join("pack ไทย")).unwrap();
    fs::write(fixture.cn().join("pack ไทย/__init__.py"), "x").unwrap();
    fs::write(fixture.cn().join("solo.py"), "x").unwrap();
    let mut pack = node("pack ไทย", NodeSource::Manual, true);
    let mut solo = node("solo.py", NodeSource::Manual, true);
    solo.single_file = true;
    for item in [&mut pack, &mut solo] {
        fixture
            .files
            .move_enabled(&fixture.root, item, false)
            .await
            .unwrap();
        item.enabled = false;
        assert!(fixture.disabled().join(item.name.as_str()).exists());
        assert!(!fixture.cn().join(item.name.as_str()).exists());
        fixture
            .files
            .move_enabled(&fixture.root, item, true)
            .await
            .unwrap();
        item.enabled = true;
        assert!(fixture.cn().join(item.name.as_str()).exists());
    }
    // A target that already exists is never replaced.
    fs::create_dir_all(fixture.disabled().join("pack ไทย")).unwrap();
    assert_eq!(
        fixture
            .files
            .move_enabled(&fixture.root, &pack, false)
            .await
            .unwrap_err(),
        NodeAppError::TargetExists
    );
    assert!(fixture.cn().join("pack ไทย/__init__.py").is_file());
}

#[tokio::test]
async fn linked_node_folders_are_never_moved_or_backed_up() {
    let fixture = Fixture::new().await;
    let outside = fixture.root.join("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("keep.txt"), "important").unwrap();
    let link = fixture.cn().join("linked");
    #[cfg(windows)]
    let made = Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&link)
        .arg(&outside)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(&outside, &link).is_ok();
    if !made {
        eprintln!("skipped: links cannot be created here");
        return;
    }
    let linked = node("linked", NodeSource::Manual, true);
    assert_eq!(
        fixture
            .files
            .move_enabled(&fixture.root, &linked, false)
            .await
            .unwrap_err(),
        NodeAppError::UnsafeSource
    );
    let op = operation(
        OperationKind::Uninstall,
        "node-link",
        linked.clone(),
        Some(linked),
        OperationPhase::PublishIntent,
    );
    assert_eq!(
        fixture.files.backup(&fixture.root, &op).await.unwrap_err(),
        NodeAppError::UnsafeSource
    );
    assert_eq!(
        fs::read_to_string(outside.join("keep.txt")).unwrap(),
        "important"
    );
    // Scanning still lists it, as an externally observed manual node.
    assert!(
        fixture
            .files
            .scan(&fixture.root, Default::default())
            .await
            .unwrap()
            .iter()
            .any(|n| n.name.as_str() == "linked")
    );
}

// ---------------------------------------------------------------------------------------
// Crash-boundary recovery
// ---------------------------------------------------------------------------------------

fn dirs(fixture: &Fixture, paths: &[&str]) {
    for path in paths {
        let full = fixture.cn().join(path);
        fs::create_dir_all(&full).unwrap();
        fs::write(full.join("__init__.py"), path).unwrap();
    }
}

#[tokio::test]
async fn install_recovery_at_every_boundary() {
    let fixture = Fixture::new().await;
    let mut new = node("pack", registry_source("pack", "1.0.0"), true);
    // Crash before publication: only staging exists and it is discarded.
    dirs(&fixture, &[".disabled/.vjump-staging-node-a"]);
    let op = operation(
        OperationKind::Install,
        "node-a",
        new.clone(),
        None,
        OperationPhase::PublishIntent,
    );
    assert_eq!(
        fixture
            .files
            .recover(&fixture.root, &op, Default::default())
            .await
            .unwrap(),
        RecoveryAction::Discarded
    );
    assert!(fixture.owned().is_empty());
    // Crash after publication with a matching tree: roll forward, keep the node.
    dirs(&fixture, &["pack"]);
    new.tree_digest =
        tree::tree_digest(&pin_absolute(&fixture.cn().join("pack")).unwrap()).unwrap();
    let op = operation(
        OperationKind::Install,
        "node-b",
        new.clone(),
        None,
        OperationPhase::PublishIntent,
    );
    assert_eq!(
        fixture
            .files
            .recover(&fixture.root, &op, Default::default())
            .await
            .unwrap(),
        RecoveryAction::RolledForward
    );
    // The folder holds something this operation did not publish: block, preserve everything.
    fs::write(
        fixture.cn().join("pack/__init__.py"),
        "changed by someone else",
    )
    .unwrap();
    assert_eq!(
        fixture
            .files
            .recover(&fixture.root, &op, Default::default())
            .await
            .unwrap(),
        RecoveryAction::Blocked
    );
    assert!(fixture.cn().join("pack/__init__.py").is_file());
    // Both staging and the published folder: ambiguous.
    dirs(&fixture, &[".disabled/.vjump-staging-node-c"]);
    let op = operation(
        OperationKind::Install,
        "node-c",
        new.clone(),
        None,
        OperationPhase::PublishIntent,
    );
    assert_eq!(
        fixture
            .files
            .recover(&fixture.root, &op, Default::default())
            .await
            .unwrap(),
        RecoveryAction::Blocked
    );
    assert!(fixture.disabled().join(".vjump-staging-node-c").is_dir());
    // A prepared plan that was never committed is discarded.
    dirs(&fixture, &[".disabled/.vjump-staging-node-d"]);
    let op = operation(
        OperationKind::Install,
        "node-d",
        new,
        None,
        OperationPhase::Prepared,
    );
    assert_eq!(
        fixture
            .files
            .recover(&fixture.root, &op, Default::default())
            .await
            .unwrap(),
        RecoveryAction::Discarded
    );
    assert!(!fixture.disabled().join(".vjump-staging-node-d").exists());
}

#[tokio::test]
async fn update_recovery_preserves_the_only_recoverable_copy() {
    let fixture = Fixture::new().await;
    let mut prior = node("pack", registry_source("pack", "1.0.0"), true);
    let new_node = {
        let mut n = prior.clone();
        n.source = registry_source("pack", "2.0.0");
        n
    };
    let cases = |id: &str, prior: &InstalledNode| {
        operation(
            OperationKind::Update,
            id,
            new_node.clone(),
            Some(prior.clone()),
            OperationPhase::OldBackedUp,
        )
    };
    // (live, backup, staging) = (T, F, T): nothing moved yet.
    dirs(&fixture, &["pack", ".disabled/.vjump-staging-node-u1"]);
    prior.tree_digest =
        tree::tree_digest(&pin_absolute(&fixture.cn().join("pack")).unwrap()).unwrap();
    assert_eq!(
        fixture
            .files
            .recover(&fixture.root, &cases("node-u1", &prior), Default::default())
            .await
            .unwrap(),
        RecoveryAction::RolledBack
    );
    assert!(fixture.cn().join("pack").is_dir() && fixture.owned().is_empty());
    // (F, T, T): old copy backed up, new one not published: restore the old copy.
    fs::rename(
        fixture.cn().join("pack"),
        fixture.disabled().join(".vjump-backup-node-u2"),
    )
    .unwrap();
    dirs(&fixture, &[".disabled/.vjump-staging-node-u2"]);
    assert_eq!(
        fixture
            .files
            .recover(&fixture.root, &cases("node-u2", &prior), Default::default())
            .await
            .unwrap(),
        RecoveryAction::RolledBack
    );
    assert_eq!(
        fs::read_to_string(fixture.cn().join("pack/__init__.py")).unwrap(),
        "pack"
    );
    assert!(fixture.owned().is_empty());
    // (T, T, F): new copy published, old one retained: roll forward and keep the backup.
    let mut published = new_node.clone();
    fs::rename(
        fixture.cn().join("pack"),
        fixture.disabled().join(".vjump-backup-node-u3"),
    )
    .unwrap();
    dirs(&fixture, &["pack-new"]);
    fs::rename(fixture.cn().join("pack-new"), fixture.cn().join("pack")).unwrap();
    published.tree_digest =
        tree::tree_digest(&pin_absolute(&fixture.cn().join("pack")).unwrap()).unwrap();
    let op = operation(
        OperationKind::Update,
        "node-u3",
        published,
        Some(prior.clone()),
        OperationPhase::Published,
    );
    assert_eq!(
        fixture
            .files
            .recover(&fixture.root, &op, Default::default())
            .await
            .unwrap(),
        RecoveryAction::RolledForward
    );
    assert!(
        fixture.disabled().join(".vjump-backup-node-u3").is_dir(),
        "the backup is retained"
    );
    // After verification the same state completes by recycling the backup.
    let mut done = op.clone();
    done.phase = OperationPhase::CleanupPending;
    assert_eq!(
        fixture
            .files
            .recover(&fixture.root, &done, Default::default())
            .await
            .unwrap(),
        RecoveryAction::RolledForward
    );
    assert!(fixture.owned().is_empty());
    assert_eq!(fixture.recycled.lock().unwrap().len(), 1);
    // (T, T, T) and an unknown live tree are ambiguous: nothing is touched.
    dirs(
        &fixture,
        &[
            ".disabled/.vjump-backup-node-u4",
            ".disabled/.vjump-staging-node-u4",
        ],
    );
    assert_eq!(
        fixture
            .files
            .recover(&fixture.root, &cases("node-u4", &prior), Default::default())
            .await
            .unwrap(),
        RecoveryAction::Blocked
    );
    assert!(fixture.disabled().join(".vjump-backup-node-u4").is_dir());
    assert!(fixture.disabled().join(".vjump-staging-node-u4").is_dir());
}

#[tokio::test]
async fn uninstall_and_toggle_recovery() {
    let fixture = Fixture::new().await;
    let prior = node("pack", NodeSource::Manual, true);
    dirs(&fixture, &["pack"]);
    fs::create_dir_all(fixture.disabled()).unwrap();
    // Uninstall: moved to the backup, crash before the node record changed: put it back.
    fs::rename(
        fixture.cn().join("pack"),
        fixture.disabled().join(".vjump-backup-node-x1"),
    )
    .unwrap();
    let op = operation(
        OperationKind::Uninstall,
        "node-x1",
        prior.clone(),
        Some(prior.clone()),
        OperationPhase::OldBackedUp,
    );
    assert_eq!(
        fixture
            .files
            .recover(&fixture.root, &op, Default::default())
            .await
            .unwrap(),
        RecoveryAction::RolledBack
    );
    assert!(fixture.cn().join("pack/__init__.py").is_file());
    // Uninstall after verification finishes the recycle.
    fs::rename(
        fixture.cn().join("pack"),
        fixture.disabled().join(".vjump-backup-node-x2"),
    )
    .unwrap();
    let mut op = operation(
        OperationKind::Uninstall,
        "node-x2",
        prior.clone(),
        Some(prior.clone()),
        OperationPhase::CleanupPending,
    );
    assert_eq!(
        fixture
            .files
            .recover(&fixture.root, &op, Default::default())
            .await
            .unwrap(),
        RecoveryAction::RolledForward
    );
    assert!(fixture.owned().is_empty());
    // A failing recycler keeps the backup and reports pending cleanup.
    dirs(&fixture, &[".disabled/.vjump-backup-node-x3"]);
    op.id = node_id("node-x3");
    let failing: Recycler = Arc::new(|_, _, _| Err(std::io::Error::other("no trash")));
    let stuck = LocalNodeFiles {
        registry: fixture.files.registry.clone(),
        git: fixture.files.git.clone(),
        recycler: failing,
        limits: node_limits(),
    };
    assert_eq!(
        stuck
            .recover(&fixture.root, &op, Default::default())
            .await
            .unwrap(),
        RecoveryAction::CleanupPending
    );
    assert!(fixture.disabled().join(".vjump-backup-node-x3").is_dir());
    // Toggle: the move completed, or did not start; anything else blocks.
    dirs(&fixture, &["pack"]);
    let mut disable = prior.clone();
    disable.enabled = false;
    let op = operation(
        OperationKind::Disable,
        "node-t1",
        disable.clone(),
        Some(prior.clone()),
        OperationPhase::PublishIntent,
    );
    assert_eq!(
        fixture
            .files
            .recover(&fixture.root, &op, Default::default())
            .await
            .unwrap(),
        RecoveryAction::RolledBack
    );
    fs::rename(fixture.cn().join("pack"), fixture.disabled().join("pack")).unwrap();
    assert_eq!(
        fixture
            .files
            .recover(&fixture.root, &op, Default::default())
            .await
            .unwrap(),
        RecoveryAction::RolledForward
    );
    dirs(&fixture, &["pack"]);
    assert_eq!(
        fixture
            .files
            .recover(&fixture.root, &op, Default::default())
            .await
            .unwrap(),
        RecoveryAction::Blocked
    );
}

#[tokio::test]
async fn startup_scan_never_treats_owned_staging_as_installed() {
    let fixture = Fixture::new().await;
    dirs(
        &fixture,
        &[
            ".disabled/.vjump-staging-node-s",
            ".disabled/.vjump-backup-node-s",
            "real",
        ],
    );
    fs::write(fixture.disabled().join(".vjump-staging-node-s.zip"), "zip").unwrap();
    let nodes = fixture
        .files
        .scan(&fixture.root, Default::default())
        .await
        .unwrap();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].name.as_str(), "real");
    // Discarding removes the staging directory, the download and the hoist folder only.
    fixture
        .files
        .discard_staging(&fixture.root, &node_id("node-s"))
        .await
        .unwrap();
    assert!(!fixture.disabled().join(".vjump-staging-node-s").exists());
    assert!(
        !fixture
            .disabled()
            .join(".vjump-staging-node-s.zip")
            .exists()
    );
    assert!(
        fixture.disabled().join(".vjump-backup-node-s").is_dir(),
        "backups are never discarded"
    );
    assert!(fixture.cn().join("real").is_dir());
}

// ---------------------------------------------------------------------------------------
// SQLite persistence
// ---------------------------------------------------------------------------------------

#[tokio::test]
async fn nodes_operations_dependencies_and_trust_round_trip() {
    let fixture = Fixture::new().await;
    let store = &fixture.store;
    let first = node("alpha", registry_source("alpha", "1.0.0"), true);
    let mut second = node(
        "beta",
        NodeSource::Git {
            repository: "https://github.com/o/r".into(),
            commit: "a".repeat(40),
            branch: Some("main".into()),
        },
        false,
    );
    second.dependencies = DependencyState::Failed;
    second.dirty = Some(true);
    store.save_node(&first).await.unwrap();
    store.save_node(&second).await.unwrap();
    assert_eq!(
        store.nodes().await.unwrap(),
        [first.clone(), second.clone()]
    );
    // A different record for the same folder replaces the old one.
    let mut replaced = first.clone();
    replaced.id = node_id("n-other");
    store.save_node(&replaced).await.unwrap();
    assert_eq!(store.nodes().await.unwrap().len(), 2);
    store.remove_node(&second.id).await.unwrap();
    assert_eq!(store.nodes().await.unwrap(), [replaced]);
    // The journal keeps phase, identities and a sanitized failure.
    let mut op = operation(
        OperationKind::Update,
        "node-j",
        first.clone(),
        Some(first.clone()),
        OperationPhase::Prepared,
    );
    op.failure = Some("failed with token=abc123def456 at https://user:pw@host/x".into());
    op.dependency_requirements = vec!["numpy>=1".into()];
    store.save_operation(&op).await.unwrap();
    op.phase = OperationPhase::DependenciesInProgress;
    store.save_operation(&op).await.unwrap();
    let loaded = store.operations().await.unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].phase, OperationPhase::DependenciesInProgress);
    assert_eq!(loaded[0].dependency_requirements, ["numpy>=1"]);
    assert_eq!(loaded[0].prior.as_ref().unwrap().id, first.id);
    assert!(
        !loaded[0].failure.as_deref().unwrap().contains("pw@"),
        "{:?}",
        loaded[0].failure
    );
    // Trust is keyed by its namespace only.
    let a = registry_source("a", "1");
    let b = registry_source("b", "1");
    store
        .save_trust(&Trust {
            namespace: a.trust_namespace().unwrap(),
            allow_install_py: true,
        })
        .await
        .unwrap();
    assert!(
        store
            .trust(&a.trust_namespace().unwrap())
            .await
            .unwrap()
            .unwrap()
            .allow_install_py
    );
    assert!(
        store
            .trust(&b.trust_namespace().unwrap())
            .await
            .unwrap()
            .is_none()
    );
    // Dependency outcomes are per environment and source revision.
    for (env, state) in [
        ("env-1", DependencyState::Ok),
        ("env-2", DependencyState::Failed),
    ] {
        store
            .save_dependency(&DependencyOutcome {
                node: first.id.clone(),
                environment: EnvId::new(env).unwrap(),
                source_identity: first.source.identity(),
                state,
            })
            .await
            .unwrap();
    }
    let count: i64 = store
        .db
        .run(|c| {
            Ok(
                c.query_row("SELECT COUNT(*) FROM node_env_deps", [], |r| r.get(0))
                    .unwrap(),
            )
        })
        .await
        .unwrap();
    assert_eq!(count, 2);
}

// ---------------------------------------------------------------------------------------
// Git
// ---------------------------------------------------------------------------------------

/// The status text of a Git refusal; any other error fails the test.
fn refused_status(error: NodeAppError) -> String {
    match error {
        NodeAppError::GitRefused { status } => status,
        other => panic!("expected a Git refusal, got {other:?}"),
    }
}

struct Repo {
    dir: PathBuf,
    git: PathBuf,
}

impl Repo {
    fn run(&self, args: &[&str]) -> String {
        let output = Command::new(&self.git)
            .current_dir(&self.dir)
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@example.com",
                "-c",
                "commit.gpgsign=false",
            ])
            .args([
                "-c",
                "core.autocrlf=false",
                "-c",
                "protocol.file.allow=always",
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

    fn new(parent: &Path, git: &Path, name: &str) -> Self {
        let dir = parent.join(name);
        fs::create_dir_all(&dir).unwrap();
        let repo = Self {
            dir,
            git: git.to_owned(),
        };
        repo.run(&["init", "-q", "-b", "main"]);
        repo
    }

    fn commit(&self, file: &str, text: &str, message: &str) -> String {
        let path = self.dir.join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
        self.run(&["add", "-A"]);
        self.run(&["commit", "-q", "-m", message]);
        self.run(&["rev-parse", "HEAD"])
    }
}

struct GitFixture {
    fixture: Fixture,
    repo: Repo,
    url: String,
}

async fn git_fixture(git: PathBuf) -> GitFixture {
    let fixture = Fixture::with(Some(git.clone()), node_limits()).await;
    let repo = Repo::new(fixture._directory.path(), &git, "ComfyUI-Remote");
    repo.commit("__init__.py", "NODE_CLASS_MAPPINGS = {}", "one");
    repo.commit("requirements.txt", "numpy\n", "two");
    let url = repo.dir.to_string_lossy().into_owned();
    GitFixture { fixture, repo, url }
}

impl GitFixture {
    fn request(&self, reference: Option<&str>) -> PrepareRequest {
        PrepareRequest::Git {
            repository: self.url.clone(),
            reference: reference.map(str::to_owned),
        }
    }

    /// Prepares, then publishes a Git install and returns the installed node.
    async fn install(&self, id: &str, reference: Option<&str>) -> InstalledNode {
        let op = node_id(id);
        let staged = self
            .fixture
            .files
            .prepare(
                &self.fixture.root,
                &op,
                &self.request(reference),
                None,
                &ctx(),
            )
            .await
            .unwrap();
        let mut installed = node(staged.name.as_str(), staged.source.clone(), true);
        installed.tree_digest = staged.tree_digest;
        let operation = operation(
            OperationKind::Install,
            id,
            installed.clone(),
            None,
            OperationPhase::PublishIntent,
        );
        self.fixture
            .files
            .publish(&self.fixture.root, &operation, Default::default())
            .await
            .unwrap();
        installed
    }

    async fn update(
        &self,
        id: &str,
        prior: &InstalledNode,
        reference: Option<&str>,
    ) -> Result<NodeOperation, NodeAppError> {
        let request = PrepareRequest::Update {
            node: prior.id.clone(),
            reference: reference.map(str::to_owned),
        };
        let staged = self
            .fixture
            .files
            .prepare(
                &self.fixture.root,
                &node_id(id),
                &request,
                Some(prior),
                &ctx(),
            )
            .await?;
        let mut updated = prior.clone();
        updated.source = staged.source;
        updated.tree_digest = staged.tree_digest;
        Ok(operation(
            OperationKind::Update,
            id,
            updated,
            Some(prior.clone()),
            OperationPhase::PublishIntent,
        ))
    }
}

#[tokio::test]
async fn git_install_resolves_one_immutable_commit_and_tracks_a_branch() {
    let git = require_git!();
    let g = git_fixture(git).await;
    let head = g.repo.run(&["rev-parse", "HEAD"]);
    let installed = g.install("node-g1", None).await;
    assert_eq!(installed.name.as_str(), "ComfyUI-Remote");
    let NodeSource::Git {
        commit,
        branch,
        repository,
    } = &installed.source
    else {
        panic!()
    };
    assert_eq!(
        (commit.as_str(), branch.as_deref()),
        (head.as_str(), Some("main"))
    );
    assert_eq!(repository, &g.url);
    let live = g.fixture.cn().join("ComfyUI-Remote");
    assert_eq!(
        installed.tree_digest,
        tree::tree_digest(&pin_absolute(&live).unwrap()).unwrap()
    );
    assert!(installed.tree_digest.starts_with("tree:"));
    assert!(live.join("requirements.txt").is_file());
    let meta = scan::read_git(&pin_absolute(&live).unwrap()).unwrap();
    assert_eq!(meta.commit.as_deref(), Some(head.as_str()));
    assert_eq!(meta.branch.as_deref(), Some("main"));
    // The app's marker does not make the clone dirty.
    assert!(
        !g.fixture
            .files
            .git_status(&g.fixture.root, &installed, Default::default())
            .await
            .unwrap()
            .state
            .dirty
    );
    assert!(g.fixture.owned().is_empty());
    // A second install of the same repository is refused.
    let again = g
        .fixture
        .files
        .prepare(
            &g.fixture.root,
            &node_id("node-g2"),
            &g.request(None),
            None,
            &ctx(),
        )
        .await;
    assert_eq!(again.unwrap_err(), NodeAppError::TargetExists);
}

#[tokio::test]
async fn git_staged_content_changes_invalidate_the_review() {
    let git = locate_git().expect("Git must execute for staged-content coverage");
    let g = git_fixture(git).await;
    g.repo
        .commit(".gitignore", "ignored.dat\n", "ignore generated data");
    g.repo
        .commit("nodes/deep.py", "reviewed = True\n", "nested code");
    let install_id = node_id("staged-install");
    let staged = g
        .fixture
        .files
        .prepare(&g.fixture.root, &install_id, &g.request(None), None, &ctx())
        .await
        .unwrap();
    let mut reviewed = node(staged.name.as_str(), staged.source, true);
    reviewed.tree_digest = staged.tree_digest;
    for update in [false, true] {
        let (id, candidate) = if update {
            g.repo.commit("update.py", "target = True\n", "new target");
            let op = g.update("staged-update", &reviewed, None).await.unwrap();
            (op.id, op.node)
        } else {
            (install_id.clone(), reviewed.clone())
        };
        let directory = g.fixture.files.path(&g.fixture.root, &candidate, Some(&id));
        let before = g
            .fixture
            .files
            .snapshot(&g.fixture.root, &candidate, Some(&id), Default::default())
            .await
            .unwrap();
        assert_eq!(before.digest, candidate.tree_digest);
        for (file, original) in [
            ("nodes/deep.py", Some("reviewed = True\n")),
            ("untracked.py", None),
            ("ignored.dat", None),
        ] {
            fs::write(directory.join(file), "changed after review\n").unwrap();
            assert_ne!(
                tree::tree_digest(&pin_absolute(&directory).unwrap()).unwrap(),
                before.digest,
                "{file}"
            );
            let changed = g
                .fixture
                .files
                .snapshot(&g.fixture.root, &candidate, Some(&id), Default::default())
                .await;
            if file == "ignored.dat" {
                assert_ne!(changed.unwrap().digest, before.digest);
            } else {
                assert!(refused_status(changed.unwrap_err()).contains(file));
            }
            match original {
                Some(text) => fs::write(directory.join(file), text).unwrap(),
                None => fs::remove_file(directory.join(file)).unwrap(),
            }
        }
        if update {
            // A clean checkout at a different commit is stale even with identical files.
            let checkout = Repo {
                dir: directory.clone(),
                git: g.repo.git.clone(),
            };
            checkout.run(&["commit", "--allow-empty", "-q", "-m", "foreign commit"]);
            assert_eq!(
                tree::tree_digest(&pin_absolute(&directory).unwrap()).unwrap(),
                before.digest
            );
            assert_eq!(
                g.fixture
                    .files
                    .snapshot(&g.fixture.root, &candidate, Some(&id), Default::default())
                    .await
                    .unwrap_err(),
                NodeAppError::StalePlan
            );
            let metadata = directory.join(".git");
            let saved = directory.join(".git-saved");
            fs::rename(&metadata, &saved).unwrap();
            assert!(
                refused_status(
                    g.fixture
                        .files
                        .snapshot(&g.fixture.root, &candidate, Some(&id), Default::default())
                        .await
                        .unwrap_err()
                )
                .contains("metadata")
            );
            fs::write(&metadata, "gitdir: elsewhere\n").unwrap();
            assert_eq!(
                g.fixture
                    .files
                    .snapshot(&g.fixture.root, &candidate, Some(&id), Default::default())
                    .await
                    .unwrap_err(),
                NodeAppError::UnsafeSource
            );
            fs::remove_file(&metadata).unwrap();
            assert!(directory_link(&g.repo.dir.join(".git"), &metadata));
            assert_eq!(
                g.fixture
                    .files
                    .snapshot(&g.fixture.root, &candidate, Some(&id), Default::default())
                    .await
                    .unwrap_err(),
                NodeAppError::UnsafeSource
            );
            remove_directory_link(&metadata);
            fs::rename(&saved, &metadata).unwrap();
        } else {
            let op = operation(
                OperationKind::Install,
                id.as_str(),
                candidate,
                None,
                OperationPhase::PublishIntent,
            );
            g.fixture
                .files
                .publish(&g.fixture.root, &op, Default::default())
                .await
                .unwrap();
        }
    }
}

#[tokio::test]
async fn git_update_stages_the_complete_frozen_tree() {
    let git = locate_git().expect("Git must execute for complete-tree staging coverage");
    let g = git_fixture(git).await;
    let prior = g.install("complete-install", None).await;
    g.repo
        .commit("install.py", "print('reviewed hook')\n", "hook");
    g.repo
        .commit("requirements/extra.txt", "pillow\n", "nested requirements");
    g.repo.commit(
        "requirements.txt",
        "-r requirements/extra.txt\n",
        "include requirements",
    );
    let target = g
        .repo
        .commit("nodes/nested/code.py", "value = 7\n", "nested code");
    let op = g.update("complete-update", &prior, None).await.unwrap();
    g.repo
        .commit("later.py", "not reviewed\n", "moved upstream");
    let staging = g
        .fixture
        .files
        .path(&g.fixture.root, &op.node, Some(&op.id));
    assert!(staging.join(".git").is_dir());
    assert_eq!(
        scan::read_git(&pin_absolute(&staging).unwrap())
            .unwrap()
            .commit
            .as_deref(),
        Some(target.as_str())
    );
    assert_eq!(
        fs::read_to_string(staging.join("nodes/nested/code.py")).unwrap(),
        "value = 7\n"
    );
    assert_eq!(
        fs::read_to_string(staging.join("requirements/extra.txt")).unwrap(),
        "pillow\n"
    );
    assert!(staging.join("install.py").is_file());
    assert!(!staging.join("later.py").exists());
    let snapshot = g
        .fixture
        .files
        .snapshot(&g.fixture.root, &op.node, Some(&op.id), Default::default())
        .await
        .unwrap();
    assert_eq!(snapshot.digest, op.node.tree_digest);
    assert_eq!(
        snapshot.digest,
        tree::tree_digest(&pin_absolute(&staging).unwrap()).unwrap()
    );
    assert_eq!(snapshot.requirement_lines, ["-r requirements/extra.txt"]);
    assert!(snapshot.requirements_digest.is_some() && snapshot.hook_digest.is_some());
    fs::write(staging.join("nodes/nested/code.py"), "tampered\n").unwrap();
    assert_ne!(
        tree::tree_digest(&pin_absolute(&staging).unwrap()).unwrap(),
        snapshot.digest
    );
    assert!(
        refused_status(
            g.fixture
                .files
                .snapshot(&g.fixture.root, &op.node, Some(&op.id), Default::default())
                .await
                .unwrap_err()
        )
        .contains("nodes/nested/code.py")
    );
    fs::write(staging.join("nodes/nested/code.py"), "value = 7\n").unwrap();
    g.fixture
        .files
        .publish(&g.fixture.root, &op, Default::default())
        .await
        .unwrap();
    let live = g.fixture.cn().join(prior.name.as_str());
    assert_eq!(
        scan::read_git(&pin_absolute(&live).unwrap())
            .unwrap()
            .commit
            .as_deref(),
        Some(target.as_str())
    );
    assert_eq!(
        tree::tree_digest(&pin_absolute(&live).unwrap()).unwrap(),
        snapshot.digest
    );
    assert!(!live.join("later.py").exists());
}

#[tokio::test]
async fn git_post_apply_validation_precedes_dependency_execution() {
    let git = locate_git().expect("Git must execute for post-apply validation coverage");
    for change in ["tracked", "ignored", "origin", "unfinished", "head", "link"] {
        let g = git_fixture(git.clone()).await;
        g.repo
            .commit(".gitignore", "ignored.dat\n", "ignore generated data");
        let prior = g.install("post-install", None).await;
        let target = g.repo.commit("target.py", "reviewed = True\n", "target");
        let op = g.update("post-update", &prior, None).await.unwrap();
        let live = g.fixture.cn().join(prior.name.as_str());
        let external = g.fixture._directory.path().join("external");
        fs::create_dir(&external).unwrap();
        fs::write(external.join("sentinel"), "keep").unwrap();
        let (modified, linked, git) = (live.clone(), external.clone(), g.repo.git.clone());
        super::files::after_git_apply(move || {
            // This boundary runs only after the real Git apply returned successfully.
            let local = Repo {
                dir: modified.clone(),
                git,
            };
            assert_eq!(local.run(&["rev-parse", "HEAD"]), target);
            match change {
                "tracked" => fs::write(modified.join("target.py"), "late edit\n").unwrap(),
                "ignored" => {
                    fs::write(modified.join("ignored.dat"), "late ignored edit\n").unwrap()
                }
                "origin" => {
                    local.run(&["remote", "set-url", "origin", "https://example.com/other"]);
                }
                "unfinished" => {
                    fs::write(modified.join(".git/MERGE_HEAD"), format!("{target}\n")).unwrap()
                }
                "head" => {
                    local.run(&["commit", "--allow-empty", "-q", "-m", "late commit"]);
                }
                "link" => {
                    // Even entries excluded from the digest must pass link/reparse validation.
                    assert!(directory_link(&linked, &modified.join("__pycache__")));
                }
                _ => unreachable!(),
            }
        });
        let error = g
            .fixture
            .files
            .publish(&g.fixture.root, &op, Default::default())
            .await
            .unwrap_err();
        match change {
            "tracked" => assert!(refused_status(error).contains("target.py")),
            "ignored" => assert_eq!(error, NodeAppError::StalePlan),
            "origin" | "link" => assert_eq!(error, NodeAppError::UnsafeSource),
            "unfinished" => assert!(refused_status(error).contains("unfinished")),
            "head" => assert!(matches!(error, NodeAppError::Failed(_))),
            _ => unreachable!(),
        }
        assert_eq!(
            g.fixture
                .files
                .recover(&g.fixture.root, &op, Default::default())
                .await
                .unwrap(),
            RecoveryAction::Blocked,
            "{change} must remain unresolved"
        );
        assert!(live.join("target.py").exists());
        assert_eq!(
            fs::read_to_string(external.join("sentinel")).unwrap(),
            "keep"
        );
        if change == "tracked" {
            assert_eq!(
                fs::read_to_string(live.join("target.py")).unwrap(),
                "late edit\n"
            );
        } else if change == "ignored" {
            assert_eq!(
                fs::read_to_string(live.join("ignored.dat")).unwrap(),
                "late ignored edit\n"
            );
        } else if change == "link" {
            remove_directory_link(&live.join("__pycache__"));
        }
    }
}

#[tokio::test]
async fn dirty_git_nodes_can_still_be_disabled_and_uninstalled() {
    let git = locate_git().expect("Git must execute for dirty-node lifecycle coverage");
    let g = git_fixture(git).await;
    let mut installed = g.install("dirty-install", None).await;
    let live = g.fixture.cn().join(installed.name.as_str());
    fs::write(live.join("__init__.py"), "user edit\n").unwrap();
    fs::write(live.join("untracked.txt"), "user file\n").unwrap();
    let snapshot = g
        .fixture
        .files
        .snapshot(&g.fixture.root, &installed, None, Default::default())
        .await
        .unwrap();
    assert_ne!(snapshot.digest, installed.tree_digest);
    installed.tree_digest = snapshot.digest.clone();
    assert_eq!(
        {
            let nodes = g
                .fixture
                .files
                .scan(&g.fixture.root, Default::default())
                .await
                .unwrap();
            assert!(
                !nodes[0].conflict,
                "observed={:?}; expected={:?}",
                nodes[0], installed.source
            );
            nodes[0].dirty
        },
        Some(true)
    );
    g.fixture
        .files
        .move_enabled(&g.fixture.root, &installed, false)
        .await
        .unwrap();
    installed.enabled = false;
    let disabled = g.fixture.disabled().join(installed.name.as_str());
    assert!(!live.exists());
    assert_eq!(
        g.fixture
            .files
            .snapshot(&g.fixture.root, &installed, None, Default::default())
            .await
            .unwrap()
            .digest,
        snapshot.digest
    );
    assert_eq!(
        fs::read_to_string(disabled.join("__init__.py")).unwrap(),
        "user edit\n"
    );
    assert_eq!(
        fs::read_to_string(disabled.join("untracked.txt")).unwrap(),
        "user file\n"
    );
    let uninstall = operation(
        OperationKind::Uninstall,
        "dirty-remove",
        installed.clone(),
        Some(installed),
        OperationPhase::PublishIntent,
    );
    g.fixture
        .files
        .backup(&g.fixture.root, &uninstall)
        .await
        .unwrap();
    let backup = g.fixture.disabled().join(".vjump-backup-dirty-remove");
    assert_eq!(
        fs::read_to_string(backup.join("__init__.py")).unwrap(),
        "user edit\n"
    );
    assert_eq!(
        fs::read_to_string(backup.join("untracked.txt")).unwrap(),
        "user file\n"
    );
    assert!(!disabled.exists());
    g.fixture
        .files
        .recycle(&g.fixture.root, &uninstall)
        .await
        .unwrap();
    assert!(!backup.exists());
    assert_eq!(g.fixture.recycled.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn git_status_is_read_only_and_reports_relative_changes() {
    let git = locate_git().expect("Git must be available for the status regression");
    let g = git_fixture(git.clone()).await;
    let installed = g.install("status-install", None).await;
    let live = g.fixture.cn().join(installed.name.as_str());
    let checkout = Repo {
        dir: live.clone(),
        git,
    };
    let remote_head = g
        .repo
        .commit("remote.py", "remote = True\n", "remote change");
    checkout.run(&["fetch", "origin"]);
    let local_head = checkout.commit("local.py", "local = True\n", "local change");
    fs::write(live.join("requirements.txt"), "edited requirement\n").unwrap();
    fs::write(
        live.join("install.py"),
        "raise Exception('must never execute')\n",
    )
    .unwrap();
    // A later remote commit must remain unseen: status never fetches.
    g.repo
        .commit("later.py", "later = True\n", "later remote change");
    let status = g
        .fixture
        .files
        .git_status(&g.fixture.root, &installed, Default::default())
        .await
        .unwrap();
    assert_eq!(status.revision.commit, local_head);
    assert_eq!(status.revision.branch.as_deref(), Some("main"));
    assert!(status.state.dirty && status.state.has_upstream);
    assert!(!status.state.unfinished_operation);
    assert_eq!((status.state.ahead, status.state.behind), (1, 1));
    assert!(status.state.status.contains("requirements.txt"));
    assert!(status.state.status.contains("install.py"));
    assert!(
        !status
            .state
            .status
            .contains(g.fixture.root.to_string_lossy().as_ref())
    );
    assert_eq!(checkout.run(&["rev-parse", "HEAD"]), local_head);
    assert_eq!(checkout.run(&["rev-parse", "origin/main"]), remote_head);
    assert_eq!(
        fs::read_to_string(live.join("requirements.txt")).unwrap(),
        "edited requirement\n"
    );
    assert!(g.fixture.owned().is_empty());

    checkout.run(&["branch", "--unset-upstream"]);
    let status = g
        .fixture
        .files
        .git_status(&g.fixture.root, &installed, Default::default())
        .await
        .unwrap();
    assert!(!status.state.has_upstream);
    assert_eq!((status.state.ahead, status.state.behind), (0, 0));
    fs::write(live.join(".git/MERGE_HEAD"), format!("{remote_head}\n")).unwrap();
    assert!(
        g.fixture
            .files
            .git_status(&g.fixture.root, &installed, Default::default())
            .await
            .unwrap()
            .state
            .unfinished_operation
    );
    fs::remove_file(live.join(".git/MERGE_HEAD")).unwrap();
    checkout.run(&["checkout", "--detach"]);
    assert!(
        g.fixture
            .files
            .git_status(&g.fixture.root, &installed, Default::default())
            .await
            .unwrap()
            .revision
            .branch
            .is_none()
    );

    let secret = "node-git-status-secret-7392";
    crate::shared::logging::register_secret(secret);
    fs::write(live.join(format!("000-{secret}.txt")), "secret filename").unwrap();
    for index in 0..100 {
        fs::write(
            live.join(format!("status-file-{index:03}-long-relative-name.txt")),
            "x",
        )
        .unwrap();
    }
    let status = g
        .fixture
        .files
        .git_status(&g.fixture.root, &installed, Default::default())
        .await
        .unwrap();
    assert_eq!(status.state.status.chars().count(), 2000);
    assert!(!status.state.status.contains(secret));
    assert!(status.state.status.contains("[REDACTED]"));
}

#[tokio::test]
async fn git_status_rejects_links_and_reports_missing_git() {
    let git = locate_git().expect("Git must be available for the status regression");
    let mut g = git_fixture(git).await;
    let installed = g.install("status-links", None).await;
    let live = g.fixture.cn().join(installed.name.as_str());
    let link = g.fixture.cn().join("linked");
    #[cfg(windows)]
    assert!(
        Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&live)
            .output()
            .unwrap()
            .status
            .success()
    );
    #[cfg(unix)]
    std::os::unix::fs::symlink(&live, &link).unwrap();
    let mut linked = installed.clone();
    linked.name = SafeFileName::new("linked").unwrap();
    assert_eq!(
        g.fixture
            .files
            .git_status(&g.fixture.root, &linked, Default::default())
            .await
            .unwrap_err(),
        NodeAppError::UnsafeSource
    );
    g.fixture.files.git = Arc::new(GitNodes::with_program(
        g.fixture.root.join("missing-git.exe"),
        false,
    ));
    assert_eq!(
        g.fixture
            .files
            .git_status(&g.fixture.root, &installed, Default::default())
            .await
            .unwrap_err(),
        NodeAppError::GitUnavailable
    );
    let mut missing = installed.clone();
    missing.name = SafeFileName::new("missing-node").unwrap();
    assert_eq!(
        g.fixture
            .files
            .git_status(&g.fixture.root, &missing, Default::default())
            .await
            .unwrap_err(),
        NodeAppError::NotFound
    );
    // Querying a disabled node uses its retained location, without creating layout or staging.
    g.fixture
        .files
        .move_enabled(&g.fixture.root, &installed, false)
        .await
        .unwrap();
    let mut disabled = installed;
    disabled.enabled = false;
    assert_eq!(
        g.fixture
            .files
            .git_status(&g.fixture.root, &disabled, Default::default())
            .await
            .unwrap_err(),
        NodeAppError::GitUnavailable
    );
}

#[tokio::test]
async fn git_tags_and_commits_stay_detached_and_ambiguous_names_are_rejected() {
    let git = require_git!();
    let g = git_fixture(git).await;
    let first = g.repo.run(&["rev-list", "--max-parents=0", "HEAD"]);
    g.repo.run(&["tag", "v1", &first]);
    let staged = g
        .fixture
        .files
        .prepare(
            &g.fixture.root,
            &node_id("node-t1"),
            &g.request(Some("v1")),
            None,
            &ctx(),
        )
        .await
        .unwrap();
    assert_eq!(
        staged.source,
        NodeSource::Git {
            repository: g.url.clone(),
            commit: first.clone(),
            branch: None
        }
    );
    assert!(staged.warnings.iter().any(|w| w.contains("detached")));
    g.fixture
        .files
        .discard_staging(&g.fixture.root, &node_id("node-t1"))
        .await
        .unwrap();
    let pinned = g
        .fixture
        .files
        .prepare(
            &g.fixture.root,
            &node_id("node-t2"),
            &g.request(Some(&first)),
            None,
            &ctx(),
        )
        .await
        .unwrap();
    assert_eq!(
        pinned.source,
        NodeSource::Git {
            repository: g.url.clone(),
            commit: first.clone(),
            branch: None
        }
    );
    g.fixture
        .files
        .discard_staging(&g.fixture.root, &node_id("node-t2"))
        .await
        .unwrap();
    // A name that is both a branch and a tag is not guessed.
    g.repo.run(&["branch", "release"]);
    g.repo.run(&["tag", "release", &first]);
    let ambiguous = g
        .fixture
        .files
        .prepare(
            &g.fixture.root,
            &node_id("node-t3"),
            &g.request(Some("release")),
            None,
            &ctx(),
        )
        .await;
    assert_eq!(ambiguous.unwrap_err(), NodeAppError::AmbiguousReference);
    assert!(g.fixture.owned().is_empty());
    // Unsafe references never reach Git.
    for bad in ["--upload-pack=evil", "a..b", "x@{1}", "-x"] {
        let result = g
            .fixture
            .files
            .prepare(
                &g.fixture.root,
                &node_id("node-t4"),
                &g.request(Some(bad)),
                None,
                &ctx(),
            )
            .await;
        assert!(result.is_err(), "{bad}");
    }
}

#[tokio::test]
async fn git_symlink_entries_are_not_materialized() {
    let git = require_git!();
    let g = git_fixture(git).await;
    // A symlink entry (mode 120000) added without needing symlink support on this file system.
    let blob = {
        let path = g.repo.dir.join("target.txt");
        fs::write(&path, "/etc/passwd").unwrap();
        g.repo.run(&["hash-object", "-w", "target.txt"])
    };
    fs::remove_file(g.repo.dir.join("target.txt")).unwrap();
    g.repo.run(&[
        "update-index",
        "--add",
        "--cacheinfo",
        &format!("120000,{blob},link"),
    ]);
    g.repo.run(&["commit", "-q", "-m", "link"]);
    let installed = g.install("node-l1", None).await;
    let link = g.fixture.cn().join(installed.name.as_str()).join("link");
    let meta = link.symlink_metadata().unwrap();
    assert!(
        !tree::is_link(&meta),
        "the entry must be a plain file, never a link"
    );
    tree::validate_tree(
        &pin_absolute(&g.fixture.cn().join(installed.name.as_str())).unwrap(),
        true,
    )
    .unwrap();
}

#[tokio::test]
async fn git_urls_must_be_https_without_credentials_when_local_remotes_are_not_allowed() {
    let git = require_git!();
    let fixture = Fixture::new().await;
    let strict = LocalNodeFiles {
        registry: fixture.files.registry.clone(),
        git: Arc::new(GitNodes::with_program(git, false)),
        recycler: fixture.files.recycler.clone(),
        limits: node_limits(),
    };
    for url in [
        "http://github.com/o/r",
        "ssh://git@github.com/o/r",
        "git@github.com:o/r.git",
        "file:///tmp/repo",
        "https://user:secret@github.com/o/r",
        "https://github.com/o/r?x=1",
        "https://github.com/o/r#x",
        "/tmp/local/repo",
        "-c core.sshCommand=evil",
        "https://github.com/o/..%2f..%2fevil",
    ] {
        let request = PrepareRequest::Git {
            repository: url.into(),
            reference: None,
        };
        let result = strict
            .prepare(&fixture.root, &node_id("node-url"), &request, None, &ctx())
            .await;
        assert_eq!(result.unwrap_err(), NodeAppError::UnsafeSource, "{url}");
    }
    assert!(fixture.owned().is_empty());
}

#[tokio::test]
async fn git_update_fast_forwards_in_place_and_never_discards_edits() {
    let git = require_git!();
    let g = git_fixture(git).await;
    let installed = g.install("node-f1", None).await;
    let live = g.fixture.cn().join(installed.name.as_str());
    // Up to date: nothing to do.
    assert_eq!(
        g.update("node-f2", &installed, None).await.unwrap_err(),
        NodeAppError::UpToDate
    );
    // Upstream moves forward.
    let new_head = g.repo.commit("extra.py", "y = 2", "three");
    let operation = g.update("node-f3", &installed, None).await.unwrap();
    assert_eq!(
        operation.node.source,
        NodeSource::Git {
            repository: g.url.clone(),
            commit: new_head.clone(),
            branch: Some("main".into())
        }
    );
    // The reviewed target is frozen even if upstream moves again before the commit.
    g.repo.commit("later.py", "z", "four");
    g.fixture
        .files
        .backup(&g.fixture.root, &operation)
        .await
        .unwrap();
    g.fixture
        .files
        .publish(&g.fixture.root, &operation, Default::default())
        .await
        .unwrap();
    assert_eq!(
        scan::read_git(&pin_absolute(&live).unwrap())
            .unwrap()
            .commit
            .as_deref(),
        Some(new_head.as_str())
    );
    assert!(live.join("extra.py").is_file() && !live.join("later.py").exists());
    assert_eq!(
        tree::tree_digest(&pin_absolute(&live).unwrap()).unwrap(),
        operation.node.tree_digest,
        "the live result is exactly the reviewed content"
    );
    g.fixture
        .files
        .discard_staging(&g.fixture.root, &node_id("node-f3"))
        .await
        .unwrap();
    assert!(g.fixture.owned().is_empty());
    // Dirty and untracked trees are refused and keep their edits.
    g.repo.commit("more.py", "m", "five");
    fs::write(live.join("__init__.py"), "my local edit").unwrap();
    let mut current = installed.clone();
    current.source = operation.node.source.clone();
    let status = refused_status(g.update("node-f4", &current, None).await.unwrap_err());
    assert!(status.contains("M __init__.py"), "{status}");
    assert_eq!(
        fs::read_to_string(live.join("__init__.py")).unwrap(),
        "my local edit"
    );
    assert!(g.fixture.owned().is_empty(), "a refusal stages nothing");
    g.repo.run(&["status", "--short"]);
    assert!(
        g.fixture
            .files
            .git_status(&g.fixture.root, &current, Default::default())
            .await
            .unwrap()
            .state
            .dirty
    );
    fs::write(live.join("__init__.py"), "NODE_CLASS_MAPPINGS = {}").unwrap();
    fs::write(live.join("untracked.txt"), "keep me").unwrap();
    let status = refused_status(g.update("node-f5", &current, None).await.unwrap_err());
    assert!(status.contains("?? untracked.txt"), "{status}");
    assert!(
        !status.contains(g.fixture.root.to_string_lossy().as_ref()),
        "the status is repository relative"
    );
    assert_eq!(
        fs::read_to_string(live.join("untracked.txt")).unwrap(),
        "keep me"
    );
    // The retained status is redacted and bounded.
    let secret = "node-git-refusal-secret-5521";
    crate::shared::logging::register_secret(secret);
    fs::write(live.join(format!("{secret}.txt")), "secret filename").unwrap();
    for index in 0..150 {
        fs::write(
            live.join(format!("refusal-file-{index:03}-long-relative-name.txt")),
            "x",
        )
        .unwrap();
    }
    let status = refused_status(g.update("node-f5b", &current, None).await.unwrap_err());
    assert!(status.chars().count() <= 2000);
    assert!(!status.contains(secret) && status.contains("[REDACTED]"));
    for entry in fs::read_dir(&live).unwrap().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.contains(secret) || name.starts_with("refusal-file-") {
            fs::remove_file(entry.path()).unwrap();
        }
    }
    // The publish step also re-checks the tree after review (edits made after the plan).
    fs::remove_file(live.join("untracked.txt")).unwrap();
    let planned = g.update("node-f6", &current, None).await.unwrap();
    fs::write(live.join("late-edit.txt"), "late").unwrap();
    let status = refused_status(
        g.fixture
            .files
            .publish(&g.fixture.root, &planned, Default::default())
            .await
            .unwrap_err(),
    );
    assert!(status.contains("?? late-edit.txt"), "{status}");
    assert_eq!(
        fs::read_to_string(live.join("late-edit.txt")).unwrap(),
        "late"
    );
    assert_eq!(
        scan::read_git(&pin_absolute(&live).unwrap())
            .unwrap()
            .commit
            .as_deref(),
        Some(new_head.as_str()),
        "a refused publish moves nothing"
    );
    fs::remove_file(live.join("late-edit.txt")).unwrap();
    // A head that moved after the plan makes the plan stale.
    let mut moved = planned.clone();
    if let Some(prior) = &mut moved.prior {
        prior.source = NodeSource::Git {
            repository: g.url.clone(),
            commit: "b".repeat(40),
            branch: Some("main".into()),
        };
    }
    assert_eq!(
        g.fixture
            .files
            .publish(&g.fixture.root, &moved, Default::default())
            .await
            .unwrap_err(),
        NodeAppError::StalePlan
    );
    g.fixture
        .files
        .publish(&g.fixture.root, &planned, Default::default())
        .await
        .unwrap();
    // The live result is exactly the reviewed content.
    assert_eq!(
        tree::tree_digest(&pin_absolute(&live).unwrap()).unwrap(),
        planned.node.tree_digest
    );
}

#[tokio::test]
async fn git_diverged_and_detached_nodes_need_an_explicit_target() {
    let git = require_git!();
    let g = git_fixture(git).await;
    let installed = g.install("node-d1", None).await;
    let live = g.fixture.cn().join(installed.name.as_str());
    // A local commit plus an upstream commit: diverged, refused.
    let local = Repo {
        dir: live.clone(),
        git: g.repo.git.clone(),
    };
    local.commit("local.py", "l", "local work");
    g.repo.commit("remote.py", "r", "remote work");
    let status = refused_status(g.update("node-d2", &installed, None).await.unwrap_err());
    assert!(status.contains("diverged"), "{status}");
    assert!(live.join("local.py").is_file());
    // A detached node (installed at a tag) has no upstream: an update needs a selected target.
    let first = g.repo.run(&["rev-list", "--max-parents=0", "HEAD"]);
    g.repo.run(&["tag", "v1", &first]);
    let tag_head = g.repo.commit("v2.py", "v2", "release two");
    g.repo.run(&["tag", "v2"]);
    g.fixture
        .files
        .discard_staging(&g.fixture.root, &node_id("node-d2"))
        .await
        .unwrap();
    fs::remove_dir_all(&live).unwrap();
    let detached = g.install("node-d3", Some("v1")).await;
    let status = refused_status(g.update("node-d4", &detached, None).await.unwrap_err());
    assert!(status.contains("detached"), "{status}");
    let operation = g.update("node-d5", &detached, Some("v2")).await.unwrap();
    assert_eq!(
        operation.node.source,
        NodeSource::Git {
            repository: g.url.clone(),
            commit: tag_head.clone(),
            branch: None
        }
    );
    g.fixture
        .files
        .publish(&g.fixture.root, &operation, Default::default())
        .await
        .unwrap();
    assert_eq!(
        scan::read_git(&pin_absolute(&live).unwrap())
            .unwrap()
            .commit
            .as_deref(),
        Some(tag_head.as_str())
    );
    g.fixture
        .files
        .discard_staging(&g.fixture.root, &node_id("node-d5"))
        .await
        .unwrap();
}

#[tokio::test]
async fn git_update_recovery_compares_the_head_with_the_recorded_commits() {
    let git = require_git!();
    let g = git_fixture(git).await;
    let installed = g.install("node-r1", None).await;
    let old_head = g.repo.run(&["rev-parse", "HEAD"]);
    g.repo.commit("next.py", "n", "next");
    let operation = g.update("node-r2", &installed, None).await.unwrap();
    // Crash before the merge: roll back (nothing moved), staged checkout removed.
    assert_eq!(
        g.fixture
            .files
            .recover(&g.fixture.root, &operation, Default::default())
            .await
            .unwrap(),
        RecoveryAction::RolledBack
    );
    assert!(g.fixture.owned().is_empty());
    let _ = old_head;
    // Crash after the merge: roll forward.
    g.fixture
        .files
        .publish(&g.fixture.root, &operation, Default::default())
        .await
        .unwrap();
    assert_eq!(
        g.fixture
            .files
            .recover(&g.fixture.root, &operation, Default::default())
            .await
            .unwrap(),
        RecoveryAction::RolledForward
    );
    // A head that matches neither recorded commit blocks.
    let mut odd = operation.clone();
    odd.node.source = NodeSource::Git {
        repository: g.url.clone(),
        commit: "c".repeat(40),
        branch: Some("main".into()),
    };
    if let Some(prior) = &mut odd.prior {
        prior.source = NodeSource::Git {
            repository: g.url.clone(),
            commit: "d".repeat(40),
            branch: Some("main".into()),
        };
    }
    assert_eq!(
        g.fixture
            .files
            .recover(&g.fixture.root, &odd, Default::default())
            .await
            .unwrap(),
        RecoveryAction::Blocked
    );
}

#[test]
fn git_commands_disable_hooks_and_every_protocol_but_https() {
    let git = GitNodes::with_program(PathBuf::from("git"), false);
    let root = Path::new("/root");
    let spec = git.spec_for_test(root);
    let joined = spec
        .args
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(" ");
    for needle in [
        "core.hooksPath=",
        "protocol.allow=never",
        "protocol.https.allow=always",
        "core.symlinks=false",
        "submodule.recurse=false",
        "credential.helper=",
    ] {
        assert!(joined.contains(needle), "{needle} missing: {joined}");
    }
    assert!(!joined.contains("protocol.file.allow=always"));
    assert!(
        spec.env
            .iter()
            .any(|(k, v)| k == "GIT_TERMINAL_PROMPT" && v == "0")
    );
}

#[tokio::test]
async fn registry_git_conflict_never_invokes_git() {
    let git = locate_git().expect("real Git is required");
    let fixture = Fixture::with(Some(git.clone()), node_limits()).await;
    let repo = Repo::new(&fixture.cn(), &git, "registry");
    repo.commit("node.py", "x = 1\n", "initial");
    let source = NodeSource::Registry {
        id: "registry".into(),
        publisher: None,
        version: "1".into(),
        repository: None,
    };
    super::provenance::write_marker(&repo.dir, "registry", &source, None).unwrap();
    super::git::TEST_SPECS.with(|slot| *slot.borrow_mut() = Some(Vec::new()));
    let nodes = fixture
        .files
        .scan(&fixture.root, Default::default())
        .await
        .unwrap();
    let calls = super::git::TEST_SPECS.with(|slot| slot.borrow_mut().take().unwrap());
    assert!(calls.is_empty());
    assert_eq!(nodes[0].source, source);
    assert!(nodes[0].conflict);
    assert_eq!(nodes[0].dirty, None);
    // A Manual marker also preserves its provenance in the presence of contradictory Git.
    super::provenance::write_marker(&repo.dir, "registry", &NodeSource::Manual, None).unwrap();
    let nodes = fixture
        .files
        .scan(&fixture.root, Default::default())
        .await
        .unwrap();
    assert_eq!(nodes[0].source, NodeSource::Manual);
    assert!(nodes[0].conflict);
}

#[tokio::test]
async fn manual_and_disabled_git_nodes_receive_safe_status() {
    let git = locate_git().expect("real Git is required");
    let fixture = Fixture::with(Some(git.clone()), node_limits()).await;
    fs::create_dir_all(fixture.disabled()).unwrap();
    for (parent, enabled) in [(fixture.cn(), true), (fixture.disabled(), false)] {
        let repo = Repo::new(&parent, &git, if enabled { "manual" } else { "disabled" });
        let commit = repo.commit("node.py", "x = 1\n", "initial");
        repo.run(&[
            "remote",
            "add",
            "origin",
            "https://github.com/example/node.git",
        ]);
        fs::write(repo.dir.join("node.py"), "x = 2\n").unwrap();
        let nodes = fixture
            .files
            .scan(&fixture.root, Default::default())
            .await
            .unwrap();
        let node = nodes.iter().find(|node| node.enabled == enabled).unwrap();
        assert!(!node.conflict);
        assert_eq!(
            node.source,
            NodeSource::Git {
                repository: "https://github.com/example/node".into(),
                commit: commit.clone(),
                branch: Some("main".into())
            }
        );
        assert_eq!(node.dirty, Some(true));
        let status = fixture
            .files
            .git_status(&fixture.root, node, Default::default())
            .await
            .unwrap();
        assert_eq!(status.revision.commit, commit);
        assert_eq!(status.revision.branch.as_deref(), Some("main"));
        assert!(status.state.dirty);
    }
}

#[tokio::test]
async fn git_scan_never_executes_repository_filters() {
    let git = locate_git().expect("real Git is required");
    for driver in ["clean", "process"] {
        for kind in ["marked", "manual", "disabled"] {
            let fixture = Fixture::with(Some(git.clone()), node_limits()).await;
            fs::create_dir_all(fixture.disabled()).unwrap();
            let repo = Repo::new(
                &if kind == "disabled" {
                    fixture.disabled()
                } else {
                    fixture.cn()
                },
                &git,
                "filter-node",
            );
            let commit = repo.commit("node.py", "x = 1\n", "initial");
            repo.run(&[
                "remote",
                "add",
                "origin",
                "https://github.com/example/node.git",
            ]);
            if kind == "marked" {
                super::provenance::write_marker(
                    &repo.dir,
                    "filter-node",
                    &NodeSource::Git {
                        repository: "https://github.com/example/node".into(),
                        commit,
                        branch: Some("main".into()),
                    },
                    None,
                )
                .unwrap();
            }
            let sentinel = fixture._directory.path().join("filter-executed");
            let command = format!(
                "printf FILTER > '{}'; exit 1",
                sentinel.to_string_lossy().replace('\\', "/")
            );
            repo.run(&["config", &format!("filter.evil.{driver}"), &command]);
            fs::write(repo.dir.join(".gitattributes"), "*.py filter=evil\n").unwrap();
            fs::write(repo.dir.join("node.py"), "x = 2\n").unwrap();
            let nodes = fixture
                .files
                .scan(&fixture.root, Default::default())
                .await
                .unwrap();
            assert_eq!(nodes[0].dirty, None, "{kind}/{driver}");
            assert!(!sentinel.exists(), "scan ran {kind}/{driver}");
            assert!(matches!(
                fixture
                    .files
                    .git_status(&fixture.root, &nodes[0], Default::default())
                    .await,
                Err(NodeAppError::GitRefused { .. })
            ));
            assert!(!sentinel.exists(), "explicit status ran {kind}/{driver}");
            // Control: this is executable Git configuration, not an inert fixture.
            let _ = Command::new(&git)
                .arg("-C")
                .arg(&repo.dir)
                .args(["status", "--porcelain"])
                .output()
                .unwrap();
            assert!(
                sentinel.exists(),
                "malicious {driver} fixture did not execute in the control"
            );
        }
    }
}

#[tokio::test]
async fn git_inspection_refuses_includes_and_worktree_redirection() {
    let g = git_fixture(locate_git().expect("real Git is required")).await;
    let installed = g.install("config-refusal", None).await;
    let repo = Repo {
        dir: g.fixture.cn().join(installed.name.as_str()),
        git: g.repo.git.clone(),
    };
    let head = repo.run(&["rev-parse", "HEAD"]);
    for (key, value) in [
        ("include.path", "missing-config"),
        ("includeIf.gitdir:/.path", "missing-config"),
        ("extensions.worktreeConfig", "true"),
        ("core.worktree", ".."),
    ] {
        repo.run(&["config", "--local", key, value]);
        let error = g
            .fixture
            .files
            .git_status(&g.fixture.root, &installed, Default::default())
            .await
            .unwrap_err();
        assert_eq!(
            error,
            NodeAppError::GitRefused {
                status: "Repository configuration requires external inspection.".into()
            }
        );
        assert_eq!(
            g.update("unsafe-update", &installed, None)
                .await
                .unwrap_err(),
            error
        );
        // --local --no-includes is safe even while the repository is refused.
        let output = Command::new(&repo.git)
            .arg("-C")
            .arg(&repo.dir)
            .args(["config", "--local", "--no-includes", "--unset", key])
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(repo.run(&["rev-parse", "HEAD"]), head);
    }
    fs::write(repo.dir.join(".git/commondir"), "../external").unwrap();
    assert!(matches!(
        g.fixture
            .files
            .git_status(&g.fixture.root, &installed, Default::default())
            .await,
        Err(NodeAppError::GitRefused { .. })
    ));
}

#[tokio::test]
async fn git_failures_do_not_expose_stderr_or_absolute_paths() {
    let g = git_fixture(locate_git().expect("real Git is required")).await;
    let paths = Arc::new(super::paths::NodePaths::open(&g.fixture.root, true).unwrap());
    let bound = g
        .fixture
        .files
        .git
        .bound(paths.clone(), vec![], Default::default())
        .unwrap();
    let message = format!(
        "{} https://user:credential-secret@example.invalid/private",
        g.fixture.root.display()
    );
    // Git itself echoes an unknown command in stderr, including its supplied sensitive text.
    let error = bound
        .run(
            &paths.root.anchored_path(),
            &paths.hooks(),
            None,
            &[&message],
            &CancelToken::new(),
            None,
            std::time::Duration::from_secs(20),
        )
        .await
        .unwrap_err();
    assert_eq!(
        error,
        NodeAppError::Failed("The Git command failed.".into())
    );
    assert!(!error.to_string().contains("credential-secret"));
    assert!(
        !error
            .to_string()
            .contains(&g.fixture.root.to_string_lossy().to_string())
    );
}

#[tokio::test]
async fn git_commands_retain_all_descriptor_owners() {
    use application::shared::{GateError, MaintenanceGate};
    let g = git_fixture(locate_git().expect("real Git is required")).await;
    let gate = crate::shared::gate::TokioMaintenanceGate::new();
    let lease = gate.acquire().await.unwrap();
    let step = StepContext {
        retained: lease.retained(),
        ..ctx()
    };
    super::git::TEST_SPECS.with(|slot| *slot.borrow_mut() = Some(Vec::new()));
    let id = node_id("retained-install");
    let staged = g
        .fixture
        .files
        .prepare(&g.fixture.root, &id, &g.request(None), None, &step)
        .await
        .unwrap();
    let install_specs = super::git::TEST_SPECS.with(|slot| slot.borrow_mut().take().unwrap());
    for command in ["ls-remote", "clone"] {
        assert!(
            install_specs
                .iter()
                .any(|spec| spec.args.iter().any(|arg| arg == command))
        );
    }
    assert!(install_specs.iter().all(|spec| !spec.retained.is_empty()));
    // Actual completed commands release their pins before publication moves staging.
    drop(install_specs);
    super::git::TEST_SPECS.with(|slot| *slot.borrow_mut() = Some(Vec::new()));
    let mut installed = node(staged.name.as_str(), staged.source, true);
    installed.tree_digest = staged.tree_digest;
    let op = operation(
        OperationKind::Install,
        id.as_str(),
        installed.clone(),
        None,
        OperationPhase::PublishIntent,
    );
    g.fixture
        .files
        .publish(&g.fixture.root, &op, lease.retained())
        .await
        .unwrap();
    g.fixture
        .files
        .git_status(&g.fixture.root, &installed, lease.retained())
        .await
        .unwrap();
    g.repo.commit("new.py", "new = True\n", "update");
    let request = PrepareRequest::Update {
        node: installed.id.clone(),
        reference: None,
    };
    let staged = g
        .fixture
        .files
        .prepare(
            &g.fixture.root,
            &node_id("retained-update"),
            &request,
            Some(&installed),
            &step,
        )
        .await
        .unwrap();
    let mut updated = installed.clone();
    updated.source = staged.source;
    updated.tree_digest = staged.tree_digest;
    let op = operation(
        OperationKind::Update,
        "retained-update",
        updated,
        Some(installed.clone()),
        OperationPhase::PublishIntent,
    );
    g.fixture
        .files
        .publish(&g.fixture.root, &op, lease.retained())
        .await
        .unwrap();
    let before_recovery =
        super::git::TEST_SPECS.with(|slot| slot.borrow_mut().replace(Vec::new()).unwrap());
    assert!(
        before_recovery
            .iter()
            .any(|spec| spec.args.iter().any(|arg| arg == "merge"))
    );
    drop(before_recovery);
    // Recovery's commands are recorded without their owners: holding them would keep the node
    // pinned while recovery settles it. Whether each carried owners is still recorded.
    super::git::TEST_RELEASE_OWNERS.with(|flag| flag.set(true));
    g.fixture
        .files
        .recover(&g.fixture.root, &op, lease.retained())
        .await
        .unwrap();
    super::git::TEST_RELEASE_OWNERS.with(|flag| flag.set(false));
    let recovered =
        super::git::TEST_SPECS.with(|slot| slot.borrow_mut().replace(Vec::new()).unwrap());
    assert!(
        recovered
            .iter()
            .any(|spec| spec.args.iter().any(|arg| arg == "status")),
        "recovery inspects the repository"
    );
    // Held commands keep every owner alive until they are dropped.
    g.fixture
        .files
        .git_status(&g.fixture.root, &installed, lease.retained())
        .await
        .unwrap();
    let held = super::git::TEST_SPECS.with(|slot| slot.borrow_mut().take().unwrap());
    assert!(!held.is_empty());
    for spec in recovered.iter().chain(&held) {
        assert!(!spec.retained.is_empty());
        assert!(
            spec.env
                .iter()
                .any(|(key, value)| key == "GIT_CONFIG_NOSYSTEM" && value == "1")
        );
    }
    for spec in &held {
        let config = spec
            .env
            .iter()
            .find(|(key, _)| key == "GIT_CONFIG_GLOBAL")
            .unwrap();
        assert_eq!(fs::read(Path::new(&config.1)).unwrap(), b"");
    }
    drop(step);
    drop(lease);
    assert_eq!(gate.try_acquire().await.err(), Some(GateError::Busy));
    #[cfg(windows)]
    assert!(fs::rename(&g.fixture.root, g.fixture.root.with_extension("moved")).is_err());
    drop(held);
    assert!(gate.try_acquire().await.is_ok());
}

// Operation-time containment. Windows uses junctions, which do not require symlink privilege.
#[cfg(windows)]
fn directory_link(target: &Path, link: &Path) -> bool {
    let output = Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .output()
        .expect("cmd must be available on Windows");
    if output.status.success() {
        return true;
    }
    let message = String::from_utf8_lossy(&output.stderr).to_lowercase();
    assert!(
        message.contains("denied") || message.contains("privilege"),
        "junction creation failed unexpectedly: {message}"
    );
    eprintln!("skipped: directory junction creation is not permitted: {message}");
    false
}

#[cfg(unix)]
fn directory_link(target: &Path, link: &Path) -> bool {
    std::os::unix::fs::symlink(target, link).expect("temporary directory must permit symlinks");
    true
}

#[cfg(windows)]
fn remove_directory_link(link: &Path) {
    fs::remove_dir(link).unwrap();
}
#[cfg(unix)]
fn remove_directory_link(link: &Path) {
    fs::remove_file(link).unwrap();
}

#[cfg(any(windows, unix))]
#[tokio::test]
async fn linked_comfyui_and_disabled_parents_are_refused() {
    for parent in ["ComfyUI", "ComfyUI/custom_nodes/.disabled"] {
        let fixture = Fixture::new().await;
        drop(super::paths::NodePaths::open(&fixture.root, true).unwrap());
        let outside = fixture.root.join("outside");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("sentinel.txt"), "external contents").unwrap();
        let original = fixture.root.join(parent);
        let saved = fixture.root.join("saved-parent");
        fs::rename(&original, &saved).unwrap();
        if !directory_link(&outside, &original) {
            return;
        }
        let prior = node("pack", NodeSource::Manual, true);
        let op = operation(
            OperationKind::Uninstall,
            "node-linked-parent",
            prior.clone(),
            Some(prior.clone()),
            OperationPhase::OldBackedUp,
        );
        let request = PrepareRequest::Registry {
            id: "pack".into(),
            version: None,
        };
        assert_eq!(
            fixture
                .files
                .prepare(&fixture.root, &op.id, &request, None, &ctx())
                .await
                .unwrap_err(),
            NodeAppError::UnsafeSource
        );
        assert_eq!(
            fixture
                .files
                .publish(&fixture.root, &op, Default::default())
                .await
                .unwrap_err(),
            NodeAppError::UnsafeSource
        );
        assert_eq!(
            fixture.files.backup(&fixture.root, &op).await.unwrap_err(),
            NodeAppError::UnsafeSource
        );
        assert_eq!(
            fixture
                .files
                .move_enabled(&fixture.root, &prior, false)
                .await
                .unwrap_err(),
            NodeAppError::UnsafeSource
        );
        assert_eq!(
            fixture
                .files
                .discard_staging(&fixture.root, &op.id)
                .await
                .unwrap_err(),
            NodeAppError::UnsafeSource
        );
        assert_eq!(
            fixture.files.recycle(&fixture.root, &op).await.unwrap_err(),
            NodeAppError::UnsafeSource
        );
        assert_eq!(
            fixture
                .files
                .recover(&fixture.root, &op, Default::default())
                .await
                .unwrap_err(),
            NodeAppError::UnsafeSource
        );
        assert_eq!(
            fs::read_to_string(outside.join("sentinel.txt")).unwrap(),
            "external contents"
        );
        assert_eq!(fs::read_dir(&outside).unwrap().count(), 1);
        remove_directory_link(&original);
        fs::rename(&saved, &original).unwrap();
    }
}

#[cfg(any(windows, unix))]
#[tokio::test]
async fn node_mutations_resist_parent_substitution() {
    for action in ["publication", "restore", "cleanup", "recycling"] {
        let fixture = Fixture::new().await;
        drop(super::paths::NodePaths::open(&fixture.root, true).unwrap());
        let id = "node-substitution";
        dirs(
            &fixture,
            &[
                ".disabled/.vjump-staging-node-substitution",
                ".disabled/.vjump-backup-node-substitution",
            ],
        );
        let outside = fixture.root.join("outside");
        fs::create_dir_all(outside.join(".disabled/.vjump-staging-node-substitution")).unwrap();
        fs::create_dir_all(outside.join(".disabled/.vjump-backup-node-substitution")).unwrap();
        fs::write(outside.join("sentinel.txt"), "outside").unwrap();
        for entry in [
            ".vjump-staging-node-substitution",
            ".vjump-backup-node-substitution",
        ] {
            fs::write(
                outside.join(".disabled").join(entry).join("keep.txt"),
                "external copy",
            )
            .unwrap();
        }
        let before = tree::tree_digest(&pin_absolute(&outside).unwrap()).unwrap();
        // Establish junction availability separately: a sharing refusal below must be evidence
        // that a retained pin stopped substitution, never a silent privilege-related skip.
        let probe = fixture.root.join("junction-probe");
        if !directory_link(&outside, &probe) {
            return;
        }
        remove_directory_link(&probe);
        let cn = fixture.cn();
        let saved = fixture.root.join("saved-custom-nodes");
        let substituted = Arc::new(Mutex::new(false));
        let changed = substituted.clone();
        let (boundary_cn, boundary_saved, boundary_outside) =
            (cn.clone(), saved.clone(), outside.clone());
        super::paths::after_open(move || match fs::rename(&boundary_cn, &boundary_saved) {
            Ok(()) => {
                assert!(directory_link(&boundary_outside, &boundary_cn));
                *changed.lock().unwrap() = true;
            }
            Err(error) => {
                #[cfg(windows)]
                assert!(
                    matches!(error.raw_os_error(), Some(5 | 32)),
                    "unexpected parent rename error: {error}"
                );
                #[cfg(unix)]
                panic!("Unix substitution must execute: {error}");
            }
        });
        let prior = node("pack", NodeSource::Manual, true);
        let mut op = operation(
            OperationKind::Install,
            id,
            prior.clone(),
            None,
            OperationPhase::PublishIntent,
        );
        match action {
            "publication" => fixture
                .files
                .publish(&fixture.root, &op, Default::default())
                .await
                .unwrap(),
            "restore" => {
                op.kind = OperationKind::Uninstall;
                op.prior = Some(prior.clone());
                op.phase = OperationPhase::OldBackedUp;
                assert_eq!(
                    fixture
                        .files
                        .recover(&fixture.root, &op, Default::default())
                        .await
                        .unwrap(),
                    RecoveryAction::RolledBack
                );
            }
            "cleanup" => fixture
                .files
                .discard_staging(&fixture.root, &op.id)
                .await
                .unwrap(),
            "recycling" => fixture.files.recycle(&fixture.root, &op).await.unwrap(),
            _ => unreachable!(),
        }
        assert_eq!(
            tree::tree_digest(&pin_absolute(&outside).unwrap()).unwrap(),
            before,
            "{action} changed the substituted tree"
        );
        let moved = *substituted.lock().unwrap();
        #[cfg(windows)]
        assert!(
            !moved,
            "Windows directory pins must deny parent substitution"
        );
        #[cfg(unix)]
        assert!(moved, "Unix substitution must actually execute");
        let protected = if moved { &saved } else { &cn };
        match action {
            "publication" | "restore" => assert!(protected.join("pack/__init__.py").is_file()),
            "cleanup" => assert!(
                !protected
                    .join(".disabled/.vjump-staging-node-substitution")
                    .exists()
            ),
            "recycling" => assert!(
                !protected
                    .join(".disabled/.vjump-backup-node-substitution")
                    .exists()
            ),
            _ => unreachable!(),
        }
        if moved {
            remove_directory_link(&cn);
            fs::rename(&saved, &cn).unwrap();
        }
        eprintln!(
            "{action}: parent substitution {}",
            if moved {
                "executed against retained pins"
            } else {
                "refused by retained Windows pins"
            }
        );
    }
}

#[test]
fn pinned_directory_renames_never_replace_a_destination() {
    use crate::shared::model_paths::{pin_absolute, rename_no_replace};
    let temp = tempfile::tempdir().unwrap();
    let parent = pin_absolute(temp.path()).unwrap();
    for directory in [true, false] {
        let prefix = if directory { "directory" } else { "file" };
        let source = SafeFileName::new(format!("{prefix}-source")).unwrap();
        let destination = SafeFileName::new(format!("{prefix}-destination")).unwrap();
        let published = SafeFileName::new(format!("{prefix}-published")).unwrap();
        if directory {
            fs::create_dir(temp.path().join(source.as_str())).unwrap();
            fs::create_dir(temp.path().join(destination.as_str())).unwrap();
            fs::write(
                temp.path().join(destination.as_str()).join("keep"),
                "destination",
            )
            .unwrap();
        } else {
            fs::write(temp.path().join(source.as_str()), "source").unwrap();
            fs::write(temp.path().join(destination.as_str()), "destination").unwrap();
        }
        let file = parent.open_entry(&source).unwrap();
        assert_eq!(
            rename_no_replace(&parent, &source, &file, &parent, &destination)
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::AlreadyExists
        );
        let sentinel = if directory {
            temp.path().join(destination.as_str()).join("keep")
        } else {
            temp.path().join(destination.as_str())
        };
        assert_eq!(fs::read_to_string(sentinel).unwrap(), "destination");
        rename_no_replace(&parent, &source, &file, &parent, &published).unwrap();
        assert!(!temp.path().join(source.as_str()).exists());
        assert!(temp.path().join(published.as_str()).exists());
        if directory {
            assert!(parent.open(&published, false, false).is_err());
        }
    }
}

#[tokio::test]
async fn failed_safe_recycling_retains_the_backup() {
    let mut fixture = Fixture::new().await;
    drop(super::paths::NodePaths::open(&fixture.root, true).unwrap());
    dirs(&fixture, &[".disabled/.vjump-backup-node-retain"]);
    let backup = fixture.disabled().join(".vjump-backup-node-retain");
    let contents = fs::read(backup.join("__init__.py")).unwrap();
    fixture.files.recycler = Arc::new(|parent, name, expected| {
        assert_eq!(
            crate::shared::model_paths::identity(&parent.open_entry(name)?)?,
            *expected
        );
        Err(std::io::ErrorKind::Unsupported.into())
    });
    let prior = node("pack", NodeSource::Manual, true);
    let op = operation(
        OperationKind::Uninstall,
        "node-retain",
        prior.clone(),
        Some(prior),
        OperationPhase::CleanupPending,
    );
    assert_eq!(
        fixture.files.recycle(&fixture.root, &op).await.unwrap_err(),
        NodeAppError::Filesystem
    );
    assert_eq!(
        fixture
            .files
            .recover(&fixture.root, &op, Default::default())
            .await
            .unwrap(),
        RecoveryAction::CleanupPending
    );
    assert_eq!(fs::read(backup.join("__init__.py")).unwrap(), contents);
    // The real recycler also rejects a mismatched identity before invoking any OS trash API.
    let paths = super::paths::NodePaths::open(&fixture.root, false).unwrap();
    let (parent, name) = paths.backup(&op.id);
    let mut wrong =
        crate::shared::model_paths::identity(&parent.open_entry(&name).unwrap()).unwrap();
    wrong.platform_id.push_str("-changed");
    assert!(crate::shared::trash::recycle_entry(&parent, &name, &wrong).is_err());
    assert_eq!(fs::read(backup.join("__init__.py")).unwrap(), contents);
}

#[cfg(unix)]
#[tokio::test]
async fn node_root_traversal_never_follows_an_intermediate_symlink() {
    let fixture = Fixture::new().await;
    let alias = fixture._directory.path().join("alias");
    std::os::unix::fs::symlink(&fixture.root, &alias).unwrap();
    assert!(matches!(
        super::paths::NodePaths::open(&alias, true),
        Err(NodeAppError::UnsafeSource)
    ));
    let nested = alias.join("ComfyUI/custom_nodes");
    assert!(crate::shared::model_paths::pin_absolute(&nested).is_err());
}

// ---------------------------------------------------------------------------------------
// Execution guards: pinned locations whose requirements and script are read once
// ---------------------------------------------------------------------------------------

fn write_node(fixture: &Fixture, name: &str, files: &[(&str, &[u8])]) -> InstalledNode {
    let directory = fixture.cn().join(name);
    fs::create_dir_all(&directory).unwrap();
    for (file, body) in files {
        fs::write(directory.join(file), body).unwrap();
    }
    node(name, NodeSource::Manual, true)
}

fn locate_tool(name: &str) -> Option<PathBuf> {
    let file = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_owned()
    };
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(&file))
        .find(|candidate| candidate.is_file())
}

/// A pip-less virtual environment of the machine's Python; it needs no network.
fn make_venv(python: &Path, venv: &Path) {
    let status = Command::new(python)
        .args(["-m", "venv", "--without-pip"])
        .arg(venv)
        .status()
        .expect("python starts");
    assert!(status.success(), "the virtual environment was not created");
}

fn wheel_bytes(name: &str, version: &str) -> Vec<u8> {
    let (module, metadata, wheel, record) = (
        format!("{name}.py"),
        format!("{name}-{version}.dist-info/METADATA"),
        format!("{name}-{version}.dist-info/WHEEL"),
        format!("{name}-{version}.dist-info/RECORD"),
    );
    let text = format!("Metadata-Version: 2.1\nName: {name}\nVersion: {version}\n");
    zip_bytes(&[
        (&module, b"VALUE = 1\n"),
        (&metadata, text.as_bytes()),
        (
            &wheel,
            b"Wheel-Version: 1.0\nGenerator: test\nRoot-Is-Purelib: true\nTag: py3-none-any\n",
        ),
        (&record, b""),
    ])
}

#[tokio::test]
async fn node_preview_never_executes_editable_build_backend() {
    use crate::environment::{
        layout::{Layout, ensure_cache_dirs},
        packages::UvPackages,
    };
    use application::environment::{
        GuardedRequirements, InstallSpec, PackageInstaller, ProvisionError,
    };
    let python = locate_tool("python").expect("real Python is required");
    let uv = locate_tool("uv").expect("real uv is required");
    let version = Command::new(&uv).arg("--version").output().unwrap();
    assert!(version.status.success());
    eprintln!(
        "preview tool: {}",
        String::from_utf8_lossy(&version.stdout).trim()
    );
    let fixture = Fixture::new().await;
    let layout = Layout::new(&fixture.root);
    ensure_cache_dirs(&layout).unwrap();
    fs::create_dir_all(layout.uv_dir()).unwrap();
    fs::copy(uv, layout.uv_exe()).unwrap();
    let venv = fixture.root.join("envs/preview");
    make_venv(&python, &venv);
    let source = fixture.cn().join("source");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("pyproject.toml"), "[build-system]\nrequires = []\nbuild-backend = 'backend'\nbackend-path = ['.']\n[project]\nname = 'malicious-preview'\ndynamic = ['version']\n").unwrap();
    let sentinel = source.join("backend-executed");
    fs::write(source.join("backend.py"), "from pathlib import Path\nPath(__file__).with_name('backend-executed').write_text('ran')\nraise RuntimeError('backend ran before consent')\n").unwrap();
    let inventory = || {
        let mut files = walkdir::WalkDir::new(&venv)
            .into_iter()
            .map(|entry| {
                let entry = entry.unwrap();
                (
                    entry.path().strip_prefix(&venv).unwrap().to_path_buf(),
                    entry.metadata().unwrap().len(),
                )
            })
            .collect::<Vec<_>>();
        files.sort();
        files
    };
    let before = inventory();
    for (index, text) in [
        "-e ../source\n",
        "--editable=../source\n",
        "../source\n",
        "malicious-preview @ ../source\n",
        "-r nested.txt\n",
        "--unknown-option ../source\n",
    ]
    .iter()
    .enumerate()
    {
        let name = format!("preview-{index}");
        let node = write_node(
            &fixture,
            &name,
            &[
                ("requirements.txt", text.as_bytes()),
                ("nested.txt", b"-r deeper.txt\n"),
                ("deeper.txt", b"--editable ../source\n"),
            ],
        );
        let guard = fixture
            .files
            .pin_location(&fixture.root, &node, None)
            .await
            .unwrap();
        let path = guard.directory().join("requirements.txt");
        let spec = InstallSpec {
            requirement_files: vec![path.clone()],
            guarded: Some(GuardedRequirements {
                path,
                directory: guard.directory().to_owned(),
                text: guard.requirements().unwrap(),
                retained: guard.retained(),
            }),
            ..Default::default()
        };
        let error = UvPackages::new()
            .preview_install(&fixture.root, &venv, &spec, &ctx())
            .await
            .unwrap_err();
        assert!(
            matches!(error, ProvisionError::Failed(ref text) if text.contains("external inspection")),
            "{error:?}"
        );
        assert!(!sentinel.exists(), "backend ran for {text}");
        assert_eq!(inventory(), before);
    }
    let spec = InstallSpec {
        requirements: vec![format!("--editable={}", source.display())],
        ..Default::default()
    };
    assert!(
        UvPackages::new()
            .preview_install(&fixture.root, &venv, &spec, &ctx())
            .await
            .is_err()
    );
    assert!(!sentinel.exists());
    assert_eq!(inventory(), before);
}

#[cfg(windows)]
fn assert_sharing_refusal(error: &std::io::Error) {
    // ERROR_ACCESS_DENIED / ERROR_SHARING_VIOLATION: the pin, not a missing privilege.
    assert!(
        matches!(error.raw_os_error(), Some(5 | 32)),
        "unexpected substitution error: {error}"
    );
}

#[cfg(any(windows, unix))]
#[tokio::test]
async fn node_subprocess_inputs_resist_parent_and_leaf_substitution() {
    for phase in ["acquisition", "execution"] {
        let fixture = Fixture::new().await;
        let node = write_node(
            &fixture,
            "pack",
            &[
                ("__init__.py", b""),
                ("origin.txt", b"reviewed"),
                ("requirements.txt", b"numpy\n"),
                ("install.py", b"print('reviewed')\n"),
            ],
        );
        let outside = fixture.root.join("outside");
        fs::create_dir_all(outside.join("pack")).unwrap();
        for (file, body) in [
            ("origin.txt", "decoy"),
            ("requirements.txt", "decoy-package\n"),
            ("install.py", "print('decoy')\n"),
        ] {
            fs::write(outside.join("pack").join(file), body).unwrap();
        }
        fs::write(outside.join("sentinel.txt"), "external contents").unwrap();
        let before = tree::tree_digest(&pin_absolute(&outside).unwrap()).unwrap();
        // Establish link availability separately: a sharing refusal below must be evidence of a
        // retained pin, never a silent privilege-related skip.
        let probe = fixture.root.join("junction-probe");
        if !directory_link(&outside, &probe) {
            return;
        }
        remove_directory_link(&probe);
        let (cn, saved) = (fixture.cn(), fixture.root.join("saved-custom-nodes"));
        let swapped = Arc::new(Mutex::new(false));
        let swap = {
            let (cn, saved, outside, swapped) =
                (cn.clone(), saved.clone(), outside.clone(), swapped.clone());
            move || match fs::rename(&cn, &saved) {
                Ok(()) => {
                    assert!(directory_link(&outside, &cn));
                    *swapped.lock().unwrap() = true;
                }
                Err(error) => {
                    #[cfg(windows)]
                    assert_sharing_refusal(&error);
                    #[cfg(unix)]
                    panic!("Unix substitution must execute: {error}");
                }
            }
        };
        if phase == "acquisition" {
            // Between pinning the parents and opening the node directory.
            super::paths::after_open(swap.clone());
        }
        let guard = fixture
            .files
            .pin_location(&fixture.root, &node, None)
            .await
            .expect("the node location is pinned");
        if phase == "execution" {
            swap();
        }
        let moved = *swapped.lock().unwrap();
        #[cfg(windows)]
        assert!(!moved, "Windows pins must deny parent substitution");
        #[cfg(unix)]
        assert!(moved, "Unix substitution must actually execute");
        // Leaf substitution of the selected inputs.
        let leaf = |name: &str| guard.directory().join(name);
        #[cfg(windows)]
        {
            for name in ["requirements.txt", "install.py"] {
                assert_sharing_refusal(&fs::write(leaf(name), "decoy").unwrap_err());
                assert_sharing_refusal(&fs::remove_file(leaf(name)).unwrap_err());
                assert_sharing_refusal(&fs::rename(leaf(name), leaf("moved.txt")).unwrap_err());
            }
            assert_sharing_refusal(
                &fs::rename(fixture.cn().join("pack"), fixture.root.join("moved-pack"))
                    .unwrap_err(),
            );
            // Readers such as uv and Python are still admitted.
            assert_eq!(fs::read(leaf("requirements.txt")).unwrap(), b"numpy\n");
        }
        #[cfg(unix)]
        for (name, decoy) in [
            ("requirements.txt", "decoy-package\n"),
            ("install.py", "print('decoy')\n"),
        ] {
            fs::remove_file(leaf(name)).unwrap();
            fs::write(leaf(name), decoy).unwrap();
            assert_eq!(fs::read_to_string(leaf(name)).unwrap(), decoy);
        }
        // Whatever happened to the names, the guard still serves the reviewed location and bytes.
        assert_eq!(fs::read(leaf("origin.txt")).unwrap(), b"reviewed");
        assert_eq!(guard.requirements().as_deref(), Some(&b"numpy\n"[..]));
        assert_eq!(
            guard.install_script().as_deref(),
            Some(&b"print('reviewed')\n"[..])
        );
        assert_eq!(
            guard.snapshot().requirements_digest.as_deref(),
            Some(crate::shared::download::sha256_hex(b"numpy\n").as_str())
        );
        assert_eq!(
            tree::tree_digest(&pin_absolute(&outside).unwrap()).unwrap(),
            before
        );
        assert_eq!(
            fs::read_to_string(outside.join("sentinel.txt")).unwrap(),
            "external contents"
        );
        drop(guard);
        if moved {
            remove_directory_link(&cn);
            fs::rename(&saved, &cn).unwrap();
        }
        eprintln!(
            "{phase}: parent substitution {}",
            if moved {
                "executed against retained pins"
            } else {
                "refused by retained Windows pins"
            }
        );
    }
}

fn file_link(target: &Path, link: &Path) -> bool {
    #[cfg(windows)]
    let made = std::os::windows::fs::symlink_file(target, link);
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(target, link);
    match made {
        Ok(()) => true,
        // ERROR_PRIVILEGE_NOT_HELD: file symlinks need Developer Mode or elevation on Windows.
        Err(error) if error.raw_os_error() == Some(1314) => {
            eprintln!("skipped: file symlink creation is not permitted: {error}");
            false
        }
        Err(error) => panic!("link creation failed unexpectedly: {error}"),
    }
}

#[cfg(any(windows, unix))]
#[tokio::test]
async fn node_execution_refuses_linked_requirements_and_hooks() {
    let fixture = Fixture::new().await;
    let outside = fixture.root.join("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("sentinel.txt"), "external contents").unwrap();
    let refused = |result: Result<Box<dyn NodeLocationGuard>, NodeAppError>| {
        assert_eq!(result.err(), Some(NodeAppError::UnsafeSource));
    };
    for (name, node_name) in [
        ("requirements.txt", "linked-requirements"),
        ("install.py", "linked-hook"),
    ] {
        let linked = write_node(&fixture, node_name, &[("__init__.py", b"")]);
        let link = fixture.cn().join(node_name).join(name);
        if file_link(&outside.join("sentinel.txt"), &link) {
            refused(
                fixture
                    .files
                    .pin_location(&fixture.root, &linked, None)
                    .await,
            );
        }
        // A directory link (a junction on Windows, which needs no privilege) in place of the
        // input is a reparse point or symlink too.
        let node_name = format!("{node_name}-directory-link");
        let linked = write_node(&fixture, &node_name, &[("__init__.py", b"")]);
        let link = fixture.cn().join(&node_name).join(name);
        if directory_link(&outside, &link) {
            refused(
                fixture
                    .files
                    .pin_location(&fixture.root, &linked, None)
                    .await,
            );
            remove_directory_link(&link);
        }
    }
    // A linked node directory is refused whether or not file links are permitted.
    let linked_dir = node("linked-dir", NodeSource::Manual, true);
    let link = fixture.cn().join("linked-dir");
    if directory_link(&outside, &link) {
        refused(
            fixture
                .files
                .pin_location(&fixture.root, &linked_dir, None)
                .await,
        );
        remove_directory_link(&link);
    }
    // A directory in place of an input is not a regular file either.
    let special = write_node(&fixture, "special", &[("__init__.py", b"")]);
    fs::create_dir(fixture.cn().join("special").join("requirements.txt")).unwrap();
    refused(
        fixture
            .files
            .pin_location(&fixture.root, &special, None)
            .await,
    );
    assert_eq!(
        fs::read_to_string(outside.join("sentinel.txt")).unwrap(),
        "external contents"
    );
    assert_eq!(fs::read_dir(&outside).unwrap().count(), 1);
}

#[tokio::test]
async fn guarded_requirements_preserve_relative_includes_and_local_paths() {
    use crate::{
        environment::{
            layout::{Layout, Tool, command, ensure_cache_dirs, venv_python},
            packages::{install_args, with_guarded},
        },
        shared::process::{DiscardSink, RunLimits, run},
    };
    use application::environment::{GuardedRequirements, InstallSpec};
    let (Some(uv), Some(python)) = (locate_tool("uv"), locate_tool("python")) else {
        eprintln!("skipped: uv and python are required");
        return;
    };
    let fixture = Fixture::new().await;
    let layout = Layout::new(&fixture.root);
    ensure_cache_dirs(&layout).unwrap();
    let venv = fixture.root.join("envs/e1");
    make_venv(&python, &venv);
    let (local, included, constrained) = (
        wheel_bytes("localpkg", "1.0"),
        wheel_bytes("incl_pkg", "1.0"),
        wheel_bytes("cons_pkg", "1.0"),
    );
    let node = write_node(
        &fixture,
        "pack",
        &[
            (
                "requirements.txt",
                b"-r more.txt\n-c cons.txt\n./localpkg-1.0-py3-none-any.whl\ncons_pkg\n",
            ),
            ("more.txt", b"./incl_pkg-1.0-py3-none-any.whl\n"),
            ("cons.txt", b"cons_pkg @ ./cons_pkg-1.0-py3-none-any.whl\n"),
            ("localpkg-1.0-py3-none-any.whl", &local),
            ("incl_pkg-1.0-py3-none-any.whl", &included),
            ("cons_pkg-1.0-py3-none-any.whl", &constrained),
        ],
    );
    let guard = fixture
        .files
        .pin_location(&fixture.root, &node, None)
        .await
        .expect("the node location is pinned");
    let path = guard.directory().join("requirements.txt");
    let spec = InstallSpec {
        requirement_files: vec![path.clone()],
        guarded: Some(GuardedRequirements {
            path: path.clone(),
            directory: guard.directory().to_path_buf(),
            text: guard.requirements().unwrap(),
            retained: guard.retained(),
        }),
        ..InstallSpec::default()
    };
    let args = install_args(&venv_python(&venv), &spec, true);
    assert!(
        args.windows(2)
            .any(|pair| pair[0] == "-r" && pair[1] == "-")
    );
    assert!(!args.iter().any(|arg| arg.as_os_str() == path.as_os_str()));
    let command = with_guarded(
        command(&layout, Tool::Uv, &uv)
            .arg("--no-config")
            .args(args)
            .arg("--no-index"),
        &spec,
    );
    assert_eq!(command.cwd.as_deref(), Some(guard.directory()));
    let limits = RunLimits {
        max_tail_lines: 200,
        timeout: Some(std::time::Duration::from_secs(120)),
        ..RunLimits::default()
    };
    let output = run(&command, &DiscardSink, &CancelToken::new(), &limits)
        .await
        .unwrap_or_else(|error| panic!("uv failed: {error:?}"));
    // The included file, the constraint and the local path all resolved inside the node.
    let text = output.tail.join("\n").replace('\\', "/");
    for wheel in ["localpkg", "incl_pkg", "cons_pkg"] {
        assert!(
            text.contains(&format!("/pack/{wheel}-1.0-py3-none-any.whl")),
            "{wheel} did not resolve in the node directory: {text}"
        );
    }
}

#[tokio::test]
async fn guarded_hook_preserves_cwd_imports_and_script_filename() {
    use crate::environment::layout::{Layout, Tool, tool_environment, venv_python};
    use domain::environment::{
        NewEnvironment, Ownership, PythonEnvironment, PythonSourceKind, PythonVersion,
    };
    let Some(python) = locate_tool("python") else {
        eprintln!("skipped: python is required");
        return;
    };
    let fixture = Fixture::new().await;
    let venv = fixture.root.join("envs/e1");
    make_venv(&python, &venv);
    let script = b"import json, os, sys
import helper
observed = {
    'cwd': os.getcwd(),
    'file': __file__,
    'main_file': sys.modules['__main__'].__file__,
    'name': __name__,
    'argv': sys.argv,
    'path0': sys.path[0],
    'helper': helper.VALUE,
    'helper_file': helper.__file__,
    'stdin': sys.stdin.read(),
    'no_user_site': sys.flags.no_user_site,
}
with open('observed.json', 'w') as out:
    json.dump(observed, out, sort_keys=True)
";
    let node = write_node(
        &fixture,
        "pack",
        &[
            ("helper.py", b"VALUE = 'sibling import'\n"),
            ("install.py", script),
        ],
    );
    let directory = fixture.cn().join("pack");
    let observed = directory.join("observed.json");
    // The reference: the script run by name, exactly as before the guard existed.
    let layout = Layout::new(&fixture.root);
    let status = Command::new(venv_python(&venv))
        .args(["-s", "install.py"])
        .current_dir(&directory)
        .env_clear()
        .envs(tool_environment(&layout, Tool::Python))
        .stdin(std::process::Stdio::null())
        .status()
        .unwrap();
    assert!(status.success());
    let reference: serde_json::Value =
        serde_json::from_slice(&fs::read(&observed).unwrap()).unwrap();
    fs::remove_file(&observed).unwrap();
    let env = PythonEnvironment::provisioning(NewEnvironment {
        id: EnvId::new("env-1").unwrap(),
        path: venv.clone(),
        ownership: Ownership::AppOwned,
        source: PythonSourceKind::Managed,
        base_interpreter: None,
        python_version: PythonVersion::new(3, 12, 15),
        profile_id: "test".into(),
        profile_revision: 1,
        authorized_location: venv,
        now: "2026-10-17T00:00:00Z".into(),
    });
    let guard = fixture
        .files
        .pin_location(&fixture.root, &node, None)
        .await
        .expect("the node location is pinned");
    super::hook::PythonHook
        .run(&fixture.root, &env, &*guard, &ctx())
        .await
        .unwrap();
    let guarded: serde_json::Value = serde_json::from_slice(&fs::read(&observed).unwrap()).unwrap();
    assert_eq!(guarded, reference);
    assert!(guarded["file"].as_str().unwrap().ends_with("install.py"));
    assert_eq!(guarded["name"], "__main__");
    assert_eq!(guarded["helper"], "sibling import");
    assert_eq!(guarded["stdin"], "");
}
