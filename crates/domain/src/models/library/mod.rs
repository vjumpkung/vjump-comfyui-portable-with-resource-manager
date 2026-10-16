use crate::shared::model_values::{ModelValueError, SafeFileName};

mod model_file;
mod sidecars;

pub use model_file::*;
pub use sidecars::*;

/// Owned by the library context. Acquisition only supplies category suggestions.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ModelCategory(SafeFileName);
impl ModelCategory {
    pub fn new(value: &str) -> Result<Self, ModelValueError> {
        SafeFileName::new(match value {
            "unet" => "diffusion_models",
            "clip" => "text_encoders",
            _ => value,
        })
        .map(Self)
    }
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
    pub fn suggest_civitai(model_type: &str) -> Option<Self> {
        let category = match model_type.to_ascii_lowercase().as_str() {
            "checkpoint" => "checkpoints",
            "lora" | "locon" | "dora" => "loras",
            "textualinversion" | "embedding" => "embeddings",
            "vae" => "vae",
            "controlnet" => "controlnet",
            "upscaler" => "upscale_models",
            "hypernetwork" => "hypernetworks",
            _ => return None,
        };
        Self::new(category).ok()
    }
}
