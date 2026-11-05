//! Operation core tests through the Daemon's HTTP seam: done and cancel
//! requests, status/output/result queries, the per-Workspace busy rule, lock
//! conflicts with a concurrent command-line process, and record retention.
//! The Daemon runs in-process on a unix socket in a temporary directory with a
//! test runner double; nothing here touches the real ~/.config/zootree.

use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::os::unix::process::ExitStatusExt;
use std::process::{ExitStatus, Output};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};
use tempfile::TempDir;
use zootree::config::global::{GlobalConfig, HookValue, MultiplexerConfig};
use zootree::config::repo::RepoConfig;
use zootree::config::workspace::{RepoEntry, WorkspaceConfig, WorkspaceStatus};
use zootree::config::ConfigManager;
use zootree::daemon::client::{request, request_with_body, HttpResponse};
use zootree::daemon::operations::{Retention, RunnerFactory};
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
    /// Blocks every command until released.
    gate: Option<Arc<Gate>>,
    /// Stdout of every command.
    stdout: String,
    /// Stdout of the first command only (for example dirty `git status`).
    first_stdout: Arc<Mutex<Option<String>>>,
    /// When set, every command fails with this stderr.
    fail_stderr: Option<String>,
    /// Environments the commands ran with.
    envs: Arc<Mutex<Vec<BTreeMap<String, String>>>>,
}

struct ScriptRunner(Script);

