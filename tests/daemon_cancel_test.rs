//! Explicit cancel of Operations through the Daemon's HTTP seam, plus the
//! guarantees around it: a client going away does not stop an Operation, a
//! running Operation keeps the Daemon from idle-exiting, cancelling a finished
//! Operation is a clear no-op, and a canceled start rolls back. The Daemon runs
//! in-process on a unix socket in a temporary directory; commands are a
//! cancellable double (or, once, real child processes).

use serde_json::{json, Value};
use std::os::unix::process::ExitStatusExt;
use std::process::{ExitStatus, Output};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tempfile::TempDir;
use zootree::config::global::{GlobalConfig, HookValue, MultiplexerConfig};
use zootree::config::repo::RepoConfig;
use zootree::config::workspace::{RepoEntry, WorkspaceConfig, WorkspaceStatus};
use zootree::config::ConfigManager;
use zootree::daemon::client::{request, request_with_body, HttpResponse};
use zootree::daemon::operations::{DynRunner, RunnerFactory};
use zootree::daemon::{self, DaemonHandle, DaemonOptions};
use zootree::runner::{CancelToken, CommandRunner, CommandSpec, OutputLine};
use zootree::service::context::CollectingSink;
use zootree::service::start::{start_workspace, StartRequest};
use zootree::service::{EnvSnapshot, RequestContext};

const T: Duration = Duration::from_secs(5);

// ---- runner double --------------------------------------------------------

/// Succeeds at once, except pre_done / post_create hooks, which run "forever"
/// until a cancel request interrupts them (like a long child process).
#[derive(Clone, Default)]
struct HangingHooks {
    calls: Arc<Mutex<Vec<Vec<String>>>>,
    hook_running: Arc<Mutex<bool>>,
    /// Let the hook finish on its own after this many polls (None: never).
    finish_hook_after: Option<u32>,
}

impl HangingHooks {
    fn ok(stdout: &str) -> Output {
        Output {
            status: ExitStatus::from_raw(0),
            stdout: stdout.as_bytes().to_vec(),
            stderr: Vec::new(),
        }
    }
}

impl CommandRunner for HangingHooks {
    fn run(&self, spec: &CommandSpec) -> anyhow::Result<Output> {
        let mut call = vec![spec.program.clone()];
        call.extend(spec.args.iter().cloned());
        self.calls.lock().unwrap().push(call);
        if spec.args.iter().any(|a| a == "for-each-ref") {
            return Ok(Self::ok("refs/heads/main\n"));
        }
        Ok(Self::ok(""))
    }

