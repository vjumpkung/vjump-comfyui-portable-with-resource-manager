//! Gallery adapter tests over real temporary folders. Faults are injected on the calling thread
//! and travel with the request to its blocking worker.
use super::files::{Faults, LocalGalleryFiles, inject};
use application::{
    gallery::*,
    shared::{ExplorerTarget, Retained},
};
use domain::{
    gallery::*,
    shared::model_values::{RelativeModelPath, SafeFileName},
};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

fn loc(area: GalleryArea, path: &str) -> GalleryLocation {
    GalleryLocation::new(area, RelativeModelPath::new(path).unwrap()).unwrap()
}
fn out(path: &str) -> GalleryLocation {
    loc(GalleryArea::Output, path)
}
fn inp(path: &str) -> GalleryLocation {
    loc(GalleryArea::Input, path)
}
fn name(text: &str) -> SafeFileName {
    SafeFileName::new(text).unwrap()
}
fn code(result: Result<impl Sized>) -> GalleryErrorCode {
    result.err().expect("the operation must fail").0
}
/// What walking through a link yields: Windows refuses the reparse point; on Linux the
/// no-follow directory open reports "not a directory".
fn refused(code: GalleryErrorCode) -> bool {
    matches!(
        code,
        GalleryErrorCode::AccessDenied | GalleryErrorCode::InvalidRequest
    )
}

/// A directory link: a junction on Windows (no privilege needed), a symlink elsewhere. `false`
/// when the platform refuses to create one.
#[cfg(windows)]
fn directory_link(target: &Path, link: &Path) -> bool {
    let made = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .output();
    let created = made.is_ok_and(|output| output.status.success());
    if !created {
        eprintln!("skipped: junctions cannot be created here");
    }
    created
}
#[cfg(unix)]
fn directory_link(target: &Path, link: &Path) -> bool {
    std::os::unix::fs::symlink(target, link).expect("temporary directory must permit symlinks");
    true
}

struct Fixture {
    _directory: tempfile::TempDir,
    root: PathBuf,
    files: LocalGalleryFiles,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("install root ไทย");
        fs::create_dir_all(root.join("ComfyUI/input")).unwrap();
        fs::create_dir_all(root.join("ComfyUI/output")).unwrap();
        Self {
            _directory: directory,
            root,
            files: LocalGalleryFiles::new(),
        }
    }
    fn area(&self, area: GalleryArea) -> PathBuf {
        self.root.join("ComfyUI").join(area.dir_name())
    }
    fn path(&self, location: &GalleryLocation) -> PathBuf {
        let mut path = self.area(location.area());
        for part in location.path().components() {
            path.push(part.as_str());
        }
        path
    }
    fn write(&self, location: &GalleryLocation, bytes: &[u8]) {
        let path = self.path(location);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
    fn mkdir(&self, location: &GalleryLocation) {
        fs::create_dir_all(self.path(location)).unwrap();
    }
    fn read(&self, location: &GalleryLocation) -> Vec<u8> {
        fs::read(self.path(location)).unwrap()
    }
    fn exists(&self, location: &GalleryLocation) -> bool {
        fs::symlink_metadata(self.path(location)).is_ok()
    }
    /// Names inside the area's staging folder; empty when there is none.
    fn staged(&self, area: GalleryArea) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(self.area(area).join(STAGING_DIR))
            .map(|entries| {
                entries
                    .flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }
    async fn list(&self, directory: &GalleryLocation) -> GalleryListing {
        self.files.list(&self.root, directory).await.unwrap()
    }
    /// The entry as a fresh listing shows it.
    async fn entry(&self, location: &GalleryLocation) -> GalleryEntryRef {
        let parent = location.parent().unwrap();
        let listing = self.list(&parent).await;
        let found = listing
            .entries
            .iter()
            .find(|entry| &entry.location == location)
            .unwrap_or_else(|| panic!("{location:?} is not listed"));
        GalleryEntryRef {
            location: found.location.clone(),
            revision: found.revision.clone(),
        }
    }
    async fn rename(&self, from: &GalleryLocation, to: &str) -> Result<GalleryLocation> {
        let entry = self.entry(from).await;
        self.files.rename(&self.root, &entry, &name(to)).await
    }
    async fn transfer(
        &self,
        from: &GalleryLocation,
        to: &GalleryLocation,
        mode: TransferMode,
    ) -> ItemResult {
        let entry = self.entry(from).await;
        self.files
            .transfer(&self.root, &entry, to, mode, "op1", Retained::default())
            .await
    }
    async fn move_to(&self, from: &GalleryLocation, to: &GalleryLocation) -> ItemResult {
        self.transfer(from, to, TransferMode::Move).await
    }
    async fn copy_to(&self, from: &GalleryLocation, to: &GalleryLocation) -> ItemResult {
        self.transfer(from, to, TransferMode::Copy).await
    }
    async fn plan(&self, location: &GalleryLocation) -> Result<DeletePlanItem> {
        let entry = self.entry(location).await;
        self.files.plan_delete(&self.root, &entry).await
    }
}

fn succeeded(result: &ItemResult, destination: &GalleryLocation) {
    assert_eq!(result.outcome, ItemOutcome::Succeeded, "{result:?}");
    assert_eq!(result.error, None);
    assert_eq!(result.destination.as_ref(), Some(destination));
}
fn failed(result: &ItemResult, expected: GalleryErrorCode) {
    assert_eq!(result.outcome, ItemOutcome::Failed, "{result:?}");
    assert_eq!(result.error, Some(GalleryError(expected)));
    assert_eq!(result.destination, None);
}

/// A folder with nested files, an empty folder and a Thai name.
fn make_tree(fx: &Fixture, base: &str) {
    fx.write(&out(&format!("{base}/a.png")), b"first image");
    fx.write(&out(&format!("{base}/sub/b.txt")), b"nested file");
    fx.write(&out(&format!("{base}/sub/ภาพ.webp")), b"thai name");
    fx.mkdir(&out(&format!("{base}/sub/empty")));
}
fn assert_tree(fx: &Fixture, base: &str) {
    assert_eq!(fx.read(&out(&format!("{base}/a.png"))), b"first image");
    assert_eq!(fx.read(&out(&format!("{base}/sub/b.txt"))), b"nested file");
    assert_eq!(fx.read(&out(&format!("{base}/sub/ภาพ.webp"))), b"thai name");
    assert!(fx.path(&out(&format!("{base}/sub/empty"))).is_dir());
}

// ---- resolution and listing ----

#[tokio::test]
async fn a_missing_comfyui_folder_is_not_installed_and_is_never_created() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("install");
    fs::create_dir(&root).unwrap();
    let files = LocalGalleryFiles::new();
    let area = GalleryLocation::root(GalleryArea::Output);
    assert_eq!(
        code(files.list(&root, &area).await),
        GalleryErrorCode::NotInstalled
    );
    assert_eq!(
        code(files.explorer_target(&root, &area).await),
        GalleryErrorCode::NotInstalled
    );
    assert_eq!(
        code(files.open_media(&root, &out("a.png")).await),
        GalleryErrorCode::NotInstalled
    );
    assert!(files.cleanup_staging(&root).await.is_empty());
    assert!(
        !root.join("ComfyUI").exists(),
        "ComfyUI must never be created"
    );
    // The install root itself may be missing too.
    let absent = directory.path().join("absent");
    assert_eq!(
        code(files.list(&absent, &area).await),
        GalleryErrorCode::NotInstalled
    );
    assert!(!absent.exists());
}

