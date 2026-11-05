use std::os::unix::process::ExitStatusExt;
use std::process::{ExitStatus, Output};
use zootree::config::global::{GlobalConfig, HookValue, MultiplexerConfig, MultiplexerKind};
use zootree::config::repo::RepoConfig;
use zootree::config::workspace::{
    RepoEntry, StoredTerminalEnvironmentState, WorkspaceConfig, WorkspaceStatus,
};
use zootree::config::ConfigManager;
use zootree::core::reopen::{NonInteractiveReopenPrompt, ReopenOptions};
use zootree::runner::MockRunner;
use zootree::service::context::CollectingSink;
use zootree::service::reopen::{reopen_workspace, ReopenRequest};
use zootree::service::start::{
    open_workspace, start_workspace, AttachRequest, OpenRequest, StartRequest,
};
use zootree::service::{EnvSnapshot, RequestContext, ServiceEvent};

fn success_output() -> Output {
    success_stdout("")
}

fn success_stdout(stdout: &str) -> Output {
    Output {
        status: ExitStatus::from_raw(0),
        stdout: stdout.as_bytes().to_vec(),
        stderr: Vec::new(),
    }
}

fn failure_output(stderr: &str) -> Output {
    Output {
        status: ExitStatus::from_raw(1 << 8),
        stdout: Vec::new(),
        stderr: stderr.as_bytes().to_vec(),
    }
}

fn setup() -> (tempfile::TempDir, ConfigManager) {
    let tmp = tempfile::tempdir().unwrap();
    let mgr = ConfigManager::with_base_dir(tmp.path().join("config"));
    mgr.ensure_dirs().unwrap();
    (tmp, mgr)
}

fn repo_config(path: &str) -> RepoConfig {
    RepoConfig {
        path: path.into(),
        default_target_branch: None,
        copy_files: Vec::new(),
        hooks: Default::default(),
        lazygit: None,
    }
}

fn workspace(name: &str, dir: &std::path::Path, repos: &[&str]) -> WorkspaceConfig {
    WorkspaceConfig {
        title: format!("{name} title"),
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
    }
}

fn stored_terminal_state(source: &str) -> StoredTerminalEnvironmentState {
    toml::from_str(source).unwrap()
}

fn state_table(state: &StoredTerminalEnvironmentState) -> toml::Table {
    toml::Value::try_from(state)
        .unwrap()
        .as_table()
        .unwrap()
        .clone()
}

fn snapshot(extra: &[(&str, &str)]) -> EnvSnapshot {
    let mut pairs = vec![("PATH".to_string(), "/caller/bin".to_string())];
    pairs.extend(extra.iter().map(|(k, v)| (k.to_string(), v.to_string())));
    EnvSnapshot::from_pairs(pairs)
}

fn start_request(name: &str, no_multiplexer: bool) -> StartRequest {
    StartRequest {
        name: name.into(),
        multiplexer: None,
        no_multiplexer,
        run_agent: None,
    }
}

#[test]
fn start_rolls_back_created_worktree_when_post_create_hook_fails() {
    let (tmp, mgr) = setup();
    let workspace_dir = tmp.path().join("workspaces/fair-fox");
    let mut repo_cfg = repo_config("/repo/api");
    repo_cfg.hooks.post_create = Some(HookValue::Simple("exit 42".into()));
    mgr.save_repo_config("api", &repo_cfg).unwrap();
    mgr.save_workspace(
        &WorkspaceStatus::Pending,
        &workspace("fair-fox", &workspace_dir, &["api"]),
    )
    .unwrap();
    let runner = MockRunner::new();
    runner.push_response(success_stdout("refs/heads/main\n"));
    runner.push_response(success_output());
    runner.push_response(failure_output("boom"));
    runner.push_response(success_output());
    let sink = CollectingSink::new();
    let ctx = RequestContext::new(&sink, snapshot(&[]));

    let err = start_workspace(&mgr, &runner, &ctx, &start_request("fair-fox", true)).unwrap_err();
    let msg = format!("{err:#}");

    assert!(msg.contains("hook failed"), "unexpected error: {msg}");
    assert!(
        !workspace_dir.exists(),
        "rollback should remove empty workspace dir created by start"
    );
    assert_eq!(
        mgr.load_workspace("fair-fox").unwrap().0,
        WorkspaceStatus::Pending
    );
    let calls = runner.take_calls();
    assert_eq!(
        calls[2].env.get("ZOOTREE_HOOK").map(String::as_str),
        Some("post_create")
    );
    assert_eq!(
        calls[2].env.get("PATH").map(String::as_str),
        Some("/caller/bin")
    );
    assert_eq!(
        calls[3].args,
        vec![
            "-C",
            "/repo/api",
            "worktree",
            "remove",
            "--force",
            &format!("{}/api", workspace_dir.to_string_lossy()),
        ]
    );
}

