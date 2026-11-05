//! The global event stream (`GET /events`, Server-Sent Events).
//!
//! One [`EventHub`] per Daemon holds a ring buffer of recent events. Every
//! event gets a monotonically increasing sequence number, used as the SSE `id`.
//! The buffer keeps the most recent [`EventRetention::max_events`] events no
//! older than [`EventRetention::max_age`].
//!
//! Subscribing is "listen first, snapshot second, ready last": the hub fixes
//! the subscriber's position under its lock, then the snapshot is read, then
//! `ready` is sent, then everything published after the fixed position is
//! streamed. A change that lands between the position and the snapshot is
//! therefore delivered again as an event (workspace events carry the full
//! summary, so applying one twice is harmless); it is never lost.
//!
//! Frames on the wire (`event:` name, `id:` only where stated):
//!
//! - `snapshot` (no id): `{seq, workspaces[], operations[]}`, the
//!   authoritative state. Sent to a fresh subscriber and after `resync`.
//! - `resync` (no id): `{reason, seq}`. The requested position cannot be
//!   served from the buffer; discard local state and use the snapshot that
//!   follows (or re-pull `/workspaces`).
//! - `replay` (no id): `{from, to, count}`. Marks a resumed connection: the
//!   next `count` frames are buffered events after `from`, not live ones.
//! - `ready` (id = current seq): `{seq, replay}`. Everything before it was
//!   history; everything after it is live.
//! - the events themselves (id = seq), see [`DaemonEvent`].
//!
//! External change detection (`super::watcher`) publishes through
//! [`EventHub::publish_external`] and scans only while
//! [`EventHub::subscriber_count`] is above zero.

use super::error::ApiError;
use super::operations::OperationRegistry;
use super::{api, Activity, ActivityGuard};
use crate::cli::json_view;
use crate::config::workspace::WorkspaceStatus;
use crate::config::ConfigManager;
use axum::response::sse::Event;
use futures_util::Stream;
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::convert::Infallible;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::watch;

/// How much history the hub keeps for resuming clients.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventRetention {
    pub max_events: usize,
    pub max_age: Duration,
}

impl Default for EventRetention {
    fn default() -> Self {
        Self {
            max_events: 1000,
            max_age: Duration::from_secs(10 * 60),
        }
    }
}

/// An event a producer publishes. The hub adds `seq` and `at` to the data.
#[derive(Debug, Clone)]
pub enum DaemonEvent {
    /// Created, changed, started, archived: `workspace` is the complete
    /// `--json` summary (same structure as `GET /workspaces` entries).
    WorkspaceChanged { summary: Value },
    /// The Workspace no longer exists.
    WorkspaceRemoved { name: String },
    OperationStarted {
        operation_id: String,
        kind: &'static str,
        workspace: String,
        started_at: String,
    },
    /// `phase`: `step`, `step_started`, `step_finished` (with `ok`), `warning`.
    OperationProgress {
        operation_id: String,
        workspace: String,
        phase: &'static str,
        message: String,
        ok: Option<bool>,
    },
    /// One line of output from a child process.
    OperationLog {
        operation_id: String,
        workspace: String,
        message: String,
    },
    OperationFinished {
        operation_id: String,
        kind: &'static str,
        workspace: String,
        /// `succeeded` or `failed`.
        status: &'static str,
        result: Value,
        error: Value,
        finished_at: Option<String>,
    },
    /// A registered repository changed; `action` is `added`, `removed` or
    /// `updated`.
    RepoChanged { name: String, action: &'static str },
}

impl DaemonEvent {
    pub fn name(&self) -> &'static str {
        match self {
            Self::WorkspaceChanged { .. } => "workspace.changed",
            Self::WorkspaceRemoved { .. } => "workspace.removed",
            Self::OperationStarted { .. } => "operation.started",
            Self::OperationProgress { .. } => "operation.progress",
            Self::OperationLog { .. } => "operation.log",
            Self::OperationFinished { .. } => "operation.finished",
            Self::RepoChanged { .. } => "repo.changed",
        }
    }

