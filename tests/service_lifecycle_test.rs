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
