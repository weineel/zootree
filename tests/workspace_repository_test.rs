use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::ExitStatusExt;
use std::process::{ExitStatus, Output};

use tempfile::TempDir;
use zootree::config::global::{
    GlobalConfig, HookValue, HooksConfig, MultiplexerConfig, MultiplexerKind,
};
use zootree::config::repo::RepoConfig;
use zootree::config::template::TemplateConfig;
use zootree::config::workspace::{RepoEntry, WorkspaceConfig, WorkspaceStatus};
use zootree::config::ConfigManager;
use zootree::core::workspace_repository::{add, AddRepositoryRequest, TerminalUpdate};
use zootree::runner::{CommandRunner, CommandSpec, MockRunner};

struct CreatePathOnWorktreeAddRunner {
    inner: MockRunner,
    worktree_path: std::path::PathBuf,
}

impl CommandRunner for CreatePathOnWorktreeAddRunner {
    fn run(&self, spec: &CommandSpec) -> anyhow::Result<Output> {
        if spec.program == "git"
            && spec.args.get(2).map(String::as_str) == Some("worktree")
            && spec.args.get(3).map(String::as_str) == Some("add")
        {
            std::fs::create_dir_all(&self.worktree_path)?;
        }
        self.inner.run(spec)
    }

    fn run_interactive(&self, spec: &CommandSpec) -> anyhow::Result<ExitStatus> {
        self.inner.run_interactive(spec)
    }
}

struct AssertIndexBeforeTerminalRunner {
    inner: CreatePathOnWorktreeAddRunner,
    index_path: std::path::PathBuf,
    expected_index: String,
}

impl CommandRunner for AssertIndexBeforeTerminalRunner {
    fn run(&self, spec: &CommandSpec) -> anyhow::Result<Output> {
        if spec.program == "zellij" && spec.args.iter().any(|arg| arg == "new-tab") {
            let actual = std::fs::read_to_string(&self.index_path)?;
            anyhow::ensure!(
                actual == self.expected_index,
                "workspace instruction index was not synchronized before terminal mutation"
            );
        }
        self.inner.run(spec)
    }

    fn run_interactive(&self, spec: &CommandSpec) -> anyhow::Result<ExitStatus> {
        self.inner.run_interactive(spec)
    }
}

struct FailTerminalStatePersistenceRunner {
    inner: MockRunner,
    config_dir: std::path::PathBuf,
}

impl CommandRunner for FailTerminalStatePersistenceRunner {
    fn run(&self, spec: &CommandSpec) -> anyhow::Result<Output> {
        let is_cleanup = (spec.program == "zellij"
            && spec.args.iter().any(|arg| arg == "close-tab"))
            || (spec.program == "cmux" && spec.args.get(1).map(String::as_str) == Some("close"));
        if is_cleanup {
            std::fs::set_permissions(&self.config_dir, std::fs::Permissions::from_mode(0o755))?;
        }

        let output = self.inner.run(spec)?;
        let is_addition = (spec.program == "zellij"
            && spec.args.iter().any(|arg| arg == "new-tab"))
            || (spec.program == "cmux" && spec.args.get(1).map(String::as_str) == Some("create"));
        if is_addition && output.status.success() {
            std::fs::set_permissions(&self.config_dir, std::fs::Permissions::from_mode(0o555))?;
        }
        Ok(output)
    }

    fn run_interactive(&self, spec: &CommandSpec) -> anyhow::Result<ExitStatus> {
        self.inner.run_interactive(spec)
    }
}

fn output(status: i32, stdout: &[u8], stderr: &[u8]) -> Output {
    Output {
        status: ExitStatus::from_raw(status << 8),
        stdout: stdout.to_vec(),
        stderr: stderr.to_vec(),
    }
}

fn success(stdout: &[u8]) -> Output {
    output(0, stdout, b"")
}

fn failure(stderr: &[u8]) -> Output {
    output(1, b"", stderr)
}

fn setup() -> (TempDir, ConfigManager, WorkspaceConfig) {
    let temp = TempDir::new().unwrap();
    let config_manager = ConfigManager::with_base_dir(temp.path().join("config"));
    config_manager.ensure_dirs().unwrap();
    let workspace_dir = temp.path().join("workspaces/calm-river");
    std::fs::create_dir_all(&workspace_dir).unwrap();
    let workspace = WorkspaceConfig {
        title: "Add backend".into(),
        name: "calm-river".into(),
        description: String::new(),
        branch: "zootree/calm-river".into(),
        workspace_dir: workspace_dir.to_string_lossy().into_owned(),
        created_at: "2026-08-25T10:00:00+08:00".into(),
        agent_cli: None,
        multiplexer: MultiplexerConfig::default(),
        multiplexer_state: Default::default(),
        repos: vec![RepoEntry {
            name: "frontend".into(),
            target_branch: Some("main".into()),
        }],
        events: Vec::new(),
    };
    config_manager
        .save_workspace(&WorkspaceStatus::InProgress, &workspace)
        .unwrap();
    config_manager
        .save_repo_config(
            "backend",
            &RepoConfig {
                path: "/repos/backend".into(),
                default_target_branch: Some("main".into()),
                copy_files: Vec::new(),
                hooks: HooksConfig::default(),
                lazygit: None,
            },
        )
        .unwrap();
    (temp, config_manager, workspace)
}

#[test]
fn add_persists_membership_and_event_when_terminal_environment_is_absent() {
    let (_temp, config_manager, workspace) = setup();
    let mut global = GlobalConfig::default();
    global.hooks.post_start = Some(HookValue::Simple("must-not-run".into()));
    let runner = MockRunner::new();
    runner.push_response(success(b"refs/heads/main\n"));
    runner.push_response(success(b""));
    runner.push_response(success(b""));
    runner.push_response(success(b""));

    let result = add(
        &config_manager,
        &global,
        &runner,
        &AddRepositoryRequest {
            workspace: workspace.name.clone(),
            repo: "backend".into(),
            target_branch: None,
        },
    )
    .unwrap();

    assert_eq!(result.repo, "backend");
    assert_eq!(result.target_branch, "main");
    assert_eq!(result.workspace_branch, "zootree/calm-river");
    assert_eq!(result.terminal, TerminalUpdate::Absent);
    let (status, saved) = config_manager.load_workspace("calm-river").unwrap();
    assert_eq!(status, WorkspaceStatus::InProgress);
    assert_eq!(saved.repos.len(), 2);
    assert_eq!(saved.repos[1].name, "backend");
    assert_eq!(saved.repos[1].target_branch.as_deref(), Some("main"));
    assert_eq!(saved.events.len(), 1);
    assert_eq!(saved.events[0].action, "repo_added");
    assert_eq!(
        saved.events[0].detail.as_deref(),
        Some("repo=backend, target_branch=main")
    );
    assert!(saved.multiplexer_state.is_empty());

    let calls = runner.take_calls();
    assert_eq!(calls.len(), 4);
    assert_eq!(
        calls[3].args,
        vec![
            "-C",
            "/repos/backend",
            "worktree",
            "add",
            "-b",
            "zootree/calm-river",
            result.worktree_path.as_str(),
            "main"
        ]
    );
    assert!(calls.iter().all(|call| call.program != "sh"));
}

