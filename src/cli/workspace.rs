use crate::cli::create_flow::{
    create_args_need_wizard, discover_current_repo_candidate, draft_from_args,
    persist_selected_pending_repos, resolve_agent_cli_for_draft, workspace_from_draft,
    AfterCreateMode, CreateDraftError, CreateWizardOutput,
};
use crate::config::global::GlobalConfig;
use crate::config::template::TemplateConfig;
use crate::config::workspace::{Event, RepoEntry, WorkspaceConfig, WorkspaceStatus};
use crate::config::ConfigManager;
use crate::core::completers::{
    complete_agent_cli_alias, complete_repos_list, complete_template, complete_workspace,
    WorkspaceFilter,
};
use crate::core::copy_files;
use crate::core::git::GitOps;
use crate::core::hook::{HookContext, HookEngine};
use crate::core::reopen::{
    build_reopen_plan, execute_reopen_plan, format_reopen_plan, ReopenBase, ReopenLifecyclePlan,
    ReopenOptions, ReopenPrompt, ReopenSources, WorktreeAction,
};
use crate::core::repo_status::missing_registered_repo_names;
use crate::core::terminal_environment::{AgentIntent, CloseReport, TerminalEnvironment};
use crate::core::worktree_status::{
    format_missing_worktrees_error, missing_worktrees, repo_worktree_statuses, RepoWorktreeStatus,
};
use crate::runner::{CommandRunner, RealRunner};
use crate::tui;
use crate::tui_app::create_wizard::run_create_wizard;
use anyhow::Result;
use chrono::Local;
use clap::Args;
use clap_complete::ArgValueCompleter;
use std::collections::BTreeSet;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
#[clap(rename_all = "kebab-case")]
pub enum MergeStrategy {
    Squash,
    Rebase,
    Merge,
}

impl MergeStrategy {
    pub fn as_str(self) -> &'static str {
        match self {
            MergeStrategy::Squash => "squash",
            MergeStrategy::Rebase => "rebase",
            MergeStrategy::Merge => "merge",
        }
    }
}

pub fn parse_repos_arg(repos_str: &str) -> Vec<(String, Option<String>)> {
    repos_str
        .split(',')
        .map(|s| {
            let s = s.trim();
            if let Some((name, branch)) = s.split_once(':') {
                (name.to_string(), Some(branch.to_string()))
            } else {
                (s.to_string(), None)
            }
        })
        .collect()
}

pub fn build_repo_entries<R: crate::runner::CommandRunner>(
    config_mgr: &ConfigManager,
    runner: &R,
    repos: Vec<(String, Option<String>)>,
) -> Result<Vec<RepoEntry>> {
    let git = GitOps::new(runner);
    let mut entries = Vec::new();

    for (name, branch) in repos {
        let repo_config = config_mgr.load_repo_config(&name)?;
        let repo_path = shellexpand::tilde(&repo_config.path).into_owned();
        let target_branch = branch
            .or(repo_config.default_target_branch.clone())
            .unwrap_or_else(|| {
                git.current_branch(&repo_path)
                    .unwrap_or_else(|_| "main".into())
            });
        entries.push(RepoEntry {
            name,
            target_branch: Some(target_branch),
        });
    }

    Ok(entries)
}

#[cfg(test)]
fn template_repos_to_entries_input(
    tmpl_name: &str,
    repos: Vec<String>,
) -> Result<Vec<(String, Option<String>)>> {
    if repos.is_empty() {
        anyhow::bail!("template '{}' has no repos", tmpl_name);
    }
    Ok(repos.into_iter().map(|name| (name, None)).collect())
}

#[derive(Debug, Clone, PartialEq)]
struct ListWorkspaceItem {
    status: WorkspaceStatus,
    workspace: WorkspaceConfig,
    worktrees: Vec<RepoWorktreeStatus>,
    missing_repos: Vec<String>,
}

fn selected_agent_cli_value(
    run_agent: &Option<Option<String>>,
    global: &GlobalConfig,
) -> Result<Option<String>> {
    match run_agent {
        None => Ok(None),
        Some(Some(value)) if !value.is_empty() => Ok(Some(value.clone())),
        Some(_) => Ok(Some(global.agent_cli.clone().ok_or_else(|| {
            anyhow::anyhow!(
                "--run-agent requires agent_cli in global config (~/.config/zootree/config.toml)"
            )
        })?)),
    }
}

pub fn handle_create(args: &CreateArgs) -> Result<()> {
    let config_mgr = ConfigManager::new()?;
    config_mgr.ensure_dirs()?;
    let global = config_mgr.load_global_config()?;
    let runner = RealRunner;
    let existing: Vec<String> = config_mgr
        .list_workspaces(None::<&[WorkspaceStatus]>)?
        .iter()
        .map(|w| w.name.clone())
        .collect();
    let needs_wizard = create_args_need_wizard(args);
    let needs_repo_selection = args.repos.is_none() && args.template.is_none();
    let current_repo = if needs_wizard && needs_repo_selection {
        discover_current_repo_candidate(&config_mgr, &runner, &std::env::current_dir()?)?
    } else {
        None
    };
    let draft = draft_from_args(args, &config_mgr, &runner, &global, current_repo, &existing)?;
    let mut output = if needs_wizard {
        run_create_wizard(draft, global.clone(), existing.clone())?
    } else {
        let errors = draft.validate(&existing, &global);
        if !errors.is_empty() {
            anyhow::bail!("invalid create options: {}", format_draft_errors(&errors));
        }
        CreateWizardOutput { draft }
    };
    persist_selected_pending_repos(&config_mgr, &mut output.draft)?;
    let agent_cli = resolve_agent_cli_for_draft(&output.draft.after_create, &global)?;
    let multiplexer = output
        .draft
        .multiplexer
        .clone()
        .unwrap_or_else(|| global.multiplexer.clone());
    let workspace = workspace_from_draft(
        &output.draft,
        Local::now().to_rfc3339(),
        agent_cli,
        multiplexer,
    );
    let name = workspace.name.clone();

    config_mgr.save_workspace(&WorkspaceStatus::Pending, &workspace)?;
    save_recently_template(&config_mgr, &workspace)?;

    println!("workspace '{}' created (pending)", name);
    println!("  branch: {}", workspace.branch);
    println!(
        "  repos: {}",
        workspace
            .repos
            .iter()
            .map(|r| format!("{}:{}", r.name, r.target_branch.as_deref().unwrap_or("*")))
            .collect::<Vec<_>>()
            .join(", ")
    );

    start_after_create_if_needed(&name, &output.draft.after_create)?;

    Ok(())
}