#[tokio::test]
async fn an_area_folder_is_created_on_demand_below_an_existing_comfyui() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("install");
    fs::create_dir_all(root.join("ComfyUI")).unwrap();
    let files = LocalGalleryFiles::new();
    let listing = files
        .list(&root, &GalleryLocation::root(GalleryArea::Output))
        .await
        .unwrap();
    assert!(listing.entries.is_empty() && listing.warnings.is_empty());
    assert!(root.join("ComfyUI/output").is_dir());
    assert!(!root.join("ComfyUI/input").exists());
}

#[tokio::test]
async fn listing_sorts_folders_first_shows_dot_files_and_hides_staging() {
    let fx = Fixture::new();
    fx.mkdir(&out("b folder"));
    fx.mkdir(&out("A folder"));
    // The staging folder cannot be addressed, so it is built on disk directly.
    let staging = fx.area(GalleryArea::Output).join(STAGING_DIR).join("op");
    fs::create_dir_all(&staging).unwrap();
    fs::write(staging.join("x"), b"x").unwrap();
    fx.write(&out("z.png"), b"123456");
    fx.write(&out("a.MP4"), b"12");
    fx.write(&out(".hidden.txt"), b"1");
    fx.write(&out("B.glb"), b"1234");
    fx.write(&out("notes"), b"");
    let listing = fx.list(&GalleryLocation::root(GalleryArea::Output)).await;
    assert!(listing.warnings.is_empty(), "{:?}", listing.warnings);
    assert_eq!(
        listing.directory,
        GalleryLocation::root(GalleryArea::Output)
    );
    let names: Vec<_> = listing.entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "A folder",
            "b folder",
            ".hidden.txt",
            "a.MP4",
            "B.glb",
            "notes",
            "z.png"
        ]
    );
    let folder = &listing.entries[0];
    assert_eq!(folder.kind, GalleryEntryKind::Directory);
    assert_eq!(folder.media, GalleryMediaKind::Other);
    assert_eq!(folder.size_bytes, None);
    assert_eq!(folder.location, out("A folder"));
    let video = &listing.entries[3];
    assert_eq!(video.kind, GalleryEntryKind::File);
    assert_eq!(video.media, GalleryMediaKind::Video);
    assert_eq!(video.size_bytes, Some(2));
    assert!(video.modified_ms.is_some_and(|ms| ms > 1_500_000_000_000));
    assert!(!video.revision.0.is_empty());
    assert_eq!(listing.entries[4].media, GalleryMediaKind::Model3d);
    assert_eq!(listing.entries[5].media, GalleryMediaKind::Other);
    assert_eq!(listing.entries[6].media, GalleryMediaKind::Image);
    // The staging folder cannot be browsed into either.
    assert!(
        GalleryLocation::new(
            GalleryArea::Output,
            RelativeModelPath::new(STAGING_DIR).unwrap()
        )
        .is_err()
    );
}

#[tokio::test]
async fn listing_a_file_or_a_missing_folder_fails() {
    let fx = Fixture::new();
    fx.write(&out("a.png"), b"a");
    assert_eq!(
        code(fx.files.list(&fx.root, &out("a.png")).await),
        GalleryErrorCode::InvalidRequest
    );
    assert_eq!(
        code(fx.files.list(&fx.root, &out("missing")).await),
        GalleryErrorCode::NotFound
    );
    assert_eq!(
        code(fx.files.list(&fx.root, &out("a.png/deeper")).await),
        GalleryErrorCode::InvalidRequest
    );
}

#[tokio::test]
async fn revisions_change_when_an_entry_changes() {
    let fx = Fixture::new();
    fx.write(&out("a.png"), b"one");
    let before = fx.entry(&out("a.png")).await;
    assert_eq!(
        fx.entry(&out("a.png")).await,
        before,
        "stable while unchanged"
    );
    fx.write(&out("a.png"), b"longer content");
    assert_ne!(fx.entry(&out("a.png")).await.revision, before.revision);
}

#[tokio::test]
async fn links_are_reported_never_followed_and_refused_in_paths() {
    let fx = Fixture::new();
    let real = fx.root.join("real");
    fs::create_dir_all(&real).unwrap();
    fs::write(real.join("secret.png"), b"secret").unwrap();
    fx.write(&out("plain.png"), b"p");
    if !directory_link(&real, &fx.path(&out("link"))) {
        return;
    }
    let listing = fx.list(&GalleryLocation::root(GalleryArea::Output)).await;
    assert_eq!(listing.entries.len(), 1);
    assert_eq!(listing.entries[0].name.as_str(), "plain.png");
    assert_eq!(
        listing.warnings,
        [GalleryWarning {
            name: Some("link".into()),
            code: GalleryWarningCode::Link
        }]
    );
    // Neither the link nor anything below it can be addressed.
    for through in [out("link"), out("link/secret.png")] {
        let result = fx.files.explorer_target(&fx.root, &through).await;
        assert!(refused(code(result)), "{through:?}");
    }
    assert!(refused(code(fx.files.list(&fx.root, &out("link")).await)));
    assert!(refused(code(
        fx.files.open_media(&fx.root, &out("link/secret.png")).await
    )));
    let moved = fx
        .files
        .transfer(
            &fx.root,
            &GalleryEntryRef {
                location: out("plain.png"),
                revision: fx.entry(&out("plain.png")).await.revision,
            },
            &out("link"),
            TransferMode::Copy,
            "op1",
            Retained::default(),
        )
        .await;
    assert_eq!(moved.outcome, ItemOutcome::Failed);
    assert!(refused(moved.error.unwrap().0));
    assert_eq!(
        fs::read_dir(&real).unwrap().count(),
        1,
        "nothing written through the link"
    );
}

#[tokio::test]
async fn a_linked_area_folder_is_refused() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("install");
    let elsewhere = directory.path().join("elsewhere");
    fs::create_dir_all(root.join("ComfyUI")).unwrap();
    fs::create_dir_all(&elsewhere).unwrap();
    fs::write(elsewhere.join("a.png"), b"a").unwrap();
    if !directory_link(&elsewhere, &root.join("ComfyUI").join("output")) {
        return;
    }
    let files = LocalGalleryFiles::new();
    let listed = files
        .list(&root, &GalleryLocation::root(GalleryArea::Output))
        .await;
    assert!(refused(code(listed)));
    assert_eq!(fs::read_dir(&elsewhere).unwrap().count(), 1);
}

#[cfg(unix)]
#[tokio::test]
async fn names_that_are_not_safe_are_warned_about_and_not_listed() {
    let fx = Fixture::new();
    fx.write(&out("fine.png"), b"f");
    fs::write(fx.area(GalleryArea::Output).join("bad:name.png"), b"b").unwrap();
    let listing = fx.list(&GalleryLocation::root(GalleryArea::Output)).await;
    assert_eq!(listing.entries.len(), 1);
    assert_eq!(
        listing.warnings,
        [GalleryWarning {
            name: Some("bad:name.png".into()),
            code: GalleryWarningCode::UnsafeName
        }]
    );
}

