use clap::Parser;
use std::os::unix::process::ExitStatusExt;
use std::process::{ExitStatus, Output};
use tempfile::TempDir;
use zootree::cli::{Cli, Commands};
use zootree::config::global::{GlobalConfig, MultiplexerConfig, MultiplexerKind};
use zootree::config::repo::RepoConfig;
use zootree::config::workspace::{
    RepoEntry, StoredTerminalEnvironmentState, WorkspaceConfig, WorkspaceStatus,
};
use zootree::config::ConfigManager;
use zootree::core::reopen::{
    build_reopen_plan, execute_reopen_plan, format_reopen_plan, NonInteractiveReopenPrompt,
    ReopenBase, ReopenLifecyclePlan, ReopenOptions, ReopenSources, TaskBranchSource,
    WorktreeAction,
};
use zootree::runner::MockRunner;

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

fn archived_workspace(workspace_dir: &str) -> WorkspaceConfig {
    WorkspaceConfig {
        title: "Continue calm river".into(),
        name: "calm-river".into(),
        description: String::new(),
        branch: "zootree/calm-river".into(),
        workspace_dir: workspace_dir.into(),
        created_at: "2026-08-25T10:00:00+08:00".into(),
        agent_cli: None,
        multiplexer: MultiplexerConfig::default(),
        multiplexer_state: Default::default(),
        repos: vec![RepoEntry {
            name: "frontend".into(),
            target_branch: Some("develop".into()),
        }],
        events: Vec::new(),
    }
}

fn setup_archived_workspace() -> (TempDir, ConfigManager, WorkspaceConfig) {
    let tmp = TempDir::new().unwrap();
    let manager = ConfigManager::with_base_dir(tmp.path().join("config"));
    manager.ensure_dirs().unwrap();
    manager
        .save_repo_config(
            "frontend",
            &RepoConfig {
                path: "/repos/frontend".into(),
                default_target_branch: Some("develop".into()),
                copy_files: Vec::new(),
                hooks: Default::default(),
                lazygit: None,
            },
        )
        .unwrap();
    let workspace = archived_workspace(&tmp.path().join("calm-river").to_string_lossy());
    manager
        .save_workspace(&WorkspaceStatus::Canceled, &workspace)
        .unwrap();
    (tmp, manager, workspace)
}

fn stored_terminal_state(source: &str) -> StoredTerminalEnvironmentState {
    toml::from_str(source).unwrap()
}

#[test]
fn reopen_cli_accepts_recovery_and_lifecycle_options() {
    let cli = Cli::parse_from([
        "zootree",
        "reopen",
        "calm-river",
        "--from",
        "current",
        "--from",
        "frontend:develop",
        "--overwrite",
        "frontend",
        "--skip-hooks",
        "--dry-run",
        "--no-multiplexer",
        "--run-agent",
        "codex",
    ]);

    let Commands::Reopen(args) = cli.command else {
        panic!("expected reopen command");
    };
    assert_eq!(args.name.as_deref(), Some("calm-river"));
    assert_eq!(args.from, vec!["current", "frontend:develop"]);
    assert_eq!(args.overwrite, vec!["frontend"]);
    assert!(args.skip_hooks);
    assert!(args.dry_run);
    assert!(args.no_multiplexer);
    assert_eq!(args.run_agent, Some(Some("codex".into())));
}

#[test]
fn reopen_sources_apply_repo_override_over_current_default() {
    let sources = ReopenSources::parse(&[
        "current".into(),
        "frontend:develop".into(),
        "backend:release/2026".into(),
    ])
    .unwrap();

    assert_eq!(
        sources.for_repo("frontend"),
        Some(ReopenBase::Branch("develop".into()))
    );
    assert_eq!(
        sources.for_repo("backend"),
        Some(ReopenBase::Branch("release/2026".into()))
    );
    assert_eq!(sources.for_repo("docs"), Some(ReopenBase::Current));
}

