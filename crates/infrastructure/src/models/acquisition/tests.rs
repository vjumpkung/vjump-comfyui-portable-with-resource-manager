use super::{
    codec,
    finalize::{LocalDownloadFiles, StagingArea},
    http::ArtifactHttp,
    native_engine::NativeDownloader,
    progress::DownloadHub,
    resolver::ProviderResolver,
    sources::SourceIngress,
    sqlite::SqliteDownloads,
};
use crate::{
    settings::secrets::{CredentialSnapshot, CredentialVault},
    shared::{
        model_paths::AuthorizedModelRoots,
        path_locks::PathLocks,
        sqlite::Db,
        test_http::{Route, TestServer},
    },
};
use application::{
    models::acquisition::{finalization::FinalizeDownload, *},
    shared::CancelToken,
};
use async_trait::async_trait;
use domain::{
    models::{acquisition::*, library::ModelCategory},
    settings::CredentialKind,
    shared::model_values::{JobId, RelativeModelPath, SafeFileName, Sha256},
};
use std::{
    fs,
    sync::{Arc, Mutex},
};

struct NoCredentials;

#[tokio::test]
async fn shared_bucket_bounds_combined_native_and_relay_bytes() {
    use std::{
        num::NonZeroU32,
        time::{Duration, Instant},
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let server = TestServer::start().await;
    server.route("/limited", Route::ok(vec![7; 2048]));
    let f = Fixture::new();
    let job = f.insert(f.job("limited", &server.url("/limited"))).await;
    let started = Instant::now();
    let limiter = Arc::new(super::bandwidth::BandwidthLimiter::new(NonZeroU32::new(4)));
    let mut native = f.native();
    native.bandwidth = limiter.clone();
    let (relay, mut source, mut child) = super::hf_proxy::tests::mock_tunnel(limiter.clone()).await;
    source.write_all(&[9; 2048]).await.unwrap();
    source.shutdown().await.unwrap();
    let cancel = CancelToken::new();
    let (native_result, bytes) = tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(
            native.transfer(&job, f.hub.clone(), cancel.clone()),
            async {
                let mut bytes = Vec::new();
                child.read_to_end(&mut bytes).await.unwrap();
                bytes
            },
        )
    })
    .await
    .unwrap();
    assert_eq!(native_result.unwrap().size, 2048);
    assert_eq!(bytes, vec![9; 2048]);
    // 4 KiB/s, one shared burst of floor(rate/10) = 409 bytes.
    assert!(4096.0 <= 4096.0 * started.elapsed().as_secs_f64() + 409.0);
    relay.shutdown().await;
}

