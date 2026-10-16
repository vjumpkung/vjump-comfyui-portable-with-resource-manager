use std::{
    borrow::Cow,
    io::{self, Write},
    path::Path,
    sync::{Mutex, OnceLock},
};

use file_rotate::{ContentLimit, FileRotate, compression::Compression, suffix::AppendCount};
use thiserror::Error;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{
    EnvFilter, fmt::MakeWriter, layer::SubscriberExt, util::SubscriberInitExt,
};

#[derive(Default)]
pub struct Redactor {
    secrets: Mutex<Vec<String>>,
}

impl Redactor {
    pub fn register_secret(&self, value: &str) {
        if value.chars().count() < 8 {
            return;
        }
        let mut secrets = self
            .secrets
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !secrets.iter().any(|secret| secret == value) {
            secrets.push(value.to_owned());
        }
    }

    pub fn redact<'a>(&self, input: &'a str) -> Cow<'a, str> {
        let mut spans = Vec::new();
        {
            let secrets = self
                .secrets
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            for secret in secrets.iter() {
                for (start, _) in input.match_indices(secret) {
                    spans.push((start, start + secret.len()));
                }
            }
        }
        let lower = input.to_ascii_lowercase();
        let bytes = input.as_bytes();
        for (start, _) in lower.match_indices("authorization:") {
            let value_start = skip_horizontal_space(bytes, start + "authorization:".len());
            let end = bytes[value_start..]
                .iter()
                .position(|byte| matches!(byte, b'\r' | b'\n' | b'"' | b'\''))
                .map_or(bytes.len(), |offset| value_start + offset);
            if end > value_start {
                spans.push((value_start, end));
            }
        }
        for (start, _) in lower.match_indices("bearer") {
            let after_scheme = start + "bearer".len();
            if !matches!(bytes.get(after_scheme), Some(b' ' | b'\t')) {
                continue;
            }
            let value_start = skip_horizontal_space(bytes, after_scheme);
            let end = value_end(bytes, value_start);
            if end > value_start {
                spans.push((value_start, end));
            }
        }
        let mut index = 0;
        while index < bytes.len() {
            if is_key_byte(bytes[index]) && (index == 0 || !is_key_byte(bytes[index - 1])) {
                let start = index;
                while index < bytes.len() && is_key_byte(bytes[index]) {
                    index += 1;
                }
                if bytes.get(index) == Some(&b'=') {
                    let key = &lower[start..index];
                    if matches!(key, "token" | "access_token" | "api_key")
                        || key.starts_with("x-amz-")
                    {
                        let value_start = index + 1;
                        let end = value_end(bytes, value_start);
                        if end > value_start {
                            spans.push((value_start, end));
                        }
                    }
                }
            } else {
                index += 1;
            }
        }
        for (start, _) in input.match_indices("hf_") {
            let mut end = start + 3;
            while end < bytes.len() && bytes[end].is_ascii_alphanumeric() {
                end += 1;
            }
            if end - start - 3 >= 20 {
                spans.push((start, end));
            }
        }
        // Credentials embedded in a URL authority: `https://user:password@host/`.
        for (start, _) in input.match_indices("://") {
            let authority_start = start + 3;
            let authority_end = bytes[authority_start..]
                .iter()
                .position(|byte| {
                    byte.is_ascii_whitespace()
                        || matches!(byte, b'/' | b'?' | b'#' | b'"' | b'\'' | b'<' | b'>')
                })
                .map_or(bytes.len(), |offset| authority_start + offset);
            if let Some(at) = input[authority_start..authority_end].rfind('@')
                && at > 0
            {
                spans.push((authority_start, authority_start + at));
            }
        }
        if spans.is_empty() {
            return Cow::Borrowed(input);
        }
        spans.sort_unstable();
        let mut merged: Vec<(usize, usize)> = Vec::new();
        for (start, end) in spans {
            if let Some(last) = merged.last_mut()
                && start <= last.1
            {
                last.1 = last.1.max(end);
            } else {
                merged.push((start, end));
            }
        }
        let mut output = String::with_capacity(input.len());
        let mut cursor = 0;
        for (start, end) in merged {
            output.push_str(&input[cursor..start]);
            output.push_str("[REDACTED]");
            cursor = end;
        }
        output.push_str(&input[cursor..]);
        Cow::Owned(output)
    }
}

