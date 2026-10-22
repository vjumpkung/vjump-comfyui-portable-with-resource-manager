//! Gallery workflows against a recording fake: gate use, request screening, deduplication and
//! the single-use delete plans.
use super::*;
use crate::{
    environment::{
        RootInfo, RootSource, RootSwitchError, RootSwitcher,
        tests::fakes::{FakeGate, ROOT},
    },
    shared::{ExplorerTarget, GateError, IdGenerator, MaintenanceGate, Retained},
    test_support::block_on,
};
use async_trait::async_trait;
use domain::{
    gallery::{GalleryArea, GalleryEntryKind, GalleryLocation},
    shared::model_values::{RelativeModelPath, SafeFileName},
};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

fn place(area: GalleryArea, path: &str) -> GalleryLocation {
    GalleryLocation::new(area, RelativeModelPath::new(path).unwrap()).unwrap()
}
fn out(path: &str) -> GalleryLocation {
    place(GalleryArea::Output, path)
}
fn entry(path: &str) -> GalleryEntryRef {
    GalleryEntryRef {
        location: out(path),
        revision: EntryRevision(format!("rev:{path}")),
    }
}
fn text(location: &GalleryLocation) -> String {
    format!(
        "{}:{}",
        location.area().dir_name(),
        location.path().as_string()
    )
}
fn code(error: GalleryErrorCode) -> Option<GalleryError> {
    Some(GalleryError(error))
}

#[derive(Default)]
struct State {
    calls: Vec<String>,
    roots: Vec<PathBuf>,
    trash_fails: HashSet<String>,
    plan_fails: HashSet<String>,
    transfer_fails: HashSet<String>,
    /// Whether each transfer was handed protection from the gate's lease.
    protected: Vec<bool>,
    /// When set, transfers keep the protection they are handed, as a slow adapter would.
    keep_protection: bool,
    kept: Vec<Retained>,
}

#[derive(Default)]
struct FakeFiles(Mutex<State>);
impl FakeFiles {
    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.0.lock().unwrap()
    }
    fn record(&self, root: &Path, call: String) {
        let mut state = self.state();
        state.roots.push(root.to_owned());
        state.calls.push(call);
    }
    fn calls(&self) -> Vec<String> {
        self.state().calls.clone()
    }
    fn count(&self, prefix: &str) -> usize {
        self.calls()
            .iter()
            .filter(|call| call.starts_with(prefix))
            .count()
    }
}

struct Media(Vec<u8>);
impl GalleryMedia for Media {
    fn len(&self) -> u64 {
        self.0.len() as u64
    }
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> std::io::Result<usize> {
        let start = (offset as usize).min(self.0.len());
        let n = buf.len().min(self.0.len() - start);
        buf[..n].copy_from_slice(&self.0[start..start + n]);
        Ok(n)
    }
}

