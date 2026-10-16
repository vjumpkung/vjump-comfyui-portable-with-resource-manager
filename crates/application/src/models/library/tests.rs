//! Library workflows against in-memory fakes: recovery, compensation, planned deletion and
//! identification, with fault injection at every file operation.
use super::*;
use crate::shared::{
    Clock, IdGenerator,
    path_locks::{PathLease, PathLockError, PathLocks},
};
use crate::test_support::block_on;
use async_trait::async_trait;
use domain::{
    models::{
        acquisition::FileIdentity,
        library::{ModelCategory, ModelFile, ModelLocation, SidecarSet},
    },
    shared::model_values::{RelativeModelPath, SafeFileName, Sha256},
};
use std::{
    collections::{BTreeMap, HashSet},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

fn loc(path: &str) -> ModelLocation {
    ModelLocation::new("models", RelativeModelPath::new(path).unwrap())
}

#[derive(Default)]
struct World {
    files: BTreeMap<String, (FileIdentity, Vec<u8>)>,
    rows: BTreeMap<String, ModelFile>,
    journal: BTreeMap<String, OperationRecord>,
    locked: HashSet<String>,
    reserved: HashSet<String>,
    trash_fails: HashSet<String>,
    fail_relocate_to: HashSet<String>,
    fail_all_relocations_after: Option<usize>,
    relocations: usize,
    hashes: usize,
    trashed: Vec<String>,
    sidecars_written: Vec<String>,
    mutate_during_lookup: Option<String>,
    changed: usize,
}
type Shared = Arc<Mutex<World>>;

fn identity(size: u64, n: u64) -> FileIdentity {
    FileIdentity {
        size,
        modified_ns: 1_000 + u128::from(n),
        platform_id: format!("id-{n}"),
        digest: None,
    }
}

struct Fake(Shared);
impl Fake {
    fn world(&self) -> std::sync::MutexGuard<'_, World> {
        self.0.lock().unwrap()
    }
}
#[async_trait]
impl ModelCatalog for Fake {
    async fn list(&self) -> Result<Vec<ModelFile>> {
        Ok(self.world().rows.values().cloned().collect())
    }
    async fn revision(&self) -> Result<u64> {
        Ok(1)
    }
    async fn get(&self, location: &ModelLocation) -> Result<Option<ModelFile>> {
        Ok(self.world().rows.get(&location.key()).cloned())
    }
    async fn accepts(&self, location: &ModelLocation) -> bool {
        location.root_id == "models" && location.path.components().len() >= 2
    }
    async fn apply_operation(
        &self,
        op_id: &str,
        from: &ModelLocation,
        to: &ModelLocation,
    ) -> Result<()> {
        let mut world = self.world();
        if let Some(mut row) = world.rows.remove(&from.key()) {
            row.location = to.clone();
            world.rows.insert(to.key(), row);
        }
        world.journal.remove(op_id);
        Ok(())
    }
    async fn remove(&self, location: &ModelLocation) -> Result<()> {
        self.world().rows.remove(&location.key());
        Ok(())
    }
    async fn cached_hash(
        &self,
        location: &ModelLocation,
        _: &FileIdentity,
    ) -> Result<Option<Sha256>> {
        Ok(self
            .world()
            .rows
            .get(&location.key())
            .and_then(|row| row.sha256.clone()))
    }
    async fn record_hash(
        &self,
        location: &ModelLocation,
        _: &FileIdentity,
        sha256: &Sha256,
    ) -> Result<()> {
        if let Some(row) = self.world().rows.get_mut(&location.key()) {
            row.sha256 = Some(sha256.clone());
        }
        Ok(())
    }
    async fn record_identification(
        &self,
        location: &ModelLocation,
        _: &FileIdentity,
        sha256: &Sha256,
        version: u64,
    ) -> Result<()> {
        if let Some(row) = self.world().rows.get_mut(&location.key()) {
            row.sha256 = Some(sha256.clone());
            row.civitai_version_id = Some(version);
        }
        Ok(())
    }
}
#[async_trait]
impl ModelFiles for Fake {
    async fn inspect(&self, location: &ModelLocation) -> Result<Option<FileIdentity>> {
        Ok(self.world().files.get(&location.key()).map(|f| f.0.clone()))
    }
    async fn list_directory(&self, directory: &ModelLocation) -> Result<Vec<SafeFileName>> {
        let prefix = format!("{}/", directory.key().trim_end_matches(':'));
        let prefix = prefix.replacen("models:/", "models:", 1);
        Ok(self
            .world()
            .files
            .keys()
            .filter_map(|key| key.strip_prefix(&prefix))
            .filter(|rest| !rest.contains('/'))
            .filter_map(|rest| SafeFileName::new(rest).ok())
            .collect())
    }
    async fn check_unlocked(&self, location: &ModelLocation) -> Result<()> {
        if self.world().locked.contains(&location.key()) {
            Err(LibraryErrorCode::Locked.into())
        } else {
            Ok(())
        }
    }
    async fn relocate(
        &self,
        from: &ModelLocation,
        to: &ModelLocation,
        _: Option<&FileIdentity>,
        _: &str,
    ) -> Result<Relocated> {
        let mut world = self.world();
        world.relocations += 1;
        if world
            .fail_all_relocations_after
            .is_some_and(|n| world.relocations > n)
        {
            return Err(LibraryErrorCode::Io.into());
        }
        if world.fail_relocate_to.contains(&to.key()) {
            return Err(LibraryErrorCode::Io.into());
        }
        if world.files.contains_key(&to.key()) {
            return Err(LibraryErrorCode::TargetExists.into());
        }
        let file = world
            .files
            .remove(&from.key())
            .ok_or(LibraryErrorCode::NotFound)?;
        world.files.insert(to.key(), file);
        Ok(Relocated::Renamed)
    }
    async fn trash(&self, location: &ModelLocation, _: &FileIdentity) -> Result<()> {
        let mut world = self.world();
        if world.trash_fails.contains(&location.key()) {
            return Err(LibraryErrorCode::TrashFailed.into());
        }
        world.files.remove(&location.key());
        world.trashed.push(location.key());
        Ok(())
    }
    async fn delete_permanently(&self, location: &ModelLocation, _: &FileIdentity) -> Result<()> {
        self.world().files.remove(&location.key());
        Ok(())
    }
    async fn hash(&self, _: &ModelLocation, _: &FileIdentity) -> Result<Sha256> {
        self.world().hashes += 1;
        Ok(Sha256::new(&"ab".repeat(32)).unwrap())
    }
    async fn write_sidecar(
        &self,
        location: &ModelLocation,
        bytes: &[u8],
        replace: bool,
    ) -> Result<()> {
        let mut world = self.world();
        if !replace && world.files.contains_key(&location.key()) {
            return Err(LibraryErrorCode::TargetExists.into());
        }
        world.files.insert(
            location.key(),
            (identity(bytes.len() as u64, 99), bytes.to_vec()),
        );
        world.sidecars_written.push(location.key());
        Ok(())
    }
    async fn read_sidecar(&self, location: &ModelLocation, _: usize) -> Result<Option<Vec<u8>>> {
        Ok(self.world().files.get(&location.key()).map(|f| f.1.clone()))
    }
}
#[async_trait]
impl OperationJournal for Fake {
    async fn begin(&self, record: &OperationRecord) -> Result<()> {
        self.world()
            .journal
            .insert(record.id.clone(), record.clone());
        Ok(())
    }
    async fn set_phase(&self, id: &str, phase: OperationPhase) -> Result<()> {
        if let Some(record) = self.world().journal.get_mut(id) {
            record.phase = phase;
        }
        Ok(())
    }
    async fn complete(&self, id: &str) -> Result<()> {
        self.world().journal.remove(id);
        Ok(())
    }
    async fn unfinished(&self) -> Result<Vec<OperationRecord>> {
        Ok(self.world().journal.values().cloned().collect())
    }
}
#[async_trait]
impl PathLocks for Fake {
    async fn acquire_many(
        &self,
        locations: Vec<ModelLocation>,
    ) -> std::result::Result<Box<dyn PathLease>, PathLockError> {
        let world = self.world();
        if locations.iter().any(|l| world.reserved.contains(&l.key())) {
            return Err(PathLockError::BusyReserved);
        }
        struct Lease;
        impl PathLease for Lease {}
        Ok(Box::new(Lease))
    }
}
#[async_trait]
impl CivitaiLookup for Fake {
    async fn by_hash(&self, sha256: &Sha256) -> Result<Option<CivitaiMatch>> {
        let mut world = self.world();
        if sha256.as_str().starts_with("00") {
            return Ok(None);
        }
        if let Some(path) = world.mutate_during_lookup.take() {
            let key = loc(&path).key();
            if let Some(file) = world.files.get_mut(&key) {
                file.0.modified_ns += 5;
            }
        }
        Ok(Some(CivitaiMatch {
            version: CivitaiVersion {
                version_id: 77,
                model_id: 7,
                model_name: "Model".into(),
                version_name: "v1".into(),
                base_model: Some("SDXL".into()),
                trigger_words: vec!["trigger".into()],
                image_url: Some("https://image/1".into()),
            },
            info_json: b"{\"versionId\":77}".to_vec(),
        }))
    }
    async fn preview_png(&self, _: &CivitaiVersion) -> Result<Option<Vec<u8>>> {
        Ok(Some(b"\x89PNG".to_vec()))
    }
}
#[async_trait]
impl ModelDetailsReader for Fake {
    async fn read(&self, _: &ModelLocation) -> Result<ModelDetails> {
        Ok(ModelDetails::default())
    }
}
#[async_trait]
impl LibraryControl for Fake {
    async fn rescan(&self) -> Result<()> {
        Ok(())
    }
    fn status(&self) -> LibraryStatus {
        LibraryStatus {
            sequence: 0,
            roots: Vec::new(),
            warnings: Vec::new(),
            scanning: false,
            watcher: WatcherState::Native,
        }
    }
    fn changed(&self) {
        self.world().changed += 1;
    }
}
struct Ids(AtomicU64);
impl IdGenerator for Ids {
    fn new_id(&self) -> String {
        format!("id{}", self.0.fetch_add(1, Ordering::SeqCst))
    }
}
struct FixedClock;
impl Clock for FixedClock {
    fn now(&self) -> String {
        "2026-10-16T00:00:00Z".into()
    }
}