fn format_draft_errors(errors: &[CreateDraftError]) -> String {
    errors
        .iter()
        .map(|error| match error {
            CreateDraftError::TitleRequired => "title is required".to_string(),
            CreateDraftError::TitleSingleLineRequired => "title must be a single line".to_string(),
            CreateDraftError::WorkspaceNameRequired => "workspace name is required".to_string(),
            CreateDraftError::WorkspaceNameInvalid(name) => {
                format!("workspace name '{name}' must use only ASCII letters, numbers, '-' and '_'")
            }
            CreateDraftError::WorkspaceNameSingleLineRequired => {
                "workspace name must be a single line".to_string()
            }
            CreateDraftError::WorkspaceBranchRequired => "workspace branch is required".to_string(),
            CreateDraftError::WorkspaceBranchSingleLineRequired => {
                "workspace branch must be a single line".to_string()
            }
            CreateDraftError::WorkspaceNameExists(name) => {
                format!("workspace name '{}' already exists", name)
            }
            CreateDraftError::RepoRequired => "at least one repo must be selected".to_string(),
            CreateDraftError::TargetBranchRequired(repo) => {
                format!("target branch for repo '{}' is required", repo)
            }
            CreateDraftError::TargetBranchSingleLineRequired(repo) => {
                format!("target branch for repo '{}' must be a single line", repo)
            }
            CreateDraftError::DefaultAgentMissing => {
                "--run-agent requires agent_cli in global config (~/.config/zootree/config.toml)"
                    .to_string()
            }
            CreateDraftError::RunAgentSingleLineRequired => {
                "run-agent must be a single line".to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn save_recently_template(config_mgr: &ConfigManager, workspace: &WorkspaceConfig) -> Result<()> {
    let recently = TemplateConfig {
        repos: workspace.repos.iter().map(|r| r.name.clone()).collect(),
        multiplexer: workspace.multiplexer.clone(),
    };
    config_mgr.save_template("recently", &recently)
}

fn start_after_create_if_needed(name: &str, mode: &AfterCreateMode) -> Result<()> {
    if mode.should_start() {
        let start_args = StartArgs {
            name: Some(name.to_string()),
            no_multiplexer: false,
            run_agent: mode.run_agent_arg(),
        };
        handle_start(&start_args)?;
    }

    Ok(())
}

pub fn handle_start(args: &StartArgs) -> Result<()> {
    let config_mgr = ConfigManager::new()?;
    let global = config_mgr.load_global_config()?;
    let runner = RealRunner;

    let (workspace, warnings) =
        start_workspace_and_activate_with(&config_mgr, &global, &runner, args)?;
    println!("workspace '{}' started", workspace.name);
    report_terminal_environment_warnings(&workspace.name, warnings);

    Ok(())
}

#[derive(Debug)]
struct CreatedWorktree {
    repo_path: String,
    worktree_path: String,
}

#[derive(Debug)]
struct StartRollback {
    created_worktrees: Vec<CreatedWorktree>,
    workspace_dir_to_remove: Option<PathBuf>,
    active: bool,
}

impl StartRollback {
    fn new(workspace_dir_to_remove: Option<PathBuf>) -> Self {
        Self {
            created_worktrees: Vec::new(),
            workspace_dir_to_remove,
            active: true,
        }
    }

    fn record_worktree(&mut self, repo_path: String, worktree_path: String) {
        self.created_worktrees.push(CreatedWorktree {
            repo_path,
            worktree_path,
        });
    }

    fn disarm(&mut self) {
        self.active = false;
    }

    fn rollback<R: CommandRunner>(&mut self, git: &GitOps<'_, R>) -> Result<()> {
        if !self.active {
            return Ok(());
        }

        let mut failures = Vec::new();
        for created in self.created_worktrees.iter().rev() {
            if let Err(e) = git.worktree_remove(&created.repo_path, &created.worktree_path, true) {
                tracing::warn!(
                    "failed to rollback worktree '{}': {}",
                    created.worktree_path,
                    e
                );
                failures.push(format!("{}: {:#}", created.worktree_path, e));
            }
        }

        if let Some(dir) = &self.workspace_dir_to_remove {
            match std::fs::remove_dir(dir) {
                Ok(()) => {}
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::NotFound | std::io::ErrorKind::DirectoryNotEmpty
                    ) => {}
                Err(e) => {
                    tracing::warn!(
                        "failed to rollback workspace dir '{}': {}",
                        dir.display(),
                        e
                    );
                    failures.push(format!("{}: {}", dir.display(), e));
                }
            }
        }

        self.active = false;
        if failures.is_empty() {
            Ok(())
        } else {
            anyhow::bail!("rollback failed: {}", failures.join("; "))
        }
    }
}

fn finish_start_failure<T, R: CommandRunner>(
    err: anyhow::Error,
    rollback: &mut StartRollback,
    git: &GitOps<'_, R>,
) -> Result<T> {
    if let Err(rollback_err) = rollback.rollback(git) {
        Err(anyhow::anyhow!(
            "start failed: {:#}; rollback failed: {:#}",
            err,
            rollback_err
        ))
    } else {
        Err(err)
    }
}

fn start_workspace_with<R: CommandRunner>(
    config_mgr: &ConfigManager,
    global: &GlobalConfig,
    runner: &R,
    args: &StartArgs,
) -> Result<WorkspaceConfig> {
    let git = GitOps::new(runner);
    let hook_engine = HookEngine::new(runner);
    let name = match &args.name {
        Some(n) => n.clone(),
        None => {
            let pending = config_mgr.list_workspaces(Some(&[WorkspaceStatus::Pending]))?;
            if pending.is_empty() {
                anyhow::bail!("no pending workspaces");
            }
            let names: Vec<String> = pending
                .iter()
                .map(|w| format!("{} - {}", w.name, w.title))
                .collect();
            let idx = tui::select_one("Select workspace to start", &names)?;
            pending[idx].name.clone()
        }
    };

    let (status, mut workspace) = config_mgr.load_workspace(&name)?;
    if !matches!(status, WorkspaceStatus::Pending) {
        anyhow::bail!("workspace '{}' is not in pending state", name);
    }

    let ws_dir = shellexpand::tilde(&workspace.workspace_dir).into_owned();
    let ws_dir_path = PathBuf::from(&ws_dir);
    let created_workspace_dir = !ws_dir_path.exists();
    std::fs::create_dir_all(&ws_dir)?;
    let mut rollback = StartRollback::new(created_workspace_dir.then_some(ws_dir_path));

    let prepare_result = (|| -> Result<()> {
        if args.run_agent.is_some() {
            workspace.agent_cli = selected_agent_cli_value(&args.run_agent, global)?;
        }

        for repo_entry in &workspace.repos {
            let repo_config = config_mgr.load_repo_config(&repo_entry.name)?;
            let repo_path = shellexpand::tilde(&repo_config.path).into_owned();

            let target_branch = match &repo_entry.target_branch {
                Some(tb) if git.branch_exists(&repo_path, tb)? => tb.clone(),
                Some(tb) => {
                    let current = git.current_branch(&repo_path)?;
                    tracing::warn!(
                        "target branch '{}' not found in repo '{}', using current branch '{}'",
                        tb,
                        repo_entry.name,
                        current
                    );
                    current
                }
                None => {
                    let current = git.current_branch(&repo_path)?;
                    tracing::warn!(
                        "target branch not configured for repo '{}', using current branch '{}'",
                        repo_entry.name,
                        current
                    );
                    current
                }
            };

            let worktree_path = format!("{}/{}", ws_dir, repo_entry.name);

            tracing::info!(
                "creating worktree for {} at {}",
                repo_entry.name,
                worktree_path
            );
            git.worktree_add(
                &repo_path,
                &workspace.branch,
                &worktree_path,
                &target_branch,
            )?;
            rollback.record_worktree(repo_path.clone(), worktree_path.clone());

            let patterns =
                copy_files::merge_copy_files(&global.copy_files, &repo_config.copy_files);
            if !patterns.is_empty() {
                copy_files::copy_files_to_worktree(
                    Path::new(&repo_path),
                    Path::new(&worktree_path),
                    &patterns,
                )?;
            }

            let hook = repo_config
                .hooks
                .post_create
                .as_ref()
                .or(global.hooks.post_create.as_ref());
            if let Some(h) = hook {
                let ctx = HookContext {
                    workspace: workspace.name.clone(),
                    repo: Some(repo_entry.name.clone()),
                    branch: workspace.branch.clone(),
                    target_branch: Some(target_branch.clone()),
                    worktree_path: Some(worktree_path.clone()),
                    workspace_dir: ws_dir.clone(),
                };
                hook_engine.execute(h, &ctx)?;
            }
        }

        Ok(())
    })();

    if let Err(err) = prepare_result {
        return finish_start_failure(err, &mut rollback, &git);
    }

    let now = Local::now().to_rfc3339();
    workspace.events.push(Event {
        action: "started".into(),
        timestamp: now,
        detail: None,
    });
    if let Err(err) = config_mgr.save_workspace(&WorkspaceStatus::Pending, &workspace) {
        return finish_start_failure(err, &mut rollback, &git);
    }
    if let Err(err) = config_mgr.move_workspace(
        &name,
        &WorkspaceStatus::Pending,
        &WorkspaceStatus::InProgress,
    ) {
        return finish_start_failure(err, &mut rollback, &git);
    }
    rollback.disarm();

    if let Some(h) = &global.hooks.post_start {
        let ctx = HookContext {
            workspace: workspace.name.clone(),
            repo: None,
            branch: workspace.branch.clone(),
            target_branch: None,
            worktree_path: None,
            workspace_dir: ws_dir.clone(),
        };
        hook_engine.execute(h, &ctx)?;
    }

    Ok(workspace)
}

fn start_workspace_and_activate_with<R: CommandRunner>(
    config_mgr: &ConfigManager,
    global: &GlobalConfig,
    runner: &R,
    args: &StartArgs,
) -> Result<(WorkspaceConfig, Vec<String>)> {
    let workspace = start_workspace_with(config_mgr, global, runner, args)?;
    if args.no_multiplexer {
        return Ok((workspace, Vec::new()));
    }

    let warnings = activate_terminal_environment_with(
        config_mgr,
        global,
        &workspace,
        runner,
        agent_intent(args.run_agent.clone()),
    )
    .map_err(|error| {
        anyhow::anyhow!(
            "workspace '{}' started and remains in_progress, but terminal environment activation failed: {:#}. Run `zootree open {}` to retry",
            workspace.name,
            error,
            workspace.name
        )
    })?;

    Ok((workspace, warnings))
}

pub fn handle_list(args: &ListArgs) -> Result<()> {
    let config_mgr = ConfigManager::new()?;

    let status_filter: Vec<WorkspaceStatus> = if args.status.is_empty() {
        vec![WorkspaceStatus::Pending, WorkspaceStatus::InProgress]
    } else {
        args.status.clone()
    };

    let workspaces = config_mgr.list_workspaces_with_status(Some(status_filter.as_slice()))?;

    if workspaces.is_empty() {
        println!("no workspaces found");
        return Ok(());
    }

    let mut items = Vec::with_capacity(workspaces.len());
    for entry in workspaces {
        let status = entry.status;
        let ws = entry.config;
        let worktrees = if matches!(status, WorkspaceStatus::InProgress) {
            let ws_dir = shellexpand::tilde(&ws.workspace_dir).into_owned();
            repo_worktree_statuses(&ws, &ws_dir)
        } else {
            Vec::new()
        };
        let missing_repos = missing_registered_repo_names(&config_mgr, &ws.repos);
        items.push(ListWorkspaceItem {
            status,
            workspace: ws,
            worktrees,
            missing_repos,
        });
    }

    let output = if args.oneline {
        render_list_oneline(&items)
    } else {
        render_list_cards(&items)
    };
    print!("{}", output);

    Ok(())
}

fn format_status(status: &WorkspaceStatus) -> &'static str {
    match status {
        WorkspaceStatus::Pending => "pending",
        WorkspaceStatus::InProgress => "in_progress",
        WorkspaceStatus::Done => "done",
        WorkspaceStatus::Canceled => "canceled",
    }
}

fn format_repo_targets(repos: &[RepoEntry], missing_repos: &[String]) -> String {
    if repos.is_empty() {
        return "(none)".into();
    }

    repos
        .iter()
        .map(|r| {
            let target = r.target_branch.as_deref().unwrap_or("*");
            if missing_repos.contains(&r.name) {
                format!("{}:{} (missing)", r.name, target)
            } else {
                format!("{}:{}", r.name, target)
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_missing_worktree_names(worktrees: &[RepoWorktreeStatus]) -> Option<String> {
    let names = missing_worktrees(worktrees)
        .iter()
        .map(|status| status.repo_name.as_str())
        .collect::<Vec<_>>();
    if names.is_empty() {
        None
    } else {
        Some(names.join(", "))
    }
}

fn render_list_oneline(items: &[ListWorkspaceItem]) -> String {
    let mut out = String::new();

    for item in items {
        let ws = &item.workspace;
        let status_str = format_status(&item.status);
        let repos_str = format_repo_targets(&ws.repos, &item.missing_repos);

        if matches!(item.status, WorkspaceStatus::InProgress) {
            let missing = format_missing_worktree_names(&item.worktrees)
                .map(|names| format!(" [missing: {}]", names))
                .unwrap_or_default();
            out.push_str(&format!(
                "  {} ({}) - {} [{}] {}{}\n",
                ws.name, status_str, ws.title, repos_str, ws.workspace_dir, missing
            ));
        } else {
            out.push_str(&format!(
                "  {} ({}) - {} [{}]\n",
                ws.name, status_str, ws.title, repos_str
            ));
        }
    }

    out
}

fn render_list_cards(items: &[ListWorkspaceItem]) -> String {
    let mut out = String::new();

    for (idx, item) in items.iter().enumerate() {
        if idx > 0 {
            out.push('\n');
        }

        let ws = &item.workspace;
        out.push_str(&format!(
            "{}  [{}]  {}\n",
            ws.name,
            format_status(&item.status),
            ws.branch
        ));
        out.push_str(&format!("  title: {}\n", ws.title));
        out.push_str(&format!(
            "  repos: {}\n",
            format_repo_targets(&ws.repos, &item.missing_repos)
        ));

        if matches!(item.status, WorkspaceStatus::InProgress) {
            out.push_str(&format!("  dir:   {}\n", ws.workspace_dir));
            if let Some(names) = format_missing_worktree_names(&item.worktrees) {
                out.push_str(&format!("  missing worktrees: {}\n", names));
            }
        }
    }

    out
}

const CANCELABLE_STATUSES: &[WorkspaceStatus] =
    &[WorkspaceStatus::Pending, WorkspaceStatus::InProgress];

fn cancel_candidate_statuses() -> &'static [WorkspaceStatus] {
    CANCELABLE_STATUSES
}

fn is_cancelable_status(status: &WorkspaceStatus) -> bool {
    CANCELABLE_STATUSES.contains(status)
}

fn archive_canceled_workspace(
    config_mgr: &ConfigManager,
    from_status: &WorkspaceStatus,
    workspace: &mut WorkspaceConfig,
) -> Result<()> {
    let now = Local::now().to_rfc3339();
    workspace.events.push(Event {
        action: "canceled".into(),
        timestamp: now,
        detail: None,
    });
    config_mgr.save_workspace(from_status, workspace)?;
    config_mgr.move_workspace(&workspace.name, from_status, &WorkspaceStatus::Canceled)?;
    Ok(())
}

fn archive_canceled_workspace_and_close_with<R: CommandRunner>(
    config_mgr: &ConfigManager,
    global: &GlobalConfig,
    from_status: &WorkspaceStatus,
    workspace: &mut WorkspaceConfig,
    runner: &R,
) -> Result<Vec<String>> {
    archive_canceled_workspace(config_mgr, from_status, workspace)?;
    Ok(close_terminal_environment_with(
        config_mgr, global, workspace, runner,
    ))
}

pub fn handle_open(args: &OpenArgs) -> Result<()> {
    let config_mgr = ConfigManager::new()?;
    let global = config_mgr.load_global_config()?;
    let runner = RealRunner;

    let name = match &args.name {
        Some(n) => n.clone(),
        None => {
            let in_progress = config_mgr.list_workspaces(Some(&[WorkspaceStatus::InProgress]))?;
            if in_progress.is_empty() {
                anyhow::bail!("no in_progress workspaces");
            }
            let names: Vec<String> = in_progress
                .iter()
                .map(|w| format!("{} - {}", w.name, w.title))
                .collect();
            let idx = tui::select_one("Select workspace to open", &names)?;
            in_progress[idx].name.clone()
        }
    };

    let warnings = open_workspace_with(&config_mgr, &global, &runner, &name)?;
    report_terminal_environment_warnings(&name, warnings);
    Ok(())
}

fn open_workspace_with<R: CommandRunner>(
    config_mgr: &ConfigManager,
    global: &GlobalConfig,
    runner: &R,
    name: &str,
) -> Result<Vec<String>> {
    let (status, workspace) = config_mgr.load_workspace(name)?;
    if !matches!(status, WorkspaceStatus::InProgress) {
        anyhow::bail!("workspace '{}' is not in_progress", name);
    }

    ensure_required_worktrees_exist(&workspace)?;

    activate_terminal_environment_with(config_mgr, global, &workspace, runner, AgentIntent::None)
}

fn agent_intent(run_agent: Option<Option<String>>) -> AgentIntent {
    match run_agent {
        None => AgentIntent::None,
        Some(None) => AgentIntent::Default,
        Some(Some(command)) if command.is_empty() => AgentIntent::Default,
        Some(Some(command)) => AgentIntent::Override(command),
    }
}

fn activate_terminal_environment_with<R: CommandRunner>(
    config_mgr: &ConfigManager,
    global: &GlobalConfig,
    workspace: &WorkspaceConfig,
    runner: &R,
    agent_intent: AgentIntent,
) -> Result<Vec<String>> {
    let terminal_environment = TerminalEnvironment::new(config_mgr, global, runner);
    let activation = terminal_environment.activate(workspace, agent_intent)?;
    let mut updated = workspace.clone();
    updated.multiplexer_state = activation.stored_state;
    config_mgr.save_workspace(&WorkspaceStatus::InProgress, &updated)?;
    Ok(activation.warnings)
}

fn close_terminal_environment_with<R: CommandRunner>(
    config_mgr: &ConfigManager,
    global: &GlobalConfig,
    workspace: &WorkspaceConfig,
    runner: &R,
) -> Vec<String> {
    match config_mgr.load_workspace(&workspace.name) {
        Ok((WorkspaceStatus::Done | WorkspaceStatus::Canceled, _)) => {}
        Ok((status, _)) => {
            return vec![format!(
                "terminal environment close was skipped because workspace '{}' is still {}",
                workspace.name,
                status.as_str()
            )];
        }
        Err(error) => {
            return vec![format!(
                "terminal environment close was skipped because final workspace state could not be verified for '{}': {error:#}",
                workspace.name
            )];
        }
    }

    let terminal_environment = TerminalEnvironment::new(config_mgr, global, runner);
    terminal_environment.close(workspace).warnings
}

fn require_terminal_environment_closed(report: CloseReport) -> Result<Vec<String>> {
    if report.closed {
        return Ok(report.warnings);
    }

    let detail = if report.warnings.is_empty() {
        "terminal environment could not be confirmed closed".to_string()
    } else {
        report.warnings.join("; ")
    };
    anyhow::bail!("terminal environment could not be closed: {detail}")
}

fn close_terminal_environment_for_reopen_with<R: CommandRunner>(
    config_mgr: &ConfigManager,
    global: &GlobalConfig,
    workspace: &WorkspaceConfig,
    runner: &R,
) -> Result<Vec<String>> {
    match config_mgr.load_workspace(&workspace.name)? {
        (WorkspaceStatus::Done | WorkspaceStatus::Canceled, _) => {}
        (status, _) => anyhow::bail!(
            "cannot close terminal environment before reopen because workspace '{}' is {}",
            workspace.name,
            status.as_str()
        ),
    }

    let terminal_environment = TerminalEnvironment::new(config_mgr, global, runner);
    require_terminal_environment_closed(terminal_environment.close(workspace))
}

fn archive_done_workspace_and_close_with<R: CommandRunner>(
    config_mgr: &ConfigManager,
    global: &GlobalConfig,
    workspace: &mut WorkspaceConfig,
    runner: &R,
) -> Result<Vec<String>> {
    workspace.events.push(Event {
        action: "done".into(),
        timestamp: Local::now().to_rfc3339(),
        detail: None,
    });
    config_mgr.save_workspace(&WorkspaceStatus::InProgress, workspace)?;
    config_mgr.move_workspace(
        &workspace.name,
        &WorkspaceStatus::InProgress,
        &WorkspaceStatus::Done,
    )?;
    Ok(close_terminal_environment_with(
        config_mgr, global, workspace, runner,
    ))
}

fn report_terminal_environment_warnings(workspace_name: &str, warnings: Vec<String>) {
    for warning in warnings {
        tracing::warn!(
            "terminal environment for workspace '{}': {}",
            workspace_name,
            warning
        );
    }
}

#[derive(Args)]
pub struct CreateArgs {
    #[arg(long, help = "Workspace title (interactive if omitted)")]
    pub title: Option<String>,
    #[arg(long, help = "Workspace name (auto-generated if omitted)")]
    pub name: Option<String>,
    #[arg(long, help = "Workspace description")]
    pub description: Option<String>,
    #[arg(
        long,
        help = "Comma-separated repos, optionally with branch: repo1:branch1,repo2",
        add = ArgValueCompleter::new(|c: &std::ffi::OsStr| complete_repos_list(c))
    )]
    pub repos: Option<String>,
    #[arg(
        long,
        help = "Git branch name for worktrees (defaults to <prefix>/<name>)"
    )]
    pub branch: Option<String>,
    #[arg(
        long,
        help = "Template name to use for repo selection",
        add = ArgValueCompleter::new(|c: &std::ffi::OsStr| complete_template(c))
    )]
    pub template: Option<String>,
    #[arg(long, help = "Start the workspace immediately after creation")]
    pub start: bool,
    #[arg(
        long,
        num_args = 0..=1,
        default_missing_value = "",
        value_name = "ALIAS_OR_CMD",
        help = "Launch agent_cli in the designated pane after start (implies --start)",
        add = ArgValueCompleter::new(|c: &std::ffi::OsStr| complete_agent_cli_alias(c)),
    )]
    pub run_agent: Option<Option<String>>,
}

