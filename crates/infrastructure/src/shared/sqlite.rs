use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};

use rusqlite::Connection;
use rusqlite_migration::{M, Migrations};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum DbError {
    #[error("the database is unavailable")]
    Unavailable,
}

/// The single, ordered migration list for the app database. Contexts append to this list;
/// they never keep their own `PRAGMA user_version` sequences.
pub fn migrations() -> Migrations<'static> {
    Migrations::new(vec![
        M::up(include_str!("../../migrations/001_settings.sql")),
        M::up(include_str!("../../migrations/002_environment.sql")),
        M::up(include_str!("../../migrations/003_comfy_runtime.sql")),
        M::up(include_str!("../../migrations/004_downloads.sql")),
        M::up(include_str!("../../migrations/005_library.sql")),
        M::up(include_str!("../../migrations/006_custom_nodes.sql")),
        M::up(include_str!(
            "../../migrations/007_environment_components.sql"
        )),
        M::up(include_str!("../../migrations/008_install_relocation.sql")),
        M::up(include_str!("../../migrations/009_settings_language.sql")),
        M::up(include_str!(
            "../../migrations/010_node_operation_git_evidence.sql"
        )),
    ])
}

/// The shared SQLite connection. Work runs on blocking threads behind one mutex.
#[derive(Clone)]
pub struct Db {
    pub(crate) connection: Arc<Mutex<Connection>>,
}

impl Db {
    /// Reads an existing database without creating it, migrating it or changing its settings.
    pub async fn open_existing(path: impl AsRef<Path>) -> Result<Self, DbError> {
        let path = path.as_ref().to_owned();
        tokio::task::spawn_blocking(move || {
            let connection =
                Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                    .map_err(|_| DbError::Unavailable)?;
            connection
                .busy_timeout(Duration::from_secs(5))
                .map_err(|_| DbError::Unavailable)?;
            let _: u32 = connection
                .pragma_query_value(None, "user_version", |row| row.get(0))
                .map_err(|_| DbError::Unavailable)?;
            Ok(Self {
                connection: Arc::new(Mutex::new(connection)),
            })
        })
        .await
        .map_err(|_| DbError::Unavailable)?
    }

    pub async fn open(path: impl AsRef<Path>) -> Result<Self, DbError> {
        let path = path.as_ref().to_owned();
        tokio::task::spawn_blocking(move || {
            let mut connection = Connection::open(path).map_err(|_| DbError::Unavailable)?;
            connection
                .busy_timeout(Duration::from_secs(5))
                .map_err(|_| DbError::Unavailable)?;
            connection
                .pragma_update(None, "journal_mode", "WAL")
                .map_err(|_| DbError::Unavailable)?;
            connection
                .pragma_update(None, "foreign_keys", true)
                .map_err(|_| DbError::Unavailable)?;
            migrations()
                .to_latest(&mut connection)
                .map_err(|_| DbError::Unavailable)?;
            Ok(Self {
                connection: Arc::new(Mutex::new(connection)),
            })
        })
        .await
        .map_err(|_| DbError::Unavailable)?
    }