fn setup() -> (Shared, LibraryDeps) {
    let shared: Shared = Arc::default();
    let fake = Arc::new(Fake(shared.clone()));
    let deps = LibraryDeps {
        catalog: fake.clone(),
        files: fake.clone(),
        journal: fake.clone(),
        locks: fake.clone(),
        civitai: fake.clone(),
        control: fake.clone(),
        details: fake,
        ids: Arc::new(Ids(AtomicU64::new(1))),
        clock: Arc::new(FixedClock),
        plans: Arc::default(),
    };
    (shared, deps)
}

fn add_model(shared: &Shared, path: &str, size: u64, n: u64) -> ModelLocation {
    let location = loc(path);
    let mut world = shared.lock().unwrap();
    world
        .files
        .insert(location.key(), (identity(size, n), Vec::new()));
    let identity = identity(size, n);
    world.rows.insert(
        location.key(),
        ModelFile {
            location: location.clone(),
            category: ModelCategory::new("checkpoints").unwrap(),
            size,
            modified_ns: identity.modified_ns,
            platform_id: identity.platform_id,
            sha256: None,
            source_job_id: None,
            civitai_version_id: None,
            sidecars: SidecarSet::default(),
        },
    );
    location
}
fn add_file(shared: &Shared, path: &str) {
    shared
        .lock()
        .unwrap()
        .files
        .insert(loc(path).key(), (identity(1, 50), Vec::new()));
}
fn has(shared: &Shared, path: &str) -> bool {
    shared.lock().unwrap().files.contains_key(&loc(path).key())
}