// ---- rename ----

#[tokio::test]
async fn rename_keeps_the_folder_and_never_replaces() {
    let fx = Fixture::new();
    fx.write(&out("shots/a.png"), b"a");
    fx.write(&out("shots/b.png"), b"b");
    fx.mkdir(&out("shots/folder"));
    assert_eq!(
        code(fx.rename(&out("shots/a.png"), "b.png").await),
        GalleryErrorCode::TargetExists
    );
    assert_eq!(fx.read(&out("shots/a.png")), b"a");
    assert_eq!(fx.read(&out("shots/b.png")), b"b");
    assert_eq!(
        fx.rename(&out("shots/a.png"), "c.png").await.unwrap(),
        out("shots/c.png")
    );
    assert!(!fx.exists(&out("shots/a.png")));
    assert_eq!(fx.read(&out("shots/c.png")), b"a");
    assert_eq!(
        fx.rename(&out("shots/folder"), "renamed folder")
            .await
            .unwrap(),
        out("shots/renamed folder")
    );
    assert!(fx.path(&out("shots/renamed folder")).is_dir());
}

#[tokio::test]
async fn rename_of_a_changed_or_vanished_entry_is_stale_or_missing() {
    let fx = Fixture::new();
    fx.write(&out("a.png"), b"one");
    let seen = fx.entry(&out("a.png")).await;
    fx.write(&out("a.png"), b"changed meanwhile");
    assert_eq!(
        code(fx.files.rename(&fx.root, &seen, &name("b.png")).await),
        GalleryErrorCode::Stale
    );
    assert_eq!(fx.read(&out("a.png")), b"changed meanwhile");
    fs::remove_file(fx.path(&out("a.png"))).unwrap();
    assert_eq!(
        code(fx.files.rename(&fx.root, &seen, &name("b.png")).await),
        GalleryErrorCode::NotFound
    );
    assert_eq!(
        code(
            fx.files
                .rename(
                    &fx.root,
                    &GalleryEntryRef {
                        location: GalleryLocation::root(GalleryArea::Output),
                        revision: seen.revision.clone()
                    },
                    &name("b.png")
                )
                .await
        ),
        GalleryErrorCode::InvalidRequest
    );
    fx.write(&out("c.png"), b"c");
    assert_eq!(
        code(
            fx.files
                .rename(&fx.root, &fx.entry(&out("c.png")).await, &name(STAGING_DIR))
                .await
        ),
        GalleryErrorCode::InvalidName
    );
}

#[tokio::test]
async fn a_case_only_rename_changes_the_spelling_of_a_file_and_a_folder() {
    let fx = Fixture::new();
    fx.write(&out("Photo.png"), b"p");
    fx.mkdir(&out("Folder"));
    fx.write(&out("Folder/inside.txt"), b"i");
    assert_eq!(
        fx.rename(&out("Photo.png"), "photo.PNG").await.unwrap(),
        out("photo.PNG")
    );
    assert_eq!(
        fx.rename(&out("Folder"), "FOLDER").await.unwrap(),
        out("FOLDER")
    );
    let listing = fx.list(&GalleryLocation::root(GalleryArea::Output)).await;
    let names: Vec<_> = listing.entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["FOLDER", "photo.PNG"], "exact spelling on disk");
    assert_eq!(fx.read(&out("photo.PNG")), b"p");
    assert_eq!(fx.read(&out("FOLDER/inside.txt")), b"i");
}

// A case-sensitive volume holds `foo.png` and `Foo.png` as two entries; Linux temporary folders
// are case-sensitive by default.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_case_only_rename_onto_a_different_entry_is_refused_and_changes_nothing() {
    let fx = Fixture::new();
    fx.write(&out("foo.png"), b"lower case");
    fx.write(&out("Foo.png"), b"upper case");
    assert_eq!(
        code(fx.rename(&out("foo.png"), "Foo.png").await),
        GalleryErrorCode::TargetExists
    );
    assert_eq!(fx.read(&out("foo.png")), b"lower case");
    assert_eq!(fx.read(&out("Foo.png")), b"upper case");
}

// ---- move on one volume ----

#[tokio::test]
async fn a_move_renames_in_place_and_keeps_content() {
    let fx = Fixture::new();
    fx.write(&out("a.png"), b"a");
    fx.mkdir(&out("archive"));
    let moved = fx.move_to(&out("a.png"), &out("archive")).await;
    succeeded(&moved, &out("archive/a.png"));
    assert_eq!(moved.source, out("a.png"));
    assert!(!fx.exists(&out("a.png")));
    assert_eq!(fx.read(&out("archive/a.png")), b"a");
    assert!(
        fx.staged(GalleryArea::Output).is_empty(),
        "no copy was needed"
    );

    make_tree(&fx, "shots");
    let moved = fx.move_to(&out("shots"), &out("archive")).await;
    succeeded(&moved, &out("archive/shots"));
    assert!(!fx.exists(&out("shots")));
    assert_tree(&fx, "archive/shots");

    // Back to the area root, and across areas.
    let back = fx
        .move_to(
            &out("archive/a.png"),
            &GalleryLocation::root(GalleryArea::Output),
        )
        .await;
    succeeded(&back, &out("a.png"));
    fx.mkdir(&inp("keep"));
    let across = fx.move_to(&out("a.png"), &inp("keep")).await;
    succeeded(&across, &inp("keep/a.png"));
    assert_eq!(fx.read(&inp("keep/a.png")), b"a");
}

#[tokio::test]
async fn a_name_conflict_gets_a_numbered_suffix_and_nothing_is_merged() {
    let fx = Fixture::new();
    fx.write(&out("dest/a.png"), b"existing 0");
    fx.write(&out("src/a.png"), b"moved 1");
    fx.write(&out("src2/a.png"), b"moved 2");
    fx.mkdir(&out("dest/folder"));
    fx.write(&out("dest/folder/keep.txt"), b"keep");
    fx.write(&out("other/folder/new.txt"), b"new");
    let first = fx.move_to(&out("src/a.png"), &out("dest")).await;
    succeeded(&first, &out("dest/a (1).png"));
    let second = fx.move_to(&out("src2/a.png"), &out("dest")).await;
    succeeded(&second, &out("dest/a (2).png"));
    assert_eq!(fx.read(&out("dest/a.png")), b"existing 0");
    assert_eq!(fx.read(&out("dest/a (1).png")), b"moved 1");
    assert_eq!(fx.read(&out("dest/a (2).png")), b"moved 2");
    // A folder is never merged into one of the same name.
    let folder = fx.move_to(&out("other/folder"), &out("dest")).await;
    succeeded(&folder, &out("dest/folder (1)"));
    assert_eq!(fx.read(&out("dest/folder/keep.txt")), b"keep");
    assert_eq!(fx.read(&out("dest/folder (1)/new.txt")), b"new");
}

