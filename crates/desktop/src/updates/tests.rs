use std::{
    collections::VecDeque,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

use super::*;

type Events = Arc<Mutex<Vec<String>>>;

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    tauri::async_runtime::block_on(future)
}

fn log(events: &Events, entry: impl Into<String>) {
    events.lock().unwrap().push(entry.into());
}

fn entries(events: &Events) -> Vec<String> {
    events.lock().unwrap().clone()
}

fn position(events: &Events, entry: &str) -> usize {
    entries(events)
        .iter()
        .position(|logged| logged == entry)
        .unwrap_or_else(|| panic!("`{entry}` was never logged: {:?}", entries(events)))
}

struct FakeRelease {
    version: &'static str,
    events: Events,
    chunks: Vec<(u64, Option<u64>)>,
    download: Result<(), ReleaseFault>,
    install: Result<(), ReleaseFault>,
}

impl FakeRelease {
    fn new(version: &'static str, events: &Events) -> Self {
        Self {
            version,
            events: events.clone(),
            chunks: vec![(4, Some(4))],
            download: Ok(()),
            install: Ok(()),
        }
    }
}

#[async_trait]
impl Release for FakeRelease {
    fn summary(&self) -> ReleaseSummary {
        ReleaseSummary {
            version: self.version.to_owned(),
            date: Some("2026-10-17".into()),
            notes: Some("<b>notes</b>".into()),
        }
    }

    async fn download(
        &self,
        on_chunk: &mut (dyn FnMut(u64, Option<u64>) + Send),
    ) -> Result<Vec<u8>, ReleaseFault> {
        log(&self.events, format!("download:{}", self.version));
        for (chunk, total) in &self.chunks {
            on_chunk(*chunk, *total);
        }
        self.download.map(|()| vec![0; 4])
    }

    fn install(&self, _: Vec<u8>) -> Result<(), ReleaseFault> {
        // On Windows the real installer ends the process here; the order is what matters.
        log(&self.events, format!("install:{}", self.version));
        self.install
    }

    fn restart(&self) {
        log(&self.events, "restart");
    }
}

/// Answers each check with the next prepared release (or none).
struct FakeChannel {
    events: Events,
    answers: Mutex<VecDeque<Option<FakeRelease>>>,
    failing: Arc<AtomicBool>,
}

#[async_trait]
impl ReleaseChannel for FakeChannel {
    async fn check(&self) -> Result<Option<Box<dyn Release>>, ReleaseFault> {
        log(&self.events, "check");
        if self.failing.load(Ordering::SeqCst) {
            return Err(ReleaseFault::Other);
        }
        let answer = self.answers.lock().unwrap().pop_front().flatten();
        Ok(answer.map(|release| Box::new(release) as Box<dyn Release>))
    }
}

struct Quiescer(Events);

#[async_trait]
impl Quiesce for Quiescer {
    async fn stop_downloads(&self) {
        log(&self.0, "stop:downloads");
    }

    async fn stop_library(&self) {
        log(&self.0, "stop:library");
    }

    fn flush_logs(&self) {
        log(&self.0, "flush:logs");
    }

    async fn restore(&self) {
        log(&self.0, "restore:library");
    }
}

#[derive(Default)]
struct CountingIds(AtomicUsize);

impl IdGenerator for CountingIds {
    fn new_id(&self) -> String {
        format!("id-{}", self.0.fetch_add(1, Ordering::SeqCst) + 1)
    }
}

struct LoggingSink(Events);

impl UpdateProgressSink for LoggingSink {
    fn report(&self, progress: UpdateProgress) {
        log(&self.0, format!("sink:{progress:?}"));
    }
}

fn config() -> UpdaterBuildConfig {
    UpdaterBuildConfig::parse(
        Some("dW50cnVzdGVkIGNvbW1lbnQ6IHRlc3Q="),
        Some("https://updates.example.test/{{target}}/{{arch}}/{{current_version}}"),
    )
    .unwrap()
}