#[test]
fn rename_moves_the_model_and_its_three_sidecars_and_updates_the_catalog() {
    block_on(async {
        let (shared, deps) = setup();
        let model = add_model(&shared, "checkpoints/foo.safetensors", 10, 1);
        for sidecar in [
            "foo.preview.png",
            "foo.civitai.info",
            "foo.json",
            "foo.metadata.json",
        ] {
            add_file(&shared, &format!("checkpoints/{sidecar}"));
        }
        let placed = deps.rename(&model, "bar.safetensors", false).await.unwrap();
        assert_eq!(placed.location.display(), "checkpoints/bar.safetensors");
        for moved in [
            "bar.safetensors",
            "bar.preview.png",
            "bar.civitai.info",
            "bar.json",
        ] {
            assert!(has(&shared, &format!("checkpoints/{moved}")), "{moved}");
        }
        // Only the exact suffix set follows the model.
        assert!(has(&shared, "checkpoints/foo.metadata.json"));
        assert!(!has(&shared, "checkpoints/foo.json"));
        let world = shared.lock().unwrap();
        assert!(world.rows.contains_key(&placed.location.key()));
        assert!(!world.rows.contains_key(&model.key()));
        assert!(world.journal.is_empty());
        assert!(world.changed > 0);
    });
}