#[tokio::test]
async fn live_cap_changes_affect_already_running_hf_and_native_jobs() {
    use std::{num::NonZeroU32, time::Duration};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    // Hold the native response body until after the uncapped transfer started.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (native_ready, native_wait) = tokio::sync::oneshot::channel();
    let (release_native, released) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            request.push(stream.read_u8().await.unwrap());
        }
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 8192\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        native_ready.send(()).unwrap();
        released.await.unwrap();
        stream.write_all(&[1; 8192]).await.unwrap();
    });
    let f = Fixture::new();
    let job = f
        .insert(f.job("live", &format!("http://{address}/file")))
        .await;
    let limiter = Arc::new(super::bandwidth::BandwidthLimiter::new(None));
    let mut native = f.native();
    native.bandwidth = limiter.clone();
    let native_hub = f.hub.clone();
    let native_task =
        tokio::spawn(async move { native.transfer(&job, native_hub, CancelToken::new()).await });
    let (relay, mut source, mut child) = super::hf_proxy::tests::mock_tunnel(limiter.clone()).await;
    let hf_task = tokio::spawn(async move {
        let mut bytes = Vec::new();
        child.read_to_end(&mut bytes).await.unwrap();
        bytes
    });
    native_wait.await.unwrap();
    tokio::task::yield_now().await;
    limiter.set_limit(NonZeroU32::new(1));
    release_native.send(()).unwrap();
    source.write_all(&[2; 8192]).await.unwrap();
    source.shutdown().await.unwrap();
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert!(!native_task.is_finished());
    assert!(!hf_task.is_finished());
    let written = fs::metadata(f.staging.root.path.join("live.part"))
        .unwrap()
        .len();
    assert!(written < 8192);
    // Raising the cap affects the existing attempts, then unlimited wakes them.
    limiter.set_limit(NonZeroU32::new(2));
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(!native_task.is_finished());
    assert!(!hf_task.is_finished());
    limiter.set_limit(None);
    let (native_result, bytes) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(native_task, hf_task)
    })
    .await
    .unwrap();
    assert_eq!(native_result.unwrap().unwrap().size, 8192);
    assert_eq!(bytes.unwrap(), vec![2; 8192]);
    server.await.unwrap();
    relay.shutdown().await;
}
#[test]
fn platform_no_replace_rename_uses_pinned_parent() {
    let directory = tempfile::tempdir().unwrap();
    let parent = crate::shared::model_paths::pin_absolute(directory.path()).unwrap();
    let a = SafeFileName::new("a.bin").unwrap();
    let b = SafeFileName::new("b.bin").unwrap();
    fs::write(directory.path().join(a.as_str()), b"payload").unwrap();
    let file = parent.open(&a, false, false).unwrap();
    crate::shared::model_paths::rename_no_replace(&parent, &a, &file, &parent, &b).unwrap();
}
#[async_trait]
impl CredentialVault for NoCredentials {
    async fn read(
        &self,
        _: CredentialKind,
    ) -> std::result::Result<CredentialSnapshot, application::settings::CredentialStoreError> {
        Ok(CredentialSnapshot {
            generation: 0,
            value: None,
        })
    }
    async fn generation(
        &self,
        _: CredentialKind,
    ) -> std::result::Result<u64, application::settings::CredentialStoreError> {
        Ok(0)
    }
}
struct Fixture {
    _temp: tempfile::TempDir,
    repository: Arc<SqliteDownloads>,
    staging: Arc<StagingArea>,
    files: Arc<LocalDownloadFiles>,
    hub: Arc<DownloadHub>,
    models: std::path::PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let models = temp.path().join("models");
        fs::create_dir(&models).unwrap();
        let connection = rusqlite::Connection::open(temp.path().join("test.db")).unwrap();
        connection
            .execute_batch(include_str!("../../../migrations/004_downloads.sql"))
            .unwrap();
        let repository = Arc::new(SqliteDownloads {
            db: Db {
                connection: Arc::new(Mutex::new(connection)),
            },
        });
        let staging = Arc::new(StagingArea::new(&temp.path().join("downloads")).unwrap());
        let roots =
            Arc::new(AuthorizedModelRoots::new([("models".into(), models.clone())]).unwrap());
        let files = Arc::new(LocalDownloadFiles {
            roots,
            staging: staging.clone(),
            locks: Arc::new(PathLocks::default()),
        });
        Self {
            _temp: temp,
            repository,
            staging,
            files,
            hub: Arc::new(DownloadHub::new("test-epoch".into(), 4)),
            models,
        }
    }
    fn job(&self, id: &str, url: &str) -> DownloadJob {
        DownloadJob {
            id: JobId::new(id).unwrap(),
            source: SourceRef::Direct {
                locator: url.into(),
                ingress_id: None,
                requires_source_refresh: false,
                insecure_http_confirmed: true,
            },
            identity: ProviderIdentity(format!("direct:{id}")),
            destination: Destination {
                root_id: "models".into(),
                category: ModelCategory::new("checkpoints").unwrap(),
                subdirectory: RelativeModelPath::default(),
                filename: SafeFileName::new(format!("{id}.bin")).unwrap(),
            },
            state: DownloadState::Queued,
            requested_engine: DownloadEngine::Native,
            effective_engine: DownloadEngine::Native,
            generation: 0,
            size: DownloadSize::Unknown,
            expected_sha256: None,
            bytes_done: 0,
            validator: None,
            retry_count: 0,
            retry_at: None,
            stop_intent: None,
            error: None,
            conflict: None,
            decision_id: None,
            observed: None,
            publication: None,
            created_at: 1,
            updated_at: 1,
        }
    }
    async fn insert(&self, job: DownloadJob) -> DownloadJob {
        self.repository
            .enqueue(EnqueueRequest {
                job: job.clone(),
                force: false,
                catalog_match: None,
            })
            .await
            .unwrap();
        job
    }
    fn native(&self) -> NativeDownloader {
        let http = ArtifactHttp::new(Arc::new(NoCredentials)).unwrap();
        NativeDownloader {
            bandwidth: Arc::new(super::bandwidth::BandwidthLimiter::new(None)),
            resolver: Arc::new(ProviderResolver::new(
                http,
                Arc::new(SourceIngress::default()),
            )),
            staging: self.staging.clone(),
            repository: self.repository.clone(),
        }
    }
}

