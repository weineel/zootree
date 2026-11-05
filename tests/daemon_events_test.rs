//! Event stream tests through the Daemon's HTTP seam (`GET /events`): snapshot
//! then ready, sequence numbers, resume with `Last-Event-ID`, resync, replay
//! marker, live Operation progress, independent subscribers, shutdown with an
//! open stream. The Daemon runs in-process on a temporary unix socket with a
//! test runner double.

use serde_json::{json, Value};
use std::os::unix::process::ExitStatusExt;
use std::process::{ExitStatus, Output};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};
use tempfile::TempDir;
use zootree::config::global::{GlobalConfig, HookValue, MultiplexerConfig};
use zootree::config::workspace::{RepoEntry, WorkspaceConfig, WorkspaceStatus};
use zootree::config::ConfigManager;
use zootree::daemon::client::{request_with_body, EventStream, SseFrame};
use zootree::daemon::events::EventRetention;
use zootree::daemon::operations::RunnerFactory;
use zootree::daemon::{self, DaemonHandle, DaemonOptions};
use zootree::runner::{CommandRunner, CommandSpec};

const T: Duration = Duration::from_secs(5);

// ---- runner double --------------------------------------------------------

#[derive(Default)]
struct Gate {
    open: Mutex<bool>,
    changed: Condvar,
}

impl Gate {
    fn release(&self) {
        *self.open.lock().unwrap() = true;
        self.changed.notify_all();
    }

    fn wait(&self) {
        let mut open = self.open.lock().unwrap();
        while !*open {
            open = self.changed.wait(open).unwrap();
        }
    }
}

#[derive(Clone, Default)]
struct Script {
    gate: Option<Arc<Gate>>,
    stdout: String,
    fail: bool,
}

struct ScriptRunner(Script);

impl CommandRunner for ScriptRunner {
    fn run(&self, _spec: &CommandSpec) -> anyhow::Result<Output> {
        if let Some(gate) = &self.0.gate {
            gate.wait();
        }
        Ok(Output {
            status: ExitStatus::from_raw(if self.0.fail { 1 << 8 } else { 0 }),
            stdout: self.0.stdout.clone().into_bytes(),
            stderr: Vec::new(),
        })
    }

    fn run_interactive(&self, _spec: &CommandSpec) -> anyhow::Result<ExitStatus> {
        Ok(ExitStatus::from_raw(0))
    }
}

// ---- fixtures -------------------------------------------------------------

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

fn add_workspace(env: &Env, name: &str) {
    let ws = WorkspaceConfig {
        title: "Title".into(),
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
    };
    env.mgr
        .save_workspace(&WorkspaceStatus::InProgress, &ws)
        .unwrap();
}

fn set_pre_done_hook(env: &Env) {
    let mut global = GlobalConfig::default();
    global.hooks.pre_done = Some(HookValue::Simple("build".into()));
    env.mgr.save_global_config(&global).unwrap();
}

fn start(env: &Env, script: Script, tweak: impl FnOnce(&mut DaemonOptions)) -> DaemonHandle {
    let mut options = DaemonOptions::new(env.mgr.base_dir.clone());
    options.idle_timeout = Duration::from_secs(60);
    options.runner = RunnerFactory::new(move || Box::new(ScriptRunner(script.clone())));
    tweak(&mut options);
    daemon::start(options).unwrap()
}

fn subscribe(daemon: &DaemonHandle, last_event_id: Option<u64>) -> EventStream {
    EventStream::connect(&daemon.socket, Some(&daemon.token), &[], last_event_id).unwrap()
}

fn next(stream: &mut EventStream) -> SseFrame {
    stream.next_frame(T).unwrap().expect("stream ended")
}

/// Frames up to and including `ready`.
fn until_ready(stream: &mut EventStream) -> Vec<SseFrame> {
    let mut out = Vec::new();
    loop {
        let frame = next(stream);
        let done = frame.event == "ready";
        out.push(frame);
        if done {
            return out;
        }
    }
}

/// Frames up to and including the first one named `name`.
fn until(stream: &mut EventStream, name: &str) -> Vec<SseFrame> {
    let mut out = Vec::new();
    loop {
        let frame = next(stream);
        let done = frame.event == name;
        out.push(frame);
        if done {
            return out;
        }
    }
}

fn names(frames: &[SseFrame]) -> Vec<&str> {
    frames.iter().map(|f| f.event.as_str()).collect()
}