#[test]
fn start_reports_rollback_failure_together_with_the_original_error() {
    let (tmp, mgr) = setup();
    let workspace_dir = tmp.path().join("workspaces/fair-fox");
    let mut repo_cfg = repo_config("/repo/api");
    repo_cfg.hooks.post_create = Some(HookValue::Simple("exit 42".into()));
    mgr.save_repo_config("api", &repo_cfg).unwrap();
    mgr.save_workspace(
        &WorkspaceStatus::Pending,
        &workspace("fair-fox", &workspace_dir, &["api"]),
    )
    .unwrap();
    let runner = MockRunner::new();
    runner.push_response(success_stdout("refs/heads/main\n"));
    runner.push_response(success_output());
    runner.push_response(failure_output("boom"));
    runner.push_response(failure_output("cannot remove"));
    let sink = CollectingSink::new();
    let ctx = RequestContext::new(&sink, snapshot(&[]));

    let err = start_workspace(&mgr, &runner, &ctx, &start_request("fair-fox", true)).unwrap_err();
    let msg = format!("{err:#}");

    assert!(msg.contains("start failed"), "{msg}");
    assert!(msg.contains("rollback failed"), "{msg}");
    assert_eq!(
        mgr.load_workspace("fair-fox").unwrap().0,
        WorkspaceStatus::Pending
    );
}

#[test]
fn start_emits_started_runs_post_start_with_in_progress_status_and_syncs_indexes() {
    let (tmp, mgr) = setup();
    mgr.save_repo_config("api", &repo_config("/repo/api"))
        .unwrap();
    let mut global = GlobalConfig::default();
    global.hooks.post_start = Some(HookValue::Simple("after-start".into()));
    mgr.save_global_config(&global).unwrap();
    let workspace_dir = tmp.path().join("workspaces/calm-river");
    std::fs::create_dir_all(workspace_dir.join("api")).unwrap();
    std::fs::write(workspace_dir.join("api/AGENTS.md"), "api rules").unwrap();
    mgr.save_workspace(
        &WorkspaceStatus::Pending,
        &workspace("calm-river", &workspace_dir, &["api"]),
    )
    .unwrap();
    let runner = MockRunner::new();
    runner.push_response(success_stdout("refs/heads/main\n"));
    runner.push_response(success_output());
    runner.push_response(success_output());
    let sink = CollectingSink::new();
    let ctx = RequestContext::new(&sink, snapshot(&[]));

    let outcome = start_workspace(&mgr, &runner, &ctx, &start_request("calm-river", true)).unwrap();

    assert_eq!(outcome.workspace, "calm-river");
    assert!(outcome.attach.is_none());
    assert_eq!(
        sink.events(),
        vec![ServiceEvent::Started {
            operation: "start",
            workspace: "calm-river".into()
        }]
    );
    let calls = runner.take_calls();
    let hook = calls.iter().find(|call| call.program == "sh").unwrap();
    assert_eq!(hook.args, vec!["-c", "after-start"]);
    assert_eq!(
        hook.env.get("ZOOTREE_WORKSPACE_STATUS").map(String::as_str),
        Some("in_progress")
    );
    assert_eq!(
        mgr.load_workspace("calm-river").unwrap().0,
        WorkspaceStatus::InProgress
    );
    assert_eq!(
        std::fs::read_to_string(workspace_dir.join("AGENTS.md")).unwrap(),
        "# Workspace repository instructions\n\n\
- For work in `api/`, read and follow `api/AGENTS.md`.\n"
    );
}

#[test]
fn start_rejects_a_workspace_that_is_not_pending() {
    let (tmp, mgr) = setup();
    mgr.save_workspace(
        &WorkspaceStatus::InProgress,
        &workspace("busy", &tmp.path().join("ws"), &[]),
    )
    .unwrap();
    let sink = CollectingSink::new();
    let ctx = RequestContext::new(&sink, snapshot(&[]));

    let err =
        start_workspace(&mgr, &MockRunner::new(), &ctx, &start_request("busy", true)).unwrap_err();
    assert!(err.to_string().contains("not in pending state"));

    let err =
        start_workspace(&mgr, &MockRunner::new(), &ctx, &start_request("", true)).unwrap_err();
    assert!(err.to_string().contains("name is required"));
}

