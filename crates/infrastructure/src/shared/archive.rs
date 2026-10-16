//! Hardened archive extraction (NFR-SEC-4). Hash verification of a download does not replace
//! these checks: every entry is validated, limits are enforced while reading, and the output
//! always goes to a fresh staging directory.

use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Component, Path, PathBuf},
};

use flate2::read::GzDecoder;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExtractLimits {
    pub max_entries: usize,
    pub max_total_bytes: u64,
    /// The maximum ratio of uncompressed bytes to archive bytes.
    pub max_ratio: u64,
}

impl Default for ExtractLimits {
    fn default() -> Self {
        Self {
            max_entries: 20_000,
            max_total_bytes: 2 << 30,
            max_ratio: 200,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveKind {
    Zip,
    TarGz,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ArchiveError {
    #[error("unsafe archive entry: {0}")]
    UnsafeEntry(&'static str),
    #[error("the archive has too many entries")]
    TooManyEntries,
    #[error("the archive expands beyond the size limit")]
    TooLarge,
    #[error("the archive compression ratio is implausible")]
    CompressionRatio,
    #[error("two archive entries collide on a case-insensitive file system")]
    Collision,
    #[error("the staging directory is not empty")]
    StagingNotFresh,
    #[error("the archive is damaged")]
    Corrupt,
    #[error("the archive could not be extracted: {0}")]
    Io(String),
}

impl From<io::Error> for ArchiveError {
    fn from(error: io::Error) -> Self {
        Self::Io(error.kind().to_string())
    }
}

const RESERVED: [&str; 22] = [
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// Validates one entry name and returns its normalized components.
pub fn validate_entry_name(name: &str) -> Result<Vec<String>, ArchiveError> {
    let unsafe_entry = ArchiveError::UnsafeEntry;
    if name.is_empty() || name.contains('\0') {
        return Err(unsafe_entry("empty or NUL name"));
    }
    let name = name.replace('\\', "/");
    if name.starts_with('/') {
        return Err(unsafe_entry("absolute or UNC path"));
    }
    let bytes = name.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return Err(unsafe_entry("drive-relative path"));
    }
    let mut components = Vec::new();
    for component in name.split('/') {
        if component.is_empty() || component == "." {
            continue;
        }
        if component == ".." {
            return Err(unsafe_entry("parent directory component"));
        }
        if component.len() > 255 {
            return Err(unsafe_entry("component longer than 255 bytes"));
        }
        if component.contains(':') {
            return Err(unsafe_entry("colon or alternate data stream"));
        }
        if component
            .chars()
            .any(|character| character.is_control() || "<>\"|?*".contains(character))
        {
            return Err(unsafe_entry("control or reserved character"));
        }
        if component.ends_with('.') || component.ends_with(' ') {
            return Err(unsafe_entry("trailing dot or space"));
        }
        let stem = component.split('.').next().unwrap_or(component);
        if RESERVED.contains(&stem.to_ascii_lowercase().as_str()) {
            return Err(unsafe_entry("reserved device name"));
        }
        components.push(component.to_owned());
    }
    if components.is_empty() {
        return Err(unsafe_entry("empty path"));
    }
    Ok(components)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    File,
    Dir,
}

/// Tracks entries so case-insensitive collisions and file/directory clashes are detected.
#[derive(Default)]
struct Seen(HashMap<String, Kind>);

impl Seen {
    fn add(&mut self, components: &[String], kind: Kind) -> Result<(), ArchiveError> {
        let mut path = String::new();
        for (index, component) in components.iter().enumerate() {
            if !path.is_empty() {
                path.push('/');
            }
            path.push_str(&component.to_lowercase());
            let last = index + 1 == components.len();
            let wanted = if last { kind } else { Kind::Dir };
            match self.0.get(&path) {
                Some(existing) if *existing == Kind::Dir && wanted == Kind::Dir => {}
                Some(_) => return Err(ArchiveError::Collision),
                None => {
                    self.0.insert(path.clone(), wanted);
                }
            }
        }
        Ok(())
    }
}

struct Budget {
    limits: ExtractLimits,
    archive_bytes: u64,
    entries: usize,
    total: u64,
}

impl Budget {
    fn new(limits: ExtractLimits, archive_bytes: u64) -> Self {
        Self {
            limits,
            archive_bytes: archive_bytes.max(1),
            entries: 0,
            total: 0,
        }
    }

    fn entry(&mut self) -> Result<(), ArchiveError> {
        self.entries += 1;
        if self.entries > self.limits.max_entries {
            return Err(ArchiveError::TooManyEntries);
        }
        Ok(())
    }

    fn bytes(&mut self, count: u64) -> Result<(), ArchiveError> {
        self.total = self.total.saturating_add(count);
        if self.total > self.limits.max_total_bytes {
            return Err(ArchiveError::TooLarge);
        }
        if self.total > self.archive_bytes.saturating_mul(self.limits.max_ratio) {
            return Err(ArchiveError::CompressionRatio);
        }
        Ok(())
    }
}

fn prepare_staging(destination: &Path) -> Result<PathBuf, ArchiveError> {
    fs::create_dir_all(destination)?;
    if fs::read_dir(destination)?.next().is_some() {
        return Err(ArchiveError::StagingNotFresh);
    }
    Ok(fs::canonicalize(destination)?)
}

/// Resolves the output path and verifies it stays inside the staging root after
/// canonicalizing its parent.
fn output_path(
    staging: &Path,
    components: &[String],
    create_parents: bool,
) -> Result<PathBuf, ArchiveError> {
    let mut path = staging.to_owned();
    for component in components {
        path.push(component);
    }
    if create_parents && let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let parent = path
        .parent()
        .ok_or(ArchiveError::UnsafeEntry("no parent directory"))?;
    let canonical_parent = fs::canonicalize(parent)?;
    if !canonical_parent.starts_with(staging) {
        return Err(ArchiveError::UnsafeEntry("escapes the staging directory"));
    }
    if path
        .components()
        .any(|part| matches!(part, Component::ParentDir))
    {
        return Err(ArchiveError::UnsafeEntry("parent directory component"));
    }
    Ok(path)
}

#[cfg(unix)]
fn set_mode(path: &Path, executable: bool) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(
        path,
        fs::Permissions::from_mode(if executable { 0o755 } else { 0o644 }),
    )
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _executable: bool) -> io::Result<()> {
    Ok(())
}

fn write_file(
    path: &Path,
    reader: &mut dyn Read,
    declared: u64,
    executable: bool,
    budget: &mut Budget,
) -> Result<(), ArchiveError> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    let mut limited = reader.take(declared.saturating_add(1));
    let mut buffer = [0u8; 64 * 1024];
    let mut written = 0u64;
    loop {
        let read = limited
            .read(&mut buffer)
            .map_err(|_| ArchiveError::Corrupt)?;
        if read == 0 {
            break;
        }
        written += read as u64;
        if written > declared {
            return Err(ArchiveError::Corrupt);
        }
        budget.bytes(read as u64)?;
        file.write_all(&buffer[..read])?;
    }
    file.flush()?;
    drop(file);
    set_mode(path, executable)?;
    Ok(())
}

/// Extracts `archive` into the fresh directory `destination`. On any error the caller deletes
/// the staging directory; nothing outside it is ever written.
pub fn extract(
    archive: &Path,
    kind: ArchiveKind,
    destination: &Path,
    limits: ExtractLimits,
) -> Result<Vec<PathBuf>, ArchiveError> {
    let archive_bytes = fs::metadata(archive)?.len();
    let staging = prepare_staging(destination)?;
    let budget = Budget::new(limits, archive_bytes);
    match kind {
        ArchiveKind::Zip => extract_zip(archive, &staging, budget),
        ArchiveKind::TarGz => extract_tar_gz(archive, &staging, budget),
    }
}

struct PlannedEntry {
    index: usize,
    components: Vec<String>,
    is_dir: bool,
    executable: bool,
    size: u64,
}

fn extract_zip(
    archive: &Path,
    staging: &Path,
    mut budget: Budget,
) -> Result<Vec<PathBuf>, ArchiveError> {
    let mut zip = zip::ZipArchive::new(File::open(archive)?).map_err(|_| ArchiveError::Corrupt)?;
    if zip.len() > budget.limits.max_entries {
        return Err(ArchiveError::TooManyEntries);
    }
    // Validate every entry before anything is written: names, types, collisions and the
    // declared sizes against the limits.
    let mut seen = Seen::default();
    let mut planned = Vec::with_capacity(zip.len());
    let mut declared_total = 0u64;
    let mut compressed_total = 0u64;
    for index in 0..zip.len() {
        let entry = zip.by_index_raw(index).map_err(|_| ArchiveError::Corrupt)?;
        if entry.encrypted() {
            return Err(ArchiveError::UnsafeEntry("encrypted entry"));
        }
        if entry.is_symlink() {
            return Err(ArchiveError::UnsafeEntry("symbolic link"));
        }
        if entry.external_attributes() & 0x400 != 0 {
            return Err(ArchiveError::UnsafeEntry("reparse point"));
        }
        if entry.name_raw().contains(&0) {
            return Err(ArchiveError::UnsafeEntry("NUL name"));
        }
        if entry.size()
            > entry
                .compressed_size()
                .saturating_mul(budget.limits.max_ratio)
        {
            return Err(ArchiveError::CompressionRatio);
        }
        let mode = entry.unix_mode();
        if let Some(mode) = mode
            && !matches!(mode & 0o170_000, 0 | 0o100_000 | 0o040_000)
        {
            return Err(ArchiveError::UnsafeEntry("special file"));
        }
        let name = entry
            .name()
            .map_err(|_| ArchiveError::UnsafeEntry("invalid entry name"))?;
        let components = validate_entry_name(&name)?;
        let is_dir = entry.is_dir();
        seen.add(&components, if is_dir { Kind::Dir } else { Kind::File })?;
        declared_total = declared_total.saturating_add(entry.size());
        compressed_total = compressed_total.saturating_add(entry.compressed_size());
        planned.push(PlannedEntry {
            index,
            components,
            is_dir,
            executable: mode.is_some_and(|mode| mode & 0o111 != 0),
            size: entry.size(),
        });
    }
    if declared_total > budget.limits.max_total_bytes {
        return Err(ArchiveError::TooLarge);
    }
    if declared_total > compressed_total.saturating_mul(budget.limits.max_ratio) {
        return Err(ArchiveError::CompressionRatio);
    }
    // Opening each decoded entry checks method support before any output is written.
    // The raw metadata pass above intentionally runs first (including encryption checks).
    for entry in &planned {
        zip.by_index(entry.index)
            .map_err(|_| ArchiveError::Corrupt)?;
    }

    let mut files = Vec::new();
    for entry in planned {
        budget.entry()?;
        let path = output_path(staging, &entry.components, true)?;
        if entry.is_dir {
            fs::create_dir_all(&path)?;
            continue;
        }
        let mut file = zip
            .by_index(entry.index)
            .map_err(|_| ArchiveError::Corrupt)?;
        write_file(&path, &mut file, entry.size, entry.executable, &mut budget)?;
        files.push(entry.components.iter().collect());
    }
    Ok(files)
}

fn extract_tar_gz(
    archive: &Path,
    staging: &Path,
    mut budget: Budget,
) -> Result<Vec<PathBuf>, ArchiveError> {
    let mut tar = tar::Archive::new(GzDecoder::new(File::open(archive)?));
    let mut seen = Seen::default();
    let mut files = Vec::new();
    for entry in tar.entries().map_err(|_| ArchiveError::Corrupt)? {
        let mut entry = entry.map_err(|_| ArchiveError::Corrupt)?;
        budget.entry()?;
        let entry_type = entry.header().entry_type();
        if entry_type.is_pax_global_extensions() || entry_type.is_pax_local_extensions() {
            continue;
        }
        let is_dir = entry_type.is_dir();
        if !(is_dir || entry_type.is_file()) {
            return Err(ArchiveError::UnsafeEntry(
                "link, device, fifo or sparse entry",
            ));
        }
        let name = std::str::from_utf8(&entry.path_bytes())
            .map_err(|_| ArchiveError::UnsafeEntry("non UTF-8 name"))?
            .to_owned();
        let components = validate_entry_name(&name)?;
        if is_dir {
            seen.add(&components, Kind::Dir)?;
            fs::create_dir_all(output_path(staging, &components, true)?)?;
            continue;
        }
        seen.add(&components, Kind::File)?;
        let path = output_path(staging, &components, true)?;
        let executable = entry.header().mode().is_ok_and(|mode| mode & 0o111 != 0);
        let declared = entry.header().size().map_err(|_| ArchiveError::Corrupt)?;
        write_file(&path, &mut entry, declared, executable, &mut budget)?;
        files.push(components.iter().collect());
    }
    Ok(files)
}

#[cfg(test)]
mod tests;