#[async_trait]
impl GalleryFiles for FakeFiles {
    async fn list(&self, root: &Path, directory: &GalleryLocation) -> Result<GalleryListing> {
        self.record(root, format!("list:{}", text(directory)));
        Ok(GalleryListing {
            directory: directory.clone(),
            entries: Vec::new(),
            warnings: Vec::new(),
        })
    }
    async fn rename(
        &self,
        root: &Path,
        entry: &GalleryEntryRef,
        new_name: &SafeFileName,
    ) -> Result<GalleryLocation> {
        self.record(
            root,
            format!("rename:{}->{}", text(&entry.location), new_name.as_str()),
        );
        Ok(entry
            .location
            .parent()
            .unwrap()
            .join(new_name.clone())
            .unwrap())
    }
    async fn transfer(
        &self,
        root: &Path,
        entry: &GalleryEntryRef,
        destination: &GalleryLocation,
        mode: TransferMode,
        operation: &str,
        retained: Retained,
    ) -> ItemResult {
        {
            let mut state = self.state();
            state.protected.push(!retained.is_empty());
            if state.keep_protection {
                state.kept.push(retained);
            }
        }
        self.record(
            root,
            format!(
                "transfer:{}->{}:{mode:?}:{operation}",
                text(&entry.location),
                text(destination)
            ),
        );
        if self.state().transfer_fails.contains(&text(&entry.location)) {
            return ItemResult {
                source: entry.location.clone(),
                destination: None,
                outcome: ItemOutcome::Failed,
                error: code(GalleryErrorCode::Locked),
            };
        }
        ItemResult {
            source: entry.location.clone(),
            destination: Some(
                destination
                    .join(entry.location.name().unwrap().clone())
                    .unwrap(),
            ),
            outcome: ItemOutcome::Succeeded,
            error: None,
        }
    }
    async fn plan_delete(&self, root: &Path, entry: &GalleryEntryRef) -> Result<DeletePlanItem> {
        self.record(root, format!("plan:{}", text(&entry.location)));
        if self.state().plan_fails.contains(&text(&entry.location)) {
            return Err(GalleryErrorCode::Stale.into());
        }
        Ok(DeletePlanItem {
            entry: entry.clone(),
            kind: GalleryEntryKind::File,
            bytes: 10,
            fingerprint: format!("fp:{}", text(&entry.location)),
        })
    }
    async fn trash(&self, root: &Path, item: &DeletePlanItem) -> Result<()> {
        let location = text(&item.entry.location);
        self.record(root, format!("trash:{location}"));
        if self.state().trash_fails.contains(&location) {
            Err(GalleryErrorCode::TrashFailed.into())
        } else {
            Ok(())
        }
    }
    async fn open_media(
        &self,
        root: &Path,
        location: &GalleryLocation,
    ) -> Result<Box<dyn GalleryMedia>> {
        self.record(root, format!("open:{}", text(location)));
        Ok(Box::new(Media(b"preview bytes".to_vec())))
    }
    async fn explorer_target(
        &self,
        root: &Path,
        location: &GalleryLocation,
    ) -> Result<ExplorerTarget> {
        self.record(root, format!("locate:{}", text(location)));
        Ok(ExplorerTarget::RevealItem(
            root.join(location.path().as_string()),
        ))
    }
    async fn cleanup_staging(&self, root: &Path) -> Vec<GalleryWarning> {
        self.record(root, "cleanup".into());
        Vec::new()
    }
}

struct FixedRoot;
#[async_trait]
impl RootSwitcher for FixedRoot {
    fn current(&self) -> RootInfo {
        RootInfo {
            root: PathBuf::from(ROOT),
            source: RootSource::Default,
        }
    }
    async fn switch_root(
        &self,
        _: &Path,
        _: Option<&str>,
    ) -> std::result::Result<(), RootSwitchError> {
        Ok(())
    }
}

struct Ids(AtomicU64);
impl IdGenerator for Ids {
    fn new_id(&self) -> String {
        format!("id{}", self.0.fetch_add(1, Ordering::SeqCst))
    }
}

struct Harness {
    files: Arc<FakeFiles>,
    gate: Arc<FakeGate>,
    deps: GalleryDeps,
}
impl Harness {
    fn new() -> Self {
        let files = Arc::new(FakeFiles::default());
        let gate = Arc::new(FakeGate::default());
        let deps = GalleryDeps {
            root: Arc::new(FixedRoot),
            files: files.clone(),
            gate: gate.clone(),
            ids: Arc::new(Ids(AtomicU64::new(1))),
            plans: Arc::default(),
        };
        Self { files, gate, deps }
    }
    fn hold_gate(&self) -> Box<dyn crate::shared::MaintenanceLease> {
        block_on(self.gate.try_acquire()).unwrap()
    }
    fn transfer(
        &self,
        items: Vec<GalleryEntryRef>,
        destination: GalleryLocation,
        mode: TransferMode,
    ) -> Result<Vec<ItemResult>> {
        block_on(TransferGalleryEntries::new(self.deps.clone()).execute(items, destination, mode))
    }
    fn plan(&self, items: Vec<GalleryEntryRef>) -> Result<DeletePlan> {
        block_on(PlanGalleryDelete::new(self.deps.clone()).execute(items))
    }
    fn delete(&self, id: &str) -> Result<Vec<ItemResult>> {
        block_on(DeleteGalleryEntries::new(self.deps.clone()).execute(id))
    }
    fn rename(&self, path: &str, new_name: &str) -> Result<GalleryLocation> {
        block_on(RenameGalleryEntry::new(self.deps.clone()).execute(&entry(path), new_name))
    }
}