    fn data(&self) -> Value {
        match self {
            Self::WorkspaceChanged { summary } => json!({ "workspace": summary }),
            Self::WorkspaceRemoved { name } => json!({ "name": name }),
            Self::OperationStarted {
                operation_id,
                kind,
                workspace,
                started_at,
            } => json!({
                "operation_id": operation_id, "kind": kind,
                "workspace": workspace, "started_at": started_at,
            }),
            Self::OperationProgress {
                operation_id,
                workspace,
                phase,
                message,
                ok,
            } => {
                let mut v = json!({
                    "operation_id": operation_id, "workspace": workspace,
                    "phase": phase, "message": message,
                });
                if let Some(ok) = ok {
                    v["ok"] = json!(ok);
                }
                v
            }
            Self::OperationLog {
                operation_id,
                workspace,
                message,
            } => json!({
                "operation_id": operation_id, "workspace": workspace, "message": message,
            }),
            Self::OperationFinished {
                operation_id,
                kind,
                workspace,
                status,
                result,
                error,
                finished_at,
            } => json!({
                "operation_id": operation_id, "kind": kind, "workspace": workspace,
                "status": status, "result": result, "error": error,
                "finished_at": finished_at,
            }),
            Self::RepoChanged { name, action } => json!({ "name": name, "action": action }),
        }
    }
}

/// What anything that detects or causes a change talks to. Kept tiny so the
/// external change detector (and tests) can stand in for the hub.
pub trait EventProducer: Send + Sync {
    fn publish(&self, event: DaemonEvent);
}

#[derive(Debug, Clone)]
struct Stored {
    seq: u64,
    at: Instant,
    name: &'static str,
    data: Value,
}

impl Stored {
    fn frame(&self) -> Event {
        Event::default()
            .id(self.seq.to_string())
            .event(self.name)
            .data(self.data.to_string())
    }
}

#[derive(Default)]
struct Inner {
    last_seq: u64,
    /// Highest sequence number already evicted; a client at or above it can
    /// still be served everything after its position.
    dropped_through: u64,
    buffer: VecDeque<Stored>,
    /// What the Daemon's own producers last said about each Workspace and
    /// repository (sequence number and content), so the external change
    /// detector can tell its findings apart from the Daemon's own work.
    own_workspaces: HashMap<String, (u64, Option<Value>)>,
    own_repos: HashMap<String, (u64, &'static str)>,
}

#[derive(Clone, Copy)]
struct Signal {
    seq: u64,
    closed: bool,
}

pub struct EventHub {
    inner: Mutex<Inner>,
    signal: watch::Sender<Signal>,
    retention: EventRetention,
    subscribers: AtomicUsize,
}

enum Start {
    Fresh,
    Resumed { from: u64, replay: Vec<Stored> },
    Resync { reason: &'static str },
}

/// Events after a position, or the fact that the position was evicted.
enum Since {
    Events(Vec<Stored>),
    Lagged,
}

impl EventHub {
    pub fn new(retention: EventRetention) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(Inner::default()),
            signal: watch::channel(Signal {
                seq: 0,
                closed: false,
            })
            .0,
            retention,
            subscribers: AtomicUsize::new(0),
        })
    }

    /// Number of open event streams.
    pub fn subscriber_count(&self) -> usize {
        self.subscribers.load(Ordering::SeqCst)
    }

    /// Ends every open stream (Daemon shutdown); later streams end at once.
    pub fn close(&self) {
        self.signal.send_modify(|s| s.closed = true);
    }

    fn prune(&self, inner: &mut Inner) {
        while let Some(front) = inner.buffer.front() {
            if inner.buffer.len() > self.retention.max_events
                || front.at.elapsed() > self.retention.max_age
            {
                inner.dropped_through = front.seq;
                inner.buffer.pop_front();
            } else {
                break;
            }
        }
    }

    /// Fixes a new subscriber's position and decides how to serve it, in one
    /// step under the lock. Returns the position (`last_seq` at this moment).
    fn begin(&self, last_event_id: Option<u64>) -> (Start, u64, watch::Receiver<Signal>) {
        let mut inner = self.inner.lock().unwrap();
        self.prune(&mut inner);
        let rx = self.signal.subscribe();
        let cursor = inner.last_seq;
        let start = match last_event_id {
            None => Start::Fresh,
            // Ahead of us: the id came from a previous Daemon run.
            Some(n) if n > cursor => Start::Resync {
                reason: "unknown_position",
            },
            Some(n) if n < inner.dropped_through => Start::Resync {
                reason: "gap_too_large",
            },
            Some(n) => Start::Resumed {
                from: n,
                replay: inner.buffer.iter().filter(|e| e.seq > n).cloned().collect(),
            },
        };
        (start, cursor, rx)
    }

    fn since(&self, cursor: u64) -> Since {
        let mut inner = self.inner.lock().unwrap();
        self.prune(&mut inner);
        if cursor < inner.dropped_through {
            return Since::Lagged;
        }
        Since::Events(
            inner
                .buffer
                .iter()
                .filter(|e| e.seq > cursor)
                .cloned()
                .collect(),
        )
    }
}