#[test]
fn reopen_plan_replaces_archived_terminal_snapshot_with_current_global_config() {
    let (_tmp, manager, mut workspace) = setup_archived_workspace();
    workspace.multiplexer.kind = MultiplexerKind::Cmux;
    workspace.multiplexer_state = stored_terminal_state(
        r#"
version = 1
adapter = "cmux"

[payload]
group = "workspace_group:old"
"#,
    );
    manager
        .save_workspace(&WorkspaceStatus::Canceled, &workspace)
        .unwrap();
    let runner = MockRunner::new();
    runner.push_response(success_stdout("refs/heads/zootree/calm-river\n"));
    runner.push_response(success_stdout(
        "worktree /repos/frontend\0HEAD 1111111\0branch refs/heads/main\0",
    ));
    let mut prompt = NonInteractiveReopenPrompt;
    let mut plan = build_reopen_plan(
        &manager,
        &runner,
        "calm-river",
        &ReopenOptions::default(),
        &mut prompt,
    )
    .unwrap();
    let mut global = GlobalConfig::default();
    global.multiplexer.kind = MultiplexerKind::Herdr;
    global.multiplexer.herdr.session = "current-session".into();

    plan.apply_current_terminal_config(&global);

    assert_eq!(plan.workspace.multiplexer, global.multiplexer);
    assert!(plan.workspace.multiplexer_state.is_empty());
    let output = format_reopen_plan(&plan, &ReopenLifecyclePlan::default());
    assert!(
        output.contains("terminal config: current global config (herdr)"),
        "{output}"
    );
}

#[test]
fn reopen_plan_rejects_source_for_repo_outside_workspace_before_git_checks() {
    let (_tmp, manager, _) = setup_archived_workspace();
    let runner = MockRunner::new();
    let mut prompt = NonInteractiveReopenPrompt;
    let options = ReopenOptions {
        sources: ReopenSources::parse(&["backend:main".into()]).unwrap(),
        ..Default::default()
    };

    let error =
        build_reopen_plan(&manager, &runner, "calm-river", &options, &mut prompt).unwrap_err();

    assert!(error
        .to_string()
        .contains("--from references repo 'backend' outside workspace 'calm-river'"));
    assert!(runner.take_calls().is_empty());
}

#[test]
fn reopen_plan_creates_missing_worktree_from_existing_local_task_branch() {
    let (_tmp, manager, workspace) = setup_archived_workspace();
    let runner = MockRunner::new();
    runner.push_response(success_stdout("refs/heads/zootree/calm-river\n"));
    runner.push_response(success_stdout(
        "worktree /repos/frontend\0HEAD 1111111\0branch refs/heads/main\0",
    ));
    let mut prompt = NonInteractiveReopenPrompt;

    let plan = build_reopen_plan(
        &manager,
        &runner,
        "calm-river",
        &ReopenOptions::default(),
        &mut prompt,
    )
    .unwrap();

    assert_eq!(plan.from_status, WorkspaceStatus::Canceled);
    assert_eq!(plan.workspace, workspace);
    assert_eq!(plan.repos.len(), 1);
    assert_eq!(plan.repos[0].branch_source, TaskBranchSource::Local);
    assert_eq!(plan.repos[0].worktree_action, WorktreeAction::Create);
    assert_eq!(
        plan.repos[0].worktree_path,
        plan.workspace.workspace_dir.to_owned() + "/frontend"
    );
    assert_eq!(
        manager.load_workspace("calm-river").unwrap().0,
        WorkspaceStatus::Canceled
    );
}

#[test]
fn reopen_plan_prefers_origin_task_branch_when_local_branch_is_missing() {
    let (_tmp, manager, _) = setup_archived_workspace();
    let runner = MockRunner::new();
    runner.push_response(success_stdout(""));
    runner.push_response(success_stdout(
        "refs/remotes/upstream/zootree/calm-river\nrefs/remotes/origin/zootree/calm-river\n",
    ));
    runner.push_response(success_stdout(
        "worktree /repos/frontend\0HEAD 1111111\0branch refs/heads/main\0",
    ));
    let mut prompt = NonInteractiveReopenPrompt;

    let plan = build_reopen_plan(
        &manager,
        &runner,
        "calm-river",
        &ReopenOptions::default(),
        &mut prompt,
    )
    .unwrap();

    assert_eq!(
        plan.repos[0].branch_source,
        TaskBranchSource::Remote("origin/zootree/calm-river".into())
    );
}