#[derive(Args)]
pub struct ListArgs {
    #[arg(
        long,
        value_enum,
        help = "Filter by status (repeatable: pending, in_progress, done, canceled)"
    )]
    pub status: Vec<WorkspaceStatus>,

    #[arg(long, help = "Use the legacy one-line output format")]
    pub oneline: bool,
}

#[derive(Args)]
pub struct StartArgs {
    #[arg(
        help = "Workspace name to start (interactive if omitted)",
        add = ArgValueCompleter::new(|c: &std::ffi::OsStr| complete_workspace(c, WorkspaceFilter::Pending))
    )]
    pub name: Option<String>,
    #[arg(
        long,
        help = "Skip launching the configured terminal multiplexer after start"
    )]
    pub no_multiplexer: bool,
    #[arg(
        long,
        num_args = 0..=1,
        default_missing_value = "",
        value_name = "ALIAS_OR_CMD",
        help = "Launch agent_cli in the designated pane (alias name or literal command)",
        add = ArgValueCompleter::new(|c: &std::ffi::OsStr| complete_agent_cli_alias(c)),
    )]
    pub run_agent: Option<Option<String>>,
}

#[derive(Args)]
pub struct OpenArgs {
    #[arg(
        help = "Workspace name to open (interactive if omitted)",
        add = ArgValueCompleter::new(|c: &std::ffi::OsStr| complete_workspace(c, WorkspaceFilter::InProgress))
    )]
    pub name: Option<String>,
}

