//! External change detection through the Daemon's event stream: Workspace and
//! repository changes made behind the Daemon's back become events, nothing is
//! scanned without a subscriber, the Daemon's own work is not announced twice,
//! and temporary or half-written files are ignored. The Daemon runs in-process
//! with a very short scan interval.

use serde_json::json;
use std::os::unix::process::ExitStatusExt;
use std::process::{ExitStatus, Output};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tempfile::TempDir;
use zootree::config::global::MultiplexerConfig;
use zootree::config::repo::RepoConfig;
use zootree::config::workspace::{RepoEntry, WorkspaceConfig, WorkspaceStatus};
use zootree::config::ConfigManager;
use zootree::daemon::client::{request_with_body, EventStream, SseFrame};
use zootree::daemon::operations::RunnerFactory;
use zootree::daemon::watcher::{ChangeDetector, DetectedChange, DetectorFactory};
use zootree::daemon::{self, DaemonHandle, DaemonOptions};
use zootree::runner::{CommandRunner, CommandSpec};

const T: Duration = Duration::from_secs(5);
const SCAN: Duration = Duration::from_millis(40);
/// Long enough for a baseline scan and several more.
const SETTLE: Duration = Duration::from_millis(300);

struct OkRunner;

impl CommandRunner for OkRunner {
    fn run(&self, _spec: &CommandSpec) -> anyhow::Result<Output> {
        Ok(Output {
            status: ExitStatus::from_raw(0),
            stdout: Vec::new(),
            stderr: Vec::new(),
        })
    }

    fn run_interactive(&self, _spec: &CommandSpec) -> anyhow::Result<ExitStatus> {
        Ok(ExitStatus::from_raw(0))
    }
}

struct Env {
    tmp: TempDir,
    mgr: ConfigManager,
}

fn env() -> Env {
    let tmp = TempDir::new().unwrap();
    let mgr = ConfigManager::with_base_dir(tmp.path().join("config"));
    mgr.ensure_dirs().unwrap();
    Env { tmp, mgr }
}

fn workspace(env: &Env, name: &str, title: &str) -> WorkspaceConfig {
    WorkspaceConfig {
        title: title.into(),
        name: name.into(),
        description: String::new(),
        branch: format!("zootree/{name}"),
        workspace_dir: env
            .tmp
            .path()
            .join("ws")
            .join(name)
            .to_string_lossy()
            .into_owned(),
        created_at: "2026-06-29T10:00:00+08:00".into(),
        agent_cli: None,
        multiplexer: MultiplexerConfig::default(),
        multiplexer_state: Default::default(),
        repos: Vec::<RepoEntry>::new(),
        events: Vec::new(),
    }
}

fn save(env: &Env, status: WorkspaceStatus, name: &str, title: &str) {
    env.mgr
        .save_workspace(&status, &workspace(env, name, title))
        .unwrap();
}

fn repo_config() -> RepoConfig {
    RepoConfig {
        path: "/tmp/somewhere".into(),
        default_target_branch: None,
        copy_files: Vec::new(),
        hooks: Default::default(),
        lazygit: None,
    }
}

fn start(env: &Env, tweak: impl FnOnce(&mut DaemonOptions)) -> DaemonHandle {
    let mut options = DaemonOptions::new(env.mgr.base_dir.clone());
    options.idle_timeout = Duration::from_secs(60);
    options.runner = RunnerFactory::new(|| Box::new(OkRunner));
    options.scan_interval = SCAN;
    tweak(&mut options);
    daemon::start(options).unwrap()
}

fn subscribe(daemon: &DaemonHandle) -> EventStream {
    let mut stream = EventStream::connect(&daemon.socket, Some(&daemon.token), &[], None).unwrap();
    loop {
        if next(&mut stream).event == "ready" {
            break;
        }
    }
    // Let the detector record its baseline before the test changes anything.
    std::thread::sleep(SETTLE);
    stream
}

fn next(stream: &mut EventStream) -> SseFrame {
    stream.next_frame(T).unwrap().expect("stream ended")
}

/// The next frame that is not an Operation event.
fn next_change(stream: &mut EventStream) -> SseFrame {
    loop {
        let frame = next(stream);
        if !frame.event.starts_with("operation.") {
            return frame;
        }
    }
}