fn configured_updater(events: &Events, answers: Vec<Option<FakeRelease>>) -> SignedUpdater {
    SignedUpdater::new(
        Some(config()),
        |_| {
            Box::new(FakeChannel {
                events: events.clone(),
                answers: Mutex::new(answers.into()),
                failing: Arc::default(),
            })
        },
        Arc::new(Quiescer(events.clone())),
        Arc::new(CountingIds::default()),
    )
}

fn sink(events: &Events) -> Arc<dyn UpdateProgressSink> {
    Arc::new(LoggingSink(events.clone()))
}

#[test]
fn updater_without_complete_config_never_checks_network() {
    // Each of these is incomplete or invalid and leaves the build unconfigured.
    let key = Some("dW50cnVzdGVkIGNvbW1lbnQ6IHRlc3Q=");
    let endpoint = Some("https://updates.example.test/latest.json");
    for (pubkey, endpoint) in [
        (None, None),
        (key, None),
        (None, endpoint),
        (Some("  "), endpoint),
        (Some("not base64 !"), endpoint),
        (key, Some("")),
        (key, Some("not a url")),
        (key, Some("http://updates.example.test/latest.json")),
        (key, Some("file:///C:/updates/latest.json")),
        (key, Some("https://")),
    ] {
        assert_eq!(
            UpdaterBuildConfig::parse(pubkey, endpoint),
            None,
            "{pubkey:?} {endpoint:?}"
        );
    }

    let channels_made = AtomicUsize::new(0);
    let events = Events::default();
    let unconfigured = SignedUpdater::new(
        UpdaterBuildConfig::parse(key, None),
        |_| {
            channels_made.fetch_add(1, Ordering::SeqCst);
            Box::new(FakeChannel {
                events: events.clone(),
                answers: Mutex::default(),
                failing: Arc::default(),
            })
        },
        Arc::new(Quiescer(events.clone())),
        Arc::new(CountingIds::default()),
    );
    assert!(!unconfigured.configured());
    assert_eq!(
        block_on(unconfigured.check()),
        Err(UpdateError::NotConfigured)
    );
    assert_eq!(
        block_on(unconfigured.install("any", sink(&events))),
        Err(UpdateError::NotConfigured)
    );
    assert_eq!(channels_made.load(Ordering::SeqCst), 0);
    assert!(entries(&events).is_empty());
    assert!(!SignedUpdater::disabled().configured());
    assert_eq!(
        block_on(SignedUpdater::disabled().check()),
        Err(UpdateError::NotConfigured)
    );

    // A complete configuration is surrounded by whitespace-tolerant parsing and exposes
    // exactly the key and the one HTTPS endpoint to the plugin.
    let complete = UpdaterBuildConfig::parse(
        Some(" dW50cnVzdGVkIGNvbW1lbnQ6IHRlc3Q= \n"),
        Some("https://updates.example.test/latest.json"),
    )
    .unwrap();
    assert_eq!(
        complete.plugin_config(),
        serde_json::json!({
            "pubkey": "dW50cnVzdGVkIGNvbW1lbnQ6IHRlc3Q=",
            "endpoints": ["https://updates.example.test/latest.json"],
        })
    );
    assert!(configured_updater(&events, Vec::new()).configured());
}