#[test]
fn a_locked_file_stays_unchanged_and_no_journal_entry_is_left() {
    block_on(async {
        let (shared, deps) = setup();
        let model = add_model(&shared, "checkpoints/a.safetensors", 10, 1);
        add_file(&shared, "checkpoints/a.json");
        shared
            .lock()
            .unwrap()
            .locked
            .insert(loc("checkpoints/a.json").key());
        let error = deps
            .rename(&model, "b.safetensors", false)
            .await
            .unwrap_err();
        assert_eq!(error.0, LibraryErrorCode::Locked);
        assert!(has(&shared, "checkpoints/a.safetensors"));
        assert!(has(&shared, "checkpoints/a.json"));
        let world = shared.lock().unwrap();
        assert!(world.journal.is_empty());
        assert_eq!(world.relocations, 0);
    });
}

#[test]
fn target_conflicts_ambiguous_stems_and_reserved_destinations_are_refused() {
    block_on(async {
        let (shared, deps) = setup();
        let model = add_model(&shared, "checkpoints/a.safetensors", 10, 1);
        add_file(&shared, "checkpoints/b.safetensors");
        assert_eq!(
            deps.rename(&model, "b.safetensors", false)
                .await
                .unwrap_err()
                .0,
            LibraryErrorCode::TargetExists
        );
        shared
            .lock()
            .unwrap()
            .reserved
            .insert(loc("checkpoints/c.safetensors").key());
        assert_eq!(
            deps.rename(&model, "c.safetensors", false)
                .await
                .unwrap_err()
                .0,
            LibraryErrorCode::BusyReserved
        );
        // Two model files share the stem `a`, so the sidecar belongs to both.
        add_file(&shared, "checkpoints/a.ckpt");
        add_file(&shared, "checkpoints/a.json");
        assert_eq!(
            deps.rename(&model, "d.safetensors", false)
                .await
                .unwrap_err()
                .0,
            LibraryErrorCode::AmbiguousSidecar
        );
        assert_eq!(
            deps.rename(&model, "../evil.safetensors", false)
                .await
                .unwrap_err()
                .0,
            LibraryErrorCode::InvalidName
        );
        assert_eq!(
            deps.rename(&model, "a.ckpt", false).await.unwrap_err().0,
            LibraryErrorCode::ExtensionChanged
        );
    });
}

#[test]
fn a_failing_member_is_compensated_and_the_journal_is_cleared() {
    block_on(async {
        let (shared, deps) = setup();
        let model = add_model(&shared, "checkpoints/a.safetensors", 10, 1);
        add_file(&shared, "checkpoints/a.preview.png");
        add_file(&shared, "checkpoints/a.json");
        shared
            .lock()
            .unwrap()
            .fail_relocate_to
            .insert(loc("checkpoints/b.json").key());
        let error = deps
            .rename(&model, "b.safetensors", false)
            .await
            .unwrap_err();
        assert_eq!(error.0, LibraryErrorCode::Io);
        for original in ["a.safetensors", "a.preview.png", "a.json"] {
            assert!(
                has(&shared, &format!("checkpoints/{original}")),
                "{original}"
            );
        }
        let world = shared.lock().unwrap();
        assert!(world.journal.is_empty());
        assert!(world.rows.contains_key(&model.key()));
    });
}

