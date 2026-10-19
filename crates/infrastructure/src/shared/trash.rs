use super::model_paths::{PinnedDirectory, identity};
use domain::{models::acquisition::FileIdentity, shared::model_values::SafeFileName};
use std::{io, path::Path};

/// Recycles exactly a validated entry while its parent remains protected. Failure never
/// falls back to permanent deletion; the caller retains the backup and its journal.
pub fn recycle_entry(
    parent: &PinnedDirectory,
    name: &SafeFileName,
    expected: &FileIdentity,
) -> io::Result<()> {
    let entry = parent.open_entry(name)?;
    if identity(&entry)? != *expected {
        return Err(io::ErrorKind::PermissionDenied.into());
    }
    #[cfg(windows)]
    {
        recycle(&parent.path.join(name.as_str()))
    }
    #[cfg(target_os = "linux")]
    {
        recycle_linux(parent, name, &entry)
    }
}

#[cfg(target_os = "linux")]
fn recycle_linux(
    parent: &PinnedDirectory,
    name: &SafeFileName,
    entry: &std::fs::File,
) -> io::Result<()> {
    use super::model_paths::{pin_absolute, rename_no_replace};
    use application::shared::IdGenerator;
    use std::{
        fs,
        io::Write,
        os::{
            fd::AsRawFd,
            unix::{
                ffi::OsStrExt,
                fs::{MetadataExt, PermissionsExt},
            },
        },
        time::{SystemTime, UNIX_EPOCH},
    };

    // Only the home trash on the same filesystem is supported. Missing/linked data roots,
    // unsafe trash permissions and cross-volume placement fail while the source remains.
    let data = dirs::data_dir().ok_or(io::ErrorKind::Unsupported)?;
    let data = pin_absolute(&data)?;
    let component =
        |s: &str| SafeFileName::new(s).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput));
    let trash = data.child(&component("Trash")?, true)?;
    for directory in [
        &trash,
        &trash.child(&component("files")?, true)?,
        &trash.child(&component("info")?, true)?,
    ] {
        let meta = directory.file.metadata()?;
        if meta.uid() != unsafe { libc::geteuid() } {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        directory
            .file
            .set_permissions(fs::Permissions::from_mode(0o700))?;
    }
    let files = trash.child(&component("files")?, false)?;
    let info = trash.child(&component("info")?, false)?;
    if entry.metadata()?.dev() != files.file.metadata()?.dev() {
        return Err(io::ErrorKind::Unsupported.into());
    }
    let original_parent = fs::read_link(format!("/proc/self/fd/{}", parent.file.as_raw_fd()))?;
    let original = original_parent.join(name.as_str());
    if !original.is_absolute() || original.as_os_str().as_bytes().ends_with(b" (deleted)") {
        return Err(io::ErrorKind::PermissionDenied.into());
    }
    let token = super::clock::RandomIds::default().new_id();
    let target = component(&format!("vjump-{token}"))?;
    let record = component(&format!("vjump-{token}.trashinfo"))?;
    let mut metadata = info.open(&record, true, true)?;
    let encoded = percent_encoding::percent_encode(
        original.as_os_str().as_bytes(),
        percent_encoding::NON_ALPHANUMERIC,
    );
    let date = super::clock::rfc3339_utc(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    );
    let result = (|| {
        writeln!(
            metadata,
            "[Trash Info]\nPath={encoded}\nDeletionDate={}",
            date.trim_end_matches('Z')
        )?;
        metadata.sync_all()?;
        info.file.sync_all()?;
        rename_no_replace(parent, name, entry, &files, &target)
    })();
    if result.is_err() {
        // rename_no_replace can report a directory-sync error after moving the source.
        // Keep its recovery record in that case and acknowledge the confirmed placement.
        if let Ok(placed) = files.open_entry(&target)
            && identity(&placed)?.platform_id == identity(entry)?.platform_id
        {
            return Ok(());
        }
        if let Ok(retained) = parent.open_entry(name)
            && identity(&retained)?.platform_id == identity(entry)?.platform_id
        {
            let _ = info.remove_owned_file(&record);
        }
    }
    result
}