#[derive(Args)]
pub struct ReopenArgs {
    #[arg(
        help = "Workspace name to reopen (interactive if omitted)",
        add = ArgValueCompleter::new(|c: &std::ffi::OsStr| complete_workspace(c, WorkspaceFilter::Archived))
    )]
    pub name: Option<String>,
    #[arg(
        long = "from",
        value_name = "current|REPO:BRANCH",
        help = "Choose a base when the task branch is missing (repeatable)"
    )]
    pub from: Vec<String>,
    #[arg(
        long,
        value_name = "REPO",
        help = "Replace an occupied worktree path for a repo (repeatable)"
    )]
    pub overwrite: Vec<String>,
    #[arg(long, help = "Skip post_create and post_start hooks")]
    pub skip_hooks: bool,
    #[arg(long, help = "Show the recovery plan without making changes")]
    pub dry_run: bool,
    #[arg(long, help = "Skip terminal environment activation after reopen")]
    pub no_multiplexer: bool,
    #[arg(
        long,
        num_args = 0..=1,
        default_missing_value = "",
        value_name = "ALIAS_OR_CMD",
        help = "Launch agent_cli when a new terminal environment is created",
        add = ArgValueCompleter::new(|c: &std::ffi::OsStr| complete_agent_cli_alias(c)),
    )]
    pub run_agent: Option<Option<String>>,
}

struct CliReopenPrompt {
    interactive: bool,
}

impl ReopenPrompt for CliReopenPrompt {
    fn is_interactive(&self) -> bool {
        self.interactive
    }

    fn choose_remote(&mut self, repo: &str, branches: &[String]) -> Result<String> {
        if !self.interactive {
            anyhow::bail!(
                "repo '{}' has ambiguous remote task branches; use --from {}:REMOTE/BRANCH",
                repo,
                repo
            );
        }
        let idx = tui::select_one(
            &format!("Select remote task branch for repo '{}'", repo),
            branches,
        )?;
        Ok(branches[idx].clone())
    }

    fn choose_base(&mut self, repo: &str, current: &str) -> Result<ReopenBase> {
        if !self.interactive {
            anyhow::bail!(
                "repo '{}' has no recoverable task branch; use --from current or --from {}:BRANCH",
                repo,
                repo
            );
        }
        let choices = vec![
            format!("current ({current})"),
            "specify another branch".into(),
        ];
        match tui::select_one(&format!("Select base for repo '{}'", repo), &choices)? {
            0 => Ok(ReopenBase::Current),
            _ => Ok(ReopenBase::Branch(tui::input_required("Base branch")?)),
        }
    }

    fn confirm_overwrite(
        &mut self,
        repo: &str,
        path: &str,
        valid_worktree: bool,
        dirty: bool,
    ) -> Result<bool> {
        if !self.interactive {
            anyhow::bail!(
                "repo '{}' requires --overwrite before replacing '{}'",
                repo,
                path
            );
        }
        let kind = if valid_worktree {
            "matching worktree"
        } else {
            "occupied path"
        };
        let loss = if dirty {
            " It contains uncommitted or untracked content that zootree cannot recover."
        } else {
            " Existing content will be permanently removed."
        };
        tui::confirm(
            &format!("Overwrite {kind} for repo '{repo}' at '{path}'?{loss}"),
            false,
        )
    }
}

pub fn handle_reopen(args: &ReopenArgs) -> Result<()> {
    let config_manager = ConfigManager::new()?;
    let global = config_manager.load_global_config()?;
    let runner = RealRunner;
    let interactive = std::io::stdin().is_terminal();
    let name = match &args.name {
        Some(name) => name.clone(),
        None if interactive => {
            let archived = config_manager
                .list_workspaces(Some(&[WorkspaceStatus::Done, WorkspaceStatus::Canceled]))?;
            if archived.is_empty() {
                anyhow::bail!("no archived workspaces to reopen");
            }
            let choices = archived
                .iter()
                .map(|workspace| format!("{} - {}", workspace.name, workspace.title))
                .collect::<Vec<_>>();
            let idx = tui::select_one("Select workspace to reopen", &choices)?;
            archived[idx].name.clone()
        }
        None => anyhow::bail!("workspace name is required when stdin is not interactive"),
    };
    let options = ReopenOptions {
        sources: ReopenSources::parse(&args.from)?,
        overwrite_repos: args.overwrite.iter().cloned().collect::<BTreeSet<_>>(),
    };
    let mut prompt = CliReopenPrompt { interactive };
    let mut plan = build_reopen_plan(&config_manager, &runner, &name, &options, &mut prompt)?;
    if args.run_agent.is_some() {
        plan.workspace.agent_cli = selected_agent_cli_value(&args.run_agent, &global)?;
    }
    let lifecycle = ReopenLifecyclePlan {
        skip_hooks: args.skip_hooks,
        activate_terminal_environment: !args.no_multiplexer,
        run_agent: args.run_agent.is_some(),
    };
    print!("{}", format_reopen_plan(&plan, &lifecycle));
    if args.dry_run {
        println!("dry run: no changes made");
        return Ok(());
    }

    if plan
        .repos
        .iter()
        .any(|repo| matches!(repo.worktree_action, WorktreeAction::Overwrite { .. }))
    {
        let warnings = close_terminal_environment_for_reopen_with(
            &config_manager,
            &global,
            &plan.workspace,
            &runner,
        )?;
        report_terminal_environment_warnings(&plan.workspace.name, warnings);
    }

    let workspace = execute_reopen_plan(&config_manager, &global, &runner, plan, args.skip_hooks)?;
    let warnings = if args.no_multiplexer {
        Vec::new()
    } else {
        activate_terminal_environment_with(
            &config_manager,
            &global,
            &workspace,
            &runner,
            agent_intent(args.run_agent.clone()),
        )
        .map_err(|error| {
            anyhow::anyhow!(
                "workspace '{}' reopened and remains in_progress, but terminal environment activation failed: {error:#}. Run `zootree open {}` to retry",
                workspace.name,
                workspace.name
            )
        })?
    };
    println!("workspace '{}' reopened", workspace.name);
    report_terminal_environment_warnings(&workspace.name, warnings);
    Ok(())
}

#[derive(Args)]
pub struct DoneArgs {
    #[arg(
        help = "Workspace name to complete (interactive if omitted)",
        add = ArgValueCompleter::new(|c: &std::ffi::OsStr| complete_workspace(c, WorkspaceFilter::InProgress))
    )]
    pub name: Option<String>,
    #[arg(long, help = "Skip merging branches back to target")]
    pub no_merge: bool,
    #[arg(long, help = "Keep worktrees and workspace directory")]
    pub no_clean: bool,
    #[arg(long, help = "Push target branch to remote after merge")]
    pub push: bool,
    #[arg(long, value_enum, help = "Merge strategy (default: squash)")]
    pub strategy: Option<MergeStrategy>,
    #[arg(long, help = "Continue even if steps fail (errors become warnings)")]
    pub force: bool,
    #[arg(long, help = "Skip all hooks (pre_done/pre_remove)")]
    pub skip_hooks: bool,
    #[arg(long, help = "Show what would be done without executing")]
    pub dry_run: bool,
}

#[derive(Args)]
pub struct CancelArgs {
    #[arg(
        help = "Workspace name to cancel (interactive if omitted)",
        add = ArgValueCompleter::new(|c: &std::ffi::OsStr| complete_workspace(c, WorkspaceFilter::Active))
    )]
    pub name: Option<String>,
    #[arg(long, help = "Keep worktrees and workspace directory")]
    pub no_clean: bool,
    #[arg(long, help = "Continue even if steps fail (errors become warnings)")]
    pub force: bool,
    #[arg(long, help = "Skip all hooks (pre_cancel/pre_remove)")]
    pub skip_hooks: bool,
}

fn warn_or_bail(force: bool, err: anyhow::Error, context: &str) -> Result<()> {
    if force {
        tracing::warn!("{}: {:#}", context, err);
        Ok(())
    } else {
        Err(err.context(format!("{} (use --force to proceed anyway)", context)))
    }
}

fn expanded_workspace_dir(workspace: &WorkspaceConfig) -> String {
    shellexpand::tilde(&workspace.workspace_dir).into_owned()
}

fn ensure_required_worktrees_exist(workspace: &WorkspaceConfig) -> Result<()> {
    let ws_dir = expanded_workspace_dir(workspace);
    let statuses = repo_worktree_statuses(workspace, &ws_dir);
    if missing_worktrees(&statuses).is_empty() {
        Ok(())
    } else {
        anyhow::bail!(
            "{}",
            format_missing_worktrees_error(&workspace.name, &statuses)
        )
    }
}

#[derive(Debug, PartialEq, Eq)]
enum CancelRepoWorktreeDecision {
    Proceed,
    SkipMissing {
        repo_name: String,
        worktree_path: String,
    },
}