impl CommandRunner for ScriptRunner {
    fn run(&self, spec: &CommandSpec) -> anyhow::Result<Output> {
        if let Some(gate) = &self.0.gate {
            gate.wait();
        }
        self.0.envs.lock().unwrap().push(
            spec.env
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        );
        if let Some(stderr) = &self.0.fail_stderr {
            return Ok(Output {
                status: ExitStatus::from_raw(1 << 8),
                stdout: Vec::new(),
                stderr: stderr.clone().into_bytes(),
            });
        }
        let stdout = self
            .0
            .first_stdout
            .lock()
            .unwrap()
            .take()
            .unwrap_or_else(|| self.0.stdout.clone());
        Ok(Output {
            status: ExitStatus::from_raw(0),
            stdout: stdout.into_bytes(),
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

fn workspace(env: &Env, name: &str, repos: &[&str]) -> WorkspaceConfig {
    WorkspaceConfig {
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
        repos: repos
            .iter()
            .map(|r| RepoEntry {
                name: (*r).into(),
                target_branch: Some("main".into()),
            })
            .collect(),
        events: Vec::new(),
    }
}

fn add_workspace(env: &Env, name: &str) {
    env.mgr
        .save_workspace(&WorkspaceStatus::InProgress, &workspace(env, name, &[]))
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

fn get(daemon: &DaemonHandle, path: &str) -> HttpResponse {
    request(&daemon.socket, "GET", path, Some(&daemon.token), T).unwrap()
}

fn post(daemon: &DaemonHandle, path: &str, body: Value) -> HttpResponse {
    let body = body.to_string();
    request_with_body(
        &daemon.socket,
        "POST",
        path,
        Some(&daemon.token),
        &[],
        Some(&body),
        T,
    )
    .unwrap()
}

/// Starts an Operation and returns its id; asserts the 202 answer.
fn submit(daemon: &DaemonHandle, path: &str, body: Value) -> String {
    let response = post(daemon, path, body);
    assert_eq!(response.status, 202, "{}", response.body);
    let body = response.json().unwrap();
    assert_eq!(body["status"], "running");
    body["id"].as_str().unwrap().to_string()
}

fn wait_finished(daemon: &DaemonHandle, id: &str) -> Value {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let body = get(daemon, &format!("/operations/{id}")).json().unwrap();
        if body["status"] != "running" {
            return body;
        }
        assert!(
            Instant::now() < deadline,
            "operation never finished: {body}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn stop(daemon: DaemonHandle) {
    daemon.shutdown();
    daemon.join().unwrap();
}

fn lines(op: &Value) -> Vec<String> {
    op["output"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l.as_str().unwrap().to_string())
        .collect()
}

// ---- done / cancel --------------------------------------------------------

#[test]
fn done_returns_an_operation_at_once_and_reports_output_and_result() {
    let env = env();
    set_pre_done_hook(&env);
    add_workspace(&env, "alpha");
    let gate = Arc::new(Gate::default());
    let daemon = start(
        &env,
        Script {
            gate: Some(gate.clone()),
            stdout: "compiling\nlinking\n".into(),
            ..Default::default()
        },
        |_| {},
    );

    let id = submit(
        &daemon,
        "/workspaces/alpha/done",
        json!({"no_merge": true, "no_clean": true}),
    );
    // The work is blocked in the runner, yet the request already answered.
    let running = get(&daemon, &format!("/operations/{id}")).json().unwrap();
    assert_eq!(running["status"], "running");
    assert_eq!(running["kind"], "done");
    assert_eq!(running["workspace"], "alpha");
    assert!(running["result"].is_null());
    assert!(running["finished_at"].is_null());
    assert!(chrono::DateTime::parse_from_rfc3339(running["started_at"].as_str().unwrap()).is_ok());

    gate.release();
    let finished = wait_finished(&daemon, &id);
    assert_eq!(finished["status"], "succeeded");
    assert_eq!(
        finished["result"],
        json!({"workspace": "alpha", "dry_run": false})
    );
    assert!(finished["error"].is_null());
    assert!(
        chrono::DateTime::parse_from_rfc3339(finished["finished_at"].as_str().unwrap()).is_ok()
    );
    let output = lines(&finished);
    let pos = |needle: &str| {
        output
            .iter()
            .position(|l| l == needle)
            .unwrap_or_else(|| panic!("{needle:?} missing from {output:?}"))
    };
    assert!(pos("==> pre_done") < pos("[pre_done] compiling"));
    assert!(pos("[pre_done] compiling") < pos("[pre_done] linking"));
    assert!(pos("[pre_done] linking") < pos("<== pre_done ok"));
    assert!(output.iter().any(|l| l.contains("completed")));
    assert_eq!(
        env.mgr.load_workspace("alpha").unwrap().0,
        WorkspaceStatus::Done
    );
    stop(daemon);
}

#[test]
fn done_dry_run_option_is_honoured() {
    let env = env();
    add_workspace(&env, "alpha");
    let daemon = start(&env, Script::default(), |_| {});

    let id = submit(&daemon, "/workspaces/alpha/done", json!({"dry_run": true}));
    let finished = wait_finished(&daemon, &id);
    assert_eq!(
        finished["result"],
        json!({"workspace": "alpha", "dry_run": true})
    );
    assert_eq!(
        env.mgr.load_workspace("alpha").unwrap().0,
        WorkspaceStatus::InProgress
    );
    stop(daemon);
}

#[test]
fn failure_carries_the_complete_error_output() {
    let env = env();
    set_pre_done_hook(&env);
    add_workspace(&env, "alpha");
    let daemon = start(
        &env,
        Script {
            fail_stderr: Some("line a\nline b\n".into()),
            ..Default::default()
        },
        |_| {},
    );

    let id = submit(
        &daemon,
        "/workspaces/alpha/done",
        json!({"no_merge": true, "no_clean": true}),
    );
    let finished = wait_finished(&daemon, &id);
    assert_eq!(finished["status"], "failed");
    assert!(finished["result"].is_null());
    assert_eq!(finished["error"]["code"], "operation_failed");
    let message = finished["error"]["message"].as_str().unwrap();
    assert!(
        message.contains("line a") && message.contains("line b"),
        "{message}"
    );
    assert_eq!(
        env.mgr.load_workspace("alpha").unwrap().0,
        WorkspaceStatus::InProgress
    );
    // The Workspace is free again.
    let again = post(&daemon, "/workspaces/alpha/done", json!({"dry_run": true}));
    assert_eq!(again.status, 202);
    stop(daemon);
}

#[test]
fn cancel_asks_for_confirmation_through_a_request_field_never_a_prompt() {
    let env = env();
    let ws = workspace(&env, "dirty", &["api"]);
    std::fs::create_dir_all(std::path::Path::new(&ws.workspace_dir).join("api")).unwrap();
    env.mgr
        .save_repo_config(
            "api",
            &RepoConfig {
                path: env
                    .tmp
                    .path()
                    .join("api-src")
                    .to_string_lossy()
                    .into_owned(),
                default_target_branch: None,
                copy_files: Vec::new(),
                hooks: Default::default(),
                lazygit: None,
            },
        )
        .unwrap();
    env.mgr
        .save_workspace(&WorkspaceStatus::InProgress, &ws)
        .unwrap();
    let script = Script {
        first_stdout: Arc::new(Mutex::new(Some(" M file.rs\n".into()))),
        ..Default::default()
    };
    let daemon = start(&env, script, |_| {});

    let id = submit(&daemon, "/workspaces/dirty/cancel", json!({}));
    let needs = wait_finished(&daemon, &id);
    assert_eq!(needs["status"], "failed");
    assert_eq!(needs["error"]["code"], "confirmation_required");
    assert_eq!(needs["error"]["repos"], json!(["api"]));
    assert_eq!(
        env.mgr.load_workspace("dirty").unwrap().0,
        WorkspaceStatus::InProgress
    );

    let id = submit(
        &daemon,
        "/workspaces/dirty/cancel",
        json!({"confirmed_uncommitted": true}),
    );
    let finished = wait_finished(&daemon, &id);
    assert_eq!(finished["status"], "succeeded", "{finished}");
    assert_eq!(finished["result"]["workspace"], "dirty");
    assert_eq!(
        env.mgr.load_workspace("dirty").unwrap().0,
        WorkspaceStatus::Canceled
    );
    stop(daemon);
}

#[test]
fn request_environment_snapshot_reaches_child_commands() {
    let env = env();
    set_pre_done_hook(&env);
    add_workspace(&env, "alpha");
    let script = Script::default();
    let seen = script.envs.clone();
    let daemon = start(&env, script, |_| {});

    let id = submit(
        &daemon,
        "/workspaces/alpha/done",
        json!({
            "no_merge": true, "no_clean": true,
            "env": {"PATH": "/caller/bin", "SSH_AUTH_SOCK": "/tmp/caller.sock", "UNLISTED": "dropped"}
        }),
    );
    assert_eq!(wait_finished(&daemon, &id)["status"], "succeeded");
    let seen = seen.lock().unwrap();
    assert!(!seen.is_empty());
    for env in seen.iter() {
        assert_eq!(env.get("PATH").map(String::as_str), Some("/caller/bin"));
        assert_eq!(
            env.get("SSH_AUTH_SOCK").map(String::as_str),
            Some("/tmp/caller.sock")
        );
        assert!(!env.contains_key("UNLISTED"));
    }
    drop(seen);
    stop(daemon);
}

// ---- busy rule ------------------------------------------------------------

#[test]
fn second_request_on_a_busy_workspace_is_refused_while_others_run_in_parallel() {
    let env = env();
    set_pre_done_hook(&env);
    add_workspace(&env, "alpha");
    add_workspace(&env, "beta");
    let gate = Arc::new(Gate::default());
    let daemon = start(
        &env,
        Script {
            gate: Some(gate.clone()),
            ..Default::default()
        },
        |_| {},
    );
    let quick = json!({"no_merge": true, "no_clean": true});

    let first = submit(&daemon, "/workspaces/alpha/done", quick.clone());
    for (path, body) in [
        ("/workspaces/alpha/done", quick.clone()),
        ("/workspaces/alpha/cancel", json!({})),
    ] {
        let refused = post(&daemon, path, body);
        assert_eq!(refused.status, 409, "{}", refused.body);
        let error = &refused.json().unwrap()["error"];
        assert_eq!(error["code"], "busy");
        assert_eq!(error["operation_id"], first.as_str());
    }
    // A different Workspace is unaffected.
    let other = submit(&daemon, "/workspaces/beta/done", quick.clone());
    assert_ne!(other, first);

    gate.release();
    assert_eq!(wait_finished(&daemon, &first)["status"], "succeeded");
    assert_eq!(wait_finished(&daemon, &other)["status"], "succeeded");
    // Finished Operations no longer block: alpha is done now, so a new request
    // is accepted and fails on its own merits rather than being busy.
    let after = post(&daemon, "/workspaces/alpha/done", json!({}));
    assert_eq!(after.status, 202);
    assert_eq!(
        wait_finished(&daemon, after.json().unwrap()["id"].as_str().unwrap())["status"],
        "failed"
    );
    stop(daemon);
}

#[test]
fn lock_held_by_a_command_line_process_becomes_a_busy_failure() {
    let env = env();
    add_workspace(&env, "alpha");
    let daemon = start(&env, Script::default(), |_| {});

    // Stands in for `zootree done alpha` running concurrently outside the Daemon.
    let cli_lock = env.mgr.lock_workspace("alpha").unwrap();
    let id = submit(&daemon, "/workspaces/alpha/done", json!({"dry_run": true}));
    let failed = wait_finished(&daemon, &id);
    assert_eq!(failed["status"], "failed");
    assert_eq!(failed["error"]["code"], "busy");
    assert!(failed["error"]["message"]
        .as_str()
        .unwrap()
        .contains("another zootree process"));

    // And the other way round: while the Daemon works, the lock is taken.
    drop(cli_lock);
    let id = submit(&daemon, "/workspaces/alpha/done", json!({"dry_run": true}));
    assert_eq!(wait_finished(&daemon, &id)["status"], "succeeded");
    stop(daemon);
}

#[test]
fn running_operation_holds_the_workspace_lock_against_the_command_line() {
    let env = env();
    set_pre_done_hook(&env);
    add_workspace(&env, "alpha");
    let gate = Arc::new(Gate::default());
    let daemon = start(
        &env,
        Script {
            gate: Some(gate.clone()),
            ..Default::default()
        },
        |_| {},
    );
    let id = submit(
        &daemon,
        "/workspaces/alpha/done",
        json!({"no_merge": true, "no_clean": true}),
    );
    // The Started event is emitted after the worker took the lock. Probing the
    // lock any earlier could itself make the worker lose the race.
    let deadline = Instant::now() + T;
    while lines(&get(&daemon, &format!("/operations/{id}")).json().unwrap()).is_empty() {
        assert!(Instant::now() < deadline, "operation never started");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(env.mgr.lock_workspace("alpha").is_err());
    gate.release();
    wait_finished(&daemon, &id);
    assert!(env.mgr.lock_workspace("alpha").is_ok());
    stop(daemon);
}

// ---- validation -----------------------------------------------------------

#[test]
fn invalid_requests_get_stable_error_codes() {
    let env = env();
    add_workspace(&env, "alpha");
    let daemon = start(&env, Script::default(), |_| {});
    let code = |r: HttpResponse| (r.status, r.json().unwrap()["error"]["code"].clone());

    assert_eq!(
        code(post(&daemon, "/workspaces/missing/done", json!({}))),
        (404, json!("not_found"))
    );
    assert_eq!(
        code(post(&daemon, "/workspaces/Bad%20Name/done", json!({}))),
        (400, json!("bad_request"))
    );
    assert_eq!(
        code(post(&daemon, "/workspaces/alpha/done", json!({"nope": 1}))),
        (400, json!("bad_request"))
    );
    assert_eq!(
        code(post(
            &daemon,
            "/workspaces/alpha/cancel",
            json!({"force": "yes"})
        )),
        (400, json!("bad_request"))
    );
    assert_eq!(
        code(get(&daemon, "/operations/op-unknown")),
        (404, json!("not_found"))
    );
    let unauthenticated =
        request(&daemon.socket, "POST", "/workspaces/alpha/done", None, T).unwrap();
    assert_eq!(unauthenticated.status, 401);
    // Nothing was started by any of the above.
    assert_eq!(
        env.mgr.load_workspace("alpha").unwrap().0,
        WorkspaceStatus::InProgress
    );
    assert!(daemon::CAPABILITIES.contains(&"operations"));
    stop(daemon);
}

// ---- retention and idle ---------------------------------------------------

#[test]
fn only_the_most_recent_records_are_kept() {
    let env = env();
    add_workspace(&env, "alpha");
    let daemon = start(&env, Script::default(), |o| {
        o.operation_retention = Retention {
            max_records: 2,
            max_age: Duration::from_secs(3600),
        };
    });
    let mut ids = Vec::new();
    for _ in 0..3 {
        let id = submit(&daemon, "/workspaces/alpha/done", json!({"dry_run": true}));
        wait_finished(&daemon, &id);
        ids.push(id);
    }
    assert_eq!(get(&daemon, &format!("/operations/{}", ids[0])).status, 404);
    assert_eq!(get(&daemon, &format!("/operations/{}", ids[1])).status, 200);
    assert_eq!(get(&daemon, &format!("/operations/{}", ids[2])).status, 200);
    stop(daemon);
}

#[test]
fn records_expire_after_the_age_limit_but_running_ones_never_do() {
    let env = env();
    set_pre_done_hook(&env);
    add_workspace(&env, "alpha");
    add_workspace(&env, "beta");
    let gate = Arc::new(Gate::default());
    let daemon = start(
        &env,
        Script {
            gate: Some(gate.clone()),
            ..Default::default()
        },
        |o| {
            o.operation_retention = Retention {
                max_records: 100,
                max_age: Duration::from_millis(150),
            };
        },
    );
    let quick = json!({"no_merge": true, "no_clean": true});
    let running = submit(&daemon, "/workspaces/beta/done", quick);
    let finished = submit(&daemon, "/workspaces/alpha/done", json!({"dry_run": true}));
    wait_finished(&daemon, &finished);

    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(get(&daemon, &format!("/operations/{finished}")).status, 404);
    assert_eq!(get(&daemon, &format!("/operations/{running}")).status, 200);
    gate.release();
    wait_finished(&daemon, &running);
    stop(daemon);
}

#[test]
fn daemon_does_not_idle_exit_while_an_operation_runs() {
    let env = env();
    set_pre_done_hook(&env);
    add_workspace(&env, "alpha");
    let gate = Arc::new(Gate::default());
    let daemon = start(
        &env,
        Script {
            gate: Some(gate.clone()),
            ..Default::default()
        },
        |o| o.idle_timeout = Duration::from_millis(300),
    );
    let id = submit(
        &daemon,
        "/workspaces/alpha/done",
        json!({"no_merge": true, "no_clean": true}),
    );
    std::thread::sleep(Duration::from_millis(900));
    assert_eq!(get(&daemon, "/status").status, 200, "exited while busy");

    gate.release();
    wait_finished(&daemon, &id);
    // With the Operation over, the idle rule applies again.
    daemon.join().unwrap();
}