#[test]
fn add_syncs_workspace_instruction_indexes_after_membership_commit() {
    let (temp, config_manager, workspace) = setup();
    let workspace_dir = std::path::Path::new(&workspace.workspace_dir);
    std::fs::create_dir_all(workspace_dir.join("frontend")).unwrap();
    std::fs::write(workspace_dir.join("frontend/AGENTS.md"), "frontend rules").unwrap();
    let backend_source = temp.path().join("backend-source");
    std::fs::create_dir_all(&backend_source).unwrap();
    std::fs::write(backend_source.join("AGENTS.md"), "backend rules").unwrap();
    let mut backend = config_manager.load_repo_config("backend").unwrap();
    backend.path = backend_source.to_string_lossy().into_owned();
    backend.copy_files = vec!["AGENTS.md".into()];
    config_manager
        .save_repo_config("backend", &backend)
        .unwrap();
    let expected_index = "# Workspace repository instructions\n\n\
- For work in `frontend/`, read and follow `frontend/AGENTS.md`.\n\
- For work in `backend/`, read and follow `backend/AGENTS.md`.\n";
    let runner = AssertIndexBeforeTerminalRunner {
        inner: CreatePathOnWorktreeAddRunner {
            inner: MockRunner::new(),
            worktree_path: workspace_dir.join("backend"),
        },
        index_path: workspace_dir.join("AGENTS.md"),
        expected_index: expected_index.into(),
    };
    runner
        .inner
        .inner
        .push_response(success(b"refs/heads/main\n"));
    runner.inner.inner.push_response(success(b""));
    runner
        .inner
        .inner
        .push_response(success(b"zootree-calm-river [Created 1m ago]\n"));
    runner
        .inner
        .inner
        .push_response(success(b"frontend\noverview\n"));
    runner.inner.inner.push_response(success(b""));
    runner.inner.inner.push_response(success(b"7\n"));
    runner.inner.inner.push_response(success(b""));
    runner.inner.inner.push_response(success(b""));

    add(
        &config_manager,
        &GlobalConfig::default(),
        &runner,
        &AddRepositoryRequest {
            workspace: workspace.name,
            repo: "backend".into(),
            target_branch: None,
        },
    )
    .unwrap();

    assert_eq!(
        std::fs::read_to_string(workspace_dir.join("AGENTS.md")).unwrap(),
        expected_index
    );
}

#[test]
fn add_restores_membership_and_indexes_when_terminal_mutation_fails() {
    let (temp, config_manager, workspace) = setup();
    let workspace_dir = std::path::Path::new(&workspace.workspace_dir);
    std::fs::create_dir_all(workspace_dir.join("frontend")).unwrap();
    std::fs::write(workspace_dir.join("frontend/AGENTS.md"), "frontend rules").unwrap();
    let backend_source = temp.path().join("backend-source");
    std::fs::create_dir_all(&backend_source).unwrap();
    std::fs::write(backend_source.join("AGENTS.md"), "backend rules").unwrap();
    let mut backend = config_manager.load_repo_config("backend").unwrap();
    backend.path = backend_source.to_string_lossy().into_owned();
    backend.copy_files = vec!["AGENTS.md".into()];
    config_manager
        .save_repo_config("backend", &backend)
        .unwrap();
    let runner = CreatePathOnWorktreeAddRunner {
        inner: MockRunner::new(),
        worktree_path: workspace_dir.join("backend"),
    };
    runner.inner.push_response(success(b"refs/heads/main\n"));
    runner.inner.push_response(success(b""));
    runner
        .inner
        .push_response(success(b"zootree-calm-river [Created 1m ago]\n"));
    runner.inner.push_response(success(b"frontend\noverview\n"));
    runner.inner.push_response(success(b""));
    runner
        .inner
        .push_response(failure(b"terminal mutation failed"));
    runner.inner.push_response(success(b""));
    runner.inner.push_response(success(b""));

    let error = add(
        &config_manager,
        &GlobalConfig::default(),
        &runner,
        &AddRepositoryRequest {
            workspace: workspace.name,
            repo: "backend".into(),
            target_branch: None,
        },
    )
    .unwrap_err();

    assert!(error.to_string().contains("terminal mutation failed"));
    let (_, saved) = config_manager.load_workspace("calm-river").unwrap();
    assert_eq!(saved.repos.len(), 1);
    assert!(saved.events.is_empty());
    assert_eq!(
        std::fs::read_to_string(workspace_dir.join("AGENTS.md")).unwrap(),
        "# Workspace repository instructions\n\n\
- For work in `frontend/`, read and follow `frontend/AGENTS.md`.\n"
    );
}

#[test]
fn add_preserves_unknown_terminal_state_and_the_recently_template_when_absent() {
    let (_temp, config_manager, mut workspace) = setup();
    workspace.multiplexer_state = toml::from_str(
        r#"
version = 99
adapter = "zellij"
future_field = "keep-me"
"#,
    )
    .unwrap();
    let original_state = workspace.multiplexer_state.clone();
    config_manager
        .save_workspace(&WorkspaceStatus::InProgress, &workspace)
        .unwrap();
    let recently = TemplateConfig {
        repos: vec!["frontend".into()],
        multiplexer: workspace.multiplexer.clone(),
    };
    config_manager.save_template("recently", &recently).unwrap();
    let runner = MockRunner::new();
    runner.push_response(success(b"refs/heads/main\n"));
    runner.push_response(success(b""));
    runner.push_response(success(b""));
    runner.push_response(success(b""));

    let result = add(
        &config_manager,
        &GlobalConfig::default(),
        &runner,
        &AddRepositoryRequest {
            workspace: workspace.name,
            repo: "backend".into(),
            target_branch: None,
        },
    )
    .unwrap();

    assert_eq!(result.terminal, TerminalUpdate::Absent);
    let (_, saved) = config_manager.load_workspace("calm-river").unwrap();
    assert_eq!(saved.multiplexer_state, original_state);
    assert_eq!(config_manager.load_template("recently").unwrap(), recently);
}