#[test]
fn errors_convert_from_codes_and_the_gate() {
    assert_eq!(
        GalleryError::from(GateError::Busy),
        GalleryError(GalleryErrorCode::Busy)
    );
    assert_eq!(
        GalleryError::from(GateError::Unavailable),
        GalleryError(GalleryErrorCode::Io)
    );
    assert_eq!(
        GalleryError::from(GalleryErrorCode::Stale),
        GalleryError(GalleryErrorCode::Stale)
    );
}

#[test]
fn listing_uses_the_current_root_without_the_gate() {
    let h = Harness::new();
    let _busy = h.hold_gate();
    let listing = block_on(ListGallery::new(h.deps.clone()).execute(&out("shots/day1"))).unwrap();
    assert_eq!(listing.directory, out("shots/day1"));
    assert_eq!(h.files.calls(), ["list:output:shots/day1"]);
    assert_eq!(h.files.state().roots, [PathBuf::from(ROOT)]);
    assert_eq!(
        h.gate.try_acquires.load(Ordering::SeqCst),
        1,
        "only the test's own lease"
    );
}

#[test]
fn rename_needs_the_gate_and_leaves_it_free_afterwards() {
    let h = Harness::new();
    let lease = h.hold_gate();
    assert_eq!(
        h.rename("a.png", "b.png"),
        Err(GalleryErrorCode::Busy.into())
    );
    assert!(h.files.calls().is_empty());
    drop(lease);
    assert_eq!(h.rename("shots/a.png", "b.png"), Ok(out("shots/b.png")));
    assert_eq!(h.files.calls(), ["rename:output:shots/a.png->b.png"]);
    assert_eq!(h.rename("shots/b.png", "c.png"), Ok(out("shots/c.png")));
}

#[test]
fn rename_screens_the_request_before_the_adapter() {
    let h = Harness::new();
    let root = GalleryEntryRef {
        location: GalleryLocation::root(GalleryArea::Output),
        revision: EntryRevision("r".into()),
    };
    assert_eq!(
        block_on(RenameGalleryEntry::new(h.deps.clone()).execute(&root, "x")),
        Err(GalleryErrorCode::InvalidRequest.into())
    );
    for hostile in [
        "",
        "a/b.png",
        "a\\b.png",
        "..",
        "CON",
        "trailing.",
        ".vjump-gallery-staging",
        ".VJUMP-GALLERY-Staging",
    ] {
        assert_eq!(
            h.rename("a.png", hostile),
            Err(GalleryErrorCode::InvalidName.into()),
            "{hostile:?}"
        );
    }
    assert!(h.files.calls().is_empty());
}

#[test]
fn renaming_to_the_identical_name_is_a_no_op_but_a_case_change_is_not() {
    let h = Harness::new();
    assert_eq!(h.rename("shots/a.png", "a.png"), Ok(out("shots/a.png")));
    assert!(h.files.calls().is_empty());
    assert_eq!(h.rename("shots/a.png", "A.png"), Ok(out("shots/A.png")));
    assert_eq!(h.files.calls(), ["rename:output:shots/a.png->A.png"]);
}

#[test]
fn transfer_rejects_empty_and_oversized_batches_before_touching_anything() {
    let h = Harness::new();
    assert_eq!(
        h.transfer(Vec::new(), out("dest"), TransferMode::Copy),
        Err(GalleryErrorCode::InvalidRequest.into())
    );
    let too_many: Vec<_> = (0..=MAX_ITEMS)
        .map(|n| entry(&format!("f{n}.png")))
        .collect();
    assert_eq!(
        h.transfer(too_many, out("dest"), TransferMode::Move),
        Err(GalleryErrorCode::InvalidRequest.into())
    );
    let limit: Vec<_> = (0..MAX_ITEMS)
        .map(|n| entry(&format!("f{n}.png")))
        .collect();
    let results = h.transfer(limit, out("dest"), TransferMode::Copy).unwrap();
    assert_eq!(results.len(), MAX_ITEMS);
    assert_eq!(
        h.gate.try_acquires.load(Ordering::SeqCst),
        1,
        "only the valid batch"
    );
}

#[test]
fn transfer_is_refused_while_the_gate_is_busy() {
    let h = Harness::new();
    let lease = h.hold_gate();
    assert_eq!(
        h.transfer(vec![entry("a.png")], out("dest"), TransferMode::Move),
        Err(GalleryErrorCode::Busy.into())
    );
    assert!(
        h.files.calls().is_empty(),
        "not even the staging cleanup runs"
    );
    drop(lease);
    assert!(
        h.transfer(vec![entry("a.png")], out("dest"), TransferMode::Move)
            .is_ok()
    );
}