#[tokio::test]
async fn transfers_validate_the_source_and_destination() {
    let fx = Fixture::new();
    fx.write(&out("a.png"), b"a");
    fx.write(&out("file.txt"), b"f");
    fx.make_nested();
    let seen = fx.entry(&out("a.png")).await;
    failed(
        &fx.move_to(&out("a.png"), &out("missing")).await,
        GalleryErrorCode::NotFound,
    );
    failed(
        &fx.move_to(&out("a.png"), &out("file.txt")).await,
        GalleryErrorCode::InvalidRequest,
    );
    failed(
        &fx.copy_to(&out("a.png"), &out("file.txt/deeper")).await,
        GalleryErrorCode::InvalidRequest,
    );
    // A folder cannot go into itself or below itself.
    failed(
        &fx.move_to(&out("outer"), &out("outer/inner")).await,
        GalleryErrorCode::InvalidRequest,
    );
    failed(
        &fx.copy_to(&out("outer"), &out("outer")).await,
        GalleryErrorCode::InvalidRequest,
    );
    assert!(fx.path(&out("outer/inner")).is_dir());
    // The root is not an entry.
    let root_entry = GalleryEntryRef {
        location: GalleryLocation::root(GalleryArea::Output),
        revision: seen.revision.clone(),
    };
    failed(
        &fx.files
            .transfer(
                &fx.root,
                &root_entry,
                &inp(""),
                TransferMode::Copy,
                "op1",
                Retained::default(),
            )
            .await,
        GalleryErrorCode::InvalidRequest,
    );
    // A changed source is stale, and a vanished one is missing.
    fx.mkdir(&out("dest"));
    fx.write(&out("a.png"), b"changed after listing");
    let stale = fx
        .files
        .transfer(
            &fx.root,
            &seen,
            &out("dest"),
            TransferMode::Move,
            "op1",
            Retained::default(),
        )
        .await;
    failed(&stale, GalleryErrorCode::Stale);
    let stale_copy = fx
        .files
        .transfer(
            &fx.root,
            &seen,
            &out("dest"),
            TransferMode::Copy,
            "op2",
            Retained::default(),
        )
        .await;
    failed(&stale_copy, GalleryErrorCode::Stale);
    assert_eq!(fx.read(&out("a.png")), b"changed after listing");
    fs::remove_file(fx.path(&out("a.png"))).unwrap();
    failed(
        &fx.files
            .transfer(
                &fx.root,
                &seen,
                &out("dest"),
                TransferMode::Move,
                "op3",
                Retained::default(),
            )
            .await,
        GalleryErrorCode::NotFound,
    );
    assert!(fx.staged(GalleryArea::Output).is_empty());
}

impl Fixture {
    fn make_nested(&self) {
        self.mkdir(&out("outer/inner"));
    }
}

// ---- copy ----

#[tokio::test]
async fn a_copy_leaves_the_source_and_verifies_the_content() {
    let fx = Fixture::new();
    fx.write(&out("a.png"), &vec![7u8; 3 * 1024 * 1024 + 5]);
    fx.write(&out("empty.txt"), b"");
    fx.mkdir(&out("dest"));
    let copied = fx.copy_to(&out("a.png"), &out("dest")).await;
    succeeded(&copied, &out("dest/a.png"));
    assert_eq!(fx.read(&out("a.png")), fx.read(&out("dest/a.png")));
    succeeded(
        &fx.copy_to(&out("empty.txt"), &out("dest")).await,
        &out("dest/empty.txt"),
    );
    assert_eq!(fx.read(&out("dest/empty.txt")), b"");
    // Copying next to the original never overwrites it.
    let beside = fx
        .copy_to(&out("a.png"), &GalleryLocation::root(GalleryArea::Output))
        .await;
    succeeded(&beside, &out("a (1).png"));
    let again = fx
        .copy_to(&out("a.png"), &GalleryLocation::root(GalleryArea::Output))
        .await;
    succeeded(&again, &out("a (2).png"));
    // Nothing of the staging copy remains, and the marker never reaches the destination.
    assert!(fx.staged(GalleryArea::Output).is_empty());
    assert!(!fx.exists(&out("dest/.owner")));
}

#[tokio::test]
async fn a_copied_folder_matches_its_source_and_works_across_areas() {
    let fx = Fixture::new();
    make_tree(&fx, "shots");
    fx.mkdir(&out("dest"));
    let copied = fx.copy_to(&out("shots"), &out("dest")).await;
    succeeded(&copied, &out("dest/shots"));
    assert_tree(&fx, "shots");
    assert_tree(&fx, "dest/shots");
    let conflict = fx.copy_to(&out("shots"), &out("dest")).await;
    succeeded(&conflict, &out("dest/shots (1)"));
    assert_tree(&fx, "dest/shots (1)");

    // input -> output, and output -> input
    fx.write(&inp("upload.png"), b"uploaded");
    let into_output = fx
        .copy_to(
            &inp("upload.png"),
            &GalleryLocation::root(GalleryArea::Output),
        )
        .await;
    succeeded(&into_output, &out("upload.png"));
    let into_input = fx
        .copy_to(&out("shots"), &GalleryLocation::root(GalleryArea::Input))
        .await;
    succeeded(&into_input, &inp("shots"));
    assert_eq!(fx.read(&inp("shots/a.png")), b"first image");
    assert_eq!(fx.read(&inp("upload.png")), b"uploaded");
    assert!(fx.staged(GalleryArea::Input).is_empty());
    assert!(fx.staged(GalleryArea::Output).is_empty());
}

#[tokio::test]
async fn a_folder_holding_a_link_is_not_copied_at_all() {
    let fx = Fixture::new();
    let real = fx.root.join("real");
    fs::create_dir_all(&real).unwrap();
    fs::write(real.join("secret.png"), b"secret").unwrap();
    fx.write(&out("shots/a.png"), b"a");
    fx.write(&out("shots/z.png"), b"z");
    fx.mkdir(&out("dest"));
    if !directory_link(&real, &fx.path(&out("shots/link"))) {
        return;
    }
    for mode in [TransferMode::Copy, TransferMode::Move] {
        inject(Faults {
            force_cross_device: true,
            ..Faults::default()
        });
        let entry = fx.entry(&out("shots")).await;
        let result = fx
            .files
            .transfer(
                &fx.root,
                &entry,
                &out("dest"),
                mode,
                "op1",
                Retained::default(),
            )
            .await;
        failed(&result, GalleryErrorCode::Unsupported);
    }
    assert!(
        !fx.exists(&out("dest/shots")),
        "no incomplete copy is published"
    );
    assert!(fx.staged(GalleryArea::Output).is_empty());
    assert_eq!(fx.read(&out("shots/a.png")), b"a");
    assert!(real.join("secret.png").exists());
}

/// `levels` nested folders below `base` (which counts as the first), the last one holding a file.
fn nested(fx: &Fixture, base: &str, levels: usize) -> String {
    let mut path = base.to_owned();
    for _ in 1..levels {
        path.push_str("/d");
    }
    fx.write(&out(&format!("{path}/leaf.txt")), b"leaf");
    path
}

