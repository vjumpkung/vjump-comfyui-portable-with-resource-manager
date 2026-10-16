//! Offline tests: a loopback registry, zip fixtures and local Git repositories created with the
//! `git` binary (skipped with a message when it is missing). No Python is ever executed.
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
        let recycler: Recycler = Arc::new(move |path| {
            log.lock().unwrap().push(path.to_owned());
            fs::remove_dir_all(path).or_else(|_| fs::remove_file(path))
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
                    "latest_version": {"version": version}
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
        .snapshot(&fixture.root, &installed, Some(&op))
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
        .publish(&fixture.root, &operation)
        .await
        .unwrap();
    assert!(fixture.cn().join("good-node/__init__.py").is_file());
    assert!(!staging.exists() && fixture.owned().is_empty());
    let scanned = fixture.files.scan(&fixture.root).await.unwrap();
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
    prior.tree_digest = tree::tree_digest(&fixture.cn().join("good-node")).unwrap();
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
        .publish(&fixture.root, &operation)
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
    prior.tree_digest = tree::tree_digest(&fixture.disabled().join("off-node")).unwrap();
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
        .publish(&fixture.root, &operation)
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
            .scan(&fixture.root)
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
        fixture.files.recover(&fixture.root, &op).await.unwrap(),
        RecoveryAction::Discarded
    );
    assert!(fixture.owned().is_empty());
    // Crash after publication with a matching tree: roll forward, keep the node.
    dirs(&fixture, &["pack"]);
    new.tree_digest = tree::tree_digest(&fixture.cn().join("pack")).unwrap();
    let op = operation(
        OperationKind::Install,
        "node-b",
        new.clone(),
        None,
        OperationPhase::PublishIntent,
    );
    assert_eq!(
        fixture.files.recover(&fixture.root, &op).await.unwrap(),
        RecoveryAction::RolledForward
    );
    // The folder holds something this operation did not publish: block, preserve everything.
    fs::write(
        fixture.cn().join("pack/__init__.py"),
        "changed by someone else",
    )
    .unwrap();
    assert_eq!(
        fixture.files.recover(&fixture.root, &op).await.unwrap(),
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
        fixture.files.recover(&fixture.root, &op).await.unwrap(),
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
        fixture.files.recover(&fixture.root, &op).await.unwrap(),
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
    prior.tree_digest = tree::tree_digest(&fixture.cn().join("pack")).unwrap();
    assert_eq!(
        fixture
            .files
            .recover(&fixture.root, &cases("node-u1", &prior))
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
            .recover(&fixture.root, &cases("node-u2", &prior))
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
    published.tree_digest = tree::tree_digest(&fixture.cn().join("pack")).unwrap();
    let op = operation(
        OperationKind::Update,
        "node-u3",
        published,
        Some(prior.clone()),
        OperationPhase::Published,
    );
    assert_eq!(
        fixture.files.recover(&fixture.root, &op).await.unwrap(),
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
        fixture.files.recover(&fixture.root, &done).await.unwrap(),
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
            .recover(&fixture.root, &cases("node-u4", &prior))
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
        fixture.files.recover(&fixture.root, &op).await.unwrap(),
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
        fixture.files.recover(&fixture.root, &op).await.unwrap(),
        RecoveryAction::RolledForward
    );
    assert!(fixture.owned().is_empty());
    // A failing recycler keeps the backup and reports pending cleanup.
    dirs(&fixture, &[".disabled/.vjump-backup-node-x3"]);
    op.id = node_id("node-x3");
    let failing: Recycler = Arc::new(|_| Err(std::io::Error::other("no trash")));
    let stuck = LocalNodeFiles {
        registry: fixture.files.registry.clone(),
        git: fixture.files.git.clone(),
        recycler: failing,
        limits: node_limits(),
    };
    assert_eq!(
        stuck.recover(&fixture.root, &op).await.unwrap(),
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
        fixture.files.recover(&fixture.root, &op).await.unwrap(),
        RecoveryAction::RolledBack
    );
    fs::rename(fixture.cn().join("pack"), fixture.disabled().join("pack")).unwrap();
    assert_eq!(
        fixture.files.recover(&fixture.root, &op).await.unwrap(),
        RecoveryAction::RolledForward
    );
    dirs(&fixture, &["pack"]);
    assert_eq!(
        fixture.files.recover(&fixture.root, &op).await.unwrap(),
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
    let nodes = fixture.files.scan(&fixture.root).await.unwrap();
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
            .publish(&self.fixture.root, &operation)
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
    assert!(live.join("requirements.txt").is_file());
    let meta = scan::read_git(&live).unwrap();
    assert_eq!(meta.commit.as_deref(), Some(head.as_str()));
    assert_eq!(meta.branch.as_deref(), Some("main"));
    // The app's marker does not make the clone dirty.
    assert_eq!(
        g.fixture
            .files
            .git
            .is_dirty(
                &g.fixture.root,
                &g.fixture.disabled().join(".vjump-no-hooks"),
                &live
            )
            .await,
        Some(false)
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
        .git_status(&g.fixture.root, &installed)
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
        .git_status(&g.fixture.root, &installed)
        .await
        .unwrap();
    assert!(!status.state.has_upstream);
    assert_eq!((status.state.ahead, status.state.behind), (0, 0));
    fs::write(live.join(".git/MERGE_HEAD"), format!("{remote_head}\n")).unwrap();
    assert!(
        g.fixture
            .files
            .git_status(&g.fixture.root, &installed)
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
            .git_status(&g.fixture.root, &installed)
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
        .git_status(&g.fixture.root, &installed)
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
            .git_status(&g.fixture.root, &linked)
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
            .git_status(&g.fixture.root, &installed)
            .await
            .unwrap_err(),
        NodeAppError::GitUnavailable
    );
    let mut missing = installed.clone();
    missing.name = SafeFileName::new("missing-node").unwrap();
    assert_eq!(
        g.fixture
            .files
            .git_status(&g.fixture.root, &missing)
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
            .git_status(&g.fixture.root, &disabled)
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
    tree::validate_tree(&g.fixture.cn().join(installed.name.as_str())).unwrap();
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
        .publish(&g.fixture.root, &operation)
        .await
        .unwrap();
    assert_eq!(
        scan::read_git(&live).unwrap().commit.as_deref(),
        Some(new_head.as_str())
    );
    assert!(live.join("extra.py").is_file() && !live.join("later.py").exists());
    // Dirty and untracked trees are refused and keep their edits.
    g.repo.commit("more.py", "m", "five");
    fs::write(live.join("__init__.py"), "my local edit").unwrap();
    let mut current = installed.clone();
    current.source = operation.node.source.clone();
    assert_eq!(
        g.update("node-f4", &current, None).await.unwrap_err(),
        NodeAppError::GitRefused
    );
    assert_eq!(
        fs::read_to_string(live.join("__init__.py")).unwrap(),
        "my local edit"
    );
    g.repo.run(&["status", "--short"]);
    let hooks = g.fixture.disabled().join(".vjump-no-hooks");
    assert_eq!(
        g.fixture
            .files
            .git
            .is_dirty(&g.fixture.root, &hooks, &live)
            .await,
        Some(true)
    );
    fs::write(live.join("__init__.py"), "NODE_CLASS_MAPPINGS = {}").unwrap();
    fs::write(live.join("untracked.txt"), "keep me").unwrap();
    assert_eq!(
        g.update("node-f5", &current, None).await.unwrap_err(),
        NodeAppError::GitRefused
    );
    assert_eq!(
        fs::read_to_string(live.join("untracked.txt")).unwrap(),
        "keep me"
    );
    // The publish step also re-checks the tree after review (edits made after the plan).
    fs::remove_file(live.join("untracked.txt")).unwrap();
    let planned = g.update("node-f6", &current, None).await.unwrap();
    fs::write(live.join("late-edit.txt"), "late").unwrap();
    assert_eq!(
        g.fixture
            .files
            .publish(&g.fixture.root, &planned)
            .await
            .unwrap_err(),
        NodeAppError::GitRefused
    );
    assert_eq!(
        fs::read_to_string(live.join("late-edit.txt")).unwrap(),
        "late"
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
            .publish(&g.fixture.root, &moved)
            .await
            .unwrap_err(),
        NodeAppError::StalePlan
    );
    g.fixture
        .files
        .publish(&g.fixture.root, &planned)
        .await
        .unwrap();
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
    assert_eq!(
        g.update("node-d2", &installed, None).await.unwrap_err(),
        NodeAppError::GitRefused
    );
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
    assert_eq!(
        g.update("node-d4", &detached, None).await.unwrap_err(),
        NodeAppError::GitRefused
    );
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
        .publish(&g.fixture.root, &operation)
        .await
        .unwrap();
    assert_eq!(
        scan::read_git(&live).unwrap().commit.as_deref(),
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
    // Crash before the merge: roll back (nothing moved), staging stub removed.
    assert_eq!(
        g.fixture
            .files
            .recover(&g.fixture.root, &operation)
            .await
            .unwrap(),
        RecoveryAction::RolledBack
    );
    assert!(g.fixture.owned().is_empty());
    let _ = old_head;
    // Crash after the merge: roll forward.
    g.fixture
        .files
        .publish(&g.fixture.root, &operation)
        .await
        .unwrap();
    assert_eq!(
        g.fixture
            .files
            .recover(&g.fixture.root, &operation)
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
            .recover(&g.fixture.root, &odd)
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