#[test]
fn update_install_requires_the_reviewed_offer() {
    let events = Events::default();
    let updater = configured_updater(
        &events,
        vec![
            Some(FakeRelease::new("1.1.0", &events)),
            Some(FakeRelease::new("1.2.0", &events)),
            None,
        ],
    );

    // No offer exists yet: nothing is checked, downloaded or installed.
    assert_eq!(
        block_on(updater.install("id-1", sink(&events))),
        Err(UpdateError::Stale)
    );
    assert!(entries(&events).is_empty());

    let first = block_on(updater.check()).unwrap().unwrap();
    assert_eq!(first.update_id, "id-1");
    assert_eq!(first.version, "1.1.0");
    assert_eq!(first.notes.as_deref(), Some("<b>notes</b>"));

    // A different id never touches the reviewed release, and leaves the offer in place.
    assert_eq!(
        block_on(updater.install("id-2", sink(&events))),
        Err(UpdateError::Stale)
    );
    assert_eq!(entries(&events), ["check"]);

    // A newer check supersedes the offer: the earlier id is now stale.
    let second = block_on(updater.check()).unwrap().unwrap();
    assert_eq!(second.update_id, "id-2");
    assert_eq!(
        block_on(updater.install(&first.update_id, sink(&events))),
        Err(UpdateError::Stale)
    );

    // Installing names the reviewed offer and installs exactly that release, once.
    assert_eq!(
        block_on(updater.install(&second.update_id, sink(&events))),
        Ok(())
    );
    let logged = entries(&events);
    assert!(logged.contains(&"download:1.2.0".to_owned()), "{logged:?}");
    assert!(logged.contains(&"install:1.2.0".to_owned()));
    assert!(!logged.iter().any(|entry| entry.contains("1.1.0")));
    assert_eq!(
        block_on(updater.install(&second.update_id, sink(&events))),
        Err(UpdateError::Stale)
    );

    // A check that finds nothing withdraws the offer.
    let third = SignedUpdater::new(
        Some(config()),
        |_| {
            Box::new(FakeChannel {
                events: events.clone(),
                answers: Mutex::new(vec![Some(FakeRelease::new("1.3.0", &events)), None].into()),
                failing: Arc::default(),
            })
        },
        Arc::new(Quiescer(events.clone())),
        Arc::new(CountingIds::default()),
    );
    let offer = block_on(third.check()).unwrap().unwrap();
    assert_eq!(block_on(third.check()), Ok(None));
    assert_eq!(
        block_on(third.install(&offer.update_id, sink(&events))),
        Err(UpdateError::Stale)
    );
}

#[test]
fn failed_check_reports_check_failed_and_keeps_the_reviewed_offer() {
    let events = Events::default();
    let failing = Arc::new(AtomicBool::new(false));
    let updater = SignedUpdater::new(
        Some(config()),
        |_| {
            Box::new(FakeChannel {
                events: events.clone(),
                answers: Mutex::new(vec![Some(FakeRelease::new("1.1.0", &events))].into()),
                failing: failing.clone(),
            })
        },
        Arc::new(Quiescer(events.clone())),
        Arc::new(CountingIds::default()),
    );
    let offer = block_on(updater.check()).unwrap().unwrap();

    failing.store(true, Ordering::SeqCst);
    assert_eq!(block_on(updater.check()), Err(UpdateError::CheckFailed));

    // The reviewed release survives a check that could not reach the server.
    assert_eq!(
        block_on(updater.install(&offer.update_id, sink(&events))),
        Ok(())
    );
}

#[test]
fn invalid_signature_never_invokes_installer() {
    let events = Events::default();
    let mut release = FakeRelease::new("1.1.0", &events);
    release.download = Err(ReleaseFault::Signature);
    let updater = configured_updater(&events, vec![Some(release)]);
    let offer = block_on(updater.check()).unwrap().unwrap();

    assert_eq!(
        block_on(updater.install(&offer.update_id, sink(&events))),
        Err(UpdateError::SignatureInvalid)
    );
    let logged = entries(&events);
    assert!(logged.contains(&"download:1.1.0".to_owned()));
    for forbidden in [
        "stop:downloads",
        "stop:library",
        "flush:logs",
        "restore:library",
        "install:1.1.0",
        "restart",
    ] {
        assert!(
            !logged.contains(&forbidden.to_owned()),
            "{forbidden} ran after a failed verification: {logged:?}"
        );
    }
    assert!(!logged.iter().any(|entry| entry.contains("Installing")));
    // The failed release is gone; the user must check and review again.
    assert_eq!(
        block_on(updater.install(&offer.update_id, sink(&events))),
        Err(UpdateError::Stale)
    );

    // The installer re-verifies: a rejection there is also a signature failure.
    let events = Events::default();
    let mut release = FakeRelease::new("1.1.0", &events);
    release.install = Err(ReleaseFault::Signature);
    let updater = configured_updater(&events, vec![Some(release)]);
    let offer = block_on(updater.check()).unwrap().unwrap();
    assert_eq!(
        block_on(updater.install(&offer.update_id, sink(&events))),
        Err(UpdateError::SignatureInvalid)
    );
    assert!(!entries(&events).contains(&"restart".to_owned()));
}