#[tokio::test]
async fn a_tree_too_deep_to_remove_from_staging_is_refused_and_leaves_nothing_behind() {
    let fx = Fixture::new();
    fx.mkdir(&out("dest"));
    // Staging adds a level, so 63 nested folders are the most a copy may hold.
    let fits = nested(&fx, "fits", 63);
    succeeded(
        &fx.copy_to(&out("fits"), &out("dest")).await,
        &out("dest/fits"),
    );
    assert_eq!(fx.read(&out(&format!("dest/{fits}/leaf.txt"))), b"leaf");
    assert!(fx.staged(GalleryArea::Output).is_empty());

    nested(&fx, "over", 64);
    failed(
        &fx.copy_to(&out("over"), &out("dest")).await,
        GalleryErrorCode::Unsupported,
    );
    assert!(!fx.exists(&out("dest/over")));
    assert!(
        fx.staged(GalleryArea::Output).is_empty(),
        "the partial copy was removable"
    );
    assert!(fx.files.cleanup_staging(&fx.root).await.is_empty());
    assert!(!fx.area(GalleryArea::Output).join(STAGING_DIR).exists());
}

#[cfg(unix)]
#[tokio::test]
async fn a_copy_keeps_the_permission_bits_of_its_source() {
    use std::os::unix::fs::PermissionsExt;
    let fx = Fixture::new();
    fx.write(&out("shots/a.png"), b"a");
    fx.write(&out("shots/sub/b.txt"), b"b");
    fx.write(&out("shots/ro/c.txt"), b"c");
    let modes = [
        ("shots/a.png", 0o644),
        ("shots/sub/b.txt", 0o640),
        ("shots/ro/c.txt", 0o444),
        ("shots/sub", 0o750),
        ("shots", 0o755),
    ];
    for (path, mode) in modes {
        fs::set_permissions(fx.path(&out(path)), fs::Permissions::from_mode(mode)).unwrap();
    }
    // A folder the owner cannot write still has to be filled, published and removed again.
    let read_only = fx.path(&out("shots/ro"));
    fs::set_permissions(&read_only, fs::Permissions::from_mode(0o555)).unwrap();
    fx.mkdir(&out("dest"));
    let mode_of = |path: &str| {
        fs::metadata(fx.path(&out(path)))
            .unwrap()
            .permissions()
            .mode()
            & 0o777
    };

    let copied = fx.copy_to(&out("shots"), &out("dest")).await;
    fs::set_permissions(&read_only, fs::Permissions::from_mode(0o755)).unwrap();
    succeeded(&copied, &out("dest/shots"));
    for (path, mode) in modes {
        assert_eq!(mode_of(&format!("dest/{path}")), mode, "{path}");
    }
    assert_eq!(mode_of("dest/shots/ro"), 0o755, "the owner keeps access");
    let file = fx.copy_to(&out("shots/a.png"), &out("dest")).await;
    succeeded(&file, &out("dest/a.png"));
    assert_eq!(mode_of("dest/a.png"), 0o644);
    assert!(fx.staged(GalleryArea::Output).is_empty());
}

#[tokio::test]
async fn staging_needs_a_usable_operation_id_and_an_unused_one() {
    let fx = Fixture::new();
    fx.write(&out("a.png"), b"a");
    fx.mkdir(&out("dest"));
    let entry = fx.entry(&out("a.png")).await;
    let bad = fx
        .files
        .transfer(
            &fx.root,
            &entry,
            &out("dest"),
            TransferMode::Copy,
            "a/b",
            Retained::default(),
        )
        .await;
    failed(&bad, GalleryErrorCode::InvalidRequest);
    // An operation directory that already exists is never adopted.
    fs::create_dir_all(fx.area(GalleryArea::Output).join(STAGING_DIR).join("taken")).unwrap();
    let taken = fx
        .files
        .transfer(
            &fx.root,
            &entry,
            &out("dest"),
            TransferMode::Copy,
            "taken",
            Retained::default(),
        )
        .await;
    failed(&taken, GalleryErrorCode::Io);
    assert_eq!(fx.staged(GalleryArea::Output), ["taken"]);
    assert!(!fx.exists(&out("dest/a.png")));
    // A file whose name collides with the ownership marker is not staged.
    fx.write(&out(".owner"), b"o");
    let owner = fx.entry(&out(".owner")).await;
    let refused = fx
        .files
        .transfer(
            &fx.root,
            &owner,
            &out("dest"),
            TransferMode::Copy,
            "op9",
            Retained::default(),
        )
        .await;
    failed(&refused, GalleryErrorCode::Unsupported);
}

// ---- cross-device moves and faults ----

fn cross_device() -> Faults {
    Faults {
        force_cross_device: true,
        ..Faults::default()
    }
}

#[tokio::test]
async fn a_cross_device_move_copies_then_removes_a_file_source() {
    let fx = Fixture::new();
    fx.write(&out("a.png"), b"payload");
    fx.write(&out("dest/a.png"), b"already there");
    inject(cross_device());
    let moved = fx.move_to(&out("a.png"), &out("dest")).await;
    succeeded(&moved, &out("dest/a (1).png"));
    assert!(!fx.exists(&out("a.png")));
    assert_eq!(fx.read(&out("dest/a (1).png")), b"payload");
    assert_eq!(fx.read(&out("dest/a.png")), b"already there");
    assert!(fx.staged(GalleryArea::Output).is_empty());
}

#[tokio::test]
async fn a_cross_device_move_removes_the_whole_folder_source() {
    let fx = Fixture::new();
    make_tree(&fx, "shots");
    fx.mkdir(&inp("archive"));
    inject(cross_device());
    let moved = fx.move_to(&out("shots"), &inp("archive")).await;
    succeeded(&moved, &inp("archive/shots"));
    assert!(!fx.exists(&out("shots")), "source folder is gone");
    assert_eq!(fx.read(&inp("archive/shots/a.png")), b"first image");
    assert_eq!(fx.read(&inp("archive/shots/sub/b.txt")), b"nested file");
    assert_eq!(fx.read(&inp("archive/shots/sub/ภาพ.webp")), b"thai name");
    assert!(fx.path(&inp("archive/shots/sub/empty")).is_dir());
    assert!(fx.staged(GalleryArea::Input).is_empty());
    assert!(fx.staged(GalleryArea::Output).is_empty());
}

#[tokio::test]
async fn a_failure_before_publication_leaves_the_source_alone_and_no_staging() {
    let fx = Fixture::new();
    make_tree(&fx, "shots");
    fx.write(&out("a.png"), b"a");
    fx.mkdir(&out("dest"));
    for source in [out("shots"), out("a.png")] {
        for mode in [TransferMode::Copy, TransferMode::Move] {
            inject(Faults {
                force_cross_device: true,
                fail_before_publish: true,
                ..Faults::default()
            });
            let entry = fx.entry(&source).await;
            let result = fx
                .files
                .transfer(
                    &fx.root,
                    &entry,
                    &out("dest"),
                    mode,
                    "op1",
                    Retained::default(),
                )
                .await;
            failed(&result, GalleryErrorCode::Io);
            assert!(fx.exists(&source));
            assert!(!fx.exists(&out("dest").join(source.name().unwrap().clone()).unwrap()));
            assert!(
                fx.staged(GalleryArea::Output).is_empty(),
                "failure cleans up"
            );
        }
    }
    assert_tree(&fx, "shots");
    // A crash would leave the staging copy behind; the next cleanup settles it.
    let leftover = fx
        .area(GalleryArea::Output)
        .join(STAGING_DIR)
        .join("crashed");
    fs::create_dir_all(&leftover).unwrap();
    fs::write(leftover.join(".owner"), "vjump-gallery-staging:crashed").unwrap();
    fs::write(leftover.join("a.png"), b"half a copy").unwrap();
    assert!(fx.files.cleanup_staging(&fx.root).await.is_empty());
    assert!(!fx.area(GalleryArea::Output).join(STAGING_DIR).exists());
}