#[test]
fn reopen_plan_recreates_missing_task_branch_from_each_repo_current_branch() {
    let (_tmp, manager, _) = setup_archived_workspace();
    let runner = MockRunner::new();
    runner.push_response(success_stdout(""));
    runner.push_response(success_stdout(""));
    runner.push_response(success_stdout("feature/current\n"));
    runner.push_response(success_stdout(
        "worktree /repos/frontend\0HEAD 1111111\0branch refs/heads/feature/current\0",
    ));
    let mut prompt = NonInteractiveReopenPrompt;
    let options = ReopenOptions {
        sources: ReopenSources::parse(&["current".into()]).unwrap(),
        ..Default::default()
    };

    let plan = build_reopen_plan(&manager, &runner, "calm-river", &options, &mut prompt).unwrap();

    assert_eq!(
        plan.repos[0].branch_source,
        TaskBranchSource::Base {
            revision: "feature/current".into(),
            display: "feature/current".into(),
        }
    );
}

#[test]
fn explicit_reopen_base_does_not_change_original_target_branch() {
    let (_tmp, manager, _) = setup_archived_workspace();
    let runner = MockRunner::new();
    runner.push_response(success_stdout(""));
    runner.push_response(success_stdout(""));
    runner.push_response(success_stdout("refs/heads/release/2026\n"));
    runner.push_response(success_stdout(
        "worktree /repos/frontend\0HEAD 1111111\0branch refs/heads/main\0",
    ));
    let mut prompt = NonInteractiveReopenPrompt;
    let options = ReopenOptions {
        sources: ReopenSources::parse(&["frontend:release/2026".into()]).unwrap(),
        ..Default::default()
    };

    let plan = build_reopen_plan(&manager, &runner, "calm-river", &options, &mut prompt).unwrap();

    assert_eq!(
        plan.repos[0].branch_source,
        TaskBranchSource::Base {
            revision: "release/2026".into(),
            display: "release/2026".into(),
        }
    );
    assert_eq!(plan.repos[0].target_branch.as_deref(), Some("develop"));
    assert_eq!(
        plan.workspace.repos[0].target_branch.as_deref(),
        Some("develop")
    );
}

#[test]
fn current_reopen_base_supports_detached_head_with_visible_commit() {
    let (_tmp, manager, _) = setup_archived_workspace();
    let runner = MockRunner::new();
    runner.push_response(success_stdout(""));
    runner.push_response(success_stdout(""));
    runner.push_response(success_stdout("HEAD\n"));
    runner.push_response(success_stdout("abc1234\n"));
    runner.push_response(success_stdout(
        "worktree /repos/frontend\0HEAD abc1234\0detached\0",
    ));
    let mut prompt = NonInteractiveReopenPrompt;
    let options = ReopenOptions {
        sources: ReopenSources::parse(&["current".into()]).unwrap(),
        ..Default::default()
    };

    let plan = build_reopen_plan(&manager, &runner, "calm-river", &options, &mut prompt).unwrap();

    assert_eq!(
        plan.repos[0].branch_source,
        TaskBranchSource::Base {
            revision: "HEAD".into(),
            display: "HEAD at abc1234".into(),
        }
    );
}