#[tokio::test]
async fn enqueue_is_atomic_for_identity_and_destination_and_rejects_stale_worker() {
    let f = Fixture::new();
    let a = f.job("a", "https://x/a.bin");
    let mut b = f.job("b", "https://x/b.bin");
    b.identity = a.identity.clone();
    let (first, second) = tokio::join!(
        f.repository.enqueue(EnqueueRequest {
            job: a.clone(),
            force: false,
            catalog_match: None
        }),
        f.repository.enqueue(EnqueueRequest {
            job: b,
            force: true,
            catalog_match: None
        })
    );
    let outcomes = [first.unwrap(), second.unwrap()];
    assert_eq!(
        outcomes
            .iter()
            .filter(|r| matches!(r, EnqueueOutcome::Queued(_)))
            .count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|r| matches!(r, EnqueueOutcome::Duplicate(_)))
            .count(),
        1
    );
    let claimed = f.repository.claim(2).await.unwrap().unwrap();
    assert_eq!(claimed.generation, 1);
    let mut old = claimed.clone();
    old.generation = 0;
    assert!(f.repository.save(&old, 0).await.is_err());
    let mut conflict = f.job("c", "https://x/c.bin");
    conflict.destination = claimed.destination.clone();
    assert!(matches!(
        f.repository
            .enqueue(EnqueueRequest {
                job: conflict,
                force: true,
                catalog_match: None
            })
            .await
            .unwrap(),
        EnqueueOutcome::Conflict { .. }
    ));
    assert!(matches!(
        f.repository
            .enqueue(EnqueueRequest {
                job: a,
                force: false,
                catalog_match: Some(claimed.destination)
            })
            .await
            .unwrap(),
        EnqueueOutcome::Duplicate(_)
    ));
}

#[tokio::test]
async fn resumes_from_actual_length_with_strong_if_range_and_restarts_on_200() {
    let server = TestServer::start().await;
    server.route(
        "/file",
        Route::Body {
            status: 206,
            headers: vec![
                ("Content-Range".into(), "bytes 3-5/6".into()),
                ("ETag".into(), "\"v1\"".into()),
            ],
            body: b"def".to_vec(),
        },
    );
    let f = Fixture::new();
    let mut job = f.job("resume", &server.url("/file"));
    job.validator = Some(ResumeValidator::StrongEtag("\"v1\"".into()));
    job.bytes_done = 999;
    job.size = DownloadSize::Exact(6);
    job = f.insert(job).await;
    fs::write(f.staging.root.path.join("resume.part"), b"abc").unwrap();
    let result = f
        .native()
        .transfer(&job, f.hub.clone(), CancelToken::new())
        .await
        .unwrap();
    assert_eq!(result.size, 6);
    assert_eq!(
        fs::read(f.staging.root.path.join("resume.part")).unwrap(),
        b"abcdef"
    );
    let request = server.requests.lock().unwrap()[0].to_lowercase();
    assert!(request.contains("range: bytes=3-"));
    assert!(request.contains("if-range: \"v1\""));
    assert!(request.contains("accept-encoding: identity"));
    server.route("/file", Route::ok(b"NEWNEW".to_vec()));
    f.native()
        .transfer(&job, f.hub.clone(), CancelToken::new())
        .await
        .unwrap();
    assert_eq!(
        fs::read(f.staging.root.path.join("resume.part")).unwrap(),
        b"NEWNEW"
    );
}

