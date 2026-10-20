//! Real Git: an update keeps ComfyUI on the default branch whenever nothing local is lost.

use std::{fs, path::Path, process::Command};

use application::{
    comfy_runtime::{ComfyRepository, FrozenUpdate},
    environment::{NullSink, RunId, StepContext, StepId},
    shared::CancelToken,
};

use super::*;
use crate::{
    environment::source::{Endpoints, GitSource},
    shared::download::Downloader,
};

fn locate_git() -> PathBuf {
    let names: &[&str] = if cfg!(windows) {
        &["git.exe"]
    } else {
        &["git"]
    };
    std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .flat_map(|dir| names.iter().map(move |name| dir.join(name)))
        .find(|candidate| candidate.is_file())
        .expect("git must be installed to run the update tests")
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

/// A plain clone of a local remote whose `main` has v1.0.0 and v1.1.0 (the clone is at v1.1.0).
struct Install {
    _directory: tempfile::TempDir,
    remote: PathBuf,
    root: PathBuf,
    comfy: PathBuf,
    first: String,
    second: String,
}

impl Install {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let remote = directory.path().join("remote");
        fs::create_dir_all(&remote).unwrap();
        git(&remote, &["init", "-b", "main"]);
        fs::write(remote.join("README.md"), "one").unwrap();
        git(&remote, &["add", "-A"]);
        git(&remote, &["commit", "-m", "first"]);
        git(&remote, &["tag", "v1.0.0"]);
        let first = git(&remote, &["rev-parse", "HEAD"]);
        fs::write(remote.join("README.md"), "two").unwrap();
        git(&remote, &["commit", "-am", "second"]);
        git(&remote, &["tag", "v1.1.0"]);
        let second = git(&remote, &["rev-parse", "HEAD"]);
        let root = directory.path().join("install root");
        fs::create_dir_all(&root).unwrap();
        let comfy = root.join("ComfyUI");
        git(&root, &["clone", "../remote", "ComfyUI"]);
        Self {
            _directory: directory,
            remote,
            root,
            comfy,
            first,
            second,
        }
    }

    /// Publishes `release/v1.0` with the patch tag `v1.0.1` (only on that branch) and fetches
    /// it into the install. Returns the tagged commit.
    fn publish_patch(&self) -> String {
        git(&self.remote, &["checkout", "-b", "release/v1.0", "v1.0.0"]);
        fs::write(self.remote.join("README.md"), "patch").unwrap();
        git(&self.remote, &["commit", "-am", "patch"]);
        git(&self.remote, &["tag", "v1.0.1"]);
        let patch = git(&self.remote, &["rev-parse", "HEAD"]);
        git(&self.remote, &["checkout", "main"]);
        git(&self.comfy, &["fetch", "--tags"]);
        patch
    }

    /// Adds a second patch commit to the remote's `release/v1.0`. Returns that commit.
    fn publish_patch_two(&self) -> String {
        git(&self.remote, &["checkout", "release/v1.0"]);
        fs::write(self.remote.join("README.md"), "patch two").unwrap();
        git(&self.remote, &["commit", "-am", "patch two"]);
        let patch_two = git(&self.remote, &["rev-parse", "HEAD"]);
        git(&self.remote, &["checkout", "main"]);
        patch_two
    }

    fn repository(&self) -> GitRepository {
        let source = GitSource::with(Downloader::new(), Endpoints::pinned(), Some(locate_git()));
        GitRepository::with_git(Arc::new(source), locate_git())
    }

    async fn switch(&self, tag: &str, commit: &str) {
        let frozen = FrozenUpdate {
            commit: commit.to_owned(),
            tag: Some(tag.to_owned()),
            branch: None,
            fast_forward: false,
        };
        self.repository()
            .apply(&self.root, &frozen, &ctx())
            .await
            .unwrap();
    }

    /// `main` when HEAD is on that branch, else `HEAD`.
    fn head(&self) -> String {
        git(&self.comfy, &["rev-parse", "--abbrev-ref", "HEAD"])
    }
}

fn ctx() -> StepContext {
    StepContext {
        run_id: RunId::new("test").unwrap(),
        step: StepId::CloneComfyUi,
        sink: Arc::new(NullSink),
        cancel: CancelToken::new(),
        allow_source_builds: false,
        retained: Default::default(),
    }
}