    /// Runs `work` on a blocking thread with exclusive access to the connection.
    pub(crate) async fn run<T, F>(&self, work: F) -> Result<T, DbError>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T, DbError> + Send + 'static,
    {
        let connection = self.connection.clone();
        tokio::task::spawn_blocking(move || {
            let mut connection = connection.lock().map_err(|_| DbError::Unavailable)?;
            work(&mut connection)
        })
        .await
        .map_err(|_| DbError::Unavailable)?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use application::settings::SettingsRepository;
    use domain::settings::{CloseBehavior, HfEngine, Language, Settings, Theme};

    use crate::settings::sqlite::SqliteSettingsRepository;

    #[tokio::test]
    async fn upgrades_an_m0_database_without_losing_settings() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("app.db");
        {
            // An M0 database: only migration 001 applied, with a stored setting.
            let mut connection = Connection::open(&path).unwrap();
            Migrations::new(vec![M::up(include_str!(
                "../../migrations/001_settings.sql"
            ))])
            .to_latest(&mut connection)
            .unwrap();
            connection
                .execute(
                    "INSERT INTO settings(key, value, updated_at) VALUES ('theme', 'dark', 't')",
                    [],
                )
                .unwrap();
        }
        let db = Db::open(&path).await.unwrap();
        db.run(|connection| {
            let version: u32 = connection
                .pragma_query_value(None, "user_version", |row| row.get(0))
                .unwrap();
            assert_eq!(version, 10);
            let theme: String = connection
                .query_row(
                    "SELECT value FROM settings WHERE key = 'theme'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(theme, "dark");
            for table in ["envs", "provisioning_runs", "wizard_draft"] {
                let count: u32 = connection
                    .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                        row.get(0)
                    })
                    .unwrap();
                assert_eq!(count, 0);
            }
            Ok(())
        })
        .await
        .unwrap();
        drop(db);
        // Reopening an up-to-date database is a no-op.
        Db::open(&path).await.unwrap();
    }

    #[tokio::test]
    async fn upgrades_pre_language_settings_with_english_and_preserves_values() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("app.db");
        {
            let mut connection = Connection::open(&path).unwrap();
            migrations().to_version(&mut connection, 8).unwrap();
            for (key, value) in [
                ("download_concurrency", "7"),
                ("hf_engine", "native"),
                ("close_behavior", "keep_running_headless"),
                ("theme", "dark"),
                ("registry_cache_ttl", "24"),
                ("bandwidth_cap_kib_s", "1024"),
            ] {
                connection
                    .execute(
                        "INSERT INTO settings(key, value, updated_at) VALUES (?1, ?2, 't')",
                        rusqlite::params![key, value],
                    )
                    .unwrap();
            }
        }
        let repository = SqliteSettingsRepository::new(Db::open(&path).await.unwrap());
        let expected = Settings::new(
            7,
            HfEngine::Native,
            CloseBehavior::KeepRunningHeadless,
            Theme::Dark,
            Language::En,
            24,
            Some(1024),
        )
        .unwrap();
        assert_eq!(repository.load().await.unwrap(), Some(expected));
    }

    #[tokio::test]
    async fn language_migration_keeps_an_empty_database_empty() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("app.db");
        {
            let mut connection = Connection::open(&path).unwrap();
            migrations().to_version(&mut connection, 8).unwrap();
        }
        let repository = SqliteSettingsRepository::new(Db::open(&path).await.unwrap());
        assert_eq!(repository.load().await.unwrap(), None);
    }

    #[test]
    fn language_migration_preserves_existing_thai_on_reapplication() {
        let mut connection = Connection::open_in_memory().unwrap();
        migrations().to_version(&mut connection, 8).unwrap();
        connection
            .execute(
                "INSERT INTO settings(key, value, updated_at) VALUES ('language', 'th', 't')",
                [],
            )
            .unwrap();
        migrations().to_latest(&mut connection).unwrap();
        connection
            .execute_batch(include_str!("../../migrations/009_settings_language.sql"))
            .unwrap();
        let language: String = connection
            .query_row(
                "SELECT value FROM settings WHERE key = 'language'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(language, "th");
    }

    #[tokio::test]
    async fn at_most_one_environment_may_be_active() {
        let directory = tempfile::tempdir().unwrap();
        let db = Db::open(directory.path().join("app.db")).await.unwrap();
        db.run(|connection| {
            let insert = |id: &str, active: i32| {
                connection.execute(
                    "INSERT INTO envs(id, path, owned, source, python_version, profile_id, \
                     profile_revision, pins_json, authorized_location, state, created_at, \
                     updated_at, active) VALUES (?1, 'p', 1, 'managed', '3.13.16', 'x', 1, '{}', \
                     'p', 'verified', 't', 't', ?2)",
                    rusqlite::params![id, active],
                )
            };
            insert("a", 1).unwrap();
            insert("b", 0).unwrap();
            insert("c", 0).unwrap();
            assert!(
                insert("d", 1).is_err(),
                "a second active row must be rejected"
            );
            Ok(())
        })
        .await
        .unwrap();
    }
}
