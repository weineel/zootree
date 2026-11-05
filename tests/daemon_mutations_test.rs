//! The remaining state-changing endpoints through the Daemon's HTTP seam:
//! start, reopen, add-repo, create, archived-workspace deletion and repo
//! registration and removal. The Daemon runs in-process on a unix socket in a
//! temporary directory with a recording runner double; nothing here touches
//! the real ~/.config/zootree or runs a real external command.

use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::os::unix::process::ExitStatusExt;
use std::process::{ExitStatus, Output};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};
use tempfile::TempDir;
use zootree::config::global::MultiplexerConfig;
use zootree::config::repo::RepoConfig;
use zootree::config::workspace::{RepoEntry, WorkspaceConfig, WorkspaceStatus};
use zootree::config::ConfigManager;
use zootree::daemon::client::{request, request_with_body, HttpResponse};
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
    /// Fails every command whose arguments contain all of these words.
    fail_when: Option<Vec<String>>,
    /// Every command that ran: program, args and the environment it got.
    calls: Arc<Mutex<Vec<Call>>>,
    /// Workspace task branches (`zootree/...`) do not exist yet.
    no_task_branches: bool,
    /// How many interactive (foreground) commands were run.
    interactive: Arc<Mutex<usize>>,
}

#[derive(Clone, Debug)]
struct Call {
    env: BTreeMap<String, String>,
    env_remove: Vec<String>,
}

struct ScriptRunner(Script);

impl CommandRunner for ScriptRunner {
    fn run(&self, spec: &CommandSpec) -> anyhow::Result<Output> {
        if let Some(gate) = &self.0.gate {
            gate.wait();
        }
        self.0.calls.lock().unwrap().push(Call {
            env: spec
                .env
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            env_remove: spec.env_remove.clone(),
        });
        let failing = self
            .0
            .fail_when
            .as_ref()
            .is_some_and(|words| words.iter().all(|w| spec.args.contains(w)));
        // Every local branch exists: a branch lookup answers with its ref.
        let stdout = if spec.args.iter().any(|a| a == "for-each-ref")
            && !(self.0.no_task_branches && spec.args.iter().any(|a| a.contains("zootree/")))
        {
            spec.args.last().cloned().unwrap_or_default() + "\n"
        } else {
            String::new()
        };
        Ok(Output {
            status: ExitStatus::from_raw(if failing { 1 << 8 } else { 0 }),
            stdout: stdout.into_bytes(),
            stderr: if failing {
                b"boom".to_vec()
            } else {
                Vec::new()
            },
        })
    }