/// Called only from a blocking worker. Failure retains the owned replacement backup.
pub fn recycle(path: &Path) -> io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        trash::delete(path).map_err(|_| io::Error::other("OS trash operation failed"))
    }
    #[cfg(windows)]
    {
        windows_recycle::recycle(path)
    }
}

#[cfg(all(test, windows))]
pub(crate) mod tests;

#[cfg(windows)]
mod windows_recycle {
    use super::*;
    use std::sync::{Arc, Mutex};
    use windows::Win32::{Foundation::E_ABORT, UI::Shell::*};
    use windows_core::{HRESULT, PCWSTR, Ref, implement};

    #[derive(Default, Clone)]
    pub(super) struct Completion {
        pub refused: bool,
        pub pre_delete: usize,
        pub post_delete: usize,
        pub failed: bool,
        pub recycled_destination: bool,
    }

    impl Completion {
        pub(super) fn check(
            &self,
            operation: windows_core::Result<()>,
            aborted: windows_core::Result<bool>,
            source: io::Result<std::fs::Metadata>,
        ) -> io::Result<()> {
            if operation.is_ok()
                && matches!(aborted, Ok(false))
                && !self.refused
                && self.pre_delete > 0
                && self.pre_delete == self.post_delete
                && !self.failed
                && self.recycled_destination
                && source.is_err_and(|error| error.kind() == io::ErrorKind::NotFound)
            {
                Ok(())
            } else {
                Err(io::Error::other("Recycle Bin did not confirm recycling"))
            }
        }
    }

    #[implement(IFileOperationProgressSink)]
    pub(super) struct RecycleSink {
        pub completion: Arc<Mutex<Completion>>,
        #[cfg(test)]
        pub refusal: Option<Arc<std::sync::atomic::AtomicBool>>,
    }

    #[allow(non_snake_case)]
    impl IFileOperationProgressSink_Impl for RecycleSink_Impl {
        fn PreDeleteItem(&self, flags: u32, _: Ref<IShellItem>) -> windows_core::Result<()> {
            let mut state = self.completion.lock().unwrap_or_else(|p| p.into_inner());
            state.pre_delete += 1;
            let refuse = flags & TSF_DELETE_RECYCLE_IF_POSSIBLE.0 as u32 == 0;
            #[cfg(test)]
            let refuse = {
                let injected = self.refusal.as_ref().is_some_and(|seen| {
                    seen.store(true, std::sync::atomic::Ordering::SeqCst);
                    true
                });
                refuse || injected
            };
            if refuse {
                state.refused = true;
                return Err(E_ABORT.into());
            }
            Ok(())
        }

        fn PostDeleteItem(
            &self,
            _: u32,
            _: Ref<IShellItem>,
            result: HRESULT,
            destination: Ref<IShellItem>,
        ) -> windows_core::Result<()> {
            let mut state = self.completion.lock().unwrap_or_else(|p| p.into_inner());
            state.post_delete += 1;
            state.failed |= result.is_err() || destination.is_none();
            state.recycled_destination |= destination.is_some();
            Ok(())
        }