#[test]
fn add_prefers_repo_post_create_and_passes_the_full_hook_invocation() {
    let (_temp, config_manager, workspace) = setup();
    let mut repo = config_manager.load_repo_config("backend").unwrap();
    repo.hooks.post_create = Some(HookValue::Simple("repo-hook".into()));
    config_manager.save_repo_config("backend", &repo).unwrap();
    let mut global = GlobalConfig::default();
    global.hooks.post_create = Some(HookValue::Simple("global-hook".into()));
    let runner = MockRunner::new();
    runner.push_response(success(b"refs/heads/main\n"));
    runner.push_response(success(b""));
    runner.push_response(success(b""));
    runner.push_response(success(b""));
    runner.push_response(success(b""));

    let result = add(
        &config_manager,
        &global,
        &runner,
        &AddRepositoryRequest {
            workspace: workspace.name.clone(),
            repo: "backend".into(),
            target_branch: None,
        },
    )
    .unwrap();

    let calls = runner.take_calls();
    let hooks = calls
        .iter()
        .filter(|call| call.program == "sh")
        .collect::<Vec<_>>();
    assert_eq!(hooks.len(), 1);
    assert_eq!(hooks[0].args, vec!["-c", "repo-hook"]);
    assert_eq!(hooks[0].cwd.as_deref(), Some(result.worktree_path.as_str()));
    assert_eq!(
        hooks[0].env.get("ZOOTREE_HOOK").map(String::as_str),
        Some("post_create")
    );
    assert_eq!(
        hooks[0].env.get("ZOOTREE_OPERATION").map(String::as_str),
        Some("add-repo")
    );
    assert_eq!(
        hooks[0]
            .env
            .get("ZOOTREE_HOOK_CONFIG_SCOPE")
            .map(String::as_str),
        Some("repo")
    );
    assert_eq!(
        hooks[0]
            .env
            .get("ZOOTREE_WORKSPACE_STATUS")
            .map(String::as_str),
        Some("in_progress")
    );
    assert_eq!(
        hooks[0]
            .env
            .get("ZOOTREE_REPO_SOURCE_DIR")
            .map(String::as_str),
        Some("/repos/backend")
    );
    assert_eq!(
        hooks[0].env.get("ZOOTREE_WORKSPACE").map(String::as_str),
        Some("calm-river")
    );
    assert_eq!(
        hooks[0].env.get("ZOOTREE_REPO").map(String::as_str),
        Some("backend")
    );
    assert_eq!(
        hooks[0].env.get("ZOOTREE_BRANCH").map(String::as_str),
        Some("zootree/calm-river")
    );
    assert_eq!(
        hooks[0]
            .env
            .get("ZOOTREE_TARGET_BRANCH")
            .map(String::as_str),
        Some("main")
    );
    assert_eq!(
        hooks[0]
            .env
            .get("ZOOTREE_WORKTREE_PATH")
            .map(String::as_str),
        Some(result.worktree_path.as_str())
    );
    assert_eq!(
        hooks[0]
            .env
            .get("ZOOTREE_WORKSPACE_DIR")
            .map(String::as_str),
        Some(workspace.workspace_dir.as_str())
    );
}

#[test]
fn add_uses_global_post_create_when_the_repo_hook_is_absent() {
    let (_temp, config_manager, workspace) = setup();
    let mut global = GlobalConfig::default();
    global.hooks.post_create = Some(HookValue::Simple("global-hook".into()));
    let runner = MockRunner::new();
    runner.push_response(success(b"refs/heads/main\n"));
    runner.push_response(success(b""));
    runner.push_response(success(b""));
    runner.push_response(success(b""));
    runner.push_response(success(b""));

    add(
        &config_manager,
        &global,
        &runner,
        &AddRepositoryRequest {
            workspace: workspace.name,
            repo: "backend".into(),
            target_branch: None,
        },
    )
    .unwrap();

    let calls = runner.take_calls();
    let hooks = calls
        .iter()
        .filter(|call| call.program == "sh")
        .collect::<Vec<_>>();
    assert_eq!(hooks.len(), 1);
    assert_eq!(hooks[0].args, vec!["-c", "global-hook"]);
    assert_eq!(
        hooks[0]
            .env
            .get("ZOOTREE_HOOK_CONFIG_SCOPE")
            .map(String::as_str),
        Some("global")
    );
}

#[test]
fn add_rejects_a_workspace_outside_in_progress_without_running_commands() {
    let (_temp, config_manager, workspace) = setup();
    config_manager
        .move_workspace(
            &workspace.name,
            &WorkspaceStatus::InProgress,
            &WorkspaceStatus::Pending,
        )
        .unwrap();
    let runner = MockRunner::new();

    let error = add(
        &config_manager,
        &GlobalConfig::default(),
        &runner,
        &AddRepositoryRequest {
            workspace: workspace.name,
            repo: "backend".into(),
            target_branch: None,
        },
    )
    .unwrap_err();

    assert!(error.to_string().contains("is not in_progress"));
    assert!(runner.take_calls().is_empty());
}

#[test]
fn add_rejects_an_unregistered_repository_without_running_commands() {
    let (_temp, config_manager, workspace) = setup();
    config_manager.remove_repo_config("backend").unwrap();
    let runner = MockRunner::new();

    let error = add(
        &config_manager,
        &GlobalConfig::default(),
        &runner,
        &AddRepositoryRequest {
            workspace: workspace.name,
            repo: "backend".into(),
            target_branch: None,
        },
    )
    .unwrap_err();

    assert!(error.to_string().contains("could not be loaded"));
    assert!(runner.take_calls().is_empty());
}

#[test]
fn add_rolls_back_git_when_copy_files_fails() {
    let (_temp, config_manager, workspace) = setup();
    let mut repo = config_manager.load_repo_config("backend").unwrap();
    repo.copy_files = vec!["[".into()];
    config_manager.save_repo_config("backend", &repo).unwrap();
    let runner = MockRunner::new();
    runner.push_response(success(b"refs/heads/main\n"));
    runner.push_response(success(b""));
    runner.push_response(success(b""));
    runner.push_response(success(b""));
    runner.push_response(success(b""));
    runner.push_response(success(b""));

    let error = add(
        &config_manager,
        &GlobalConfig::default(),
        &runner,
        &AddRepositoryRequest {
            workspace: workspace.name,
            repo: "backend".into(),
            target_branch: None,
        },
    )
    .unwrap_err();

    assert!(error.to_string().contains("invalid copy_files pattern"));
    let calls = runner.take_calls();
    assert_eq!(calls[4].args[2..5], ["worktree", "remove", "--force"]);
    assert_eq!(calls[5].args[2..5], ["branch", "-D", "zootree/calm-river"]);
}

#[test]
fn add_rejects_a_missing_target_branch_before_terminal_or_worktree_mutation() {
    let (_temp, config_manager, workspace) = setup();
    let runner = MockRunner::new();
    runner.push_response(success(b""));

    let error = add(
        &config_manager,
        &GlobalConfig::default(),
        &runner,
        &AddRepositoryRequest {
            workspace: workspace.name,
            repo: "backend".into(),
            target_branch: None,
        },
    )
    .unwrap_err();

    assert!(error.to_string().contains("does not exist locally"));
    assert_eq!(runner.take_calls().len(), 1);
}