    fn run_interactive(&self, _spec: &CommandSpec) -> anyhow::Result<ExitStatus> {
        *self.0.interactive.lock().unwrap() += 1;
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

fn register_api(env: &Env) {
    let src = env.tmp.path().join("api-src");
    std::fs::create_dir_all(&src).unwrap();
    env.mgr
        .save_repo_config(
            "api",
            &RepoConfig {
                path: src.to_string_lossy().into_owned(),
                default_target_branch: Some("main".into()),
                copy_files: Vec::new(),
                hooks: Default::default(),
                lazygit: None,
            },
        )
        .unwrap();
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

fn save(env: &Env, status: WorkspaceStatus, name: &str, repos: &[&str]) {
    env.mgr
        .save_workspace(&status, &workspace(env, name, repos))
        .unwrap();
}

fn start(env: &Env, script: Script) -> DaemonHandle {
    let mut options = DaemonOptions::new(env.mgr.base_dir.clone());
    options.idle_timeout = Duration::from_secs(60);
    options.runner = RunnerFactory::new(move || Box::new(ScriptRunner(script.clone())));
    daemon::start(options).unwrap()
}

fn get(daemon: &DaemonHandle, path: &str) -> HttpResponse {
    request(&daemon.socket, "GET", path, Some(&daemon.token), T).unwrap()
}

fn send(daemon: &DaemonHandle, method: &str, path: &str, body: Value) -> HttpResponse {
    let body = body.to_string();
    request_with_body(
        &daemon.socket,
        method,
        path,
        Some(&daemon.token),
        &[],
        Some(&body),
        T,
    )
    .unwrap()
}

fn post(daemon: &DaemonHandle, path: &str, body: Value) -> HttpResponse {
    send(daemon, "POST", path, body)
}

fn submit(daemon: &DaemonHandle, method: &str, path: &str, body: Value) -> String {
    let response = send(daemon, method, path, body);
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

fn run(daemon: &DaemonHandle, method: &str, path: &str, body: Value) -> Value {
    let id = submit(daemon, method, path, body);
    wait_finished(daemon, &id)
}

fn stop(daemon: DaemonHandle) {
    daemon.shutdown();
    daemon.join().unwrap();
}

fn error_code(r: &HttpResponse) -> (u16, Value) {
    (r.status, r.json().unwrap()["error"]["code"].clone())
}

// ---- start ----------------------------------------------------------------

#[test]
fn start_is_an_operation_and_applies_the_request_environment() {
    let env = env();
    register_api(&env);
    save(&env, WorkspaceStatus::Pending, "alpha", &["api"]);
    let script = Script::default();
    let daemon = start(&env, script.clone());

    let finished = run(
        &daemon,
        "POST",
        "/workspaces/alpha/start",
        json!({
            "no_multiplexer": true,
            "env": {"PATH": "/snapshot/bin", "SSH_AUTH_SOCK": "/snapshot/agent.sock"},
        }),
    );
    assert_eq!(finished["kind"], "start", "{finished}");
    assert_eq!(finished["workspace"], "alpha");
    assert_eq!(finished["status"], "succeeded", "{finished}");
    assert_eq!(finished["result"]["workspace"], "alpha");
    assert!(finished["result"]["attach"].is_null());
    assert_eq!(
        env.mgr.load_workspace("alpha").unwrap().0,
        WorkspaceStatus::InProgress
    );

    let calls = script.calls.lock().unwrap();
    assert!(!calls.is_empty());
    for call in calls.iter() {
        assert_eq!(call.env["PATH"], "/snapshot/bin", "{call:?}");
        assert_eq!(call.env["SSH_AUTH_SOCK"], "/snapshot/agent.sock");
        // Whitelisted variables the client did not send are removed rather
        // than inherited from the Daemon.
        assert!(call.env_remove.contains(&"ZELLIJ".to_string()), "{call:?}");
    }
    drop(calls);
    stop(daemon);
}

#[test]
fn start_returns_the_attach_request_and_never_runs_it() {
    let env = env();
    register_api(&env);
    save(&env, WorkspaceStatus::Pending, "alpha", &["api"]);
    let script = Script::default();
    let daemon = start(&env, script.clone());

    let finished = run(
        &daemon,
        "POST",
        "/workspaces/alpha/start",
        json!({"run_agent": "my-agent --flag"}),
    );
    assert_eq!(finished["status"], "succeeded", "{finished}");
    let attach = &finished["result"]["attach"];
    assert!(attach["program"].is_string(), "{finished}");
    assert!(attach["args"].is_array());
    assert_eq!(finished["result"]["agent_cli"], "my-agent --flag");
    assert_eq!(*script.interactive.lock().unwrap(), 0);
    stop(daemon);
}

#[test]
fn start_failure_rolls_back_like_the_command_line() {
    let env = env();
    register_api(&env);
    save(&env, WorkspaceStatus::Pending, "alpha", &["api"]);
    let daemon = start(
        &env,
        Script {
            fail_when: Some(vec!["worktree".into(), "add".into()]),
            ..Default::default()
        },
    );

    let finished = run(
        &daemon,
        "POST",
        "/workspaces/alpha/start",
        json!({"no_multiplexer": true}),
    );
    assert_eq!(finished["status"], "failed", "{finished}");
    assert_eq!(finished["error"]["code"], "operation_failed");
    assert_eq!(
        env.mgr.load_workspace("alpha").unwrap().0,
        WorkspaceStatus::Pending
    );
    assert!(!std::path::Path::new(&workspace(&env, "alpha", &[]).workspace_dir).exists());
    stop(daemon);
}

#[test]
fn start_follows_the_busy_rule_and_validates_the_request() {
    let env = env();
    register_api(&env);
    save(&env, WorkspaceStatus::Pending, "alpha", &["api"]);
    let gate = Arc::new(Gate::default());
    let daemon = start(
        &env,
        Script {
            gate: Some(gate.clone()),
            ..Default::default()
        },
    );

    assert_eq!(
        error_code(&post(&daemon, "/workspaces/missing/start", json!({}))),
        (404, json!("not_found"))
    );
    assert_eq!(
        error_code(&post(
            &daemon,
            "/workspaces/alpha/start",
            json!({"nope": 1})
        )),
        (400, json!("bad_request"))
    );
    let id = submit(
        &daemon,
        "POST",
        "/workspaces/alpha/start",
        json!({"no_multiplexer": true}),
    );
    let second = post(&daemon, "/workspaces/alpha/start", json!({}));
    assert_eq!(error_code(&second), (409, json!("busy")));
    assert_eq!(second.json().unwrap()["error"]["operation_id"], json!(id));
    gate.release();
    wait_finished(&daemon, &id);
    stop(daemon);
}

// ---- reopen ---------------------------------------------------------------

#[test]
fn reopen_dry_run_is_an_operation_and_changes_nothing() {
    let env = env();
    register_api(&env);
    save(&env, WorkspaceStatus::Done, "old", &["api"]);
    let daemon = start(&env, Script::default());

    let finished = run(
        &daemon,
        "POST",
        "/workspaces/old/reopen",
        json!({"dry_run": true, "no_multiplexer": true}),
    );
    assert_eq!(finished["kind"], "reopen");
    assert_eq!(finished["status"], "succeeded", "{finished}");
    assert_eq!(finished["result"]["dry_run"], true);
    assert!(finished["result"]["attach"].is_null());
    assert_eq!(
        env.mgr.load_workspace("old").unwrap().0,
        WorkspaceStatus::Done
    );

    assert_eq!(
        error_code(&post(
            &daemon,
            "/workspaces/old/reopen",
            json!({"from": ["garbage"]})
        )),
        (400, json!("bad_request"))
    );
    stop(daemon);
}

// ---- add-repo -------------------------------------------------------------

#[test]
fn add_repo_requires_a_repo_and_runs_as_an_operation() {
    let env = env();
    register_api(&env);
    save(&env, WorkspaceStatus::InProgress, "alpha", &[]);
    let script = Script {
        no_task_branches: true,
        ..Default::default()
    };
    let daemon = start(&env, script.clone());

    assert_eq!(
        error_code(&post(&daemon, "/workspaces/alpha/add-repo", json!({}))),
        (400, json!("bad_request"))
    );
    let finished = run(
        &daemon,
        "POST",
        "/workspaces/alpha/add-repo",
        json!({"repo": "api", "target_branch": "main", "env": {"PATH": "/snapshot/bin"}}),
    );
    assert_eq!(finished["kind"], "add-repo");
    assert_eq!(finished["status"], "succeeded", "{finished}");
    assert_eq!(finished["result"]["repo"], "api");
    assert_eq!(finished["result"]["workspace"], "alpha");
    assert_eq!(finished["result"]["target_branch"], "main");
    let (_, ws) = env.mgr.load_workspace("alpha").unwrap();
    assert_eq!(ws.repos.len(), 1);
    assert!(script
        .calls
        .lock()
        .unwrap()
        .iter()
        .all(|c| c.env["PATH"] == "/snapshot/bin"));

    let unknown = run(
        &daemon,
        "POST",
        "/workspaces/alpha/add-repo",
        json!({"repo": "ghost"}),
    );
    assert_eq!(unknown["status"], "failed");
    assert_eq!(unknown["error"]["code"], "operation_failed");
    stop(daemon);
}

// ---- create ---------------------------------------------------------------

#[test]
fn create_without_required_parameters_is_bad_request_not_a_wizard() {
    let env = env();
    register_api(&env);
    let daemon = start(&env, Script::default());

    for body in [
        json!({}),
        json!({"repos": ["api"]}),
        json!({"title": "feat: x"}),
        json!({"title": "feat: x", "repos": []}),
        json!({"title": "feat: x", "repos": ["api"], "name": "bad name"}),
    ] {
        assert_eq!(
            error_code(&post(&daemon, "/workspaces", body.clone())),
            (400, json!("bad_request")),
            "{body}"
        );
    }
    assert!(env
        .mgr
        .list_workspaces(None::<&[WorkspaceStatus]>)
        .unwrap()
        .is_empty());
    stop(daemon);
}

#[test]
fn create_makes_a_pending_workspace() {
    let env = env();
    register_api(&env);
    let daemon = start(&env, Script::default());

    let finished = run(
        &daemon,
        "POST",
        "/workspaces",
        json!({
            "title": "feat(x): add x",
            "name": "newone",
            "repos": ["api"],
            "description": "why",
        }),
    );
    assert_eq!(finished["kind"], "create");
    assert_eq!(finished["workspace"], "newone");
    assert_eq!(finished["status"], "succeeded", "{finished}");
    assert_eq!(finished["result"]["workspace"], "newone");
    assert_eq!(finished["result"]["status"], "pending");
    assert_eq!(finished["result"]["started"], false);
    let (status, ws) = env.mgr.load_workspace("newone").unwrap();
    assert_eq!(status, WorkspaceStatus::Pending);
    assert_eq!(ws.title, "feat(x): add x");
    assert_eq!(ws.repos[0].name, "api");

    // A second create with the same name fails as an operation.
    let again = run(
        &daemon,
        "POST",
        "/workspaces",
        json!({"title": "t", "name": "newone", "repos": [{"name": "api", "target_branch": "main"}]}),
    );
    assert_eq!(again["status"], "failed");
    assert_eq!(again["error"]["code"], "operation_failed");
    stop(daemon);
}

#[test]
fn create_generates_a_name_and_can_start_with_the_attach_request() {
    let env = env();
    register_api(&env);
    let script = Script::default();
    let daemon = start(&env, script.clone());

    let finished = run(
        &daemon,
        "POST",
        "/workspaces",
        json!({"title": "feat: y", "repos": ["api"], "start": true, "run_agent": "agent-x"}),
    );
    assert_eq!(finished["status"], "succeeded", "{finished}");
    let name = finished["result"]["workspace"].as_str().unwrap();
    assert_eq!(finished["workspace"], name);
    assert_eq!(finished["result"]["status"], "in_progress");
    assert_eq!(finished["result"]["started"], true);
    assert!(finished["result"]["attach"]["program"].is_string());
    assert_eq!(finished["result"]["agent_cli"], "agent-x");
    assert_eq!(
        env.mgr.load_workspace(name).unwrap().0,
        WorkspaceStatus::InProgress
    );
    assert_eq!(*script.interactive.lock().unwrap(), 0);
    stop(daemon);
}

// ---- delete archived ------------------------------------------------------

#[test]
fn only_archived_workspaces_can_be_deleted() {
    let env = env();
    save(&env, WorkspaceStatus::Done, "old", &[]);
    save(&env, WorkspaceStatus::Canceled, "gone", &[]);
    save(&env, WorkspaceStatus::InProgress, "live", &[]);
    let old_dir = workspace(&env, "old", &[]).workspace_dir;
    std::fs::create_dir_all(&old_dir).unwrap();
    let daemon = start(&env, Script::default());

    assert_eq!(
        error_code(&send(&daemon, "DELETE", "/workspaces/live", json!({}))),
        (400, json!("bad_request"))
    );
    assert_eq!(
        error_code(&send(&daemon, "DELETE", "/workspaces/missing", json!({}))),
        (404, json!("not_found"))
    );
    let finished = run(&daemon, "DELETE", "/workspaces/old", json!({}));
    assert_eq!(finished["kind"], "prune");
    assert_eq!(finished["status"], "succeeded", "{finished}");
    assert_eq!(finished["result"]["pruned"], json!(["old"]));
    assert!(!std::path::Path::new(&old_dir).exists());
    assert!(env.mgr.load_workspace("old").is_err());
    let canceled = run(&daemon, "DELETE", "/workspaces/gone", json!({}));
    assert_eq!(canceled["result"]["pruned"], json!(["gone"]));
    assert_eq!(
        env.mgr.load_workspace("live").unwrap().0,
        WorkspaceStatus::InProgress
    );
    stop(daemon);
}

// ---- repo registration and removal ----------------------------------------

#[test]
fn repos_can_be_registered_and_removed_as_operations() {
    let env = env();
    let src = env.tmp.path().join("checkout");
    std::fs::create_dir_all(&src).unwrap();
    let daemon = start(&env, Script::default());

    assert_eq!(
        error_code(&post(&daemon, "/repos", json!({}))),
        (400, json!("bad_request"))
    );
    let registered = run(
        &daemon,
        "POST",
        "/repos",
        json!({"path": src.to_string_lossy(), "default_target_branch": "main"}),
    );
    assert_eq!(registered["kind"], "repo-add");
    assert_eq!(registered["status"], "succeeded", "{registered}");
    assert_eq!(registered["result"]["name"], "checkout");
    let listed = get(&daemon, "/repos").json().unwrap();
    assert!(listed.to_string().contains("checkout"), "{listed}");
    let saved = env.mgr.load_repo_config("checkout").unwrap();
    assert_eq!(saved.default_target_branch.as_deref(), Some("main"));

    let removed = run(&daemon, "DELETE", "/repos/checkout", json!({}));
    assert_eq!(removed["kind"], "repo-remove");
    assert_eq!(removed["status"], "succeeded", "{removed}");
    assert!(env.mgr.list_repos().unwrap().is_empty());
    assert_eq!(
        error_code(&send(&daemon, "DELETE", "/repos/checkout", json!({}))),
        (404, json!("not_found"))
    );
    stop(daemon);
}

#[test]
fn mutating_endpoints_require_the_token() {
    let env = env();
    let daemon = start(&env, Script::default());
    for (method, path) in [
        ("POST", "/workspaces"),
        ("POST", "/workspaces/a/start"),
        ("POST", "/workspaces/a/reopen"),
        ("POST", "/workspaces/a/add-repo"),
        ("DELETE", "/workspaces/a"),
        ("POST", "/repos"),
        ("DELETE", "/repos/a"),
    ] {
        let r = request(&daemon.socket, method, path, None, T).unwrap();
        assert_eq!(r.status, 401, "{method} {path}");
    }
    stop(daemon);
}
