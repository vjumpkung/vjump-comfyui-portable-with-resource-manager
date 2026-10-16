//! Runtime persistence in the existing shared app database.
use crate::shared::{
    logging::redact,
    sqlite::{Db, DbError},
};
use application::comfy_runtime::*;
use async_trait::async_trait;
use domain::{
    comfy_runtime::*,
    environment::{DistName, EnvId, PinSet},
};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone)]
pub struct SqliteRuntimeStore {
    db: Db,
}
impl SqliteRuntimeStore {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}
fn sql(_: rusqlite::Error) -> DbError {
    DbError::Unavailable
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OptionsDoc {
    version: u32,
    port: u16,
    vram: String,
    attention: String,
    preview: String,
    enable_manager: bool,
    listen_lan: bool,
    extra_args: Vec<String>,
}
impl From<&LaunchOptions> for OptionsDoc {
    fn from(o: &LaunchOptions) -> Self {
        Self {
            version: 1,
            port: o.port.get(),
            vram: format!("{:?}", o.vram),
            attention: format!("{:?}", o.attention),
            preview: o.preview.as_str().into(),
            enable_manager: o.enable_manager,
            listen_lan: o.listen_lan,
            extra_args: o.extra_args.clone(),
        }
    }
}
impl TryFrom<OptionsDoc> for LaunchOptions {
    type Error = RuntimeError;
    fn try_from(o: OptionsDoc) -> Result<Self, Self::Error> {
        if o.version != 1 {
            return Err(RuntimeError::InvalidRecord);
        }
        let options = Self {
            port: Port::new(o.port).map_err(|_| RuntimeError::InvalidRecord)?,
            vram: match o.vram.as_str() {
                "Auto" => VramMode::Auto,
                "Low" => VramMode::Low,
                "High" => VramMode::High,
                "NoVram" => VramMode::NoVram,
                _ => return Err(RuntimeError::InvalidRecord),
            },
            attention: match o.attention.as_str() {
                "Default" => Attention::Default,
                "Sage" => Attention::Sage,
                "Flash" => Attention::Flash,
                "Pytorch" => Attention::Pytorch,
                "Split" => Attention::Split,
                "Quad" => Attention::Quad,
                _ => return Err(RuntimeError::InvalidRecord),
            },
            preview: match o.preview.as_str() {
                "auto" => PreviewMethod::Auto,
                "latent2rgb" => PreviewMethod::Latent2Rgb,
                "taesd" => PreviewMethod::Taesd,
                "none" => PreviewMethod::None,
                _ => return Err(RuntimeError::InvalidRecord),
            },
            enable_manager: o.enable_manager,
            listen_lan: o.listen_lan,
            extra_args: o.extra_args,
        };
        options
            .validate()
            .map_err(|_| RuntimeError::InvalidRecord)?;
        Ok(options)
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionDoc {
    version: u32,
    id: String,
    env_id: String,
    interpreter: PathBuf,
    checkout: PathBuf,
    commit: String,
    tag: Option<String>,
    branch: Option<String>,
    options: OptionsDoc,
    started_at: String,
    identity: Option<(u32, String, String)>,
}
impl From<&RuntimeSession> for SessionDoc {
    fn from(s: &RuntimeSession) -> Self {
        Self {
            version: 1,
            id: s.id.as_str().into(),
            env_id: s.env_id.as_str().into(),
            interpreter: s.interpreter.clone(),
            checkout: s.checkout.clone(),
            commit: s.revision.commit.clone(),
            tag: s.revision.tag.clone(),
            branch: s.revision.branch.clone(),
            options: (&s.options).into(),
            started_at: s.started_at.clone(),
            identity: s
                .identity
                .as_ref()
                .map(|i| (i.pid, i.created.clone(), i.scope.clone())),
        }
    }
}
impl TryFrom<SessionDoc> for RuntimeSession {
    type Error = RuntimeError;
    fn try_from(s: SessionDoc) -> Result<Self, Self::Error> {
        if s.version != 1
            || !oid(&s.commit)
            || !s.interpreter.is_absolute()
            || !s.checkout.is_absolute()
        {
            return Err(RuntimeError::InvalidRecord);
        }
        if s.identity.as_ref().is_some_and(|(pid, created, scope)| {
            *pid == 0 || created.is_empty() || scope.is_empty()
        }) {
            return Err(RuntimeError::InvalidRecord);
        }
        Ok(Self {
            id: SessionId::new(s.id)?,
            env_id: EnvId::new(s.env_id).map_err(|_| RuntimeError::InvalidRecord)?,
            interpreter: s.interpreter,
            checkout: s.checkout,
            revision: Revision {
                commit: s.commit,
                tag: s.tag,
                branch: s.branch,
            },
            options: s.options.try_into()?,
            started_at: s.started_at,
            identity: s.identity.map(|(pid, created, scope)| ProcessIdentity {
                pid,
                created,
                scope,
            }),
        })
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateDoc {
    version: u32,
    id: String,
    env_id: String,
    old_commit: String,
    target_commit: String,
    target_tag: Option<String>,
    fast_forward: bool,
    pins: Vec<(String, String)>,
    phase: String,
    failure: Option<String>,
    created_at: String,
}
impl From<&UpdateRun> for UpdateDoc {
    fn from(r: &UpdateRun) -> Self {
        Self {
            version: 1,
            id: r.id.as_str().into(),
            env_id: r.env_id.as_str().into(),
            old_commit: r.old_commit.clone(),
            target_commit: r.target.commit.clone(),
            target_tag: r.target.tag.clone(),
            fast_forward: r.target.fast_forward,
            pins: r
                .pins
                .iter()
                .map(|(n, v)| (n.to_string(), v.into()))
                .collect(),
            phase: r.phase.as_str().into(),
            failure: r.failure.as_ref().map(|s| redact(s).into_owned()),
            created_at: r.created_at.clone(),
        }
    }
}
impl TryFrom<UpdateDoc> for UpdateRun {
    type Error = RuntimeError;
    fn try_from(r: UpdateDoc) -> Result<Self, Self::Error> {
        if r.version != 1 || !oid(&r.old_commit) || !oid(&r.target_commit) {
            return Err(RuntimeError::InvalidRecord);
        }
        let phase = match r.phase.as_str() {
            "intent" => UpdatePhase::Intent,
            "source_changed" => UpdatePhase::SourceChanged,
            "dependencies" => UpdatePhase::Dependencies,
            "verifying" => UpdatePhase::Verifying,
            "publishing" => UpdatePhase::Publishing,
            "complete" => UpdatePhase::Complete,
            _ => return Err(RuntimeError::InvalidRecord),
        };
        let mut pins = PinSet::new();
        for (name, version) in r.pins {
            pins.insert(
                DistName::new(&name).map_err(|_| RuntimeError::InvalidRecord)?,
                version,
            );
        }
        Ok(Self {
            id: SessionId::new(r.id)?,
            env_id: EnvId::new(r.env_id).map_err(|_| RuntimeError::InvalidRecord)?,
            old_commit: r.old_commit,
            target: FrozenUpdate {
                commit: r.target_commit,
                tag: r.target_tag,
                fast_forward: r.fast_forward,
            },
            pins,
            phase,
            failure: r.failure,
            created_at: r.created_at,
        })
    }
}
pub(crate) fn oid(s: &str) -> bool {
    matches!(s.len(), 40 | 64) && s.bytes().all(|b| b.is_ascii_hexdigit())
}
fn encode<T: Serialize>(value: &T) -> Result<String, RuntimeError> {
    serde_json::to_string(value).map_err(|_| RuntimeError::InvalidRecord)
}
fn decode<T: serde::de::DeserializeOwned>(value: &str) -> Result<T, RuntimeError> {
    serde_json::from_str(value).map_err(|_| RuntimeError::InvalidRecord)
}

#[async_trait]
impl RuntimeStore for SqliteRuntimeStore {
    async fn launch_options(&self) -> Result<LaunchOptions, RuntimeError> {
        let row: Option<(u32, String)> = self
            .db
            .run(|c| {
                c.query_row(
                    "SELECT version, json FROM comfy_launch_options WHERE singleton=1",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()
                .map_err(sql)
            })
            .await
            .map_err(|_| RuntimeError::Storage)?;
        match row {
            None => Ok(LaunchOptions::default()),
            Some((1, json)) => decode::<OptionsDoc>(&json)?.try_into(),
            _ => Err(RuntimeError::InvalidRecord),
        }
    }
    async fn save_launch_options(&self, options: &LaunchOptions) -> Result<(), RuntimeError> {
        options.validate()?;
        let json = encode(&OptionsDoc::from(options))?;
        self.db.run(move |c| c.execute("INSERT INTO comfy_launch_options(singleton,version,json) VALUES(1,1,?1) ON CONFLICT(singleton) DO UPDATE SET version=1,json=excluded.json", [json]).map(|_| ()).map_err(sql)).await.map_err(|_| RuntimeError::Storage)
    }
    async fn save_session(
        &self,
        session: &RuntimeSession,
        finished: bool,
    ) -> Result<(), RuntimeError> {
        let id = session.id.as_str().to_owned();
        let json = encode(&SessionDoc::from(session))?;
        self.db.run(move |c| c.execute("INSERT INTO comfy_runtime_sessions(id,json,finished) VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET json=excluded.json,finished=excluded.finished", params![id,json,finished]).map(|_| ()).map_err(sql)).await.map_err(|_| RuntimeError::Storage)
    }
    async fn unfinished_sessions(&self) -> Result<Vec<RuntimeSession>, RuntimeError> {
        let rows = self
            .db
            .run(|c| {
                c.prepare("SELECT json FROM comfy_runtime_sessions WHERE finished=0 ORDER BY rowid")
                    .map_err(sql)?
                    .query_map([], |r| r.get::<_, String>(0))
                    .map_err(sql)?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(sql)
            })
            .await
            .map_err(|_| RuntimeError::Storage)?;
        rows.iter()
            .map(|s| decode::<SessionDoc>(s)?.try_into())
            .collect()
    }
    async fn save_update(&self, run: &UpdateRun) -> Result<(), RuntimeError> {
        let id = run.id.as_str().to_owned();
        let phase = run.phase.as_str();
        let json = encode(&UpdateDoc::from(run))?;
        self.db.run(move |c| c.execute("INSERT INTO comfy_update_runs(id,json,phase) VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET json=excluded.json,phase=excluded.phase", params![id,json,phase]).map(|_| ()).map_err(sql)).await.map_err(|_| RuntimeError::Storage)
    }
    async fn unfinished_update(&self) -> Result<Option<UpdateRun>, RuntimeError> {
        let row = self
            .db
            .run(|c| {
                c.query_row(
                    "SELECT json FROM comfy_update_runs WHERE phase <> 'complete'",
                    [],
                    |r| r.get::<_, String>(0),
                )
                .optional()
                .map_err(sql)
            })
            .await
            .map_err(|_| RuntimeError::Storage)?;
        row.map(|s| decode::<UpdateDoc>(&s)?.try_into()).transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn options_roundtrip_and_corruption_is_explicit() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(dir.path().join("app.db")).await.unwrap();
        let store = SqliteRuntimeStore::new(db.clone());
        assert_eq!(
            store.launch_options().await.unwrap(),
            LaunchOptions::default()
        );
        let options = LaunchOptions {
            port: Port::new(9000).unwrap(),
            ..Default::default()
        };
        store.save_launch_options(&options).await.unwrap();
        assert_eq!(store.launch_options().await.unwrap(), options);
        db.run(|c| {
            c.execute("UPDATE comfy_launch_options SET json='{}'", [])
                .map_err(sql)?;
            Ok(())
        })
        .await
        .unwrap();
        assert_eq!(
            store.launch_options().await,
            Err(RuntimeError::InvalidRecord)
        );
    }
}
