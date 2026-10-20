use std::{io::Cursor, io::Write, sync::Arc};

use application::environment::{NullSink, RunId, StepId};
use application::shared::CancelToken;
use flate2::{Compression, write::GzEncoder};
use zip::write::SimpleFileOptions;

use super::*;
use crate::shared::test_http::{Route, TestServer};

/// A stand-in "tool": a copy of the platform shell that prints a fake version when asked.
fn shell_bytes() -> Vec<u8> {
    let source = if cfg!(windows) {
        PathBuf::from(std::env::var("SystemRoot").unwrap()).join(r"System32\cmd.exe")
    } else {
        PathBuf::from("/bin/sh")
    };
    fs::read(source).unwrap()
}

fn version_args(text: &str) -> Vec<OsString> {
    if cfg!(windows) {
        vec!["/d".into(), "/c".into(), format!("echo {text}").into()]
    } else {
        // The text may contain shell metacharacters such as parentheses.
        vec!["-c".into(), format!("echo '{text}'").into()]
    }
}

fn zip_of(entries: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in entries {
        writer
            .start_file(
                *name,
                SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated)
                    .unix_permissions(0o755),
            )
            .unwrap();
        writer.write_all(data).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn tar_gz_of(entries: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut builder = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::default()));
    for (name, data) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o755);
        header.set_entry_type(tar::EntryType::Regular);
        header.set_cksum();
        builder
            .append_data(&mut header, name, data.as_slice())
            .unwrap();
    }
    builder.into_inner().unwrap().finish().unwrap()
}

fn exe_name() -> &'static str {
    if cfg!(windows) { "uv.exe" } else { "uv" }
}

struct Fixture {
    server: TestServer,
    root: PathBuf,
    _directory: tempfile::TempDir,
}

