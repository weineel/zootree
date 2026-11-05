//! External change detection.
//!
//! While at least one event stream is open, a background thread asks a
//! [`ChangeDetector`] for changes every [`DaemonOptions::scan_interval`]
//! (2 seconds by default) and publishes them as `workspace.changed`,
//! `workspace.removed` and `repo.changed`. With no subscriber nothing is
//! scanned. The detector sits behind a small trait so a file-system listener
//! can replace the polling one without touching the event API; tests use a
//! scripted detector.
//!
//! [`FsDetector`], the default, compares `(path, modification time in
//! nanoseconds, size, inode)` of every `*.toml` file in the four Workspace
//! state directories and in `repos/`. Size and inode back up the timestamp, so
//! two edits within the same second are not confused (and every atomic write,
//! which renames a new file into place, changes the inode). Dot files and
//! `*.tmp` files are the atomic-write temporaries and are never looked at. A
//! file that does not parse yet (caught mid-edit) is skipped and retried on
//! the next scan rather than reported.
//!
//! The Daemon's own work already publishes its events directly, so findings
//! that merely echo it are dropped (see [`Watcher`]): a Workspace or
//! repository with a running Operation is skipped, and a finding that equals
//! what the Daemon itself published since the previous scan is not repeated.
//!
//! The first scan after scanning (re)starts only records a baseline; the
//! subscriber's snapshot already covers everything before it.
//!
//! [`DaemonOptions::scan_interval`]: super::DaemonOptions::scan_interval

use super::events::{DaemonEvent, EventHub};
use crate::cli::json_view;
use crate::config::name;
use crate::config::ConfigManager;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Default time between scans while someone is subscribed.
pub const DEFAULT_SCAN_INTERVAL: Duration = Duration::from_secs(2);

/// How often the watcher thread checks whether anyone is subscribed (and
/// whether it must stop), at most.
const POLL_STEP: Duration = Duration::from_millis(50);

/// One change a [`ChangeDetector`] found.
#[derive(Debug, Clone, PartialEq)]
pub enum DetectedChange {
    /// Created, edited or moved to another state; `summary` is the complete
    /// `--json` summary (it carries the Workspace `name`).
    WorkspaceChanged {
        summary: Value,
    },
    WorkspaceRemoved {
        name: String,
    },
    /// `action`: `added`, `removed` or `updated`.
    RepoChanged {
        name: String,
        action: &'static str,
    },
}

/// Finds what changed outside the Daemon since the previous call.
pub trait ChangeDetector: Send {
    /// Changes since the previous `scan`. The first call after construction or
    /// [`reset`](Self::reset) only records the current state and returns
    /// nothing.
    fn scan(&mut self) -> Vec<DetectedChange>;

    /// Forgets the recorded state (scanning is being paused).
    fn reset(&mut self);
}

/// Builds the detector for a config directory; tests substitute a double.
#[derive(Clone)]
pub struct DetectorFactory(Arc<dyn Fn(ConfigManager) -> Box<dyn ChangeDetector> + Send + Sync>);

impl DetectorFactory {
    pub fn new(
        make: impl Fn(ConfigManager) -> Box<dyn ChangeDetector> + Send + Sync + 'static,
    ) -> Self {
        Self(Arc::new(make))
    }

    /// The polling detector over the real config directory.
    pub fn filesystem() -> Self {
        Self::new(|mgr| Box::new(FsDetector::new(mgr)))
    }

    pub(crate) fn make(&self, mgr: ConfigManager) -> Box<dyn ChangeDetector> {
        (self.0)(mgr)
    }
}

impl std::fmt::Debug for DetectorFactory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DetectorFactory")
    }
}

// ---- driver ---------------------------------------------------------------

/// Whether an Operation is running on a key (Workspace name or `repo:<name>`).
pub(crate) type BusyCheck = Arc<dyn Fn(&str) -> bool + Send + Sync>;

