use super::codec;
use crate::shared::sqlite::{Db, DbError};
use application::models::acquisition::*;
use async_trait::async_trait;
use domain::{
    models::acquisition::*,
    shared::model_values::{JobId, Sha256},
};
use rusqlite::{OptionalExtension, params};

#[derive(Clone)]
pub struct SqliteDownloads {
    pub db: Db,
}
fn db_error(_: DbError) -> DownloadError {
    DownloadErrorCode::Storage.into()
}
fn integer(value: u64) -> std::result::Result<i64, DbError> {
    i64::try_from(value).map_err(|_| DbError::Unavailable)
}
fn persist(
    c: &rusqlite::Connection,
    j: &DownloadJob,
    generation: u64,
) -> std::result::Result<bool, DbError> {
    let doc = codec::encode(j).map_err(|_| DbError::Unavailable)?;
    let phase = j.publication.as_ref().map(|p| format!("{:?}", p.phase));
    c.execute("UPDATE jobs SET state=?1,generation=?2,updated_at=?3,retry_at=?4,finalize_phase=?5,document=?6,provider_identity=?9,root_id=?10,relative_path=?11,reservation=?12 WHERE id=?7 AND generation=?8",params![j.state.as_str(),integer(j.generation)?,integer(j.updated_at)?,j.retry_at.map(integer).transpose()?,phase,doc,j.id.as_str(),integer(generation)?,j.identity.0,j.destination.root_id,j.destination.relative_path().as_string(),j.destination.reservation_key()]).map(|n|n==1).map_err(|_|DbError::Unavailable)
}
fn read(c: &rusqlite::Connection, id: &str) -> std::result::Result<DownloadJob, DbError> {
    let raw: String = c
        .query_row("SELECT document FROM jobs WHERE id=?1", [id], |r| r.get(0))
        .map_err(|_| DbError::Unavailable)?;
    codec::decode(&raw).map_err(|_| DbError::Unavailable)
}
#[async_trait]
impl JobRepository for SqliteDownloads {
    async fn enqueue(&self, request: EnqueueRequest) -> Result<EnqueueOutcome> {
        self.db.run(move |c| {
            let tx=c.transaction().map_err(|_|DbError::Unavailable)?;let j=&request.job;
            let existing:Option<String>=tx.query_row("SELECT id FROM jobs WHERE provider_identity=?1 AND (state NOT IN ('completed','failed','cancelled') OR finalize_phase IS NOT NULL AND state!='completed')",[&j.identity.0],|r|r.get(0)).optional().map_err(|_|DbError::Unavailable)?;
            if let Some(id)=existing {return Ok(EnqueueOutcome::Duplicate(JobId::new(id).map_err(|_|DbError::Unavailable)?));}
            if !request.force&&let Some(destination)=request.catalog_match{return Ok(EnqueueOutcome::AlreadyDownloaded(destination));}
            let reserved:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM jobs WHERE reservation=?1 AND (state NOT IN ('completed','failed','cancelled') OR finalize_phase IS NOT NULL AND state!='completed'))",[j.destination.reservation_key()],|r|r.get(0)).map_err(|_|DbError::Unavailable)?;
            if reserved {return Ok(EnqueueOutcome::Conflict {destination:j.destination.clone(),current:None});}
            let doc=codec::encode(j).map_err(|_|DbError::Unavailable)?;
            tx.execute("INSERT INTO jobs(id,provider_identity,root_id,relative_path,reservation,state,generation,created_at,updated_at,retry_at,finalize_phase,document) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,NULL,NULL,?10)",params![j.id.as_str(),j.identity.0,j.destination.root_id,j.destination.relative_path().as_string(),j.destination.reservation_key(),j.state.as_str(),integer(j.generation)?,integer(j.created_at)?,integer(j.updated_at)?,doc]).map_err(|_|DbError::Unavailable)?;
            tx.commit().map_err(|_|DbError::Unavailable)?;Ok(EnqueueOutcome::Queued(j.id.clone()))
        }).await.map_err(db_error)
    }
    async fn get(&self, id: &JobId) -> Result<DownloadJob> {
        let id = id.as_str().to_owned();
        self.db.run(move |c| read(c, &id)).await.map_err(db_error)
    }
    async fn list(&self, before: Option<u64>, limit: u32) -> Result<Vec<DownloadJob>> {
        self.db.run(move|c| {
            let mut q=c.prepare("SELECT document FROM jobs WHERE (?1 IS NULL OR created_at<?1) ORDER BY created_at DESC,id DESC LIMIT ?2").map_err(|_|DbError::Unavailable)?;
            let rows=q.query_map(params![before.map(integer).transpose()?,limit.clamp(1,500)],|r|r.get::<_,String>(0)).map_err(|_|DbError::Unavailable)?;
            rows.map(|r|codec::decode(&r.map_err(|_|DbError::Unavailable)?).map_err(|_|DbError::Unavailable)).collect()
        }).await.map_err(db_error)
    }
    async fn unfinished(&self) -> Result<Vec<DownloadJob>> {
        self.db.run(move|c| {
            let mut q=c.prepare("SELECT document FROM jobs WHERE state NOT IN ('completed','failed','cancelled') OR finalize_phase IS NOT NULL AND state!='completed' ORDER BY created_at,id").map_err(|_|DbError::Unavailable)?;
            let rows=q.query_map([],|r|r.get::<_,String>(0)).map_err(|_|DbError::Unavailable)?;
            rows.map(|r|codec::decode(&r.map_err(|_|DbError::Unavailable)?).map_err(|_|DbError::Unavailable)).collect()
        }).await.map_err(db_error)
    }
    async fn save(&self, job: &DownloadJob, expected_generation: u64) -> Result<()> {
        let job = job.clone();
        let updated = self
            .db
            .run(move |c| persist(c, &job, expected_generation))
            .await
            .map_err(db_error)?;
        if updated {
            Ok(())
        } else {
            Err(DownloadErrorCode::InvalidTransition.into())
        }
    }
    async fn claim(&self, now: u64) -> Result<Option<DownloadJob>> {
        self.db.run(move|c| {
            let tx=c.transaction().map_err(|_|DbError::Unavailable)?;
            let id:Option<String>=tx.query_row("SELECT id FROM jobs WHERE state='queued' AND (retry_at IS NULL OR retry_at<=?1) ORDER BY created_at,id LIMIT 1",[integer(now)?],|r|r.get(0)).optional().map_err(|_|DbError::Unavailable)?;
            let Some(id)=id else{return Ok(None)};let mut job=read(&tx,&id)?;let generation=job.generation;
            job.generation=job.generation.checked_add(1).ok_or(DbError::Unavailable)?;
            job.state=DownloadState::Resolving;job.updated_at=now;
            if !persist(&tx,&job,generation)? {return Err(DbError::Unavailable);}
            tx.commit().map_err(|_|DbError::Unavailable)?;Ok(Some(job))
        }).await.map_err(db_error)
    }
    async fn catalog_matches(&self, hash: &Sha256) -> Result<Vec<(Destination, FileIdentity)>> {
        let hash = hash.as_str().to_owned();
        self.db
            .run(move |c| {
                let mut q = c
                    .prepare("SELECT meta_json,file_identity FROM models WHERE sha256=?1 AND root_id='models'")
                    .map_err(|_| DbError::Unavailable)?;
                let rows = q
                    .query_map([hash], |r| {
                        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                    })
                    .map_err(|_| DbError::Unavailable)?;
                rows.map(|r| {
                    let (d, i) = r.map_err(|_| DbError::Unavailable)?;
                    let d = serde_json::from_str(&d).map_err(|_| DbError::Unavailable)?;
                    let i = serde_json::from_str(&i).map_err(|_| DbError::Unavailable)?;
                    Ok((
                        codec::destination(&d).map_err(|_| DbError::Unavailable)?,
                        codec::identity(&i).map_err(|_| DbError::Unavailable)?,
                    ))
                })
                .collect()
            })
            .await
            .map_err(db_error)
    }
    async fn catalog_and_mark(&self, job: &DownloadJob) -> Result<DownloadJob> {
        let mut job = job.clone();
        self.db.run(move|c| {
            let tx=c.transaction().map_err(|_|DbError::Unavailable)?;
            let intent=job.publication.as_ref().ok_or(DbError::Unavailable)?;let identity=intent.published_identity.as_ref().ok_or(DbError::Unavailable)?;
            tx.execute("INSERT INTO models(root_id,path,category,size,mtime,file_identity,sha256,source_job_id,civitai_version_id,meta_json) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10) ON CONFLICT(root_id,path) DO UPDATE SET category=excluded.category,size=excluded.size,mtime=excluded.mtime,file_identity=excluded.file_identity,sha256=excluded.sha256,source_job_id=excluded.source_job_id,civitai_version_id=excluded.civitai_version_id,meta_json=excluded.meta_json",params![job.destination.root_id,job.destination.relative_path().as_string(),job.destination.category.as_str(),integer(identity.size)?,identity.modified_ns.to_string(),codec::identity_value(identity).to_string(),intent.digest.as_str(),job.id.as_str(),match &job.source {SourceRef::CivitAi {version_id,..}=>version_id.map(integer).transpose()?,_=>None},codec::destination_value(&job.destination).to_string()]).map_err(|_|DbError::Unavailable)?;
            job.publication.as_mut().unwrap().phase=FinalizePhase::Cataloged;
            if !persist(&tx,&job,job.generation)? {return Err(DbError::Unavailable);}
            tx.commit().map_err(|_|DbError::Unavailable)?;Ok(job)
        }).await.map_err(db_error)
    }
}
