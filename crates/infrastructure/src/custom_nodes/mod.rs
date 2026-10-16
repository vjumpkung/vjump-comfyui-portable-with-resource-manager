//! Custom-node adapters (M5): registry client, provenance, Git, hardened publication, scanning
//! and SQLite persistence.
pub mod compat;
pub mod files;
pub mod git;
pub mod hook;
pub mod provenance;
pub mod registry;
pub mod scan;
pub mod sqlite;
pub mod stack;
#[cfg(test)]
mod tests;
pub mod tree;