/// Running watcher thread; stop it with [`Watcher::stop`].
pub(crate) struct Watcher {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Watcher {
    pub(crate) fn spawn(
        detector: Box<dyn ChangeDetector>,
        hub: Arc<EventHub>,
        busy: BusyCheck,
        interval: Duration,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let stop = stop.clone();
            std::thread::Builder::new()
                .name("zootree-watcher".to_string())
                .spawn(move || run(detector, hub, busy, interval, stop))
                .ok()
        };
        Self { stop, thread }
    }

    pub(crate) fn stop(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run(
    mut detector: Box<dyn ChangeDetector>,
    hub: Arc<EventHub>,
    busy: BusyCheck,
    interval: Duration,
    stop: Arc<AtomicBool>,
) {
    let step = interval.min(POLL_STEP);
    let mut active = false;
    // Sequence number the previous scan started at; only the Daemon's own
    // events after it can explain a finding.
    let mut window_start = 0;
    let mut last_scan = Instant::now();
    while !stop.load(Ordering::SeqCst) {
        std::thread::sleep(step);
        if hub.subscriber_count() == 0 {
            if active {
                detector.reset();
                active = false;
            }
            continue;
        }
        let first = !active;
        if !first && last_scan.elapsed() < interval {
            continue;
        }
        active = true;
        last_scan = Instant::now();
        let started_at = hub.current_seq();
        let changes = detector.scan();
        if first {
            window_start = started_at;
            continue;
        }
        for change in changes {
            if let Some(event) = admit(&hub, &busy, window_start, change) {
                hub.publish_external(event);
            }
        }
        window_start = started_at;
    }
}

/// Turns a finding into an event, unless the Daemon's own work explains it.
fn admit(
    hub: &EventHub,
    busy: &BusyCheck,
    since: u64,
    change: DetectedChange,
) -> Option<DaemonEvent> {
    match change {
        DetectedChange::WorkspaceChanged { summary } => {
            let name = summary.get("name").and_then(Value::as_str)?.to_string();
            if busy(&name) {
                return None;
            }
            match hub.own_workspace_since(&name, since) {
                Some(Some(own)) if own == summary => None,
                _ => Some(DaemonEvent::WorkspaceChanged { summary }),
            }
        }
        DetectedChange::WorkspaceRemoved { name } => {
            if busy(&name) {
                return None;
            }
            match hub.own_workspace_since(&name, since) {
                Some(None) => None,
                _ => Some(DaemonEvent::WorkspaceRemoved { name }),
            }
        }
        DetectedChange::RepoChanged { name, action } => {
            if busy(&format!("repo:{name}")) {
                return None;
            }
            // Registering writes the file in more than one step, so an
            // "updated" right after our own "added" is still ours.
            let echoed = match hub.own_repo_since(&name, since) {
                Some(own) => own == action || (action == "updated" && own != "removed"),
                None => false,
            };
            (!echoed).then_some(DaemonEvent::RepoChanged { name, action })
        }
    }
}

// ---- polling detector -----------------------------------------------------

/// Identity of a file's content as far as a stat can tell.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Fingerprint {
    path: PathBuf,
    modified_ns: i128,
    size: u64,
    inode: u64,
}

impl Fingerprint {
    fn of(path: &Path) -> Option<Self> {
        use std::os::unix::fs::MetadataExt;
        let metadata = std::fs::metadata(path).ok()?;
        if !metadata.is_file() {
            return None;
        }
        Some(Self {
            path: path.to_path_buf(),
            modified_ns: i128::from(metadata.mtime()) * 1_000_000_000
                + i128::from(metadata.mtime_nsec()),
            size: metadata.len(),
            inode: metadata.ino(),
        })
    }
}

/// Polling [`ChangeDetector`] over the Workspace state directories and
/// `repos/`.
pub struct FsDetector {
    mgr: ConfigManager,
    workspaces: Option<HashMap<String, Fingerprint>>,
    repos: Option<HashMap<String, Fingerprint>>,
}

impl FsDetector {
    pub fn new(mgr: ConfigManager) -> Self {
        Self {
            mgr,
            workspaces: None,
            repos: None,
        }
    }