    fn run_cancellable(
        &self,
        spec: &CommandSpec,
        _on_line: &mut dyn FnMut(OutputLine),
        cancel: &CancelToken,
    ) -> anyhow::Result<Output> {
        if let Some(err) = cancel.interrupt(spec) {
            return Err(err);
        }
        if !spec.env.contains_key("ZOOTREE_HOOK") {
            return self.run(spec);
        }
        *self.hook_running.lock().unwrap() = true;
        let mut polls = 0u32;
        loop {
            if let Some(err) = cancel.interrupt(spec) {
                return Err(err);
            }
            polls += 1;
            if self.finish_hook_after.is_some_and(|n| polls > n) {
                return self.run(spec);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
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

fn start(
    env: &Env,
    runner: Option<HangingHooks>,
    tweak: impl FnOnce(&mut DaemonOptions),
) -> DaemonHandle {
    let mut options = DaemonOptions::new(env.mgr.base_dir.clone());
    options.idle_timeout = Duration::from_secs(60);
    if let Some(runner) = runner {
        options.runner = RunnerFactory::new(move || Box::new(runner.clone()));
    }
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

fn cancel_op(daemon: &DaemonHandle, id: &str) -> HttpResponse {
    post(daemon, &format!("/operations/{id}/cancel"), json!({}))
}

fn wait_until(what: &str, mut cond: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !cond() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn cancel_interrupts_the_running_command_and_reports_where_it_stopped() {
    let env = env();
    set_pre_done_hook(&env);
    add_workspace(&env, "alpha");
    let runner = HangingHooks::default();
    let daemon = start(&env, Some(runner.clone()), |_| {});
    let id = submit(
        &daemon,
        "/workspaces/alpha/done",
        json!({"no_merge": true, "no_clean": true}),
    );
    wait_until("hook to start", || *runner.hook_running.lock().unwrap());

    let response = cancel_op(&daemon, &id);
    assert_eq!(response.status, 202, "{}", response.body);
    let body = response.json().unwrap();
    assert_eq!(body["outcome"], "cancel_requested");
    assert_eq!(body["id"], json!(id));

    let op = wait_finished(&daemon, &id);
    assert_eq!(op["status"], "canceled", "{op}");
    assert_eq!(op["error"], Value::Null);
    assert_eq!(op["result"], Value::Null);
    assert_eq!(op["cancel_requested"], true);
    let canceled = &op["canceled"];
    assert!(
        canceled["stopped_at"]
            .as_str()
            .is_some_and(|s| !s.is_empty()),
        "{canceled}"
    );
    assert!(canceled["interrupted_command"].is_string(), "{canceled}");
    assert_eq!(canceled["cleanup"], "none");
    assert!(canceled["notes"].as_array().unwrap().len() >= 2);
    assert!(op["finished_at"].is_string());

    // The Workspace is free again and the lifecycle can be retried.
    let again = post(&daemon, "/workspaces/alpha/done", json!({"dry_run": true}));
    assert_eq!(again.status, 202, "{}", again.body);
    stop(daemon);
}

#[test]
fn cancelling_a_finished_operation_is_a_clear_no_op() {
    let env = env();
    add_workspace(&env, "alpha");
    let daemon = start(&env, Some(HangingHooks::default()), |_| {});
    let id = submit(
        &daemon,
        "/workspaces/alpha/done",
        json!({"no_merge": true, "no_clean": true}),
    );
    let finished = wait_finished(&daemon, &id);
    assert_eq!(finished["status"], "succeeded", "{finished}");

    for _ in 0..2 {
        let response = cancel_op(&daemon, &id);
        assert_eq!(response.status, 200, "{}", response.body);
        let body = response.json().unwrap();
        assert_eq!(body["outcome"], "already_finished");
        assert_eq!(body["status"], "succeeded");
        assert_eq!(body["operation"]["status"], "succeeded");
    }
    // The record itself is untouched.
    let after = get(&daemon, &format!("/operations/{id}")).json().unwrap();
    assert_eq!(after["status"], "succeeded");
    assert_eq!(after["cancel_requested"], false);

    let unknown = cancel_op(&daemon, "op-0000000000000000");
    assert_eq!(unknown.status, 404, "{}", unknown.body);
    assert_eq!(unknown.json().unwrap()["error"]["code"], "not_found");
    stop(daemon);
}

#[test]
fn cancelling_twice_while_running_is_accepted_both_times() {
    let env = env();
    set_pre_done_hook(&env);
    add_workspace(&env, "alpha");
    let runner = HangingHooks::default();
    let daemon = start(&env, Some(runner.clone()), |_| {});
    let id = submit(&daemon, "/workspaces/alpha/done", json!({"no_merge": true}));
    wait_until("hook to start", || *runner.hook_running.lock().unwrap());
    assert_eq!(cancel_op(&daemon, &id).status, 202);
    let second = cancel_op(&daemon, &id);
    assert!(
        second.status == 202 || second.status == 200,
        "{}",
        second.body
    );
    assert_eq!(wait_finished(&daemon, &id)["status"], "canceled");
    stop(daemon);
}

#[test]
fn a_cancel_that_arrives_too_late_leaves_the_real_outcome() {
    let env = env();
    set_pre_done_hook(&env);
    add_workspace(&env, "alpha");
    // The hook ends by itself before any interrupt can reach it.
    let runner = HangingHooks {
        finish_hook_after: Some(5),
        ..Default::default()
    };
    let daemon = start(&env, Some(runner.clone()), |_| {});
    let id = submit(
        &daemon,
        "/workspaces/alpha/done",
        json!({"no_merge": true, "no_clean": true}),
    );
    let op = wait_finished(&daemon, &id);
    assert_eq!(op["status"], "succeeded");
    assert_eq!(
        cancel_op(&daemon, &id).json().unwrap()["outcome"],
        "already_finished"
    );
    stop(daemon);
}

#[test]
fn client_disconnect_does_not_stop_an_operation() {
    let env = env();
    set_pre_done_hook(&env);
    add_workspace(&env, "alpha");
    let runner = HangingHooks {
        finish_hook_after: Some(40),
        ..Default::default()
    };
    let daemon = start(&env, Some(runner.clone()), |_| {});
    // The request helper closes its connection as soon as it has the answer.
    let id = submit(
        &daemon,
        "/workspaces/alpha/done",
        json!({"no_merge": true, "no_clean": true}),
    );
    // A connection that is opened and dropped without a request changes nothing either.
    drop(std::os::unix::net::UnixStream::connect(&daemon.socket).unwrap());
    let op = wait_finished(&daemon, &id);
    assert_eq!(op["status"], "succeeded", "{op}");
    assert_eq!(op["cancel_requested"], false);
    stop(daemon);
}

#[test]
fn running_operation_blocks_idle_exit_and_cancel_releases_it() {
    let env = env();
    set_pre_done_hook(&env);
    add_workspace(&env, "alpha");
    let runner = HangingHooks::default();
    let daemon = start(&env, Some(runner.clone()), |o| {
        o.idle_timeout = Duration::from_millis(300)
    });
    let id = submit(
        &daemon,
        "/workspaces/alpha/done",
        json!({"no_merge": true, "no_clean": true}),
    );
    std::thread::sleep(Duration::from_millis(900));
    assert_eq!(get(&daemon, "/status").status, 200, "exited while busy");

    cancel_op(&daemon, &id);
    assert_eq!(wait_finished(&daemon, &id)["status"], "canceled");
    // Nothing counts as running any more, so the idle rule fires.
    let started = Instant::now();
    daemon.join().unwrap();
    assert!(started.elapsed() < Duration::from_secs(4));
}

#[test]
fn cancel_kills_a_real_child_process_tree() {
    let env = env();
    set_pre_done_hook(&env);
    let mut global = GlobalConfig::default();
    global.hooks.pre_done = Some(HookValue::Simple("sleep 30; echo unreachable".into()));
    env.mgr.save_global_config(&global).unwrap();
    let ws = workspace(&env, "alpha", &[]);
    std::fs::create_dir_all(&ws.workspace_dir).unwrap();
    env.mgr
        .save_workspace(&WorkspaceStatus::InProgress, &ws)
        .unwrap();
    // Real runner (the default factory).
    let daemon = start(&env, None, |_| {});
    let id = submit(
        &daemon,
        "/workspaces/alpha/done",
        json!({"no_merge": true, "no_clean": true}),
    );
    // Give the hook a moment to start its sleep.
    std::thread::sleep(Duration::from_millis(500));
    let began = Instant::now();
    cancel_op(&daemon, &id);
    let op = wait_finished(&daemon, &id);
    assert_eq!(op["status"], "canceled", "{op}");
    assert!(
        began.elapsed() < Duration::from_secs(4),
        "the child tree was not killed"
    );
    assert!(op["canceled"]["interrupted_command"].is_string());
    stop(daemon);
}

#[test]
fn canceled_start_removes_the_worktrees_it_created() {
    let env = env();
    let repo_cfg = RepoConfig {
        path: "/repo/api".into(),
        default_target_branch: None,
        copy_files: Vec::new(),
        hooks: zootree::config::global::HooksConfig {
            post_create: Some(HookValue::Simple("sleep 30".into())),
            ..Default::default()
        },
        lazygit: None,
    };
    env.mgr.save_repo_config("api", &repo_cfg).unwrap();
    let ws = workspace(&env, "fair-fox", &["api"]);
    env.mgr
        .save_workspace(&WorkspaceStatus::Pending, &ws)
        .unwrap();

    let runner = HangingHooks::default();
    let token = CancelToken::new();
    let (worker_runner, worker_token) = (runner.clone(), token.clone());
    let mgr = ConfigManager::with_base_dir(env.mgr.base_dir.clone());
    let worker = std::thread::spawn(move || {
        let dyn_runner = DynRunner::new(Box::new(worker_runner)).with_cancel(worker_token);
        let sink = CollectingSink::new();
        let ctx = RequestContext::new(&sink, EnvSnapshot::from_pairs([]));
        start_workspace(
            &mgr,
            &dyn_runner,
            &ctx,
            &StartRequest {
                name: "fair-fox".into(),
                multiplexer: None,
                no_multiplexer: true,
                run_agent: None,
            },
        )
    });
    wait_until("post_create hook to start", || {
        *runner.hook_running.lock().unwrap()
    });
    token.request();
    let err = worker.join().unwrap().unwrap_err();
    assert!(format!("{err:#}").contains("cancel"), "{err:#}");
    assert!(token.interrupted_command().is_some());

    let calls = runner.calls.lock().unwrap().clone();
    let removed = calls
        .iter()
        .any(|c| c.windows(3).any(|w| w == ["worktree", "remove", "--force"]));
    assert!(removed, "no rollback worktree remove in {calls:?}");
    assert!(
        !std::path::Path::new(&ws.workspace_dir).exists(),
        "workspace dir created by start should be gone"
    );
    assert_eq!(
        env.mgr.load_workspace("fair-fox").unwrap().0,
        WorkspaceStatus::Pending
    );
}
