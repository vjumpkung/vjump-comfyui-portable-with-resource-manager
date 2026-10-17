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
        use std::os::windows::ffi::OsStrExt;
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
            let name: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
            let item: IShellItem = SHCreateItemFromParsingName(PCWSTR(name.as_ptr()), None)
                .map_err(|_| io::Error::other("Recycle Bin item unavailable"))?;
            operation
                .DeleteItem(&item, None)
                .map_err(|_| io::Error::other("Recycle Bin refused item"))?;
            operation
                .PerformOperations()
                .map_err(|_| io::Error::other("Recycle Bin operation failed"))?;
            if operation
                .GetAnyOperationsAborted()
                .map_err(|_| io::Error::other("Recycle Bin status unavailable"))?
                .as_bool()
                || path.exists()
            {
                return Err(io::Error::other("Recycle Bin did not recycle file"));
            }
        }
        Ok(())
    }
}