#[test]
fn add_rejects_an_existing_workspace_branch_before_terminal_or_worktree_mutation() {
    let (_temp, config_manager, workspace) = setup();
    let runner = MockRunner::new();
    runner.push_response(success(b"refs/heads/main\n"));
    runner.push_response(success(b"refs/heads/zootree/calm-river\n"));

    let error = add(
        &config_manager,
        &GlobalConfig::default(),
        &runner,
        &AddRepositoryRequest {
            workspace: workspace.name,
            repo: "backend".into(),
            target_branch: None,
        },
    )
    .unwrap_err();

    assert!(error.to_string().contains("refusing to adopt it"));
    assert_eq!(runner.take_calls().len(), 2);
}

#[test]
fn add_rejects_an_existing_worktree_path_before_terminal_mutation() {
    let (_temp, config_manager, workspace) = setup();
    std::fs::create_dir_all(std::path::Path::new(&workspace.workspace_dir).join("backend"))
        .unwrap();
    let runner = MockRunner::new();
    runner.push_response(success(b"refs/heads/main\n"));
    runner.push_response(success(b""));

    let error = add(
        &config_manager,
        &GlobalConfig::default(),
        &runner,
        &AddRepositoryRequest {
            workspace: workspace.name,
            repo: "backend".into(),
            target_branch: None,
        },
    )
    .unwrap_err();

    assert!(error
        .to_string()
        .contains("refusing to adopt or replace it"));
    assert_eq!(runner.take_calls().len(), 2);
}

#[test]
fn add_rolls_back_owned_git_artifacts_when_post_create_fails() {
    let (_temp, config_manager, workspace) = setup();
    let mut repo = config_manager.load_repo_config("backend").unwrap();
    repo.hooks.post_create = Some(HookValue::Simple("false".into()));
    config_manager.save_repo_config("backend", &repo).unwrap();
    let runner = MockRunner::new();
    runner.push_response(success(b"refs/heads/main\n"));
    runner.push_response(success(b""));
    runner.push_response(success(b""));
    runner.push_response(success(b""));
    runner.push_response(failure(b"setup failed"));
    runner.push_response(success(b""));
    runner.push_response(success(b""));

    let error = add(
        &config_manager,
        &GlobalConfig::default(),
        &runner,
        &AddRepositoryRequest {
            workspace: workspace.name,
            repo: "backend".into(),
            target_branch: None,
        },
    )
    .unwrap_err();

    assert!(error.to_string().contains("setup failed"));
    let (_, saved) = config_manager.load_workspace("calm-river").unwrap();
    assert_eq!(saved.repos.len(), 1);
    assert!(saved.events.is_empty());
    let calls = runner.take_calls();
    assert_eq!(
        calls[5].args[2..],
        ["worktree", "remove", "--force", calls[5].args[5].as_str()]
    );
    assert_eq!(
        calls[6].args,
        vec!["-C", "/repos/backend", "branch", "-D", "zootree/calm-river"]
    );
}

#[test]
fn add_appends_and_focuses_one_zellij_tab_without_attaching() {
    let (_temp, config_manager, workspace) = setup();
    let runner = MockRunner::new();
    runner.push_response(success(b"refs/heads/main\n"));
    runner.push_response(success(b""));
    runner.push_response(success(b"zootree-calm-river [Created 1m ago]\n"));
    runner.push_response(success(b"frontend\noverview\n"));
    runner.push_response(success(b""));
    runner.push_response(success(b"7\n"));

    let result = add(
        &config_manager,
        &GlobalConfig::default(),
        &runner,
        &AddRepositoryRequest {
            workspace: workspace.name,
            repo: "backend".into(),
            target_branch: None,
        },
    )
    .unwrap();

    assert_eq!(result.terminal, TerminalUpdate::Updated);
    let (_, saved) = config_manager.load_workspace("calm-river").unwrap();
    let state = toml::Value::try_from(saved.multiplexer_state).unwrap();
    assert_eq!(state["adapter"].as_str(), Some("zellij"));
    assert_eq!(
        state["payload"]["session"].as_str(),
        Some("zootree-calm-river")
    );
    let calls = runner.take_calls();
    assert_eq!(
        calls[3].args,
        vec![
            "--session",
            "zootree-calm-river",
            "action",
            "query-tab-names"
        ]
    );
    assert_eq!(
        calls[5].args[..5],
        [
            "--session",
            "zootree-calm-river",
            "action",
            "new-tab",
            "--layout"
        ]
    );
    assert!(calls
        .iter()
        .all(|call| !call.args.iter().any(|arg| arg == "attach")));
}

#[test]
fn add_rejects_an_existing_zellij_tab_before_creating_git_artifacts() {
    let (_temp, config_manager, workspace) = setup();
    let runner = MockRunner::new();
    runner.push_response(success(b"refs/heads/main\n"));
    runner.push_response(success(b""));
    runner.push_response(success(b"zootree-calm-river\n"));
    runner.push_response(success(b"overview\nbackend\n"));

    let error = add(
        &config_manager,
        &GlobalConfig::default(),
        &runner,
        &AddRepositoryRequest {
            workspace: workspace.name,
            repo: "backend".into(),
            target_branch: None,
        },
    )
    .unwrap_err();

    assert!(error.to_string().contains("already contains a tab"));
    assert_eq!(runner.take_calls().len(), 4);
}

#[test]
fn add_rejects_terminal_inspection_failure_before_creating_git_artifacts() {
    let (_temp, config_manager, workspace) = setup();
    let runner = MockRunner::new();
    runner.push_response(success(b"refs/heads/main\n"));
    runner.push_response(success(b""));
    runner.push_response(failure(b"zellij unavailable"));

    let error = add(
        &config_manager,
        &GlobalConfig::default(),
        &runner,
        &AddRepositoryRequest {
            workspace: workspace.name,
            repo: "backend".into(),
            target_branch: None,
        },
    )
    .unwrap_err();

    assert!(error.to_string().contains("zellij unavailable"));
    assert_eq!(runner.take_calls().len(), 3);
}

#[test]
fn add_rejects_an_incremental_zellij_layout_without_one_marker_before_git() {
    let (_temp, config_manager, mut workspace) = setup();
    workspace.multiplexer.zellij.layout = Some("custom".into());
    config_manager
        .save_workspace(&WorkspaceStatus::InProgress, &workspace)
        .unwrap();
    std::fs::write(
        config_manager.base_dir.join("layouts/custom.kdl"),
        "layout { tab name=\"overview\" {} }",
    )
    .unwrap();
    let runner = MockRunner::new();
    runner.push_response(success(b"refs/heads/main\n"));
    runner.push_response(success(b""));
    runner.push_response(success(b"zootree-calm-river\n"));
    runner.push_response(success(b"overview\nfrontend\n"));

    let error = add(
        &config_manager,
        &GlobalConfig::default(),
        &runner,
        &AddRepositoryRequest {
            workspace: workspace.name,
            repo: "backend".into(),
            target_branch: None,
        },
    )
    .unwrap_err();

    assert!(error.to_string().contains("exactly one"));
    assert_eq!(runner.take_calls().len(), 4);
}

