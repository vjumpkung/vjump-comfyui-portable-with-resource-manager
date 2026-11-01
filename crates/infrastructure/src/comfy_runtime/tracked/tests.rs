use super::*;
use std::{fs, path::PathBuf, process::Command};

pub(crate) fn git(directory: &Path, args: &[&str]) {
    let result = Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(args)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

/// Uses real Git in a temporary installation without downloading portable tools.
pub(crate) fn repository(root: &Path) -> PathBuf {
    let comfy = root.join("ComfyUI");
    fs::create_dir_all(&comfy).unwrap();
    git(&comfy, &["init"]);
    #[cfg(windows)]
    {
        let program = std::env::split_paths(&std::env::var_os("PATH").unwrap())
            .map(|directory| directory.join("git.exe"))
            .find(|program| program.is_file())
            .expect("Git is required for repository protection tests");
        let home = program
            .ancestors()
            .find(|directory| directory.join("cmd/git.exe").is_file())
            .expect("Git for Windows installation");
        let tools = root.join("tools");
        fs::create_dir_all(&tools).unwrap();
        let result = Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(tools.join("git"))
            .arg(home)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    comfy
}

#[tokio::test]
async fn absent_checkout_git_does_not_consult_a_parent_repository() {
    let directory = tempfile::tempdir().unwrap();
    git(directory.path(), &["init"]);
    fs::create_dir_all(directory.path().join("ComfyUI")).unwrap();
    assert_eq!(
        comfy_tracked_files(directory.path()).await,
        TrackedFiles::NotRepository
    );
}

#[tokio::test]
async fn queries_raw_tracked_paths_under_only_the_three_areas() {
    let directory = tempfile::tempdir().unwrap();
    let comfy = repository(directory.path());
    for path in [
        "models/configs/a.yaml",
        "input/nested/ไทย.png",
        "output/token=filename.wav",
        "user/visible.json",
    ] {
        let file = comfy.join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, b"tracked").unwrap();
    }
    git(&comfy, &["add", "."]);
    let tracked = comfy_tracked_files(directory.path()).await;
    assert!(matches!(tracked, TrackedFiles::Known(_)), "{tracked:?}");
    assert!(tracked.hides("models/configs/a.yaml"));
    assert!(tracked.hides("input/nested/ไทย.png"));
    assert!(tracked.hides("output/token=filename.wav"));
    assert!(!tracked.hides("user/visible.json"));
    assert!(!tracked.hides("input/nested"));
    assert_eq!(tracked.guard("input/nested"), Err(Protected));
    assert_eq!(tracked.guard("input/nest"), Ok(()));
}

#[tokio::test]
async fn gitfile_checkout_is_a_confirmed_repository() {
    let directory = tempfile::tempdir().unwrap();
    let comfy = repository(directory.path());
    fs::create_dir_all(comfy.join("input")).unwrap();
    fs::write(comfy.join("input/example.png"), b"tracked").unwrap();
    git(&comfy, &["add", "input"]);
    fs::rename(comfy.join(".git"), directory.path().join("checkout.git")).unwrap();
    fs::write(comfy.join(".git"), "gitdir: ../checkout.git\n").unwrap();
    assert!(
        comfy_tracked_files(directory.path())
            .await
            .hides("input/example.png")
    );
}

#[tokio::test]
async fn confirmed_but_broken_repository_allows_mutations() {
    let directory = tempfile::tempdir().unwrap();
    let comfy = repository(directory.path());
    fs::remove_dir_all(comfy.join(".git")).unwrap();
    fs::write(comfy.join(".git"), "gitdir: missing").unwrap();
    let tracked = comfy_tracked_files(directory.path()).await;
    assert_eq!(tracked, TrackedFiles::Unavailable);
    assert_eq!(tracked.guard("input/untracked.png"), Ok(()));
    assert!(!tracked.hides("input/untracked.png"));
    assert_eq!(TrackedFiles::NotRepository.guard("input/a"), Ok(()));
}

async fn counted(root: &Path, calls: &std::sync::atomic::AtomicUsize) -> TrackedFiles {
    comfy_tracked_files_with(root, |layout| async move {
        calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        query(layout).await
    })
    .await
}

#[tokio::test]
async fn cache_hit_does_not_spawn_git_and_changed_index_invalidates() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let directory = tempfile::tempdir().unwrap();
    let comfy = repository(directory.path());
    fs::create_dir_all(comfy.join("input")).unwrap();
    fs::write(comfy.join("input/first.png"), b"first").unwrap();
    git(&comfy, &["add", "input"]);
    let calls = AtomicUsize::new(0);
    assert!(
        counted(directory.path(), &calls)
            .await
            .hides("input/first.png")
    );
    assert!(
        counted(directory.path(), &calls)
            .await
            .hides("input/first.png")
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    fs::write(comfy.join("input/second.png"), b"second").unwrap();
    git(&comfy, &["add", "input"]);
    assert!(
        counted(directory.path(), &calls)
            .await
            .hides("input/second.png")
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn equal_size_and_mtime_index_replacement_invalidates() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let directory = tempfile::tempdir().unwrap();
    let comfy = repository(directory.path());
    fs::create_dir_all(comfy.join("input")).unwrap();
    fs::write(comfy.join("input/first.png"), b"same").unwrap();
    git(&comfy, &["add", "input"]);
    let index = comfy.join(".git/index");
    let before = fs::metadata(&index).unwrap();
    let old_content = fs::read(&index).unwrap();
    let calls = AtomicUsize::new(0);
    assert!(
        counted(directory.path(), &calls)
            .await
            .hides("input/first.png")
    );
    git(&comfy, &["rm", "--cached", "input/first.png"]);
    fs::write(comfy.join("input/other.png"), b"same").unwrap();
    git(&comfy, &["add", "input/other.png"]);
    let replacement = comfy.join(".git/replacement");
    fs::rename(&index, &replacement).unwrap();
    File::options()
        .write(true)
        .open(&replacement)
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(before.modified().unwrap()))
        .unwrap();
    fs::rename(&replacement, &index).unwrap();
    let after = fs::metadata(&index).unwrap();
    assert_eq!(before.len(), after.len());
    assert_eq!(before.modified().unwrap(), after.modified().unwrap());
    assert_ne!(old_content, fs::read(&index).unwrap());
    let tracked = counted(directory.path(), &calls).await;
    assert!(tracked.hides("input/other.png"));
    assert!(!tracked.hides("input/first.png"));
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn failed_query_is_not_cached() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let directory = tempfile::tempdir().unwrap();
    let comfy = repository(directory.path());
    fs::create_dir_all(comfy.join("input")).unwrap();
    fs::write(comfy.join("input/example.png"), b"tracked").unwrap();
    git(&comfy, &["add", "input"]);
    assert_eq!(
        comfy_tracked_files_with(directory.path(), |_| async { TrackedFiles::Unavailable }).await,
        TrackedFiles::Unavailable
    );
    let calls = AtomicUsize::new(0);
    assert!(
        counted(directory.path(), &calls)
            .await
            .hides("input/example.png")
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn index_changed_during_query_is_not_cached() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let directory = tempfile::tempdir().unwrap();
    let comfy = repository(directory.path());
    fs::create_dir_all(comfy.join("input")).unwrap();
    fs::write(comfy.join("input/first.png"), b"first").unwrap();
    git(&comfy, &["add", "input"]);
    comfy_tracked_files_with(directory.path(), |layout| async {
        let tracked = query(layout).await;
        fs::write(comfy.join("input/second.png"), b"second").unwrap();
        git(&comfy, &["add", "input"]);
        tracked
    })
    .await;
    let calls = AtomicUsize::new(0);
    assert!(
        counted(directory.path(), &calls)
            .await
            .hides("input/second.png")
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn gitfile_always_bypasses_cache() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let directory = tempfile::tempdir().unwrap();
    let comfy = repository(directory.path());
    fs::create_dir_all(comfy.join("input")).unwrap();
    fs::write(comfy.join("input/example.png"), b"tracked").unwrap();
    git(&comfy, &["add", "input"]);
    assert!(
        comfy_tracked_files(directory.path())
            .await
            .hides("input/example.png")
    );
    fs::rename(comfy.join(".git"), directory.path().join("checkout.git")).unwrap();
    fs::write(comfy.join(".git"), "gitdir: ../checkout.git\n").unwrap();
    let calls = AtomicUsize::new(0);
    for _ in 0..2 {
        assert!(
            counted(directory.path(), &calls)
                .await
                .hides("input/example.png")
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn component_and_platform_case_matching() {
    let tracked = TrackedFiles::Known(HashSet::from(["models/Configs/ไทย.yaml".into()]));
    assert_eq!(tracked.guard("models/Configs"), Err(Protected));
    assert_eq!(tracked.guard("models/Config"), Ok(()));
    #[cfg(windows)]
    {
        assert!(tracked.hides("MODELS/configs/ไทย.YAML"));
        assert_eq!(tracked.guard("MODELS/CONFIGS"), Err(Protected));
    }
    #[cfg(not(windows))]
    {
        assert!(!tracked.hides("MODELS/configs/ไทย.YAML"));
        assert_eq!(tracked.guard("MODELS/CONFIGS"), Ok(()));
    }
}
