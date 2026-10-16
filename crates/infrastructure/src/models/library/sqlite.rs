//! SQLite adapters of the library: the model catalog, the compare-and-apply scan commit, the
//! hash cache, the operation journal and the download destination reservations.
use super::{
    roots::LibraryRoots,
    scan::{KnownFiles, RootScan},
};
use crate::{
    models::acquisition::{codec, sqlite::SqliteDownloads},
    shared::sqlite::{Db, DbError},
};
use application::{models::library::*, shared::path_locks::DestinationReservations};
use async_trait::async_trait;
use domain::{
    models::{
        acquisition::{Destination, FileIdentity},
        library::{ModelCategory, ModelFile, ModelLocation, SidecarSet},
    },
    shared::model_values::{JobId, RelativeModelPath, Sha256},
};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use std::{collections::HashMap, sync::Arc};

pub struct SqliteCatalog {
    pub db: Db,
    pub roots: Arc<LibraryRoots>,
}

fn storage(_: DbError) -> LibraryError {
    LibraryErrorCode::Storage.into()
}
fn sql(_: rusqlite::Error) -> DbError {
    DbError::Unavailable
}
fn integer(value: u64) -> std::result::Result<i64, DbError> {
    i64::try_from(value).map_err(|_| DbError::Unavailable)
}