impl EventHub {
    /// Sequence number of the newest event (0 before the first).
    pub fn current_seq(&self) -> u64 {
        self.inner.lock().unwrap().last_seq
    }

    /// Publishes an event found by the external change detector. Unlike
    /// [`EventProducer::publish`] it is not remembered as the Daemon's own
    /// statement about the target.
    pub fn publish_external(&self, event: DaemonEvent) {
        self.push(event, false);
    }

    /// The Daemon's own newest statement about a Workspace made after
    /// sequence number `since`: `Some(Some(summary))` for `workspace.changed`,
    /// `Some(None)` for `workspace.removed`.
    pub fn own_workspace_since(&self, name: &str, since: u64) -> Option<Option<Value>> {
        let inner = self.inner.lock().unwrap();
        match inner.own_workspaces.get(name) {
            Some((seq, state)) if *seq > since => Some(state.clone()),
            _ => None,
        }
    }

    /// The Daemon's own newest `repo.changed` action for a repository made
    /// after sequence number `since`.
    pub fn own_repo_since(&self, name: &str, since: u64) -> Option<&'static str> {
        let inner = self.inner.lock().unwrap();
        match inner.own_repos.get(name) {
            Some((seq, action)) if *seq > since => Some(action),
            _ => None,
        }
    }

    fn push(&self, event: DaemonEvent, own: bool) {
        let mut inner = self.inner.lock().unwrap();
        inner.last_seq += 1;
        let seq = inner.last_seq;
        if own {
            match &event {
                DaemonEvent::WorkspaceChanged { summary } => {
                    if let Some(name) = summary.get("name").and_then(Value::as_str) {
                        inner
                            .own_workspaces
                            .insert(name.to_string(), (seq, Some(summary.clone())));
                    }
                }
                DaemonEvent::WorkspaceRemoved { name } => {
                    inner.own_workspaces.insert(name.clone(), (seq, None));
                }
                DaemonEvent::RepoChanged { name, action } => {
                    inner.own_repos.insert(name.clone(), (seq, action));
                }
                _ => {}
            }
        }
        let mut data = event.data();
        data["seq"] = json!(seq);
        data["at"] = json!(chrono::Utc::now().to_rfc3339());
        inner.buffer.push_back(Stored {
            seq,
            at: Instant::now(),
            name: event.name(),
            data,
        });
        self.prune(&mut inner);
        self.signal.send_modify(|s| s.seq = seq);
    }
}

impl EventProducer for EventHub {
    fn publish(&self, event: DaemonEvent) {
        self.push(event, true);
    }
}

/// Publishes the current state of one Workspace after something may have
/// changed it: `workspace.changed` with the full summary, or
/// `workspace.removed` when it no longer exists. Used for the Daemon's own
/// operations, which therefore never depend on scanning.
pub fn announce_workspace(events: &dyn EventProducer, mgr: &ConfigManager, name: &str) {
    match mgr.load_workspace(name) {
        Ok((status, config)) => {
            if let Ok(summary) =
                serde_json::to_value(json_view::workspace_summary(&status, &config))
            {
                events.publish(DaemonEvent::WorkspaceChanged { summary });
            }
        }
        // Unreadable but present (mid-write): say nothing, a later change
        // will announce it.
        Err(_) if mgr.locate_workspace_config(name).is_ok() => {}
        Err(_) => events.publish(DaemonEvent::WorkspaceRemoved {
            name: name.to_string(),
        }),
    }
}

/// Announces what a finished Daemon operation may have changed. `key` is the
/// Operation's target: a Workspace name, or `repo:<name>` for repositories.
/// After a failure only a Workspace that still exists is re-announced (it may
/// be partially changed); a removal is announced only after a success.
pub fn announce_outcome(
    events: &dyn EventProducer,
    mgr: &ConfigManager,
    kind: &str,
    key: &str,
    succeeded: bool,
) {
    if let Some(repo) = key.strip_prefix("repo:") {
        if succeeded {
            let action = if kind == "repo-remove" {
                "removed"
            } else {
                "added"
            };
            events.publish(DaemonEvent::RepoChanged {
                name: repo.to_string(),
                action,
            });
        }
    } else if succeeded || mgr.locate_workspace_config(key).is_ok() {
        announce_workspace(events, mgr, key);
    }
}

