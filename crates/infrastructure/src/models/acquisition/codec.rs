//! Persistence DTO conversion. No transport URL or credential is serialized here.
use application::models::acquisition::{DownloadError, Result};
use domain::{
    models::{acquisition::*, library::ModelCategory},
    shared::model_values::{JobId, RelativeModelPath, SafeFileName, Sha256},
};
use serde_json::{Value, json};

macro_rules! string_enum {
    ($decode:ident,$ty:ty,[$($variant:ident),+])=>{
        fn $decode(s:&str)->Result<$ty> { match s { $(stringify!($variant)=>Ok(<$ty>::$variant),)+ _=>Err(DownloadErrorCode::Storage.into()) } }
    }
}
string_enum!(engine, DownloadEngine, [Native, HfCli]);
string_enum!(
    state,
    DownloadState,
    [
        Queued,
        Resolving,
        Downloading,
        Verifying,
        Paused,
        Completed,
        Failed,
        Cancelled
    ]
);
string_enum!(phase, FinalizePhase, [Verified, Published, Cataloged]);
string_enum!(
    error,
    DownloadErrorCode,
    [
        InvalidSource,
        InvalidMetadata,
        SelectionRequired,
        SelectionNotFound,
        TokenRequired,
        InvalidToken,
        EarlyAccessOrPaid,
        GatedLicense,
        RepositoryUnavailable,
        FileNotFound,
        RevisionNotFound,
        NotFound,
        DriveQuota,
        DrivePermission,
        DriveFolderUnsupported,
        UnexpectedConfirmationPage,
        HttpConfirmationRequired,
        SourceRefreshRequired,
        Network,
        Service,
        HttpClient,
        SignedUrlExpired,
        Disk,
        HashMismatch,
        LengthMismatch,
        InvalidRange,
        ToolLaunch,
        ToolRuntime,
        ProcessCleanupFailed,
        NativeUnsupportedUseHf,
        Conflict,
        StaleDecision,
        Containment,
        TrashFailed,
        Cancelled,
        Storage,
        InvalidTransition,
        SubscriptionGap
    ]
);
fn bad() -> DownloadError {
    DownloadErrorCode::Storage.into()
}
fn text<'a>(v: &'a Value, k: &str) -> Result<&'a str> {
    v.get(k).and_then(Value::as_str).ok_or_else(bad)
}
fn number(v: &Value, k: &str) -> Result<u64> {
    v.get(k).and_then(Value::as_u64).ok_or_else(bad)
}
fn hash(v: &Value) -> Result<Option<Sha256>> {
    if v.is_null() {
        Ok(None)
    } else {
        Sha256::new(v.as_str().ok_or_else(bad)?)
            .map(Some)
            .map_err(|_| bad())
    }
}
pub fn source_value(s: &SourceRef) -> Value {
    match s {
        SourceRef::CivitAi {
            model_id,
            version_id,
            file_id,
            filters,
        } => {
            json!({"kind":"civitai","model":model_id,"version":version_id,"file":file_id,"type":filters.file_type,"format":filters.format,"size":filters.size,"fp":filters.fp})
        }
        SourceRef::HuggingFace {
            repo_type,
            repo,
            revision,
            path,
        } => {
            json!({"kind":"hf","repo_type":repo_type.singular(),"repo":repo,"revision":revision,"path":path.as_ref().map(RelativeModelPath::as_string)})
        }
        SourceRef::GoogleDrive { file_id } => json!({"kind":"drive","file":file_id}),
        SourceRef::Direct {
            locator,
            ingress_id,
            requires_source_refresh,
            insecure_http_confirmed,
        } => {
            json!({"kind":"direct","locator":locator,"ingress_id":ingress_id,"requires_source_refresh":requires_source_refresh,"http_confirmed":insecure_http_confirmed})
        }
    }
}
fn source(v: &Value) -> Result<SourceRef> {
    let optional = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_owned);
    Ok(match text(v, "kind")? {
        "civitai" => SourceRef::CivitAi {
            model_id: v["model"].as_u64(),
            version_id: v["version"].as_u64(),
            file_id: v["file"].as_u64(),
            filters: CivitFilters {
                file_type: optional("type"),
                format: optional("format"),
                size: optional("size"),
                fp: optional("fp"),
            },
        },
        "hf" => SourceRef::HuggingFace {
            repo_type: match text(v, "repo_type")? {
                "model" => RepoType::Model,
                "dataset" => RepoType::Dataset,
                "space" => RepoType::Space,
                _ => return Err(bad()),
            },
            repo: text(v, "repo")?.into(),
            revision: text(v, "revision")?.into(),
            path: optional("path")
                .map(|s| RelativeModelPath::new(&s))
                .transpose()
                .map_err(|_| bad())?,
        },
        "drive" => SourceRef::GoogleDrive {
            file_id: text(v, "file")?.into(),
        },
        "direct" => {
            let locator = text(v, "locator")?;
            let url = url::Url::parse(locator).map_err(|_| bad())?;
            if url.query().is_some() || url.password().is_some() || !url.username().is_empty() {
                return Err(bad());
            }
            SourceRef::Direct {
                locator: locator.into(),
                ingress_id: v["ingress_id"].as_u64(),
                requires_source_refresh: v["requires_source_refresh"].as_bool().unwrap_or(false),
                insecure_http_confirmed: v["http_confirmed"].as_bool().unwrap_or(false),
            }
        }
        _ => return Err(bad()),
    })
}
pub fn destination_value(d: &Destination) -> Value {
    json!({"root":d.root_id,"category":d.category.as_str(),"subdirectory":d.subdirectory.as_string(),"filename":d.filename.as_str()})
}
pub fn destination(v: &Value) -> Result<Destination> {
    Ok(Destination {
        root_id: text(v, "root")?.into(),
        category: ModelCategory::new(text(v, "category")?).map_err(|_| bad())?,
        subdirectory: RelativeModelPath::new(text(v, "subdirectory")?).map_err(|_| bad())?,
        filename: SafeFileName::new(text(v, "filename")?).map_err(|_| bad())?,
    })
}
pub fn identity_value(i: &FileIdentity) -> Value {
    json!({"size":i.size,"modified_ns":i.modified_ns.to_string(),"platform_id":i.platform_id,"digest":i.digest.as_ref().map(Sha256::as_str)})
}
pub fn identity(v: &Value) -> Result<FileIdentity> {
    Ok(FileIdentity {
        size: number(v, "size")?,
        modified_ns: text(v, "modified_ns")?.parse().map_err(|_| bad())?,
        platform_id: text(v, "platform_id")?.into(),
        digest: hash(&v["digest"])?,
    })
}
fn size_value(s: DownloadSize) -> Value {
    match s {
        DownloadSize::Exact(n) => json!({"kind":"exact","bytes":n}),
        DownloadSize::Estimated(n) => json!({"kind":"estimated","bytes":n}),
        DownloadSize::Unknown => json!({"kind":"unknown"}),
    }
}
fn size(v: &Value) -> Result<DownloadSize> {
    Ok(match text(v, "kind")? {
        "exact" => DownloadSize::Exact(number(v, "bytes")?),
        "estimated" => DownloadSize::Estimated(number(v, "bytes")?),
        "unknown" => DownloadSize::Unknown,
        _ => return Err(bad()),
    })
}
pub fn encode(j: &DownloadJob) -> Result<String> {
    let conflict = match &j.conflict {
        None => Value::Null,
        Some(ConflictChoice::KeepBoth) => json!({"choice":"keep_both"}),
        Some(ConflictChoice::Skip) => json!({"choice":"skip"}),
        Some(ConflictChoice::Overwrite { approved }) => {
            json!({"choice":"overwrite","approved":identity_value(approved)})
        }
    };
    let publication=j.publication.as_ref().map(|p|json!({"phase":format!("{:?}",p.phase),"staged_key":p.staged_key,"staged_identity":identity_value(&p.staged_identity),"digest":p.digest.as_str(),"external_verified":p.external_verified,"backup_name":p.backup_name.as_str(),"published_identity":p.published_identity.as_ref().map(identity_value)}));
    serde_json::to_string(&json!({"id":j.id.as_str(),"source":source_value(&j.source),"identity":j.identity.0,"destination":destination_value(&j.destination),"state":format!("{:?}",j.state),"requested_engine":format!("{:?}",j.requested_engine),"effective_engine":format!("{:?}",j.effective_engine),"generation":j.generation,"size":size_value(j.size),"expected_sha256":j.expected_sha256.as_ref().map(Sha256::as_str),"bytes_done":j.bytes_done,"validator":j.validator.as_ref().map(|ResumeValidator::StrongEtag(s)|s),"retry_count":j.retry_count,"retry_at":j.retry_at,"stop_intent":j.stop_intent.map(|s|format!("{s:?}")),"error":j.error.map(|e|format!("{e:?}")),"conflict":conflict,"decision_id":j.decision_id,"observed":j.observed.as_ref().map(identity_value),"publication":publication,"created_at":j.created_at,"updated_at":j.updated_at})).map_err(|_|bad())
}
pub fn decode(raw: &str) -> Result<DownloadJob> {
    let v: Value = serde_json::from_str(raw).map_err(|_| bad())?;
    let publication = if v["publication"].is_null() {
        None
    } else {
        let p = &v["publication"];
        Some(PublicationIntent {
            phase: phase(text(p, "phase")?)?,
            staged_key: text(p, "staged_key")?.into(),
            staged_identity: identity(&p["staged_identity"])?,
            digest: hash(&p["digest"])?.ok_or_else(bad)?,
            external_verified: p["external_verified"].as_bool().ok_or_else(bad)?,
            backup_name: SafeFileName::new(text(p, "backup_name")?).map_err(|_| bad())?,
            published_identity: if p["published_identity"].is_null() {
                None
            } else {
                Some(identity(&p["published_identity"])?)
            },
        })
    };
    let conflict = if v["conflict"].is_null() {
        None
    } else {
        Some(match text(&v["conflict"], "choice")? {
            "keep_both" => ConflictChoice::KeepBoth,
            "skip" => ConflictChoice::Skip,
            "overwrite" => ConflictChoice::Overwrite {
                approved: identity(&v["conflict"]["approved"])?,
            },
            _ => return Err(bad()),
        })
    };
    Ok(DownloadJob {
        id: JobId::new(text(&v, "id")?).map_err(|_| bad())?,
        source: source(&v["source"])?,
        identity: ProviderIdentity(text(&v, "identity")?.into()),
        destination: destination(&v["destination"])?,
        state: state(text(&v, "state")?)?,
        requested_engine: engine(text(&v, "requested_engine")?)?,
        effective_engine: engine(text(&v, "effective_engine")?)?,
        generation: number(&v, "generation")?,
        size: size(&v["size"])?,
        expected_sha256: hash(&v["expected_sha256"])?,
        bytes_done: number(&v, "bytes_done")?,
        validator: v["validator"]
            .as_str()
            .map(|s| ResumeValidator::StrongEtag(s.into())),
        retry_count: u8::try_from(number(&v, "retry_count")?).map_err(|_| bad())?,
        retry_at: v["retry_at"].as_u64(),
        stop_intent: v["stop_intent"]
            .as_str()
            .map(|s| match s {
                "Pause" => Ok(StopIntent::Pause),
                "Cancel" => Ok(StopIntent::Cancel),
                _ => Err(bad()),
            })
            .transpose()?,
        error: v["error"].as_str().map(error).transpose()?,
        conflict,
        decision_id: v["decision_id"].as_str().map(str::to_owned),
        observed: if v["observed"].is_null() {
            None
        } else {
            Some(identity(&v["observed"])?)
        },
        publication,
        created_at: number(&v, "created_at")?,
        updated_at: number(&v, "updated_at")?,
    })
}