#[test]
fn transfer_cleans_staging_once_then_uses_a_fresh_operation_per_item() {
    let h = Harness::new();
    let results = h
        .transfer(
            vec![entry("a.png"), entry("b.png"), entry("sub/c.png")],
            out("dest"),
            TransferMode::Copy,
        )
        .unwrap();
    assert_eq!(
        h.files.calls(),
        [
            "cleanup",
            "transfer:output:a.png->output:dest:Copy:id1",
            "transfer:output:b.png->output:dest:Copy:id2",
            "transfer:output:sub/c.png->output:dest:Copy:id3",
        ]
    );
    assert_eq!(h.files.count("cleanup"), 1);
    assert!(results.iter().all(|r| r.outcome == ItemOutcome::Succeeded));
    assert_eq!(results[2].destination, Some(out("dest/c.png")));
    assert_eq!(h.files.state().roots, vec![PathBuf::from(ROOT); 4]);
}

#[test]
fn every_transfer_is_handed_the_gate_protection_to_keep_until_its_work_ends() {
    let h = Harness::new();
    h.files.state().keep_protection = true;
    let results = h
        .transfer(
            vec![entry("a.png"), entry("b.png")],
            out("dest"),
            TransferMode::Move,
        )
        .unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(h.files.state().protected, [true, true]);
    // The use case has returned and dropped its lease, but the protection it handed to the
    // adapter still holds the gate until the adapter lets go.
    assert_eq!(
        block_on(h.gate.try_acquire()).err(),
        Some(GateError::Busy),
        "the adapter's protection keeps the gate busy"
    );
    h.files.state().kept.clear();
    assert!(block_on(h.gate.try_acquire()).is_ok());
}

#[test]
fn transfer_dedupes_by_location_and_keeps_the_first_reference() {
    let h = Harness::new();
    let mut again = entry("a.png");
    again.revision = EntryRevision("newer".into());
    let results = h
        .transfer(
            vec![entry("a.png"), entry("b.png"), again],
            out("dest"),
            TransferMode::Move,
        )
        .unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(h.files.count("transfer:output:a.png"), 1);
    assert_eq!(h.files.count("transfer:output:b.png"), 1);
}

#[test]
fn transfer_fails_roots_and_folders_moved_into_themselves_per_item() {
    let h = Harness::new();
    let root = GalleryEntryRef {
        location: GalleryLocation::root(GalleryArea::Output),
        revision: EntryRevision("r".into()),
    };
    let results = h
        .transfer(
            vec![
                root,
                entry("shots"),
                entry("shots/day1"),
                entry("ok.png"),
                entry("other"),
            ],
            out("shots/day1"),
            TransferMode::Copy,
        )
        .unwrap();
    let outcomes: Vec<_> = results.iter().map(|r| (r.outcome, r.error)).collect();
    let refused = (ItemOutcome::Failed, code(GalleryErrorCode::InvalidRequest));
    assert_eq!(
        outcomes,
        [
            refused,
            refused,
            refused,
            (ItemOutcome::Succeeded, None),
            (ItemOutcome::Succeeded, None),
        ]
    );
    assert_eq!(results[1].source, out("shots"));
    assert_eq!(results[1].destination, None);
    assert_eq!(
        h.files.count("transfer:"),
        2,
        "refused items never reach the adapter"
    );
    // The other area is a different place, so a same-named folder may be copied there.
    let across = h
        .transfer(
            vec![entry("shots")],
            place(GalleryArea::Input, "shots/day1"),
            TransferMode::Copy,
        )
        .unwrap();
    assert_eq!(across[0].outcome, ItemOutcome::Succeeded);
}