#[test]
fn reopen_execution_restores_worktree_then_moves_workspace_to_in_progress() {
    let (_tmp, manager, mut archived) = setup_archived_workspace();
    archived.multiplexer.kind = MultiplexerKind::Cmux;
    archived.multiplexer_state = stored_terminal_state(
        r#"
version = 1
adapter = "cmux"

[payload]
group = "workspace_group:old"
"#,
    );
    manager
        .save_workspace(&WorkspaceStatus::Canceled, &archived)
        .unwrap();
    let runner = MockRunner::new();
    runner.push_response(success_stdout("refs/heads/zootree/calm-river\n"));
    runner.push_response(success_stdout(
        "worktree /repos/frontend\0HEAD 1111111\0branch refs/heads/main\0",
    ));
    let mut prompt = NonInteractiveReopenPrompt;
    let plan = build_reopen_plan(
        &manager,
        &runner,
        "calm-river",
        &ReopenOptions::default(),
        &mut prompt,
    )
    .unwrap();
    runner.push_response(success_stdout(""));
    let mut global = GlobalConfig::default();
    global.multiplexer.kind = MultiplexerKind::Herdr;
    global.multiplexer.herdr.session = "current-session".into();

    let reopened = execute_reopen_plan(&manager, &global, &runner, plan, true).unwrap();

    assert_eq!(reopened.name, "calm-river");
    assert_eq!(reopened.multiplexer, global.multiplexer);
    assert!(reopened.multiplexer_state.is_empty());
    let (status, persisted) = manager.load_workspace("calm-river").unwrap();
    assert_eq!(status, WorkspaceStatus::InProgress);
    assert_eq!(persisted.multiplexer, global.multiplexer);
    assert!(persisted.multiplexer_state.is_empty());
    assert_eq!(
        persisted.events.last().unwrap().action,
        "reopened".to_string()
    );
    assert_eq!(
        persisted.events.last().unwrap().detail.as_deref(),
        Some("from canceled")
    );
    let calls = runner.take_calls();
    assert_eq!(
        calls.last().unwrap().args,
        vec![
            "-C".to_string(),
            "/repos/frontend".to_string(),
            "worktree".to_string(),
            "add".to_string(),
            persisted.workspace_dir.to_owned() + "/frontend",
            "zootree/calm-river".to_string(),
        ]
    );
}

#[test]
fn reopen_moves_done_workspace_to_in_progress_and_reuses_matching_worktree() {
    let (_tmp, manager, workspace) = setup_archived_workspace();
    std::fs::remove_file(
        manager
            .base_dir
            .join("workspaces/archived/canceled/calm-river.toml"),
    )
    .unwrap();
    manager
        .save_workspace(&WorkspaceStatus::Done, &workspace)
        .unwrap();
    let worktree_path = std::path::Path::new(&workspace.workspace_dir).join("frontend");
    std::fs::create_dir_all(&worktree_path).unwrap();
    let runner = MockRunner::new();
    runner.push_response(success_stdout("refs/heads/zootree/calm-river\n"));
    runner.push_response(success_stdout(&format!(
        "worktree /repos/frontend\0HEAD 1111111\0branch refs/heads/main\0\0worktree {}\0HEAD 2222222\0branch refs/heads/zootree/calm-river\0",
        worktree_path.display()
    )));
    let mut prompt = NonInteractiveReopenPrompt;
    let plan = build_reopen_plan(
        &manager,
        &runner,
        "calm-river",
        &ReopenOptions::default(),
        &mut prompt,
    )
    .unwrap();

    let reopened =
        execute_reopen_plan(&manager, &GlobalConfig::default(), &runner, plan, true).unwrap();

    assert_eq!(
        reopened.events.last().unwrap().detail.as_deref(),
        Some("from done")
    );
    assert_eq!(
        manager.load_workspace("calm-river").unwrap().0,
        WorkspaceStatus::InProgress
    );
    assert_eq!(runner.take_calls().len(), 2);
}

#[test]
fn reopen_syncs_workspace_instruction_indexes_when_hooks_are_skipped() {
    let (_tmp, manager, workspace) = setup_archived_workspace();
    let worktree_path = std::path::Path::new(&workspace.workspace_dir).join("frontend");
    std::fs::create_dir_all(&worktree_path).unwrap();
    std::fs::write(worktree_path.join("AGENTS.md"), "frontend rules").unwrap();
    let runner = MockRunner::new();
    runner.push_response(success_stdout("refs/heads/zootree/calm-river\n"));
    runner.push_response(success_stdout(&format!(
        "worktree /repos/frontend\0HEAD 1111111\0branch refs/heads/main\0\0worktree {}\0HEAD 2222222\0branch refs/heads/zootree/calm-river\0",
        worktree_path.display()
    )));
    let mut prompt = NonInteractiveReopenPrompt;
    let plan = build_reopen_plan(
        &manager,
        &runner,
        "calm-river",
        &ReopenOptions::default(),
        &mut prompt,
    )
    .unwrap();

    execute_reopen_plan(&manager, &GlobalConfig::default(), &runner, plan, true).unwrap();

    assert_eq!(
        std::fs::read_to_string(std::path::Path::new(&workspace.workspace_dir).join("AGENTS.md"))
            .unwrap(),
        "# Workspace repository instructions\n\n\
- For work in `frontend/`, read and follow `frontend/AGENTS.md`.\n"
    );
}

