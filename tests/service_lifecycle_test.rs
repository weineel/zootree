use std::os::unix::process::ExitStatusExt;
use std::process::{ExitStatus, Output};
use zootree::config::global::{GlobalConfig, HookValue, MultiplexerConfig};
use zootree::config::repo::RepoConfig;
use zootree::config::workspace::{RepoEntry, WorkspaceConfig, WorkspaceStatus};
use zootree::config::ConfigManager;
use zootree::runner::MockRunner;
use zootree::service::context::CollectingSink;
use zootree::service::lifecycle::{
    cancel_workspace, done_workspace, CancelRequest, DoneRequest, UncommittedConfirmationRequired,
};
use zootree::service::{EnvSnapshot, RequestContext, ServiceEvent};

fn output(stdout: &str) -> Output {
    Output {
        status: ExitStatus::from_raw(0),
        stdout: stdout.as_bytes().to_vec(),
        stderr: Vec::new(),
    }
}

fn runner_with_successes(first: Option<&str>) -> MockRunner {
    let runner = MockRunner::new();
    if let Some(first) = first {
        runner.push_response(output(first));
    }
    for _ in 0..20 {
        runner.push_response(output(""));
    }
    runner
}

fn workspace(name: &str, dir: &std::path::Path, repos: &[&str]) -> WorkspaceConfig {
    WorkspaceConfig {
        title: "Title".into(),
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

fn snapshot() -> EnvSnapshot {
    EnvSnapshot::from_pairs([
        ("PATH".to_string(), "/caller/bin".to_string()),
        ("SSH_AUTH_SOCK".to_string(), "/tmp/caller.sock".to_string()),
        ("UNLISTED".to_string(), "dropped".to_string()),
    ])
}

fn setup() -> (tempfile::TempDir, ConfigManager) {
    let tmp = tempfile::tempdir().unwrap();
    let mgr = ConfigManager::with_base_dir(tmp.path().join("config"));
    mgr.ensure_dirs().unwrap();
    (tmp, mgr)
}

#[test]
fn done_dry_run_emits_events_in_order_and_changes_nothing() {
    let (tmp, mgr) = setup();
    let ws = workspace("dry", &tmp.path().join("ws"), &["api"]);
    mgr.save_workspace(&WorkspaceStatus::InProgress, &ws)
        .unwrap();
    let sink = CollectingSink::new();
    let ctx = RequestContext::new(&sink, snapshot());
    let runner = MockRunner::new();

    // dry run still requires nothing from git
    let req = DoneRequest {
        name: "dry".into(),
        dry_run: true,
        ..Default::default()
    };
    let outcome = done_workspace(&mgr, &runner, &ctx, &req).unwrap();

    assert!(outcome.dry_run);
    assert_eq!(
        sink.events(),
        vec![
            ServiceEvent::Started {
                operation: "done",
                workspace: "dry".into()
            },
            ServiceEvent::Step("dry run for workspace 'dry':".into()),
            ServiceEvent::Step("  merge zootree/dry -> main".into()),
            ServiceEvent::Step("  clean worktrees and workspace directory".into()),
        ]
    );
    assert!(runner.take_calls().is_empty());
    assert_eq!(
        mgr.load_workspace("dry").unwrap().0,
        WorkspaceStatus::InProgress
    );
}

#[test]
fn done_applies_environment_snapshot_to_hook_and_finishes() {
    let (tmp, mgr) = setup();
    let mut global = GlobalConfig::default();
    global.hooks.pre_done = Some(HookValue::Simple("true".into()));
    mgr.save_global_config(&global).unwrap();
    let ws = workspace("env", &tmp.path().join("ws"), &[]);
    mgr.save_workspace(&WorkspaceStatus::InProgress, &ws)
        .unwrap();
    let sink = CollectingSink::new();
    let ctx = RequestContext::new(&sink, snapshot());
    let runner = runner_with_successes(None);

    let req = DoneRequest {
        name: "env".into(),
        no_merge: true,
        no_clean: true,
        ..Default::default()
    };
    done_workspace(&mgr, &runner, &ctx, &req).unwrap();

    let calls = runner.take_calls();
    let hook = &calls[0];
    assert_eq!(hook.program, "sh");
    assert_eq!(
        hook.env.get("PATH").map(String::as_str),
        Some("/caller/bin")
    );
    assert_eq!(
        hook.env.get("SSH_AUTH_SOCK").map(String::as_str),
        Some("/tmp/caller.sock")
    );
    assert!(!hook.env.contains_key("UNLISTED"));
    // Whitelisted variables absent from the snapshot must not leak from the host.
    assert!(hook.env_remove.iter().any(|n| n == "ZELLIJ"));
    assert!(calls
        .iter()
        .all(|c| c.env.get("PATH").map(String::as_str) == Some("/caller/bin")));

    let events = sink.events();
    assert!(matches!(
        events.first(),
        Some(ServiceEvent::Started {
            operation: "done",
            ..
        })
    ));
    assert_eq!(
        events.last(),
        Some(&ServiceEvent::Finished {
            operation: "done",
            workspace: "env".into(),
            summary: "workspace 'env' completed".into()
        })
    );
    assert_eq!(mgr.load_workspace("env").unwrap().0, WorkspaceStatus::Done);
}

#[test]
fn done_without_a_name_errors_instead_of_prompting() {
    let (_tmp, mgr) = setup();
    let sink = CollectingSink::new();
    let ctx = RequestContext::new(&sink, snapshot());
    let runner = MockRunner::new();

    let err = done_workspace(&mgr, &runner, &ctx, &DoneRequest::default()).unwrap_err();
    assert!(err.to_string().contains("workspace name is required"));
    let err = cancel_workspace(&mgr, &runner, &ctx, &CancelRequest::default()).unwrap_err();
    assert!(err.to_string().contains("workspace name is required"));
    assert!(sink.events().is_empty());
}

#[test]
fn cancel_pending_workspace_emits_started_then_finished() {
    let (tmp, mgr) = setup();
    let ws = workspace("pend", &tmp.path().join("ws"), &[]);
    mgr.save_workspace(&WorkspaceStatus::Pending, &ws).unwrap();
    let sink = CollectingSink::new();
    let ctx = RequestContext::new(&sink, snapshot());

    cancel_workspace(
        &mgr,
        &MockRunner::new(),
        &ctx,
        &CancelRequest {
            name: "pend".into(),
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(
        sink.events(),
        vec![
            ServiceEvent::Started {
                operation: "cancel",
                workspace: "pend".into()
            },
            ServiceEvent::Finished {
                operation: "cancel",
                workspace: "pend".into(),
                summary: "workspace 'pend' canceled".into()
            },
        ]
    );
    assert_eq!(
        mgr.load_workspace("pend").unwrap().0,
        WorkspaceStatus::Canceled
    );
}

#[test]
fn cancel_with_uncommitted_changes_requires_confirmation_then_proceeds() {
    let (tmp, mgr) = setup();
    let ws_dir = tmp.path().join("ws");
    std::fs::create_dir_all(ws_dir.join("api")).unwrap();
    mgr.save_repo_config(
        "api",
        &RepoConfig {
            path: tmp.path().join("api-src").to_string_lossy().into_owned(),
            default_target_branch: None,
            copy_files: Vec::new(),
            hooks: Default::default(),
            lazygit: None,
        },
    )
    .unwrap();
    let ws = workspace("dirty", &ws_dir, &["api"]);
    mgr.save_workspace(&WorkspaceStatus::InProgress, &ws)
        .unwrap();
    let sink = CollectingSink::new();
    let ctx = RequestContext::new(&sink, snapshot());
    let runner = runner_with_successes(Some(" M file.rs\n"));

    let mut req = CancelRequest {
        name: "dirty".into(),
        ..Default::default()
    };
    let err = cancel_workspace(&mgr, &runner, &ctx, &req).unwrap_err();
    let needed = err
        .downcast_ref::<UncommittedConfirmationRequired>()
        .unwrap();
    assert_eq!(needed.repos, vec!["api".to_string()]);
    assert_eq!(
        mgr.load_workspace("dirty").unwrap().0,
        WorkspaceStatus::InProgress
    );

    req.confirmed_uncommitted = true;
    cancel_workspace(&mgr, &runner, &ctx, &req).unwrap();
    assert_eq!(
        mgr.load_workspace("dirty").unwrap().0,
        WorkspaceStatus::Canceled
    );
}

fn hook_workspace(name: &str, hook: &str) -> (tempfile::TempDir, ConfigManager) {
    let (tmp, mgr) = setup();
    let mut global = GlobalConfig::default();
    global.hooks.pre_done = Some(HookValue::Simple(hook.into()));
    mgr.save_global_config(&global).unwrap();
    let ws = workspace(name, &tmp.path().join("ws"), &[]);
    mgr.save_workspace(&WorkspaceStatus::InProgress, &ws)
        .unwrap();
    (tmp, mgr)
}

fn failing_output(stdout: &str, stderr: &str) -> Output {
    Output {
        status: ExitStatus::from_raw(1 << 8),
        stdout: stdout.as_bytes().to_vec(),
        stderr: stderr.as_bytes().to_vec(),
    }
}

#[test]
fn done_emits_hook_step_and_log_events_in_order() {
    let (_tmp, mgr) = hook_workspace("logs", "build");
    let sink = CollectingSink::new();
    let ctx = RequestContext::new(&sink, snapshot());
    let runner = runner_with_successes(Some("compiling\nlinking\n"));
    let req = DoneRequest {
        name: "logs".into(),
        no_merge: true,
        no_clean: true,
        ..Default::default()
    };
    done_workspace(&mgr, &runner, &ctx, &req).unwrap();

    let events = sink.events();
    let hook_events: Vec<_> = events
        .iter()
        .filter(|e| {
            matches!(
                e,
                ServiceEvent::StepStarted(_)
                    | ServiceEvent::Log(_)
                    | ServiceEvent::StepFinished { .. }
            )
        })
        .cloned()
        .collect();
    assert_eq!(
        hook_events,
        vec![
            ServiceEvent::StepStarted("pre_done".into()),
            ServiceEvent::Log("[pre_done] compiling".into()),
            ServiceEvent::Log("[pre_done] linking".into()),
            ServiceEvent::StepFinished {
                name: "pre_done".into(),
                ok: true
            },
        ]
    );
}

#[test]
fn failing_hook_still_aborts_without_force_and_carries_full_stderr() {
    let (_tmp, mgr) = hook_workspace("fail", "build");
    let sink = CollectingSink::new();
    let ctx = RequestContext::new(&sink, snapshot());
    let runner = MockRunner::new();
    runner.push_response(failing_output("partial\n", "line a\nline b\n"));
    let req = DoneRequest {
        name: "fail".into(),
        no_merge: true,
        no_clean: true,
        ..Default::default()
    };
    let err = done_workspace(&mgr, &runner, &ctx, &req).unwrap_err();
    let text = format!("{err:#}");
    assert!(text.contains("pre_done hook failed (use --force to proceed anyway)"));
    assert!(text.contains("line a\nline b"));
    assert_eq!(
        mgr.load_workspace("fail").unwrap().0,
        WorkspaceStatus::InProgress
    );
    let events = sink.events();
    assert!(events.contains(&ServiceEvent::Log("[pre_done] partial".into())));
    assert!(events.contains(&ServiceEvent::StepFinished {
        name: "pre_done".into(),
        ok: false
    }));
}

#[test]
fn failing_hook_with_force_warns_and_continues() {
    let (_tmp, mgr) = hook_workspace("forced", "build");
    let sink = CollectingSink::new();
    let ctx = RequestContext::new(&sink, snapshot());
    let runner = runner_with_successes(None);
    runner
        .responses
        .borrow_mut()
        .insert(0, failing_output("", "bad\n"));
    let req = DoneRequest {
        name: "forced".into(),
        no_merge: true,
        no_clean: true,
        force: true,
        ..Default::default()
    };
    done_workspace(&mgr, &runner, &ctx, &req).unwrap();
    assert!(sink
        .events()
        .iter()
        .any(|e| matches!(e, ServiceEvent::Warning(m) if m.contains("pre_done hook failed"))));
    assert_eq!(
        mgr.load_workspace("forced").unwrap().0,
        WorkspaceStatus::Done
    );
}

#[test]
fn snapshot_without_path_keeps_the_host_path_but_strips_other_whitelisted_vars() {
    use zootree::runner::{CommandRunner, CommandSpec};
    use zootree::service::EnvRunner;

    let inner = MockRunner::new();
    inner.push_response(output(""));
    // A snapshot that carries SSH_AUTH_SOCK but no PATH.
    let env = EnvSnapshot::from_pairs([("SSH_AUTH_SOCK".to_string(), "/tmp/caller.sock".into())]);
    let runner = EnvRunner::new(&inner, &env);
    runner
        .run(&CommandSpec {
            program: "git".into(),
            args: vec![],
            cwd: None,
            env: Default::default(),
            env_remove: vec![],
        })
        .unwrap();
    let call = &inner.take_calls()[0];
    assert!(
        !call.env_remove.iter().any(|n| n == "PATH"),
        "PATH must stay inherited: {:?}",
        call.env_remove
    );
    assert!(!call.env.contains_key("PATH"));
    assert_eq!(
        call.env.get("SSH_AUTH_SOCK").map(String::as_str),
        Some("/tmp/caller.sock")
    );
    // Everything else on the whitelist is removed, never taken from the Daemon.
    for name in [
        "ZELLIJ",
        "ZELLIJ_SESSION_NAME",
        "HERDR_ENV",
        "SSH_AGENT_PID",
    ] {
        assert!(call.env_remove.iter().any(|n| n == name), "{name}");
    }
    // Variables outside the whitelist are never touched.
    assert!(!call.env_remove.iter().any(|n| n == "HOME"));
}

#[test]
fn request_context_builds_the_caller_context_from_the_snapshot() {
    use zootree::core::terminal_environment::CallerContext;

    let sink = CollectingSink::new();
    let inside = EnvSnapshot::from_pairs([("ZELLIJ".to_string(), "0".to_string())]);
    let ctx = RequestContext::new(&sink, inside);
    assert_eq!(
        ctx.caller(),
        CallerContext::from_vars(|n| (n == "ZELLIJ").then(|| "0".to_string()))
    );
    let outside = RequestContext::new(&sink, EnvSnapshot::default());
    assert_eq!(outside.caller(), CallerContext::from_vars(|_| None));
    assert_ne!(ctx.caller(), outside.caller());
}