/// Asserts that no frame arrives for a while.
fn quiet(stream: &mut EventStream, period: Duration) {
    if let Ok(frame) = stream.next_frame(period) {
        panic!("unexpected frame: {frame:?}");
    }
}

fn stop(daemon: DaemonHandle) {
    daemon.shutdown();
    daemon.join().unwrap();
}

fn dir(env: &Env, rel: &str) -> std::path::PathBuf {
    env.mgr.base_dir.join(rel)
}

// ---- Workspaces -------------------------------------------------------------

#[test]
fn external_create_edit_move_and_delete_of_a_workspace_become_events() {
    let env = env();
    let daemon = start(&env, |_| {});
    let mut stream = subscribe(&daemon);

    save(&env, WorkspaceStatus::InProgress, "alpha", "One");
    let created = next_change(&mut stream);
    assert_eq!(created.event, "workspace.changed");
    assert_eq!(created.data["workspace"]["name"], "alpha");
    assert_eq!(created.data["workspace"]["status"], "in_progress");
    assert_eq!(created.data["workspace"]["title"], "One");

    save(&env, WorkspaceStatus::InProgress, "alpha", "Changed title");
    let edited = next_change(&mut stream);
    assert_eq!(edited.event, "workspace.changed");
    assert_eq!(edited.data["workspace"]["title"], "Changed title");

    // A state change is a file moving between directories.
    std::fs::rename(
        dir(&env, "workspaces/in_progress/alpha.toml"),
        dir(&env, "workspaces/archived/done/alpha.toml"),
    )
    .unwrap();
    let moved = next_change(&mut stream);
    assert_eq!(moved.event, "workspace.changed");
    assert_eq!(moved.data["workspace"]["status"], "done");

    std::fs::remove_file(dir(&env, "workspaces/archived/done/alpha.toml")).unwrap();
    let removed = next_change(&mut stream);
    assert_eq!(removed.event, "workspace.removed");
    assert_eq!(removed.data["name"], "alpha");

    quiet(&mut stream, SETTLE);
    stop(daemon);
}

#[test]
fn consecutive_edits_with_the_same_size_are_each_noticed() {
    let env = env();
    save(&env, WorkspaceStatus::InProgress, "alpha", "AAAA");
    let daemon = start(&env, |_| {});
    let mut stream = subscribe(&daemon);

    // Same size, written back to back (well within one second).
    for title in ["BBBB", "CCCC", "DDDD"] {
        let path = dir(&env, "workspaces/in_progress/alpha.toml");
        let old = std::fs::read_to_string(&path).unwrap();
        let current = old
            .lines()
            .find_map(|l| l.strip_prefix("title = \"")?.strip_suffix('"'))
            .unwrap()
            .to_string();
        std::fs::write(&path, old.replace(&current, title)).unwrap();
        let event = next_change(&mut stream);
        assert_eq!(event.event, "workspace.changed");
        assert_eq!(event.data["workspace"]["title"], title);
    }
    stop(daemon);
}

#[test]
fn temporary_files_and_half_written_configs_are_ignored() {
    let env = env();
    let daemon = start(&env, |_| {});
    let mut stream = subscribe(&daemon);

    // What an atomic write leaves behind while in flight.
    let tmp = dir(&env, "workspaces/in_progress/.alpha.toml.123.456.tmp");
    std::fs::write(&tmp, "title = \"half").unwrap();
    quiet(&mut stream, SETTLE);
    std::fs::remove_file(&tmp).unwrap();

    // A truncated config is not reported until it parses.
    let path = dir(&env, "workspaces/in_progress/alpha.toml");
    std::fs::write(&path, "title = \"half").unwrap();
    quiet(&mut stream, SETTLE);
    save(&env, WorkspaceStatus::InProgress, "alpha", "Whole");
    let event = next_change(&mut stream);
    assert_eq!(event.event, "workspace.changed");
    assert_eq!(event.data["workspace"]["title"], "Whole");
    stop(daemon);
}

