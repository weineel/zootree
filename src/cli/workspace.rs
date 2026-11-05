use crate::cli::create_flow::{
    create_args_need_wizard, discover_current_repo_candidate, draft_from_args,
    persist_selected_pending_repos, resolve_agent_cli_for_draft, workspace_from_draft,
    AfterCreateMode, CreateDraftError, CreateWizardOutput,
};
use crate::config::global::MultiplexerKind;
use crate::config::template::TemplateConfig;
use crate::config::workspace::{RepoEntry, WorkspaceConfig, WorkspaceStatus};
use crate::config::ConfigManager;
use crate::core::completers::{
    complete_agent_cli_alias, complete_repos_list, complete_template, complete_workspace,
    WorkspaceFilter,
};
use crate::core::git::GitOps;
use crate::core::reopen::{ReopenBase, ReopenOptions, ReopenPrompt, ReopenSources};
use crate::core::repo_status::missing_registered_repo_names;
use crate::core::worktree_status::{missing_worktrees, repo_worktree_statuses, RepoWorktreeStatus};
use crate::runner::RealRunner;
use crate::service::lifecycle::{
    cancel_candidate_statuses, cancel_workspace, done_workspace, CancelRequest, DoneRequest,
    UncommittedConfirmationRequired,
};
use crate::service::reopen::{reopen_workspace, ReopenRequest};
use crate::service::start::{
    open_workspace, start_workspace, OpenRequest, SessionOutcome, StartRequest,
};
use crate::service::{EnvSnapshot, EventSink, RequestContext, ServiceEvent};
use crate::tui;
use crate::tui_app::create_wizard::run_create_wizard;
use anyhow::Result;
use chrono::Local;
use clap::Args;
use clap_complete::ArgValueCompleter;
use std::collections::BTreeSet;
use std::io::IsTerminal;

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
            multiplexer: None,
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
    // Load up front so a broken config fails before any selection prompt.
    config_mgr.load_global_config()?;
    let runner = RealRunner;

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

    let request = StartRequest {
        name,
        multiplexer: args.multiplexer.clone(),
        no_multiplexer: args.no_multiplexer,
        run_agent: args.run_agent.clone(),
    };
    let sink = CliEventSink;
    let ctx = RequestContext::new(&sink, EnvSnapshot::capture());
    let outcome = start_workspace(&config_mgr, &runner, &ctx, &request)?;
    let warning = run_attach(&outcome, &runner, |error| {
        anyhow::anyhow!(
            "workspace '{}' started and remains in_progress, but terminal environment activation failed: {:#}. Run `zootree open {}` to retry",
            outcome.workspace,
            error,
            outcome.workspace
        )
    })?;
    println!("workspace '{}' started", outcome.workspace);
    report_terminal_environment_warnings(&outcome.workspace, warning.into_iter().collect());

    Ok(())
}

/// Performs the foreground attach a service function asked for. A failure the
/// service layer would have wrapped is wrapped by `on_error`.
fn run_attach(
    outcome: &SessionOutcome,
    runner: &RealRunner,
    on_error: impl FnOnce(anyhow::Error) -> anyhow::Error,
) -> Result<Option<String>> {
    match &outcome.attach {
        Some(attach) => attach.run(runner).map_err(on_error),
        None => Ok(None),
    }
}

pub fn handle_list(args: &ListArgs) -> Result<()> {
    let config_mgr = ConfigManager::new()?;

    let status_filter: Vec<WorkspaceStatus> = if args.status.is_empty() {
        vec![WorkspaceStatus::Pending, WorkspaceStatus::InProgress]
    } else {
        args.status.clone()
    };

    let workspaces = config_mgr.list_workspaces_with_status(Some(status_filter.as_slice()))?;

    if args.json {
        let out: Vec<_> = workspaces
            .iter()
            .map(|entry| crate::cli::json_view::workspace_summary(&entry.status, &entry.config))
            .collect();
        println!("{}", crate::cli::json_view::to_pretty_json(&out)?);
        return Ok(());
    }

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

pub fn handle_open(args: &OpenArgs) -> Result<()> {
    let config_mgr = ConfigManager::new()?;
    config_mgr.load_global_config()?;
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

    let sink = CliEventSink;
    let ctx = RequestContext::new(&sink, EnvSnapshot::capture());
    let outcome = open_workspace(&config_mgr, &runner, &ctx, &OpenRequest { name })?;
    let warning = run_attach(&outcome, &runner, |error| error)?;
    report_terminal_environment_warnings(&outcome.workspace, warning.into_iter().collect());
    Ok(())
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
    #[arg(
        long,
        value_enum,
        help = "Select and save the terminal multiplexer for this workspace"
    )]
    pub multiplexer: Option<MultiplexerKind>,
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

    #[arg(long, conflicts_with = "oneline", help = "Print machine-readable JSON")]
    pub json: bool,
}