#[test]
fn reopen_plan_output_describes_source_and_worktree_action() {
    let (_tmp, manager, _) = setup_archived_workspace();
    let runner = MockRunner::new();
    runner.push_response(success_stdout("refs/heads/zootree/calm-river\n"));
    runner.push_response(success_stdout(
        "worktree /repos/frontend\0HEAD 1111111\0branch refs/heads/main\0",
    ));
    let mut prompt = NonInteractiveReopenPrompt;
    let mut plan = build_reopen_plan(
        &manager,
        &runner,
        "calm-river",
        &ReopenOptions::default(),
        &mut prompt,
    )
    .unwrap();
    plan.repos[0].worktree_action = WorktreeAction::Overwrite { registered: true };
    let lifecycle = ReopenLifecyclePlan {
        skip_hooks: true,
        activate_terminal_environment: true,
        run_agent: true,
    };

    let output = format_reopen_plan(&plan, &lifecycle);

    assert!(
        output.contains("reopen 'calm-river' from canceled"),
        "{output}"
    );
    assert!(
        output.contains("frontend: overwrite registered worktree from local task branch"),
        "{output}"
    );
    assert!(output.contains("/frontend"), "{output}");
    assert!(
        output.contains("terminal before recovery: close before overwriting worktrees"),
        "{output}"
    );
    assert!(
        output.contains("worktree setup: copy files; skip post_create hooks"),
        "{output}"
    );
    assert!(
        output.contains("state: append reopened event and move canceled -> in_progress"),
        "{output}"
    );
    assert!(output.contains("post_start: skip"), "{output}");
    assert!(
        output.contains("terminal after recovery: activate with requested agent"),
        "{output}"
    );
}

#[test]
fn reopen_rejects_a_worktree_path_that_resolves_to_the_registered_source_repo() {
    let tmp = tempfile::tempdir().unwrap();
    let manager = ConfigManager::with_base_dir(tmp.path().join("config"));
    manager.ensure_dirs().unwrap();
    let real_workspace_dir = tmp.path().join("real-workspace");
    let source_repo = real_workspace_dir.join("frontend");
    std::fs::create_dir_all(&source_repo).unwrap();
    let linked_workspace_dir = tmp.path().join("linked-workspace");
    std::os::unix::fs::symlink(&real_workspace_dir, &linked_workspace_dir).unwrap();
    manager
        .save_repo_config(
            "frontend",
            &RepoConfig {
                path: source_repo.to_string_lossy().into_owned(),
                default_target_branch: Some("develop".into()),
                copy_files: Vec::new(),
                hooks: Default::default(),
                lazygit: None,
            },
        )
        .unwrap();
    let workspace = archived_workspace(&linked_workspace_dir.to_string_lossy());
    manager
        .save_workspace(&WorkspaceStatus::Canceled, &workspace)
        .unwrap();
    let runner = MockRunner::new();
    let mut prompt = NonInteractiveReopenPrompt;

    let error = build_reopen_plan(
        &manager,
        &runner,
        "calm-river",
        &ReopenOptions::default(),
        &mut prompt,
    )
    .unwrap_err();

    assert!(error.to_string().contains("registered source path"));
    assert!(runner.take_calls().is_empty());
}

#[test]
fn noninteractive_reopen_reuses_matching_worktree_without_overwrite_flag() {
    let (_tmp, manager, workspace) = setup_archived_workspace();
    let worktree_path = std::path::Path::new(&workspace.workspace_dir).join("frontend");
    std::fs::create_dir_all(&worktree_path).unwrap();
    let runner = MockRunner::new();
    runner.push_response(success_stdout("refs/heads/zootree/calm-river\n"));
    runner.push_response(success_stdout(&format!(
        "worktree /repos/frontend\0HEAD 1111111\0branch refs/heads/main\0\0worktree {}\0HEAD 2222222\0branch refs/heads/zootree/calm-river\0",
        worktree_path.display()
    )));
    let mut prompt = NonInteractiveReopenPrompt;

    let plan = build_reopen_plan(
        &manager,
        &runner,
        "calm-river",
        &ReopenOptions::default(),
        &mut prompt,
    )
    .unwrap();
    assert_eq!(plan.repos[0].worktree_action, WorktreeAction::Reuse);
}

