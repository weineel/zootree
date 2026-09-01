use std::collections::HashMap;
use std::os::unix::process::ExitStatusExt;
use std::process::{ExitStatus, Output};
use zootree::config::global::{HookValue, MultiplexerConfig};
use zootree::config::workspace::{
    StoredTerminalEnvironmentState, WorkspaceConfig, WorkspaceStatus,
};
use zootree::core::hook::{
    HookEngine, HookInvocation, HookOperation, HookStage, RepositoryHookContext,
};
use zootree::runner::MockRunner;

fn success_output() -> Output {
    Output {
        status: ExitStatus::from_raw(0),
        stdout: Vec::new(),
        stderr: Vec::new(),
    }
}

fn workspace() -> WorkspaceConfig {
    WorkspaceConfig {
        title: "Fix authentication".into(),
        name: "calm-river".into(),
        description: "Keep login sessions stable".into(),
        branch: "zootree/calm-river".into(),
        workspace_dir: "/home/user/ws/calm-river".into(),
        created_at: "2026-09-01T10:00:00+08:00".into(),
        agent_cli: None,
        multiplexer: MultiplexerConfig::default(),
        multiplexer_state: StoredTerminalEnvironmentState::default(),
        repos: Vec::new(),
        events: Vec::new(),
    }
}

#[test]
fn repository_hook_receives_deterministic_invocation_environment() {
    let runner = MockRunner::new();
    runner.push_response(success_output());
    let engine = HookEngine::new(&runner);
    let workspace = workspace();
    let repo_hook = HookValue::Simple("repo-hook".into());
    let global_hook = HookValue::Simple("global-hook".into());
    let invocation = HookInvocation::for_repository(
        Some(&repo_hook),
        Some(&global_hook),
        HookStage::PostCreate,
        HookOperation::Start,
        WorkspaceStatus::Pending,
        &workspace,
        RepositoryHookContext {
            name: "frontend",
            source_dir: "/home/user/projects/frontend",
            worktree_path: "/home/user/ws/calm-river/frontend",
            target_branch: Some("develop"),
        },
    )
    .expect("repo hook should be selected");

    engine.execute(&invocation).unwrap();

    let calls = runner.take_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].program, "sh");
    assert_eq!(calls[0].args, vec!["-c", "repo-hook"]);
    assert_eq!(
        calls[0].env,
        HashMap::from([
            ("ZOOTREE_HOOK".into(), "post_create".into()),
            ("ZOOTREE_OPERATION".into(), "start".into()),
            ("ZOOTREE_HOOK_SCOPE".into(), "repo".into()),
            ("ZOOTREE_HOOK_CONFIG_SCOPE".into(), "repo".into()),
            ("ZOOTREE_WORKSPACE".into(), "calm-river".into()),
            (
                "ZOOTREE_WORKSPACE_TITLE".into(),
                "Fix authentication".into(),
            ),
            (
                "ZOOTREE_WORKSPACE_DESCRIPTION".into(),
                "Keep login sessions stable".into(),
            ),
            ("ZOOTREE_WORKSPACE_STATUS".into(), "pending".into()),
            (
                "ZOOTREE_WORKSPACE_DIR".into(),
                "/home/user/ws/calm-river".into(),
            ),
            ("ZOOTREE_BRANCH".into(), "zootree/calm-river".into()),
            ("ZOOTREE_VERSION".into(), env!("CARGO_PKG_VERSION").into()),
            ("ZOOTREE_REPO".into(), "frontend".into()),
            (
                "ZOOTREE_REPO_SOURCE_DIR".into(),
                "/home/user/projects/frontend".into(),
            ),
            (
                "ZOOTREE_WORKTREE_PATH".into(),
                "/home/user/ws/calm-river/frontend".into(),
            ),
            ("ZOOTREE_TARGET_BRANCH".into(), "develop".into()),
        ])
    );
    assert_eq!(
        calls[0].cwd.as_deref(),
        Some("/home/user/ws/calm-river/frontend")
    );
}