pub fn revision_of(connection: &Connection) -> std::result::Result<u64, DbError> {
    connection
        .query_row(
            "SELECT value FROM library_state WHERE key='catalog_revision'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .map(|v| v as u64)
        .map_err(sql)
}

const COLUMNS: &str = "root_id,path,category,size,mtime,file_identity,sha256,source_job_id,civitai_version_id,sidecars";

fn model_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Option<ModelFile>> {
    let root_id: String = row.get(0)?;
    let path: String = row.get(1)?;
    let category: String = row.get(2)?;
    let size: i64 = row.get(3)?;
    let mtime: String = row.get(4)?;
    let identity: String = row.get(5)?;
    let sha256: Option<String> = row.get(6)?;
    let job: Option<String> = row.get(7)?;
    let version: Option<i64> = row.get(8)?;
    let sidecars: i64 = row.get(9)?;
    let platform_id = serde_json::from_str::<Value>(&identity)
        .ok()
        .and_then(|v| v["platform_id"].as_str().map(str::to_owned))
        .unwrap_or_default();
    // A row that no longer validates is skipped instead of failing the whole list.
    let (Ok(path), Ok(category)) = (RelativeModelPath::new(&path), ModelCategory::new(&category))
    else {
        return Ok(None);
    };
    Ok(Some(ModelFile {
        location: ModelLocation::new(root_id, path),
        category,
        size: size.max(0) as u64,
        modified_ns: mtime.parse().unwrap_or(0),
        platform_id,
        sha256: sha256.and_then(|s| Sha256::new(&s).ok()),
        source_job_id: job.and_then(|j| JobId::new(j).ok()),
        civitai_version_id: version.map(|v| v as u64),
        sidecars: SidecarSet::from_bits(sidecars as u8),
    }))
}

fn destination_json(
    category: &ModelCategory,
    path: &RelativeModelPath,
    root_id: &str,
    fixed_category: bool,
) -> Option<String> {
    let parts = path.components();
    let (name, directories) = parts.split_last()?;
    let below = if fixed_category {
        directories
    } else {
        directories.get(1..)?
    };
    let mut subdirectory = RelativeModelPath::default();
    for part in below {
        subdirectory = subdirectory.join(part.clone());
    }
    Some(
        codec::destination_value(&Destination {
            root_id: root_id.to_owned(),
            category: category.clone(),
            subdirectory,
            filename: name.clone(),
        })
        .to_string(),
    )
}

impl SqliteCatalog {
    pub async fn known_files(&self, root_id: &str) -> Result<KnownFiles> {
        let root_id = root_id.to_owned();
        self.db
            .run(move |c| {
                let mut q = c
                    .prepare("SELECT path,size,mtime FROM models WHERE root_id=?1")
                    .map_err(sql)?;
                let rows = q
                    .query_map([root_id], |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, i64>(1)? as u64,
                            r.get::<_, String>(2)?,
                        ))
                    })
                    .map_err(sql)?;
                let mut known = KnownFiles::new();
                for row in rows {
                    let (path, size, mtime) = row.map_err(sql)?;
                    known.insert(path, (size, mtime.parse().unwrap_or(0)));
                }
                Ok(known)
            })
            .await
            .map_err(storage)
    }

    /// Commits scan observations only if the catalog revision is still the one the scan began
    /// with. Rows of complete roots that were not seen are deleted; incomplete roots only add
    /// and update. Returns `false` when the observations are stale.
    pub async fn apply_scan(
        &self,
        expected_revision: u64,
        scans: Vec<RootScan>,
        now: String,
    ) -> Result<bool> {
        let fixed: HashMap<String, bool> = self
            .roots
            .specs()
            .into_iter()
            .map(|s| (s.id.clone(), s.category.is_some()))
            .collect();
        self.db
            .run(move |c| {
                let tx = c.transaction().map_err(sql)?;
                if revision_of(&tx)? != expected_revision {
                    return Ok(false);
                }
                for scan in &scans {
                    let fixed_category = fixed.get(&scan.root_id).copied().unwrap_or(false);
                    let mut existing: HashMap<String, (i64, String, i64)> = HashMap::new();
                    {
                        let mut q = tx
                            .prepare("SELECT path,size,mtime,sidecars FROM models WHERE root_id=?1")
                            .map_err(sql)?;
                        let rows = q
                            .query_map([&scan.root_id], |r| {
                                Ok((
                                    r.get::<_, String>(0)?,
                                    (r.get::<_, i64>(1)?, r.get::<_, String>(2)?, r.get::<_, i64>(3)?),
                                ))
                            })
                            .map_err(sql)?;
                        for row in rows {
                            let (path, value) = row.map_err(sql)?;
                            existing.insert(path, value);
                        }
                    }
                    let mut seen = std::collections::HashSet::new();
                    for file in &scan.files {
                        seen.insert(file.path.clone());
                        let bits = i64::from(file.sidecars.bits());
                        let mtime = file.modified_ns.to_string();
                        match existing.get(&file.path) {
                            Some((size, stored_mtime, sidecars))
                                if *size == file.size as i64 && *stored_mtime == mtime =>
                            {
                                if *sidecars != bits {
                                    tx.execute(
                                        "UPDATE models SET sidecars=?3 WHERE root_id=?1 AND path=?2",
                                        params![scan.root_id, file.path, bits],
                                    )
                                    .map_err(sql)?;
                                }
                            }
                            other => {
                                let identity = file.identity.clone().unwrap_or(FileIdentity {
                                    size: file.size,
                                    modified_ns: file.modified_ns,
                                    platform_id: String::new(),
                                    digest: None,
                                });
                                let identity_json = codec::identity_value(&identity).to_string();
                                let cached: Option<String> = tx
                                    .query_row(
                                        "SELECT sha256 FROM model_hash_cache WHERE root_id=?1 AND path=?2 AND size=?3 AND mtime_ns=?4",
                                        params![scan.root_id, file.path, integer(file.size)?, mtime],
                                        |r| r.get(0),
                                    )
                                    .optional()
                                    .map_err(sql)?;
                                let path = RelativeModelPath::new(&file.path)
                                    .map_err(|_| DbError::Unavailable)?;
                                let meta = destination_json(
                                    &file.category,
                                    &path,
                                    &scan.root_id,
                                    fixed_category,
                                );
                                if other.is_some() {
                                    // A replaced file never inherits the old hash or provenance.
                                    tx.execute(
                                        "UPDATE models SET category=?3,size=?4,mtime=?5,file_identity=?6,sha256=?7,source_job_id=NULL,civitai_version_id=NULL,meta_json=?8,sidecars=?9 WHERE root_id=?1 AND path=?2",
                                        params![scan.root_id, file.path, file.category.as_str(), integer(file.size)?, mtime, identity_json, cached, meta, bits],
                                    )
                                    .map_err(sql)?;
                                } else {
                                    tx.execute(
                                        "INSERT INTO models(root_id,path,category,size,mtime,file_identity,sha256,source_job_id,civitai_version_id,meta_json,sidecars) VALUES (?1,?2,?3,?4,?5,?6,?7,NULL,NULL,?8,?9)",
                                        params![scan.root_id, file.path, file.category.as_str(), integer(file.size)?, mtime, identity_json, cached, meta, bits],
                                    )
                                    .map_err(sql)?;
                                }
                            }
                        }
                    }
                    if scan.complete {
                        for path in existing.keys().filter(|p| !seen.contains(*p)) {
                            tx.execute(
                                "DELETE FROM models WHERE root_id=?1 AND path=?2",
                                params![scan.root_id, path],
                            )
                            .map_err(sql)?;
                        }
                    }
                    tx.execute(
                        "INSERT INTO model_roots(id,path,category,status,scanned_at) VALUES (?1,'',NULL,?2,?3) ON CONFLICT(id) DO UPDATE SET status=excluded.status,scanned_at=excluded.scanned_at",
                        params![scan.root_id, if scan.complete { "ok" } else { "partial" }, now],
                    )
                    .map_err(sql)?;
                }
                tx.commit().map_err(sql)?;
                Ok(true)
            })
            .await
            .map_err(storage)
    }
}