#[test]
fn start_activation_failure_is_partial_success_and_open_retries() {
    let (tmp, mgr) = setup();
    mgr.save_repo_config("api", &repo_config("/repo/api"))
        .unwrap();
    let workspace_dir = tmp.path().join("workspaces/partial-start");
    mgr.save_workspace(
        &WorkspaceStatus::Pending,
        &workspace("partial-start", &workspace_dir, &["api"]),
    )
    .unwrap();
    let start_runner = MockRunner::new();
    start_runner.push_response(success_stdout("refs/heads/main\n"));
    start_runner.push_response(success_output());
    start_runner.push_response(failure_output("cmux unavailable"));
    let sink = CollectingSink::new();
    let ctx = RequestContext::new(&sink, snapshot(&[]));
    let request = StartRequest {
        multiplexer: Some(MultiplexerKind::Cmux),
        ..start_request("partial-start", false)
    };

    let error = start_workspace(&mgr, &start_runner, &ctx, &request).unwrap_err();

    let message = format!("{error:#}");
    assert!(
        message.contains("started and remains in_progress"),
        "{message}"
    );
    assert!(message.contains("zootree open partial-start"), "{message}");
    let (status, after_failure) = mgr.load_workspace("partial-start").unwrap();
    assert_eq!(status, WorkspaceStatus::InProgress);
    assert!(after_failure.multiplexer_state.is_empty());
    assert_eq!(after_failure.multiplexer.kind, MultiplexerKind::Cmux);
    assert!(workspace_dir.exists());
    assert!(!start_runner
        .take_calls()
        .iter()
        .any(|call| call.program == "git" && call.args.iter().any(|arg| arg == "remove")));

    std::fs::create_dir_all(workspace_dir.join("api")).unwrap();
    let open_runner = MockRunner::new();
    open_runner.push_response(success_stdout(
        r#"{"groups":[{"name":"partial-start title","ref":"workspace_group:7"}]}"#,
    ));
    open_runner.push_response(success_output());
    let outcome = open_workspace(
        &mgr,
        &open_runner,
        &ctx,
        &OpenRequest {
            name: "partial-start".into(),
        },
    )
    .unwrap();

    assert!(outcome.attach.is_none());
    let (_, after_retry) = mgr.load_workspace("partial-start").unwrap();
    let state = state_table(&after_retry.multiplexer_state);
    assert_eq!(
        state.get("adapter").and_then(toml::Value::as_str),
        Some("cmux")
    );
}

#[test]
fn no_multiplexer_start_leaves_the_terminal_environment_for_open() {
    let (tmp, mgr) = setup();
    mgr.save_repo_config("api", &repo_config("/repo/api"))
        .unwrap();
    let workspace_dir = tmp.path().join("workspaces/deferred-terminal");
    let mut ws = workspace("deferred-terminal", &workspace_dir, &["api"]);
    ws.multiplexer.kind = MultiplexerKind::Zellij;
    mgr.save_workspace(&WorkspaceStatus::Pending, &ws).unwrap();
    let runner = MockRunner::new();
    runner.push_response(success_stdout("refs/heads/main\n"));
    runner.push_response(success_output());
    let sink = CollectingSink::new();
    let ctx = RequestContext::new(&sink, snapshot(&[]));
    let request = StartRequest {
        multiplexer: Some(MultiplexerKind::Cmux),
        ..start_request("deferred-terminal", true)
    };

    start_workspace(&mgr, &runner, &ctx, &request).unwrap();

    let (_, started) = mgr.load_workspace("deferred-terminal").unwrap();
    assert_eq!(started.multiplexer.kind, MultiplexerKind::Cmux);
    assert!(started.multiplexer_state.is_empty());
    assert!(runner.take_calls().iter().all(|call| call.program == "git"));
}

#[test]
fn open_persists_canonical_state_and_emits_reconciliation_warning() {
    let (tmp, mgr) = setup();
    let mut ws = workspace("warning-state", &tmp.path().join("ws"), &[]);
    ws.multiplexer.kind = MultiplexerKind::Cmux;
    ws.multiplexer_state = stored_terminal_state(
        r#"
version = 99
adapter = "future"

[payload]
identity = "future:1"
"#,
    );
    mgr.save_workspace(&WorkspaceStatus::InProgress, &ws)
        .unwrap();
    let runner = MockRunner::new();
    runner.push_response(success_stdout(
        r#"{"groups":[{"name":"warning-state title","ref":"workspace_group:11"}]}"#,
    ));
    runner.push_response(success_output());
    let sink = CollectingSink::new();
    let ctx = RequestContext::new(&sink, snapshot(&[]));

    open_workspace(
        &mgr,
        &runner,
        &ctx,
        &OpenRequest {
            name: "warning-state".into(),
        },
    )
    .unwrap();

    let events = sink.events();
    assert!(events.iter().any(|event| matches!(
        event,
        ServiceEvent::Warning(message)
            if message.starts_with("terminal environment for workspace 'warning-state':")
                && message.contains("version 99")
    )));
    let (_, opened) = mgr.load_workspace("warning-state").unwrap();
    let state = state_table(&opened.multiplexer_state);
    assert_eq!(
        state.get("version").and_then(toml::Value::as_integer),
        Some(1)
    );
}

