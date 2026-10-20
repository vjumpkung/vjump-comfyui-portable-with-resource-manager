//! The writer side of `extra_model_paths.yaml`. The app owns exactly one section; every other
//! section of an existing file is kept as data. The reader in the parent module parses what is
//! written here, so the two cannot drift apart unnoticed.
use super::{YamlExtraModelPaths, invalid};
use crate::{
    environment::layout::Layout,
    shared::{
        clock::rfc3339_utc,
        fs::{atomic_write, nearest_existing},
        model_paths::reject_reparse,
    },
};
use application::comfy_runtime::{ExtraModelPathsWriter, RuntimeError};
use async_trait::async_trait;
use domain::shared::model_values::SafeFileName;
use serde_yaml_ng::{Mapping, Value};
use std::{
    collections::BTreeMap,
    fs,
    io::{self, Write},
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

/// The top-level key of the section this app writes.
pub const APP_SECTION: &str = "vjumpkung_desktop";
const FILE_NAME: &str = "extra_model_paths.yaml";
const HEADER: &str = "# Managed by vjumpkung ComfyUI desktop. Other sections are kept; the vjumpkung_desktop section is rewritten by the app.";
const MAX_BYTES: u64 = 1 << 20;
/// REQUIREMENTS section 4.6 primary names; these folders are created under the base.
const PRIMARY: [&str; 18] = [
    "checkpoints",
    "diffusion_models",
    "text_encoders",
    "vae",
    "loras",
    "controlnet",
    "clip_vision",
    "embeddings",
    "upscale_models",
    "latent_upscale_models",
    "style_models",
    "hypernetworks",
    "photomaker",
    "gligen",
    "audio_encoders",
    "model_patches",
    "vae_approx",
    "diffusers",
];

const NOT_ABSOLUTE: &str = "Choose an absolute folder path on a local drive or network share.";
const EXPANDED: &str = "The folder path must not contain $ or % and must not start with ~, because ComfyUI expands them.";
const INSIDE_APP: &str = "Choose a folder outside the app's ComfyUI, envs, data and tools folders.";
const LINKED: &str =
    "The folder or one of its parents is a link or junction. Choose the real folder.";
const NOT_INSTALLED: &str = "ComfyUI is not installed yet.";
const NOT_A_FOLDER: &str = "The chosen path is a file or lies inside one. Choose a folder.";

#[async_trait]
impl ExtraModelPathsWriter for YamlExtraModelPaths {
    async fn generate(&self, root: &Path, base: &Path) -> Result<(), RuntimeError> {
        let (root, base) = (root.to_owned(), base.to_owned());
        blocking(move || generate(&root, &base)).await
    }

    async fn remove(&self, root: &Path) -> Result<(), RuntimeError> {
        let root = root.to_owned();
        blocking(move || remove(&root)).await
    }
}

async fn blocking(
    work: impl FnOnce() -> Result<(), RuntimeError> + Send + 'static,
) -> Result<(), RuntimeError> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|_| invalid("the update was interrupted"))?
}

fn rejected(message: impl Into<String>) -> RuntimeError {
    RuntimeError::ExtraPathsRejected(message.into())
}

fn unsafe_file(reason: &str) -> RuntimeError {
    rejected(format!(
        "{FILE_NAME} could not be updated safely: {reason}. Edit or move it, then retry."
    ))
}

pub(super) fn generate(root: &Path, base: &Path) -> Result<(), RuntimeError> {
    let base = checked_base(base)?;
    let layout = Layout::new(root);
    let comfyui = layout.comfyui_dir();
    if !comfyui.is_dir() {
        return Err(rejected(NOT_INSTALLED));
    }
    outside_app_folders(&layout, &base)?;
    reject_links(&base)?;
    let path = comfyui.join(FILE_NAME);
    // Read and validate the existing file before anything on disk is created.
    let existing = load(&path)?;
    create_folders(&base)?;
    let mut sections = existing
        .as_ref()
        .map_or_else(Mapping::new, |file| file.sections.clone());
    sections.insert(
        Value::String(APP_SECTION.into()),
        Value::Mapping(app_section(&base, &comfyui.join("models"))?),
    );
    if let Some(file) = existing.filter(|file| !file.managed) {
        back_up(root, &file.bytes)?;
    }
    write(&path, &sections)
}

pub(super) fn remove(root: &Path) -> Result<(), RuntimeError> {
    let path = Layout::new(root).comfyui_dir().join(FILE_NAME);
    let Some(mut file) = load(&path)? else {
        return Ok(());
    };
    if file.sections.shift_remove(APP_SECTION).is_none() {
        return Ok(());
    }
    if !file.managed {
        back_up(root, &file.bytes)?;
    }
    if file.sections.is_empty() {
        fs::remove_file(&path).map_err(|_| invalid("the configuration could not be deleted"))
    } else {
        write(&path, &file.sections)
    }
}

