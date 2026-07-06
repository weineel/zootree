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
use crate::core::layout::{LayoutRenderer, LayoutVar};
use crate::core::repo_status::missing_registered_repo_names;
use crate::core::worktree_status::{
    format_missing_worktrees_error, missing_worktrees, repo_worktree_statuses, RepoWorktreeStatus,
};
use crate::core::zellij::ZellijOps;
use crate::runner::RealRunner;
use crate::tui;
use crate::tui_app::create_wizard::run_create_wizard;
use anyhow::Result;
use chrono::Local;
use clap::Args;
use clap_complete::ArgValueCompleter;
use std::path::Path;

#[cfg(test)]
#[allow(unused_imports)]
use crate::config::global::ZellijConfig;

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
    let workspace = workspace_from_draft(&output.draft, Local::now().to_rfc3339(), agent_cli);
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
        zellij: workspace.zellij.clone(),
    };
    config_mgr.save_template("recently", &recently)
}

fn start_after_create_if_needed(name: &str, mode: &AfterCreateMode) -> Result<()> {
    if mode.should_start() {
        let start_args = StartArgs {
            name: Some(name.to_string()),
            no_zellij: false,
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
    let git = GitOps::new(&runner);
    let hook_engine = HookEngine::new(&runner);

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
    std::fs::create_dir_all(&ws_dir)?;

    if args.run_agent.is_some() {
        workspace.agent_cli = selected_agent_cli_value(&args.run_agent, &global)?;
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

        let patterns = copy_files::merge_copy_files(&global.copy_files, &repo_config.copy_files);
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

    let now = Local::now().to_rfc3339();
    workspace.events.push(Event {
        action: "started".into(),
        timestamp: now,
        detail: None,
    });
    config_mgr.save_workspace(&WorkspaceStatus::Pending, &workspace)?;
    config_mgr.move_workspace(
        &name,
        &WorkspaceStatus::Pending,
        &WorkspaceStatus::InProgress,
    )?;

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

    println!("workspace '{}' started", name);

    if !args.no_zellij {
        launch_zellij(
            &config_mgr,
            &global,
            &workspace,
            &runner,
            args.run_agent.clone(),
            crate::core::zellij::is_inside_zellij(),
        )?;
    }

    Ok(())
}

pub fn handle_list(args: &ListArgs) -> Result<()> {
    let config_mgr = ConfigManager::new()?;

    let status_filter: Vec<WorkspaceStatus> = if args.status.is_empty() {
        vec![WorkspaceStatus::Pending, WorkspaceStatus::InProgress]
    } else {
        args.status.clone()
    };

    let workspaces = config_mgr.list_workspaces(Some(status_filter.as_slice()))?;

    if workspaces.is_empty() {
        println!("no workspaces found");
        return Ok(());
    }

    let mut items = Vec::with_capacity(workspaces.len());
    for ws in workspaces {
        let (status, _) = config_mgr.load_workspace(&ws.name)?;
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

    let (status, workspace) = config_mgr.load_workspace(&name)?;
    if !matches!(status, WorkspaceStatus::InProgress) {
        anyhow::bail!("workspace '{}' is not in_progress", name);
    }

    ensure_required_worktrees_exist(&workspace)?;

    launch_zellij(
        &config_mgr,
        &global,
        &workspace,
        &runner,
        None,
        crate::core::zellij::is_inside_zellij(),
    )?;
    Ok(())
}

fn write_default_layout(base_dir: &Path) -> String {
    let content = LayoutRenderer::default_layout().to_string();
    let path = base_dir.join("layouts").join("default.kdl");
    let _ = std::fs::create_dir_all(path.parent().unwrap());
    let _ = std::fs::write(&path, &content);
    content
}

pub fn dispatch_launch<R: crate::runner::CommandRunner>(
    zellij: &ZellijOps<'_, R>,
    workspace_name: &str,
    session_name: &str,
    layout_file: &std::path::Path,
    in_zellij: bool,
) -> Result<()> {
    let session_exists = zellij.session_exists(session_name)?;
    let plan = crate::core::zellij::plan_launch(in_zellij, session_exists);

    match plan {
        crate::core::zellij::LaunchPlan::ForegroundCreate => {
            zellij.start_session(session_name, layout_file)?;
        }
        crate::core::zellij::LaunchPlan::ForegroundAttach => {
            zellij.attach_session(session_name)?;
        }
        crate::core::zellij::LaunchPlan::BackgroundCreate => {
            zellij.start_session_background(session_name, layout_file)?;
            println!(
                "zellij session '{}' is running in background.",
                session_name
            );
            println!(
                "Run `zootree open {}` (outside zellij) to attach.",
                workspace_name
            );
        }
        crate::core::zellij::LaunchPlan::AlreadyRunningHint => {
            println!("zellij session '{}' already exists.", session_name);
            println!(
                "Run `zootree open {}` (outside zellij) to attach.",
                workspace_name
            );
        }
    }

    Ok(())
}

fn launch_zellij(
    config_mgr: &ConfigManager,
    global: &crate::config::global::GlobalConfig,
    workspace: &WorkspaceConfig,
    runner: &RealRunner,
    run_agent: Option<Option<String>>,
    in_zellij: bool,
) -> Result<()> {
    let zellij = ZellijOps::new(runner);

    let layout_name = workspace
        .zellij
        .layout
        .as_deref()
        .or(global.zellij.layout.as_deref())
        .unwrap_or("default");

    let template_content = if layout_name == "default" {
        write_default_layout(&config_mgr.base_dir)
    } else {
        let layout_path = config_mgr
            .base_dir
            .join("layouts")
            .join(format!("{}.kdl", layout_name));
        if layout_path.exists() {
            std::fs::read_to_string(&layout_path)?
        } else {
            write_default_layout(&config_mgr.base_dir)
        }
    };

    let ws_dir = shellexpand::tilde(&workspace.workspace_dir).into_owned();

    let agent_cli_tpl: Option<String> = match run_agent.as_ref() {
        None => None,
        Some(value) => {
            let raw: String = match value.as_deref() {
                Some(s) if !s.is_empty() => s.to_string(),
                _ => global.agent_cli.clone().ok_or_else(|| {
                    anyhow::anyhow!(
                        "--run-agent requires agent_cli in global config (~/.config/zootree/config.toml)"
                    )
                })?,
            };
            let resolved = crate::core::layout::resolve_agent_cli(&raw, &global.agent_cli_alias);
            Some(resolved.to_string())
        }
    };

    let (overview_kdl, repo_kdl_for_first) = match agent_cli_tpl.as_deref() {
        None => (String::new(), String::new()),
        Some(tpl) => {
            let prompt = crate::core::layout::build_prompt(workspace);
            let kdl = crate::core::layout::build_agent_cli_kdl(tpl, &prompt)?;
            if workspace.repos.len() == 1 {
                (String::new(), kdl)
            } else {
                (kdl, String::new())
            }
        }
    };

    let mut vars = Vec::new();
    for (i, repo_entry) in workspace.repos.iter().enumerate() {
        let repo_config = config_mgr.load_repo_config(&repo_entry.name)?;
        let lazygit_config = repo_config.lazygit.map(|lg| lg.config).unwrap_or_default();

        let repo_agent_cli = if i == 0 {
            repo_kdl_for_first.clone()
        } else {
            String::new()
        };

        vars.push(LayoutVar {
            repo_name: repo_entry.name.clone(),
            worktree_path: format!("{}/{}", ws_dir, repo_entry.name),
            branch: workspace.branch.clone(),
            workspace_name: workspace.name.clone(),
            workspace_dir: ws_dir.clone(),
            lazygit_config,
            overview_agent_cli: overview_kdl.clone(),
            repo_agent_cli,
        });
    }

    let rendered = LayoutRenderer::render(&template_content, &vars);

    if run_agent.is_some()
        && !template_content.contains("$overview_agent_cli")
        && !template_content.contains("$repo_agent_cli")
    {
        tracing::warn!(
            "--run-agent is set but layout '{}' contains no $overview_agent_cli or $repo_agent_cli placeholder; agent_cli will not be executed",
            layout_name
        );
    }

    let layout_dir = config_mgr.base_dir.join("layouts");
    std::fs::create_dir_all(&layout_dir)?;
    let layout_file = layout_dir.join("recently.kdl");
    std::fs::write(&layout_file, &rendered)?;

    let session_name = match workspace.zellij.session_mode.as_deref() {
        Some("shared") => workspace
            .zellij
            .session_name
            .clone()
            .ok_or_else(|| anyhow::anyhow!("shared mode requires session_name"))?,
        _ => format!("zootree-{}", workspace.name),
    };

    dispatch_launch(
        &zellij,
        &workspace.name,
        &session_name,
        &layout_file,
        in_zellij,
    )?;
    Ok(())
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
    #[arg(long, help = "Skip launching zellij session after start")]
    pub no_zellij: bool,
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
    let zellij = ZellijOps::new(&runner);

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

    // Archive
    let now = Local::now().to_rfc3339();
    workspace.events.push(Event {
        action: "done".into(),
        timestamp: now,
        detail: None,
    });
    config_mgr.save_workspace(&WorkspaceStatus::InProgress, &workspace)?;
    config_mgr.move_workspace(&name, &WorkspaceStatus::InProgress, &WorkspaceStatus::Done)?;

    // Kill zellij session
    let session_name = match workspace.zellij.session_mode.as_deref() {
        Some("shared") => workspace.zellij.session_name.clone(),
        _ => Some(format!("zootree-{}", workspace.name)),
    };
    if let Some(sn) = &session_name {
        if let Err(e) = zellij.kill_session(sn) {
            tracing::warn!("failed to kill zellij session '{}': {}", sn, e);
        }
    }

    println!("workspace '{}' completed", name);
    Ok(())
}

pub fn handle_cancel(args: &CancelArgs) -> Result<()> {
    let config_mgr = ConfigManager::new()?;
    let global = config_mgr.load_global_config()?;
    let runner = RealRunner;
    let git = GitOps::new(&runner);
    let hook_engine = HookEngine::new(&runner);
    let zellij = ZellijOps::new(&runner);

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

    // Archive
    archive_canceled_workspace(&config_mgr, &status, &mut workspace)?;

    // Kill zellij session
    let session_name = match workspace.zellij.session_mode.as_deref() {
        Some("shared") => workspace.zellij.session_name.clone(),
        _ => Some(format!("zootree-{}", workspace.name)),
    };
    if let Some(sn) = &session_name {
        if let Err(e) = zellij.kill_session(sn) {
            tracing::warn!("failed to kill zellij session '{}': {}", sn, e);
        }
    }

    println!("workspace '{}' canceled", name);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Parser)]
    struct TestListCli {
        #[command(flatten)]
        args: ListArgs,
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
                zellij: ZellijConfig::default(),
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
            zellij: None,
        }
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
            zellij: ZellijConfig::default(),
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