#[tokio::test]
async fn a_failure_after_publication_keeps_both_copies() {
    let fx = Fixture::new();
    make_tree(&fx, "shots");
    fx.write(&out("a.png"), b"a");
    fx.mkdir(&out("dest"));
    for source in [out("a.png"), out("shots")] {
        inject(Faults {
            force_cross_device: true,
            fail_after_publish: true,
            ..Faults::default()
        });
        let moved = fx.move_to(&source, &out("dest")).await;
        let target = out("dest").join(source.name().unwrap().clone()).unwrap();
        assert_eq!(moved.outcome, ItemOutcome::SourceRetained, "{moved:?}");
        assert_eq!(moved.destination.as_ref(), Some(&target));
        assert_eq!(
            moved.error,
            Some(GalleryError(GalleryErrorCode::SourceRetained))
        );
        assert!(fx.exists(&source), "the source is untouched");
    }
    assert_tree(&fx, "shots");
    assert_tree(&fx, "dest/shots");
    assert_eq!(fx.read(&out("dest/a.png")), b"a");
    assert!(fx.staged(GalleryArea::Output).is_empty());
}

#[tokio::test]
async fn a_failure_during_source_cleanup_retains_the_remainder() {
    let fx = Fixture::new();
    fx.write(&out("src/one.png"), b"one");
    fx.write(&out("src/sub/two.png"), b"two");
    fx.mkdir(&out("dest"));
    inject(Faults {
        force_cross_device: true,
        fail_during_cleanup: true,
        ..Faults::default()
    });
    let moved = fx.move_to(&out("src"), &out("dest")).await;
    assert_eq!(moved.outcome, ItemOutcome::SourceRetained, "{moved:?}");
    assert_eq!(moved.destination, Some(out("dest/src")));
    assert_eq!(
        moved.error,
        Some(GalleryError(GalleryErrorCode::SourceRetained))
    );
    assert_eq!(fx.read(&out("dest/src/one.png")), b"one");
    assert_eq!(fx.read(&out("dest/src/sub/two.png")), b"two");
    // Exactly the file whose removal failed remains, with the folders that hold it.
    let remaining = [out("src/one.png"), out("src/sub/two.png")]
        .into_iter()
        .filter(|file| fx.exists(file))
        .count();
    assert_eq!(remaining, 1);
    assert!(fx.path(&out("src")).is_dir());

    // The same for a single file.
    fx.write(&out("solo.png"), b"solo");
    inject(Faults {
        force_cross_device: true,
        fail_during_cleanup: true,
        ..Faults::default()
    });
    let solo = fx.move_to(&out("solo.png"), &out("dest")).await;
    assert_eq!(solo.outcome, ItemOutcome::SourceRetained);
    assert_eq!(fx.read(&out("dest/solo.png")), b"solo");
    assert_eq!(fx.read(&out("solo.png")), b"solo");
}

#[tokio::test]
async fn anything_changed_or_added_in_a_moved_folder_stays() {
    let fx = Fixture::new();
    fx.write(&out("src/keep.png"), b"original keep");
    fx.write(&out("src/gone.png"), b"original gone");
    fx.write(&out("src/sub/deep.png"), b"original deep");
    fx.mkdir(&out("dest"));
    let changed = fx.path(&out("src/keep.png"));
    let added = fx.path(&out("src/sub/added.txt"));
    inject(Faults {
        force_cross_device: true,
        after_publish: Some(Box::new(move || {
            fs::write(&changed, b"edited after the copy was published").unwrap();
            fs::write(&added, b"new").unwrap();
        })),
        ..Faults::default()
    });
    let moved = fx.move_to(&out("src"), &out("dest")).await;
    assert_eq!(moved.outcome, ItemOutcome::SourceRetained, "{moved:?}");
    // The destination is the complete original copy.
    assert_eq!(fx.read(&out("dest/src/keep.png")), b"original keep");
    assert_eq!(fx.read(&out("dest/src/gone.png")), b"original gone");
    assert_eq!(fx.read(&out("dest/src/sub/deep.png")), b"original deep");
    // Only what still matched was removed.
    assert_eq!(
        fx.read(&out("src/keep.png")),
        b"edited after the copy was published"
    );
    assert_eq!(fx.read(&out("src/sub/added.txt")), b"new");
    assert!(!fx.exists(&out("src/gone.png")));
    assert!(!fx.exists(&out("src/sub/deep.png")));

    // A single changed file is retained too.
    fx.write(&out("one.png"), b"one");
    let target = fx.path(&out("one.png"));
    inject(Faults {
        force_cross_device: true,
        after_publish: Some(Box::new(move || {
            fs::write(&target, b"a different size").unwrap();
        })),
        ..Faults::default()
    });
    let single = fx.move_to(&out("one.png"), &out("dest")).await;
    assert_eq!(single.outcome, ItemOutcome::SourceRetained);
    assert_eq!(fx.read(&out("dest/one.png")), b"one");
    assert_eq!(fx.read(&out("one.png")), b"a different size");
}

#[tokio::test]
async fn a_source_replaced_by_a_new_file_after_the_copy_is_retained_not_deleted() {
    let fx = Fixture::new();
    fx.write(&out("a.png"), b"original bytes");
    fx.mkdir(&out("dest"));
    let source = fx.path(&out("a.png"));
    let spare = fx.path(&out("spare.tmp"));
    inject(Faults {
        force_cross_device: true,
        after_publish: Some(Box::new(move || {
            // Same name, same size, but another file.
            fs::write(&spare, b"replaced bytes").unwrap();
            fs::rename(&spare, &source).unwrap();
        })),
        ..Faults::default()
    });
    let moved = fx.move_to(&out("a.png"), &out("dest")).await;
    assert_eq!(moved.outcome, ItemOutcome::SourceRetained, "{moved:?}");
    assert_eq!(moved.destination, Some(out("dest/a.png")));
    assert_eq!(
        moved.error,
        Some(GalleryError(GalleryErrorCode::SourceRetained))
    );
    assert_eq!(fx.read(&out("a.png")), b"replaced bytes");
    assert_eq!(fx.read(&out("dest/a.png")), b"original bytes");
    assert!(fx.staged(GalleryArea::Output).is_empty());
}

#[tokio::test]
async fn the_gate_protection_lives_until_the_blocking_work_has_returned() {
    let fx = Fixture::new();
    fx.write(&out("a.png"), b"a");
    fx.mkdir(&out("dest"));
    let owner = Arc::new(());
    let probe = Arc::downgrade(&owner);
    let alive_during_work = Arc::new(AtomicBool::new(false));
    let (seen, watched) = (alive_during_work.clone(), probe.clone());
    inject(Faults {
        force_cross_device: true,
        after_publish: Some(Box::new(move || {
            seen.store(watched.upgrade().is_some(), Ordering::SeqCst);
        })),
        ..Faults::default()
    });
    let entry = fx.entry(&out("a.png")).await;
    let moved = fx
        .files
        .transfer(
            &fx.root,
            &entry,
            &out("dest"),
            TransferMode::Move,
            "op1",
            Retained::new(owner),
        )
        .await;
    succeeded(&moved, &out("dest/a.png"));
    assert!(alive_during_work.load(Ordering::SeqCst));
    assert!(probe.upgrade().is_none(), "released once the work returned");
}