fn is_key_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')
}

fn skip_horizontal_space(bytes: &[u8], mut index: usize) -> usize {
    while index < bytes.len() && matches!(bytes[index], b' ' | b'\t') {
        index += 1;
    }
    index
}

fn value_end(bytes: &[u8], start: usize) -> usize {
    bytes[start..]
        .iter()
        .position(|byte| {
            byte.is_ascii_whitespace()
                || matches!(
                    byte,
                    b'&' | b'#' | b'"' | b'\'' | b',' | b';' | b'<' | b'>' | b')' | b']' | b'}'
                )
        })
        .map_or(bytes.len(), |offset| start + offset)
}

fn global_redactor() -> &'static Redactor {
    static REDACTOR: OnceLock<Redactor> = OnceLock::new();
    REDACTOR.get_or_init(Redactor::default)
}

pub fn register_secret(value: &str) {
    global_redactor().register_secret(value);
}

pub fn redact(input: &str) -> Cow<'_, str> {
    global_redactor().redact(input)
}

pub struct RedactingMakeWriter<W> {
    inner: W,
}

impl<W> RedactingMakeWriter<W> {
    pub fn new(inner: W) -> Self {
        Self { inner }
    }
}

impl<'a, W: MakeWriter<'a>> MakeWriter<'a> for RedactingMakeWriter<W> {
    type Writer = RedactingWriter<W::Writer>;

    fn make_writer(&'a self) -> Self::Writer {
        RedactingWriter {
            inner: self.inner.make_writer(),
            event: Vec::new(),
        }
    }
}

/// One writer is created per formatted event; fragments stay buffered until flush/drop.
pub struct RedactingWriter<W: Write> {
    inner: W,
    event: Vec<u8>,
}

impl<W: Write> Write for RedactingWriter<W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.event.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if !self.event.is_empty() {
            let event = std::mem::take(&mut self.event);
            self.inner
                .write_all(redact(&String::from_utf8_lossy(&event)).as_bytes())?;
        }
        self.inner.flush()
    }
}

impl<W: Write> Drop for RedactingWriter<W> {
    fn drop(&mut self) {
        let _ = self.flush();
    }
}

pub struct LoggingGuard {
    _worker: WorkerGuard,
}

#[derive(Debug, Error)]
pub enum LoggingError {
    #[error("log directory is unavailable")]
    Unavailable,
    #[error("logging subscriber could not be initialized")]
    Subscriber,
}

pub fn init(logs_dir: &Path) -> Result<LoggingGuard, LoggingError> {
    std::fs::create_dir_all(logs_dir).map_err(|_| LoggingError::Unavailable)?;
    // Verify the active file can be opened before FileRotate's lazy first write.
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(logs_dir.join("app.log"))
        .map_err(|_| LoggingError::Unavailable)?;
    let rotating = FileRotate::new(
        logs_dir.join("app.log"),
        AppendCount::new(5),
        ContentLimit::BytesSurpassed(10 * 1024 * 1024),
        Compression::None,
        None,
    );
    let (writer, worker) = tracing_appender::non_blocking(rotating);
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let subscriber = tracing_subscriber::registry().with(filter).with(
        tracing_subscriber::fmt::layer()
            .with_ansi(false)
            .with_writer(RedactingMakeWriter::new(writer)),
    );
    #[cfg(debug_assertions)]
    let subscriber = subscriber.with(
        tracing_subscriber::fmt::layer()
            .with_ansi(false)
            .with_writer(RedactingMakeWriter::new(std::io::stdout)),
    );
    subscriber
        .try_init()
        .map_err(|_| LoggingError::Subscriber)?;
    Ok(LoggingGuard { _worker: worker })
}

#[cfg(test)]
mod tests;