#[test]
fn workspace_hook_omits_repository_context_and_clears_every_official_variable() {
    let runner = MockRunner::new();
    runner.push_response(success_output());
    let engine = HookEngine::new(&runner);
    let workspace = workspace();
    let hook = HookValue::File {
        file: "~/.config/zootree/hooks/cleanup.sh".into(),
    };
    let invocation = HookInvocation::for_workspace(
        Some(&hook),
        HookStage::PreDone,
        HookOperation::Done,
        WorkspaceStatus::InProgress,
        &workspace,
    )
    .expect("workspace hook should be selected");

    engine.execute(&invocation).unwrap();

    let calls = runner.take_calls();
    let call = &calls[0];
    assert_eq!(call.program, "sh");
    assert!(call.args[0].ends_with("/.config/zootree/hooks/cleanup.sh"));
    assert_eq!(call.cwd.as_deref(), Some("/home/user/ws/calm-river"));
    assert_eq!(
        call.env.get("ZOOTREE_HOOK_SCOPE").map(String::as_str),
        Some("workspace")
    );
    assert!(!call.env.contains_key("ZOOTREE_REPO"));
    assert!(!call.env.contains_key("ZOOTREE_REPO_SOURCE_DIR"));
    assert!(!call.env.contains_key("ZOOTREE_WORKTREE_PATH"));
    assert!(!call.env.contains_key("ZOOTREE_TARGET_BRANCH"));
    assert_eq!(
        call.env_remove,
        vec![
            "ZOOTREE_HOOK",
            "ZOOTREE_OPERATION",
            "ZOOTREE_HOOK_SCOPE",
            "ZOOTREE_HOOK_CONFIG_SCOPE",
            "ZOOTREE_WORKSPACE",
            "ZOOTREE_WORKSPACE_TITLE",
            "ZOOTREE_WORKSPACE_DESCRIPTION",
            "ZOOTREE_WORKSPACE_STATUS",
            "ZOOTREE_WORKSPACE_DIR",
            "ZOOTREE_BRANCH",
            "ZOOTREE_VERSION",
            "ZOOTREE_REPO",
            "ZOOTREE_REPO_SOURCE_DIR",
            "ZOOTREE_WORKTREE_PATH",
            "ZOOTREE_TARGET_BRANCH",
        ]
    );
}

#[test]
fn repository_hook_uses_global_fallback_and_marks_its_configuration_scope() {
    let runner = MockRunner::new();
    runner.push_response(success_output());
    let engine = HookEngine::new(&runner);
    let workspace = workspace();
    let global_hook = HookValue::Simple("global-hook".into());
    let invocation = HookInvocation::for_repository(
        None,
        Some(&global_hook),
        HookStage::PreRemove,
        HookOperation::Cancel,
        WorkspaceStatus::InProgress,
        &workspace,
        RepositoryHookContext {
            name: "frontend",
            source_dir: "/home/user/projects/frontend",
            worktree_path: "/home/user/ws/calm-river/frontend",
            target_branch: None,
        },
    )
    .expect("global fallback should be selected");

    engine.execute(&invocation).unwrap();

    let calls = runner.take_calls();
    assert_eq!(calls[0].args, vec!["-c", "global-hook"]);
    assert_eq!(
        calls[0]
            .env
            .get("ZOOTREE_HOOK_CONFIG_SCOPE")
            .map(String::as_str),
        Some("global")
    );
    assert!(!calls[0].env.contains_key("ZOOTREE_TARGET_BRANCH"));
}

#[test]
fn invocation_is_absent_when_no_hook_is_configured() {
    let workspace = workspace();

    let invocation = HookInvocation::for_repository(
        None,
        None,
        HookStage::PostCreate,
        HookOperation::AddRepo,
        WorkspaceStatus::InProgress,
        &workspace,
        RepositoryHookContext {
            name: "frontend",
            source_dir: "/home/user/projects/frontend",
            worktree_path: "/home/user/ws/calm-river/frontend",
            target_branch: Some("develop"),
        },
    );

    assert!(invocation.is_none());
}

#[test]
fn inline_hook_preserves_the_configured_script() {
    let runner = MockRunner::new();
    runner.push_response(success_output());
    let engine = HookEngine::new(&runner);
    let workspace = workspace();
    let script = "npm install\nnpm run db:migrate";
    let hook = HookValue::Inline {
        inline: script.into(),
    };
    let invocation = HookInvocation::for_workspace(
        Some(&hook),
        HookStage::PostStart,
        HookOperation::Start,
        WorkspaceStatus::InProgress,
        &workspace,
    )
    .unwrap();

    engine.execute(&invocation).unwrap();

    assert_eq!(runner.take_calls()[0].args, vec!["-c", script]);
}