fn zellij_workspace(mgr: &ConfigManager, tmp: &std::path::Path) {
    let mut ws = workspace("zellij-open", &tmp.join("ws"), &[]);
    ws.multiplexer.kind = MultiplexerKind::Zellij;
    ws.multiplexer_state = stored_terminal_state(
        r#"
version = 1
adapter = "zellij"

[payload]
session = "zootree-zellij-open"
"#,
    );
    mgr.save_workspace(&WorkspaceStatus::InProgress, &ws)
        .unwrap();
}

#[test]
fn open_outside_zellij_returns_the_attach_instead_of_running_it() {
    let (tmp, mgr) = setup();
    zellij_workspace(&mgr, tmp.path());
    let runner = MockRunner::new();
    runner.push_response(success_stdout("zootree-zellij-open\n"));
    let sink = CollectingSink::new();
    let ctx = RequestContext::new(&sink, snapshot(&[]));

    let outcome = open_workspace(
        &mgr,
        &runner,
        &ctx,
        &OpenRequest {
            name: "zellij-open".into(),
        },
    )
    .unwrap();

    assert_eq!(
        outcome.attach,
        Some(AttachRequest {
            program: "zellij".into(),
            args: vec!["attach".into(), "zootree-zellij-open".into()],
        })
    );
    let calls = runner.take_calls();
    assert_eq!(calls.len(), 1, "the attach must not run in the service");
    assert_eq!(calls[0].args, vec!["list-sessions"]);
    let (_, opened) = mgr.load_workspace("zellij-open").unwrap();
    assert_eq!(
        state_table(&opened.multiplexer_state)
            .get("adapter")
            .and_then(toml::Value::as_str),
        Some("zellij")
    );
}

#[test]
fn open_uses_the_injected_caller_context_for_zellij() {
    let (tmp, mgr) = setup();
    zellij_workspace(&mgr, tmp.path());
    let runner = MockRunner::new();
    runner.push_response(success_stdout("zootree-zellij-open\n"));
    let sink = CollectingSink::new();
    let ctx = RequestContext::new(&sink, snapshot(&[("ZELLIJ", "0")]));

    let outcome = open_workspace(
        &mgr,
        &runner,
        &ctx,
        &OpenRequest {
            name: "zellij-open".into(),
        },
    )
    .unwrap();

    assert!(outcome.attach.is_none());
    assert_eq!(
        sink.events()
            .into_iter()
            .filter_map(|event| match event {
                ServiceEvent::Step(line) => Some(line),
                _ => None,
            })
            .collect::<Vec<_>>(),
        vec![
            "zellij session 'zootree-zellij-open' already exists.".to_string(),
            "Run `zootree open zellij-open` (outside zellij) to attach.".to_string(),
        ]
    );
}

#[test]
fn open_rejects_a_workspace_that_is_not_in_progress() {
    let (tmp, mgr) = setup();
    mgr.save_workspace(
        &WorkspaceStatus::Pending,
        &workspace("early", &tmp.path().join("ws"), &[]),
    )
    .unwrap();
    let sink = CollectingSink::new();
    let ctx = RequestContext::new(&sink, snapshot(&[]));

    let err = open_workspace(
        &mgr,
        &MockRunner::new(),
        &ctx,
        &OpenRequest {
            name: "early".into(),
        },
    )
    .unwrap_err();
    assert!(err.to_string().contains("not in_progress"));
}

#[test]
fn attach_request_failures_match_the_adapter_behavior() {
    let zellij = AttachRequest {
        program: "zellij".into(),
        args: vec!["attach".into(), "s".into()],
    };
    let runner = MockRunner::new();
    runner.push_response(success_output());
    assert_eq!(zellij.run(&runner).unwrap(), None);
    let runner = MockRunner::new();
    runner.push_response(failure_output(""));
    let err = zellij.run(&runner).unwrap_err();
    assert_eq!(err.to_string(), "zellij exited with exit code 1");

    let herdr = AttachRequest {
        program: "herdr".into(),
        args: vec!["session".into(), "attach".into(), "agents".into()],
    };
    let runner = MockRunner::new();
    runner.push_response(failure_output(""));
    let warning = herdr.run(&runner).unwrap().unwrap();
    assert_eq!(
        warning,
        "failed to attach Herdr session 'agents': Herdr session attach failed with exit code 1; run `herdr session attach agents`"
    );
}

