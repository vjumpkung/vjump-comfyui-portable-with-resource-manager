//! Authorized roots and pinned, non-following parent traversal shared with M4.
use domain::{
    models::acquisition::FileIdentity,
    shared::model_values::{RelativeModelPath, SafeFileName, Sha256},
};
use sha2::{Digest, Sha256 as Hasher};
use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::Arc,
    time::UNIX_EPOCH,
};

pub struct AuthorizedModelRoots {
    roots: HashMap<String, Arc<PinnedDirectory>>,
}
pub struct PinnedDirectory {
    pub path: PathBuf,
    pub file: File,
    _parents: Vec<Arc<PinnedDirectory>>,
}
impl AuthorizedModelRoots {
    pub fn new(roots: impl IntoIterator<Item = (String, PathBuf)>) -> io::Result<Self> {
        let mut authorized = HashMap::new();
        for (id, path) in roots {
            if id.is_empty() || authorized.contains_key(&id) {
                return Err(io::Error::from(io::ErrorKind::InvalidInput));
            }
            let canonical = fs::canonicalize(path)?;
            let root = pin_absolute(&canonical)?;
            authorized.insert(id, root);
        }
        Ok(Self { roots: authorized })
    }
    pub fn parent(
        &self,
        root_id: &str,
        path: &RelativeModelPath,
        create: bool,
    ) -> io::Result<(Arc<PinnedDirectory>, SafeFileName)> {
        let mut directory = self
            .roots
            .get(root_id)
            .cloned()
            .ok_or(io::ErrorKind::PermissionDenied)?;
        let (name, parents) = path
            .components()
            .split_last()
            .ok_or(io::ErrorKind::InvalidInput)?;
        for component in parents {
            directory = directory.child(component, create)?;
        }
        Ok((directory, name.clone()))
    }
}
pub fn pin_absolute(path: &Path) -> io::Result<Arc<PinnedDirectory>> {
    // On Windows, pin the complete chain with delete sharing denied. Unix mutations use dirfds.
    #[cfg(windows)]
    {
        let mut chain = Vec::new();
        for ancestor in path.ancestors().collect::<Vec<_>>().into_iter().rev() {
            if ancestor.as_os_str().is_empty() {
                continue;
            }
            let file = open_directory(ancestor)?;
            let pin = Arc::new(PinnedDirectory {
                path: ancestor.to_owned(),
                file,
                _parents: chain.clone(),
            });
            chain.push(pin);
        }
        chain
            .pop()
            .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))
    }
    #[cfg(unix)]
    {
        Ok(Arc::new(PinnedDirectory {
            path: path.to_owned(),
            file: open_directory(path)?,
            _parents: Vec::new(),
        }))
    }
}
fn open_directory(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::*;
        options
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    let file = options.open(path)?;
    reject_reparse(&file)?;
    if !file.metadata()?.is_dir() {
        return Err(io::ErrorKind::NotADirectory.into());
    }
    Ok(file)
}
impl PinnedDirectory {
    pub fn child(self: &Arc<Self>, name: &SafeFileName, create: bool) -> io::Result<Arc<Self>> {
        let path = self.path.join(name.as_str());
        #[cfg(windows)]
        let file = {
            if create {
                match fs::create_dir(&path) {
                    Ok(()) => {}
                    Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(e) => return Err(e),
                }
            }
            open_directory(&path)?
        };
        #[cfg(unix)]
        let file = {
            use std::os::fd::{AsRawFd, FromRawFd};
            let name = std::ffi::CString::new(name.as_str())?;
            if create {
                let result = unsafe { libc::mkdirat(self.file.as_raw_fd(), name.as_ptr(), 0o755) };
                if result < 0 && io::Error::last_os_error().kind() != io::ErrorKind::AlreadyExists {
                    return Err(io::Error::last_os_error());
                }
            }
            let fd = unsafe {
                libc::openat(
                    self.file.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                )
            };
            if fd < 0 {
                return Err(io::Error::last_os_error());
            }
            unsafe { File::from_raw_fd(fd) }
        };
        Ok(Arc::new(Self {
            path,
            file,
            _parents: vec![self.clone()],
        }))
    }
    pub fn open(&self, name: &SafeFileName, write: bool, create_new: bool) -> io::Result<File> {
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            use windows_sys::Win32::Storage::FileSystem::*;
            let mut o = OpenOptions::new();
            o.read(true)
                .write(write)
                .create_new(create_new)
                .access_mode(if write {
                    0x80000000 | 0x40000000 | 0x00010000
                } else {
                    0x80000000 | 0x00010000
                })
                .share_mode(FILE_SHARE_READ | FILE_SHARE_DELETE)
                .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
            let file = o.open(self.path.join(name.as_str()))?;
            reject_reparse(&file)?;
            if !file.metadata()?.is_file() {
                return Err(io::ErrorKind::InvalidInput.into());
            }
            Ok(file)
        }
        #[cfg(unix)]
        {
            use std::os::fd::{AsRawFd, FromRawFd};
            let name = std::ffi::CString::new(name.as_str())?;
            let flags = if write { libc::O_RDWR } else { libc::O_RDONLY }
                | libc::O_NOFOLLOW
                | libc::O_CLOEXEC
                | if create_new {
                    libc::O_CREAT | libc::O_EXCL
                } else {
                    0
                };
            let fd = unsafe { libc::openat(self.file.as_raw_fd(), name.as_ptr(), flags, 0o600) };
            if fd < 0 {
                return Err(io::Error::last_os_error());
            }
            let file = unsafe { File::from_raw_fd(fd) };
            if !file.metadata()?.is_file() {
                return Err(io::ErrorKind::InvalidInput.into());
            }
            Ok(file)
        }
    }
    pub fn remove_owned_file(&self, name: &SafeFileName) -> io::Result<()> {
        #[cfg(windows)]
        {
            fs::remove_file(self.path.join(name.as_str()))
        }
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            let name = std::ffi::CString::new(name.as_str())?;
            if unsafe { libc::unlinkat(self.file.as_raw_fd(), name.as_ptr(), 0) } < 0 {
                Err(io::Error::last_os_error())
            } else {
                self.file.sync_all()
            }
        }
    }
}
pub fn reject_reparse(file: &File) -> io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if file.metadata()?.file_attributes() & 0x400 != 0 {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
    }
    let _ = file;
    Ok(())
}
pub fn identity(file: &File) -> io::Result<FileIdentity> {
    let meta = file.metadata()?;
    #[cfg(unix)]
    let platform_id = {
        use std::os::unix::fs::MetadataExt;
        format!("{}:{}", meta.dev(), meta.ino())
    };
    #[cfg(windows)]
    let platform_id = {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::*;
        let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
            return Err(io::Error::last_os_error());
        }
        format!(
            "{}:{}:{}",
            info.dwVolumeSerialNumber, info.nFileIndexHigh, info.nFileIndexLow
        )
    };
    Ok(FileIdentity {
        size: meta.len(),
        modified_ns: meta
            .modified()?
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        platform_id,
        digest: None,
    })
}
pub fn digest(file: &mut File) -> io::Result<Sha256> {
    file.seek(SeekFrom::Start(0))?;
    let mut hasher = Hasher::new();
    let mut buffer = vec![0; 1024 * 1024];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Sha256::new(&hex(&hasher.finalize())).map_err(|_| io::ErrorKind::InvalidData.into())
}
pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut result, "{byte:02x}").expect("String writing cannot fail");
    }
    result
}
/// Rename the opened source without replacing a destination. Parents remain pinned.
pub fn rename_no_replace(
    source: &PinnedDirectory,
    name: &SafeFileName,
    file: &File,
    target: &PinnedDirectory,
    new_name: &SafeFileName,
) -> io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::{ffi::OsStrExt, io::AsRawHandle};
        use windows_sys::Win32::Storage::FileSystem::*;
        // Parent directories stay pinned (delete sharing denied), so a full NT path with a
        // null RootDirectory is equivalent and is the form Windows accepts for this call.
        let full = target.path.join(new_name.as_str());
        let text = full.as_os_str().to_string_lossy();
        // SetFileInformationByHandle re-prefixes the name itself; hand it a plain drive path.
        let plain = text.strip_prefix(r"\\?\").unwrap_or(&text);
        let wide: Vec<u16> = std::ffi::OsStr::new(plain).encode_wide().collect();
        let offset = std::mem::offset_of!(FILE_RENAME_INFO, FileName);
        // The name must stay NUL-terminated: Windows converts it as a DOS path.
        let bytes = offset + (wide.len() + 1) * 2;
        let mut storage = vec![0u64; bytes.div_ceil(8)];
        let info = storage.as_mut_ptr().cast::<FILE_RENAME_INFO>();
        unsafe {
            (*info).Anonymous.ReplaceIfExists = false;
            (*info).RootDirectory = std::ptr::null_mut();
            (*info).FileNameLength = (wide.len() * 2) as u32;
            std::ptr::copy_nonoverlapping(wide.as_ptr(), (*info).FileName.as_mut_ptr(), wide.len());
            if SetFileInformationByHandle(
                file.as_raw_handle(),
                FileRenameInfo,
                info.cast(),
                bytes as u32,
            ) == 0
            {
                return Err(io::Error::last_os_error());
            }
        }
        let _ = (source, name);
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::fd::AsRawFd;
        let a = std::ffi::CString::new(name.as_str())?;
        let b = std::ffi::CString::new(new_name.as_str())?;
        // Identity is checked against the open handle immediately before the dirfd operation.
        if identity(file)? != identity(&source.open(name, false, false)?)? {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        if unsafe {
            libc::renameat2(
                source.file.as_raw_fd(),
                a.as_ptr(),
                target.file.as_raw_fd(),
                b.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        } < 0
        {
            return Err(io::Error::last_os_error());
        }
        target.file.sync_all()?;
        source.file.sync_all()?;
    }
    Ok(())
}