impl Fixture {
    async fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        Self {
            server: TestServer::start().await,
            root: directory.path().join("install root ไทย"),
            _directory: directory,
        }
    }

    fn tools(&self, archive: &[u8], kind: ArchiveKind, executable: &str, contains: &str) -> Tools {
        self.server.route("/uv", Route::ok(archive.to_vec()));
        let source = ToolSource {
            url: self.server.url("/uv"),
            sha256: sha256_hex(archive),
            kind,
            executable: executable.to_owned(),
            version_args: version_args("uv 0.13.1 (fake build)"),
            version_contains: contains.to_owned(),
        };
        Tools::with(
            Downloader::with_loopback_http(),
            ToolSources {
                uv: source.clone(),
                mingit: source,
            },
        )
    }

    fn requests(&self) -> usize {
        self.server.requests.lock().unwrap().len()
    }

    fn tools_dir_entries(&self) -> Vec<String> {
        fs::read_dir(self.root.join("tools"))
            .map(|entries| {
                entries
                    .flatten()
                    .map(|entry| entry.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default()
    }
}

fn ctx() -> StepContext {
    StepContext {
        run_id: RunId::new("test").unwrap(),
        step: StepId::Tools,
        sink: Arc::new(NullSink),
        cancel: CancelToken::new(),
        allow_source_builds: false,
        retained: Default::default(),
    }
}

#[tokio::test]
async fn uv_is_downloaded_verified_extracted_and_published_once() {
    let fixture = Fixture::new().await;
    let archive = zip_of(&[(exe_name(), shell_bytes()), ("uvx.exe", b"second".to_vec())]);
    let tools = fixture.tools(&archive, ArchiveKind::Zip, exe_name(), "uv 0.13.1 ");

    let info = tools.ensure_uv(&fixture.root, &ctx()).await.unwrap();
    assert_eq!(info.name, "uv");
    assert!(info.version.starts_with("uv 0.13.1"), "{}", info.version);
    assert!(fixture.root.join("tools/uv").join(exe_name()).is_file());
    assert!(fixture.root.join("tools/uv/uvx.exe").is_file());
    assert_eq!(fixture.tools_dir_entries(), ["uv"], "no staging leftovers");
    assert_eq!(fixture.requests(), 1);

    // A second call recognises the installed version and downloads nothing.
    tools.ensure_uv(&fixture.root, &ctx()).await.unwrap();
    assert_eq!(fixture.requests(), 1);
}

#[tokio::test]
async fn a_gzipped_tarball_is_flattened_into_the_tool_directory() {
    let fixture = Fixture::new().await;
    let inner = format!("uv-x86_64-unknown-linux-gnu/{}", exe_name());
    let archive = tar_gz_of(&[
        (&inner, shell_bytes()),
        ("uv-x86_64-unknown-linux-gnu/uvx", b"x".to_vec()),
    ]);
    let tools = fixture.tools(&archive, ArchiveKind::TarGz, &inner, "uv 0.13.1 ");
    tools.ensure_uv(&fixture.root, &ctx()).await.unwrap();
    assert!(fixture.root.join("tools/uv").join(exe_name()).is_file());
    assert!(fixture.root.join("tools/uv/uvx").is_file());
    assert!(
        !fixture
            .root
            .join("tools/uv/uv-x86_64-unknown-linux-gnu")
            .exists()
    );
}

#[tokio::test]
async fn a_hash_mismatch_publishes_nothing() {
    let fixture = Fixture::new().await;
    let archive = zip_of(&[(exe_name(), shell_bytes())]);
    let mut tools = fixture.tools(&archive, ArchiveKind::Zip, exe_name(), "uv 0.13.1 ");
    tools.sources.uv.sha256 = sha256_hex(b"something else");
    let error = tools.ensure_uv(&fixture.root, &ctx()).await.unwrap_err();
    assert!(
        matches!(&error, ProvisionError::Failed(message) if message.contains("SHA-256")),
        "{error:?}"
    );
    assert!(fixture.tools_dir_entries().is_empty());
    assert!(!fixture.root.join("data/downloads/tools").join("x").exists());
    let leftovers: Vec<_> = fs::read_dir(fixture.root.join("data/downloads/tools"))
        .map(|entries| entries.flatten().collect())
        .unwrap_or_default();
    assert!(
        leftovers.is_empty(),
        "no .part or archive is kept: {leftovers:?}"
    );
}

#[tokio::test]
async fn a_malicious_archive_with_a_valid_hash_is_still_rejected() {
    let fixture = Fixture::new().await;
    let archive = zip_of(&[
        (exe_name(), shell_bytes()),
        ("../escape.txt", b"x".to_vec()),
    ]);
    let tools = fixture.tools(&archive, ArchiveKind::Zip, exe_name(), "uv 0.13.1 ");
    let error = tools.ensure_uv(&fixture.root, &ctx()).await.unwrap_err();
    assert!(
        matches!(&error, ProvisionError::Failed(message) if message.contains("rejected")),
        "{error:?}"
    );
    assert!(
        fixture.tools_dir_entries().is_empty(),
        "{:?}",
        fixture.tools_dir_entries()
    );
    assert!(!fixture.root.join("tools/escape.txt").exists());
    assert!(!fixture.root.join("escape.txt").exists());
}

#[tokio::test]
async fn a_tool_with_the_wrong_version_is_not_published_and_keeps_nothing_behind() {
    let fixture = Fixture::new().await;
    let archive = zip_of(&[(exe_name(), shell_bytes())]);
    let tools = fixture.tools(&archive, ArchiveKind::Zip, exe_name(), "uv 9.9.9 ");
    let error = tools.ensure_uv(&fixture.root, &ctx()).await.unwrap_err();
    assert!(
        matches!(&error, ProvisionError::Failed(message) if message.contains("unexpected version"))
    );
    assert!(
        fixture.tools_dir_entries().is_empty(),
        "{:?}",
        fixture.tools_dir_entries()
    );

    // An archive missing the executable is rejected too.
    let archive = zip_of(&[("readme.txt", b"hello".to_vec())]);
    let tools = fixture.tools(&archive, ArchiveKind::Zip, exe_name(), "uv 0.13.1 ");
    assert!(tools.ensure_uv(&fixture.root, &ctx()).await.is_err());
    assert!(fixture.tools_dir_entries().is_empty());
}

#[tokio::test]
async fn an_interrupted_download_is_retryable_and_leaves_no_tool() {
    let fixture = Fixture::new().await;
    fixture.server.route(
        "/uv",
        Route::Truncated {
            body: vec![1u8; 2048],
            declared: 1 << 20,
        },
    );
    let archive = zip_of(&[(exe_name(), shell_bytes())]);
    let mut tools = fixture.tools(&archive, ArchiveKind::Zip, exe_name(), "uv 0.13.1 ");
    fixture.server.route(
        "/uv",
        Route::Truncated {
            body: vec![1u8; 2048],
            declared: 1 << 20,
        },
    );
    tools.sources.uv.sha256 = sha256_hex(&archive);
    let error = tools.ensure_uv(&fixture.root, &ctx()).await.unwrap_err();
    assert!(matches!(error, ProvisionError::Retryable(_)), "{error:?}");
    assert!(fixture.tools_dir_entries().is_empty());
}

#[tokio::test]
async fn cancellation_stops_before_anything_is_published() {
    let fixture = Fixture::new().await;
    let archive = zip_of(&[(exe_name(), shell_bytes())]);
    let tools = fixture.tools(&archive, ArchiveKind::Zip, exe_name(), "uv 0.13.1 ");
    let ctx = ctx();
    ctx.cancel.cancel();
    assert_eq!(
        tools.ensure_uv(&fixture.root, &ctx).await.unwrap_err(),
        ProvisionError::Cancelled
    );
    assert!(fixture.tools_dir_entries().is_empty());
}

#[tokio::test]
async fn a_broken_existing_tool_is_replaced() {
    let fixture = Fixture::new().await;
    let broken = fixture.root.join("tools/uv");
    fs::create_dir_all(&broken).unwrap();
    fs::write(broken.join(exe_name()), b"not an executable").unwrap();
    fs::write(broken.join("stale.txt"), b"old").unwrap();
    let archive = zip_of(&[(exe_name(), shell_bytes())]);
    let tools = fixture.tools(&archive, ArchiveKind::Zip, exe_name(), "uv 0.13.1 ");
    tools.ensure_uv(&fixture.root, &ctx()).await.unwrap();
    assert!(!broken.join("stale.txt").exists());
    assert_eq!(fixture.requests(), 1);
}

#[cfg(windows)]
#[tokio::test]
async fn mingit_keeps_its_directory_layout() {
    let fixture = Fixture::new().await;
    let archive = zip_of(&[
        ("cmd/git.exe", shell_bytes()),
        ("mingw64/bin/git-core.txt", b"x".to_vec()),
        ("etc/gitconfig", b"[core]".to_vec()),
    ]);
    let mut tools = fixture.tools(&archive, ArchiveKind::Zip, "cmd/git.exe", "git version");
    tools.sources.mingit.version_args = version_args("git version 2.55.0.windows.3");
    tools.sources.mingit.version_contains = MINGIT_GIT_VERSION.to_owned();
    let info = tools.ensure_git(&fixture.root, &ctx()).await.unwrap();
    assert_eq!(info.name, "git");
    assert!(fixture.root.join("tools/git/cmd/git.exe").is_file());
    assert!(
        fixture
            .root
            .join("tools/git/mingw64/bin/git-core.txt")
            .is_file()
    );
    assert_eq!(
        git_program(&Layout::new(&fixture.root)).unwrap(),
        fixture.root.join("tools/git/cmd/git.exe")
    );
}

#[test]
fn git_versions_are_parsed_and_compared() {
    assert_eq!(parse_git_version("git version 2.43.0"), Some((2, 43)));
    assert_eq!(
        parse_git_version("git version 2.55.0.windows.3\n"),
        Some((2, 55))
    );
    assert_eq!(
        parse_git_version("git version 2.39.5 (Apple Git-154)"),
        Some((2, 39))
    );
    assert_eq!(parse_git_version("something else"), None);
    assert!((2, 39) < MIN_SYSTEM_GIT && (2, 40) >= MIN_SYSTEM_GIT && (3, 0) >= MIN_SYSTEM_GIT);
}

#[test]
fn the_pinned_sources_use_https_hashes_and_the_expected_version_strings() {
    let sources = ToolSources::pinned();
    for source in [&sources.uv, &sources.mingit] {
        assert!(source.url.starts_with("https://"));
        assert_eq!(source.sha256.len(), 64);
    }
    assert_eq!(sources.uv.version_contains, "uv 0.13.1 ");
    assert_eq!(sources.mingit.version_contains, "2.55.0.windows.3");
}

#[tokio::test]
async fn moved_hf_marker_cannot_skip_tool_recreation() {
    let directory = tempfile::tempdir().unwrap();
    let old = Layout::new(directory.path().join("original root"));
    let moved = Layout::new(directory.path().join("moved root"));
    let old_python = old.managed_python_dir().join("cpython/python.exe");
    let moved_python = moved.managed_python_dir().join("cpython/python.exe");
    for python in [&old_python, &moved_python] {
        fs::create_dir_all(python.parent().unwrap()).unwrap();
        fs::write(python, b"same copied base interpreter").unwrap();
    }
    fs::create_dir_all(moved.hf_exe().parent().unwrap()).unwrap();
    fs::write(moved.hf_exe(), b"copied hf executable").unwrap();
    let old_marker = hf_marker_text(&old, &old_python).unwrap();
    fs::write(moved.hf_dir().join(".vj-lock"), &old_marker).unwrap();

    let moved_marker = hf_marker_text(&moved, &moved_python).unwrap();
    assert_ne!(old_marker, moved_marker);
    assert!(!reusable_hf_tool(&moved, &moved_marker, &ctx()).await);
    assert!(moved.hf_exe().is_file());
    assert_eq!(
        fs::read_to_string(moved.hf_dir().join(".vj-lock")).unwrap(),
        old_marker
    );
}

#[tokio::test]
async fn matching_hf_marker_still_requires_successful_tool_launch() {
    let directory = tempfile::tempdir().unwrap();
    let layout = Layout::new(directory.path());
    let python = layout.managed_python_dir().join("cpython/python.exe");
    fs::create_dir_all(python.parent().unwrap()).unwrap();
    fs::write(&python, b"base interpreter").unwrap();
    fs::create_dir_all(layout.hf_exe().parent().unwrap()).unwrap();
    fs::write(layout.hf_exe(), b"broken hf executable").unwrap();
    let marker = hf_marker_text(&layout, &python).unwrap();
    fs::write(layout.hf_dir().join(".vj-lock"), &marker).unwrap();
    assert!(!reusable_hf_tool(&layout, &marker, &ctx()).await);

    fs::write(&python, b"different base interpreter").unwrap();
    assert_ne!(marker, hf_marker_text(&layout, &python).unwrap());
}

#[tokio::test]
async fn moved_gdown_marker_cannot_skip_tool_recreation() {
    let directory = tempfile::tempdir().unwrap();
    let old = Layout::new(directory.path().join("original root"));
    let moved = Layout::new(directory.path().join("moved root"));
    let old_python = old.managed_python_dir().join("cpython/python.exe");
    let moved_python = moved.managed_python_dir().join("cpython/python.exe");
    for python in [&old_python, &moved_python] {
        fs::create_dir_all(python.parent().unwrap()).unwrap();
        fs::write(python, b"same copied base interpreter").unwrap();
    }
    fs::create_dir_all(moved.gdown_python().parent().unwrap()).unwrap();
    fs::write(moved.gdown_python(), b"copied gdown interpreter").unwrap();
    let old_marker = gdown_marker_text(&old, &old_python).unwrap();
    fs::write(moved.gdown_dir().join(".vj-lock"), &old_marker).unwrap();

    let moved_marker = gdown_marker_text(&moved, &moved_python).unwrap();
    assert_ne!(old_marker, moved_marker);
    assert!(!reusable_gdown_tool(&moved, &moved_marker, &ctx()).await);
    assert!(moved.gdown_python().is_file());
    assert_eq!(
        fs::read_to_string(moved.gdown_dir().join(".vj-lock")).unwrap(),
        old_marker
    );
}

#[tokio::test]
async fn matching_gdown_marker_still_requires_successful_tool_launch() {
    let directory = tempfile::tempdir().unwrap();
    let layout = Layout::new(directory.path());
    let python = layout.managed_python_dir().join("cpython/python.exe");
    fs::create_dir_all(python.parent().unwrap()).unwrap();
    fs::write(&python, b"base interpreter").unwrap();
    fs::create_dir_all(layout.gdown_python().parent().unwrap()).unwrap();
    fs::write(layout.gdown_python(), b"broken gdown interpreter").unwrap();
    let marker = gdown_marker_text(&layout, &python).unwrap();
    fs::write(layout.gdown_dir().join(".vj-lock"), &marker).unwrap();
    assert!(!reusable_gdown_tool(&layout, &marker, &ctx()).await);

    fs::write(&python, b"different base interpreter").unwrap();
    assert_ne!(marker, gdown_marker_text(&layout, &python).unwrap());
}

/// Real network: provisions uv, MinGit/system git and the hf and gdown tools into a temporary
/// root.
#[tokio::test]
#[ignore = "downloads uv, MinGit and Python from the network"]
async fn real_tools_are_provisioned() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("install root ไทย");
    let tools = Tools::new();
    let uv = tools.ensure_uv(&root, &ctx()).await.unwrap();
    assert!(uv.version.starts_with("uv 0.13.1"));
    tools.ensure_git(&root, &ctx()).await.unwrap();
    let hf = tools.ensure_hf_tool(&root, &ctx()).await.unwrap();
    assert_eq!(hf.version, "2.2.0");
    assert!(Layout::new(&root).hf_exe().is_file());
    // Idempotent.
    tools.ensure_hf_tool(&root, &ctx()).await.unwrap();
    let gdown = tools.ensure_gdown_tool(&root, &ctx()).await.unwrap();
    assert_eq!(gdown.version, "6.4.2");
    assert!(Layout::new(&root).gdown_python().is_file());
    tools.ensure_gdown_tool(&root, &ctx()).await.unwrap();
    assert!(tools.download_tools_ready(&root));
}