#[tokio::test]
async fn switching_tags_on_an_attached_checkout_keeps_the_branch() {
    let install = Install::new();
    assert_eq!(install.head(), "main");
    install.switch("v1.0.0", &install.first).await;
    assert_eq!(install.head(), "main");
    assert_eq!(git(&install.comfy, &["rev-parse", "HEAD"]), install.first);
    assert_eq!(
        git(&install.comfy, &["rev-parse", "main"]),
        install.first,
        "the local branch pointer moved with the checkout"
    );
    assert_eq!(
        git(
            &install.comfy,
            &["rev-parse", "--abbrev-ref", "main@{upstream}"]
        ),
        "origin/main"
    );
    let revision = install.repository().revision(&install.root).await.unwrap();
    assert_eq!(revision.branch.as_deref(), Some("main"));
    assert_eq!(revision.tag.as_deref(), Some("v1.0.0"));

    // Back to the newer tag, then a plain pull has nothing left to do.
    install.switch("v1.1.0", &install.second).await;
    assert_eq!(install.head(), "main");
    assert_eq!(git(&install.comfy, &["rev-parse", "main"]), install.second);
    git(&install.comfy, &["pull", "--ff-only"]);
}

#[tokio::test]
async fn a_detached_checkout_is_attached_on_update() {
    let install = Install::new();
    git(&install.comfy, &["checkout", "--detach", "v1.1.0"]);
    assert_eq!(install.head(), "HEAD");
    install.switch("v1.0.0", &install.first).await;
    assert_eq!(install.head(), "main");
    assert_eq!(git(&install.comfy, &["rev-parse", "main"]), install.first);
    assert_eq!(
        git(
            &install.comfy,
            &["rev-parse", "--abbrev-ref", "main@{upstream}"]
        ),
        "origin/main"
    );
    git(&install.comfy, &["pull", "--ff-only"]);
    assert_eq!(git(&install.comfy, &["rev-parse", "HEAD"]), install.second);

    // Without a local default branch it is created and made to track origin.
    git(&install.comfy, &["checkout", "--detach", "v1.1.0"]);
    git(&install.comfy, &["branch", "-D", "main"]);
    install.switch("v1.0.0", &install.first).await;
    assert_eq!(install.head(), "main");
    assert_eq!(
        git(
            &install.comfy,
            &["rev-parse", "--abbrev-ref", "main@{upstream}"]
        ),
        "origin/main"
    );
}

#[tokio::test]
async fn local_commits_keep_the_update_detached() {
    let install = Install::new();
    fs::write(install.comfy.join("mine.txt"), "local work").unwrap();
    git(&install.comfy, &["add", "-A"]);
    git(&install.comfy, &["commit", "-m", "local work"]);
    let local = git(&install.comfy, &["rev-parse", "HEAD"]);

    install.switch("v1.0.0", &install.first).await;
    assert_eq!(install.head(), "HEAD");
    assert_eq!(git(&install.comfy, &["rev-parse", "HEAD"]), install.first);
    assert_eq!(
        git(&install.comfy, &["rev-parse", "main"]),
        local,
        "the unpublished commit stays on the branch"
    );
    assert_eq!(
        git(&install.comfy, &["status", "--porcelain"]),
        "",
        "nothing is lost from the tree either"
    );

    // Detached with the same unpublished branch: still not attached, still not lost.
    install.switch("v1.1.0", &install.second).await;
    assert_eq!(install.head(), "HEAD");
    assert_eq!(git(&install.comfy, &["rev-parse", "main"]), local);
}

#[tokio::test]
async fn a_patch_tag_switch_attaches_to_its_release_branch_and_back() {
    let install = Install::new();
    let patch = install.publish_patch();
    let main = git(&install.comfy, &["rev-parse", "main"]);

    install.switch("v1.0.1", &patch).await;
    assert_eq!(install.head(), "release/v1.0");
    assert_eq!(git(&install.comfy, &["rev-parse", "HEAD"]), patch);
    assert_eq!(
        git(
            &install.comfy,
            &["rev-parse", "--abbrev-ref", "release/v1.0@{upstream}"]
        ),
        "origin/release/v1.0"
    );
    assert_eq!(
        git(&install.comfy, &["rev-parse", "main"]),
        main,
        "the default branch is left alone"
    );
    let revision = install.repository().revision(&install.root).await.unwrap();
    assert_eq!(revision.branch.as_deref(), Some("release/v1.0"));
    assert_eq!(revision.tag.as_deref(), Some("v1.0.1"));

    // A plain pull brings the next patch of the same release line.
    let patch_two = install.publish_patch_two();
    git(&install.comfy, &["pull", "--ff-only"]);
    assert_eq!(git(&install.comfy, &["rev-parse", "HEAD"]), patch_two);

    // Back to a tag on the default branch.
    install.switch("v1.1.0", &install.second).await;
    assert_eq!(install.head(), "main");
    assert_eq!(git(&install.comfy, &["rev-parse", "main"]), install.second);
    assert_eq!(
        git(&install.comfy, &["rev-parse", "release/v1.0"]),
        patch_two,
        "the release branch keeps its published commits"
    );
}