fn cancel_repo_worktree_decision(
    repo_entry: &RepoEntry,
    worktree_path: &str,
    worktree_statuses: &[RepoWorktreeStatus],
) -> CancelRepoWorktreeDecision {
    let worktree = worktree_statuses
        .iter()
        .find(|status| status.repo_name == repo_entry.name);

    if worktree.is_some_and(|status| !status.exists) {
        CancelRepoWorktreeDecision::SkipMissing {
            repo_name: repo_entry.name.clone(),
            worktree_path: worktree_path.into(),
        }
    } else {
        CancelRepoWorktreeDecision::Proceed
    }
}

pub fn handle_done(args: &DoneArgs) -> Result<()> {
    let config_mgr = ConfigManager::new()?;
    let global = config_mgr.load_global_config()?;
    let runner = RealRunner;
    let git = GitOps::new(&runner);
    let hook_engine = HookEngine::new(&runner);

    let name = match &args.name {
        Some(n) => n.clone(),
        None => {
            let in_progress = config_mgr.list_workspaces(Some(&[WorkspaceStatus::InProgress]))?;
            if in_progress.is_empty() {
                anyhow::bail!("no in_progress workspaces");
            }
            let names: Vec<String> = in_progress
                .iter()
                .map(|w| format!("{} - {}", w.name, w.title))
                .collect();
            let idx = tui::select_one("Select workspace to complete", &names)?;
            in_progress[idx].name.clone()
        }
    };

    let (status, mut workspace) = config_mgr.load_workspace(&name)?;
    if !matches!(status, WorkspaceStatus::InProgress) {
        anyhow::bail!("workspace '{}' is not in_progress", name);
    }

    let ws_dir = expanded_workspace_dir(&workspace);

    if args.dry_run {
        println!("dry run for workspace '{}':", name);
        if !args.no_merge {
            for repo_entry in &workspace.repos {
                println!(
                    "  merge {} -> {}",
                    workspace.branch,
                    repo_entry.target_branch.as_deref().unwrap_or("*")
                );
            }
        }
        if !args.no_clean {
            println!("  clean worktrees and workspace directory");
        }
        return Ok(());
    }

    ensure_required_worktrees_exist(&workspace)?;

    // pre_done hook
    if !args.skip_hooks {
        if let Err(e) = hook_engine.execute_if_set(
            &global.hooks.pre_done,
            &HookContext {
                workspace: workspace.name.clone(),
                repo: None,
                branch: workspace.branch.clone(),
                target_branch: None,
                worktree_path: None,
                workspace_dir: ws_dir.clone(),
            },
        ) {
            warn_or_bail(args.force, e, "pre_done hook failed")?;
        }
    }

    for repo_entry in &workspace.repos {
        let repo_config = config_mgr.load_repo_config(&repo_entry.name)?;
        let repo_path = shellexpand::tilde(&repo_config.path).into_owned();
        let worktree_path = format!("{}/{}", ws_dir, repo_entry.name);

        let target_branch = match &repo_entry.target_branch {
            Some(tb) if git.branch_exists(&repo_path, tb)? => tb.clone(),
            Some(tb) => {
                let current = git.current_branch(&repo_path)?;
                tracing::warn!(
                    "target branch '{}' not found in repo '{}', using current branch '{}'",
                    tb,
                    repo_entry.name,
                    current
                );
                current
            }
            None => {
                let current = git.current_branch(&repo_path)?;
                tracing::warn!(
                    "target branch not configured for repo '{}', using current branch '{}'",
                    repo_entry.name,
                    current
                );
                current
            }
        };

        // Check uncommitted changes
        if git.has_uncommitted_changes(&worktree_path)? && !args.force {
            anyhow::bail!(
                "repo '{}' has uncommitted changes in {}. Commit or stash first, or use --force",
                repo_entry.name,
                worktree_path
            );
        }

        // Merge
        if !args.no_merge {
            let strategy = args.strategy.map(MergeStrategy::as_str);
            let message = if workspace.description.is_empty() {
                workspace.title.clone()
            } else {
                format!("{}\n\n{}", workspace.title, workspace.description)
            };
            git.merge_with_worktree(
                &repo_path,
                Some(&worktree_path),
                &workspace.branch,
                &target_branch,
                strategy,
                &message,
            )?;
            println!(
                "  merged {} -> {} ({})",
                workspace.branch, target_branch, repo_entry.name
            );

            if args.push {
                git.push(&repo_path, &target_branch)?;
                println!("  pushed {} ({})", target_branch, repo_entry.name);
            }
        }

        // Clean
        if !args.no_clean {
            let hook = repo_config
                .hooks
                .pre_remove
                .as_ref()
                .or(global.hooks.pre_remove.as_ref());
            if let Some(h) = hook {
                if !args.skip_hooks {
                    if let Err(e) = hook_engine.execute(
                        h,
                        &HookContext {
                            workspace: workspace.name.clone(),
                            repo: Some(repo_entry.name.clone()),
                            branch: workspace.branch.clone(),
                            target_branch: Some(target_branch.clone()),
                            worktree_path: Some(worktree_path.clone()),
                            workspace_dir: ws_dir.clone(),
                        },
                    ) {
                        warn_or_bail(args.force, e, "pre_remove hook failed")?;
                    }
                }
            }

            if let Err(e) = git.worktree_remove(&repo_path, &worktree_path, false) {
                tracing::warn!("failed to remove worktree '{}': {}", worktree_path, e);
            }
            // if let Err(e) = git.delete_local_branch(&repo_path, &workspace.branch, true) {
            //     tracing::warn!("failed to delete branch '{}': {}", workspace.branch, e);
            // }
        }
    }

    // Remove workspace directory
    if !args.no_clean && Path::new(&ws_dir).exists() {
        if let Err(e) = std::fs::remove_dir_all(&ws_dir) {
            warn_or_bail(args.force, e.into(), "failed to remove workspace directory")?;
        }
    }

    let warnings =
        archive_done_workspace_and_close_with(&config_mgr, &global, &mut workspace, &runner)?;
    report_terminal_environment_warnings(&workspace.name, warnings);

    println!("workspace '{}' completed", name);
    Ok(())
}