fn snapshot(
    mgr: &ConfigManager,
    operations: &OperationRegistry,
    seq: u64,
) -> Result<Value, ApiError> {
    let all = [
        WorkspaceStatus::Pending,
        WorkspaceStatus::InProgress,
        WorkspaceStatus::Done,
        WorkspaceStatus::Canceled,
    ];
    Ok(json!({
        "seq": seq,
        "workspaces": api::list_workspaces(mgr, &all)?,
        "operations": operations.running_views(),
    }))
}

fn control(name: &'static str, data: Value) -> Event {
    Event::default().event(name).data(data.to_string())
}

struct Live {
    hub: Arc<EventHub>,
    rx: watch::Receiver<Signal>,
    cursor: u64,
    queue: VecDeque<Event>,
    done: bool,
    _subscriber: Subscriber,
    _activity: ActivityGuard,
}

/// Counts an open stream; an open stream also keeps the Daemon from idling out.
struct Subscriber(Arc<EventHub>);

impl Drop for Subscriber {
    fn drop(&mut self) {
        self.0.subscribers.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Builds the SSE stream for one subscriber: backlog (snapshot or replay),
/// `ready`, then live events until the client leaves or the Daemon stops.
pub async fn subscribe(
    hub: Arc<EventHub>,
    operations: Arc<OperationRegistry>,
    activity: &Arc<Activity>,
    base_dir: std::path::PathBuf,
    last_event_id: Option<u64>,
) -> Result<impl Stream<Item = Result<Event, Infallible>>, ApiError> {
    hub.subscribers.fetch_add(1, Ordering::SeqCst);
    let subscriber = Subscriber(hub.clone());
    // 1. listen: the position is fixed before the snapshot is read.
    let (start, cursor, rx) = hub.begin(last_event_id);
    let mut queue = VecDeque::new();
    let replayed = matches!(start, Start::Resumed { .. });
    let mut needs_snapshot = true;
    match start {
        Start::Fresh => {}
        Start::Resync { reason } => {
            queue.push_back(control(
                "resync",
                json!({ "reason": reason, "seq": cursor }),
            ));
        }
        Start::Resumed { from, replay } => {
            needs_snapshot = false;
            queue.push_back(control(
                "replay",
                json!({ "from": from, "to": cursor, "count": replay.len() }),
            ));
            queue.extend(replay.iter().map(Stored::frame));
        }
    }
    // 2. snapshot.
    if needs_snapshot {
        let mgr = ConfigManager::with_base_dir(base_dir);
        let snap = tokio::task::spawn_blocking(move || snapshot(&mgr, &operations, cursor))
            .await
            .map_err(|e| ApiError::internal(format!("snapshot task failed: {e}")))??;
        queue.push_back(control("snapshot", snap));
    }
    // 3. ready.
    queue.push_back(
        Event::default()
            .id(cursor.to_string())
            .event("ready")
            .data(json!({ "seq": cursor, "replay": replayed }).to_string()),
    );
    let state = Live {
        hub,
        rx,
        cursor,
        queue,
        done: false,
        _subscriber: subscriber,
        _activity: activity.begin_request(),
    };
    Ok(futures_util::stream::unfold(state, |mut s| async move {
        loop {
            if let Some(frame) = s.queue.pop_front() {
                return Some((Ok(frame), s));
            }
            if s.done {
                return None;
            }
            match s.hub.since(s.cursor) {
                Since::Lagged => {
                    // Too slow: the events we owed were evicted. Tell the
                    // client and hang up; its reconnect gets a fresh snapshot.
                    let seq = s.hub.inner.lock().unwrap().last_seq;
                    s.queue
                        .push_back(control("resync", json!({ "reason": "lagged", "seq": seq })));
                    s.done = true;
                    continue;
                }
                Since::Events(events) if !events.is_empty() => {
                    s.cursor = events.last().map_or(s.cursor, |e| e.seq);
                    s.queue.extend(events.iter().map(Stored::frame));
                    continue;
                }
                Since::Events(_) => {}
            }
            if s.rx.borrow().closed {
                return None;
            }
            if s.rx.changed().await.is_err() {
                return None;
            }
        }
    }))
}
