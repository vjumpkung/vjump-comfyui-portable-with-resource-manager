use std::{
    fs,
    io::{self, Write},
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use application::environment::{ManagerConfigOutcome, ManagerConfigWriter, ProvisionError};
use async_trait::async_trait;
use domain::shared::model_values::SafeFileName;

use crate::shared::model_paths::{pin_absolute, rename_no_replace};

const DEFAULT_CONFIG: &str = concat!(
    "[default]\n",
    "preview_method = auto\n",
    "git_exe = \n",
    "use_uv = True\n",
    "channel_url = https://raw.githubusercontent.com/ltdrdata/ComfyUI-Manager/main\n",
    "share_option = all\n",
    "bypass_ssl = False\n",
    "file_logging = True\n",
    "component_policy = workflow\n",
    "update_policy = nightly-comfyui\n",
    "windows_selector_event_loop_policy = False\n",
    "model_download_by_agent = False\n",
    "downgrade_blacklist = \n",
    "security_level = weak\n",
    "skip_migration_check = True\n",
    "always_lazy_install = False\n",
    "network_mode = public\n",
    "db_mode = cache\n",
);

pub struct FsManagerConfigWriter;

#[async_trait]
impl ManagerConfigWriter for FsManagerConfigWriter {
    async fn ensure_manager_config(
        &self,
        install_root: &Path,
    ) -> Result<ManagerConfigOutcome, ProvisionError> {
        let root = install_root.to_owned();
        tokio::task::spawn_blocking(move || ensure(&root))
            .await
            .map_err(config_error)?
            .map_err(config_error)
    }
}

fn config_error(_: impl std::fmt::Display) -> ProvisionError {
    ProvisionError::Failed("ComfyUI-Manager configuration could not be created safely".into())
}

fn ensure(root: &Path) -> io::Result<ManagerConfigOutcome> {
    let manager = root.join("ComfyUI/custom_nodes/ComfyUI-Manager");
    match fs::symlink_metadata(manager) {
        Ok(metadata) if metadata.is_dir() && !is_link(&metadata) => {}
        Ok(_) => return Ok(ManagerConfigOutcome::Absent),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(ManagerConfigOutcome::Absent);
        }
        Err(error) => return Err(error),
    }

    let mut directory = pin_absolute(root)?;
    for (part, create) in [("ComfyUI", false), ("user", true), ("__manager", true)] {
        let name = SafeFileName::new(part).expect("valid manager configuration directory");
        directory = directory.child(&name, create)?;
    }
    let name = SafeFileName::new("config.ini").expect("valid manager configuration filename");
    match fs::symlink_metadata(directory.anchored_path().join(name.as_str())) {
        Ok(metadata) if metadata.is_file() && !is_link(&metadata) => {
            return Ok(ManagerConfigOutcome::Kept);
        }
        Ok(_) => return Err(io::ErrorKind::PermissionDenied.into()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temp_name = SafeFileName::new(format!(
        ".config.ini.vjump-{}-{nanos}.tmp",
        std::process::id()
    ))
    .expect("valid temporary manager configuration filename");
    let mut file = directory.open(&temp_name, true, true)?;
    let result = (|| {
        file.write_all(DEFAULT_CONFIG.as_bytes())?;
        file.sync_all()?;
        rename_no_replace(&directory, &temp_name, &file, &directory, &name)
    })();
    drop(file);
    match result {
        Ok(()) => Ok(ManagerConfigOutcome::Created),
        Err(error) => {
            directory.remove_owned_file(&temp_name)?;
            if error.kind() == io::ErrorKind::AlreadyExists {
                Ok(ManagerConfigOutcome::Kept)
            } else {
                Err(error)
            }
        }
    }
}

fn is_link(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return true;
        }
    }
    metadata.file_type().is_symlink()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn installed_manager() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("ComfyUI/custom_nodes/ComfyUI-Manager")).unwrap();
        root
    }

    #[tokio::test]
    async fn absent_manager_does_not_create_configuration_directories() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(
            FsManagerConfigWriter
                .ensure_manager_config(root.path())
                .await,
            Ok(ManagerConfigOutcome::Absent)
        );
        assert!(!root.path().join("ComfyUI/user").exists());
    }

    #[tokio::test]
    async fn creates_missing_configuration_with_exact_default_bytes() {
        let root = installed_manager();
        assert_eq!(
            FsManagerConfigWriter
                .ensure_manager_config(root.path())
                .await,
            Ok(ManagerConfigOutcome::Created)
        );
        assert_eq!(
            fs::read(root.path().join("ComfyUI/user/__manager/config.ini")).unwrap(),
            DEFAULT_CONFIG.as_bytes()
        );
    }

    #[tokio::test]
    async fn keeps_existing_configuration_unchanged() {
        let root = installed_manager();
        let directory = root.path().join("ComfyUI/user/__manager");
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("config.ini");
        fs::write(&path, b"user configuration\r\n").unwrap();
        assert_eq!(
            FsManagerConfigWriter
                .ensure_manager_config(root.path())
                .await,
            Ok(ManagerConfigOutcome::Kept)
        );
        assert_eq!(fs::read(path).unwrap(), b"user configuration\r\n");
    }

    #[tokio::test]
    async fn repeated_ensure_keeps_created_configuration() {
        let root = installed_manager();
        let writer = FsManagerConfigWriter;
        assert_eq!(
            writer.ensure_manager_config(root.path()).await,
            Ok(ManagerConfigOutcome::Created)
        );
        assert_eq!(
            writer.ensure_manager_config(root.path()).await,
            Ok(ManagerConfigOutcome::Kept)
        );
    }

    #[tokio::test]
    async fn rejects_directory_at_configuration_filename() {
        let root = installed_manager();
        fs::create_dir_all(root.path().join("ComfyUI/user/__manager/config.ini")).unwrap();
        assert!(matches!(
            FsManagerConfigWriter
                .ensure_manager_config(root.path())
                .await,
            Err(ProvisionError::Failed(_))
        ));
    }

    #[tokio::test]
    async fn successful_creation_leaves_no_temporary_files() {
        let root = installed_manager();
        FsManagerConfigWriter
            .ensure_manager_config(root.path())
            .await
            .unwrap();
        let names: Vec<_> = fs::read_dir(root.path().join("ComfyUI/user/__manager"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, [std::ffi::OsString::from("config.ini")]);
    }

    #[cfg(windows)]
    fn directory_link(target: &Path, link: &Path) -> bool {
        std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .is_ok_and(|output| output.status.success())
    }

    #[cfg(unix)]
    fn directory_link(target: &Path, link: &Path) -> bool {
        std::os::unix::fs::symlink(target, link).unwrap();
        true
    }

    #[tokio::test]
    async fn linked_manager_is_not_installed() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("outside");
        fs::create_dir(&target).unwrap();
        let parent = root.path().join("ComfyUI/custom_nodes");
        fs::create_dir_all(&parent).unwrap();
        if !directory_link(&target, &parent.join("ComfyUI-Manager")) {
            eprintln!("skipped: directory links cannot be created here");
            return;
        }
        assert_eq!(
            FsManagerConfigWriter
                .ensure_manager_config(root.path())
                .await,
            Ok(ManagerConfigOutcome::Absent)
        );
        assert!(!root.path().join("ComfyUI/user").exists());
    }

    #[tokio::test]
    async fn rejects_links_in_configuration_path() {
        for relative in [
            "ComfyUI",
            "ComfyUI/user",
            "ComfyUI/user/__manager",
            "ComfyUI/user/__manager/config.ini",
        ] {
            let root = tempfile::tempdir().unwrap();
            let target = root.path().join("outside");
            fs::create_dir(&target).unwrap();
            let comfyui = if relative == "ComfyUI" {
                target.clone()
            } else {
                root.path().join("ComfyUI")
            };
            fs::create_dir_all(comfyui.join("custom_nodes/ComfyUI-Manager")).unwrap();
            let link = root.path().join(relative);
            fs::create_dir_all(link.parent().unwrap()).unwrap();
            if !directory_link(&target, &link) {
                eprintln!("skipped: directory links cannot be created here");
                return;
            }
            assert!(matches!(
                FsManagerConfigWriter
                    .ensure_manager_config(root.path())
                    .await,
                Err(ProvisionError::Failed(_))
            ));
            assert!(!target.join("config.ini").exists());
            assert!(!target.join("__manager").exists());
            assert!(!target.join("user").exists());
        }
    }
}