#[tokio::test]
async fn switching_to_the_current_tag_attaches_a_detached_checkout() {
    let install = Install::new();
    let patch = install.publish_patch();
    git(&install.comfy, &["checkout", "--detach", "v1.0.1"]);
    assert_eq!(install.head(), "HEAD");

    // The version card re-applies the tag the checkout is already on.
    install.switch("v1.0.1", &patch).await;
    assert_eq!(install.head(), "release/v1.0");
    assert_eq!(git(&install.comfy, &["rev-parse", "HEAD"]), patch);
    assert_eq!(
        git(
            &install.comfy,
            &["rev-parse", "--abbrev-ref", "release/v1.0@{upstream}"]
        ),
        "origin/release/v1.0"
    );

    // The same for a tag on the default branch.
    git(&install.comfy, &["checkout", "--detach", "v1.1.0"]);
    install.switch("v1.1.0", &install.second).await;
    assert_eq!(install.head(), "main");
    assert_eq!(git(&install.comfy, &["rev-parse", "HEAD"]), install.second);
    git(&install.comfy, &["pull", "--ff-only"]);
}

#[tokio::test]
async fn local_commits_on_the_release_branch_keep_the_update_detached() {
    let install = Install::new();
    let patch = install.publish_patch();
    install.switch("v1.0.1", &patch).await;
    assert_eq!(install.head(), "release/v1.0");
    fs::write(install.comfy.join("mine.txt"), "local work").unwrap();
    git(&install.comfy, &["add", "-A"]);
    git(&install.comfy, &["commit", "-m", "local work"]);
    let local = git(&install.comfy, &["rev-parse", "HEAD"]);

    install.switch("v1.1.0", &install.second).await;
    assert_eq!(install.head(), "main");
    assert_eq!(git(&install.comfy, &["rev-parse", "release/v1.0"]), local);

    // The unpublished release commit is neither attached nor lost.
    install.switch("v1.0.1", &patch).await;
    assert_eq!(install.head(), "HEAD");
    assert_eq!(git(&install.comfy, &["rev-parse", "HEAD"]), patch);
    assert_eq!(git(&install.comfy, &["rev-parse", "release/v1.0"]), local);
    assert_eq!(git(&install.comfy, &["status", "--porcelain"]), "");
}

#[tokio::test]
async fn latest_commit_update_moves_a_release_branch_install_to_the_default_branch_head() {
    let install = Install::new();
    let patch = install.publish_patch();
    install.switch("v1.0.1", &patch).await;
    let repository = install.repository();
    repository
        .fetch(&install.root, &UpdateTarget::LatestCommit, &ctx())
        .await
        .unwrap();
    let frozen = repository
        .freeze(&install.root, &UpdateTarget::LatestCommit)
        .await
        .unwrap();
    assert_eq!(frozen.commit, install.second);
    assert_eq!(frozen.branch.as_deref(), Some("main"));
    assert_eq!(frozen.tag, None);
    assert!(!frozen.fast_forward);
    repository
        .apply(&install.root, &frozen, &ctx())
        .await
        .unwrap();
    assert_eq!(install.head(), "main");
    assert_eq!(git(&install.comfy, &["rev-parse", "HEAD"]), frozen.commit);
    assert_eq!(
        git(
            &install.comfy,
            &["rev-parse", "--abbrev-ref", "@{upstream}"]
        ),
        "origin/main"
    );
    assert_eq!(git(&install.comfy, &["rev-parse", "release/v1.0"]), patch);
    fs::write(install.remote.join("README.md"), "development three").unwrap();
    git(&install.remote, &["commit", "-am", "development three"]);
    let third = git(&install.remote, &["rev-parse", "HEAD"]);
    git(&install.comfy, &["pull", "--ff-only"]);
    assert_eq!(git(&install.comfy, &["rev-parse", "HEAD"]), third);
}

#[tokio::test]
async fn latest_commit_update_with_unpublished_default_branch_commits_is_detached() {
    for user_branch in [false, true] {
        let install = Install::new();
        let branch = if user_branch { "my-work" } else { "main" };
        if user_branch {
            git(&install.comfy, &["checkout", "-b", branch]);
        }
        fs::write(install.comfy.join("mine.txt"), "unpublished").unwrap();
        git(&install.comfy, &["add", "-A"]);
        git(&install.comfy, &["commit", "-m", "unpublished"]);
        let local = git(&install.comfy, &["rev-parse", "HEAD"]);
        let repository = install.repository();
        repository
            .fetch(&install.root, &UpdateTarget::LatestCommit, &ctx())
            .await
            .unwrap();
        let frozen = repository
            .freeze(&install.root, &UpdateTarget::LatestCommit)
            .await
            .unwrap();
        repository
            .apply(&install.root, &frozen, &ctx())
            .await
            .unwrap();
        assert_eq!(install.head(), "HEAD");
        assert_eq!(git(&install.comfy, &["rev-parse", "HEAD"]), install.second);
        assert_eq!(git(&install.comfy, &["rev-parse", branch]), local);
        assert_eq!(git(&install.comfy, &["status", "--porcelain"]), "");
    }
}