#[async_trait]
impl ModelCatalog for SqliteCatalog {
    async fn list(&self) -> Result<Vec<ModelFile>> {
        self.db
            .run(|c| {
                let mut q = c
                    .prepare(&format!(
                        "SELECT {COLUMNS} FROM models ORDER BY category,path"
                    ))
                    .map_err(sql)?;
                let rows = q.query_map([], model_from_row).map_err(sql)?;
                let mut models = Vec::new();
                for row in rows {
                    models.extend(row.map_err(sql)?);
                }
                Ok(models)
            })
            .await
            .map_err(storage)
    }
    async fn revision(&self) -> Result<u64> {
        self.db.run(|c| revision_of(c)).await.map_err(storage)
    }
    async fn get(&self, location: &ModelLocation) -> Result<Option<ModelFile>> {
        let (root, path) = (location.root_id.clone(), location.path.as_string());
        self.db
            .run(move |c| {
                c.query_row(
                    &format!("SELECT {COLUMNS} FROM models WHERE root_id=?1 AND path=?2"),
                    [root, path],
                    model_from_row,
                )
                .optional()
                .map(Option::flatten)
                .map_err(sql)
            })
            .await
            .map_err(storage)
    }
    async fn accepts(&self, location: &ModelLocation) -> bool {
        self.roots.accepts(location)
    }
    async fn apply_operation(
        &self,
        op_id: &str,
        from: &ModelLocation,
        to: &ModelLocation,
    ) -> Result<()> {
        let (category, _, _) = self
            .roots
            .classify(to)
            .ok_or(LibraryErrorCode::OutsideRoot)?;
        let fixed = self
            .roots
            .spec(&to.root_id)
            .is_some_and(|s| s.category.is_some());
        let meta = destination_json(&category, &to.path, &to.root_id, fixed)
            .ok_or(LibraryErrorCode::OutsideRoot)?;
        let (op_id, from, to) = (op_id.to_owned(), from.clone(), to.clone());
        self.db
            .run(move |c| {
                let tx = c.transaction().map_err(sql)?;
                let (from_root, from_path) = (from.root_id.clone(), from.path.as_string());
                let (to_root, to_path) = (to.root_id.clone(), to.path.as_string());
                if (from_root.as_str(), from_path.as_str()) != (to_root.as_str(), to_path.as_str()) {
                    tx.execute(
                        "UPDATE models SET root_id=?3,path=?4,category=?5,meta_json=?6 WHERE root_id=?1 AND path=?2",
                        params![from_root, from_path, to_root, to_path, category.as_str(), meta],
                    )
                    .map_err(sql)?;
                    tx.execute(
                        "DELETE FROM model_hash_cache WHERE root_id=?1 AND path=?2",
                        params![from_root, from_path],
                    )
                    .map_err(sql)?;
                }
                tx.execute("DELETE FROM model_operations WHERE id=?1", [&op_id])
                    .map_err(sql)?;
                tx.commit().map_err(sql)?;
                Ok(())
            })
            .await
            .map_err(storage)
    }
    async fn remove(&self, location: &ModelLocation) -> Result<()> {
        let (root, path) = (location.root_id.clone(), location.path.as_string());
        self.db
            .run(move |c| {
                let tx = c.transaction().map_err(sql)?;
                tx.execute(
                    "DELETE FROM models WHERE root_id=?1 AND path=?2",
                    [&root, &path],
                )
                .map_err(sql)?;
                tx.execute(
                    "DELETE FROM model_hash_cache WHERE root_id=?1 AND path=?2",
                    [&root, &path],
                )
                .map_err(sql)?;
                tx.commit().map_err(sql)
            })
            .await
            .map_err(storage)
    }
    async fn cached_hash(
        &self,
        location: &ModelLocation,
        identity: &FileIdentity,
    ) -> Result<Option<Sha256>> {
        let (root, path) = (location.root_id.clone(), location.path.as_string());
        let (size, mtime) = (identity.size, identity.modified_ns.to_string());
        self.db
            .run(move |c| {
                let cached: Option<String> = c
                    .query_row(
                        "SELECT sha256 FROM model_hash_cache WHERE root_id=?1 AND path=?2 AND size=?3 AND mtime_ns=?4",
                        params![root, path, integer(size)?, mtime],
                        |r| r.get(0),
                    )
                    .optional()
                    .map_err(sql)?;
                if cached.is_some() {
                    return Ok(cached);
                }
                c.query_row(
                    "SELECT sha256 FROM models WHERE root_id=?1 AND path=?2 AND size=?3 AND mtime=?4 AND sha256 IS NOT NULL",
                    params![root, path, integer(size)?, mtime],
                    |r| r.get(0),
                )
                .optional()
                .map_err(sql)
            })
            .await
            .map_err(storage)
            .map(|hash| hash.and_then(|h| Sha256::new(&h).ok()))
    }
    async fn record_hash(
        &self,
        location: &ModelLocation,
        identity: &FileIdentity,
        sha256: &Sha256,
    ) -> Result<()> {
        self.record(location, identity, sha256, None).await
    }
    async fn record_identification(
        &self,
        location: &ModelLocation,
        identity: &FileIdentity,
        sha256: &Sha256,
        civitai_version_id: u64,
    ) -> Result<()> {
        self.record(location, identity, sha256, Some(civitai_version_id))
            .await
    }
}
impl SqliteCatalog {
    async fn record(
        &self,
        location: &ModelLocation,
        identity: &FileIdentity,
        sha256: &Sha256,
        version: Option<u64>,
    ) -> Result<()> {
        let (root, path) = (location.root_id.clone(), location.path.as_string());
        let (size, mtime) = (identity.size, identity.modified_ns.to_string());
        let identity_json = codec::identity_value(identity).to_string();
        let sha = sha256.as_str().to_owned();
        let stored = self
            .db
            .run(move |c| {
                let tx = c.transaction().map_err(sql)?;
                let changed = tx
                    .execute(
                        "UPDATE models SET sha256=?5,civitai_version_id=COALESCE(?6,civitai_version_id) WHERE root_id=?1 AND path=?2 AND size=?3 AND mtime=?4",
                        params![root, path, integer(size)?, mtime, sha, version.map(integer).transpose()?],
                    )
                    .map_err(sql)?;
                if changed == 1 {
                    tx.execute(
                        "INSERT INTO model_hash_cache(root_id,path,size,mtime_ns,file_identity,sha256) VALUES (?1,?2,?3,?4,?5,?6) ON CONFLICT(root_id,path) DO UPDATE SET size=excluded.size,mtime_ns=excluded.mtime_ns,file_identity=excluded.file_identity,sha256=excluded.sha256",
                        params![root, path, integer(size)?, mtime, identity_json, sha],
                    )
                    .map_err(sql)?;
                }
                tx.commit().map_err(sql)?;
                Ok(changed == 1)
            })
            .await
            .map_err(storage)?;
        if stored {
            Ok(())
        } else {
            Err(LibraryErrorCode::Stale.into())
        }
    }
}