#[derive(Args)]
pub struct StartArgs {
    #[arg(
        long,
        value_enum,
        help = "Select and save the terminal multiplexer for this workspace"
    )]
    pub multiplexer: Option<MultiplexerKind>,
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
        long,
        value_enum,
        help = "Select and save the terminal multiplexer for this workspace"
    )]
    pub multiplexer: Option<MultiplexerKind>,
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
    config_manager.load_global_config()?;
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
    let request = ReopenRequest {
        name,
        options: ReopenOptions {
            multiplexer: args.multiplexer.clone(),
            sources: ReopenSources::parse(&args.from)?,
            overwrite_repos: args.overwrite.iter().cloned().collect::<BTreeSet<_>>(),
        },
        run_agent: args.run_agent.clone(),
        skip_hooks: args.skip_hooks,
        no_multiplexer: args.no_multiplexer,
        dry_run: args.dry_run,
    };
    let mut prompt = CliReopenPrompt { interactive };
    let sink = CliEventSink;
    let ctx = RequestContext::new(&sink, EnvSnapshot::capture());
    let outcome = reopen_workspace(&config_manager, &runner, &ctx, &mut prompt, &request)?;
    if outcome.dry_run {
        return Ok(());
    }
    let warning = run_attach(&outcome, &runner, |error| {
        anyhow::anyhow!(
            "workspace '{}' reopened and remains in_progress, but terminal environment activation failed: {error:#}. Run `zootree open {}` to retry",
            outcome.workspace,
            outcome.workspace
        )
    })?;
    println!("workspace '{}' reopened", outcome.workspace);
    report_terminal_environment_warnings(&outcome.workspace, warning.into_iter().collect());
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

/// CLI adapter for service events: steps go to stdout, warnings to the log.
struct CliEventSink;

impl EventSink for CliEventSink {
    fn emit(&self, event: ServiceEvent) {
        match event {
            ServiceEvent::Started { .. } => {}
            ServiceEvent::Step(message) | ServiceEvent::Log(message) => println!("{message}"),
            ServiceEvent::Warning(message) => tracing::warn!("{}", message),
            ServiceEvent::Finished { summary, .. } => println!("{summary}"),
        }
    }
}

pub fn handle_done(args: &DoneArgs) -> Result<()> {
    let config_mgr = ConfigManager::new()?;
    // Load up front so a broken config fails before any selection prompt.
    config_mgr.load_global_config()?;

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

    let request = DoneRequest {
        name,
        no_merge: args.no_merge,
        no_clean: args.no_clean,
        push: args.push,
        strategy: args.strategy.map(|s| s.as_str().to_string()),
        force: args.force,
        skip_hooks: args.skip_hooks,
        dry_run: args.dry_run,
    };
    let sink = CliEventSink;
    let ctx = RequestContext::new(&sink, EnvSnapshot::capture());
    done_workspace(&config_mgr, &RealRunner, &ctx, &request)?;
    Ok(())
}

pub fn handle_cancel(args: &CancelArgs) -> Result<()> {
    let config_mgr = ConfigManager::new()?;
    config_mgr.load_global_config()?;

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

    let mut request = CancelRequest {
        name,
        no_clean: args.no_clean,
        force: args.force,
        skip_hooks: args.skip_hooks,
        confirmed_uncommitted: false,
    };
    let sink = CliEventSink;
    let ctx = RequestContext::new(&sink, EnvSnapshot::capture());
    match cancel_workspace(&config_mgr, &RealRunner, &ctx, &request) {
        Err(error) => {
            let Some(needed) = error.downcast_ref::<UncommittedConfirmationRequired>() else {
                return Err(error);
            };
            for repo in &needed.repos {
                if !tui::confirm(
                    &format!("repo '{}' has uncommitted changes. Continue?", repo),
                    false,
                )? {
                    anyhow::bail!("canceled by user");
                }
            }
            request.confirmed_uncommitted = true;
            cancel_workspace(&config_mgr, &RealRunner, &ctx, &request)?;
            Ok(())
        }
        Ok(_) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::global::{GlobalConfig, MultiplexerConfig, MultiplexerKind};
    use crate::config::workspace::StoredTerminalEnvironmentState;
    use crate::core::worktree_status::ensure_required_worktrees_exist;
    use crate::runner::MockRunner;
    use crate::service::lifecycle::{
        archive_canceled_workspace, archive_canceled_workspace_and_close_with,
        archive_done_workspace_and_close_with, cancel_repo_worktree_decision,
        close_terminal_environment_with, is_cancelable_status, warn_or_bail,
        CancelRepoWorktreeDecision,
    };
    use clap::Parser;
    use std::os::unix::process::ExitStatusExt;
    use std::process::{ExitStatus, Output};

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

    fn missing_worktree(repo_name: &str, worktree_path: &str) -> RepoWorktreeStatus {
        RepoWorktreeStatus {
            repo_name: repo_name.into(),
            worktree_path: worktree_path.into(),
            exists: false,
        }
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
        let sink = crate::service::context::CollectingSink::new();
        let ctx = RequestContext::new(&sink, EnvSnapshot::default());
        let err = anyhow::anyhow!("hook failed");
        let result = warn_or_bail(&ctx, true, err, "pre_done hook");
        assert!(result.is_ok());
    }

    #[test]
    fn warn_or_bail_without_force_returns_err_with_hint() {
        let sink = crate::service::context::CollectingSink::new();
        let ctx = RequestContext::new(&sink, EnvSnapshot::default());
        let err = anyhow::anyhow!("hook failed");
        let result = warn_or_bail(&ctx, false, err, "pre_done hook");
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
