//! Command-line client mode: with a compatible Daemon running, commands go
//! through it and print what they print locally; without one (or with an
//! incompatible one) they run locally as before. Real `zootree` binary against
//! an in-process Daemon (or a canned fake) on a unix socket in a temporary
//! HOME. Nothing here touches the real ~/.config/zootree.

use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::os::unix::net::UnixListener;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};
use tempfile::TempDir;
use zootree::cli::remote::{self, Connection, DaemonClient, Emit, Followed, Mismatch};
use zootree::config::global::{GlobalConfig, HookValue, MultiplexerConfig};
use zootree::config::repo::RepoConfig;
use zootree::config::workspace::{RepoEntry, WorkspaceConfig, WorkspaceStatus};
use zootree::config::ConfigManager;
use zootree::daemon::client::{request, request_with_body, EventStream};
use zootree::daemon::endpoint::Discovery;
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
    /// Stdout of every command.
    stdout: String,
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
        Ok(Output {
            status: ExitStatus::from_raw(0),
            stdout: self.0.stdout.clone().into_bytes(),
            stderr: Vec::new(),
        })
    }

    fn run_interactive(&self, _spec: &CommandSpec) -> anyhow::Result<ExitStatus> {
        Ok(ExitStatus::from_raw(0))
    }
}

// ---- fixtures -------------------------------------------------------------

struct Home {
    tmp: TempDir,
    mgr: ConfigManager,
}

impl Home {
    fn new() -> Self {
        let tmp = TempDir::new().unwrap();
        let mgr = ConfigManager::with_base_dir(tmp.path().join(".config/zootree"));
        mgr.ensure_dirs().unwrap();
        Self { tmp, mgr }
    }

    fn path(&self) -> &Path {
        self.tmp.path()
    }

    /// A directory standing in for a repository.
    fn repo_dir(&self, name: &str) -> PathBuf {
        let dir = self.path().join("repos").join(name);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn register_repo(&self, name: &str) {
        let dir = self.repo_dir(name);
        self.mgr
            .save_repo_config(
                name,
                &RepoConfig {
                    path: dir.to_string_lossy().into_owned(),
                    default_target_branch: Some("main".into()),
                    copy_files: Vec::new(),
                    hooks: Default::default(),
                    lazygit: None,
                },
            )
            .unwrap();
    }

    fn workspace(&self, name: &str, status: WorkspaceStatus, repos: &[&str]) {
        let dir = self.path().join("ws").join(name);
        if matches!(status, WorkspaceStatus::Done | WorkspaceStatus::Canceled) {
            std::fs::create_dir_all(&dir).unwrap();
        }
        self.mgr
            .save_workspace(
                &status,
                &WorkspaceConfig {
                    title: format!("Title of {name}"),
                    name: name.into(),
                    description: String::new(),
                    branch: format!("zootree/{name}"),
                    workspace_dir: dir.to_string_lossy().into_owned(),
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
                },
            )
            .unwrap();
    }

    /// The same data in every Home, so two Homes can be compared.
    fn standard_fixture(&self) {
        self.register_repo("r1");
        self.workspace("pend", WorkspaceStatus::Pending, &["r1"]);
        self.workspace("prog", WorkspaceStatus::InProgress, &["r1"]);
        self.workspace("fin", WorkspaceStatus::Done, &["r1"]);
        self.workspace("gone", WorkspaceStatus::Canceled, &["r1"]);
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_zootree"));
        command
            .env("HOME", self.path())
            .env_remove("COMPLETE")
            .env_remove("ZELLIJ")
            .env_remove("ZELLIJ_SESSION_NAME")
            .env_remove("SSH_AUTH_SOCK")
            .args(args)
            .current_dir(self.path());
        command
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command(args).output().unwrap()
    }

    /// stdout with this Home's paths replaced, so two Homes compare equal.
    fn stdout(&self, output: &Output) -> String {
        self.normalize(&String::from_utf8_lossy(&output.stdout))
    }

    fn stderr(&self, output: &Output) -> String {
        self.normalize(&String::from_utf8_lossy(&output.stderr))
    }

    fn normalize(&self, text: &str) -> String {
        let mut text = text.to_string();
        if let Ok(real) = self.path().canonicalize() {
            text = text.replace(&real.to_string_lossy().into_owned(), "<HOME>");
        }
        text.replace(&self.path().to_string_lossy().into_owned(), "<HOME>")
    }

    fn start_daemon(&self, script: Script) -> DaemonHandle {
        let mut options = DaemonOptions::new(self.mgr.base_dir.clone());
        options.idle_timeout = Duration::from_secs(60);
        options.runner = RunnerFactory::new(move || Box::new(ScriptRunner(script.clone())));
        daemon::start(options).unwrap()
    }
}

fn stop(daemon: DaemonHandle) {
    daemon.shutdown();
    daemon.join().unwrap();
}

fn assert_success(home: &Home, output: &Output) {
    assert!(
        output.status.success(),
        "exit {:?}\nstdout: {}\nstderr: {}",
        output.status.code(),
        home.stdout(output),
        home.stderr(output)
    );
}

/// Collects `operation.finished` events (kind, status) from the Daemon.
struct FinishedKinds {
    seen: Arc<Mutex<Vec<(String, String)>>>,
}

impl FinishedKinds {
    fn watch(daemon: &DaemonHandle) -> Self {
        let mut stream =
            EventStream::connect(&daemon.socket, Some(&daemon.token), &[], None).unwrap();
        let seen: Arc<Mutex<Vec<(String, String)>>> = Arc::default();
        let sink = seen.clone();
        std::thread::spawn(move || {
            while let Ok(Some(frame)) = stream.next_frame(Duration::from_secs(60)) {
                if frame.event == "operation.finished" {
                    sink.lock().unwrap().push((
                        frame.data["kind"].as_str().unwrap().to_string(),
                        frame.data["status"].as_str().unwrap().to_string(),
                    ));
                }
            }
        });
        Self { seen }
    }

