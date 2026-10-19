//! Recoverable publication into the directories retained by startup adapters.
use super::*;
use crate::shared::model_paths::{
    PinnedDirectory, identity, pin_absolute, reject_reparse, rename_no_replace,
};
use domain::shared::model_values::SafeFileName;
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::{self, Read, Write},
    sync::Arc,
};

pub(super) const CONFLICT: &str = "The clone could not be published because the ComfyUI folder contains entries that conflict with the checkout. It was left unchanged.";
pub(super) const INTERRUPTED: &str = "A previous ComfyUI publication was interrupted and could not be undone safely. The ComfyUI folder and the .ComfyUI.cloning folder were left for inspection.";
const MAX_DEPTH: usize = 64;
const MAX_ENTRIES: usize = 4096;
const MAX_RECORD_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Kind {
    File,
    Dir,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Identity {
    platform_id: String,
    size: u64,
    modified_ns: u128,
}

impl Identity {
    fn read(file: &File) -> io::Result<Self> {
        reject_reparse(file)?;
        let id = identity(file)?;
        Ok(Self {
            platform_id: id.platform_id,
            size: id.size,
            modified_ns: id.modified_ns,
        })
    }

    fn matches(&self, file: &File, kind: Kind) -> io::Result<bool> {
        let actual = Self::read(file)?;
        let meta = file.metadata()?;
        Ok(match kind {
            Kind::Dir => meta.is_dir() && self.platform_id == actual.platform_id,
            Kind::File => meta.is_file() && *self == actual,
        })
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    path: Vec<String>,
    kind: Kind,
    identity: Identity,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Record {
    version: u32,
    destination: String,
    stage: String,
    commit: String,
    stage_identity: Identity,
    moves: Vec<Entry>,
    scaffold: Vec<Entry>,
}

pub(super) struct Publication {
    parent: Arc<PinnedDirectory>,
    destination: SafeFileName,
    stage: SafeFileName,
    record_name: SafeFileName,
}

fn invalid() -> io::Error {
    io::ErrorKind::InvalidData.into()
}

fn name(text: &str) -> io::Result<SafeFileName> {
    SafeFileName::new(text).map_err(|_| invalid())
}

fn key(path: &[String]) -> Vec<String> {
    path.iter()
        .map(|part| {
            if cfg!(windows) {
                part.to_lowercase()
            } else {
                part.clone()
            }
        })
        .collect()
}

fn protected(path: &[String]) -> bool {
    path.len() > 3
        && key(&path[..3])
            == key(&[
                "custom_nodes".into(),
                ".disabled".into(),
                ".vjump-no-hooks".into(),
            ])
}

fn child(root: &Arc<PinnedDirectory>, path: &[String]) -> io::Result<Arc<PinnedDirectory>> {
    let mut directory = root.clone();
    for part in path {
        directory = directory.child(&name(part)?, false)?;
    }
    Ok(directory)
}

fn location(
    root: &Arc<PinnedDirectory>,
    path: &[String],
) -> io::Result<(Arc<PinnedDirectory>, SafeFileName)> {
    let (last, parents) = path.split_last().ok_or_else(invalid)?;
    Ok((child(root, parents)?, name(last)?))
}

fn optional(directory: &PinnedDirectory, name: &SafeFileName) -> io::Result<Option<File>> {
    match directory.open_entry(name) {
        Ok(file) => Ok(Some(file)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// Every traversed component is opened without following links; the walk and record are bounded.
fn inventory(root: &Arc<PinnedDirectory>, scaffold: bool) -> io::Result<Vec<Entry>> {
    fn walk(
        directory: &Arc<PinnedDirectory>,
        path: &mut Vec<String>,
        entries: &mut Vec<Entry>,
        scaffold: bool,
    ) -> io::Result<()> {
        let mut names = Vec::new();
        for entry in fs::read_dir(directory.anchored_path())? {
            if entries.len() + names.len() >= MAX_ENTRIES {
                return Err(invalid());
            }
            let text = entry?.file_name().into_string().map_err(|_| invalid())?;
            names.push(name(&text)?);
        }
        names.sort();
        for component in names {
            if entries.len() >= MAX_ENTRIES || path.len() >= MAX_DEPTH {
                return Err(invalid());
            }
            if scaffold && component.as_str().eq_ignore_ascii_case(".git") {
                return Err(invalid());
            }
            // Directory pins deny delete sharing on Windows. Open them directly rather than
            // overlapping a DELETE-capable open_entry handle with a directory pin.
            let nested = directory.child(&component, false).ok();
            let (kind, id) = if let Some(nested) = &nested {
                (Kind::Dir, Identity::read(&nested.file)?)
            } else {
                let file = directory.open_entry(&component)?;
                if !file.metadata()?.is_file() {
                    return Err(invalid());
                }
                (Kind::File, Identity::read(&file)?)
            };
            if scaffold && kind != Kind::Dir {
                return Err(invalid());
            }
            path.push(component.as_str().to_owned());
            if let Some(nested) = nested {
                entries.push(Entry {
                    path: path.clone(),
                    kind,
                    identity: id,
                });
                walk(&nested, path, entries, scaffold)?;
            } else {
                entries.push(Entry {
                    path: path.clone(),
                    kind,
                    identity: id,
                });
            }
            path.pop();
        }
        Ok(())
    }
    let mut entries = Vec::new();
    walk(root, &mut Vec::new(), &mut entries, scaffold)?;
    Ok(entries)
}

pub(super) fn is_scaffold(directory: &Arc<PinnedDirectory>) -> bool {
    inventory(directory, true).is_ok()
}

pub(super) fn real_directory(meta: &fs::Metadata) -> bool {
    if !meta.is_dir() || meta.file_type().is_symlink() {
        return false;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            return false;
        }
    }
    true
}

impl Publication {
    pub(super) fn open(final_dir: &Path, stage_name: &str) -> io::Result<Self> {
        let parent = pin_absolute(final_dir.parent().ok_or_else(invalid)?)?;
        let destination = name(
            final_dir
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or_else(invalid)?,
        )?;
        Ok(Self {
            parent,
            destination,
            stage: name(&format!(".{stage_name}.cloning"))?,
            record_name: name(&format!(".{stage_name}.publish.json"))?,
        })
    }

    fn load(&self) -> io::Result<Option<Record>> {
        let mut file = match self.parent.open(&self.record_name, false, false) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let mut bytes = Vec::new();
        Read::by_ref(&mut file)
            .take(MAX_RECORD_BYTES + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_RECORD_BYTES {
            return Err(invalid());
        }
        let record: Record = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
        self.validate(&record)?;
        Ok(Some(record))
    }

    fn validate(&self, record: &Record) -> io::Result<()> {
        if record.version != 1
            || record.destination != self.destination.as_str()
            || record.stage != self.stage.as_str()
            || !(40..=64).contains(&record.commit.len())
            || !record.commit.bytes().all(|b| b.is_ascii_hexdigit())
            || record.moves.is_empty()
            || record.moves.len() > MAX_ENTRIES
            || record.scaffold.is_empty()
            || record.scaffold.len() > MAX_ENTRIES + 1
            || record.scaffold[0].path != Vec::<String>::new()
            || record
                .moves
                .last()
                .is_none_or(|entry| entry.path != [".git"] || entry.kind != Kind::Dir)
        {
            return Err(invalid());
        }
        let mut scaffolds = HashSet::new();
        for entry in &record.scaffold {
            if entry.kind != Kind::Dir
                || entry.path.iter().any(|p| p.eq_ignore_ascii_case(".git"))
                || !scaffolds.insert(key(&entry.path))
                || (!entry.path.is_empty()
                    && !scaffolds.contains(&key(&entry.path[..entry.path.len() - 1])))
            {
                return Err(invalid());
            }
        }
        let mut moves = HashSet::new();
        for entry in &record.moves {
            if entry.path.is_empty()
                || protected(&entry.path)
                || !scaffolds.contains(&key(&entry.path[..entry.path.len() - 1]))
                || scaffolds.contains(&key(&entry.path))
                || !moves.insert(key(&entry.path))
            {
                return Err(invalid());
            }
        }
        for entry in record.moves.iter().chain(&record.scaffold) {
            if entry.path.len() > MAX_DEPTH
                || entry.path.iter().any(|part| name(part).is_err())
                || entry.identity.platform_id.is_empty()
                || entry.identity.platform_id.len() > 128
            {
                return Err(invalid());
            }
        }
        Ok(())
    }

    pub(super) fn pending(&self) -> bool {
        matches!(self.load(), Ok(Some(_)))
    }

    fn plan(&self, commit: String) -> io::Result<Record> {
        let destination = self.parent.child(&self.destination, false)?;
        let stage = self.parent.child(&self.stage, false)?;
        let mut scaffold = inventory(&destination, true)?;
        scaffold.insert(
            0,
            Entry {
                path: Vec::new(),
                kind: Kind::Dir,
                identity: Identity::read(&destination.file)?,
            },
        );
        let existing: HashMap<_, _> = scaffold
            .iter()
            .map(|e| (key(&e.path), &e.identity))
            .collect();
        let mut moves: Vec<Entry> = Vec::new();
        for entry in inventory(&stage, false)? {
            if protected(&entry.path) {
                return Err(invalid());
            }
            if moves.iter().any(|m| entry.path.starts_with(&m.path)) {
                continue;
            }
            let (parent, component) = location(&destination, &entry.path)?;
            if let Some(expected) = existing.get(&key(&entry.path)) {
                let directory = parent.child(&component, false)?;
                if entry.kind != Kind::Dir || !expected.matches(&directory.file, Kind::Dir)? {
                    return Err(invalid());
                }
            } else {
                if optional(&parent, &component)?.is_some() {
                    return Err(invalid());
                }
                moves.push(entry);
            }
        }
        moves.sort_by_key(|entry| entry.path == [".git"]);
        let record = Record {
            version: 1,
            destination: self.destination.as_str().to_owned(),
            stage: self.stage.as_str().to_owned(),
            commit,
            stage_identity: Identity::read(&stage.file)?,
            moves,
            scaffold,
        };
        self.validate(&record)?;
        Ok(record)
    }

    fn save(&self, record: &Record) -> io::Result<()> {
        let bytes = serde_json::to_vec(record).map_err(|_| invalid())?;
        if bytes.len() as u64 > MAX_RECORD_BYTES {
            return Err(invalid());
        }
        let mut file = self.parent.open(&self.record_name, true, true)?;
        file.write_all(&bytes)?;
        file.flush()?;
        file.sync_all()?;
        #[cfg(unix)]
        self.parent.file.sync_all()?;
        Ok(())
    }

    fn scaffold_pins(&self, record: &Record) -> io::Result<Vec<Arc<PinnedDirectory>>> {
        let root = self.parent.child(&self.destination, false)?;
        record
            .scaffold
            .iter()
            .map(|entry| {
                let directory = child(&root, &entry.path)?;
                if !entry.identity.matches(&directory.file, Kind::Dir)? {
                    return Err(invalid());
                }
                Ok(directory)
            })
            .collect()
    }

    fn stage_pin(&self, record: &Record) -> io::Result<Arc<PinnedDirectory>> {
        let stage = self.parent.child(&self.stage, false)?;
        if !record.stage_identity.matches(&stage.file, Kind::Dir)? {
            return Err(invalid());
        }
        Ok(stage)
    }

    fn rollback(&self, record: &Record) -> io::Result<()> {
        let pins = self.scaffold_pins(record)?;
        let destination = &pins[0];
        let stage = self.stage_pin(record)?;
        let mut moved = Vec::new();
        // Validate every location before undoing anything. Never guess which entry is ours.
        for entry in &record.moves {
            let (from, n) = location(&stage, &entry.path)?;
            let (to, _) = location(destination, &entry.path)?;
            match (optional(&from, &n)?, optional(&to, &n)?) {
                (Some(file), None) if entry.identity.matches(&file, entry.kind)? => {}
                (None, Some(file)) if entry.identity.matches(&file, entry.kind)? => {
                    moved.push((entry, from, to, n, file));
                }
                _ => return Err(invalid()),
            }
        }
        for (entry, from, to, n, file) in moved.into_iter().rev() {
            if !entry.identity.matches(&file, entry.kind)? {
                return Err(invalid());
            }
            rename_no_replace(&to, &n, &file, &from, &n)?;
        }
        let actual = inventory(destination, true)?;
        if actual.len() + 1 != record.scaffold.len() {
            return Err(invalid());
        }
        // Pins above already checked the identities of every original directory.
        Ok(())
    }

    pub(super) fn discard_stage(&self) -> io::Result<()> {
        match self.parent.remove_owned_tree(&self.stage) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            result => result,
        }
    }

    fn remove_record(&self) -> io::Result<()> {
        self.parent.remove_owned_file(&self.record_name)
    }

    fn cleanup_committed(&self) -> io::Result<()> {
        fn empty_dirs(
            parent: &Arc<PinnedDirectory>,
            n: &SafeFileName,
            depth: usize,
        ) -> io::Result<()> {
            if depth > MAX_DEPTH {
                return Err(invalid());
            }
            let directory = parent.child(n, false)?;
            for entry in fs::read_dir(directory.anchored_path())? {
                let text = entry?.file_name().into_string().map_err(|_| invalid())?;
                empty_dirs(&directory, &name(&text)?, depth + 1)?;
            }
            drop(directory);
            fs::remove_dir(parent.anchored_path().join(n.as_str()))
        }
        self.remove_record()?;
        match empty_dirs(&self.parent, &self.stage, 0) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            result => result,
        }
    }

    fn committed(&self, record: &Record) -> io::Result<bool> {
        let destination = self.parent.child(&self.destination, false)?;
        let git = record.moves.last().ok_or_else(invalid)?;
        match destination.child(&name(".git")?, false) {
            Ok(directory) => {
                if git.identity.matches(&directory.file, Kind::Dir)? {
                    Ok(true)
                } else {
                    Err(invalid())
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }
}

#[cfg(test)]
#[derive(Clone, Copy)]
pub(super) enum Fault {
    FailAfter(usize),
    CancelAfter(usize),
    KillAfter(usize),
    AfterCommit,
}

impl GitSource {
    async fn verify_rollback(
        &self,
        publication: &Publication,
        record: &Record,
        layout: &Layout,
        ctx: &StepContext,
    ) -> Result<(), ProvisionError> {
        publication
            .rollback(record)
            .map_err(|_| failed(INTERRUPTED))?;
        let stage = publication
            .stage_pin(record)
            .map_err(|_| failed(INTERRUPTED))?;
        let cancel = application::shared::CancelToken::new();
        let limits = RunLimits {
            timeout: Some(Duration::from_secs(120)),
            ..RunLimits::default()
        };
        let spec = self
            .git_command(layout)?
            .arg("-C")
            .arg(stage.anchored_path())
            .args([
                "status",
                "--porcelain=v1",
                "--ignored",
                "--untracked-files=all",
            ])
            .retain(ctx.retained.clone());
        let output = run_quiet(&spec, &cancel, &limits)
            .await
            .map_err(|_| failed(INTERRUPTED))?;
        if !output.stdout.trim().is_empty()
            || self
                .head_commit_with_limits(layout, &stage.anchored_path(), ctx, &cancel, &limits)
                .await
                .map_err(|_| failed(INTERRUPTED))?
                != record.commit
        {
            return Err(failed(INTERRUPTED));
        }
        drop(stage);
        publication
            .discard_stage()
            .map_err(|_| failed(INTERRUPTED))?;
        publication.remove_record().map_err(|_| failed(INTERRUPTED))
    }

    pub(super) async fn recover_publication(
        &self,
        publication: &Publication,
        layout: &Layout,
        ctx: &StepContext,
    ) -> Result<bool, ProvisionError> {
        let Some(record) = publication.load().map_err(|_| failed(INTERRUPTED))? else {
            return Ok(false);
        };
        if publication
            .committed(&record)
            .map_err(|_| failed(INTERRUPTED))?
        {
            if let Err(error) = publication.cleanup_committed() {
                ctx.log(format!(
                    "Warning: the checkout was published, but staging cleanup failed: {error}"
                ));
            }
            return Ok(true);
        }
        self.verify_rollback(publication, &record, layout, ctx)
            .await?;
        Ok(false)
    }

    pub(super) async fn publish_scaffold(
        &self,
        publication: &Publication,
        layout: &Layout,
        ctx: &StepContext,
    ) -> Result<(), ProvisionError> {
        let stage = publication
            .parent
            .anchored_path()
            .join(publication.stage.as_str());
        let commit = match self.head_commit(layout, &stage, ctx).await {
            Ok(commit) => commit,
            Err(error) => {
                let _ = publication.discard_stage();
                return Err(error);
            }
        };
        let planned = (|| {
            let record = publication.plan(commit)?;
            let pins = publication.scaffold_pins(&record)?;
            let stage = publication.stage_pin(&record)?;
            let mut moves = Vec::new();
            for entry in &record.moves {
                let (from, component) = location(&stage, &entry.path)?;
                let (to, _) = location(&pins[0], &entry.path)?;
                let file = from.open_entry(&component)?;
                if !entry.identity.matches(&file, entry.kind)?
                    || optional(&to, &component)?.is_some()
                {
                    return Err(invalid());
                }
                moves.push((from, to, component, file));
            }
            // Check the serialized bound before creating a record or moving anything.
            if serde_json::to_vec(&record).map_err(|_| invalid())?.len() as u64 > MAX_RECORD_BYTES {
                return Err(invalid());
            }
            Ok((record, pins, moves))
        })();
        let (record, pins, moves) = match planned {
            Ok(planned) => planned,
            Err(_) => {
                let _ = publication.discard_stage();
                return Err(failed(CONFLICT));
            }
        };
        publication.save(&record).map_err(|_| failed(INTERRUPTED))?;
        #[cfg(test)]
        let fault = self.publish_fault.lock().unwrap().take();
        let mut committed = false;
        let result = (|| {
            for (index, (from, to, component, file)) in moves.iter().enumerate() {
                if ctx.cancel.is_cancelled() {
                    return Err(ProvisionError::Cancelled);
                }
                let planned = &record.moves[index];
                if !planned
                    .identity
                    .matches(file, planned.kind)
                    .map_err(|_| failed("The clone publication failed."))?
                {
                    return Err(failed("The clone publication failed."));
                }
                rename_no_replace(from, component, file, to, component)
                    .map_err(|_| failed("The clone publication failed."))?;
                if index + 1 == moves.len() {
                    committed = true;
                }
                #[cfg(test)]
                match fault {
                    Some(Fault::FailAfter(n)) if n == index + 1 => {
                        return Err(failed("Injected publication failure."));
                    }
                    Some(Fault::CancelAfter(n)) if n == index + 1 => ctx.cancel.cancel(),
                    Some(Fault::KillAfter(n)) if n == index + 1 => {
                        return Err(failed(INTERRUPTED));
                    }
                    _ => {}
                }
            }
            Ok(())
        })();
        drop(moves);
        drop(pins);
        #[cfg(test)]
        if matches!(fault, Some(Fault::KillAfter(_))) && result.is_err() {
            return result;
        }
        if committed
            || publication
                .committed(&record)
                .map_err(|_| failed(INTERRUPTED))?
        {
            #[cfg(test)]
            if matches!(fault, Some(Fault::AfterCommit)) {
                return Ok(());
            }
            if result.is_err() {
                ctx.log("Warning: publication completed before its final I/O error.");
            }
            if let Err(error) = publication.cleanup_committed() {
                ctx.log(format!(
                    "Warning: the checkout was published, but staging cleanup failed: {error}"
                ));
            }
            return Ok(());
        }
        let error = result
            .err()
            .unwrap_or_else(|| failed("The clone publication failed."));
        self.verify_rollback(publication, &record, layout, ctx)
            .await?;
        Err(error)
    }
}
