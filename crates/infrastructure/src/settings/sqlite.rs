use std::collections::BTreeMap;

use application::settings::{SettingsRepoError, SettingsRepository};
use async_trait::async_trait;
use domain::settings::Settings;
use rusqlite::params;

pub use crate::shared::sqlite::Db;

#[derive(Clone)]
pub struct SqliteSettingsRepository {
    db: Db,
}

impl SqliteSettingsRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

fn invalid(reason: &'static str) -> SettingsRepoError {
    SettingsRepoError::InvalidPersisted(reason.into())
}

fn decode(values: BTreeMap<String, String>) -> Result<Option<Settings>, SettingsRepoError> {
    if values.is_empty() {
        return Ok(None);
    }
    let get = |key: &str| {
        values
            .get(key)
            .map(String::as_str)
            .ok_or_else(|| invalid("incomplete settings"))
    };
    let concurrency = get("download_concurrency")?
        .parse()
        .map_err(|_| invalid("invalid download concurrency"))?;
    let engine = get("hf_engine")?
        .parse()
        .map_err(|_| invalid("invalid Hugging Face engine"))?;
    let close = get("close_behavior")?
        .parse()
        .map_err(|_| invalid("invalid close behavior"))?;
    let theme = get("theme")?
        .parse()
        .map_err(|_| invalid("invalid theme"))?;
    let ttl = get("registry_cache_ttl")?
        .parse()
        .map_err(|_| invalid("invalid registry cache TTL"))?;
    let cap = match get("bandwidth_cap_kib_s")? {
        "unlimited" => None,
        value => Some(
            value
                .parse()
                .map_err(|_| invalid("invalid bandwidth cap"))?,
        ),
    };
    Settings::new(concurrency, engine, close, theme, ttl, cap)
        .map(Some)
        .map_err(|_| invalid("settings violate domain validation"))
}

#[async_trait]
impl SettingsRepository for SqliteSettingsRepository {
    async fn load(&self) -> Result<Option<Settings>, SettingsRepoError> {
        let connection = self.db.connection.clone();
        tokio::task::spawn_blocking(move || {
            let connection = connection
                .lock()
                .map_err(|_| SettingsRepoError::Unavailable)?;
            let mut statement = connection
                .prepare("SELECT key, value FROM settings")
                .map_err(|_| SettingsRepoError::Unavailable)?;
            let rows = statement
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(|_| SettingsRepoError::Unavailable)?;
            let values = rows
                .collect::<Result<BTreeMap<_, _>, _>>()
                .map_err(|_| invalid("invalid settings row"))?;
            decode(values)
        })
        .await
        .map_err(|_| SettingsRepoError::Unavailable)?
    }

