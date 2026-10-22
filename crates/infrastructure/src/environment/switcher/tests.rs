use application::{
    environment::RootSource,
    settings::{SettingsRepoError, SettingsRepository},
};
use domain::settings::{CloseBehavior, HfEngine, Language, Settings, Theme};

use super::*;
use crate::shared::paths::read_bootstrap;

struct FixedSettings(Result<Option<Settings>, SettingsRepoError>);

#[async_trait]
impl SettingsRepository for FixedSettings {
    async fn load(&self) -> Result<Option<Settings>, SettingsRepoError> {
        self.0.clone()
    }

    async fn save(&self, _settings: &Settings) -> Result<(), SettingsRepoError> {
        Ok(())
    }
}

fn settings() -> Settings {
    Settings::new(
        7,
        HfEngine::Native,
        CloseBehavior::StopComfy,
        Theme::Dark,
        Language::Th,
        12,
        Some(2048),
    )
    .unwrap()
}

struct Fixture {
    directory: tempfile::TempDir,
    current: PathBuf,
    pointer: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let current = directory.path().join("current root");
        fs::create_dir_all(&current).unwrap();
        let pointer = directory.path().join("config").join("bootstrap.json");
        Self {
            directory,
            current,
            pointer,
        }
    }

    fn switcher(
        &self,
        source: RootSource,
        loaded: Result<Option<Settings>, SettingsRepoError>,
    ) -> FsRootSwitcher {
        FsRootSwitcher::new(
            RootInfo {
                root: self.current.clone(),
                source,
            },
            self.pointer.clone(),
            Arc::new(FixedSettings(loaded)),
        )
    }

    fn target(&self, name: &str) -> PathBuf {
        self.directory.path().join(name)
    }
}

#[tokio::test]
async fn switching_creates_the_new_database_transfers_data_and_publishes_the_pointer_last() {
    let fixture = Fixture::new();
    let target = fixture.target("new root ไทย");
    fixture
        .switcher(RootSource::Default, Ok(Some(settings())))
        .switch_root(&target, Some("{\"profile\":\"x\"}"))
        .await
        .unwrap();

    let paths = AppPaths::from_root(normalize_path(&target));
    assert!(paths.db_path.is_file());
    let db = Db::open(&paths.db_path).await.unwrap();
    assert_eq!(
        SqliteSettingsRepository::new(db.clone())
            .load()
            .await
            .unwrap(),
        Some(settings()),
        "settings are transferred through the repository, not by copying the open database"
    );
    assert_eq!(
        WizardDraftStore::load(&SqliteEnvironmentStore::new(db))
            .await
            .unwrap()
            .as_deref(),
        Some("{\"profile\":\"x\"}")
    );
    assert_eq!(
        read_bootstrap(&fixture.pointer).unwrap(),
        Some(normalize_path(&target))
    );
    // The current root is untouched.
    assert_eq!(fs::read_dir(&fixture.current).unwrap().count(), 0);
}

#[tokio::test]
async fn the_debug_override_keeps_the_root_fixed() {
    let fixture = Fixture::new();
    let target = fixture.target("elsewhere");
    let error = fixture
        .switcher(RootSource::DebugOverride, Ok(None))
        .switch_root(&target, None)
        .await
        .unwrap_err();
    assert_eq!(error, RootSwitchError::FixedByOverride);
    assert!(!target.exists());
    assert!(!fixture.pointer.exists());
}

#[tokio::test]
async fn invalid_destinations_create_nothing_and_publish_nothing() {
    let fixture = Fixture::new();
    let switcher = fixture.switcher(RootSource::Default, Ok(None));
    let busy = fixture.target("busy");
    fs::create_dir_all(&busy).unwrap();
    fs::write(busy.join("notes.txt"), b"keep").unwrap();
    let file = fixture.target("a file");
    fs::write(&file, b"x").unwrap();
    for (name, candidate) in [
        ("relative", PathBuf::from("relative/path")),
        ("same", fixture.current.clone()),
        ("inside", fixture.current.join("inner")),
        ("around", fixture.directory.path().to_owned()),
        ("non-empty", busy.clone()),
        ("file", file.clone()),
        (
            "root",
            if cfg!(windows) {
                PathBuf::from(r"C:\")
            } else {
                PathBuf::from("/")
            },
        ),
    ] {
        let result = switcher.switch_root(&candidate, None).await;
        assert!(
            matches!(result, Err(RootSwitchError::InvalidDestination(_))),
            "{name}: {result:?}"
        );
    }
    assert!(!fixture.pointer.exists(), "nothing was published");
    assert_eq!(fs::read(busy.join("notes.txt")).unwrap(), b"keep");
    assert!(
        !busy.join("data").exists(),
        "a rejected folder is never modified"
    );
}

#[tokio::test]
async fn an_existing_install_or_an_empty_folder_is_accepted() {
    let fixture = Fixture::new();
    let switcher = fixture.switcher(RootSource::Default, Ok(None));
    let empty = fixture.target("empty");
    fs::create_dir_all(&empty).unwrap();
    switcher.switch_root(&empty, None).await.unwrap();
    assert_eq!(
        read_bootstrap(&fixture.pointer).unwrap(),
        Some(normalize_path(&empty))
    );

    let previous = fixture.target("previous install");
    fs::create_dir_all(previous.join("data")).unwrap();
    fs::write(previous.join("install.json"), b"{}").unwrap();
    switcher.switch_root(&previous, None).await.unwrap();
    assert_eq!(
        read_bootstrap(&fixture.pointer).unwrap(),
        Some(normalize_path(&previous))
    );
}

#[tokio::test]
async fn an_unreadable_settings_store_aborts_before_the_pointer_is_published() {
    let fixture = Fixture::new();
    let target = fixture.target("new root");
    let error = fixture
        .switcher(
            RootSource::Default,
            Err(SettingsRepoError::InvalidPersisted("bad".into())),
        )
        .switch_root(&target, None)
        .await
        .unwrap_err();
    assert_eq!(error, RootSwitchError::Failed);
    assert!(!fixture.pointer.exists(), "the pointer is published last");
}