// ---- staging cleanup ----

#[tokio::test]
async fn cleanup_removes_proven_operation_copies_and_keeps_everything_else() {
    let fx = Fixture::new();
    fx.write(&out("user.png"), b"user data");
    let staging = fx.area(GalleryArea::Output).join(STAGING_DIR);
    let owned = |directory: &Path, operation: &str| {
        fs::create_dir_all(directory.join("payload")).unwrap();
        fs::write(directory.join("payload/x.bin"), b"partial").unwrap();
        fs::write(
            directory.join(".owner"),
            format!("vjump-gallery-staging:{operation}"),
        )
        .unwrap();
    };
    owned(&staging.join("op1"), "op1");
    // Wrong operation named in the marker, no marker, a stray file, a longer marker.
    owned(&staging.join("op2"), "someone-else");
    fs::create_dir_all(staging.join("op3")).unwrap();
    fs::write(staging.join("op3/precious.txt"), b"not ours").unwrap();
    fs::write(staging.join("stray.txt"), b"stray").unwrap();
    owned(&staging.join("op4"), "op4");
    fs::write(
        staging.join("op4/.owner"),
        "vjump-gallery-staging:op4 extra",
    )
    .unwrap();
    // The other area holds only an owned copy, so its staging folder disappears.
    let input_staging = fx.area(GalleryArea::Input).join(STAGING_DIR);
    owned(&input_staging.join("op5"), "op5");

    let mut warnings = fx.files.cleanup_staging(&fx.root).await;
    warnings.sort_by(|a, b| a.name.cmp(&b.name));
    let expected: Vec<_> = ["op2", "op3", "op4", "stray.txt"]
        .iter()
        .map(|leftover| GalleryWarning {
            name: Some((*leftover).into()),
            code: GalleryWarningCode::StagingLeftover,
        })
        .collect();
    assert_eq!(warnings, expected);
    assert_eq!(
        fx.staged(GalleryArea::Output),
        ["op2", "op3", "op4", "stray.txt"]
    );
    assert_eq!(
        fs::read(staging.join("op3/precious.txt")).unwrap(),
        b"not ours"
    );
    assert!(!input_staging.exists());
    assert_eq!(fx.read(&out("user.png")), b"user data");
    // Repeating it changes nothing.
    let mut again = fx.files.cleanup_staging(&fx.root).await;
    again.sort_by(|a, b| a.name.cmp(&b.name));
    assert_eq!(again, expected);
}

#[cfg(unix)]
#[tokio::test]
async fn cleanup_never_follows_a_linked_operation_directory() {
    let fx = Fixture::new();
    let elsewhere = fx.root.join("elsewhere");
    fs::create_dir_all(&elsewhere).unwrap();
    fs::write(elsewhere.join(".owner"), "vjump-gallery-staging:op1").unwrap();
    fs::write(elsewhere.join("keep.txt"), b"keep").unwrap();
    let staging = fx.area(GalleryArea::Output).join(STAGING_DIR);
    fs::create_dir_all(&staging).unwrap();
    std::os::unix::fs::symlink(&elsewhere, staging.join("op1")).unwrap();
    let warnings = fx.files.cleanup_staging(&fx.root).await;
    assert_eq!(warnings.len(), 1);
    assert!(elsewhere.join("keep.txt").exists());
}

#[tokio::test]
async fn cleanup_removes_an_empty_unmarked_operation_and_keeps_a_filled_one() {
    let fx = Fixture::new();
    let staging = fx.area(GalleryArea::Output).join(STAGING_DIR);
    fs::create_dir_all(staging.join("empty")).unwrap();
    fs::create_dir_all(staging.join("filled")).unwrap();
    fs::write(staging.join("filled/precious.txt"), b"not ours").unwrap();
    // A marked operation goes completely, payload and marker both.
    fs::create_dir_all(staging.join("marked/payload/deeper")).unwrap();
    fs::write(staging.join("marked/payload/deeper/x.bin"), b"partial").unwrap();
    fs::write(
        staging.join("marked/.owner"),
        "vjump-gallery-staging:marked",
    )
    .unwrap();
    fs::write(staging.join("marked/loose.bin"), b"partial").unwrap();

    let warnings = fx.files.cleanup_staging(&fx.root).await;
    assert_eq!(
        warnings,
        [GalleryWarning {
            name: Some("filled".into()),
            code: GalleryWarningCode::StagingLeftover
        }]
    );
    assert_eq!(fx.staged(GalleryArea::Output), ["filled"]);
    assert_eq!(
        fs::read(staging.join("filled/precious.txt")).unwrap(),
        b"not ours"
    );
}

#[tokio::test]
async fn a_leftover_that_cannot_be_removed_keeps_its_marker() {
    let fx = Fixture::new();
    let real = fx.root.join("real");
    fs::create_dir_all(&real).unwrap();
    fs::write(real.join("keep.txt"), b"keep").unwrap();
    let operation = fx.area(GalleryArea::Output).join(STAGING_DIR).join("op1");
    fs::create_dir_all(operation.join("payload")).unwrap();
    fs::write(operation.join(".owner"), "vjump-gallery-staging:op1").unwrap();
    fs::write(operation.join("payload/a.png"), b"half a copy").unwrap();
    if !directory_link(&real, &operation.join("payload/link")) {
        return;
    }
    // The link is refused, so the payload stays, and the marker must stay with it.
    for _ in 0..2 {
        let warnings = fx.files.cleanup_staging(&fx.root).await;
        assert_eq!(
            warnings,
            [GalleryWarning {
                name: Some("op1".into()),
                code: GalleryWarningCode::StagingLeftover
            }]
        );
        assert_eq!(
            fs::read(operation.join(".owner")).unwrap(),
            b"vjump-gallery-staging:op1"
        );
        assert!(operation.join("payload/link").exists());
    }
    assert!(real.join("keep.txt").exists());
}

// ---- delete ----

#[tokio::test]
async fn a_file_plan_records_its_size_and_goes_stale_when_the_file_changes() {
    let fx = Fixture::new();
    fx.write(&out("a.png"), b"12345");
    let plan = fx.plan(&out("a.png")).await.unwrap();
    assert_eq!(plan.kind, GalleryEntryKind::File);
    assert_eq!(plan.bytes, 5);
    assert!(!plan.fingerprint.is_empty());
    assert_eq!(plan.entry.location, out("a.png"));
    assert_eq!(fx.plan(&out("a.png")).await.unwrap(), plan, "deterministic");

    fx.write(&out("a.png"), b"1234567");
    assert_eq!(
        code(fx.files.trash(&fx.root, &plan).await),
        GalleryErrorCode::Stale
    );
    assert_eq!(
        fx.read(&out("a.png")),
        b"1234567",
        "a stale plan removes nothing"
    );
    // The entry reference itself may be stale at planning time.
    let seen = fx.entry(&out("a.png")).await;
    fx.write(&out("a.png"), b"1");
    assert_eq!(
        code(fx.files.plan_delete(&fx.root, &seen).await),
        GalleryErrorCode::Stale
    );
    fs::remove_file(fx.path(&out("a.png"))).unwrap();
    assert_eq!(
        code(fx.files.trash(&fx.root, &plan).await),
        GalleryErrorCode::NotFound
    );
}