#[test]
fn reopen_rejects_task_branch_checked_out_by_another_worktree() {
    let (_tmp, manager, _) = setup_archived_workspace();
    let runner = MockRunner::new();
    runner.push_response(success_stdout("refs/heads/zootree/calm-river\n"));
    runner.push_response(success_stdout(
        "worktree /repos/frontend\0HEAD 1111111\0branch refs/heads/main\0\0worktree /tmp/other\0HEAD 2222222\0branch refs/heads/zootree/calm-river\0",
    ));
    let mut prompt = NonInteractiveReopenPrompt;

    let error = build_reopen_plan(
        &manager,
        &runner,
        "calm-river",
        &ReopenOptions::default(),
        &mut prompt,
    )
    .unwrap_err();

    let message = error.to_string();
    assert!(message.contains("checked out at '/tmp/other'"), "{message}");
    assert_eq!(
        manager.load_workspace("calm-river").unwrap().0,
        WorkspaceStatus::Canceled
    );
}

#[test]
fn overwrite_removes_only_canonical_symlink_not_its_target() {
    let (tmp, manager, workspace) = setup_archived_workspace();
    let workspace_dir = std::path::Path::new(&workspace.workspace_dir);
    std::fs::create_dir_all(workspace_dir).unwrap();
    let external = tmp.path().join("external");
    std::fs::create_dir_all(&external).unwrap();
    std::fs::write(external.join("keep.txt"), "keep").unwrap();
    let worktree_path = workspace_dir.join("frontend");
    std::os::unix::fs::symlink(&external, &worktree_path).unwrap();
    let runner = MockRunner::new();
    runner.push_response(success_stdout("refs/heads/zootree/calm-river\n"));
    runner.push_response(success_stdout(
        "worktree /repos/frontend\0HEAD 1111111\0branch refs/heads/main\0",
    ));
    let mut prompt = NonInteractiveReopenPrompt;
    let options = ReopenOptions {
        overwrite_repos: ["frontend".to_string()].into_iter().collect(),
        ..Default::default()
    };
    let plan = build_reopen_plan(&manager, &runner, "calm-river", &options, &mut prompt).unwrap();
    runner.push_response(success_stdout(""));

    execute_reopen_plan(&manager, &GlobalConfig::default(), &runner, plan, true).unwrap();

    assert_eq!(
        std::fs::read_to_string(external.join("keep.txt")).unwrap(),
        "keep"
    );
    assert!(std::fs::symlink_metadata(worktree_path).is_err());
}

#[test]
fn post_create_failure_keeps_workspace_archived_and_rolls_back_created_worktree() {
    let (_tmp, manager, _) = setup_archived_workspace();
    let mut repo = manager.load_repo_config("frontend").unwrap();
    repo.hooks.post_create = Some(zootree::config::global::HookValue::Simple(
        "setup frontend".into(),
    ));
    manager.save_repo_config("frontend", &repo).unwrap();
    let runner = MockRunner::new();
    runner.push_response(success_stdout("refs/heads/zootree/calm-river\n"));
    runner.push_response(success_stdout(
        "worktree /repos/frontend\0HEAD 1111111\0branch refs/heads/main\0",
    ));
    let mut prompt = NonInteractiveReopenPrompt;
    let plan = build_reopen_plan(
        &manager,
        &runner,
        "calm-river",
        &ReopenOptions::default(),
        &mut prompt,
    )
    .unwrap();
    runner.push_response(success_stdout(""));
    runner.push_response(failure_output("setup failed"));
    runner.push_response(success_stdout(""));

    let error =
        execute_reopen_plan(&manager, &GlobalConfig::default(), &runner, plan, false).unwrap_err();

    assert!(format!("{error:#}").contains("setup failed"));
    let (status, persisted) = manager.load_workspace("calm-river").unwrap();
    assert_eq!(status, WorkspaceStatus::Canceled);
    assert!(persisted.events.is_empty());
    let calls = runner.take_calls();
    let hook = calls.iter().find(|call| call.program == "sh").unwrap();
    assert_eq!(
        hook.env.get("ZOOTREE_HOOK").map(String::as_str),
        Some("post_create")
    );
    assert_eq!(
        hook.env.get("ZOOTREE_OPERATION").map(String::as_str),
        Some("reopen")
    );
    assert_eq!(
        hook.env.get("ZOOTREE_WORKSPACE_STATUS").map(String::as_str),
        Some("canceled")
    );
    assert_eq!(
        calls.last().unwrap().args,
        vec![
            "-C".to_string(),
            "/repos/frontend".to_string(),
            "worktree".to_string(),
            "remove".to_string(),
            "--force".to_string(),
            persisted.workspace_dir.to_owned() + "/frontend",
        ]
    );
}