#[test]
fn a_failed_compensation_keeps_a_recoverable_journal_and_reports_recovery() {
    block_on(async {
        let (shared, deps) = setup();
        let model = add_model(&shared, "checkpoints/a.safetensors", 10, 1);
        add_file(&shared, "checkpoints/a.json");
        // The model rename succeeds, the sidecar fails, and restoring the model fails too.
        shared.lock().unwrap().fail_all_relocations_after = Some(1);
        let error = deps
            .rename(&model, "b.safetensors", false)
            .await
            .unwrap_err();
        assert_eq!(error.0, LibraryErrorCode::RecoveryRequired);
        let journal = shared.lock().unwrap().journal.clone();
        assert_eq!(journal.len(), 1);
        assert_eq!(
            journal.values().next().unwrap().phase,
            OperationPhase::RecoveryRequired
        );
        // Later mutations of those paths are blocked until recovery succeeds.
        shared.lock().unwrap().fail_all_relocations_after = None;
        let blocked = deps.rename(&model, "c.safetensors", false).await;
        assert_eq!(blocked.unwrap_err().0, LibraryErrorCode::RecoveryRequired);
    });
}

#[test]
fn startup_reconciliation_rolls_a_half_finished_rename_forward() {
    block_on(async {
        let (shared, deps) = setup();
        let model = add_model(&shared, "checkpoints/a.safetensors", 10, 1);
        add_file(&shared, "checkpoints/a.json");
        // Crash after the sidecar moved but before the model moved: the journal has the intent.
        let members = vec![
            OperationMember {
                from: model.clone(),
                to: loc("checkpoints/b.safetensors"),
                via: None,
                model: true,
            },
            OperationMember {
                from: loc("checkpoints/a.json"),
                to: loc("checkpoints/b.json"),
                via: None,
                model: false,
            },
        ];
        {
            let mut world = shared.lock().unwrap();
            let file = world
                .files
                .remove(&loc("checkpoints/a.json").key())
                .unwrap();
            world.files.insert(loc("checkpoints/b.json").key(), file);
            world.journal.insert(
                "crashed".into(),
                OperationRecord {
                    id: "crashed".into(),
                    kind: OperationKind::Rename,
                    phase: OperationPhase::Intent,
                    members,
                    created_at: "t".into(),
                },
            );
        }
        let report = deps.reconcile().await.unwrap();
        assert_eq!(report.completed, 1);
        assert!(has(&shared, "checkpoints/b.safetensors"));
        assert!(has(&shared, "checkpoints/b.json"));
        let world = shared.lock().unwrap();
        assert!(world.journal.is_empty());
        assert!(
            world
                .rows
                .contains_key(&loc("checkpoints/b.safetensors").key())
        );
    });
}

#[test]
fn an_unresolvable_journal_entry_is_marked_for_recovery_instead_of_guessing() {
    block_on(async {
        let (shared, deps) = setup();
        let model = add_model(&shared, "checkpoints/a.safetensors", 10, 1);
        add_file(&shared, "checkpoints/b.safetensors");
        shared.lock().unwrap().journal.insert(
            "both".into(),
            OperationRecord {
                id: "both".into(),
                kind: OperationKind::Move,
                phase: OperationPhase::Intent,
                members: vec![OperationMember {
                    from: model,
                    to: loc("checkpoints/b.safetensors"),
                    via: None,
                    model: true,
                }],
                created_at: "t".into(),
            },
        );
        let report = deps.reconcile().await.unwrap();
        assert_eq!((report.completed, report.blocked), (0, 1));
        assert_eq!(
            shared.lock().unwrap().journal["both"].phase,
            OperationPhase::RecoveryRequired
        );
    });
}