#[test]
fn moving_into_the_current_folder_is_unchanged_but_copying_there_is_not() {
    let h = Harness::new();
    let results = h
        .transfer(
            vec![entry("shots/a.png"), entry("b.png")],
            out("shots"),
            TransferMode::Move,
        )
        .unwrap();
    assert_eq!(results[0].outcome, ItemOutcome::Unchanged);
    assert_eq!(results[0].destination, Some(out("shots/a.png")));
    assert_eq!(results[0].error, None);
    assert_eq!(results[1].outcome, ItemOutcome::Succeeded);
    assert_eq!(h.files.count("transfer:"), 1);

    let h = Harness::new();
    let copied = h
        .transfer(vec![entry("shots/a.png")], out("shots"), TransferMode::Copy)
        .unwrap();
    assert_eq!(copied[0].outcome, ItemOutcome::Succeeded);
    assert_eq!(h.files.count("transfer:"), 1);

    // Into the area root counts as the same folder for a top-level entry.
    let h = Harness::new();
    let root_move = h
        .transfer(
            vec![entry("a.png")],
            GalleryLocation::root(GalleryArea::Output),
            TransferMode::Move,
        )
        .unwrap();
    assert_eq!(root_move[0].outcome, ItemOutcome::Unchanged);
}

#[test]
fn adapter_failures_are_reported_per_item_and_do_not_stop_the_batch() {
    let h = Harness::new();
    h.files.state().transfer_fails.insert("output:b.png".into());
    let results = h
        .transfer(
            vec![entry("a.png"), entry("b.png"), entry("c.png")],
            out("dest"),
            TransferMode::Move,
        )
        .unwrap();
    assert_eq!(results[0].outcome, ItemOutcome::Succeeded);
    assert_eq!(results[1].outcome, ItemOutcome::Failed);
    assert_eq!(results[1].error, code(GalleryErrorCode::Locked));
    assert_eq!(results[2].outcome, ItemOutcome::Succeeded);
}

#[test]
fn plan_rejects_empty_oversized_and_root_requests() {
    let h = Harness::new();
    assert_eq!(
        h.plan(Vec::new()),
        Err(GalleryErrorCode::InvalidRequest.into())
    );
    let too_many: Vec<_> = (0..=MAX_ITEMS)
        .map(|n| entry(&format!("f{n}.png")))
        .collect();
    assert_eq!(
        h.plan(too_many),
        Err(GalleryErrorCode::InvalidRequest.into())
    );
    let root = GalleryEntryRef {
        location: GalleryLocation::root(GalleryArea::Input),
        revision: EntryRevision("r".into()),
    };
    assert_eq!(
        h.plan(vec![entry("a.png"), root]),
        Err(GalleryErrorCode::InvalidRequest.into())
    );
    assert!(h.files.calls().is_empty());
}

#[test]
fn plan_dedupes_drops_nested_items_and_sums_the_bytes() {
    let h = Harness::new();
    let plan = h
        .plan(vec![
            entry("shots/day1/a.png"),
            entry("shots"),
            entry("other.png"),
            entry("shots"),
            entry("shots/day2"),
            entry("shots/day3/b.png"),
            entry("shotsy/c.png"),
        ])
        .unwrap();
    let planned: Vec<_> = plan.items.iter().map(|i| text(&i.entry.location)).collect();
    assert_eq!(
        planned,
        ["output:shots", "output:other.png", "output:shotsy/c.png"]
    );
    assert_eq!(
        h.files.calls(),
        [
            "plan:output:shots",
            "plan:output:other.png",
            "plan:output:shotsy/c.png",
        ]
    );
    assert_eq!(plan.total_bytes, 30);
    assert_eq!(plan.id, "id1");
    assert_eq!(
        h.gate.try_acquires.load(Ordering::SeqCst),
        0,
        "planning takes no gate"
    );
}

#[test]
fn plan_treats_only_an_exact_case_ancestor_as_holding_a_selection() {
    // On a case-sensitive volume `A` and `a` are different folders, so neither holds the other.
    let h = Harness::new();
    let plan = h.plan(vec![entry("A"), entry("a/x.png")]).unwrap();
    let planned: Vec<_> = plan.items.iter().map(|i| text(&i.entry.location)).collect();
    assert_eq!(planned, ["output:A", "output:a/x.png"]);

    let h = Harness::new();
    let plan = h.plan(vec![entry("A"), entry("A/x.png")]).unwrap();
    let planned: Vec<_> = plan.items.iter().map(|i| text(&i.entry.location)).collect();
    assert_eq!(planned, ["output:A"]);
    assert_eq!(h.files.calls(), ["plan:output:A"]);
}