fn seq(frame: &SseFrame) -> u64 {
    frame.id.as_ref().expect("event has an id").parse().unwrap()
}

fn post(daemon: &DaemonHandle, path: &str, body: Value) -> String {
    let body = body.to_string();
    let response = request_with_body(
        &daemon.socket,
        "POST",
        path,
        Some(&daemon.token),
        &[],
        Some(&body),
        T,
    )
    .unwrap();
    assert_eq!(response.status, 202, "{}", response.body);
    response.json().unwrap()["id"].as_str().unwrap().to_string()
}

fn done(daemon: &DaemonHandle, name: &str) -> String {
    post(
        daemon,
        &format!("/workspaces/{name}/done"),
        json!({"no_merge": true, "no_clean": true}),
    )
}

fn stop(daemon: DaemonHandle) {
    daemon.shutdown();
    daemon.join().unwrap();
}

// ---- tests ----------------------------------------------------------------

#[test]
fn fresh_subscriber_gets_snapshot_then_ready() {
    let env = env();
    add_workspace(&env, "alpha");
    let daemon = start(&env, Script::default(), |_| {});

    let mut stream = subscribe(&daemon, None);
    let frames = until_ready(&mut stream);
    assert_eq!(names(&frames), ["snapshot", "ready"]);
    let workspaces = frames[0].data["workspaces"].as_array().unwrap();
    assert_eq!(workspaces.len(), 1);
    assert_eq!(workspaces[0]["name"], "alpha");
    assert_eq!(workspaces[0]["status"], "in_progress");
    assert_eq!(frames[1].data["replay"], false);
    assert_eq!(frames[1].data["seq"], frames[0].data["seq"]);
    stop(daemon);
}

#[test]
fn changes_after_ready_arrive_in_order_with_increasing_sequence() {
    let env = env();
    add_workspace(&env, "alpha");
    add_workspace(&env, "beta");
    let daemon = start(&env, Script::default(), |_| {});
    let mut stream = subscribe(&daemon, None);
    until_ready(&mut stream);

    done(&daemon, "alpha");
    let a = until(&mut stream, "operation.finished");
    done(&daemon, "beta");
    let b = until(&mut stream, "operation.finished");

    let all: Vec<&SseFrame> = a.iter().chain(b.iter()).collect();
    let seqs: Vec<u64> = all.iter().map(|f| seq(f)).collect();
    assert!(seqs.windows(2).all(|w| w[0] < w[1]), "{seqs:?}");
    // The payload repeats the sequence number.
    assert!(all.iter().all(|f| f.data["seq"] == json!(seq(f))));
    stop(daemon);
}

#[test]
fn operation_progress_and_logs_arrive_while_it_runs() {
    let env = env();
    set_pre_done_hook(&env);
    add_workspace(&env, "alpha");
    let gate = Arc::new(Gate::default());
    let daemon = start(
        &env,
        Script {
            gate: Some(gate.clone()),
            stdout: "compiling\n".into(),
            fail: false,
        },
        |_| {},
    );
    let mut stream = subscribe(&daemon, None);
    until_ready(&mut stream);

    let id = done(&daemon, "alpha");
    // The runner is blocked, yet started and the step start are already here.
    let live = until(&mut stream, "operation.progress");
    assert_eq!(names(&live), ["operation.started", "operation.progress"]);
    assert_eq!(live[0].data["operation_id"], id);
    assert_eq!(live[0].data["kind"], "done");
    assert_eq!(live[0].data["workspace"], "alpha");
    assert_eq!(live[1].data["phase"], "step_started");
    assert_eq!(live[1].data["message"], "pre_done");

    gate.release();
    let rest = until(&mut stream, "operation.finished");
    let rest_names = names(&rest);
    let log = rest.iter().find(|f| f.event == "operation.log").unwrap();
    assert_eq!(log.data["message"], "[pre_done] compiling");
    assert_eq!(log.data["operation_id"], id);
    let finished_step = rest
        .iter()
        .find(|f| f.data["phase"] == "step_finished")
        .unwrap();
    assert_eq!(finished_step.data["ok"], true);
    // The Workspace change is announced before the Operation is finished.
    let changed = rest
        .iter()
        .position(|f| f.event == "workspace.changed")
        .unwrap();
    assert_eq!(changed, rest_names.len() - 2, "{rest_names:?}");
    assert_eq!(rest[changed].data["workspace"]["name"], "alpha");
    assert_eq!(rest[changed].data["workspace"]["status"], "done");
    let finished = rest.last().unwrap();
    assert_eq!(finished.data["status"], "succeeded");
    assert_eq!(
        finished.data["result"],
        json!({"workspace": "alpha", "dry_run": false})
    );
    stop(daemon);
}