#[test]
fn hook_failure_returns_stderr() {
    let runner = MockRunner::new();
    runner.push_response(Output {
        status: ExitStatus::from_raw(256),
        stdout: Vec::new(),
        stderr: b"command not found".to_vec(),
    });
    let engine = HookEngine::new(&runner);
    let workspace = workspace();
    let hook = HookValue::Simple("bad-command".into());
    let invocation = HookInvocation::for_workspace(
        Some(&hook),
        HookStage::PreCancel,
        HookOperation::Cancel,
        WorkspaceStatus::InProgress,
        &workspace,
    )
    .unwrap();

    let error = engine.execute(&invocation).unwrap_err();

    assert!(error.to_string().contains("command not found"));
}

#[test]
fn invocation_matrix_reports_exact_stage_operation_scope_and_status() {
    #[derive(Clone, Copy)]
    enum InvocationScope {
        Workspace,
        Repository,
    }

    let cases = [
        (
            HookStage::PostCreate,
            HookOperation::Start,
            InvocationScope::Repository,
            WorkspaceStatus::Pending,
        ),
        (
            HookStage::PostStart,
            HookOperation::Start,
            InvocationScope::Workspace,
            WorkspaceStatus::InProgress,
        ),
        (
            HookStage::PostCreate,
            HookOperation::Reopen,
            InvocationScope::Repository,
            WorkspaceStatus::Canceled,
        ),
        (
            HookStage::PostStart,
            HookOperation::Reopen,
            InvocationScope::Workspace,
            WorkspaceStatus::InProgress,
        ),
        (
            HookStage::PostCreate,
            HookOperation::AddRepo,
            InvocationScope::Repository,
            WorkspaceStatus::InProgress,
        ),
        (
            HookStage::PreDone,
            HookOperation::Done,
            InvocationScope::Workspace,
            WorkspaceStatus::InProgress,
        ),
        (
            HookStage::PreRemove,
            HookOperation::Done,
            InvocationScope::Repository,
            WorkspaceStatus::InProgress,
        ),
        (
            HookStage::PreCancel,
            HookOperation::Cancel,
            InvocationScope::Workspace,
            WorkspaceStatus::InProgress,
        ),
        (
            HookStage::PreRemove,
            HookOperation::Cancel,
            InvocationScope::Repository,
            WorkspaceStatus::InProgress,
        ),
    ];
    let runner = MockRunner::new();
    let engine = HookEngine::new(&runner);
    let workspace = workspace();
    let hook = HookValue::Simple("true".into());

    for (stage, operation, scope, workspace_status) in cases {
        runner.push_response(success_output());
        let invocation = match scope {
            InvocationScope::Repository => HookInvocation::for_repository(
                Some(&hook),
                None,
                stage,
                operation,
                workspace_status,
                &workspace,
                RepositoryHookContext {
                    name: "frontend",
                    source_dir: "/home/user/projects/frontend",
                    worktree_path: "/home/user/ws/calm-river/frontend",
                    target_branch: Some("develop"),
                },
            ),
            InvocationScope::Workspace => HookInvocation::for_workspace(
                Some(&hook),
                stage,
                operation,
                workspace_status,
                &workspace,
            ),
        }
        .unwrap();
        engine.execute(&invocation).unwrap();
    }

    let actual = runner
        .take_calls()
        .into_iter()
        .map(|call| {
            (
                call.env["ZOOTREE_HOOK"].clone(),
                call.env["ZOOTREE_OPERATION"].clone(),
                call.env["ZOOTREE_HOOK_SCOPE"].clone(),
                call.env["ZOOTREE_WORKSPACE_STATUS"].clone(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        actual,
        vec![
            (
                "post_create".into(),
                "start".into(),
                "repo".into(),
                "pending".into()
            ),
            (
                "post_start".into(),
                "start".into(),
                "workspace".into(),
                "in_progress".into(),
            ),
            (
                "post_create".into(),
                "reopen".into(),
                "repo".into(),
                "canceled".into(),
            ),
            (
                "post_start".into(),
                "reopen".into(),
                "workspace".into(),
                "in_progress".into(),
            ),
            (
                "post_create".into(),
                "add-repo".into(),
                "repo".into(),
                "in_progress".into(),
            ),
            (
                "pre_done".into(),
                "done".into(),
                "workspace".into(),
                "in_progress".into(),
            ),
            (
                "pre_remove".into(),
                "done".into(),
                "repo".into(),
                "in_progress".into(),
            ),
            (
                "pre_cancel".into(),
                "cancel".into(),
                "workspace".into(),
                "in_progress".into(),
            ),
            (
                "pre_remove".into(),
                "cancel".into(),
                "repo".into(),
                "in_progress".into(),
            ),
        ]
    );
}