fn reopen_fixture() -> (tempfile::TempDir, ConfigManager, MockRunner) {
    let (tmp, mgr) = setup();
    mgr.save_repo_config("frontend", &repo_config("/repos/frontend"))
        .unwrap();
    let dir = tmp.path().join("calm-river");
    let worktree = dir.join("frontend");
    std::fs::create_dir_all(&worktree).unwrap();
    mgr.save_workspace(
        &WorkspaceStatus::Done,
        &workspace("calm-river", &dir, &["frontend"]),
    )
    .unwrap();
    let runner = MockRunner::new();
    runner.push_response(success_stdout("refs/heads/zootree/calm-river\n"));
    runner.push_response(success_stdout(&format!(
        "worktree /repos/frontend\0HEAD 1111111\0branch refs/heads/main\0\0worktree {}\0HEAD 2222222\0branch refs/heads/zootree/calm-river\0",
        worktree.display()
    )));
    (tmp, mgr, runner)
}

fn reopen_request(dry_run: bool) -> ReopenRequest {
    ReopenRequest {
        name: "calm-river".into(),
        options: ReopenOptions::default(),
        run_agent: None,
        skip_hooks: true,
        no_multiplexer: true,
        dry_run,
    }
}

#[test]
fn reopen_dry_run_prints_the_plan_and_changes_nothing() {
    let (_tmp, mgr, runner) = reopen_fixture();
    let sink = CollectingSink::new();
    let ctx = RequestContext::new(&sink, snapshot(&[]));

    let outcome = reopen_workspace(
        &mgr,
        &runner,
        &ctx,
        &mut NonInteractiveReopenPrompt,
        &reopen_request(true),
    )
    .unwrap();

    assert!(outcome.dry_run);
    assert!(outcome.attach.is_none());
    let events = sink.events();
    assert_eq!(
        events[0],
        ServiceEvent::Started {
            operation: "reopen",
            workspace: "calm-river".into()
        }
    );
    let ServiceEvent::Step(plan) = &events[1] else {
        panic!("expected the plan as a step: {events:?}");
    };
    assert!(plan.starts_with("reopen 'calm-river' from done:"), "{plan}");
    assert!(!plan.ends_with('\n'));
    assert_eq!(
        events[2],
        ServiceEvent::Step("dry run: no changes made".into())
    );
    assert_eq!(
        mgr.load_workspace("calm-river").unwrap().0,
        WorkspaceStatus::Done
    );
}

#[test]
fn reopen_moves_the_workspace_to_in_progress_without_touching_the_terminal() {
    let (_tmp, mgr, runner) = reopen_fixture();
    let sink = CollectingSink::new();
    let ctx = RequestContext::new(&sink, snapshot(&[]));

    let outcome = reopen_workspace(
        &mgr,
        &runner,
        &ctx,
        &mut NonInteractiveReopenPrompt,
        &reopen_request(false),
    )
    .unwrap();

    assert!(!outcome.dry_run);
    assert!(outcome.attach.is_none());
    assert_eq!(
        mgr.load_workspace("calm-river").unwrap().0,
        WorkspaceStatus::InProgress
    );
}

#[test]
fn reopen_activates_the_terminal_and_returns_the_attach() {
    let (_tmp, mgr, runner) = reopen_fixture();
    let mut global = GlobalConfig::default();
    global.multiplexer.kind = MultiplexerKind::Zellij;
    mgr.save_global_config(&global).unwrap();
    // zellij list-sessions (display name lookup) finds nothing, then the
    // layout is created in the foreground, which the service defers.
    runner.push_response(success_stdout(""));
    runner.push_response(success_output());
    let sink = CollectingSink::new();
    let ctx = RequestContext::new(&sink, snapshot(&[]));
    let request = ReopenRequest {
        no_multiplexer: false,
        ..reopen_request(false)
    };

    let outcome = reopen_workspace(
        &mgr,
        &runner,
        &ctx,
        &mut NonInteractiveReopenPrompt,
        &request,
    )
    .unwrap();

    let attach = outcome.attach.expect("zellij creates a foreground session");
    assert_eq!(attach.program, "zellij");
    assert_eq!(attach.args[0], "--new-session-with-layout");
    let calls = runner.take_calls();
    assert!(
        calls
            .iter()
            .all(|call| call.args[0] != "--new-session-with-layout"),
        "the foreground session must not start inside the service"
    );
}