#[test]
fn failed_operation_is_finished_with_its_error() {
    let env = env();
    set_pre_done_hook(&env);
    add_workspace(&env, "alpha");
    let daemon = start(
        &env,
        Script {
            fail: true,
            ..Default::default()
        },
        |_| {},
    );
    let mut stream = subscribe(&daemon, None);
    until_ready(&mut stream);

    // The pre_done hook fails, so the Operation fails.
    done(&daemon, "alpha");
    let frames = until(&mut stream, "operation.finished");
    let finished = frames.last().unwrap();
    assert_eq!(finished.data["status"], "failed");
    assert!(finished.data["result"].is_null());
    assert_eq!(finished.data["error"]["code"], "operation_failed");
    stop(daemon);
}

#[test]
fn resume_replays_only_later_events_and_marks_the_replay() {
    let env = env();
    add_workspace(&env, "alpha");
    add_workspace(&env, "beta");
    let daemon = start(&env, Script::default(), |_| {});
    let mut first = subscribe(&daemon, None);
    until_ready(&mut first);
    done(&daemon, "alpha");
    let seen = until(&mut first, "operation.finished");
    let last_seen = seq(seen.last().unwrap());
    drop(first);

    // Happens while the client is away.
    done(&daemon, "beta");
    let mut probe = subscribe(&daemon, None);
    until_ready(&mut probe);
    // Wait until the missed Operation is over (probe was fresh: poll history).
    let mut second = subscribe(&daemon, Some(last_seen));
    let frames = until_ready(&mut second);
    assert_eq!(frames[0].event, "replay");
    assert_eq!(frames[0].data["from"], last_seen);
    let count = frames[0].data["count"].as_u64().unwrap() as usize;
    assert!(count >= 1);
    let replayed = &frames[1..frames.len() - 1];
    assert_eq!(replayed.len(), count);
    assert!(replayed.iter().all(|f| seq(f) > last_seen));
    assert!(replayed.windows(2).all(|w| seq(&w[0]) < seq(&w[1])));
    assert!(replayed
        .iter()
        .all(|f| f.data["workspace"]["name"] != json!("alpha")
            && f.data["workspace"] != json!("alpha")));
    let ready = frames.last().unwrap();
    assert_eq!(ready.data["replay"], true);
    assert!(!frames.iter().any(|f| f.event == "snapshot"));

    // The stream continues live after the replay without repeating anything.
    let before = seq(ready);
    let rest_id = {
        add_workspace(&env, "gamma");
        done(&daemon, "gamma")
    };
    let live = until(&mut second, "operation.finished");
    assert!(live.iter().all(|f| seq(f) > before));
    assert!(live
        .iter()
        .any(|f| f.data["operation_id"] == json!(rest_id)));
    stop(daemon);
}

#[test]
fn resume_at_the_latest_position_replays_nothing() {
    let env = env();
    add_workspace(&env, "alpha");
    let daemon = start(&env, Script::default(), |_| {});
    let mut first = subscribe(&daemon, None);
    until_ready(&mut first);
    done(&daemon, "alpha");
    let seen = until(&mut first, "operation.finished");
    let last_seen = seq(seen.last().unwrap());

    let mut second = subscribe(&daemon, Some(last_seen));
    let frames = until_ready(&mut second);
    assert_eq!(names(&frames), ["replay", "ready"]);
    assert_eq!(frames[0].data["count"], 0);
    stop(daemon);
}

#[test]
fn too_old_position_gets_resync_then_a_fresh_snapshot() {
    let env = env();
    add_workspace(&env, "alpha");
    let daemon = start(&env, Script::default(), |o| {
        o.event_retention = EventRetention {
            max_events: 2,
            max_age: Duration::from_secs(600),
        };
    });
    let mut first = subscribe(&daemon, None);
    until_ready(&mut first);
    done(&daemon, "alpha");
    let seen = until(&mut first, "operation.finished");
    // Three events outgrow a buffer of 2.
    assert!(seen.len() >= 3);
    drop(first);

    let mut second = subscribe(&daemon, Some(0));
    let frames = until_ready(&mut second);
    assert_eq!(names(&frames), ["resync", "snapshot", "ready"]);
    assert_eq!(frames[0].data["reason"], "gap_too_large");
    assert_eq!(frames[1].data["workspaces"][0]["status"], "done");
    assert_eq!(frames[2].data["replay"], false);
    stop(daemon);
}

