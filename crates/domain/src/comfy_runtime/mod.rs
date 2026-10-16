//! ComfyUI launch, lifecycle and source-update policy. No OS handles live here.
mod launch;
mod lifecycle;
mod update;

pub use launch::*;
pub use lifecycle::*;
pub use update::*;