#[cfg(windows)]
#[test]
fn a_case_only_rename_uses_a_recorded_intermediate_name() {
    block_on(async {
        let (shared, deps) = setup();
        let model = add_model(&shared, "checkpoints/model.safetensors", 10, 1);
        // The fake treats keys case-insensitively like NTFS, so the final key equals the first.
        let placed = deps
            .rename(&model, "Model.safetensors", false)
            .await
            .unwrap();
        assert_eq!(placed.location.display(), "checkpoints/Model.safetensors");
        let world = shared.lock().unwrap();
        assert_eq!(world.relocations, 2);
        assert!(world.journal.is_empty());
    });
}

#[test]
fn delete_binds_the_confirmation_to_identities_and_total_bytes() {
    block_on(async {
        let (shared, deps) = setup();
        let a = add_model(&shared, "checkpoints/a.safetensors", 100, 1);
        let b = add_model(&shared, "loras/b.safetensors", 50, 2);
        let plan = deps
            .plan_delete(vec![a.clone(), b.clone(), a.clone()], DeleteKind::Trash)
            .await
            .unwrap();
        assert_eq!(plan.total_bytes, 150);
        assert_eq!(plan.items.len(), 2);
        // The file changed after the user confirmed the totals.
        shared
            .lock()
            .unwrap()
            .files
            .get_mut(&b.key())
            .unwrap()
            .0
            .modified_ns += 1;
        let report = deps
            .execute_delete(&plan.id, DeleteKind::Trash)
            .await
            .unwrap();
        assert_eq!(report.results[0].outcome, DeleteOutcome::Deleted);
        assert_eq!(report.results[1].outcome, DeleteOutcome::Stale);
        assert!(has(&shared, "loras/b.safetensors"));
        {
            let world = shared.lock().unwrap();
            assert!(!world.rows.contains_key(&a.key()));
            assert!(world.rows.contains_key(&b.key()));
        }
        // A confirmation is single-use.
        assert_eq!(
            deps.execute_delete(&plan.id, DeleteKind::Trash)
                .await
                .unwrap_err()
                .0,
            LibraryErrorCode::StaleDecision
        );
    });
}

#[test]
fn trash_failure_never_unlinks_and_needs_a_separate_permanent_confirmation() {
    block_on(async {
        let (shared, deps) = setup();
        let a = add_model(&shared, "checkpoints/a.safetensors", 100, 1);
        let b = add_model(&shared, "checkpoints/b.safetensors", 10, 2);
        shared.lock().unwrap().trash_fails.insert(a.key());
        let plan = deps
            .plan_delete(vec![a.clone(), b.clone()], DeleteKind::Trash)
            .await
            .unwrap();
        let report = deps
            .execute_delete(&plan.id, DeleteKind::Trash)
            .await
            .unwrap();
        assert_eq!(report.results[0].outcome, DeleteOutcome::TrashFailed);
        assert_eq!(report.results[1].outcome, DeleteOutcome::Deleted);
        assert!(has(&shared, "checkpoints/a.safetensors"));
        assert!(shared.lock().unwrap().rows.contains_key(&a.key()));
        let permanent = report.permanent_plan.expect("separate permanent plan");
        assert_eq!(permanent.kind, DeleteKind::Permanent);
        assert_eq!(permanent.items.len(), 1);
        // The trash confirmation cannot run the permanent plan and vice versa.
        assert_eq!(
            deps.execute_delete(&permanent.id, DeleteKind::Trash)
                .await
                .unwrap_err()
                .0,
            LibraryErrorCode::StaleDecision
        );
        let again = deps
            .plan_delete(vec![a.clone()], DeleteKind::Permanent)
            .await
            .unwrap();
        let done = deps
            .execute_delete(&again.id, DeleteKind::Permanent)
            .await
            .unwrap();
        assert_eq!(done.results[0].outcome, DeleteOutcome::Deleted);
        assert!(!has(&shared, "checkpoints/a.safetensors"));
        assert!(!shared.lock().unwrap().rows.contains_key(&a.key()));
    });
}

