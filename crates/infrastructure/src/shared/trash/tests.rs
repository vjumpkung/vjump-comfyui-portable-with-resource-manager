use super::{recycle, windows_recycle::*};
use std::{
    collections::HashMap,
    fs, io,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};
use windows::Win32::{Foundation::E_ABORT, UI::Shell::*};

fn refusals() -> &'static Mutex<HashMap<PathBuf, Arc<AtomicBool>>> {
    static PATHS: OnceLock<Mutex<HashMap<PathBuf, Arc<AtomicBool>>>> = OnceLock::new();
    PATHS.get_or_init(Mutex::default)
}

pub(crate) struct Refusal(PathBuf, Arc<AtomicBool>);

impl Refusal {
    pub(crate) fn was_refused(&self) -> bool {
        self.1.load(Ordering::SeqCst)
    }
}

impl Drop for Refusal {
    fn drop(&mut self) {
        refusals().lock().unwrap().remove(&self.0);
    }
}

pub(crate) fn refuse(path: &Path) -> Refusal {
    let path = path.canonicalize().unwrap();
    let seen = Arc::new(AtomicBool::new(false));
    assert!(
        refusals()
            .lock()
            .unwrap()
            .insert(path.clone(), seen.clone())
            .is_none()
    );
    Refusal(path, seen)
}

pub(super) fn refusal(path: &Path) -> Option<Arc<AtomicBool>> {
    refusals()
        .lock()
        .unwrap()
        .get(&path.canonicalize().ok()?)
        .cloned()
}

#[test]
fn windows_shell_names_convert_verbatim_drive_and_unc_paths() {
    use std::os::windows::ffi::OsStrExt;
    for (path, expected) in [
        (r"\\?\C:\models\file ไทย", r"C:\models\file ไทย"),
        (r"\\?\UNC\server\share\file ไทย", r"\\server\share\file ไทย"),
        (r"C:\models\file ไทย", r"C:\models\file ไทย"),
        (r"\\server\share\file ไทย", r"\\server\share\file ไทย"),
        (r"relative\file ไทย", r"relative\file ไทย"),
    ] {
        let expected: Vec<u16> = std::ffi::OsStr::new(expected)
            .encode_wide()
            .chain(Some(0))
            .collect();
        assert_eq!(shell_name(Path::new(path)).unwrap(), expected);
    }
}

