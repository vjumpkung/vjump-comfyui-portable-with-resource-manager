//! Core facade. Call held-lease methods when composing workflows; never acquire recursively.
mod model;
mod ports;
mod requirements;
mod runtime;
mod scan_control;
mod setup;
mod use_cases;
mod workflow;
pub use model::*;
pub use ports::*;
pub use requirements::NodeRequirements;
pub use scan_control::ScanControl;
pub use use_cases::*;
pub(crate) use workflow::SETUP_HOOK_WARNING;

#[cfg(test)]
pub(crate) mod tests;