/// Syntax checks that need no filesystem access; returns the path without trailing separators.
fn checked_base(base: &Path) -> Result<PathBuf, RuntimeError> {
    let text = base.to_str().ok_or_else(|| rejected(NOT_ABSOLUTE))?;
    if text.chars().any(char::is_control) {
        return Err(rejected(NOT_ABSOLUTE));
    }
    if text.contains(['$', '%']) || text.starts_with('~') {
        return Err(rejected(EXPANDED));
    }
    let slashed = text.replace('\\', "/");
    if slashed.starts_with("//?/") || slashed.starts_with("//./") {
        return Err(rejected(NOT_ABSOLUTE));
    }
    // `..` would let a lexical comparison with the app folders be bypassed.
    if !base.is_absolute()
        || base
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
    {
        return Err(rejected(NOT_ABSOLUTE));
    }
    Ok(base.components().collect())
}

fn folded(path: &Path) -> Vec<String> {
    path.components()
        .map(|part| {
            let text = part.as_os_str().to_string_lossy();
            if cfg!(windows) {
                text.to_lowercase()
            } else {
                text.into_owned()
            }
        })
        .collect()
}

/// The base must be neither the install root nor inside its ComfyUI, envs, data or tools
/// folders. The nearest existing ancestor is canonicalized so links and short names cannot hide
/// the overlap.
fn outside_app_folders(layout: &Layout, base: &Path) -> Result<(), RuntimeError> {
    let inspect = || invalid("the install root could not be inspected");
    let root = fs::canonicalize(&layout.root).map_err(|_| inspect())?;
    let existing = nearest_existing(base).ok_or_else(|| rejected(NOT_ABSOLUTE))?;
    let tail = base.strip_prefix(existing).unwrap_or(Path::new(""));
    let effective = folded(
        &fs::canonicalize(existing)
            .map_err(|_| inspect())?
            .join(tail),
    );
    if effective == folded(&root) {
        return Err(rejected(INSIDE_APP));
    }
    for name in ["ComfyUI", "envs", "data", "tools"] {
        let folder = fs::canonicalize(layout.root.join(name)).unwrap_or_else(|_| root.join(name));
        if effective.starts_with(&folded(&folder)) {
            return Err(rejected(INSIDE_APP));
        }
    }
    Ok(())
}

/// Rejects a link, junction or other reparse point among the existing folders of the base.
fn reject_links(base: &Path) -> Result<(), RuntimeError> {
    let existing = nearest_existing(base).ok_or_else(|| rejected(NOT_ABSOLUTE))?;
    if !existing.is_dir() {
        return Err(rejected(NOT_A_FOLDER));
    }
    for folder in existing.ancestors().filter(|f| !f.as_os_str().is_empty()) {
        match is_link(folder) {
            Ok(false) => {}
            Ok(true) => return Err(rejected(LINKED)),
            Err(_) => return Err(invalid("the model base folder could not be inspected")),
        }
    }
    Ok(())
}

#[cfg(windows)]
fn is_link(path: &Path) -> io::Result<bool> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE,
        FILE_SHARE_READ, FILE_SHARE_WRITE,
    };
    // Opening the reparse point itself (not its target) lets `reject_reparse` see it.
    let file = fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    Ok(reject_reparse(&file).is_err())
}

#[cfg(not(windows))]
fn is_link(path: &Path) -> io::Result<bool> {
    let _ = reject_reparse;
    Ok(fs::symlink_metadata(path)?.file_type().is_symlink())
}

/// An existing `extra_model_paths.yaml` that is safe to rewrite.
struct Existing {
    bytes: Vec<u8>,
    sections: Mapping,
    /// The first line is the app's header: the app wrote this file last.
    managed: bool,
}

fn load(path: &Path) -> Result<Option<Existing>, RuntimeError> {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(unsafe_file("it could not be read")),
    };
    if !meta.is_file() || is_link(path).unwrap_or(true) {
        return Err(unsafe_file("it is a link or not a regular file"));
    }
    if meta.len() > MAX_BYTES {
        return Err(unsafe_file("it is larger than 1 MiB"));
    }
    let bytes = fs::read(path).map_err(|_| unsafe_file("it could not be read"))?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(unsafe_file("it is larger than 1 MiB"));
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| unsafe_file("it is not valid UTF-8"))?;
    let document: Value =
        serde_yaml_ng::from_str(text).map_err(|_| unsafe_file("it is not valid YAML"))?;
    let sections = match document {
        Value::Null => Mapping::new(),
        Value::Mapping(sections) => sections,
        _ => return Err(unsafe_file("its top level is not a mapping of sections")),
    };
    let merges = sections.contains_key("<<")
        || sections
            .values()
            .any(|section| section.as_mapping().is_some_and(|m| m.contains_key("<<")));
    if merges {
        return Err(unsafe_file("it uses a YAML merge key (<<)"));
    }
    let managed = text.lines().next() == Some(HEADER);
    Ok(Some(Existing {
        bytes,
        sections,
        managed,
    }))
}

