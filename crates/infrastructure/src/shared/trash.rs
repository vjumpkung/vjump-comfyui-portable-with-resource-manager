use std::{io, path::Path};
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