#[tokio::test]
async fn latest_commit_apply_preserves_the_frozen_commit_and_branch() {
    let install = Install::new();
    let patch = install.publish_patch();
    install.switch("v1.0.1", &patch).await;
    let repository = install.repository();
    repository
        .fetch(&install.root, &UpdateTarget::LatestCommit, &ctx())
        .await
        .unwrap();
    let frozen = repository
        .freeze(&install.root, &UpdateTarget::LatestCommit)
        .await
        .unwrap();
    fs::write(install.remote.join("README.md"), "newer development").unwrap();
    git(&install.remote, &["commit", "-am", "newer development"]);
    git(&install.remote, &["branch", "new-default"]);
    repository
        .fetch(&install.root, &UpdateTarget::LatestCommit, &ctx())
        .await
        .unwrap();
    git(
        &install.comfy,
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/new-default",
        ],
    );
    assert_ne!(
        git(&install.comfy, &["rev-parse", "origin/main"]),
        frozen.commit
    );
    repository
        .apply(&install.root, &frozen, &ctx())
        .await
        .unwrap();
    assert_eq!(git(&install.comfy, &["rev-parse", "HEAD"]), frozen.commit);
    assert_eq!(install.head(), "main");
    assert_eq!(
        git(
            &install.comfy,
            &["rev-parse", "--abbrev-ref", "@{upstream}"]
        ),
        "origin/main"
    );
    assert_eq!(git(&install.comfy, &["rev-parse", "release/v1.0"]), patch);
    assert_eq!(frozen.branch.as_deref(), Some("main"));
}

#[tokio::test]
async fn latest_commit_fetches_origin_even_when_current_upstream_is_elsewhere() {
    let install = Install::new();
    let elsewhere = install._directory.path().join("elsewhere");
    git(install._directory.path(), &["clone", "remote", "elsewhere"]);
    git(
        &install.comfy,
        &["remote", "add", "elsewhere", elsewhere.to_str().unwrap()],
    );
    git(&install.comfy, &["fetch", "elsewhere"]);
    git(
        &install.comfy,
        &["branch", "--set-upstream-to=elsewhere/main", "main"],
    );
    // A restricted origin refspec must not leave origin/main stale.
    git(
        &install.comfy,
        &[
            "config",
            "remote.origin.fetch",
            "+refs/heads/release/*:refs/remotes/origin/release/*",
        ],
    );
    fs::write(install.remote.join("README.md"), "new origin tip").unwrap();
    git(&install.remote, &["commit", "-am", "new origin tip"]);
    let tip = git(&install.remote, &["rev-parse", "HEAD"]);
    assert_eq!(
        git(&install.comfy, &["rev-parse", "origin/main"]),
        install.second
    );
    let repository = install.repository();
    repository
        .fetch(&install.root, &UpdateTarget::LatestCommit, &ctx())
        .await
        .unwrap();
    let frozen = repository
        .freeze(&install.root, &UpdateTarget::LatestCommit)
        .await
        .unwrap();
    assert_eq!(git(&install.comfy, &["rev-parse", "origin/main"]), tip);
    assert_eq!(
        git(&install.comfy, &["rev-parse", "elsewhere/main"]),
        install.second
    );
    assert_eq!(frozen.commit, tip);
    assert_eq!(frozen.branch.as_deref(), Some("main"));
}

#[tokio::test]
async fn latest_commit_without_a_default_branch_fails_before_checkout() {
    let install = Install::new();
    let repository = install.repository();
    let head = git(&install.comfy, &["rev-parse", "HEAD"]);
    let branches = git(&install.comfy, &["show-ref", "--heads"]);
    git(
        &install.comfy,
        &["symbolic-ref", "--delete", "refs/remotes/origin/HEAD"],
    );
    let result = repository
        .freeze(&install.root, &UpdateTarget::LatestCommit)
        .await;
    assert_eq!(
        result,
        Err(RuntimeError::Failed(
            "The remote has no default branch.".into()
        ))
    );
    assert_eq!(git(&install.comfy, &["rev-parse", "HEAD"]), head);
    assert_eq!(git(&install.comfy, &["show-ref", "--heads"]), branches);
    // A symbolic default whose tip cannot be resolved also fails before checkout.
    git(
        &install.comfy,
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/missing",
        ],
    );
    assert!(
        repository
            .freeze(&install.root, &UpdateTarget::LatestCommit)
            .await
            .is_err()
    );
    assert_eq!(git(&install.comfy, &["rev-parse", "HEAD"]), head);
    assert_eq!(git(&install.comfy, &["show-ref", "--heads"]), branches);
}
