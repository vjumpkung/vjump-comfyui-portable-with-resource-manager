//! Authorized model roots: the default `ComfyUI/models` folder plus the category folders of
//! `extra_model_paths.yaml`, resolved to their physical targets.
use crate::{
    environment::layout::Layout,
    models::acquisition::stack::DEFAULT_MODEL_ROOT_ID,
    shared::{
        model_paths::{AuthorizedModelRoots, PinnedDirectory},
        path_locks::PathLocks,
    },
};
use application::{
    comfy_runtime::ExtraModelPaths,
    models::library::{RootInfo, RootState, ScanWarning, ScanWarningCode},
};
use domain::{
    models::library::{ModelCategory, ModelLocation},
    shared::model_values::{RelativeModelPath, SafeFileName},
};
use sha2::{Digest, Sha256};
use std::{
    io,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootSpec {
    pub id: String,
    pub label: String,
    /// The canonical physical directory.
    pub path: PathBuf,
    /// `Some` for an extra root (one category folder); `None` for the default root, whose
    /// first path component is the category.
    pub category: Option<ModelCategory>,
}

struct Current {
    authorized: Arc<AuthorizedModelRoots>,
    specs: Vec<RootSpec>,
    infos: Vec<RootInfo>,
}

pub struct LibraryRoots {
    install_root: PathBuf,
    extra: Arc<dyn ExtraModelPaths>,
    locks: Arc<PathLocks>,
    current: RwLock<Current>,
}

fn extra_id(path: &Path) -> String {
    let digest = Sha256::digest(path.to_string_lossy().to_lowercase().as_bytes());
    let hex: String = digest.iter().take(6).map(|b| format!("{b:02x}")).collect();
    format!("extra-{hex}")
}
fn display(path: &Path) -> String {
    let text = path.to_string_lossy();
    text.strip_prefix(r"\\?\").unwrap_or(&text).to_owned()
}

impl LibraryRoots {
    pub fn new(
        install_root: &Path,
        extra: Arc<dyn ExtraModelPaths>,
        locks: Arc<PathLocks>,
    ) -> io::Result<Self> {
        let default = Layout::new(install_root).comfyui_dir().join("models");
        std::fs::create_dir_all(&default)?;
        let this = Self::deferred(install_root, extra, locks)?;
        this.install(Self::default_only(&default)?, Vec::new());
        Ok(this)
    }

    /// No roots and nothing created on disk: used before ComfyUI is cloned. `refresh` reports
    /// the missing default root and registers it once it exists.
    pub fn deferred(
        install_root: &Path,
        extra: Arc<dyn ExtraModelPaths>,
        locks: Arc<PathLocks>,
    ) -> io::Result<Self> {
        Ok(Self {
            install_root: install_root.to_owned(),
            extra,
            locks,
            current: RwLock::new(Current {
                authorized: Arc::new(AuthorizedModelRoots::new([])?),
                specs: Vec::new(),
                infos: Vec::new(),
            }),
        })
    }

    fn default_only(default: &Path) -> io::Result<Vec<(RootSpec, RootInfo)>> {
        let canonical = std::fs::canonicalize(default)?;
        let spec = RootSpec {
            id: DEFAULT_MODEL_ROOT_ID.to_owned(),
            label: "ComfyUI models".to_owned(),
            path: canonical.clone(),
            category: None,
        };
        let info = RootInfo {
            id: spec.id.clone(),
            label: spec.label.clone(),
            path: display(&canonical),
            category: None,
            state: RootState::Ready,
        };
        Ok(vec![(spec, info)])
    }

    fn install(&self, ready: Vec<(RootSpec, RootInfo)>, unavailable: Vec<RootInfo>) {
        let authorized = AuthorizedModelRoots::new(
            ready
                .iter()
                .map(|(spec, _)| (spec.id.clone(), spec.path.clone())),
        )
        .unwrap_or_else(|_| AuthorizedModelRoots::new([]).expect("empty roots"));
        for (spec, _) in &ready {
            self.locks.register_root(&spec.id, &spec.path);
        }
        let mut infos: Vec<RootInfo> = ready.iter().map(|(_, info)| info.clone()).collect();
        infos.extend(unavailable);
        let specs = ready.into_iter().map(|(spec, _)| spec).collect();
        *self.current.write().unwrap_or_else(|p| p.into_inner()) = Current {
            authorized: Arc::new(authorized),
            specs,
            infos,
        };
    }

    /// Reads the YAML again and re-registers every root. Missing or unreadable roots are
    /// reported but never remove catalog rows.
    pub async fn refresh(&self) -> Vec<ScanWarning> {
        let mut warnings = Vec::new();
        let default = Layout::new(&self.install_root).comfyui_dir().join("models");
        let mut ready = match Self::default_only(&default) {
            Ok(ready) => ready,
            Err(_) => {
                warnings.push(ScanWarning {
                    root_id: Some(DEFAULT_MODEL_ROOT_ID.to_owned()),
                    code: ScanWarningCode::RootMissing,
                    detail: display(&default),
                });
                Vec::new()
            }
        };
        let default_physical = ready.first().map(|(spec, _)| spec.path.clone());
        let mut unavailable = Vec::new();
        let configured = match self.extra.read(&self.install_root).await {
            Ok(roots) => roots,
            Err(_) => {
                warnings.push(ScanWarning {
                    root_id: None,
                    code: ScanWarningCode::Unreadable,
                    detail: "extra_model_paths.yaml".to_owned(),
                });
                Vec::new()
            }
        };
        for root in configured {
            let Ok(category) = ModelCategory::new(&root.category) else {
                warnings.push(ScanWarning {
                    root_id: None,
                    code: ScanWarningCode::InvalidName,
                    detail: root.category,
                });
                continue;
            };
            let id = extra_id(&root.path);
            let label = format!("{} ({})", root.section, root.category);
            match std::fs::canonicalize(&root.path) {
                Ok(canonical) => {
                    // A folder below the default root is already scanned through it; an
                    // identical (folder, category) pair appears once.
                    if default_physical
                        .as_ref()
                        .is_some_and(|d| canonical.starts_with(d))
                        || ready.iter().any(|(s, _)| {
                            s.path == canonical && s.category.as_ref() == Some(&category)
                        })
                    {
                        continue;
                    }
                    let id = extra_id(&canonical);
                    let spec = RootSpec {
                        id: id.clone(),
                        label: label.clone(),
                        path: canonical.clone(),
                        category: Some(category.clone()),
                    };
                    let info = RootInfo {
                        id,
                        label,
                        path: display(&canonical),
                        category: Some(category),
                        state: RootState::Ready,
                    };
                    ready.push((spec, info));
                }
                Err(error) => {
                    let missing = error.kind() == io::ErrorKind::NotFound;
                    warnings.push(ScanWarning {
                        root_id: Some(id.clone()),
                        code: if missing {
                            ScanWarningCode::RootMissing
                        } else {
                            ScanWarningCode::RootInaccessible
                        },
                        detail: display(&root.path),
                    });
                    unavailable.push(RootInfo {
                        id,
                        label,
                        path: display(&root.path),
                        category: Some(category),
                        state: if missing {
                            RootState::Missing
                        } else {
                            RootState::Inaccessible
                        },
                    });
                }
            }
        }
        self.install(ready, unavailable);
        warnings
    }

    pub fn authorized(&self) -> Arc<AuthorizedModelRoots> {
        self.current
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .authorized
            .clone()
    }
    pub fn specs(&self) -> Vec<RootSpec> {
        self.current
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .specs
            .clone()
    }
    pub fn infos(&self) -> Vec<RootInfo> {
        self.current
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .infos
            .clone()
    }
    pub fn spec(&self, id: &str) -> Option<RootSpec> {
        self.specs().into_iter().find(|s| s.id == id)
    }

    /// Whether a model may live at `location`.
    pub fn accepts(&self, location: &ModelLocation) -> bool {
        match self.spec(&location.root_id) {
            None => false,
            Some(spec) => spec.category.is_some() || location.path.components().len() >= 2,
        }
    }

    /// The category of a file at `location` and its destination split (subdirectory, name).
    pub fn classify(
        &self,
        location: &ModelLocation,
    ) -> Option<(ModelCategory, RelativeModelPath, SafeFileName)> {
        let spec = self.spec(&location.root_id)?;
        let parts = location.path.components();
        let (name, directories) = parts.split_last()?;
        let (category, below) = match &spec.category {
            Some(category) => (category.clone(), directories),
            None => {
                let (first, rest) = directories.split_first()?;
                (ModelCategory::new(first.as_str()).ok()?, rest)
            }
        };
        let mut subdirectory = RelativeModelPath::default();
        for part in below {
            subdirectory = subdirectory.join(part.clone());
        }
        Some((category, subdirectory, name.clone()))
    }

    /// The pinned directory a location's parent resolves to.
    pub fn pinned_directory(&self, directory: &ModelLocation) -> io::Result<Arc<PinnedDirectory>> {
        let placeholder = SafeFileName::new("x").expect("valid name");
        let (parent, _) = self.authorized().parent(
            &directory.root_id,
            &directory.path.join(placeholder),
            false,
        )?;
        Ok(parent)
    }
}