pub fn handle_cancel(args: &CancelArgs) -> Result<()> {
    let config_mgr = ConfigManager::new()?;
    let global = config_mgr.load_global_config()?;
    let runner = RealRunner;
    let git = GitOps::new(&runner);
    let hook_engine = HookEngine::new(&runner);

    let name = match &args.name {
        Some(n) => n.clone(),
        None => {
            let active = config_mgr.list_workspaces(Some(cancel_candidate_statuses()))?;
            if active.is_empty() {
                anyhow::bail!("no active workspaces");
            }
            let names: Vec<String> = active
                .iter()
                .map(|w| format!("{} - {}", w.name, w.title))
                .collect();
            let idx = tui::select_one("Select workspace to cancel", &names)?;
            active[idx].name.clone()
        }
    };

    let (status, mut workspace) = config_mgr.load_workspace(&name)?;
    if !is_cancelable_status(&status) {
        anyhow::bail!("workspace '{}' is not active", name);
    }

    if matches!(status, WorkspaceStatus::Pending) {
        archive_canceled_workspace(&config_mgr, &status, &mut workspace)?;
        println!("workspace '{}' canceled", name);
        return Ok(());
    }

    let ws_dir = expanded_workspace_dir(&workspace);
    let worktree_statuses = repo_worktree_statuses(&workspace, &ws_dir);

    // Confirm if uncommitted changes exist
    if !args.force {
        for repo_entry in &workspace.repos {
            let worktree_path = format!("{}/{}", ws_dir, repo_entry.name);
            if matches!(
                cancel_repo_worktree_decision(repo_entry, &worktree_path, &worktree_statuses),
                CancelRepoWorktreeDecision::Proceed
            ) && git.has_uncommitted_changes(&worktree_path)?
                && !tui::confirm(
                    &format!(
                        "repo '{}' has uncommitted changes. Continue?",
                        repo_entry.name
                    ),
                    false,
                )?
            {
                anyhow::bail!("canceled by user");
            }
        }
    }

    // pre_cancel hook
    if !args.skip_hooks {
        if let Err(e) = hook_engine.execute_if_set(
            &global.hooks.pre_cancel,
            &HookContext {
                workspace: workspace.name.clone(),
                repo: None,
                branch: workspace.branch.clone(),
                target_branch: None,
                worktree_path: None,
                workspace_dir: ws_dir.clone(),
            },
        ) {
            warn_or_bail(args.force, e, "pre_cancel hook failed")?;
        }
    }

    if !args.no_clean {
        for repo_entry in &workspace.repos {
            let worktree_path = format!("{}/{}", ws_dir, repo_entry.name);
            match cancel_repo_worktree_decision(repo_entry, &worktree_path, &worktree_statuses) {
                CancelRepoWorktreeDecision::Proceed => {}
                CancelRepoWorktreeDecision::SkipMissing {
                    repo_name,
                    worktree_path,
                } => {
                    println!(
                        "  warning: missing worktree: {} ({})",
                        repo_name, worktree_path
                    );
                    continue;
                }
            }
            let repo_config = config_mgr.load_repo_config(&repo_entry.name)?;
            let repo_path = shellexpand::tilde(&repo_config.path).into_owned();

            // pre_remove hook
            let hook = repo_config
                .hooks
                .pre_remove
                .as_ref()
                .or(global.hooks.pre_remove.as_ref());
            if let Some(h) = hook {
                if !args.skip_hooks {
                    if let Err(e) = hook_engine.execute(
                        h,
                        &HookContext {
                            workspace: workspace.name.clone(),
                            repo: Some(repo_entry.name.clone()),
                            branch: workspace.branch.clone(),
                            target_branch: repo_entry.target_branch.clone(),
                            worktree_path: Some(worktree_path.clone()),
                            workspace_dir: ws_dir.clone(),
                        },
                    ) {
                        warn_or_bail(args.force, e, "pre_remove hook failed")?;
                    }
                }
            }

            if Path::new(&worktree_path).exists() {
                if let Err(e) = git.worktree_remove(&repo_path, &worktree_path, args.force) {
                    tracing::warn!("failed to remove worktree '{}': {}", worktree_path, e);
                }
            }
            // if let Err(e) = git.delete_local_branch(&repo_path, &workspace.branch, true) {
            //     tracing::warn!("failed to delete branch '{}': {}", workspace.branch, e);
            // }
        }

        if Path::new(&ws_dir).exists() {
            if let Err(e) = std::fs::remove_dir_all(&ws_dir) {
                warn_or_bail(args.force, e.into(), "failed to remove workspace directory")?;
            }
        }
    }

    let warnings = archive_canceled_workspace_and_close_with(
        &config_mgr,
        &global,
        &status,
        &mut workspace,
        &runner,
    )?;
    report_terminal_environment_warnings(&workspace.name, warnings);

    println!("workspace '{}' canceled", name);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::global::{MultiplexerConfig, MultiplexerKind};
    use crate::config::workspace::StoredTerminalEnvironmentState;
    use crate::runner::MockRunner;
    use clap::Parser;
    use std::os::unix::process::ExitStatusExt;
    use std::process::{ExitStatus, Output};

    #[test]
    fn reopen_overwrite_accepts_a_confirmed_close_with_fallback_warnings() {
        let warnings = require_terminal_environment_closed(CloseReport {
            closed: true,
            warnings: vec!["stored terminal id was stale; closed by name".into()],
        })
        .unwrap();

        assert_eq!(warnings.len(), 1);

        let error = require_terminal_environment_closed(CloseReport {
            closed: false,
            warnings: vec!["terminal environment is ambiguous".into()],
        })
        .unwrap_err();
        assert!(error
            .to_string()
            .contains("terminal environment is ambiguous"));
    }

    #[derive(Parser)]
    struct TestListCli {
        #[command(flatten)]
        args: ListArgs,
    }

    #[derive(Parser)]
    struct TestStartCli {
        #[command(flatten)]
        args: StartArgs,
    }

    fn list_workspace(
        status: WorkspaceStatus,
        name: &str,
        title: &str,
        branch: &str,
        workspace_dir: &str,
        repos: Vec<RepoEntry>,
    ) -> ListWorkspaceItem {
        ListWorkspaceItem {
            status,
            workspace: WorkspaceConfig {
                title: title.into(),
                name: name.into(),
                description: String::new(),
                branch: branch.into(),
                workspace_dir: workspace_dir.into(),
                created_at: "2026-06-23T10:00:00+08:00".into(),
                agent_cli: None,
                multiplexer: MultiplexerConfig::default(),
                multiplexer_state: Default::default(),
                repos,
                events: Vec::new(),
            },
            worktrees: Vec::new(),
            missing_repos: Vec::new(),
        }
    }

    fn repo(name: &str, target_branch: Option<&str>) -> RepoEntry {
        RepoEntry {
            name: name.into(),
            target_branch: target_branch.map(str::to_string),
        }
    }

    fn repo_config(path: &str) -> crate::config::repo::RepoConfig {
        crate::config::repo::RepoConfig {
            path: path.into(),
            default_target_branch: None,
            copy_files: Vec::new(),
            hooks: crate::config::global::HooksConfig::default(),
            lazygit: None,
        }
    }

    fn success_output() -> Output {
        Output {
            status: ExitStatus::from_raw(0),
            stdout: Vec::new(),
            stderr: Vec::new(),
        }
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

    fn stored_terminal_state(source: &str) -> StoredTerminalEnvironmentState {
        toml::from_str(source).unwrap()
    }

    fn stored_terminal_state_table(state: &StoredTerminalEnvironmentState) -> toml::Table {
        toml::Value::try_from(state)
            .unwrap()
            .as_table()
            .unwrap()
            .clone()
    }

    fn missing_worktree(repo_name: &str, worktree_path: &str) -> RepoWorktreeStatus {
        RepoWorktreeStatus {
            repo_name: repo_name.into(),
            worktree_path: worktree_path.into(),
            exists: false,
        }
    }

    #[test]
    fn start_rolls_back_created_worktree_when_post_create_hook_fails() {
        let temp = tempfile::tempdir().unwrap();
        let config_mgr = ConfigManager::with_base_dir(temp.path().join("config"));
        config_mgr.ensure_dirs().unwrap();
        let workspace_dir = temp.path().join("workspaces/fair-fox");
        let mut repo_cfg = repo_config("/repo/api");
        repo_cfg.hooks.post_create =
            Some(crate::config::global::HookValue::Simple("exit 42".into()));
        config_mgr.save_repo_config("api", &repo_cfg).unwrap();
        let workspace = list_workspace(
            WorkspaceStatus::Pending,
            "fair-fox",
            "Fix start rollback",
            "zootree/fair-fox",
            &workspace_dir.to_string_lossy(),
            vec![repo("api", Some("main"))],
        )
        .workspace;
        config_mgr
            .save_workspace(&WorkspaceStatus::Pending, &workspace)
            .unwrap();
        let runner = MockRunner::new();
        runner.push_response(success_stdout("refs/heads/main\n"));
        runner.push_response(success_output());
        runner.push_response(failure_output("boom"));
        runner.push_response(success_output());

        let err = start_workspace_with(
            &config_mgr,
            &GlobalConfig::default(),
            &runner,
            &StartArgs {
                name: Some("fair-fox".into()),
                no_multiplexer: true,
                run_agent: None,
            },
        )
        .unwrap_err();
        let msg = format!("{:#}", err);

        assert!(msg.contains("hook failed"), "unexpected error: {msg}");
        assert!(
            !workspace_dir.exists(),
            "rollback should remove empty workspace dir created by start"
        );
        let (status, _) = config_mgr.load_workspace("fair-fox").unwrap();
        assert_eq!(status, WorkspaceStatus::Pending);
        let calls = runner.take_calls();
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
    fn list_args_parse_oneline_flag() {
        let parsed =
            TestListCli::try_parse_from(["test", "--status", "in-progress", "--oneline"]).unwrap();

        assert_eq!(parsed.args.status, vec![WorkspaceStatus::InProgress]);
        assert!(parsed.args.oneline);
    }

    #[test]
    fn start_args_accept_no_multiplexer() {
        let cli = TestStartCli::parse_from(["test", "--no-multiplexer", "fair-fox"]);
        assert!(cli.args.no_multiplexer);
        assert_eq!(cli.args.name.as_deref(), Some("fair-fox"));
    }

    #[test]
    fn start_args_reject_disable_zellij_flag() {
        let result = TestStartCli::try_parse_from(["test", "--no-zellij", "fair-fox"]);
        assert!(result.is_err());
    }

    #[test]
    fn render_list_oneline_matches_legacy_format() {
        let items = vec![
            list_workspace(
                WorkspaceStatus::InProgress,
                "pure-vine",
                "List output redesign",
                "zootree/pure-vine",
                "/Users/lijufeng/zootree-workspaces/pure-vine",
                vec![repo("zootree", Some("main"))],
            ),
            list_workspace(
                WorkspaceStatus::Pending,
                "calm-river",
                "Pending work",
                "zootree/calm-river",
                "/Users/lijufeng/zootree-workspaces/calm-river",
                vec![repo("frontend", None)],
            ),
        ];

        let out = render_list_oneline(&items);

        assert_eq!(
            out,
            "  pure-vine (in_progress) - List output redesign [zootree:main] /Users/lijufeng/zootree-workspaces/pure-vine\n  calm-river (pending) - Pending work [frontend:*]\n"
        );
    }

    #[test]
    fn render_list_cards_shows_missing_worktrees_for_in_progress_workspace() {
        let mut item = list_workspace(
            WorkspaceStatus::InProgress,
            "live-clay",
            "Fix worktree checks",
            "zootree/live-clay",
            "/tmp/live-clay",
            vec![repo("zootree", Some("main")), repo("docs", Some("main"))],
        );
        item.worktrees = vec![missing_worktree("docs", "/tmp/live-clay/docs")];

        let out = render_list_cards(&[item]);

        assert!(out.contains("  missing worktrees: docs"), "{out}");
    }

    #[test]
    fn render_list_oneline_shows_missing_worktrees_for_in_progress_workspace() {
        let mut item = list_workspace(
            WorkspaceStatus::InProgress,
            "live-clay",
            "Fix worktree checks",
            "zootree/live-clay",
            "/tmp/live-clay",
            vec![repo("zootree", Some("main")), repo("docs", Some("main"))],
        );
        item.worktrees = vec![missing_worktree("docs", "/tmp/live-clay/docs")];

        let out = render_list_oneline(&[item]);

        assert!(out.contains("/tmp/live-clay [missing: docs]"), "{out}");
    }

    #[test]
    fn render_list_cards_includes_branch_title_repos_and_dir_for_in_progress() {
        let items = vec![list_workspace(
            WorkspaceStatus::InProgress,
            "pure-vine",
            "zootree list 每项都堆在一行显示再窄屏时可视化效果太差",
            "zootree/pure-vine",
            "/Users/lijufeng/zootree-workspaces/pure-vine",
            vec![repo("zootree", Some("main"))],
        )];

        let out = render_list_cards(&items);

        assert_eq!(
            out,
            "pure-vine  [in_progress]  zootree/pure-vine\n  title: zootree list 每项都堆在一行显示再窄屏时可视化效果太差\n  repos: zootree:main\n  dir:   /Users/lijufeng/zootree-workspaces/pure-vine\n"
        );
    }

    #[test]
    fn render_list_cards_omits_dir_for_pending() {
        let items = vec![list_workspace(
            WorkspaceStatus::Pending,
            "calm-river",
            "Pending work",
            "zootree/calm-river",
            "/Users/lijufeng/zootree-workspaces/calm-river",
            vec![repo("frontend", None)],
        )];

        let out = render_list_cards(&items);

        assert_eq!(
            out,
            "calm-river  [pending]  zootree/calm-river\n  title: Pending work\n  repos: frontend:*\n"
        );
    }

    #[test]
    fn render_list_cards_separates_items_with_blank_line() {
        let items = vec![
            list_workspace(
                WorkspaceStatus::Pending,
                "one",
                "First",
                "zootree/one",
                "/tmp/one",
                vec![repo("frontend", Some("main"))],
            ),
            list_workspace(
                WorkspaceStatus::Pending,
                "two",
                "Second",
                "zootree/two",
                "/tmp/two",
                vec![repo("backend", Some("develop"))],
            ),
        ];

        let out = render_list_cards(&items);

        assert_eq!(
            out,
            "one  [pending]  zootree/one\n  title: First\n  repos: frontend:main\n\ntwo  [pending]  zootree/two\n  title: Second\n  repos: backend:develop\n"
        );
    }

    #[test]
    fn render_list_cards_shows_none_when_repos_empty() {
        let items = vec![list_workspace(
            WorkspaceStatus::Done,
            "empty-repos",
            "No repos",
            "zootree/empty-repos",
            "/tmp/empty-repos",
            Vec::new(),
        )];

        let out = render_list_cards(&items);

        assert_eq!(
            out,
            "empty-repos  [done]  zootree/empty-repos\n  title: No repos\n  repos: (none)\n"
        );
    }

    #[test]
    fn render_list_cards_marks_missing_registered_repo() {
        let mut item = list_workspace(
            WorkspaceStatus::Pending,
            "calm-leaf",
            "ggg",
            "zootree/calm-leaf",
            "/tmp/calm-leaf",
            vec![repo("zootree-2", Some("zootree/true-stone"))],
        );
        item.missing_repos = vec!["zootree-2".into()];

        let out = render_list_cards(&[item]);

        assert!(
            out.contains("  repos: zootree-2:zootree/true-stone (missing)"),
            "{out}"
        );
    }

    #[test]
    fn render_list_oneline_marks_missing_registered_repo() {
        let mut item = list_workspace(
            WorkspaceStatus::Pending,
            "calm-leaf",
            "ggg",
            "zootree/calm-leaf",
            "/tmp/calm-leaf",
            vec![repo("zootree-2", Some("zootree/true-stone"))],
        );
        item.missing_repos = vec!["zootree-2".into()];

        let out = render_list_oneline(&[item]);

        assert!(
            out.contains("[zootree-2:zootree/true-stone (missing)]"),
            "{out}"
        );
    }

    #[test]
    fn missing_registered_repo_names_marks_absent_config_or_path() {
        let tmp = tempfile::tempdir().unwrap();
        let config_mgr = ConfigManager::with_base_dir(tmp.path().join("config"));
        config_mgr.ensure_dirs().unwrap();
        let existing_path = tmp.path().join("existing-repo");
        std::fs::create_dir(&existing_path).unwrap();
        config_mgr
            .save_repo_config("existing", &repo_config(&existing_path.to_string_lossy()))
            .unwrap();
        config_mgr
            .save_repo_config(
                "deleted",
                &repo_config(&tmp.path().join("deleted-repo").to_string_lossy()),
            )
            .unwrap();

        let missing = missing_registered_repo_names(
            &config_mgr,
            &[
                repo("existing", None),
                repo("deleted", None),
                repo("absent", None),
            ],
        );

        assert_eq!(missing, vec!["deleted".to_string(), "absent".to_string()]);
    }

    #[test]
    fn cancel_candidate_statuses_are_pending_and_in_progress() {
        assert_eq!(
            cancel_candidate_statuses(),
            &[WorkspaceStatus::Pending, WorkspaceStatus::InProgress]
        );
    }

    #[test]
    fn is_cancelable_status_accepts_only_active_statuses() {
        assert!(is_cancelable_status(&WorkspaceStatus::Pending));
        assert!(is_cancelable_status(&WorkspaceStatus::InProgress));
        assert!(!is_cancelable_status(&WorkspaceStatus::Done));
        assert!(!is_cancelable_status(&WorkspaceStatus::Canceled));
    }

    #[test]
    fn cancel_repo_worktree_decision_skips_missing_worktree() {
        let repo_entry = repo("zootree", Some("main"));
        let worktree_path = "/tmp/live-clay/zootree";
        let statuses = vec![missing_worktree("zootree", worktree_path)];

        let decision = cancel_repo_worktree_decision(&repo_entry, worktree_path, &statuses);

        assert_eq!(
            decision,
            CancelRepoWorktreeDecision::SkipMissing {
                repo_name: "zootree".into(),
                worktree_path: worktree_path.into(),
            }
        );
    }

    #[test]
    fn cancel_repo_worktree_decision_proceeds_for_existing_worktree() {
        let repo_entry = repo("zootree", Some("main"));
        let worktree_path = "/tmp/live-clay/zootree";
        let statuses = vec![RepoWorktreeStatus {
            repo_name: "zootree".into(),
            worktree_path: worktree_path.into(),
            exists: true,
        }];

        let decision = cancel_repo_worktree_decision(&repo_entry, worktree_path, &statuses);

        assert_eq!(decision, CancelRepoWorktreeDecision::Proceed);
    }

    fn test_workspace(name: &str) -> WorkspaceConfig {
        WorkspaceConfig {
            title: format!("{} title", name),
            name: name.into(),
            description: String::new(),
            branch: format!("zootree/{}", name),
            workspace_dir: format!("/tmp/{}", name),
            created_at: "2026-06-29T10:00:00+08:00".into(),
            agent_cli: None,
            multiplexer: MultiplexerConfig::default(),
            multiplexer_state: Default::default(),
            repos: Vec::new(),
            events: Vec::new(),
        }
    }

    #[test]
    fn archive_canceled_workspace_moves_pending_to_canceled_with_event() {
        let tmp = tempfile::tempdir().unwrap();
        let config_mgr = ConfigManager::with_base_dir(tmp.path().join("config"));
        config_mgr.ensure_dirs().unwrap();
        let mut workspace = test_workspace("pending-cancel");
        config_mgr
            .save_workspace(&WorkspaceStatus::Pending, &workspace)
            .unwrap();

        archive_canceled_workspace(&config_mgr, &WorkspaceStatus::Pending, &mut workspace).unwrap();

        assert!(!config_mgr
            .base_dir
            .join("workspaces/pending/pending-cancel.toml")
            .exists());
        assert!(config_mgr
            .base_dir
            .join("workspaces/archived/canceled/pending-cancel.toml")
            .exists());
        let (status, archived) = config_mgr.load_workspace("pending-cancel").unwrap();
        assert_eq!(status, WorkspaceStatus::Canceled);
        assert_eq!(
            archived.events.last().map(|event| event.action.as_str()),
            Some("canceled")
        );
    }

    #[test]
    fn start_activation_failure_is_partial_success_and_open_retries() {
        let tmp = tempfile::tempdir().unwrap();
        let config_mgr = ConfigManager::with_base_dir(tmp.path().join("config"));
        config_mgr.ensure_dirs().unwrap();
        config_mgr
            .save_repo_config("api", &repo_config("/repo/api"))
            .unwrap();
        let workspace_dir = tmp.path().join("workspaces/partial-start");
        let mut workspace = list_workspace(
            WorkspaceStatus::Pending,
            "partial-start",
            "Partial terminal activation",
            "zootree/partial-start",
            &workspace_dir.to_string_lossy(),
            vec![repo("api", Some("main"))],
        )
        .workspace;
        workspace.multiplexer.kind = MultiplexerKind::Cmux;
        config_mgr
            .save_workspace(&WorkspaceStatus::Pending, &workspace)
            .unwrap();
        let start_runner = MockRunner::new();
        start_runner.push_response(success_stdout("refs/heads/main\n"));
        start_runner.push_response(success_output());
        start_runner.push_response(failure_output("cmux unavailable"));

        let error = start_workspace_and_activate_with(
            &config_mgr,
            &GlobalConfig::default(),
            &start_runner,
            &StartArgs {
                name: Some("partial-start".into()),
                no_multiplexer: false,
                run_agent: None,
            },
        )
        .unwrap_err();

        let message = format!("{error:#}");
        assert!(
            message.contains("started and remains in_progress"),
            "{message}"
        );
        assert!(message.contains("zootree open partial-start"), "{message}");
        let (status, after_failure) = config_mgr.load_workspace("partial-start").unwrap();
        assert_eq!(status, WorkspaceStatus::InProgress);
        assert!(after_failure.multiplexer_state.is_empty());
        assert!(workspace_dir.exists());
        let start_calls = start_runner.take_calls();
        assert!(!start_calls
            .iter()
            .any(|call| { call.program == "git" && call.args.iter().any(|arg| arg == "remove") }));

        std::fs::create_dir_all(workspace_dir.join("api")).unwrap();
        let open_runner = MockRunner::new();
        open_runner.push_response(success_stdout(
            r#"{"groups":[{"name":"Partial terminal activation","ref":"workspace_group:7"}]}"#,
        ));
        open_runner.push_response(success_output());

        let warnings = open_workspace_with(
            &config_mgr,
            &GlobalConfig::default(),
            &open_runner,
            "partial-start",
        )
        .unwrap();

        assert!(warnings.is_empty());
        let (_, after_retry) = config_mgr.load_workspace("partial-start").unwrap();
        let state = stored_terminal_state_table(&after_retry.multiplexer_state);
        assert_eq!(
            state.get("version").and_then(toml::Value::as_integer),
            Some(1)
        );
        assert_eq!(
            state.get("adapter").and_then(toml::Value::as_str),
            Some("cmux")
        );
    }

    #[test]
    fn no_multiplexer_skips_only_start_and_open_can_activate() {
        let tmp = tempfile::tempdir().unwrap();
        let config_mgr = ConfigManager::with_base_dir(tmp.path().join("config"));
        config_mgr.ensure_dirs().unwrap();
        config_mgr
            .save_repo_config("api", &repo_config("/repo/api"))
            .unwrap();
        let workspace_dir = tmp.path().join("workspaces/deferred-terminal");
        let mut workspace = list_workspace(
            WorkspaceStatus::Pending,
            "deferred-terminal",
            "Deferred terminal",
            "zootree/deferred-terminal",
            &workspace_dir.to_string_lossy(),
            vec![repo("api", Some("main"))],
        )
        .workspace;
        workspace.multiplexer.kind = MultiplexerKind::Cmux;
        config_mgr
            .save_workspace(&WorkspaceStatus::Pending, &workspace)
            .unwrap();
        let start_runner = MockRunner::new();
        start_runner.push_response(success_stdout("refs/heads/main\n"));
        start_runner.push_response(success_output());

        let (started, warnings) = start_workspace_and_activate_with(
            &config_mgr,
            &GlobalConfig::default(),
            &start_runner,
            &StartArgs {
                name: Some("deferred-terminal".into()),
                no_multiplexer: true,
                run_agent: None,
            },
        )
        .unwrap();

        assert!(warnings.is_empty());
        assert!(started.multiplexer_state.is_empty());
        assert!(start_runner
            .take_calls()
            .iter()
            .all(|call| call.program == "git"));
        std::fs::create_dir_all(workspace_dir.join("api")).unwrap();
        let open_runner = MockRunner::new();
        open_runner.push_response(success_stdout(
            r#"{"groups":[{"name":"Deferred terminal","ref":"workspace_group:9"}]}"#,
        ));
        open_runner.push_response(success_output());

        open_workspace_with(
            &config_mgr,
            &GlobalConfig::default(),
            &open_runner,
            "deferred-terminal",
        )
        .unwrap();

        let (_, opened) = config_mgr.load_workspace("deferred-terminal").unwrap();
        assert!(!opened.multiplexer_state.is_empty());
        assert_eq!(open_runner.take_calls()[0].program, "cmux");
    }

    #[test]
    fn open_persists_canonical_state_and_returns_reconciliation_warning() {
        let tmp = tempfile::tempdir().unwrap();
        let config_mgr = ConfigManager::with_base_dir(tmp.path().join("config"));
        config_mgr.ensure_dirs().unwrap();
        let mut workspace = test_workspace("warning-state");
        workspace.multiplexer.kind = MultiplexerKind::Cmux;
        workspace.multiplexer_state = stored_terminal_state(
            r#"
version = 99
adapter = "future"

[payload]
identity = "future:1"
"#,
        );
        config_mgr
            .save_workspace(&WorkspaceStatus::InProgress, &workspace)
            .unwrap();
        let runner = MockRunner::new();
        runner.push_response(success_stdout(
            r#"{"groups":[{"name":"warning-state title","ref":"workspace_group:11"}]}"#,
        ));
        runner.push_response(success_output());

        let warnings = open_workspace_with(
            &config_mgr,
            &GlobalConfig::default(),
            &runner,
            "warning-state",
        )
        .unwrap();

        assert!(warnings
            .iter()
            .any(|warning| warning.contains("version 99")));
        let (_, opened) = config_mgr.load_workspace("warning-state").unwrap();
        let state = stored_terminal_state_table(&opened.multiplexer_state);
        assert_eq!(
            state.get("version").and_then(toml::Value::as_integer),
            Some(1)
        );
        assert_eq!(
            state.get("adapter").and_then(toml::Value::as_str),
            Some("cmux")
        );
    }

    #[test]
    fn open_uses_the_same_activation_path_for_zellij() {
        let tmp = tempfile::tempdir().unwrap();
        let config_mgr = ConfigManager::with_base_dir(tmp.path().join("config"));
        config_mgr.ensure_dirs().unwrap();
        let mut workspace = test_workspace("zellij-open");
        workspace.multiplexer.kind = MultiplexerKind::Zellij;
        workspace.multiplexer_state = stored_terminal_state(
            r#"
version = 1
adapter = "zellij"

[payload]
session = "zootree-zellij-open"
"#,
        );
        config_mgr
            .save_workspace(&WorkspaceStatus::InProgress, &workspace)
            .unwrap();
        let runner = MockRunner::new();
        runner.push_response(success_stdout("zootree-zellij-open\n"));
        runner.push_response(success_output());

        open_workspace_with(
            &config_mgr,
            &GlobalConfig::default(),
            &runner,
            "zellij-open",
        )
        .unwrap();

        let calls = runner.take_calls();
        assert_eq!(calls[0].program, "zellij");
        let (_, opened) = config_mgr.load_workspace("zellij-open").unwrap();
        let state = stored_terminal_state_table(&opened.multiplexer_state);
        assert_eq!(
            state.get("adapter").and_then(toml::Value::as_str),
            Some("zellij")
        );
    }

    #[test]
    fn done_archives_before_best_effort_close_and_surfaces_failure() {
        let tmp = tempfile::tempdir().unwrap();
        let config_mgr = ConfigManager::with_base_dir(tmp.path().join("config"));
        config_mgr.ensure_dirs().unwrap();
        let mut workspace = test_workspace("done-close");
        workspace.multiplexer.kind = MultiplexerKind::Cmux;
        workspace.multiplexer_state = stored_terminal_state(
            r#"
version = 1
adapter = "cmux"

[payload]
group = "workspace_group:12"
"#,
        );
        config_mgr
            .save_workspace(&WorkspaceStatus::InProgress, &workspace)
            .unwrap();
        let runner = MockRunner::new();

        let early_warnings = close_terminal_environment_with(
            &config_mgr,
            &GlobalConfig::default(),
            &workspace,
            &runner,
        );
        assert!(early_warnings[0].contains("still in_progress"));
        assert!(runner.take_calls().is_empty());

        runner.push_response(failure_output("delete failed"));
        runner.push_response(failure_output("list failed"));
        let warnings = archive_done_workspace_and_close_with(
            &config_mgr,
            &GlobalConfig::default(),
            &mut workspace,
            &runner,
        )
        .unwrap();

        let (status, archived) = config_mgr.load_workspace("done-close").unwrap();
        assert_eq!(status, WorkspaceStatus::Done);
        assert_eq!(
            archived.events.last().map(|event| event.action.as_str()),
            Some("done")
        );
        assert!(!warnings.is_empty());
        assert!(!runner.take_calls().is_empty());
    }

    #[test]
    fn cancel_archives_before_best_effort_close_and_surfaces_failure() {
        let tmp = tempfile::tempdir().unwrap();
        let config_mgr = ConfigManager::with_base_dir(tmp.path().join("config"));
        config_mgr.ensure_dirs().unwrap();
        let mut workspace = test_workspace("cancel-close");
        workspace.multiplexer.kind = MultiplexerKind::Zellij;
        workspace.multiplexer_state = stored_terminal_state(
            r#"
version = 1
adapter = "zellij"

[payload]
session = "zootree-cancel-close"
"#,
        );
        config_mgr
            .save_workspace(&WorkspaceStatus::InProgress, &workspace)
            .unwrap();
        let runner = MockRunner::new();
        runner.push_response(failure_output("zellij unavailable"));

        let warnings = archive_canceled_workspace_and_close_with(
            &config_mgr,
            &GlobalConfig::default(),
            &WorkspaceStatus::InProgress,
            &mut workspace,
            &runner,
        )
        .unwrap();

        let (status, archived) = config_mgr.load_workspace("cancel-close").unwrap();
        assert_eq!(status, WorkspaceStatus::Canceled);
        assert_eq!(
            archived.events.last().map(|event| event.action.as_str()),
            Some("canceled")
        );
        assert!(warnings
            .iter()
            .any(|warning| warning.contains("zellij unavailable")));
    }

    #[test]
    fn terminal_statuses_are_rejected_before_cancel_archive() {
        for status in [WorkspaceStatus::Done, WorkspaceStatus::Canceled] {
            assert!(
                !is_cancelable_status(&status),
                "terminal status should not be cancelable: {:?}",
                status
            );
        }
    }

    #[test]
    fn warn_or_bail_with_force_returns_ok() {
        let err = anyhow::anyhow!("hook failed");
        let result = warn_or_bail(true, err, "pre_done hook");
        assert!(result.is_ok());
    }

    #[test]
    fn warn_or_bail_without_force_returns_err_with_hint() {
        let err = anyhow::anyhow!("hook failed");
        let result = warn_or_bail(false, err, "pre_done hook");
        assert!(result.is_err());
        let msg = format!("{:#}", result.unwrap_err());
        assert!(
            msg.contains("use --force to proceed anyway"),
            "got: {}",
            msg
        );
    }

    #[test]
    fn ensure_required_worktrees_exist_allows_existing_worktrees() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("zootree")).unwrap();
        let ws = test_workspace("live-clay");
        let mut ws = WorkspaceConfig {
            workspace_dir: tmp.path().to_string_lossy().into_owned(),
            repos: vec![repo("zootree", Some("main"))],
            ..ws
        };

        let result = ensure_required_worktrees_exist(&ws);

        assert!(result.is_ok());
        ws.repos.clear();
    }

    #[test]
    fn ensure_required_worktrees_exist_reports_missing_worktrees() {
        let tmp = tempfile::tempdir().unwrap();
        let ws = test_workspace("live-clay");
        let ws = WorkspaceConfig {
            workspace_dir: tmp.path().to_string_lossy().into_owned(),
            repos: vec![repo("zootree", Some("main"))],
            ..ws
        };

        let err = ensure_required_worktrees_exist(&ws).unwrap_err();

        assert!(
            err.to_string()
                .contains("workspace 'live-clay' is missing worktrees: zootree"),
            "{err:#}"
        );
    }

    #[test]
    fn template_repos_to_entries_input_errors_on_empty_template() {
        let result = template_repos_to_entries_input("empty", Vec::new());
        assert!(result.is_err());
        let msg = format!("{:#}", result.unwrap_err());
        assert!(
            msg.contains("template 'empty' has no repos"),
            "got: {}",
            msg
        );
    }
}