/// Creates the base and the missing primary folders. An existing folder (or link) is left as it
/// is; a file with a category's name refuses the whole operation before anything is created.
fn create_folders(base: &Path) -> Result<(), RuntimeError> {
    for name in PRIMARY {
        if fs::symlink_metadata(base.join(name)).is_ok_and(|meta| meta.is_file()) {
            return Err(rejected(format!(
                "A file named {name} exists in the model base folder. Move or rename it, then retry."
            )));
        }
    }
    fs::create_dir_all(base).map_err(|_| invalid("the model base folder could not be created"))?;
    for name in PRIMARY {
        match fs::create_dir(base.join(name)) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(_) => return Err(invalid("a model folder could not be created")),
        }
    }
    Ok(())
}

/// The section: `base_path`, `is_default: false`, then one sorted key per folder name. The names
/// are the primary categories, `configs` and every folder that already exists in the install's
/// `ComfyUI/models`, without duplicates that differ only in case.
fn app_section(base: &Path, models: &Path) -> Result<Mapping, RuntimeError> {
    let base_text = base.to_str().ok_or_else(|| rejected(NOT_ABSOLUTE))?;
    let mut names: BTreeMap<String, String> = PRIMARY
        .iter()
        .chain(&["configs"])
        .map(|name| ((*name).to_owned(), (*name).to_owned()))
        .collect();
    for name in model_folders(models) {
        names.entry(name.to_lowercase()).or_insert(name);
    }
    let mut section = Mapping::new();
    section.insert("base_path".into(), Value::String(base_text.to_owned()));
    section.insert("is_default".into(), Value::Bool(false));
    for name in names.into_values() {
        // The legacy alias is listed only when it exists, as ComfyUI would otherwise warn.
        let alias = match name.as_str() {
            "text_encoders" => Some("clip"),
            "diffusion_models" => Some("unet"),
            _ => None,
        }
        .filter(|alias| base.join(alias).is_dir());
        let value = match alias {
            Some(alias) => format!("{name}/\n{alias}/"),
            None => format!("{name}/"),
        };
        section.insert(Value::String(name), Value::String(value));
    }
    Ok(section)
}

/// Folders of `ComfyUI/models` that are valid names, are not hidden and are not a legacy alias.
fn model_folders(models: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(models) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let usable = !name.starts_with('.')
                && SafeFileName::new(name.as_str()).is_ok()
                && !matches!(
                    name.to_lowercase().as_str(),
                    "clip" | "unet" | "base_path" | "is_default"
                )
                && entry.path().is_dir();
            usable.then_some(name)
        })
        .collect();
    names.sort();
    names
}

fn write(path: &Path, sections: &Mapping) -> Result<(), RuntimeError> {
    let body = serde_yaml_ng::to_string(&Value::Mapping(sections.clone()))
        .map_err(|_| invalid("the configuration could not be serialized"))?;
    atomic_write(path, format!("{HEADER}\n{body}").as_bytes())
        .map_err(|_| invalid("the configuration could not be written"))
}

/// Copies the previous file to `<root>/data/backups`, never beside the original: the ComfyUI
/// checkout ignores only `extra_model_paths.yaml`, so a sibling would block app updates.
fn back_up(root: &Path, bytes: &[u8]) -> Result<(), RuntimeError> {
    let failed = || invalid("the previous file could not be backed up");
    let directory = Layout::new(root).data_dir().join("backups");
    fs::create_dir_all(&directory).map_err(|_| failed())?;
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    let stamp = rfc3339_utc(seconds).replace(['-', ':'], "");
    for attempt in 0..100 {
        let name = if attempt == 0 {
            format!("extra_model_paths-{stamp}.yaml")
        } else {
            format!("extra_model_paths-{stamp}-{attempt}.yaml")
        };
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(directory.join(name))
        {
            Ok(mut file) => {
                file.write_all(bytes).map_err(|_| failed())?;
                return file.sync_all().map_err(|_| failed());
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(_) => return Err(failed()),
        }
    }
    Err(failed())
}

#[cfg(test)]
mod tests;