    /// `name -> fingerprint` of the `*.toml` files in a directory, skipping
    /// dot files, `*.tmp` temporaries and names that are not config names.
    fn list(dir: &Path, into: &mut HashMap<String, Fingerprint>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let file_name = entry.file_name();
            let Some(file_name) = file_name.to_str() else {
                continue;
            };
            if file_name.starts_with('.') || file_name.ends_with(".tmp") {
                continue;
            }
            let Some(stem) = file_name.strip_suffix(".toml") else {
                continue;
            };
            if !name::is_config_name(stem) {
                continue;
            }
            if let Some(fingerprint) = Fingerprint::of(&entry.path()) {
                // The same name in two states cannot happen through zootree;
                // the first state in lookup order wins, as it does elsewhere.
                into.entry(stem.to_string()).or_insert(fingerprint);
            }
        }
    }

    fn list_workspaces(&self) -> HashMap<String, Fingerprint> {
        let base = &self.mgr.base_dir;
        let mut found = HashMap::new();
        for dir in [
            base.join("workspaces/pending"),
            base.join("workspaces/in_progress"),
            base.join("workspaces/archived/done"),
            base.join("workspaces/archived/canceled"),
        ] {
            Self::list(&dir, &mut found);
        }
        found
    }

    fn scan_workspaces(&mut self, changes: &mut Vec<DetectedChange>) {
        let current = self.list_workspaces();
        let Some(previous) = self.workspaces.take() else {
            self.workspaces = Some(current);
            return;
        };
        let mut next = HashMap::new();
        let mut names: Vec<_> = current.keys().chain(previous.keys()).cloned().collect();
        names.sort();
        names.dedup();
        for name in names {
            match (current.get(&name), previous.get(&name)) {
                (Some(now), before) if before != Some(now) => {
                    match self.mgr.load_workspace(&name) {
                        Ok((status, config)) => {
                            if let Ok(summary) =
                                serde_json::to_value(json_view::workspace_summary(&status, &config))
                            {
                                changes.push(DetectedChange::WorkspaceChanged { summary });
                                next.insert(name, now.clone());
                            }
                        }
                        // Caught mid-edit or malformed: keep what we knew so
                        // the next scan looks at it again.
                        Err(_) => {
                            if let Some(before) = before {
                                next.insert(name, before.clone());
                            }
                        }
                    }
                }
                (Some(now), _) => {
                    next.insert(name, now.clone());
                }
                (None, Some(before)) => {
                    // The four directories are listed one after another, so a
                    // file moved meanwhile can be missed in both. Confirm.
                    if self.mgr.locate_workspace_config(&name).is_ok() {
                        next.insert(name, before.clone());
                    } else {
                        changes.push(DetectedChange::WorkspaceRemoved { name });
                    }
                }
                (None, None) => {}
            }
        }
        self.workspaces = Some(next);
    }

    fn scan_repos(&mut self, changes: &mut Vec<DetectedChange>) {
        let mut current = HashMap::new();
        Self::list(&self.mgr.repos_dir(), &mut current);
        let Some(previous) = self.repos.take() else {
            self.repos = Some(current);
            return;
        };
        let mut names: Vec<_> = current.keys().chain(previous.keys()).cloned().collect();
        names.sort();
        names.dedup();
        for name in names {
            let action = match (current.get(&name), previous.get(&name)) {
                (Some(_), None) => "added",
                (None, Some(_)) => "removed",
                (Some(now), Some(before)) if now != before => "updated",
                _ => continue,
            };
            changes.push(DetectedChange::RepoChanged { name, action });
        }
        self.repos = Some(current);
    }
}

impl ChangeDetector for FsDetector {
    fn scan(&mut self) -> Vec<DetectedChange> {
        let mut changes = Vec::new();
        self.scan_workspaces(&mut changes);
        self.scan_repos(&mut changes);
        changes
    }

    fn reset(&mut self) {
        self.workspaces = None;
        self.repos = None;
    }
}