#[test]
fn the_daemons_own_operation_is_announced_once() {
    let env = env();
    save(&env, WorkspaceStatus::InProgress, "alpha", "One");
    let daemon = start(&env, |_| {});
    let mut stream = subscribe(&daemon);

    let body = json!({"no_merge": true, "no_clean": true}).to_string();
    let response = request_with_body(
        &daemon.socket,
        "POST",
        "/workspaces/alpha/done",
        Some(&daemon.token),
        &[],
        Some(&body),
        T,
    )
    .unwrap();
    assert_eq!(response.status, 202, "{}", response.body);

    let mut changed = 0;
    loop {
        let frame = next(&mut stream);
        match frame.event.as_str() {
            "workspace.changed" => changed += 1,
            "operation.finished" => break,
            _ => {}
        }
    }
    // The scanner sees the moved file too; it must not repeat the news.
    let deadline = Instant::now() + SETTLE * 2;
    while Instant::now() < deadline {
        if let Ok(Some(frame)) = stream.next_frame(Duration::from_millis(50)) {
            if frame.event == "workspace.changed" {
                changed += 1;
            }
        }
    }
    assert_eq!(changed, 1);
    stop(daemon);
}

// ---- repositories -----------------------------------------------------------

#[test]
fn external_repo_changes_become_repo_events() {
    let env = env();
    let daemon = start(&env, |_| {});
    let mut stream = subscribe(&daemon);

    env.mgr.save_repo_config("lib", &repo_config()).unwrap();
    let added = next_change(&mut stream);
    assert_eq!(added.event, "repo.changed");
    assert_eq!(added.data["name"], "lib");
    assert_eq!(added.data["action"], "added");

    let mut changed = repo_config();
    changed.path = "/tmp/elsewhere-and-longer".into();
    env.mgr.save_repo_config("lib", &changed).unwrap();
    let updated = next_change(&mut stream);
    assert_eq!(updated.data["action"], "updated");

    env.mgr.remove_repo_config("lib").unwrap();
    let removed = next_change(&mut stream);
    assert_eq!(removed.event, "repo.changed");
    assert_eq!(removed.data["action"], "removed");
    quiet(&mut stream, SETTLE);
    stop(daemon);
}

// ---- scanning only while someone listens, replaceable detector ---------------

struct Counting {
    scans: Arc<AtomicUsize>,
    resets: Arc<AtomicUsize>,
    script: Arc<Mutex<Vec<DetectedChange>>>,
}

impl ChangeDetector for Counting {
    fn scan(&mut self) -> Vec<DetectedChange> {
        self.scans.fetch_add(1, Ordering::SeqCst);
        std::mem::take(&mut *self.script.lock().unwrap())
    }

    fn reset(&mut self) {
        self.resets.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn nothing_is_scanned_without_subscribers_and_a_double_can_stand_in() {
    let env = env();
    let scans = Arc::new(AtomicUsize::new(0));
    let resets = Arc::new(AtomicUsize::new(0));
    let script = Arc::new(Mutex::new(Vec::new()));
    let daemon = start(&env, {
        let (scans, resets, script) = (scans.clone(), resets.clone(), script.clone());
        move |o| {
            o.detector = DetectorFactory::new(move |_| {
                Box::new(Counting {
                    scans: scans.clone(),
                    resets: resets.clone(),
                    script: script.clone(),
                })
            })
        }
    });

    std::thread::sleep(SETTLE);
    assert_eq!(
        scans.load(Ordering::SeqCst),
        0,
        "scanned with no subscriber"
    );

    let mut stream = subscribe(&daemon);
    assert!(
        scans.load(Ordering::SeqCst) >= 2,
        "baseline plus periodic scans"
    );

    // Whatever the double reports (after its baseline) is published.
    script
        .lock()
        .unwrap()
        .push(DetectedChange::WorkspaceRemoved {
            name: "ghost".into(),
        });
    let event = next_change(&mut stream);
    assert_eq!(event.event, "workspace.removed");
    assert_eq!(event.data["name"], "ghost");

    drop(stream);
    let deadline = Instant::now() + T;
    while resets.load(Ordering::SeqCst) == 0 {
        assert!(Instant::now() < deadline, "scanning never paused");
        std::thread::sleep(Duration::from_millis(20));
    }
    std::thread::sleep(SETTLE);
    let after_pause = scans.load(Ordering::SeqCst);
    std::thread::sleep(SETTLE);
    assert_eq!(
        scans.load(Ordering::SeqCst),
        after_pause,
        "scanned after the last subscriber left"
    );
    stop(daemon);
}
