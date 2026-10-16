//! Bounded runtime logs and ordered subscriptions.
use crate::shared::logging::redact;
use application::comfy_runtime::*;
use async_trait::async_trait;
use file_rotate::{ContentLimit, FileRotate, compression::Compression, suffix::AppendCount};
use std::{
    collections::{HashMap, VecDeque},
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};
use tokio::sync::mpsc;

const RING_BYTES: usize = 1 << 20;
const LINE_BYTES: usize = 64 << 10;
const QUEUE_EVENTS: usize = 64;
struct Subscriber {
    sender: mpsc::Sender<RuntimeEvent>,
    overflow: Arc<AtomicU64>,
}
#[derive(Default)]
struct State {
    cursor: u64,
    next_subscription: u64,
    status: RuntimeStatus,
    ring: VecDeque<LogChunk>,
    bytes: usize,
    subscribers: HashMap<u64, Subscriber>,
}
#[derive(Clone, Default)]
pub struct RuntimeLogHub {
    state: Arc<Mutex<State>>,
}
impl RuntimeLogHub {
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }
    pub fn set_status(&self, status: RuntimeStatus) {
        let mut state = self.lock();
        state.cursor += 1;
        state.status = status.clone();
        let event = RuntimeEvent::State {
            cursor: state.cursor,
            status: Box::new(status),
        };
        deliver(&mut state, event);
    }
    pub fn status(&self) -> RuntimeStatus {
        self.lock().status.clone()
    }
    pub fn push(&self, session: &SessionId, text: &str) {
        let text = redact(text).into_owned();
        let mut state = self.lock();
        state.cursor += 1;
        let chunk = LogChunk {
            cursor: state.cursor,
            session: session.clone(),
            text,
        };
        state.bytes += chunk.text.len();
        state.ring.push_back(chunk.clone());
        while state.bytes > RING_BYTES {
            if let Some(removed) = state.ring.pop_front() {
                state.bytes -= removed.text.len();
            }
        }
        deliver(&mut state, RuntimeEvent::Log(chunk));
    }
    pub fn last_lines(&self, limit: usize) -> Vec<String> {
        let state = self.lock();
        let text = state
            .ring
            .iter()
            .map(|c| c.text.as_str())
            .collect::<String>();
        let lines: Vec<_> = text
            .split(['\r', '\n'])
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect();
        lines[lines.len().saturating_sub(limit)..].to_vec()
    }
}
fn deliver(state: &mut State, event: RuntimeEvent) {
    state.subscribers.retain(
        |_, subscriber| match subscriber.sender.try_send(event.clone()) {
            Ok(()) => true,
            Err(mpsc::error::TrySendError::Full(_)) => {
                subscriber.overflow.store(state.cursor, Ordering::SeqCst);
                false
            }
            Err(mpsc::error::TrySendError::Closed(_)) => false,
        },
    );
}
impl RuntimeEvents for RuntimeLogHub {
    fn subscribe(&self, after_cursor: Option<u64>) -> RuntimeSubscription {
        let (sender, receiver) = mpsc::channel(QUEUE_EVENTS);
        let overflow = Arc::new(AtomicU64::new(0));
        let mut state = self.lock();
        state.next_subscription += 1;
        let id = state.next_subscription;
        let earliest = state.ring.front().map_or(state.cursor, |c| c.cursor);
        let gap = after_cursor
            .is_some_and(|after| after > state.cursor || after.saturating_add(1) < earliest);
        let logs = state
            .ring
            .iter()
            .filter(|c| after_cursor.is_none_or(|after| c.cursor > after))
            .cloned()
            .collect();
        let _ = sender.try_send(RuntimeEvent::Snapshot {
            cursor: state.cursor,
            status: Box::new(state.status.clone()),
            logs,
            gap,
        });
        // Registration and the snapshot/replay boundary share this mutex with all publishers.
        if state.subscribers.len() < 32 {
            state.subscribers.insert(
                id,
                Subscriber {
                    sender,
                    overflow: overflow.clone(),
                },
            );
        } else {
            overflow.store(state.cursor.max(1), Ordering::SeqCst);
        }
        RuntimeSubscription {
            id,
            receiver: Box::new(Receiver {
                receiver,
                overflow,
                hub: self.clone(),
                id,
                gap_sent: false,
            }),
        }
    }
    fn unsubscribe(&self, id: u64) {
        self.lock().subscribers.remove(&id);
    }
}
impl RuntimeLogReader for RuntimeLogHub {
    fn last_lines(&self, limit: usize) -> Vec<String> {
        RuntimeLogHub::last_lines(self, limit)
    }
}
struct Receiver {
    receiver: mpsc::Receiver<RuntimeEvent>,
    overflow: Arc<AtomicU64>,
    hub: RuntimeLogHub,
    id: u64,
    gap_sent: bool,
}
#[async_trait]
impl RuntimeEventReceiver for Receiver {
    async fn next(&mut self) -> Option<RuntimeEvent> {
        if let Some(event) = self.receiver.recv().await {
            return Some(event);
        }
        let cursor = self.overflow.load(Ordering::SeqCst);
        if cursor > 0 && !self.gap_sent {
            self.gap_sent = true;
            return Some(RuntimeEvent::Gap { cursor });
        }
        None
    }
}
impl Drop for Receiver {
    fn drop(&mut self) {
        self.hub.unsubscribe(self.id);
    }
}

