//! ComfyUI runtime adapters. Process creation and teardown remain in M1's shared runner.
pub mod extra_paths;
pub mod git;
pub mod http;
pub mod inventory;
pub mod logs;
pub mod port;
pub mod process;
pub mod sqlite;
