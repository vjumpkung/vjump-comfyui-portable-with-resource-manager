//! The gallery context: ComfyUI's `input` and `output` folders as a navigable file tree. A
//! location is an area plus validated relative components, never an unchecked absolute path.
use crate::shared::model_values::{RelativeModelPath, SafeFileName};
use thiserror::Error;

/// The app-owned folder inside each area that holds in-flight copies. It is never listed and
/// never addressable, so no location can name it.
pub const STAGING_DIR: &str = ".vjump-gallery-staging";

/// Which of ComfyUI's fixed media folders a location lives in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GalleryArea {
    Input,
    Output,
}
impl GalleryArea {
    /// The folder name below `ComfyUI/`.
    pub fn dir_name(self) -> &'static str {
        match self {
            Self::Input => "input",
            Self::Output => "output",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("invalid gallery path")]
pub struct GalleryPathError;

/// An area plus validated relative components. The empty path is the area root.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GalleryLocation {
    area: GalleryArea,
    path: RelativeModelPath,
}
impl GalleryLocation {
    /// Rejects any component equal to [`STAGING_DIR`], ignoring ASCII case.
    pub fn new(area: GalleryArea, path: RelativeModelPath) -> Result<Self, GalleryPathError> {
        if path.components().iter().any(is_staging) {
            return Err(GalleryPathError);
        }
        Ok(Self { area, path })
    }
    pub fn root(area: GalleryArea) -> Self {
        Self {
            area,
            path: RelativeModelPath::default(),
        }
    }
    pub fn area(&self) -> GalleryArea {
        self.area
    }
    pub fn path(&self) -> &RelativeModelPath {
        &self.path
    }
    pub fn is_root(&self) -> bool {
        self.path.components().is_empty()
    }
    /// The final component, or `None` for the area root.
    pub fn name(&self) -> Option<&SafeFileName> {
        self.path.components().last()
    }
    /// The containing directory, or `None` for the area root.
    pub fn parent(&self) -> Option<Self> {
        let parts = self.path.components().split_last()?.1;
        let mut path = RelativeModelPath::default();
        for part in parts {
            path = path.join(part.clone());
        }
        Some(Self {
            area: self.area,
            path,
        })
    }
    pub fn join(&self, name: SafeFileName) -> Result<Self, GalleryPathError> {
        if is_staging(&name) {
            return Err(GalleryPathError);
        }
        Ok(Self {
            area: self.area,
            path: self.path.join(name),
        })
    }
    /// Same area and `other` is `self` or a descendant. Components compare ASCII
    /// case-insensitively, which is conservative on case-sensitive volumes.
    pub fn contains(&self, other: &Self) -> bool {
        let (mine, theirs) = (self.path.components(), other.path.components());
        self.area == other.area
            && theirs.len() >= mine.len()
            && mine
                .iter()
                .zip(theirs)
                .all(|(a, b)| a.as_str().eq_ignore_ascii_case(b.as_str()))
    }
}

