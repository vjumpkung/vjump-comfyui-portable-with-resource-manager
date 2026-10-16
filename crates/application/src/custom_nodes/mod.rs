//! Core facade. Call held-lease methods when composing workflows; never acquire recursively.
mod model;
mod ports;
mod use_cases;
mod workflow;
pub use model::*;
pub use ports::*;
pub use use_cases::*;

#[cfg(test)]
mod tests;
