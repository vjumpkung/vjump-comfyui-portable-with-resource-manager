use application::models::acquisition::*;
use domain::{models::acquisition::DownloadJob, shared::model_values::JobId};
use std::{
    collections::{HashMap, VecDeque},
    sync::Mutex,
    time::Instant,
};

struct Subscriber {
    queue: VecDeque<DownloadEvent>,
    gapped: bool,
}
struct State {
    sequence: u64,
    next_id: u64,
    jobs: HashMap<JobId, DownloadJob>,
    subscribers: HashMap<u64, Subscriber>,
    last_progress: HashMap<JobId, Instant>,
    history: VecDeque<DownloadEvent>,
}
pub struct DownloadHub {
    epoch: String,
    capacity: usize,
    state: Mutex<State>,
    notify: tokio::sync::Notify,
}
pub struct DownloadSubscription {
    pub id: u64,
    pub cursor: DownloadCursor,
    pub replay_gap: bool,
}
impl DownloadHub {
    pub fn new(epoch: String, capacity: usize) -> Self {
        Self {
            epoch,
            capacity: capacity.clamp(2, 4096),
            state: Mutex::new(State {
                sequence: 0,
                next_id: 0,
                jobs: HashMap::new(),
                subscribers: HashMap::new(),
                last_progress: HashMap::new(),
                history: VecDeque::new(),
            }),
            notify: tokio::sync::Notify::new(),
        }
    }
    pub fn seed(&self, jobs: impl IntoIterator<Item = DownloadJob>) {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        for job in jobs {
            state.jobs.insert(job.id.clone(), job);
        }
    }
    pub fn subscribe(&self, after: Option<&DownloadCursor>) -> DownloadSubscription {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        state.next_id += 1;
        let id = state.next_id;
        let cursor = DownloadCursor {
            epoch: self.epoch.clone(),
            sequence: state.sequence,
        };
        let replay_gap = after.is_some_and(|c| {
            c.epoch != self.epoch
                || c.sequence > state.sequence
                || state
                    .history
                    .front()
                    .is_some_and(|e| c.sequence.saturating_add(1) < e.cursor.sequence)
        });
        let mut jobs = state.jobs.values().cloned().collect::<Vec<_>>();
        jobs.sort_by(|a, b| a.id.cmp(&b.id));
        let snapshot = DownloadEvent {
            cursor: cursor.clone(),
            kind: DownloadEventKind::Snapshot(jobs),
        };
        state.subscribers.insert(
            id,
            Subscriber {
                queue: VecDeque::from([snapshot]),
                gapped: false,
            },
        );
        DownloadSubscription {
            id,
            cursor,
            replay_gap,
        }
    }
    /// Desktop pumps this bounded queue into the authorized Channel; on send failure call unsubscribe.
    pub fn drain(&self, id: u64, limit: usize) -> Option<Vec<DownloadEvent>> {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let sub = state.subscribers.get_mut(&id)?;
        Some(
            (0..limit.clamp(1, 500))
                .filter_map(|_| sub.queue.pop_front())
                .collect(),
        )
    }
    /// Resolves after the next published event. Callers also poll on a timer, so a missed
    /// wake-up only delays delivery.
    pub async fn changed(&self) {
        self.notify.notified().await;
    }
    pub fn unsubscribe(&self, id: u64) {
        self.state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .subscribers
            .remove(&id);
    }
    pub fn current(&self, id: &JobId) -> Option<DownloadJob> {
        self.state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .jobs
            .get(id)
            .cloned()
    }
    fn publish(&self, state: &mut State, kind: DownloadEventKind) {
        state.sequence += 1;
        let event = DownloadEvent {
            cursor: DownloadCursor {
                epoch: self.epoch.clone(),
                sequence: state.sequence,
            },
            kind,
        };
        state.history.push_back(event.clone());
        while state.history.len() > 1024 {
            state.history.pop_front();
        }
        for subscriber in state.subscribers.values_mut() {
            if subscriber.gapped {
                continue;
            }
            if subscriber.queue.len() >= self.capacity {
                subscriber.queue.clear();
                subscriber.queue.push_back(DownloadEvent {
                    cursor: event.cursor.clone(),
                    kind: DownloadEventKind::Gap,
                });
                subscriber.gapped = true;
            } else {
                subscriber.queue.push_back(event.clone());
            }
        }
        self.notify.notify_waiters();
    }
}
impl DownloadEvents for DownloadHub {
    fn state(&self, job: &DownloadJob) {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if state
            .jobs
            .get(&job.id)
            .is_some_and(|old| old.generation > job.generation)
        {
            return;
        }
        state.jobs.insert(job.id.clone(), job.clone());
        self.publish(&mut state, DownloadEventKind::State(Box::new(job.clone())));
    }
}
impl ProgressSink for DownloadHub {
    fn sample(&self, id: &JobId, generation: u64, progress: TransferProgress) {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if state.jobs.get(id).is_none_or(|j| {
            j.generation != generation
                || j.state != domain::models::acquisition::DownloadState::Downloading
        }) {
            return;
        }
        let now = Instant::now();
        if state
            .last_progress
            .get(id)
            .is_some_and(|last| now.duration_since(*last) < PROGRESS_INTERVAL)
        {
            return;
        }
        state.last_progress.insert(id.clone(), now);
        if let Some(job) = state.jobs.get_mut(id) {
            job.bytes_done = progress.bytes;
            job.size = progress.total;
        }
        self.publish(
            &mut state,
            DownloadEventKind::Progress {
                id: id.clone(),
                generation,
                progress,
            },
        );
    }
}