#[test]
fn locked_and_reserved_files_are_reported_per_file_and_not_deleted() {
    block_on(async {
        let (shared, deps) = setup();
        let a = add_model(&shared, "checkpoints/a.safetensors", 1, 1);
        let b = add_model(&shared, "checkpoints/b.safetensors", 1, 2);
        shared.lock().unwrap().locked.insert(a.key());
        shared.lock().unwrap().reserved.insert(b.key());
        let plan = deps
            .plan_delete(vec![a.clone(), b.clone()], DeleteKind::Trash)
            .await
            .unwrap();
        let report = deps
            .execute_delete(&plan.id, DeleteKind::Trash)
            .await
            .unwrap();
        assert_eq!(report.results[0].outcome, DeleteOutcome::Locked);
        assert_eq!(report.results[1].outcome, DeleteOutcome::Busy);
        assert!(shared.lock().unwrap().trashed.is_empty());
        assert!(report.permanent_plan.is_none());
    });
}

#[test]
fn identify_writes_curated_sidecars_and_records_the_version() {
    block_on(async {
        let (shared, deps) = setup();
        let model = add_model(&shared, "checkpoints/a.safetensors", 10, 1);
        let outcome = deps.identify(&model, false).await.unwrap();
        let IdentifyOutcome::Identified {
            version, preview, ..
        } = outcome
        else {
            panic!("expected an identified model");
        };
        assert_eq!(version.version_id, 77);
        assert_eq!(preview, PreviewOutcome::Written);
        assert!(has(&shared, "checkpoints/a.civitai.info"));
        assert!(has(&shared, "checkpoints/a.preview.png"));
        {
            let world = shared.lock().unwrap();
            assert_eq!(world.rows[&model.key()].civitai_version_id, Some(77));
            assert_eq!(world.hashes, 1);
        }
        // The digest is cached: a second run does not hash again and asks before replacing.
        let again = deps.identify(&model, false).await.unwrap();
        assert!(matches!(again, IdentifyOutcome::SidecarsExist { .. }));
        assert_eq!(shared.lock().unwrap().hashes, 1);
        let replaced = deps.identify(&model, true).await.unwrap();
        assert!(matches!(replaced, IdentifyOutcome::Identified { .. }));
    });
}

#[test]
fn identify_discards_a_response_for_a_file_that_changed_meanwhile() {
    block_on(async {
        let (shared, deps) = setup();
        let model = add_model(&shared, "checkpoints/a.safetensors", 10, 1);
        shared.lock().unwrap().mutate_during_lookup = Some("checkpoints/a.safetensors".into());
        let error = deps.identify(&model, false).await.unwrap_err();
        assert_eq!(error.0, LibraryErrorCode::Stale);
        assert!(!has(&shared, "checkpoints/a.civitai.info"));
        assert_eq!(
            shared.lock().unwrap().rows[&model.key()].civitai_version_id,
            None
        );
    });
}

#[test]
fn an_unknown_hash_is_a_normal_not_identified_result() {
    block_on(async {
        let (shared, deps) = setup();
        let model = add_model(&shared, "checkpoints/a.safetensors", 10, 1);
        shared
            .lock()
            .unwrap()
            .rows
            .get_mut(&model.key())
            .unwrap()
            .sha256 = Some(Sha256::new(&"00".repeat(32)).unwrap());
        let outcome = deps.identify(&model, false).await.unwrap();
        assert!(matches!(outcome, IdentifyOutcome::NotIdentified { .. }));
        assert!(shared.lock().unwrap().sidecars_written.is_empty());
    });
}

#[test]
fn moves_must_target_a_registered_location_below_a_category() {
    block_on(async {
        let (shared, deps) = setup();
        let model = add_model(&shared, "checkpoints/a.safetensors", 10, 1);
        let top = RelativeModelPath::default();
        assert_eq!(
            deps.move_to(&model, "models", top).await.unwrap_err().0,
            LibraryErrorCode::OutsideRoot
        );
        let placed = deps
            .move_to(
                &model,
                "models",
                RelativeModelPath::new("loras/sub").unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(placed.location.display(), "loras/sub/a.safetensors");
        assert!(has(&shared, "loras/sub/a.safetensors"));
    });
}
