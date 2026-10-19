//! Bounded reads of the newest application log lines for diagnostics.
//!
//! The log rotates through `app.log`, `app.log.1` and so on (see `logging::init`). The tail is
//! bounded by lines and by bytes, so a huge or pathological log can never be read whole, and it
//! is read on a blocking thread so the async runtime and the UI stay responsive.

use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

use application::settings::diagnostics::{AppLogTail, DiagnosticsError};
use async_trait::async_trait;

/// The most bytes read from all log files together.
const MAX_TAIL_BYTES: u64 = 512 * 1024;
/// How many rotated files `logging::init` keeps.
const ROTATED_FILES: u32 = 5;

pub struct FileLogTail {
    active: PathBuf,
    max_bytes: u64,
}

impl FileLogTail {
    pub fn new(logs_dir: &Path) -> Self {
        Self::with_byte_budget(logs_dir, MAX_TAIL_BYTES)
    }

    pub fn with_byte_budget(logs_dir: &Path, max_bytes: u64) -> Self {
        Self {
            active: logs_dir.join("app.log"),
            max_bytes,
        }
    }
}

#[async_trait]
impl AppLogTail for FileLogTail {
    async fn last_lines(&self, limit: usize) -> Result<Vec<String>, DiagnosticsError> {
        let active = self.active.clone();
        let budget = self.max_bytes;
        tokio::task::spawn_blocking(move || read_tail(&active, limit, budget))
            .await
            .map_err(|_| DiagnosticsError::Unavailable)?
    }
}

/// The newest `limit` lines over `app.log` and its rotations, reading at most `budget` bytes.
/// An unreadable active file is an error; missing or unreadable rotations only shorten the tail.
fn read_tail(active: &Path, limit: usize, budget: u64) -> Result<Vec<String>, DiagnosticsError> {
    let mut remaining = budget;
    let mut lines: Vec<String> = Vec::new();
    for index in 0..=ROTATED_FILES {
        let path = if index == 0 {
            active.to_owned()
        } else {
            let mut name = active.as_os_str().to_owned();
            name.push(format!(".{index}"));
            PathBuf::from(name)
        };
        let window = match read_window(&path, remaining) {
            Ok(window) => window,
            Err(_) if index == 0 => return Err(DiagnosticsError::Unavailable),
            Err(_) => break,
        };
        remaining -= window.bytes_read;
        let mut older = window.lines;
        older.append(&mut lines);
        lines = older;
        if lines.len() >= limit || remaining == 0 {
            break;
        }
    }
    let excess = lines.len().saturating_sub(limit);
    lines.drain(..excess);
    Ok(lines)
}

struct Window {
    bytes_read: u64,
    /// Whole lines only, oldest first.
    lines: Vec<String>,
}

/// The last `max_bytes` of the file as whole lines: when the window starts inside a line, that
/// partial line is dropped.
fn read_window(path: &Path, max_bytes: u64) -> std::io::Result<Window> {
    let mut file = File::open(path)?;
    let length = file.metadata()?.len();
    let take = length.min(max_bytes);
    file.seek(SeekFrom::Start(length - take))?;
    let mut bytes = Vec::with_capacity(usize::try_from(take).unwrap_or(0));
    file.take(take).read_to_end(&mut bytes)?;
    let mut text = &bytes[..];
    if take < length {
        match text.iter().position(|byte| *byte == b'\n') {
            Some(newline) => text = &text[newline + 1..],
            None => text = &[],
        }
    }
    let text = String::from_utf8_lossy(text);
    let mut lines: Vec<String> = text
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line).to_owned())
        .collect();
    // The text after the final newline is empty, not a line.
    if lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    Ok(Window {
        bytes_read: take,
        lines,
    })
}

#[cfg(test)]
mod tests;