pub struct SqliteJournal {
    pub db: Db,
}
fn location_json(location: &ModelLocation) -> Value {
    json!({"root": location.root_id, "path": location.path.as_string()})
}
fn location_from(value: &Value) -> Option<ModelLocation> {
    Some(ModelLocation::new(
        value["root"].as_str()?,
        RelativeModelPath::new(value["path"].as_str()?).ok()?,
    ))
}
fn kind_text(kind: OperationKind) -> &'static str {
    match kind {
        OperationKind::Rename => "rename",
        OperationKind::Move => "move",
    }
}
fn phase_text(phase: OperationPhase) -> &'static str {
    match phase {
        OperationPhase::Intent => "intent",
        OperationPhase::RecoveryRequired => "recovery_required",
    }
}
fn encode(record: &OperationRecord) -> String {
    json!({
        "members": record.members.iter().map(|m| json!({
            "from": location_json(&m.from),
            "to": location_json(&m.to),
            "via": m.via.as_ref().map(location_json),
            "model": m.model,
        })).collect::<Vec<_>>(),
    })
    .to_string()
}
fn decode(
    id: String,
    kind: &str,
    phase: &str,
    document: &str,
    created_at: String,
) -> Option<OperationRecord> {
    let value: Value = serde_json::from_str(document).ok()?;
    let members = value["members"]
        .as_array()?
        .iter()
        .map(|m| {
            Some(OperationMember {
                from: location_from(&m["from"])?,
                to: location_from(&m["to"])?,
                via: if m["via"].is_null() {
                    None
                } else {
                    Some(location_from(&m["via"])?)
                },
                model: m["model"].as_bool()?,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(OperationRecord {
        id,
        kind: if kind == "move" {
            OperationKind::Move
        } else {
            OperationKind::Rename
        },
        phase: if phase == "intent" {
            OperationPhase::Intent
        } else {
            OperationPhase::RecoveryRequired
        },
        members,
        created_at,
    })
}

#[async_trait]
impl OperationJournal for SqliteJournal {
    async fn begin(&self, record: &OperationRecord) -> Result<()> {
        let (id, kind, phase) = (
            record.id.clone(),
            kind_text(record.kind),
            phase_text(record.phase),
        );
        let (document, created) = (encode(record), record.created_at.clone());
        self.db
            .run(move |c| {
                c.execute(
                    "INSERT INTO model_operations(id,kind,phase,document,created_at) VALUES (?1,?2,?3,?4,?5)",
                    params![id, kind, phase, document, created],
                )
                .map(|_| ())
                .map_err(sql)
            })
            .await
            .map_err(storage)
    }
    async fn set_phase(&self, id: &str, phase: OperationPhase) -> Result<()> {
        let (id, phase) = (id.to_owned(), phase_text(phase));
        self.db
            .run(move |c| {
                c.execute(
                    "UPDATE model_operations SET phase=?2 WHERE id=?1",
                    params![id, phase],
                )
                .map(|_| ())
                .map_err(sql)
            })
            .await
            .map_err(storage)
    }
    async fn complete(&self, id: &str) -> Result<()> {
        let id = id.to_owned();
        self.db
            .run(move |c| {
                c.execute("DELETE FROM model_operations WHERE id=?1", [id])
                    .map(|_| ())
                    .map_err(sql)
            })
            .await
            .map_err(storage)
    }
    async fn unfinished(&self) -> Result<Vec<OperationRecord>> {
        self.db
            .run(|c| {
                let mut q = c
                    .prepare("SELECT id,kind,phase,document,created_at FROM model_operations ORDER BY created_at,id")
                    .map_err(sql)?;
                let rows = q
                    .query_map([], |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, String>(2)?,
                            r.get::<_, String>(3)?,
                            r.get::<_, String>(4)?,
                        ))
                    })
                    .map_err(sql)?;
                let mut records = Vec::new();
                for row in rows {
                    let (id, kind, phase, document, created) = row.map_err(sql)?;
                    // An undecodable entry must block, not vanish: report it as unresolved.
                    records.push(decode(id.clone(), &kind, &phase, &document, created.clone())
                        .unwrap_or(OperationRecord {
                            id,
                            kind: OperationKind::Rename,
                            phase: OperationPhase::RecoveryRequired,
                            members: Vec::new(),
                            created_at: created,
                        }));
                }
                Ok(records)
            })
            .await
            .map_err(storage)
    }
}

#[async_trait]
impl DestinationReservations for SqliteDownloads {
    async fn is_reserved(&self, key: &str) -> bool {
        let key = key.to_owned();
        self.db
            .run(move |c| {
                c.query_row(
                    "SELECT EXISTS(SELECT 1 FROM jobs WHERE reservation=?1 AND (state NOT IN ('completed','failed','cancelled') OR finalize_phase IS NOT NULL AND state != 'completed'))",
                    [key],
                    |r| r.get::<_, bool>(0),
                )
                .map_err(sql)
            })
            .await
            // When the reservation table cannot be read, refuse rather than risk a collision.
            .unwrap_or(true)
    }
}