#[test]
fn add_rolls_back_zellij_tab_and_git_when_terminal_state_persistence_fails() {
    let (temp, config_manager, workspace) = setup();
    let config_path = temp
        .path()
        .join("config/workspaces/in_progress/calm-river.toml");
    let original_config = std::fs::read(&config_path).unwrap();
    let config_dir = config_path.parent().unwrap();
    let runner = FailTerminalStatePersistenceRunner {
        inner: MockRunner::new(),
        config_dir: config_dir.to_path_buf(),
    };
    runner.inner.push_response(success(b"refs/heads/main\n"));
    runner.inner.push_response(success(b""));
    runner.inner.push_response(success(b"zootree-calm-river\n"));
    runner.inner.push_response(success(b"overview\nfrontend\n"));
    runner.inner.push_response(success(b""));
    runner.inner.push_response(success(b"9\n"));
    runner.inner.push_response(success(b""));
    runner.inner.push_response(success(b""));
    runner.inner.push_response(success(b""));

    let error = add(
        &config_manager,
        &GlobalConfig::default(),
        &runner,
        &AddRepositoryRequest {
            workspace: workspace.name,
            repo: "backend".into(),
            target_branch: None,
        },
    )
    .unwrap_err();

    std::fs::set_permissions(config_dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(error.to_string().contains("persist terminal state"));
    assert_eq!(std::fs::read(&config_path).unwrap(), original_config);
    let calls = runner.inner.take_calls();
    assert_eq!(
        calls[6].args,
        vec![
            "--session",
            "zootree-calm-river",
            "action",
            "close-tab",
            "--tab-id",
            "9"
        ]
    );
    assert_eq!(calls[7].args[2..5], ["worktree", "remove", "--force"]);
    assert_eq!(calls[8].args[2..5], ["branch", "-D", "zootree/calm-river"]);
}

#[test]
fn add_aggregates_terminal_and_branch_cleanup_residue() {
    let (temp, config_manager, workspace) = setup();
    let config_path = temp
        .path()
        .join("config/workspaces/in_progress/calm-river.toml");
    let config_dir = config_path.parent().unwrap();
    let runner = FailTerminalStatePersistenceRunner {
        inner: MockRunner::new(),
        config_dir: config_dir.to_path_buf(),
    };
    runner.inner.push_response(success(b"refs/heads/main\n"));
    runner.inner.push_response(success(b""));
    runner.inner.push_response(success(b"zootree-calm-river\n"));
    runner.inner.push_response(success(b"overview\nfrontend\n"));
    runner.inner.push_response(success(b""));
    runner.inner.push_response(success(b"9\n"));
    runner.inner.push_response(failure(b"tab close failed"));
    runner.inner.push_response(success(b""));
    runner.inner.push_response(failure(b"branch delete failed"));

    let error = add(
        &config_manager,
        &GlobalConfig::default(),
        &runner,
        &AddRepositoryRequest {
            workspace: workspace.name,
            repo: "backend".into(),
            target_branch: None,
        },
    )
    .unwrap_err();

    std::fs::set_permissions(config_dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    let message = format!("{error:#}");
    assert!(message.contains("failed to persist terminal state"));
    assert!(message.contains("tab close failed"));
    assert!(message.contains("branch delete failed"));
    assert!(message.contains("rollback residue"));
    assert_eq!(runner.inner.take_calls().len(), 9);
}

#[test]
fn add_appends_one_cmux_workspace_to_the_existing_group() {
    let (_temp, config_manager, mut workspace) = setup();
    workspace.multiplexer.kind = MultiplexerKind::Cmux;
    workspace.multiplexer_state = toml::from_str(
        r#"
version = 1
adapter = "cmux"

[payload]
group = "workspace_group:2"

[[payload.repo_workspaces]]
repo = "frontend"
workspace = "workspace:4"
"#,
    )
    .unwrap();
    config_manager
        .save_workspace(&WorkspaceStatus::InProgress, &workspace)
        .unwrap();
    let runner = MockRunner::new();
    runner.push_response(success(b"refs/heads/main\n"));
    runner.push_response(success(b""));
    runner.push_response(success(
        br#"{"groups":[{"name":"Add backend","ref":"workspace_group:2"}]}"#,
    ));
    runner.push_response(success(
        br#"{"workspaces":[{"name":"zootree-calm-river-frontend","ref":"workspace:4"}]}"#,
    ));
    runner.push_response(success(b""));
    runner.push_response(success(b"workspace:9\n"));

    let result = add(
        &config_manager,
        &GlobalConfig::default(),
        &runner,
        &AddRepositoryRequest {
            workspace: workspace.name,
            repo: "backend".into(),
            target_branch: None,
        },
    )
    .unwrap();

    assert_eq!(result.terminal, TerminalUpdate::Updated);
    let (_, saved) = config_manager.load_workspace("calm-river").unwrap();
    let state = toml::Value::try_from(saved.multiplexer_state).unwrap();
    assert_eq!(
        state["payload"]["group"].as_str(),
        Some("workspace_group:2")
    );
    assert_eq!(
        state["payload"]["repo_workspaces"][1]["repo"].as_str(),
        Some("backend")
    );
    assert_eq!(
        state["payload"]["repo_workspaces"][1]["workspace"].as_str(),
        Some("workspace:9")
    );
    let calls = runner.take_calls();
    assert_eq!(
        calls[5].args[..4],
        [
            "workspace",
            "create",
            "--name",
            "zootree-calm-river-backend"
        ]
    );
    assert!(calls[5]
        .args
        .windows(2)
        .any(|args| args == ["--group", "workspace_group:2"]));
    assert!(calls[5]
        .args
        .windows(2)
        .any(|args| args == ["--group-placement", "end"]));
    assert!(calls[5]
        .args
        .windows(2)
        .any(|args| args == ["--focus", "true"]));
}

#[test]
fn add_rejects_an_existing_cmux_repo_workspace_before_git_mutation() {
    let (_temp, config_manager, mut workspace) = setup();
    workspace.multiplexer.kind = MultiplexerKind::Cmux;
    config_manager
        .save_workspace(&WorkspaceStatus::InProgress, &workspace)
        .unwrap();
    let runner = MockRunner::new();
    runner.push_response(success(b"refs/heads/main\n"));
    runner.push_response(success(b""));
    runner.push_response(success(
        br#"{"groups":[{"name":"Add backend","ref":"workspace_group:2"}]}"#,
    ));
    runner.push_response(success(
        br#"{"workspaces":[{"name":"zootree-calm-river-backend","ref":"workspace:9"}]}"#,
    ));

    let error = add(
        &config_manager,
        &GlobalConfig::default(),
        &runner,
        &AddRepositoryRequest {
            workspace: workspace.name,
            repo: "backend".into(),
            target_branch: None,
        },
    )
    .unwrap_err();

    assert!(error.to_string().contains("already contains a workspace"));
    assert_eq!(runner.take_calls().len(), 4);
}

#[test]
fn add_rejects_an_ambiguous_cmux_group_before_git_mutation() {
    let (_temp, config_manager, mut workspace) = setup();
    workspace.multiplexer.kind = MultiplexerKind::Cmux;
    config_manager
        .save_workspace(&WorkspaceStatus::InProgress, &workspace)
        .unwrap();
    let runner = MockRunner::new();
    runner.push_response(success(b"refs/heads/main\n"));
    runner.push_response(success(b""));
    runner.push_response(success(
        br#"{"groups":[{"name":"Add backend","ref":"workspace_group:2"},{"name":"Add backend","ref":"workspace_group:3"}]}"#,
    ));

    let error = add(
        &config_manager,
        &GlobalConfig::default(),
        &runner,
        &AddRepositoryRequest {
            workspace: workspace.name,
            repo: "backend".into(),
            target_branch: None,
        },
    )
    .unwrap_err();

    assert!(error.to_string().contains("ambiguous"));
    assert_eq!(runner.take_calls().len(), 3);
}

#[test]
fn add_skips_cmux_mutation_when_the_group_is_verified_absent() {
    let (_temp, config_manager, mut workspace) = setup();
    workspace.multiplexer.kind = MultiplexerKind::Cmux;
    config_manager
        .save_workspace(&WorkspaceStatus::InProgress, &workspace)
        .unwrap();
    let runner = MockRunner::new();
    runner.push_response(success(b"refs/heads/main\n"));
    runner.push_response(success(b""));
    runner.push_response(success(br#"{"groups":[]}"#));
    runner.push_response(success(b""));

    let result = add(
        &config_manager,
        &GlobalConfig::default(),
        &runner,
        &AddRepositoryRequest {
            workspace: workspace.name,
            repo: "backend".into(),
            target_branch: None,
        },
    )
    .unwrap();

    assert_eq!(result.terminal, TerminalUpdate::Absent);
    assert_eq!(runner.take_calls().len(), 4);
}

#[test]
fn add_rolls_back_cmux_workspace_when_terminal_state_persistence_fails() {
    let (temp, config_manager, mut workspace) = setup();
    workspace.multiplexer.kind = MultiplexerKind::Cmux;
    workspace.multiplexer_state = toml::from_str(
        r#"
version = 1
adapter = "cmux"

[payload]
group = "workspace_group:2"
"#,
    )
    .unwrap();
    config_manager
        .save_workspace(&WorkspaceStatus::InProgress, &workspace)
        .unwrap();
    let config_path = temp
        .path()
        .join("config/workspaces/in_progress/calm-river.toml");
    let original_config = std::fs::read(&config_path).unwrap();
    let config_dir = config_path.parent().unwrap();
    let runner = FailTerminalStatePersistenceRunner {
        inner: MockRunner::new(),
        config_dir: config_dir.to_path_buf(),
    };
    runner.inner.push_response(success(b"refs/heads/main\n"));
    runner.inner.push_response(success(b""));
    runner.inner.push_response(success(
        br#"{"groups":[{"name":"Add backend","ref":"workspace_group:2"}]}"#,
    ));
    runner.inner.push_response(success(br#"{"workspaces":[]}"#));
    runner.inner.push_response(success(b""));
    runner.inner.push_response(success(b"workspace:9\n"));
    runner.inner.push_response(success(b""));
    runner.inner.push_response(success(b""));
    runner.inner.push_response(success(b""));

    let error = add(
        &config_manager,
        &GlobalConfig::default(),
        &runner,
        &AddRepositoryRequest {
            workspace: workspace.name,
            repo: "backend".into(),
            target_branch: None,
        },
    )
    .unwrap_err();

    std::fs::set_permissions(config_dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(error.to_string().contains("persist terminal state"));
    assert_eq!(std::fs::read(&config_path).unwrap(), original_config);
    let calls = runner.inner.take_calls();
    assert_eq!(calls[6].args, vec!["workspace", "close", "workspace:9"]);
    assert_eq!(calls[7].args[2..5], ["worktree", "remove", "--force"]);
    assert_eq!(calls[8].args[2..5], ["branch", "-D", "zootree/calm-river"]);
}

#[test]
fn add_recovers_and_closes_cmux_workspace_when_create_omits_its_ref() {
    let (_temp, config_manager, mut workspace) = setup();
    workspace.multiplexer.kind = MultiplexerKind::Cmux;
    workspace.multiplexer_state = toml::from_str(
        r#"
version = 1
adapter = "cmux"

[payload]
group = "workspace_group:2"
"#,
    )
    .unwrap();
    config_manager
        .save_workspace(&WorkspaceStatus::InProgress, &workspace)
        .unwrap();
    let runner = MockRunner::new();
    runner.push_response(success(b"refs/heads/main\n"));
    runner.push_response(success(b""));
    runner.push_response(success(
        br#"{"groups":[{"name":"Add backend","ref":"workspace_group:2"}]}"#,
    ));
    runner.push_response(success(br#"{"workspaces":[]}"#));
    runner.push_response(success(b""));
    runner.push_response(success(b"created without ref\n"));
    runner.push_response(success(
        br#"{"workspaces":[{"name":"zootree-calm-river-backend","ref":"workspace:9"}]}"#,
    ));
    runner.push_response(success(b""));
    runner.push_response(success(b""));
    runner.push_response(success(b""));

    let error = add(
        &config_manager,
        &GlobalConfig::default(),
        &runner,
        &AddRepositoryRequest {
            workspace: workspace.name,
            repo: "backend".into(),
            target_branch: None,
        },
    )
    .unwrap_err();

    assert!(error.to_string().contains("did not return a workspace ref"));
    let calls = runner.take_calls();
    assert_eq!(calls[7].args, vec!["workspace", "close", "workspace:9"]);
    assert_eq!(calls[8].args[2..5], ["worktree", "remove", "--force"]);
    assert_eq!(calls[9].args[2..5], ["branch", "-D", "zootree/calm-river"]);
}

#[test]
fn add_appends_one_herdr_repo_tab_and_focuses_it() {
    let (_temp, config_manager, mut workspace) = setup();
    workspace.multiplexer.kind = MultiplexerKind::Herdr;
    workspace.multiplexer.herdr.session = "agents".into();
    workspace.multiplexer_state = toml::from_str(
        r#"
version = 1
adapter = "herdr"

[payload]
session = "agents"
workspace_id = "w7"
label = "Add backend · zootree:calm-river"
"#,
    )
    .unwrap();
    config_manager
        .save_workspace(&WorkspaceStatus::InProgress, &workspace)
        .unwrap();
    let runner = MockRunner::new();
    runner.push_response(success(b"refs/heads/main\n"));
    runner.push_response(success(b""));
    runner.push_response(success(b"herdr 0.8.2\n"));
    runner.push_response(success(
        r#"{"result":{"type":"workspace_info","workspace":{"workspace_id":"w7","label":"Add backend · zootree:calm-river"}}}"#
            .as_bytes(),
    ));
    runner.push_response(success(
        br#"{"result":{"type":"tab_list","tabs":[{"tab_id":"w7:t1","label":"overview"},{"tab_id":"w7:t2","label":"frontend"}]}}"#,
    ));
    runner.push_response(success(b""));
    runner.push_response(success(
        br#"{"result":{"type":"tab_created","tab":{"tab_id":"w7:t3"},"root_pane":{"pane_id":"w7:p5"}}}"#,
    ));
    runner.push_response(success(
        br#"{"result":{"type":"pane_info","pane":{"pane_id":"w7:p6"}}}"#,
    ));
    runner.push_response(success(
        br#"{"result":{"type":"pane_info","pane":{"pane_id":"w7:p7"}}}"#,
    ));
    runner.push_response(success(
        br#"{"result":{"type":"tab_info","tab":{"tab_id":"w7:t3"}}}"#,
    ));

    let result = add(
        &config_manager,
        &GlobalConfig::default(),
        &runner,
        &AddRepositoryRequest {
            workspace: workspace.name,
            repo: "backend".into(),
            target_branch: None,
        },
    )
    .unwrap();

    assert_eq!(result.terminal, TerminalUpdate::Updated);
    let (_, saved) = config_manager.load_workspace("calm-river").unwrap();
    let state = toml::Value::try_from(saved.multiplexer_state).unwrap();
    assert_eq!(state["payload"]["workspace_id"].as_str(), Some("w7"));
    let calls = runner.take_calls();
    assert_eq!(
        calls[6].args,
        vec![
            "--session",
            "agents",
            "tab",
            "create",
            "--workspace",
            "w7",
            "--cwd",
            result.worktree_path.as_str(),
            "--label",
            "backend",
            "--no-focus"
        ]
    );
    assert_eq!(
        calls[9].args,
        vec!["--session", "agents", "tab", "focus", "w7:t3"]
    );
    assert!(calls
        .iter()
        .all(|call| !call.args.iter().any(|arg| arg == "attach")));
}

#[test]
fn add_rejects_an_existing_herdr_repo_tab_before_git_mutation() {
    let (_temp, config_manager, mut workspace) = setup();
    workspace.multiplexer.kind = MultiplexerKind::Herdr;
    workspace.multiplexer.herdr.session = "agents".into();
    config_manager
        .save_workspace(&WorkspaceStatus::InProgress, &workspace)
        .unwrap();
    let runner = MockRunner::new();
    runner.push_response(success(b"refs/heads/main\n"));
    runner.push_response(success(b""));
    runner.push_response(success(b"herdr 0.8.2\n"));
    runner.push_response(success(
        "{\"result\":{\"type\":\"workspace_list\",\"workspaces\":[{\"workspace_id\":\"w7\",\"label\":\"Add backend · zootree:calm-river\"}]}}"
            .as_bytes(),
    ));
    runner.push_response(success(
        br#"{"result":{"type":"tab_list","tabs":[{"tab_id":"w7:t3","label":"backend"}]}}"#,
    ));

    let error = add(
        &config_manager,
        &GlobalConfig::default(),
        &runner,
        &AddRepositoryRequest {
            workspace: workspace.name,
            repo: "backend".into(),
            target_branch: None,
        },
    )
    .unwrap_err();

    assert!(error.to_string().contains("already contains a tab"));
    assert_eq!(runner.take_calls().len(), 5);
}

#[test]
fn add_skips_herdr_mutation_when_the_workspace_is_verified_absent() {
    let (_temp, config_manager, mut workspace) = setup();
    workspace.multiplexer.kind = MultiplexerKind::Herdr;
    workspace.multiplexer.herdr.session = "agents".into();
    config_manager
        .save_workspace(&WorkspaceStatus::InProgress, &workspace)
        .unwrap();
    let runner = MockRunner::new();
    runner.push_response(success(b"refs/heads/main\n"));
    runner.push_response(success(b""));
    runner.push_response(success(b"herdr 0.8.2\n"));
    runner.push_response(success(
        br#"{"result":{"type":"workspace_list","workspaces":[]}}"#,
    ));
    runner.push_response(success(b""));

    let result = add(
        &config_manager,
        &GlobalConfig::default(),
        &runner,
        &AddRepositoryRequest {
            workspace: workspace.name,
            repo: "backend".into(),
            target_branch: None,
        },
    )
    .unwrap();

    assert_eq!(result.terminal, TerminalUpdate::Absent);
    assert_eq!(runner.take_calls().len(), 5);
}

#[test]
fn add_rolls_back_a_partially_created_herdr_tab_before_git_artifacts() {
    let (_temp, config_manager, mut workspace) = setup();
    workspace.multiplexer.kind = MultiplexerKind::Herdr;
    workspace.multiplexer.herdr.session = "agents".into();
    workspace.multiplexer_state = toml::from_str(
        r#"
version = 1
adapter = "herdr"

[payload]
session = "agents"
workspace_id = "w7"
label = "Add backend · zootree:calm-river"
"#,
    )
    .unwrap();
    config_manager
        .save_workspace(&WorkspaceStatus::InProgress, &workspace)
        .unwrap();
    let runner = MockRunner::new();
    runner.push_response(success(b"refs/heads/main\n"));
    runner.push_response(success(b""));
    runner.push_response(success(b"herdr 0.8.2\n"));
    runner.push_response(success(
        r#"{"result":{"type":"workspace_info","workspace":{"workspace_id":"w7","label":"Add backend · zootree:calm-river"}}}"#
            .as_bytes(),
    ));
    runner.push_response(success(
        br#"{"result":{"type":"tab_list","tabs":[{"tab_id":"w7:t1","label":"overview"}]}}"#,
    ));
    runner.push_response(success(b""));
    runner.push_response(success(
        br#"{"result":{"type":"tab_created","tab":{"tab_id":"w7:t3"},"root_pane":{"pane_id":"w7:p5"}}}"#,
    ));
    runner.push_response(failure(
        br#"{"error":{"code":"invalid_request","message":"split failed"}}"#,
    ));
    runner.push_response(success(br#"{"result":{"type":"ok"}}"#));
    runner.push_response(success(b""));
    runner.push_response(success(b""));

    let error = add(
        &config_manager,
        &GlobalConfig::default(),
        &runner,
        &AddRepositoryRequest {
            workspace: workspace.name,
            repo: "backend".into(),
            target_branch: None,
        },
    )
    .unwrap_err();

    assert!(error.to_string().contains("split failed"));
    let calls = runner.take_calls();
    assert_eq!(
        calls[8].args,
        vec!["--session", "agents", "tab", "close", "w7:t3"]
    );
    assert_eq!(calls[9].args[2..5], ["worktree", "remove", "--force"]);
    assert_eq!(calls[10].args[2..5], ["branch", "-D", "zootree/calm-river"]);
}

#[test]
fn add_rejects_duplicate_membership_without_running_commands() {
    let (_temp, config_manager, mut workspace) = setup();
    workspace.repos.push(RepoEntry {
        name: "backend".into(),
        target_branch: Some("release".into()),
    });
    config_manager
        .save_workspace(&WorkspaceStatus::InProgress, &workspace)
        .unwrap();
    let runner = MockRunner::new();

    let error = add(
        &config_manager,
        &GlobalConfig::default(),
        &runner,
        &AddRepositoryRequest {
            workspace: workspace.name,
            repo: "backend".into(),
            target_branch: Some("main".into()),
        },
    )
    .unwrap_err();

    assert!(error.to_string().contains("target branch 'release'"));
    assert!(runner.take_calls().is_empty());
}

#[test]
fn add_does_not_guess_a_branch_when_current_branch_resolution_fails() {
    let (_temp, config_manager, workspace) = setup();
    let mut repo = config_manager.load_repo_config("backend").unwrap();
    repo.default_target_branch = None;
    config_manager.save_repo_config("backend", &repo).unwrap();
    let runner = MockRunner::new();
    runner.push_response(failure(b"detached HEAD"));

    let error = add(
        &config_manager,
        &GlobalConfig::default(),
        &runner,
        &AddRepositoryRequest {
            workspace: workspace.name,
            repo: "backend".into(),
            target_branch: None,
        },
    )
    .unwrap_err();

    assert!(error
        .to_string()
        .contains("failed to resolve the current branch"));
    assert_eq!(runner.take_calls().len(), 1);
}

#[test]
fn add_reports_cleanup_residue_and_retains_branch_when_worktree_removal_fails() {
    let (_temp, config_manager, workspace) = setup();
    let mut repo = config_manager.load_repo_config("backend").unwrap();
    repo.hooks.post_create = Some(HookValue::Simple("false".into()));
    config_manager.save_repo_config("backend", &repo).unwrap();
    let runner = MockRunner::new();
    runner.push_response(success(b"refs/heads/main\n"));
    runner.push_response(success(b""));
    runner.push_response(success(b""));
    runner.push_response(success(b""));
    runner.push_response(failure(b"setup failed"));
    runner.push_response(failure(b"worktree busy"));

    let error = add(
        &config_manager,
        &GlobalConfig::default(),
        &runner,
        &AddRepositoryRequest {
            workspace: workspace.name,
            repo: "backend".into(),
            target_branch: None,
        },
    )
    .unwrap_err();

    let message = format!("{error:#}");
    assert!(message.contains("setup failed"));
    assert!(message.contains("worktree busy"));
    assert!(message.contains("was retained"));
    assert_eq!(runner.take_calls().len(), 6);
}

#[test]
fn add_cleans_up_a_branch_created_by_a_failed_worktree_add() {
    let (_temp, config_manager, workspace) = setup();
    let config_path = config_manager
        .base_dir
        .join("workspaces/in_progress/calm-river.toml");
    let original_config = std::fs::read(&config_path).unwrap();
    let runner = MockRunner::new();
    runner.push_response(success(b"refs/heads/main\n"));
    runner.push_response(success(b""));
    runner.push_response(success(b""));
    runner.push_response(failure(b"could not populate worktree"));
    runner.push_response(success(b""));
    runner.push_response(success(b"refs/heads/zootree/calm-river\n"));
    runner.push_response(success(b""));

    let error = add(
        &config_manager,
        &GlobalConfig::default(),
        &runner,
        &AddRepositoryRequest {
            workspace: workspace.name,
            repo: "backend".into(),
            target_branch: None,
        },
    )
    .unwrap_err();

    assert!(error.to_string().contains("could not populate worktree"));
    assert_eq!(std::fs::read(config_path).unwrap(), original_config);
    let calls = runner.take_calls();
    assert_eq!(
        calls[5].args,
        vec![
            "-C",
            "/repos/backend",
            "for-each-ref",
            "--format=%(refname)",
            "refs/heads/zootree/calm-river"
        ]
    );
    assert_eq!(
        calls[6].args,
        vec!["-C", "/repos/backend", "branch", "-D", "zootree/calm-river"]
    );
}

#[test]
fn add_cleans_up_a_worktree_and_branch_created_by_a_failed_worktree_add() {
    let (_temp, config_manager, workspace) = setup();
    let worktree_path = std::path::Path::new(&workspace.workspace_dir).join("backend");
    let inner = MockRunner::new();
    inner.push_response(success(b"refs/heads/main\n"));
    inner.push_response(success(b""));
    inner.push_response(success(b""));
    inner.push_response(failure(b"checkout failed after creating worktree"));
    inner.push_response(success(b"refs/heads/zootree/calm-river\n"));
    inner.push_response(success(b""));
    inner.push_response(success(b""));
    let runner = CreatePathOnWorktreeAddRunner {
        inner,
        worktree_path,
    };

    let error = add(
        &config_manager,
        &GlobalConfig::default(),
        &runner,
        &AddRepositoryRequest {
            workspace: workspace.name,
            repo: "backend".into(),
            target_branch: None,
        },
    )
    .unwrap_err();

    assert!(error
        .to_string()
        .contains("checkout failed after creating worktree"));
    let calls = runner.inner.take_calls();
    assert_eq!(calls[5].args[2..5], ["worktree", "remove", "--force"]);
    assert_eq!(
        calls[6].args,
        vec!["-C", "/repos/backend", "branch", "-D", "zootree/calm-river"]
    );
}

#[test]
fn add_cleans_up_a_registered_worktree_when_its_directory_is_missing() {
    let (_temp, config_manager, workspace) = setup();
    let worktree_path = std::path::Path::new(&workspace.workspace_dir).join("backend");
    let runner = MockRunner::new();
    runner.push_response(success(b"refs/heads/main\n"));
    runner.push_response(success(b""));
    runner.push_response(success(b""));
    runner.push_response(failure(b"checkout failed after registration"));
    runner.push_response(success(
        format!(
            "worktree {}\0HEAD abc\0branch refs/heads/zootree/calm-river\0\0",
            worktree_path.display()
        )
        .as_bytes(),
    ));
    runner.push_response(success(b"refs/heads/zootree/calm-river\n"));
    runner.push_response(success(b""));
    runner.push_response(success(b""));

    let error = add(
        &config_manager,
        &GlobalConfig::default(),
        &runner,
        &AddRepositoryRequest {
            workspace: workspace.name,
            repo: "backend".into(),
            target_branch: None,
        },
    )
    .unwrap_err();

    assert!(error
        .to_string()
        .contains("checkout failed after registration"));
    let calls = runner.take_calls();
    assert_eq!(calls[6].args[2..5], ["worktree", "remove", "--force"]);
    assert_eq!(
        calls[7].args,
        vec!["-C", "/repos/backend", "branch", "-D", "zootree/calm-river"]
    );
}