#[test]
fn post_start_failure_is_partial_success_after_reopen_transition() {
    let (_tmp, manager, _) = setup_archived_workspace();
    let runner = MockRunner::new();
    runner.push_response(success_stdout("refs/heads/zootree/calm-river\n"));
    runner.push_response(success_stdout(
        "worktree /repos/frontend\0HEAD 1111111\0branch refs/heads/main\0",
    ));
    let mut prompt = NonInteractiveReopenPrompt;
    let plan = build_reopen_plan(
        &manager,
        &runner,
        "calm-river",
        &ReopenOptions::default(),
        &mut prompt,
    )
    .unwrap();
    let mut global = GlobalConfig::default();
    global.hooks.post_start = Some(zootree::config::global::HookValue::Simple(
        "prepare workspace".into(),
    ));
    runner.push_response(success_stdout(""));
    runner.push_response(failure_output("prepare failed"));

    let error = execute_reopen_plan(&manager, &global, &runner, plan, false).unwrap_err();

    assert!(format!("{error:#}").contains(
        "workspace 'calm-river' reopened and remains in_progress, but post_start hook failed"
    ));
    let (status, persisted) = manager.load_workspace("calm-river").unwrap();
    assert_eq!(status, WorkspaceStatus::InProgress);
    assert_eq!(persisted.events.last().unwrap().action, "reopened");
    let calls = runner.take_calls();
    assert_eq!(calls.len(), 4);
    let hook = calls.iter().find(|call| call.program == "sh").unwrap();
    assert_eq!(
        hook.env.get("ZOOTREE_HOOK").map(String::as_str),
        Some("post_start")
    );
    assert_eq!(
        hook.env.get("ZOOTREE_OPERATION").map(String::as_str),
        Some("reopen")
    );
    assert_eq!(
        hook.env.get("ZOOTREE_WORKSPACE_STATUS").map(String::as_str),
        Some("in_progress")
    );
}

#[test]
fn state_move_failure_restores_archived_config_without_reopened_event() {
    let (_tmp, manager, _) = setup_archived_workspace();
    let runner = MockRunner::new();
    runner.push_response(success_stdout("refs/heads/zootree/calm-river\n"));
    runner.push_response(success_stdout(
        "worktree /repos/frontend\0HEAD 1111111\0branch refs/heads/main\0",
    ));
    let mut prompt = NonInteractiveReopenPrompt;
    let mut plan = build_reopen_plan(
        &manager,
        &runner,
        "calm-river",
        &ReopenOptions::default(),
        &mut prompt,
    )
    .unwrap();
    plan.workspace.agent_cli = Some("codex".into());
    let destination = manager
        .base_dir
        .join("workspaces/in_progress/calm-river.toml");
    std::fs::create_dir(&destination).unwrap();
    std::fs::write(destination.join("block"), "block rename").unwrap();
    runner.push_response(success_stdout(""));
    runner.push_response(success_stdout(""));

    let error =
        execute_reopen_plan(&manager, &GlobalConfig::default(), &runner, plan, true).unwrap_err();

    assert!(format!("{error:#}").contains("Is a directory"));
    let archived_path = manager
        .base_dir
        .join("workspaces/archived/canceled/calm-river.toml");
    let persisted: WorkspaceConfig =
        toml::from_str(&std::fs::read_to_string(archived_path).unwrap()).unwrap();
    assert!(persisted.events.is_empty());
    assert!(persisted.agent_cli.is_none());
}
