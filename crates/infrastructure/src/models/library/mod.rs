//! The model library adapters (M4): scanning, watching, catalog, journal and file operations.
pub mod file_ops;
pub mod identify;
pub mod roots;
pub mod scan;
pub mod sqlite;
pub mod stack;
pub mod supervisor;
#[cfg(test)]
mod tests;
pub mod watch;