#[tokio::test]
async fn a_folder_plan_covers_every_descendant() {
    let fx = Fixture::new();
    make_tree(&fx, "shots");
    let plan = fx.plan(&out("shots")).await.unwrap();
    assert_eq!(plan.kind, GalleryEntryKind::Directory);
    assert_eq!(plan.bytes, (11 + 11 + 9) as u64);
    assert_eq!(fx.plan(&out("shots")).await.unwrap(), plan, "deterministic");

    // A changed, added, removed or renamed descendant invalidates the plan.
    let nested = fx.path(&out("shots/sub/b.txt"));
    let mutations: [Box<dyn Fn()>; 4] = [
        Box::new(|| fs::write(&nested, b"nested FILE!").unwrap()),
        Box::new(|| fs::write(fx.path(&out("shots/new.txt")), b"n").unwrap()),
        Box::new(|| fs::remove_dir(fx.path(&out("shots/sub/empty"))).unwrap()),
        Box::new(|| {
            fs::rename(
                fx.path(&out("shots/a.png")),
                fx.path(&out("shots/renamed.png")),
            )
            .unwrap()
        }),
    ];
    for mutate in mutations {
        let before = fx.plan(&out("shots")).await.unwrap();
        mutate();
        assert_eq!(
            code(fx.files.trash(&fx.root, &before).await),
            GalleryErrorCode::Stale
        );
        assert!(fx.exists(&out("shots")));
    }
    assert_ne!(
        fx.plan(&out("shots")).await.unwrap().fingerprint,
        plan.fingerprint
    );
}

#[tokio::test]
async fn planning_a_folder_does_not_follow_links_inside_it() {
    let fx = Fixture::new();
    let real = fx.root.join("real");
    fs::create_dir_all(&real).unwrap();
    fs::write(real.join("big.bin"), vec![0u8; 4096]).unwrap();
    fx.write(&out("shots/a.png"), b"abc");
    if !directory_link(&real, &fx.path(&out("shots/link"))) {
        return;
    }
    let plan = fx.plan(&out("shots")).await.unwrap();
    assert_eq!(plan.bytes, 3, "link targets are not counted");
    assert!(real.join("big.bin").exists());
}

#[cfg(windows)]
#[tokio::test]
async fn trash_sends_a_file_and_a_folder_to_the_recycle_bin() {
    let fx = Fixture::new();
    fx.write(&out("recycle me ไทย.png"), b"bye");
    make_tree(&fx, "recycle folder");
    for target in [out("recycle me ไทย.png"), out("recycle folder")] {
        let plan = fx.plan(&target).await.unwrap();
        fx.files.trash(&fx.root, &plan).await.unwrap();
        assert!(!fx.exists(&target));
    }
    assert!(
        fx.list(&GalleryLocation::root(GalleryArea::Output))
            .await
            .entries
            .is_empty()
    );
}

// ---- preview and explorer ----

#[tokio::test]
async fn media_is_read_by_offset_from_a_pinned_regular_file() {
    let fx = Fixture::new();
    fx.write(&out("shots/clip.mp4"), b"0123456789");
    fx.mkdir(&out("folder"));
    let mut media = fx
        .files
        .open_media(&fx.root, &out("shots/clip.mp4"))
        .await
        .unwrap();
    assert_eq!(media.len(), 10);
    let mut buffer = [0u8; 4];
    assert_eq!(media.read_at(3, &mut buffer).unwrap(), 4);
    assert_eq!(&buffer, b"3456");
    assert_eq!(media.read_at(0, &mut buffer).unwrap(), 4);
    assert_eq!(&buffer, b"0123");
    assert_eq!(media.read_at(8, &mut buffer).unwrap(), 2);
    assert_eq!(&buffer[..2], b"89");
    assert_eq!(media.read_at(10, &mut buffer).unwrap(), 0);
    assert_eq!(media.read_at(100, &mut buffer).unwrap(), 0);
    drop(media);
    assert_eq!(
        code(fx.files.open_media(&fx.root, &out("folder")).await),
        GalleryErrorCode::InvalidRequest
    );
    assert_eq!(
        code(fx.files.open_media(&fx.root, &out("missing.png")).await),
        GalleryErrorCode::NotFound
    );
    assert_eq!(
        code(
            fx.files
                .open_media(&fx.root, &GalleryLocation::root(GalleryArea::Output))
                .await
        ),
        GalleryErrorCode::InvalidRequest
    );
}

#[cfg(windows)]
#[tokio::test]
async fn an_open_preview_pins_the_file_against_deletion_and_rename() {
    let fx = Fixture::new();
    fx.write(&out("clip.mp4"), b"0123456789");
    let media = fx
        .files
        .open_media(&fx.root, &out("clip.mp4"))
        .await
        .unwrap();
    assert!(fs::remove_file(fx.path(&out("clip.mp4"))).is_err());
    assert!(fs::rename(fx.path(&out("clip.mp4")), fx.path(&out("other.mp4"))).is_err());
    drop(media);
    fs::remove_file(fx.path(&out("clip.mp4"))).unwrap();
}

#[tokio::test]
async fn explorer_targets_are_built_from_the_pinned_folders() {
    let fx = Fixture::new();
    fx.write(&out("shots/a.png"), b"a");
    let area = |area: GalleryArea| fx.root.join("ComfyUI").join(area.dir_name());
    assert_eq!(
        fx.files
            .explorer_target(&fx.root, &GalleryLocation::root(GalleryArea::Input))
            .await
            .unwrap(),
        ExplorerTarget::OpenFolder(area(GalleryArea::Input))
    );
    assert_eq!(
        fx.files
            .explorer_target(&fx.root, &out("shots"))
            .await
            .unwrap(),
        ExplorerTarget::OpenFolder(area(GalleryArea::Output).join("shots"))
    );
    assert_eq!(
        fx.files
            .explorer_target(&fx.root, &out("shots/a.png"))
            .await
            .unwrap(),
        ExplorerTarget::RevealItem(area(GalleryArea::Output).join("shots").join("a.png"))
    );
    assert_eq!(
        code(
            fx.files
                .explorer_target(&fx.root, &out("shots/none.png"))
                .await
        ),
        GalleryErrorCode::NotFound
    );
}

#[tokio::test]
async fn locating_an_entry_never_creates_a_missing_area_folder() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("install");
    fs::create_dir_all(root.join("ComfyUI")).unwrap();
    let files = LocalGalleryFiles::new();
    for location in [
        GalleryLocation::root(GalleryArea::Output),
        out("shots"),
        out("shots/a.png"),
    ] {
        assert_eq!(
            code(files.explorer_target(&root, &location).await),
            GalleryErrorCode::NotFound,
            "{location:?}"
        );
    }
    assert!(!root.join("ComfyUI/output").exists());
    assert!(!root.join("ComfyUI/input").exists());
}