fn is_staging(name: &SafeFileName) -> bool {
    name.as_str().eq_ignore_ascii_case(STAGING_DIR)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GalleryEntryKind {
    File,
    Directory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GalleryMediaKind {
    Image,
    Video,
    Model3d,
    Other,
}

/// Classifies a file by its lowercase extension. A leading dot alone is not an extension.
/// Directories are not classified.
pub fn classify_media(name: &SafeFileName) -> GalleryMediaKind {
    let text = name.as_str();
    let extension = match text.rfind('.') {
        Some(0) | None => return GalleryMediaKind::Other,
        Some(index) => text[index + 1..].to_ascii_lowercase(),
    };
    match extension.as_str() {
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" | "avif" => GalleryMediaKind::Image,
        "mp4" | "webm" | "mov" | "m4v" => GalleryMediaKind::Video,
        "glb" | "gltf" | "obj" | "fbx" | "stl" | "ply" => GalleryMediaKind::Model3d,
        _ => GalleryMediaKind::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(value: &str) -> SafeFileName {
        SafeFileName::new(value).unwrap()
    }
    fn location(area: GalleryArea, path: &str) -> GalleryLocation {
        GalleryLocation::new(area, RelativeModelPath::new(path).unwrap()).unwrap()
    }

    #[test]
    fn areas_name_their_comfyui_folders() {
        assert_eq!(GalleryArea::Input.dir_name(), "input");
        assert_eq!(GalleryArea::Output.dir_name(), "output");
    }

    #[test]
    fn media_is_classified_by_lowercase_extension() {
        use GalleryMediaKind::*;
        for (file, expected) in [
            ("a.png", Image),
            ("a.JPG", Image),
            ("a.jpeg", Image),
            ("a.WebP", Image),
            ("a.gif", Image),
            ("a.bmp", Image),
            ("a.avif", Image),
            ("a.mp4", Video),
            ("a.WEBM", Video),
            ("a.mov", Video),
            ("a.m4v", Video),
            ("a.glb", Model3d),
            ("a.gltf", Model3d),
            ("a.obj", Model3d),
            ("a.FBX", Model3d),
            ("a.stl", Model3d),
            ("a.ply", Model3d),
            ("a.txt", Other),
            ("a.json", Other),
            ("png", Other),
            (".png", Other),
            ("archive.tar.gz", Other),
            ("clip.final.mp4", Video),
            ("ภาพ.png", Image),
        ] {
            assert_eq!(classify_media(&name(file)), expected, "{file}");
        }
    }

    #[test]
    fn the_staging_folder_cannot_be_named_at_any_depth_or_case() {
        for hostile in [
            ".vjump-gallery-staging",
            ".VJUMP-Gallery-Staging",
            ".vjump-gallery-staging/x.png",
            "a/.vjump-gallery-STAGING",
            "a/b/.vjump-gallery-staging/c",
        ] {
            let path = RelativeModelPath::new(hostile).unwrap();
            assert_eq!(
                GalleryLocation::new(GalleryArea::Output, path),
                Err(GalleryPathError),
                "{hostile}"
            );
        }
        let parent = location(GalleryArea::Input, "a");
        assert_eq!(parent.join(name(STAGING_DIR)), Err(GalleryPathError));
        assert_eq!(
            parent.join(name(".Vjump-Gallery-Staging")),
            Err(GalleryPathError)
        );
        // A similar name is an ordinary folder.
        assert!(parent.join(name(".vjump-gallery-staging2")).is_ok());
        assert!(
            GalleryLocation::new(GalleryArea::Input, RelativeModelPath::new("").unwrap()).is_ok()
        );
    }

    #[test]
    fn root_name_parent_and_join_follow_the_path() {
        let root = GalleryLocation::root(GalleryArea::Output);
        assert!(root.is_root());
        assert_eq!(root.area(), GalleryArea::Output);
        assert_eq!(root.name(), None);
        assert_eq!(root.parent(), None);
        assert!(root.path().components().is_empty());

        let nested = root
            .join(name("2026"))
            .unwrap()
            .join(name("cat.png"))
            .unwrap();
        assert!(!nested.is_root());
        assert_eq!(nested.name().unwrap().as_str(), "cat.png");
        assert_eq!(nested.path().as_string(), "2026/cat.png");
        let folder = nested.parent().unwrap();
        assert_eq!(folder, location(GalleryArea::Output, "2026"));
        assert_eq!(folder.parent(), Some(root.clone()));
        assert_eq!(
            nested,
            location(GalleryArea::Output, "2026/cat.png"),
            "join and parse agree"
        );
        assert_ne!(nested, location(GalleryArea::Input, "2026/cat.png"));
    }

    #[test]
    fn contains_is_same_area_self_or_descendant_ignoring_ascii_case() {
        let root = GalleryLocation::root(GalleryArea::Output);
        let folder = location(GalleryArea::Output, "Shots/Day1");
        let file = location(GalleryArea::Output, "Shots/Day1/a.png");
        assert!(root.contains(&root));
        assert!(root.contains(&folder));
        assert!(folder.contains(&folder));
        assert!(folder.contains(&file));
        assert!(!file.contains(&folder));
        assert!(!folder.contains(&location(GalleryArea::Output, "Shots")));
        assert!(!folder.contains(&location(GalleryArea::Output, "Shots/Day10")));
        assert!(!folder.contains(&location(GalleryArea::Output, "Other/Day1")));
        assert!(folder.contains(&location(GalleryArea::Output, "shots/DAY1/b.png")));
        // The same relative path in the other area is a different place.
        assert!(!root.contains(&GalleryLocation::root(GalleryArea::Input)));
        assert!(!folder.contains(&location(GalleryArea::Input, "Shots/Day1/a.png")));
    }
}