#[tokio::test]
async fn malformed_206_416_and_incomplete_body_never_publish() {
    let server = TestServer::start().await;
    let f = Fixture::new();
    for (i, route) in [
        Route::Body {
            status: 206,
            headers: vec![("Content-Range".into(), "bytes 1-3/4".into())],
            body: b"bad".to_vec(),
        },
        Route::status(416, ""),
        Route::Truncated {
            body: b"abc".to_vec(),
            declared: 10,
        },
    ]
    .into_iter()
    .enumerate()
    {
        let path = format!("/f{i}");
        server.route(&path, route);
        let job = f.insert(f.job(&format!("j{i}"), &server.url(&path))).await;
        assert!(
            f.native()
                .transfer(&job, f.hub.clone(), CancelToken::new())
                .await
                .is_err()
        );
        assert!(!f.models.join("checkpoints").exists());
    }
}

#[tokio::test]
async fn durable_publish_recovers_rename_before_sql_update_and_catalog_is_idempotent() {
    let f = Fixture::new();
    let mut job = f.insert(f.job("recover", "https://x/model.bin")).await;
    let key = "recover.part";
    fs::write(f.staging.root.path.join(key), b"payload").unwrap();
    let (identity, digest) = f
        .files
        .verify(key, None, DownloadSize::Exact(7))
        .await
        .unwrap();
    job.state = DownloadState::Verifying;
    job.publication = Some(PublicationIntent {
        phase: FinalizePhase::Verified,
        staged_key: key.into(),
        staged_identity: identity,
        digest,
        external_verified: false,
        backup_name: SafeFileName::new(".recover.backup").unwrap(),
        published_identity: None,
    });
    f.repository.save(&job, 0).await.unwrap();
    f.files.publish(&job).await.unwrap();
    assert!(!f.staging.root.path.join(key).exists());
    let mut recovered = f.repository.get(&job.id).await.unwrap();
    let finalizer = FinalizeDownload {
        repository: f.repository.clone(),
        files: f.files.clone(),
        events: f.hub.clone(),
    };
    finalizer.execute(&mut recovered).await.unwrap();
    assert_eq!(recovered.state, DownloadState::Completed);
    finalizer.execute(&mut recovered).await.unwrap();
    f.repository
        .db
        .run(|c| {
            assert_eq!(
                c.query_row("SELECT COUNT(*) FROM models", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                1
            );
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(
        fs::read(f.models.join("checkpoints/recover.bin")).unwrap(),
        b"payload"
    );
}

#[tokio::test]
async fn hash_mismatch_has_no_model_and_late_conflict_keeps_both_files() {
    let f = Fixture::new();
    let mut job = f.insert(f.job("hash", "https://x/model.bin")).await;
    fs::write(f.staging.root.path.join("hash.part"), b"payload").unwrap();
    assert!(
        matches!(f.files.verify("hash.part",Some(&Sha256::new(&"0".repeat(64)).unwrap()),DownloadSize::Unknown).await,Err(e) if e.0==DownloadErrorCode::HashMismatch)
    );
    let (identity, digest) = f
        .files
        .verify("hash.part", None, DownloadSize::Unknown)
        .await
        .unwrap();
    job.publication = Some(PublicationIntent {
        phase: FinalizePhase::Verified,
        staged_key: "hash.part".into(),
        staged_identity: identity,
        digest,
        external_verified: false,
        backup_name: SafeFileName::new(".hash.backup").unwrap(),
        published_identity: None,
    });
    fs::create_dir_all(f.models.join("checkpoints")).unwrap();
    fs::write(f.models.join("checkpoints/hash.bin"), b"late").unwrap();
    assert!(matches!(f.files.publish(&job).await,Err(e) if e.0==DownloadErrorCode::Conflict));
    assert_eq!(
        fs::read(f.models.join("checkpoints/hash.bin")).unwrap(),
        b"late"
    );
    assert_eq!(
        fs::read(f.staging.root.path.join("hash.part")).unwrap(),
        b"payload"
    );
}

#[tokio::test]
async fn startup_preserves_paused_native_partial_and_removes_only_owned_orphans() {
    let f = Fixture::new();
    let mut job = f.insert(f.job("paused", "https://x/model.bin")).await;
    job.state = DownloadState::Paused;
    f.repository.save(&job, 0).await.unwrap();
    for name in ["paused.part", "orphan.part", "unrelated.txt"] {
        fs::write(f.staging.root.path.join(name), b"data").unwrap();
    }
    f.files.clean_orphans(&[job]).await.unwrap();
    assert!(f.staging.root.path.join("paused.part").exists());
    assert!(!f.staging.root.path.join("orphan.part").exists());
    assert!(f.staging.root.path.join("unrelated.txt").exists());
}

#[test]
fn subscriptions_snapshot_atomically_drop_stale_progress_and_signal_overflow() {
    let f = Fixture::new();
    let mut job = f.job("events", "https://x/file.bin");
    job.state = DownloadState::Downloading;
    job.generation = 2;
    f.hub.state(&job);
    let sub = f.hub.subscribe(None);
    assert!(matches!(
        f.hub.drain(sub.id, 10).unwrap()[0].kind,
        DownloadEventKind::Snapshot(_)
    ));
    let sample = TransferProgress {
        bytes: 1,
        total: DownloadSize::Unknown,
        bytes_per_second: 1.0,
        eta_seconds: None,
        estimated: false,
    };
    f.hub.sample(&job.id, 1, sample.clone());
    assert!(f.hub.drain(sub.id, 10).unwrap().is_empty());
    for _ in 0..100 {
        f.hub.sample(&job.id, 2, sample.clone());
    }
    assert_eq!(f.hub.drain(sub.id, 200).unwrap().len(), 1);
    for _ in 0..10 {
        f.hub.state(&job);
    }
    assert!(matches!(
        f.hub.drain(sub.id, 10).unwrap()[0].kind,
        DownloadEventKind::Gap
    ));
    f.hub.unsubscribe(sub.id);
    assert!(f.hub.drain(sub.id, 10).is_none());
}

#[test]
fn persistence_never_contains_direct_signature_and_can_reuse_live_ingress() {
    let f = Fixture::new();
    let ingress = SourceIngress::default();
    let mut job = f.job("secret", "https://x/model.bin");
    job.error = Some(DownloadErrorCode::BandwidthProxyUnavailable);
    job.source = ingress
        .stage(
            "https://x/model.bin?X-Amz-Signature=super-secret-signature",
            false,
        )
        .unwrap();
    let raw = codec::encode(&job).unwrap();
    assert!(raw.contains("\"error\":\"bandwidth_proxy_unavailable\""));
    assert!(!raw.contains("super-secret-signature"));
    assert!(!raw.contains("X-Amz"));
    let read = codec::decode(&raw).unwrap();
    assert_eq!(read.error, job.error);
    assert!(ingress.executable(&read.source).is_ok());
    assert!(SourceIngress::default().executable(&read.source).is_err());
}

#[tokio::test]
async fn http_confirmation_happens_before_contact_and_direct_auth_is_never_forwarded() {
    let server = TestServer::start().await;
    server.route("/file", Route::ok(b"data".to_vec()));
    let http = ArtifactHttp::new(Arc::new(NoCredentials)).unwrap();
    let url = url::Url::parse(&server.url("/file")).unwrap();
    assert!(
        http.send(
            reqwest::Method::GET,
            url.clone(),
            None,
            reqwest::header::HeaderMap::new(),
            false,
            false
        )
        .await
        .is_err()
    );
    assert!(server.requests.lock().unwrap().is_empty());
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::AUTHORIZATION,
        "Bearer attacker".parse().unwrap(),
    );
    http.send(reqwest::Method::GET, url, None, headers, true, false)
        .await
        .unwrap();
    assert!(
        !server.requests.lock().unwrap()[0]
            .to_lowercase()
            .contains("authorization")
    );
}

struct ScriptedEngine {
    staging: Arc<StagingArea>,
    error: Option<DownloadErrorCode>,
    calls: std::sync::atomic::AtomicUsize,
    active: std::sync::atomic::AtomicUsize,
    peak: std::sync::atomic::AtomicUsize,
    delay: std::time::Duration,
}
impl ScriptedEngine {
    fn new(staging: Arc<StagingArea>, error: Option<DownloadErrorCode>) -> Arc<Self> {
        Arc::new(Self {
            staging,
            error,
            calls: 0.into(),
            active: 0.into(),
            peak: 0.into(),
            delay: std::time::Duration::from_millis(50),
        })
    }
}
#[async_trait]
impl Downloader for ScriptedEngine {
    async fn transfer(
        &self,
        job: &DownloadJob,
        _: Arc<dyn ProgressSink>,
        cancel: CancelToken,
    ) -> Result<TransferResult> {
        use std::sync::atomic::Ordering::SeqCst;
        self.calls.fetch_add(1, SeqCst);
        let active = self.active.fetch_add(1, SeqCst) + 1;
        self.peak.fetch_max(active, SeqCst);
        tokio::select! {_=tokio::time::sleep(self.delay)=>{},_=super::native_engine::cancelled(&cancel)=>{self.active.fetch_sub(1,SeqCst);return Err(DownloadErrorCode::Cancelled.into());}}
        self.active.fetch_sub(1, SeqCst);
        if let Some(error) = self.error {
            return Err(error.into());
        }
        let key = StagingArea::part_key(job);
        fs::write(self.staging.root.path.join(&key), b"payload").unwrap();
        Ok(TransferResult {
            staged_key: key,
            validator: None,
            size: 7,
        })
    }
}
async fn wait_terminal(repository: &dyn JobRepository, id: &JobId) -> DownloadJob {
    let outcome = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let job = repository.get(id).await.unwrap();
            if !job.state.active() {
                return job;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await;
    match outcome {
        Ok(job) => job,
        Err(_) => {
            let job = repository.get(id).await.unwrap();
            panic!(
                "{} timed out in {:?} with error {:?}",
                id.as_str(),
                job.state,
                job.error
            );
        }
    }
}

#[tokio::test]
async fn supervisor_limits_admissions_and_only_falls_back_on_spawn_failure() {
    use std::sync::atomic::Ordering::SeqCst;
    for (error, expected_native, expected_state) in [
        (DownloadErrorCode::ToolLaunch, 1, DownloadState::Completed),
        (DownloadErrorCode::ToolRuntime, 0, DownloadState::Failed),
        (
            DownloadErrorCode::BandwidthProxyUnavailable,
            0,
            DownloadState::Failed,
        ),
    ] {
        let f = Fixture::new();
        let mut job = f.job("hf", "https://x/a.bin");
        job.effective_engine = DownloadEngine::HfCli;
        job.requested_engine = DownloadEngine::HfCli;
        let job = f.insert(job).await;
        let native = ScriptedEngine::new(f.staging.clone(), None);
        let hf = ScriptedEngine::new(f.staging.clone(), Some(error));
        let supervisor = super::supervisor::DownloadSupervisor::new(
            f.repository.clone(),
            f.files.clone(),
            native.clone(),
            hf.clone(),
            f.hub.clone(),
            2,
        );
        let run = tokio::spawn(supervisor.clone().run());
        let terminal = wait_terminal(f.repository.as_ref(), &job.id).await;
        assert_eq!(terminal.state, expected_state);
        assert_eq!(native.calls.load(SeqCst), expected_native);
        assert_eq!(hf.calls.load(SeqCst), 1);
        supervisor.request_shutdown();
        run.await.unwrap().unwrap();
    }
    let f = Fixture::new();
    let mut ids = Vec::new();
    for n in 0..5 {
        ids.push(
            f.insert(f.job(&format!("c{n}"), "https://x/a.bin"))
                .await
                .id,
        );
    }
    let native = ScriptedEngine::new(f.staging.clone(), None);
    let supervisor = super::supervisor::DownloadSupervisor::new(
        f.repository.clone(),
        f.files.clone(),
        native.clone(),
        native.clone(),
        f.hub.clone(),
        2,
    );
    let run = tokio::spawn(supervisor.clone().run());
    for id in ids {
        assert_eq!(
            wait_terminal(f.repository.as_ref(), &id).await.state,
            DownloadState::Completed
        );
    }
    assert_eq!(native.peak.load(SeqCst), 2);
    supervisor.request_shutdown();
    run.await.unwrap().unwrap();
}

#[tokio::test]
async fn catalog_failure_after_publication_is_recoverable_and_overwrite_retains_owned_backup() {
    let f = Fixture::new();
    let mut job = f.insert(f.job("catalog", "https://x/a.bin")).await;
    fs::write(f.staging.root.path.join("catalog.part"), b"new-file").unwrap();
    let (identity, digest) = f
        .files
        .verify("catalog.part", None, DownloadSize::Unknown)
        .await
        .unwrap();
    fs::create_dir_all(f.models.join("checkpoints")).unwrap();
    fs::write(f.models.join("checkpoints/catalog.bin"), b"old-file").unwrap();
    let old = f.files.inspect(&job.destination).await.unwrap().unwrap();
    job.conflict = Some(ConflictChoice::Overwrite {
        approved: old.clone(),
    });
    job.publication = Some(PublicationIntent {
        phase: FinalizePhase::Verified,
        staged_key: "catalog.part".into(),
        staged_identity: identity,
        digest,
        external_verified: false,
        backup_name: SafeFileName::new(".catalog.backup").unwrap(),
        published_identity: None,
    });
    job.state = DownloadState::Verifying;
    f.repository.save(&job, 0).await.unwrap();
    let published = f.files.publish(&job).await.unwrap();
    assert_eq!(
        fs::read(f.models.join("checkpoints/.catalog.backup")).unwrap(),
        b"old-file"
    );
    assert_eq!(
        fs::read(f.models.join("checkpoints/catalog.bin")).unwrap(),
        b"new-file"
    );
    // Simulate OS recycling separately so this deterministic test does not mutate the user's bin.
    fs::remove_file(f.models.join("checkpoints/.catalog.backup")).unwrap();
    let intent = job.publication.as_mut().unwrap();
    intent.phase = FinalizePhase::Published;
    intent.published_identity = Some(published);
    f.repository.save(&job, 0).await.unwrap();
    f.repository.db.run(|c|{c.execute_batch("CREATE TRIGGER fail_catalog BEFORE INSERT ON models BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();Ok(())}).await.unwrap();
    let finalizer = FinalizeDownload {
        repository: f.repository.clone(),
        files: f.files.clone(),
        events: f.hub.clone(),
    };
    assert!(finalizer.execute(&mut job).await.is_err());
    assert_eq!(
        f.repository
            .get(&job.id)
            .await
            .unwrap()
            .publication
            .unwrap()
            .phase,
        FinalizePhase::Published
    );
    f.repository
        .db
        .run(|c| {
            c.execute_batch("DROP TRIGGER fail_catalog;").unwrap();
            Ok(())
        })
        .await
        .unwrap();
    finalizer.execute(&mut job).await.unwrap();
    assert_eq!(job.state, DownloadState::Completed);
}

#[tokio::test]
async fn retry_deadline_survives_storage_and_is_not_claimed_early() {
    let f = Fixture::new();
    let mut job = f.insert(f.job("backoff", "https://x/a.bin")).await;
    job.retry_count = 3;
    job.retry_at = Some(100);
    f.repository.save(&job, 0).await.unwrap();
    assert!(f.repository.claim(99).await.unwrap().is_none());
    let claimed = f.repository.claim(100).await.unwrap().unwrap();
    assert_eq!(claimed.retry_count, 3);
    assert_eq!(claimed.generation, 1);
}

struct NoControl;
#[async_trait]
impl DownloadControl for NoControl {
    async fn stop(&self, _: &JobId, _: StopIntent) -> Result<DownloadJob> {
        Err(DownloadErrorCode::InvalidTransition.into())
    }
    fn wake(&self) {}
}

#[tokio::test]
async fn overwrite_needs_the_observed_identity_and_a_changed_file_raises_a_fresh_prompt() {
    let f = Fixture::new();
    let mut job = f.insert(f.job("late", "https://x/late.bin")).await;
    fs::create_dir_all(f.models.join("checkpoints")).unwrap();
    let path = f.models.join("checkpoints/late.bin");
    fs::write(&path, b"first").unwrap();
    let first = f.files.inspect(&job.destination).await.unwrap().unwrap();
    job.state = DownloadState::Paused;
    job.error = Some(DownloadErrorCode::Conflict);
    job.decision_id = Some("late-0-1".into());
    job.observed = Some(first.clone());
    f.repository.save(&job, 0).await.unwrap();
    // The prompt (and its observed identity) survive a restart.
    assert_eq!(
        f.repository.get(&job.id).await.unwrap().observed,
        Some(first.clone())
    );
    // The file is replaced after the prompt was shown.
    fs::write(&path, b"replaced by something else").unwrap();
    let resume = ResumeJob {
        repository: f.repository.clone(),
        files: f.files.clone(),
        events: f.hub.clone(),
        control: Arc::new(NoControl),
    };
    let stale = resume
        .execute(
            &job.id,
            Some(BoundConflictDecision {
                decision_id: "late-0-1".into(),
                choice: ConflictChoice::Overwrite {
                    approved: first.clone(),
                },
            }),
        )
        .await
        .unwrap_err();
    assert_eq!(stale.0, DownloadErrorCode::StaleDecision);
    // Nothing was approved: the job still waits, now with the newly observed identity.
    let refreshed = f.repository.get(&job.id).await.unwrap();
    assert_eq!(refreshed.state, DownloadState::Paused);
    assert_eq!(refreshed.conflict, None);
    let second = f.files.inspect(&job.destination).await.unwrap().unwrap();
    assert_ne!(second, first);
    assert_eq!(refreshed.observed, Some(second.clone()));
    let new_decision = refreshed.decision_id.clone().unwrap();
    assert_ne!(new_decision, "late-0-1");
    // The old decision cannot be replayed; echoing the freshly observed identity works.
    assert!(
        resume
            .execute(
                &job.id,
                Some(BoundConflictDecision {
                    decision_id: "late-0-1".into(),
                    choice: ConflictChoice::Overwrite {
                        approved: second.clone()
                    },
                }),
            )
            .await
            .is_err()
    );
    let resumed = resume
        .execute(
            &job.id,
            Some(BoundConflictDecision {
                decision_id: new_decision,
                choice: ConflictChoice::Overwrite { approved: second },
            }),
        )
        .await
        .unwrap();
    assert_eq!(resumed.state, DownloadState::Queued);
}

#[tokio::test]
async fn a_deferred_download_stack_creates_nothing_under_the_root() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("fresh install");
    fs::create_dir(&root).unwrap();
    let db = crate::shared::sqlite::Db::open(temp.path().join("app.db"))
        .await
        .unwrap();
    let stack = super::stack::DownloadStack::open_with_bandwidth(
        &root,
        db,
        Arc::new(NoCredentials),
        1,
        Arc::new(PathLocks::default()),
        None,
        false,
    )
    .unwrap();
    // Only the staging area exists; the ComfyUI folder is left for the setup clone.
    assert!(!root.join("ComfyUI").exists());
    let names: Vec<_> = fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names, ["data"]);
    // No model root is authorized, so a download has nowhere to publish until the next start.
    let target = RelativeModelPath::new("checkpoints/model.safetensors").unwrap();
    assert!(
        stack
            .files
            .roots
            .parent(super::stack::DEFAULT_MODEL_ROOT_ID, &target, true)
            .is_err()
    );
    assert!(!root.join("ComfyUI").exists());
}