/// Per-stream assembler. Nothing is delivered until a complete CR/LF record is available,
/// so Unicode and secrets fragmented between reads are redacted as one record. Overlong
/// records are discarded as a whole rather than leaking a secret across a truncation boundary.
#[derive(Default)]
struct Decoder {
    bytes: Vec<u8>,
    discarded: bool,
}
impl Decoder {
    fn feed(&mut self, bytes: &[u8], mut emit: impl FnMut(String)) {
        for &byte in bytes {
            if matches!(byte, b'\r' | b'\n') {
                if self.discarded {
                    emit(format!("[overlong log record omitted]{}", char::from(byte)));
                } else {
                    let line = String::from_utf8_lossy(&self.bytes);
                    emit(format!(
                        "{}{ending}",
                        redact(&line),
                        ending = char::from(byte)
                    ));
                }
                self.bytes.clear();
                self.discarded = false;
            } else if !self.discarded {
                self.bytes.push(byte);
                if self.bytes.len() > LINE_BYTES {
                    self.bytes.clear();
                    self.discarded = true;
                }
            }
        }
    }
    fn finish(&mut self, mut emit: impl FnMut(String)) {
        if self.discarded {
            emit("[overlong log record omitted]".into());
        } else if !self.bytes.is_empty() {
            emit(redact(&String::from_utf8_lossy(&self.bytes)).into_owned());
        }
        self.bytes.clear();
        self.discarded = false;
    }
}
struct SessionOutput {
    stdout: Decoder,
    stderr: Decoder,
    file: FileRotate<AppendCount>,
    failed: bool,
}
pub struct SessionLog {
    id: SessionId,
    hub: RuntimeLogHub,
    output: Mutex<SessionOutput>,
    directory: PathBuf,
}
impl SessionLog {
    pub fn open(
        directory: &Path,
        session: &RuntimeSession,
        hub: RuntimeLogHub,
    ) -> Result<Self, RuntimeError> {
        fs::create_dir_all(directory)
            .map_err(|_| RuntimeError::Failed("runtime log directory is unavailable".into()))?;
        let stamp: String = session
            .started_at
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .collect();
        let filename = format!("comfyui-{stamp}-{}.log", session.id.as_str());
        let path = directory.join(&filename);
        let file = FileRotate::new(
            path,
            AppendCount::new(3),
            ContentLimit::Bytes(10 << 20),
            Compression::None,
            None,
        );
        retain(directory, Some(&filename))?;
        Ok(Self {
            id: session.id.clone(),
            hub,
            output: Mutex::new(SessionOutput {
                stdout: Decoder::default(),
                stderr: Decoder::default(),
                file,
                failed: false,
            }),
            directory: directory.to_path_buf(),
        })
    }
    pub fn chunk(&self, stderr: bool, bytes: &[u8]) {
        let mut out = self.output.lock().unwrap_or_else(|p| p.into_inner());
        let mut records = Vec::new();
        if stderr {
            out.stderr.feed(bytes, |text| records.push(text));
        } else {
            out.stdout.feed(bytes, |text| records.push(text));
        }
        for record in records {
            self.record(&mut out, &record);
        }
    }
    fn record(&self, out: &mut SessionOutput, text: &str) {
        if out.file.write_all(text.as_bytes()).is_err() {
            out.failed = true;
        }
        self.hub.push(&self.id, text);
    }
    pub fn finish(&self) -> Result<(), RuntimeError> {
        let mut out = self.output.lock().unwrap_or_else(|p| p.into_inner());
        let mut records = Vec::new();
        out.stdout.finish(|text| records.push(text));
        out.stderr.finish(|text| records.push(text));
        for text in records {
            self.record(&mut out, &text);
        }
        if out.file.flush().is_err() {
            out.failed = true;
        }
        if out.failed {
            return Err(RuntimeError::Failed(
                "runtime logs could not be fully saved".into(),
            ));
        }
        retain(&self.directory, None)
    }
}
fn retain(directory: &Path, active: Option<&str>) -> Result<(), RuntimeError> {
    let mut groups: HashMap<String, Vec<PathBuf>> = HashMap::new();
    for entry in fs::read_dir(directory).map_err(|_| RuntimeError::Storage)? {
        let entry = entry.map_err(|_| RuntimeError::Storage)?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !entry
            .file_type()
            .map_err(|_| RuntimeError::Storage)?
            .is_file()
            || !name.starts_with("comfyui-")
        {
            continue;
        }
        let Some((stem, suffix)) = name.split_once(".log") else {
            continue;
        };
        if !suffix.is_empty()
            && !suffix
                .strip_prefix('.')
                .is_some_and(|s| s.parse::<u8>().is_ok())
        {
            continue;
        }
        let key = format!("{stem}.log");
        if active == Some(key.as_str()) {
            continue;
        }
        groups.entry(key).or_default().push(entry.path());
    }
    let mut keys: Vec<_> = groups.keys().cloned().collect();
    keys.sort();
    for key in keys.iter().take(keys.len().saturating_sub(10)) {
        for path in &groups[key] {
            fs::remove_file(path).map_err(|_| RuntimeError::Storage)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fragmented_unicode_secrets_and_carriage_returns() {
        let mut decoder = Decoder::default();
        let mut output = String::new();
        let text = "ไทย token=verysecret\rprogress\nlast";
        for byte in text.as_bytes() {
            decoder.feed(&[*byte], |s| output.push_str(&s));
        }
        decoder.finish(|s| output.push_str(&s));
        assert_eq!(output, "ไทย token=[REDACTED]\rprogress\nlast");
    }
    #[tokio::test]
    async fn slow_subscriber_gets_gap_and_reconnect_is_ordered() {
        let hub = RuntimeLogHub::default();
        let id = SessionId::new("s1").unwrap();
        let mut subscription = hub.subscribe(None);
        for _ in 0..100 {
            hub.push(&id, "hello\r");
        }
        let mut gap = false;
        while let Some(event) = subscription.receiver.next().await {
            if matches!(event, RuntimeEvent::Gap { .. }) {
                gap = true;
            }
        }
        assert!(gap);
        let mut resumed = hub.subscribe(Some(50));
        let Some(RuntimeEvent::Snapshot { cursor, logs, .. }) = resumed.receiver.next().await
        else {
            panic!();
        };
        assert_eq!(cursor, 100);
        assert_eq!(logs.first().unwrap().cursor, 51);
        hub.push(&id, "next\n");
        assert!(matches!(
            resumed.receiver.next().await,
            Some(RuntimeEvent::Log(LogChunk { cursor: 101, .. }))
        ));
    }
    #[test]
    fn ring_is_bounded_and_oversized_records_do_not_leak() {
        let hub = RuntimeLogHub::default();
        let id = SessionId::new("x").unwrap();
        for _ in 0..100 {
            hub.push(&id, &"x".repeat(64000));
        }
        assert!(hub.lock().bytes <= RING_BYTES);
        let mut decoder = Decoder::default();
        let mut out = String::new();
        decoder.feed(&vec![b'x'; LINE_BYTES + 1], |s| out.push_str(&s));
        decoder.feed(b"secret\n", |s| out.push_str(&s));
        assert_eq!(out, "[overlong log record omitted]\n");
    }
}