#[test]
fn the_first_planning_error_aborts_and_stores_nothing() {
    let h = Harness::new();
    h.files.state().plan_fails.insert("output:b.png".into());
    assert_eq!(
        h.plan(vec![entry("a.png"), entry("b.png"), entry("c.png")]),
        Err(GalleryErrorCode::Stale.into())
    );
    assert_eq!(h.files.count("plan:"), 2, "c.png is never planned");
    // No plan id was handed out, so nothing can be confirmed.
    assert_eq!(h.delete("id1"), Err(GalleryErrorCode::Stale.into()));
    assert_eq!(h.files.count("trash:"), 0);
}

#[test]
fn a_plan_runs_once_and_unknown_ids_are_stale() {
    let h = Harness::new();
    assert_eq!(h.delete("nope"), Err(GalleryErrorCode::Stale.into()));
    let plan = h.plan(vec![entry("a.png"), entry("b.png")]).unwrap();
    let results = h.delete(&plan.id).unwrap();
    assert_eq!(
        results.iter().map(|r| r.outcome).collect::<Vec<_>>(),
        [ItemOutcome::Succeeded, ItemOutcome::Succeeded]
    );
    assert_eq!(results[0].source, out("a.png"));
    assert_eq!(results[0].destination, None);
    assert_eq!(h.delete(&plan.id), Err(GalleryErrorCode::Stale.into()));
    assert_eq!(h.files.count("trash:"), 2);
}

#[test]
fn a_busy_gate_refuses_deletion_and_keeps_the_plan_for_a_retry() {
    let h = Harness::new();
    let plan = h.plan(vec![entry("a.png")]).unwrap();
    let lease = h.hold_gate();
    assert_eq!(h.delete(&plan.id), Err(GalleryErrorCode::Busy.into()));
    assert_eq!(h.files.count("trash:"), 0);
    drop(lease);
    assert_eq!(h.delete(&plan.id).unwrap().len(), 1);
}

#[test]
fn a_trash_failure_is_reported_for_that_item_with_no_other_adapter_calls() {
    let h = Harness::new();
    h.files.state().trash_fails.insert("output:b.png".into());
    let plan = h
        .plan(vec![entry("a.png"), entry("b.png"), entry("c.png")])
        .unwrap();
    let before = h.files.calls().len();
    let results = h.delete(&plan.id).unwrap();
    assert_eq!(
        results.iter().map(|r| r.outcome).collect::<Vec<_>>(),
        [
            ItemOutcome::Succeeded,
            ItemOutcome::Failed,
            ItemOutcome::Succeeded
        ]
    );
    assert_eq!(results[1].error, code(GalleryErrorCode::TrashFailed));
    assert_eq!(results[1].source, out("b.png"));
    assert_eq!(
        h.files.calls()[before..],
        [
            "trash:output:a.png",
            "trash:output:b.png",
            "trash:output:c.png"
        ],
        "only recycle calls, never a fallback"
    );
}

#[test]
fn only_the_newest_eight_plans_stay_confirmable() {
    let h = Harness::new();
    let ids: Vec<_> = (0..9)
        .map(|n| h.plan(vec![entry(&format!("f{n}.png"))]).unwrap().id)
        .collect();
    assert_eq!(h.delete(&ids[0]), Err(GalleryErrorCode::Stale.into()));
    assert!(h.delete(&ids[1]).is_ok());
    assert!(h.delete(&ids[8]).is_ok());
}

#[test]
fn media_and_explorer_lookups_delegate_without_the_gate() {
    let h = Harness::new();
    let _busy = h.hold_gate();
    let mut media = block_on(OpenGalleryMedia::new(h.deps.clone()).execute(&out("a.png"))).unwrap();
    assert_eq!(media.len(), 13);
    let mut buf = [0u8; 4];
    assert_eq!(media.read_at(8, &mut buf).unwrap(), 4);
    assert_eq!(&buf, b"byte");
    assert_eq!(media.read_at(100, &mut buf).unwrap(), 0);
    let target =
        block_on(LocateGalleryEntry::new(h.deps.clone()).execute(&out("shots/a.png"))).unwrap();
    assert_eq!(
        target,
        ExplorerTarget::RevealItem(Path::new(ROOT).join("shots/a.png"))
    );
    assert_eq!(
        h.files.calls(),
        ["open:output:a.png", "locate:output:shots/a.png"]
    );
}
