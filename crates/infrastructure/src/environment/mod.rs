//! Adapters of the environment context: tools, Python, packages, verification, ComfyUI source,
//! persistence and the install manifest.

pub mod exec;
pub mod hardware;
pub mod layout;
pub mod manifest;
pub mod packages;
pub mod pins;
pub mod profiles;
pub mod python;
pub mod runtime_guard;
pub mod sinks;
pub mod source;
pub mod sqlite;
pub mod switcher;
pub mod tools;
pub mod verify;