    async fn save(&self, settings: &Settings) -> Result<(), SettingsRepoError> {
        let connection = self.db.connection.clone();
        let settings = settings.clone();
        tokio::task::spawn_blocking(move || {
            let mut connection = connection.lock().map_err(|_| SettingsRepoError::Unavailable)?;
            let transaction = connection.transaction().map_err(|_| SettingsRepoError::Unavailable)?;
            let values = [
                ("download_concurrency", settings.download_concurrency.get().to_string()),
                ("hf_engine", settings.hf_engine.as_str().into()),
                ("close_behavior", settings.close_behavior.as_str().into()),
                ("theme", settings.theme.as_str().into()),
                ("registry_cache_ttl", settings.registry_cache_ttl.get().to_string()),
                ("bandwidth_cap_kib_s", settings.bandwidth_cap_kib_s.map(|value| value.get().to_string()).unwrap_or_else(|| "unlimited".into())),
            ];
            for (key, value) in values {
                transaction.execute(
                    "INSERT INTO settings(key, value, updated_at) VALUES (?1, ?2, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                     ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
                    params![key, value],
                ).map_err(|_| SettingsRepoError::Unavailable)?;
            }
            transaction.commit().map_err(|_| SettingsRepoError::Unavailable)
        }).await.map_err(|_| SettingsRepoError::Unavailable)?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::settings::{CloseBehavior, HfEngine, Theme};

    #[tokio::test]
    async fn fresh_migration_sets_schema_and_connection_pragmas() {
        let directory = tempfile::tempdir().unwrap();
        let db = Db::open(directory.path().join("app.db")).await.unwrap();
        let connection = db.connection.lock().unwrap();
        let version: u32 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        let wal: String = connection
            .pragma_query_value(None, "journal_mode", |row| row.get(0))
            .unwrap();
        let foreign_keys: bool = connection
            .pragma_query_value(None, "foreign_keys", |row| row.get(0))
            .unwrap();
        let timeout: u32 = connection
            .pragma_query_value(None, "busy_timeout", |row| row.get(0))
            .unwrap();
        let latest = std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/migrations"))
            .unwrap()
            .filter(|entry| {
                entry
                    .as_ref()
                    .is_ok_and(|e| e.path().extension().is_some_and(|x| x == "sql"))
            })
            .count();
        assert_eq!(version as usize, latest);
        assert_eq!(wal, "wal");
        assert!(foreign_keys);
        assert_eq!(timeout, 5000);
    }

    #[tokio::test]
    async fn empty_round_trip_and_reopen() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("app.db");
        let repository = SqliteSettingsRepository::new(Db::open(&path).await.unwrap());
        assert_eq!(repository.load().await.unwrap(), None);
        repository.save(&Settings::default()).await.unwrap();
        assert_eq!(repository.load().await.unwrap(), Some(Settings::default()));
        let settings = Settings::new(
            u32::MAX,
            HfEngine::Native,
            CloseBehavior::KeepRunningHeadless,
            Theme::Dark,
            u32::MAX,
            Some(1024),
        )
        .unwrap();
        repository.save(&settings).await.unwrap();
        drop(repository);
        let reopened = SqliteSettingsRepository::new(Db::open(&path).await.unwrap());
        assert_eq!(reopened.load().await.unwrap(), Some(settings));
    }

    #[tokio::test]
    async fn invalid_persisted_values_and_incomplete_rows_are_not_reset() {
        let directory = tempfile::tempdir().unwrap();
        let db = Db::open(directory.path().join("app.db")).await.unwrap();
        let repository = SqliteSettingsRepository::new(db.clone());
        for (key, value) in [
            ("download_concurrency", "0"),
            ("registry_cache_ttl", "0"),
            ("bandwidth_cap_kib_s", "0"),
            ("hf_engine", "private-invalid-value"),
            ("close_behavior", "private-invalid-value"),
            ("theme", "private-invalid-value"),
            ("download_concurrency", "4294967296"),
        ] {
            repository.save(&Settings::default()).await.unwrap();
            db.connection
                .lock()
                .unwrap()
                .execute(
                    "UPDATE settings SET value = ?1 WHERE key = ?2",
                    params![value, key],
                )
                .unwrap();
            let error = repository.load().await.unwrap_err();
            assert!(matches!(error, SettingsRepoError::InvalidPersisted(_)));
            assert!(!error.to_string().contains(value));
        }
        db.connection
            .lock()
            .unwrap()
            .execute("DELETE FROM settings WHERE key = 'theme'", [])
            .unwrap();
        assert!(matches!(
            repository.load().await,
            Err(SettingsRepoError::InvalidPersisted(_))
        ));
    }

    #[tokio::test]
    async fn failed_save_rolls_back_all_previous_updates() {
        let directory = tempfile::tempdir().unwrap();
        let db = Db::open(directory.path().join("app.db")).await.unwrap();
        let repository = SqliteSettingsRepository::new(db.clone());
        repository.save(&Settings::default()).await.unwrap();
        db.connection
            .lock()
            .unwrap()
            .execute_batch(
                "CREATE TRIGGER reject_theme BEFORE UPDATE ON settings WHEN NEW.key = 'theme'
             BEGIN SELECT RAISE(ABORT, 'injected failure'); END;",
            )
            .unwrap();
        let settings = Settings::new(
            99,
            HfEngine::Native,
            CloseBehavior::StopComfy,
            Theme::Dark,
            99,
            Some(100),
        )
        .unwrap();
        assert_eq!(
            repository.save(&settings).await,
            Err(SettingsRepoError::Unavailable)
        );
        assert_eq!(repository.load().await.unwrap(), Some(Settings::default()));
    }
}