#[test]
fn windows_recycle_accepts_verbatim_drive_paths() {
    use std::path::{Component, Prefix};
    let temp = tempfile::tempdir().unwrap();
    for directory in [false, true] {
        let path = temp.path().join(if directory {
            "verbatim directory ไทย"
        } else {
            "verbatim file ไทย"
        });
        if directory {
            fs::create_dir(&path).unwrap();
            fs::write(path.join("sentinel"), b"recycle").unwrap();
        } else {
            fs::write(&path, b"recycle").unwrap();
        }
        let path = path.canonicalize().unwrap();
        assert!(matches!(
            path.components().next(),
            Some(Component::Prefix(prefix)) if matches!(prefix.kind(), Prefix::VerbatimDisk(_))
        ));
        recycle(&path).unwrap();
        assert_eq!(
            fs::symlink_metadata(&path).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
    }
}

#[test]
fn windows_recycle_refuses_unsupported_verbatim_forms() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("unsupported-namespace");
    fs::write(&path, b"keep").unwrap();
    let canonical = path.canonicalize().unwrap();
    let device = PathBuf::from(canonical.to_str().unwrap().replacen(r"\\?\", r"\\.\", 1));
    assert_eq!(fs::read(&device).unwrap(), b"keep");
    for unsupported in [
        device.as_path(),
        Path::new(r"\\?\Volume{00000000-0000-0000-0000-000000000000}\unsupported-namespace"),
    ] {
        assert_eq!(
            recycle(unsupported).unwrap_err().kind(),
            io::ErrorKind::Unsupported
        );
        assert_eq!(fs::read(&path).unwrap(), b"keep");
    }
}

#[test]
fn windows_recycle_sink_rejects_permanent_delete_flags() {
    let completion = Arc::new(Mutex::new(Completion::default()));
    let sink: IFileOperationProgressSink = RecycleSink {
        completion: completion.clone(),
        refusal: None,
    }
    .into();
    for flags in [0, TSF_DELETE_RECYCLE_IF_POSSIBLE.0 as u32 ^ u32::MAX] {
        let error = unsafe { sink.PreDeleteItem(flags, None) }.unwrap_err();
        assert_eq!(error.code(), E_ABORT);
        let state = completion.lock().unwrap();
        assert!(state.refused);
        assert_eq!(state.post_delete, 0);
        assert!(!state.recycled_destination);
    }
}

#[test]
fn windows_recycle_sink_refusal_preserves_the_item() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("recycle-refusal");
    fs::create_dir(&path).unwrap();
    fs::write(path.join("sentinel"), b"keep").unwrap();
    let refusal = refuse(&path);
    assert!(recycle(&path).is_err());
    assert!(
        refusal.was_refused(),
        "the real operation must reach PreDeleteItem"
    );
    assert!(path.is_dir());
    assert_eq!(fs::read(path.join("sentinel")).unwrap(), b"keep");
}

#[test]
fn windows_recycle_requires_positive_completion() {
    let positive = Completion {
        pre_delete: 1,
        post_delete: 1,
        recycled_destination: true,
        ..Default::default()
    };
    let absent = || Err(io::ErrorKind::NotFound.into());
    assert!(positive.check(Ok(()), Ok(false), absent()).is_ok());
    for state in [
        Completion::default(),
        Completion {
            refused: true,
            ..positive.clone()
        },
        Completion {
            pre_delete: 0,
            ..positive.clone()
        },
        Completion {
            post_delete: 0,
            ..positive.clone()
        },
        Completion {
            pre_delete: 2,
            ..positive.clone()
        },
        Completion {
            failed: true,
            ..positive.clone()
        },
        Completion {
            recycled_destination: false,
            ..positive.clone()
        },
    ] {
        assert!(state.check(Ok(()), Ok(false), absent()).is_err());
    }
    assert!(
        positive
            .check(Err(E_ABORT.into()), Ok(false), absent())
            .is_err()
    );
    assert!(positive.check(Ok(()), Ok(true), absent()).is_err());
    assert!(
        positive
            .check(Ok(()), Err(E_ABORT.into()), absent())
            .is_err()
    );
    assert!(
        positive
            .check(
                Ok(()),
                Ok(false),
                Err(io::ErrorKind::PermissionDenied.into())
            )
            .is_err()
    );
    let temp = tempfile::tempdir().unwrap();
    assert!(
        positive
            .check(Ok(()), Ok(false), fs::symlink_metadata(temp.path()))
            .is_err()
    );

    // A failed item result or a missing recycled destination poisons completion.
    for failed in [true, false] {
        let completion = Arc::new(Mutex::new(positive.clone()));
        let sink: IFileOperationProgressSink = RecycleSink {
            completion: completion.clone(),
            refusal: None,
        }
        .into();
        unsafe {
            sink.PostDeleteItem(
                0,
                None,
                if failed {
                    E_ABORT
                } else {
                    windows_core::HRESULT(0)
                },
                None,
            )
        }
        .unwrap();
        assert!(
            completion
                .lock()
                .unwrap()
                .check(Ok(()), Ok(false), absent())
                .is_err()
        );
    }
}

#[test]
fn windows_real_recycle_confirms_file_and_directory_completion() {
    let temp = tempfile::tempdir().unwrap();
    for directory in [false, true] {
        let path = temp.path().join(if directory {
            "directory ไทย"
        } else {
            "file ไทย"
        });
        if directory {
            fs::create_dir(&path).unwrap();
            fs::write(path.join("sentinel"), b"recycle").unwrap();
        } else {
            fs::write(&path, b"recycle").unwrap();
        }
        let parent = crate::shared::model_paths::pin_absolute(temp.path()).unwrap();
        let name = domain::shared::model_values::SafeFileName::new(
            path.file_name().unwrap().to_str().unwrap(),
        )
        .unwrap();
        let expected =
            crate::shared::model_paths::identity(&parent.open_entry(&name).unwrap()).unwrap();
        super::recycle_entry(&parent, &name, &expected).unwrap();
        assert_eq!(
            fs::symlink_metadata(&path).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
    }
}