#[test]
fn update_cleanup_precedes_windows_installer_exit() {
    let events = Events::default();
    let updater = configured_updater(&events, vec![Some(FakeRelease::new("1.1.0", &events))]);
    let offer = block_on(updater.check()).unwrap().unwrap();

    block_on(updater.install(&offer.update_id, sink(&events))).unwrap();

    assert_eq!(
        entries(&events),
        [
            "check",
            "download:1.1.0",
            "sink:Downloading { downloaded: 4, total: Some(4) }",
            "stop:downloads",
            "stop:library",
            "sink:Installing",
            "flush:logs",
            "install:1.1.0",
            "sink:Restarting",
            "restart",
        ]
    );
    for writer in ["stop:downloads", "stop:library", "flush:logs"] {
        assert!(position(&events, writer) < position(&events, "install:1.1.0"));
    }
    assert!(!entries(&events).contains(&"restore:library".to_owned()));
}

#[test]
fn failed_install_restores_restartable_background_work() {
    // Both installer faults must restore the library after cleanup, without restarting the app.
    for (fault, expected) in [
        (ReleaseFault::Other, UpdateError::InstallFailed),
        (ReleaseFault::Signature, UpdateError::SignatureInvalid),
    ] {
        let events = Events::default();
        let mut release = FakeRelease::new("1.1.0", &events);
        release.install = Err(fault);
        let updater = configured_updater(&events, vec![Some(release)]);
        let offer = block_on(updater.check()).unwrap().unwrap();
        assert_eq!(
            block_on(updater.install(&offer.update_id, sink(&events))),
            Err(expected)
        );
        for writer in ["stop:downloads", "stop:library", "flush:logs"] {
            assert!(position(&events, writer) < position(&events, "install:1.1.0"));
        }
        assert!(position(&events, "install:1.1.0") < position(&events, "restore:library"));
        assert!(!entries(&events).contains(&"restart".to_owned()));
    }
}

#[test]
fn app_quiesce_drains_buffered_logs_before_returning() {
    let directory = tempfile::tempdir().unwrap();
    let guard = Arc::new(Mutex::new(Some(logging::init(directory.path()).unwrap())));
    let app = tauri::test::mock_app();
    let quiesce = AppQuiesce::new(app.handle().clone(), guard.clone());
    tracing::info!("buffered line immediately before installation");
    quiesce.flush_logs();
    assert!(guard.lock().unwrap().is_none());
    let output = std::fs::read_to_string(directory.path().join("app.log")).unwrap();
    assert!(output.contains("buffered line immediately before installation"));
    quiesce.flush_logs();
}

#[test]
fn installer_error_is_redacted_and_persisted_without_the_tracing_worker() {
    let directory = tempfile::tempdir().unwrap();
    logging::register_secret("installer-private-secret");
    std::fs::write(directory.path().join("app.log"), "previous lines\n").unwrap();
    log_install_failure(Some(directory.path()), "installer-private-secret failed");
    let output = std::fs::read_to_string(directory.path().join("app.log")).unwrap();
    assert!(output.starts_with("previous lines\n"));
    assert!(output.contains("the update could not be installed: [REDACTED] failed"));
    assert!(!output.contains("installer-private-secret"));
}

#[test]
fn download_progress_is_throttled_but_always_ends_with_the_total() {
    let events = Events::default();
    let mut release = FakeRelease::new("1.1.0", &events);
    release.chunks = (0..50).map(|_| (2, Some(100))).collect();
    let updater = configured_updater(&events, vec![Some(release)]);
    let offer = block_on(updater.check()).unwrap().unwrap();
    block_on(updater.install(&offer.update_id, sink(&events))).unwrap();

    let progress: Vec<String> = entries(&events)
        .into_iter()
        .filter(|entry| entry.starts_with("sink:Downloading"))
        .collect();
    assert_eq!(
        progress.first().map(String::as_str),
        Some("sink:Downloading { downloaded: 2, total: Some(100) }")
    );
    assert_eq!(
        progress.last().map(String::as_str),
        Some("sink:Downloading { downloaded: 100, total: Some(100) }")
    );
    assert!(progress.len() < 50, "{}", progress.len());
}