    fn kinds(&self) -> Vec<String> {
        let deadline = Instant::now() + Duration::from_millis(300);
        while Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        self.seen
            .lock()
            .unwrap()
            .iter()
            .map(|(k, _)| k.clone())
            .collect()
    }
}

// ---- a canned Daemon ------------------------------------------------------

/// Serves fixed answers on the endpoint's unix socket and records requests.
struct FakeDaemon {
    requests: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl FakeDaemon {
    fn start(home: &Home, status: Value, routes: Vec<(&'static str, Value)>) -> Self {
        let paths = DaemonOptions::new(home.mgr.base_dir.clone())
            .paths()
            .unwrap();
        paths.ensure_dir().unwrap();
        let listener = UnixListener::bind(&paths.socket).unwrap();
        listener.set_nonblocking(true).unwrap();
        Discovery {
            pid: std::process::id(),
            endpoint: paths.socket.clone(),
            token: "fake-token".into(),
            started_at: "2026-01-01T00:00:00Z".into(),
            protocol_version: 1,
        }
        .write(&paths.discovery)
        .unwrap();
        let requests: Arc<Mutex<Vec<String>>> = Arc::default();
        let stop = Arc::new(AtomicBool::new(false));
        let (log, flag) = (requests.clone(), stop.clone());
        let thread = std::thread::spawn(move || {
            while !flag.load(Ordering::SeqCst) {
                let Ok((mut conn, _)) = listener.accept() else {
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                };
                let _ = conn.set_nonblocking(false);
                let _ = conn.set_read_timeout(Some(Duration::from_millis(300)));
                let mut buf = vec![0u8; 8192];
                let Ok(n) = conn.read(&mut buf) else { continue };
                if n == 0 {
                    continue; // a liveness probe
                }
                let text = String::from_utf8_lossy(&buf[..n]).to_string();
                let line = text.lines().next().unwrap_or_default().to_string();
                let mut parts = line.split_whitespace();
                let (method, path) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
                log.lock().unwrap().push(format!("{method} {path}"));
                let body = if path == "/status" {
                    Some(status.clone())
                } else {
                    routes
                        .iter()
                        .find(|(p, _)| *p == path)
                        .map(|(_, v)| v.clone())
                };
                let (code, body) = match body {
                    Some(body) => ("200 OK", body),
                    None => (
                        "404 Not Found",
                        json!({"error": {"code": "not_found", "message": "no such endpoint"}}),
                    ),
                };
                let body = body.to_string();
                let _ = write!(
                    conn,
                    "HTTP/1.1 {code}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        Self {
            requests,
            stop,
            thread: Some(thread),
        }
    }

    fn requests(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }
}

impl Drop for FakeDaemon {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn compatible_status() -> Value {
    json!({
        "version": "0.1.4",
        "protocol_version": 1,
        "min_compatible_protocol_version": 1,
        "capabilities": ["status", "read_api", "operations", "events"],
        "pid": 1,
        "started_at": "2026-01-01T00:00:00Z",
    })
}

fn canned_summary(name: &str, title: &str) -> Value {
    json!({
        "name": name, "title": title, "status": "pending",
        "workspace_dir": "/nowhere", "branch": format!("zootree/{name}"),
        "repos": [], "created_at": "2026-06-29T10:00:00+08:00",
    })
}

// ---- output matches local execution ----------------------------------------

#[test]
fn commands_through_the_daemon_print_what_they_print_locally() {
    let with = Home::new();
    let without = Home::new();
    with.standard_fixture();
    without.standard_fixture();
    let daemon = with.start_daemon(Script::default());
    let finished = FinishedKinds::watch(&daemon);

    let new_repo_with = with.repo_dir("r2");
    let new_repo_without = without.repo_dir("r2");
    let steps: Vec<(Vec<String>, Vec<String>)> = {
        let same = |args: &[&str]| {
            let owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
            (owned.clone(), owned)
        };
        vec![
            same(&["list"]),
            same(&["list", "--oneline"]),
            same(&["list", "--json"]),
            same(&["list", "--status", "done", "--status", "canceled"]),
            same(&["info", "prog", "--json"]),
            same(&["repo", "list"]),
            same(&["repo", "list", "--json"]),
            (
                vec![
                    "repo".into(),
                    "add".into(),
                    new_repo_with.to_string_lossy().into_owned(),
                ],
                vec![
                    "repo".into(),
                    "add".into(),
                    new_repo_without.to_string_lossy().into_owned(),
                ],
            ),
            same(&["repo", "remove", "r2"]),
            same(&["cancel", "pend"]),
            same(&["done", "prog", "--dry-run", "--no-clean"]),
            same(&["prune", "--all"]),
            same(&["list", "--status", "done", "--status", "canceled"]),
            same(&[
                "create", "--title", "New task", "--name", "fresh", "--repos", "r1",
            ]),
            same(&["list"]),
        ]
    };
    for (args_with, args_without) in &steps {
        let a: Vec<&str> = args_with.iter().map(String::as_str).collect();
        let b: Vec<&str> = args_without.iter().map(String::as_str).collect();
        let out_with = with.run(&a);
        let out_without = without.run(&b);
        assert_success(&with, &out_with);
        assert_success(&without, &out_without);
        assert_eq!(
            with.stdout(&out_with),
            without.stdout(&out_without),
            "stdout differs for {a:?}"
        );
        assert_eq!(with.stderr(&out_with), "", "stderr for {a:?}");
    }
    // The mutating ones really went through the Daemon.
    let kinds = finished.kinds();
    for kind in [
        "repo-add",
        "repo-remove",
        "cancel",
        "done",
        "prune",
        "create",
    ] {
        assert!(
            kinds.iter().any(|k| k == kind),
            "no {kind} operation in {kinds:?}"
        );
    }
    assert_eq!(
        kinds.iter().filter(|k| *k == "prune").count(),
        3,
        "{kinds:?}"
    );
    stop(daemon);

    // And the end state matches too.
    let state = |home: &Home| {
        let mut names = Vec::new();
        for status in [
            WorkspaceStatus::Pending,
            WorkspaceStatus::InProgress,
            WorkspaceStatus::Done,
            WorkspaceStatus::Canceled,
        ] {
            for ws in home
                .mgr
                .list_workspaces(Some(std::slice::from_ref(&status)))
                .unwrap()
            {
                names.push(format!("{}:{}", status.as_str(), ws.name));
            }
        }
        names
    };
    assert_eq!(state(&with), state(&without));
}

#[test]
fn reads_come_from_the_daemon_when_one_is_running() {
    let home = Home::new();
    home.standard_fixture();
    let fake = FakeDaemon::start(
        &home,
        compatible_status(),
        vec![
            (
                "/workspaces",
                json!([canned_summary("from-daemon", "Served by the daemon")]),
            ),
            (
                "/repos",
                json!([{"name": "dr", "path": "/nonexistent/dr", "default_target_branch": null}]),
            ),
            ("/workspaces/prog", canned_summary("prog", "Daemon detail")),
        ],
    );

    let list = home.run(&["list", "--oneline"]);
    assert_success(&home, &list);
    assert_eq!(
        home.stdout(&list),
        "  from-daemon (pending) - Served by the daemon [(none)]\n"
    );

    let info = home.run(&["info", "prog", "--json"]);
    assert_success(&home, &info);
    assert!(home.stdout(&info).contains("Daemon detail"));

    let repos = home.run(&["repo", "list"]);
    assert_success(&home, &repos);
    assert_eq!(home.stdout(&repos), "  dr -> /nonexistent/dr (missing)\n");

    assert_eq!(
        fake.requests(),
        [
            "GET /status",
            "GET /workspaces",
            "GET /repos",
            "GET /status",
            "GET /workspaces/prog",
            "GET /status",
            "GET /repos"
        ]
    );
}

#[test]
fn recovery_and_editor_commands_never_touch_the_daemon() {
    let home = Home::new();
    home.standard_fixture();
    std::fs::write(
        home.mgr.base_dir.join("config.toml"),
        "this is = = not toml",
    )
    .unwrap();
    let fake = FakeDaemon::start(&home, compatible_status(), vec![]);

    let path = home.run(&["config", "path"]);
    assert_success(&home, &path);
    let mut repo_edit = home.command(&["repo", "edit", "r1"]);
    repo_edit.env("VISUAL", "true").env_remove("EDITOR");
    repo_edit.output().unwrap();
    let mut edit_ws = home.command(&["edit", "prog"]);
    edit_ws.env("VISUAL", "true").env_remove("EDITOR");
    edit_ws.output().unwrap();

    assert_eq!(fake.requests(), Vec::<String>::new());
}

// ---- fallback ---------------------------------------------------------------

#[test]
fn without_a_daemon_commands_run_locally_and_start_none() {
    let home = Home::new();
    home.standard_fixture();

    let list = home.run(&["list", "--oneline"]);
    assert_success(&home, &list);
    assert!(home.stdout(&list).contains("pend (pending)"));
    assert_eq!(home.stderr(&list), "");

    let paths = DaemonOptions::new(home.mgr.base_dir.clone())
        .paths()
        .unwrap();
    assert!(!paths.socket.exists(), "a command started a daemon");
    assert!(!paths.discovery.exists());
}

#[test]
fn a_discovery_file_without_a_listener_means_local_execution() {
    let home = Home::new();
    home.standard_fixture();
    let paths = DaemonOptions::new(home.mgr.base_dir.clone())
        .paths()
        .unwrap();
    paths.ensure_dir().unwrap();
    Discovery {
        pid: 1,
        endpoint: paths.socket.clone(),
        token: "stale".into(),
        started_at: "2026-01-01T00:00:00Z".into(),
        protocol_version: 1,
    }
    .write(&paths.discovery)
    .unwrap();

    let list = home.run(&["list", "--oneline"]);
    assert_success(&home, &list);
    assert!(home.stdout(&list).contains("pend (pending)"));
    assert_eq!(home.stderr(&list), "");
    // The leftovers were not cleaned up by a plain command.
    assert!(paths.discovery.exists());
}

#[test]
fn an_incompatible_protocol_is_explained_and_runs_locally() {
    let home = Home::new();
    home.standard_fixture();
    let mut status = compatible_status();
    status["protocol_version"] = json!(7);
    status["min_compatible_protocol_version"] = json!(5);
    let fake = FakeDaemon::start(&home, status, vec![]);

    let list = home.run(&["list", "--oneline"]);
    assert_success(&home, &list);
    // Local output...
    assert!(
        home.stdout(&list).contains("pend (pending)"),
        "{}",
        home.stdout(&list)
    );
    // ...and the reason on stderr only.
    let stderr = home.stderr(&list);
    assert!(stderr.contains("protocol 5..=7"), "{stderr}");
    assert!(stderr.contains("running locally"), "{stderr}");
    assert!(stderr.contains("upgrade zootree"), "{stderr}");
    // Only the version-exempt status request was made.
    assert_eq!(fake.requests(), ["GET /status"]);

    let json = home.run(&["list", "--json"]);
    assert!(
        serde_json::from_slice::<Value>(&json.stdout).is_ok(),
        "stdout stays clean JSON"
    );
}

#[test]
fn a_daemon_missing_a_capability_is_ignored_quietly() {
    let home = Home::new();
    home.standard_fixture();
    let mut status = compatible_status();
    status["capabilities"] = json!(["status", "read_api"]);
    let _fake = FakeDaemon::start(&home, status, vec![]);

    let list = home.run(&["list", "--oneline"]);
    assert_success(&home, &list);
    assert!(home.stdout(&list).contains("pend (pending)"));
    assert_eq!(home.stderr(&list), "");
}

#[test]
fn status_judgement_covers_ranges_and_capabilities() {
    assert_eq!(remote::check_status(&compatible_status()), Ok(()));
    let mut newer_daemon = compatible_status();
    newer_daemon["protocol_version"] = json!(4);
    assert_eq!(remote::check_status(&newer_daemon), Ok(()));
    let mut daemon_too_old = compatible_status();
    daemon_too_old["protocol_version"] = json!(0);
    daemon_too_old["min_compatible_protocol_version"] = json!(0);
    assert!(matches!(
        remote::check_status(&daemon_too_old),
        Err(Mismatch::Incompatible(text)) if text.contains("restart the daemon")
    ));
    let mut client_too_old = compatible_status();
    client_too_old["min_compatible_protocol_version"] = json!(2);
    client_too_old["protocol_version"] = json!(2);
    assert!(matches!(
        remote::check_status(&client_too_old),
        Err(Mismatch::Incompatible(_))
    ));
    assert!(matches!(
        remote::check_status(&json!({"version": "x"})),
        Err(Mismatch::Missing(_))
    ));
}

#[test]
fn discover_finds_a_real_daemon_and_reports_local_when_there_is_none() {
    let home = Home::new();
    assert!(matches!(
        remote::discover(&home.mgr.base_dir),
        Connection::Local { notice: None }
    ));
    let daemon = home.start_daemon(Script::default());
    assert!(matches!(
        remote::discover(&home.mgr.base_dir),
        Connection::Daemon(_)
    ));
    stop(daemon);
    assert!(matches!(
        remote::discover(&home.mgr.base_dir),
        Connection::Local { notice: None }
    ));
}

// ---- environment snapshot -----------------------------------------------------

#[test]
fn the_command_sends_its_environment_as_the_snapshot() {
    let home = Home::new();
    home.workspace("prog", WorkspaceStatus::InProgress, &[]);
    let mut global = GlobalConfig::default();
    global.hooks.pre_done = Some(HookValue::Simple("build".into()));
    home.mgr.save_global_config(&global).unwrap();
    let script = Script::default();
    let daemon = home.start_daemon(script.clone());

    let mut command = home.command(&["done", "prog", "--no-merge", "--no-clean"]);
    command
        .env("SSH_AUTH_SOCK", "/tmp/cli-agent.sock")
        .env("SSH_AGENT_PID", "4242")
        .env("HERDR_ENV", "1")
        .env("PATH", "/cli/bin:/usr/bin:/bin");
    let output = command.output().unwrap();
    assert_success(&home, &output);
    assert!(home.stdout(&output).contains("workspace 'prog' completed"));

    let envs = script.envs.lock().unwrap();
    assert!(!envs.is_empty(), "the hook ran no command");
    for env in envs.iter() {
        assert_eq!(
            env.get("SSH_AUTH_SOCK").map(String::as_str),
            Some("/tmp/cli-agent.sock")
        );
        assert_eq!(env.get("SSH_AGENT_PID").map(String::as_str), Some("4242"));
        assert_eq!(env.get("HERDR_ENV").map(String::as_str), Some("1"));
        assert_eq!(
            env.get("PATH").map(String::as_str),
            Some("/cli/bin:/usr/bin:/bin")
        );
        assert!(!env.contains_key("ZELLIJ"));
    }
    drop(envs);
    stop(daemon);
}

// ---- Ctrl-C and explicit cancel -------------------------------------------------

fn wait_until(what: &str, mut check: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !check() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn blocked_done(home: &Home) -> (DaemonHandle, Arc<Gate>) {
    home.workspace("prog", WorkspaceStatus::InProgress, &[]);
    let mut global = GlobalConfig::default();
    global.hooks.pre_done = Some(HookValue::Simple("build".into()));
    home.mgr.save_global_config(&global).unwrap();
    let gate = Arc::new(Gate::default());
    let daemon = home.start_daemon(Script {
        gate: Some(gate.clone()),
        ..Default::default()
    });
    (daemon, gate)
}

#[test]
fn ctrl_c_stops_waiting_but_not_the_operation() {
    let home = Home::new();
    let (daemon, gate) = blocked_done(&home);
    let mut watcher = EventStream::connect(&daemon.socket, Some(&daemon.token), &[], None).unwrap();

    let child = home
        .command(&["done", "prog", "--no-merge", "--no-clean"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // The Operation exists once its started event arrives.
    let id = loop {
        let frame = watcher.next_frame(T).unwrap().expect("stream ended");
        if frame.event == "operation.started" {
            break frame.data["operation_id"].as_str().unwrap().to_string();
        }
    };
    // Give the command time to reach its wait.
    std::thread::sleep(Duration::from_millis(600));
    // SAFETY: signalling our own child.
    unsafe { libc::kill(child.id() as i32, libc::SIGINT) };
    let output = child.wait_with_output().unwrap();

    assert_eq!(output.status.code(), Some(remote::INTERRUPTED_EXIT_CODE));
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(stderr.contains(&id), "{stderr}");
    assert!(
        stderr.contains(&format!("zootree operations cancel {id}")),
        "{stderr}"
    );

    // The Operation is untouched and finishes normally once unblocked.
    let view = |daemon: &DaemonHandle| {
        request(
            &daemon.socket,
            "GET",
            &format!("/operations/{id}"),
            Some(&daemon.token),
            T,
        )
        .unwrap()
        .json()
        .unwrap()
    };
    assert_eq!(view(&daemon)["status"], "running");
    assert_eq!(view(&daemon)["cancel_requested"], false);
    gate.release();
    wait_until("the operation to finish", || {
        view(&daemon)["status"] == "succeeded"
    });
    stop(daemon);
}

#[test]
fn following_an_operation_can_be_interrupted_and_resumed() {
    let home = Home::new();
    let (daemon, gate) = blocked_done(&home);
    let client = DaemonClient::new(daemon.socket.clone(), daemon.token.clone());
    let mut stream = client.open_event_stream().unwrap();
    let accepted = request_with_body(
        &daemon.socket,
        "POST",
        "/workspaces/prog/done",
        Some(&daemon.token),
        &[],
        Some(&json!({"no_merge": true, "no_clean": true}).to_string()),
        T,
    )
    .unwrap()
    .json()
    .unwrap();
    let id = accepted["id"].as_str().unwrap().to_string();

    let mut lines = Vec::new();
    let polls = std::cell::Cell::new(0);
    let followed =
        remote::follow_operation(&client, &mut stream, &id, &mut |e| lines.push(e), &|| {
            polls.set(polls.get() + 1);
            polls.get() > 3
        })
        .unwrap();
    assert!(matches!(followed, Followed::Interrupted));
    let running = client.operation(&id).unwrap();
    assert_eq!(
        running["status"], "running",
        "interrupting must not stop the operation"
    );

    gate.release();
    let followed =
        remote::follow_operation(&client, &mut stream, &id, &mut |e| lines.push(e), &|| false)
            .unwrap();
    let Followed::Ended(end) = followed else {
        panic!("expected the operation to end")
    };
    assert_eq!(end.status, "succeeded");
    assert!(
        lines
            .iter()
            .any(|e| matches!(e, Emit::Debug(_) | Emit::Step(_))),
        "{lines:?}"
    );
    stop(daemon);
}

#[test]
fn operations_cancel_asks_the_daemon_to_stop_the_operation() {
    let home = Home::new();
    let (daemon, gate) = blocked_done(&home);
    let mut watcher = EventStream::connect(&daemon.socket, Some(&daemon.token), &[], None).unwrap();
    let mut child = home
        .command(&["done", "prog", "--no-merge", "--no-clean"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let id = loop {
        let frame = watcher.next_frame(T).unwrap().expect("stream ended");
        if frame.event == "operation.started" {
            break frame.data["operation_id"].as_str().unwrap().to_string();
        }
    };

    let releaser = {
        let gate = gate.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(500));
            gate.release();
        })
    };
    let cancel = home.run(&["operations", "cancel", &id]);
    releaser.join().unwrap();
    assert_success(&home, &cancel);
    let stdout = home.stdout(&cancel);
    assert!(
        stdout.starts_with(&format!("cancel requested for operation {id}")),
        "{stdout}"
    );
    let view = request(
        &daemon.socket,
        "GET",
        &format!("/operations/{id}"),
        Some(&daemon.token),
        T,
    )
    .unwrap()
    .json()
    .unwrap();
    assert_eq!(view["cancel_requested"], true);
    let _ = child.wait();

    let again = home.run(&["operations", "cancel", &id]);
    assert_success(&home, &again);
    assert!(
        home.stdout(&again).contains("already finished"),
        "{}",
        home.stdout(&again)
    );

    let unknown = home.run(&["operations", "cancel", "op-0000000000000000"]);
    assert!(!unknown.status.success());
    stop(daemon);
}

#[test]
fn operations_cancel_needs_a_daemon() {
    let home = Home::new();
    let output = home.run(&["operations", "cancel", "op-abc"]);
    assert!(!output.status.success());
    let text = format!("{}{}", home.stdout(&output), home.stderr(&output));
    assert!(text.contains("no compatible zootree daemon"), "{text}");
}

#[test]
fn a_busy_workspace_is_reported_not_run_locally() {
    let home = Home::new();
    let (daemon, gate) = blocked_done(&home);
    let mut watcher = EventStream::connect(&daemon.socket, Some(&daemon.token), &[], None).unwrap();
    let mut first = home
        .command(&["done", "prog", "--no-merge", "--no-clean"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    loop {
        let frame = watcher.next_frame(T).unwrap().expect("stream ended");
        if frame.event == "operation.started" {
            break;
        }
    }
    let second = home.run(&["done", "prog", "--no-merge", "--no-clean"]);
    assert!(!second.status.success());
    let text = format!("{}{}", home.stdout(&second), home.stderr(&second));
    assert!(text.contains("already has operation"), "{text}");
    gate.release();
    let _ = first.wait();
    stop(daemon);
}

#[test]
fn start_and_add_repo_go_through_the_daemon() {
    let home = Home::new();
    home.standard_fixture();
    home.register_repo("r2");
    let daemon = home.start_daemon(Script {
        stdout: "refs/heads/develop\nrefs/heads/main\n".into(),
        ..Default::default()
    });
    let finished = FinishedKinds::watch(&daemon);

    let start = home.run(&["start", "pend", "--no-multiplexer"]);
    assert_success(&home, &start);
    assert!(
        home.stdout(&start).ends_with("workspace 'pend' started\n"),
        "{}",
        home.stdout(&start)
    );
    let (status, _) = home.mgr.load_workspace("pend").unwrap();
    assert_eq!(status, WorkspaceStatus::InProgress);

    let add = home.run(&["add-repo", "prog", "--repo", "r2:develop"]);
    assert_success(&home, &add);
    let (_, ws) = home.mgr.load_workspace("prog").unwrap();
    assert!(ws
        .repos
        .iter()
        .any(|r| r.name == "r2" && r.target_branch.as_deref() == Some("develop")));

    let kinds = finished.kinds();
    assert!(kinds.contains(&"start".to_string()), "{kinds:?}");
    assert!(kinds.contains(&"add-repo".to_string()), "{kinds:?}");
    stop(daemon);
}
