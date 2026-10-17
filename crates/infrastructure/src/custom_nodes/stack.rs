//! Composition of the custom-node adapters over one install root, shared by the desktop root and
//! its integration tests, plus the in-process event hub the UI subscribes to.
use super::{
    compat::Host,
    files::{LocalNodeFiles, Recycler, node_limits},
    git::GitNodes,
    hook::PythonHook,
    registry::{REGISTRY_BASE, RegistryHttp},
    sqlite::SqliteNodes,
};
use crate::shared::{download::Downloader, sqlite::Db};
use application::{
    custom_nodes::{NodeDeps, NodePlans},
    environment::{EnvironmentDeps, SetupEvent, SetupProgressSink},
};
use std::{
    collections::{HashMap, VecDeque},
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};
use tokio::sync::broadcast;

const LOG_LINES: usize = 300;
const KEPT_OPERATIONS: usize = 16;
const QUEUE: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeEvent {
    /// One redacted line of an operation's log.
    Log { operation: String, line: String },
    /// Installed nodes or operation state changed; subscribers refetch.
    Changed,
}

/// The global log tail and the per-operation log tails.
type NodeLogs = (VecDeque<String>, HashMap<String, VecDeque<String>>);

/// Fan-out of node events. Queues are bounded: a subscriber that falls behind receives a gap
/// and must take a fresh snapshot instead of replaying what it missed.
pub struct NodeHub {
    sender: broadcast::Sender<(u64, NodeEvent)>,
    sequence: AtomicU64,
    logs: Mutex<NodeLogs>,
    pub epoch: String,
}

impl NodeHub {
    pub fn new(epoch: String) -> Arc<Self> {
        Arc::new(Self {
            sender: broadcast::channel(QUEUE).0,
            sequence: AtomicU64::new(0),
            logs: Mutex::default(),
            epoch,
        })
    }

    pub fn sequence(&self) -> u64 {
        self.sequence.load(Ordering::SeqCst)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<(u64, NodeEvent)> {
        self.sender.subscribe()
    }

    pub fn publish(&self, event: NodeEvent) {
        let sequence = self.sequence.fetch_add(1, Ordering::SeqCst) + 1;
        let _ = self.sender.send((sequence, event));
    }

    pub fn log(&self, operation: &str, line: &str) {
        {
            let mut guard = self.logs.lock().unwrap_or_else(|p| p.into_inner());
            let (order, lines) = &mut *guard;
            if !lines.contains_key(operation) {
                order.push_back(operation.to_owned());
                if order.len() > KEPT_OPERATIONS
                    && let Some(old) = order.pop_front()
                {
                    lines.remove(&old);
                }
            }
            let entry = lines.entry(operation.to_owned()).or_default();
            if entry.len() >= LOG_LINES {
                entry.pop_front();
            }
            entry.push_back(line.chars().take(1000).collect());
        }
        self.publish(NodeEvent::Log {
            operation: operation.to_owned(),
            line: line.chars().take(1000).collect(),
        });
    }

    /// The most recent lines of an operation, kept in memory for a reconnecting window.
    pub fn tail(&self, operation: &str) -> Vec<String> {
        self.logs
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .1
            .get(operation)
            .map(|lines| lines.iter().cloned().collect())
            .unwrap_or_default()
    }
}

/// Turns step events of a node operation into hub log lines.
pub struct HubSink(pub Arc<NodeHub>);

impl SetupProgressSink for HubSink {
    fn emit(&self, event: SetupEvent) {
        if let SetupEvent::Log { run_id, line, .. } = event {
            self.0.log(run_id.as_str(), &line);
        }
    }
}

pub struct NodeOptions {
    pub downloader: Downloader,
    pub registry_base: String,
    pub git: Arc<GitNodes>,
    pub recycler: Recycler,
}

impl Default for NodeOptions {
    fn default() -> Self {
        Self {
            downloader: Downloader::new(),
            registry_base: REGISTRY_BASE.to_owned(),
            git: Arc::new(GitNodes::new()),
            recycler: Arc::new(crate::shared::trash::recycle_entry),
        }
    }
}

pub struct NodeStack {
    pub deps: NodeDeps,
    pub hub: Arc<NodeHub>,
    pub store: Arc<SqliteNodes>,
    pub registry: Arc<RegistryHttp>,
    pub files: Arc<LocalNodeFiles>,
}

impl NodeStack {
    pub fn open(root: &Path, db: Db, environment: EnvironmentDeps, epoch: String) -> Self {
        Self::open_with(root, db, environment, epoch, NodeOptions::default())
    }

    pub fn open_with(
        root: &Path,
        db: Db,
        environment: EnvironmentDeps,
        epoch: String,
        options: NodeOptions,
    ) -> Self {
        let store = Arc::new(SqliteNodes { db });
        let observed_root = root.to_owned();
        let registry = Arc::new(RegistryHttp::new(
            options.downloader,
            options.registry_base,
            store.clone(),
            Arc::new(move || Host::observe(&observed_root)),
        ));
        let files = Arc::new(LocalNodeFiles {
            registry: registry.clone(),
            git: options.git,
            recycler: options.recycler,
            limits: node_limits(),
        });
        let deps = NodeDeps {
            environment,
            registry: registry.clone(),
            store: store.clone(),
            files: files.clone(),
            hook: Arc::new(PythonHook),
            plans: Arc::<NodePlans>::default(),
        };
        Self {
            deps,
            hub: NodeHub::new(epoch),
            store,
            registry,
            files,
        }
    }
}