        fn StartOperations(&self) -> windows_core::Result<()> {
            Ok(())
        }
        fn FinishOperations(&self, result: HRESULT) -> windows_core::Result<()> {
            self.completion
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .failed |= result.is_err();
            Ok(())
        }
        fn PreRenameItem(
            &self,
            _: u32,
            _: Ref<IShellItem>,
            _: &PCWSTR,
        ) -> windows_core::Result<()> {
            Ok(())
        }
        fn PostRenameItem(
            &self,
            _: u32,
            _: Ref<IShellItem>,
            _: &PCWSTR,
            _: HRESULT,
            _: Ref<IShellItem>,
        ) -> windows_core::Result<()> {
            Ok(())
        }
        fn PreMoveItem(
            &self,
            _: u32,
            _: Ref<IShellItem>,
            _: Ref<IShellItem>,
            _: &PCWSTR,
        ) -> windows_core::Result<()> {
            Ok(())
        }
        fn PostMoveItem(
            &self,
            _: u32,
            _: Ref<IShellItem>,
            _: Ref<IShellItem>,
            _: &PCWSTR,
            _: HRESULT,
            _: Ref<IShellItem>,
        ) -> windows_core::Result<()> {
            Ok(())
        }
        fn PreCopyItem(
            &self,
            _: u32,
            _: Ref<IShellItem>,
            _: Ref<IShellItem>,
            _: &PCWSTR,
        ) -> windows_core::Result<()> {
            Ok(())
        }
        fn PostCopyItem(
            &self,
            _: u32,
            _: Ref<IShellItem>,
            _: Ref<IShellItem>,
            _: &PCWSTR,
            _: HRESULT,
            _: Ref<IShellItem>,
        ) -> windows_core::Result<()> {
            Ok(())
        }
        fn PreNewItem(&self, _: u32, _: Ref<IShellItem>, _: &PCWSTR) -> windows_core::Result<()> {
            Ok(())
        }
        fn PostNewItem(
            &self,
            _: u32,
            _: Ref<IShellItem>,
            _: &PCWSTR,
            _: &PCWSTR,
            _: u32,
            _: HRESULT,
            _: Ref<IShellItem>,
        ) -> windows_core::Result<()> {
            Ok(())
        }
        fn UpdateProgress(&self, _: u32, _: u32) -> windows_core::Result<()> {
            Ok(())
        }
        fn ResetTimer(&self) -> windows_core::Result<()> {
            Ok(())
        }
        fn PauseTimer(&self) -> windows_core::Result<()> {
            Ok(())
        }
        fn ResumeTimer(&self) -> windows_core::Result<()> {
            Ok(())
        }
    }

    pub(super) fn shell_name(path: &Path) -> io::Result<Vec<u16>> {
        use std::{
            os::windows::ffi::OsStrExt,
            path::{Component, Prefix},
        };
        let mut name: Vec<u16> = path.as_os_str().encode_wide().collect();
        if let Some(Component::Prefix(prefix)) = path.components().next() {
            // The Shell accepts plain drive and UNC paths, not verbatim namespaces.
            match prefix.kind() {
                Prefix::VerbatimDisk(_) => {
                    name.drain(..4);
                }
                Prefix::VerbatimUNC(_, _) => {
                    name.drain(2..8);
                }
                Prefix::Verbatim(_) | Prefix::DeviceNS(_) => {
                    return Err(io::ErrorKind::Unsupported.into());
                }
                _ => {}
            }
        }
        name.push(0);
        Ok(name)
    }

    pub(super) fn recycle(path: &Path) -> io::Result<()> {
        use windows::{
            Win32::{System::Com::*, UI::Shell::*},
            core::PCWSTR,
        };
        struct ComGuard;
        impl Drop for ComGuard {
            fn drop(&mut self) {
                unsafe {
                    CoUninitialize();
                }
            }
        }
        let name = shell_name(path)?;
        unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED)
                .ok()
                .map_err(|_| io::Error::other("COM initialization failed"))?;
            let _guard = ComGuard;
            let operation: IFileOperation =
                CoCreateInstance(&FileOperation, None, CLSCTX_INPROC_SERVER)
                    .map_err(|_| io::Error::other("Recycle Bin unavailable"))?;
            operation
                .SetOperationFlags(
                    FOFX_RECYCLEONDELETE
                        | FOFX_EARLYFAILURE
                        | FOF_NOCONFIRMATION
                        | FOF_NOERRORUI
                        | FOF_SILENT,
                )
                .map_err(|_| io::Error::other("Recycle Bin flags rejected"))?;
            let item: IShellItem = SHCreateItemFromParsingName(PCWSTR(name.as_ptr()), None)
                .map_err(|_| io::Error::other("Recycle Bin item unavailable"))?;
            let completion = Arc::new(Mutex::new(Completion::default()));
            let sink: IFileOperationProgressSink = RecycleSink {
                completion: completion.clone(),
                #[cfg(test)]
                refusal: super::tests::refusal(path),
            }
            .into();
            operation
                .DeleteItem(&item, &sink)
                .map_err(|_| io::Error::other("Recycle Bin refused item"))?;
            let result = operation.PerformOperations();
            let aborted = operation
                .GetAnyOperationsAborted()
                .map(|value| value.as_bool());
            completion.lock().unwrap_or_else(|p| p.into_inner()).check(
                result,
                aborted,
                std::fs::symlink_metadata(path),
            )
        }
    }
}