#[test]
fn position_from_a_previous_daemon_run_gets_resync() {
    let env = env();
    let daemon = start(&env, Script::default(), |_| {});
    let mut stream = subscribe(&daemon, Some(999_999));
    let frames = until_ready(&mut stream);
    assert_eq!(names(&frames), ["resync", "snapshot", "ready"]);
    assert_eq!(frames[0].data["reason"], "unknown_position");
    stop(daemon);
}

#[test]
fn subscribers_are_independent() {
    let env = env();
    add_workspace(&env, "alpha");
    let daemon = start(&env, Script::default(), |_| {});
    let mut a = subscribe(&daemon, None);
    let mut b = subscribe(&daemon, None);
    let mut c = subscribe(&daemon, None);
    until_ready(&mut a);
    until_ready(&mut b);
    until_ready(&mut c);

    // One subscriber leaving mid-way changes nothing for the others.
    drop(c);
    done(&daemon, "alpha");
    let fa = until(&mut a, "operation.finished");
    let fb = until(&mut b, "operation.finished");
    assert_eq!(names(&fa), names(&fb));
    assert_eq!(
        fa.iter().map(seq).collect::<Vec<_>>(),
        fb.iter().map(seq).collect::<Vec<_>>()
    );
    stop(daemon);
}

#[test]
fn snapshot_lists_running_operations() {
    let env = env();
    add_workspace(&env, "alpha");
    set_pre_done_hook(&env);
    let gate = Arc::new(Gate::default());
    let daemon = start(
        &env,
        Script {
            gate: Some(gate.clone()),
            ..Default::default()
        },
        |_| {},
    );
    let id = done(&daemon, "alpha");
    let mut stream = subscribe(&daemon, None);
    let frames = until_ready(&mut stream);
    let ops = frames[0].data["operations"].as_array().unwrap();
    assert_eq!(ops.len(), 1);
    assert_eq!(ops[0]["id"], id);
    gate.release();
    until(&mut stream, "operation.finished");
    stop(daemon);
}

#[test]
fn an_open_stream_does_not_block_shutdown_and_keeps_the_daemon_alive() {
    let env = env();
    let daemon = start(&env, Script::default(), |o| {
        o.idle_timeout = Duration::from_millis(200);
    });
    let mut stream = subscribe(&daemon, None);
    until_ready(&mut stream);
    // Longer than the idle timeout: the open stream counts as a connection.
    assert!(stream.next_frame(Duration::from_millis(800)).is_err());

    daemon.shutdown();
    let started = Instant::now();
    daemon.join().unwrap();
    assert!(started.elapsed() < T);
    // The server ended the stream.
    let end = stream.next_frame(T);
    assert!(matches!(end, Ok(None)), "{end:?}");
}

#[test]
fn events_require_the_token_and_honour_version_rules() {
    let env = env();
    let daemon = start(&env, Script::default(), |_| {});

    let err = EventStream::connect(&daemon.socket, None, &[], None).unwrap_err();
    assert!(err.to_string().contains("401"), "{err}");
    let err = EventStream::connect(&daemon.socket, Some("wrong"), &[], None).unwrap_err();
    assert!(err.to_string().contains("401"), "{err}");

    let err = EventStream::connect(
        &daemon.socket,
        Some(&daemon.token),
        &[("X-Zootree-Protocol-Version", "999")],
        None,
    )
    .unwrap_err();
    assert!(err.to_string().contains("client_too_new"), "{err}");

    // A declared compatible version is served.
    let v = daemon::PROTOCOL_VERSION.to_string();
    let mut ok = EventStream::connect(
        &daemon.socket,
        Some(&daemon.token),
        &[("X-Zootree-Protocol-Version", &v)],
        None,
    )
    .unwrap();
    until_ready(&mut ok);

    let err = EventStream::connect(
        &daemon.socket,
        Some(&daemon.token),
        &[("Last-Event-ID", "abc")],
        None,
    )
    .unwrap_err();
    assert!(err.to_string().contains("bad_request"), "{err}");
    stop(daemon);
}

#[test]
fn events_capability_is_declared() {
    assert!(daemon::CAPABILITIES.contains(&"events"));
}
