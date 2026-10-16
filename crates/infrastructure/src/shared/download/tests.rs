use std::sync::atomic::{AtomicU64, Ordering};

use super::*;
use crate::shared::test_http::{Route, TestServer};

fn no_progress() -> impl Fn(u64, Option<u64>) + Send + Sync {
    |_, _| {}
}

fn leftovers(directory: &Path) -> Vec<String> {
    fs::read_dir(directory)
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect()
}

#[tokio::test]
async fn a_verified_download_is_published_atomically() {
    let server = TestServer::start().await;
    let body = vec![42u8; 3 << 20];
    server.route("/tool.zip", Route::ok(body.clone()));
    let directory = tempfile::tempdir().unwrap();
    let destination = directory.path().join("downloads ไทย").join("tool.zip");
    let reported = AtomicU64::new(0);
    Downloader::with_loopback_http()
        .fetch(
            &server.url("/tool.zip"),
            &destination,
            &sha256_hex(&body),
            16 << 20,
            &CancelToken::new(),
            &|done, _| reported.store(done, Ordering::SeqCst),
        )
        .await
        .unwrap();
    assert_eq!(fs::read(&destination).unwrap(), body);
    assert_eq!(reported.load(Ordering::SeqCst), body.len() as u64);
    assert_eq!(leftovers(destination.parent().unwrap()), ["tool.zip"]);
}

#[tokio::test]
async fn a_hash_mismatch_leaves_no_file_behind() {
    let server = TestServer::start().await;
    server.route("/tool.zip", Route::ok(b"tampered content".to_vec()));
    let directory = tempfile::tempdir().unwrap();
    let destination = directory.path().join("tool.zip");
    let error = Downloader::with_loopback_http()
        .fetch(
            &server.url("/tool.zip"),
            &destination,
            &sha256_hex(b"the pinned content"),
            1 << 20,
            &CancelToken::new(),
            &no_progress(),
        )
        .await
        .unwrap_err();
    assert_eq!(error, DownloadError::HashMismatch);
    assert!(
        leftovers(directory.path()).is_empty(),
        "no final file and no .part"
    );
}

#[tokio::test]
async fn an_interrupted_download_is_retryable_and_leaves_no_file_behind() {
    let server = TestServer::start().await;
    server.route(
        "/tool.zip",
        Route::Truncated {
            body: vec![1u8; 4096],
            declared: 1 << 20,
        },
    );
    let directory = tempfile::tempdir().unwrap();
    let destination = directory.path().join("tool.zip");
    let error = Downloader::with_loopback_http()
        .fetch(
            &server.url("/tool.zip"),
            &destination,
            &sha256_hex(b"irrelevant"),
            16 << 20,
            &CancelToken::new(),
            &no_progress(),
        )
        .await
        .unwrap_err();
    assert!(error.is_retryable(), "{error:?}");
    assert!(leftovers(directory.path()).is_empty());
}

#[tokio::test]
async fn http_errors_size_limits_and_cancellation_are_reported() {
    let server = TestServer::start().await;
    server.route("/missing", Route::status(404, "no"));
    server.route("/flaky", Route::status(503, "later"));
    server.route("/big", Route::ok(vec![0u8; 4096]));
    let directory = tempfile::tempdir().unwrap();
    let downloader = Downloader::with_loopback_http();
    let fetch = |path: &str, max: u64, cancel: CancelToken| {
        let destination = directory.path().join("out.bin");
        let url = server.url(path);
        let downloader = downloader.clone();
        async move {
            downloader
                .fetch(
                    &url,
                    &destination,
                    &sha256_hex(b"x"),
                    max,
                    &cancel,
                    &|_, _| {},
                )
                .await
        }
    };
    assert_eq!(
        fetch("/missing", 1 << 20, CancelToken::new()).await,
        Err(DownloadError::Status(404))
    );
    let flaky = fetch("/flaky", 1 << 20, CancelToken::new())
        .await
        .unwrap_err();
    assert_eq!(flaky, DownloadError::Status(503));
    assert!(flaky.is_retryable());
    assert!(!DownloadError::Status(404).is_retryable());
    assert_eq!(
        fetch("/big", 1024, CancelToken::new()).await,
        Err(DownloadError::TooLarge)
    );
    let cancel = CancelToken::new();
    cancel.cancel();
    assert_eq!(
        fetch("/big", 1 << 20, cancel).await,
        Err(DownloadError::Cancelled)
    );
    assert!(leftovers(directory.path()).is_empty());
}

#[tokio::test]
async fn only_https_and_loopback_test_urls_are_allowed() {
    let directory = tempfile::tempdir().unwrap();
    let destination = directory.path().join("x");
    let production = Downloader::new();
    for url in [
        "http://example.com/tool.zip",
        "http://127.0.0.1:1/tool.zip",
        "ftp://example.com/tool.zip",
        "file:///etc/passwd",
    ] {
        assert_eq!(
            production
                .fetch(url, &destination, "00", 1, &CancelToken::new(), &|_, _| {})
                .await,
            Err(DownloadError::InsecureUrl),
            "{url}"
        );
    }
    assert!(is_loopback_http("http://127.0.0.1:8080/x"));
    assert!(!is_loopback_http("http://127.0.0.1.evil.example/x"));
    assert!(!is_loopback_http("http://localhost.evil.example/x"));
}

#[tokio::test]
async fn redirects_to_insecure_hosts_are_refused() {
    let server = TestServer::start().await;
    server.route(
        "/go",
        Route::Redirect("http://example.invalid/tool.zip".into()),
    );
    let directory = tempfile::tempdir().unwrap();
    let error = Downloader::with_loopback_http()
        .fetch(
            &server.url("/go"),
            &directory.path().join("x"),
            "00",
            1 << 20,
            &CancelToken::new(),
            &|_, _| {},
        )
        .await
        .unwrap_err();
    assert_eq!(error, DownloadError::Network);
}

#[test]
fn hashing_helpers_agree() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("f");
    fs::write(&path, b"abc").unwrap();
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(sha256_file(&path).unwrap(), sha256_hex(b"abc"));
}
