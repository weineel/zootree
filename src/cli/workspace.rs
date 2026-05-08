use clap::Args;
use crate::config::ConfigManager;
use crate::config::workspace::{WorkspaceConfig, WorkspaceStatus, RepoEntry, Event};
use crate::config::global::ZellijConfig;
use crate::config::template::TemplateConfig;
use crate::core::name_gen::NameGenerator;
use crate::core::git::GitOps;
use crate::core::hook::{HookEngine, HookContext};
use crate::core::copy_files;
use crate::core::layout::{LayoutRenderer, LayoutVar};
use crate::core::zellij::ZellijOps;
use crate::runner::RealRunner;
use crate::tui;
use anyhow::Result;
use chrono::Local;
use std::path::Path;

pub fn parse_repos_arg(repos_str: &str) -> Vec<(String, Option<String>)> {
    repos_str.split(',')
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

pub fn handle_create(args: &CreateArgs) -> Result<()> {
    let config_mgr = ConfigManager::new()?;
    config_mgr.ensure_dirs()?;
    let global = config_mgr.load_global_config()?;
    let runner = RealRunner;
    let git = GitOps::new(&runner);

    let title = match &args.title {
        Some(t) => t.clone(),
        None => tui::input_required("Title")?,
    };

    let description = match &args.description {
        Some(d) => d.clone(),
        None => tui::input_optional("Description (optional)")?.unwrap_or_default(),
    };

    let repo_entries = if let Some(repos_str) = &args.repos {
        let parsed = parse_repos_arg(repos_str);
        let mut entries = Vec::new();
        for (name, branch) in parsed {
            let repo_config = config_mgr.load_repo_config(&name)?;
            let repo_path = shellexpand::tilde(&repo_config.path).into_owned();
            let target_branch = branch
                .or(repo_config.default_target_branch.clone())
                .unwrap_or_else(|| git.current_branch(&repo_path).unwrap_or_else(|_| "main".into()));
            entries.push(RepoEntry { name, target_branch: Some(target_branch) });
        }
        entries
    } else {
        let _template_repos = if let Some(tmpl_name) = &args.template {
            let tmpl = config_mgr.load_template(tmpl_name)?;
            Some(tmpl.repos)
        } else {
            None
        };

        let all_repos = config_mgr.list_repos()?;
        if all_repos.is_empty() {
            anyhow::bail!("no repos registered. Use 'zootree repo add' first.");
        }

        let selected = tui::select_multi("Select repos", &all_repos)?;
        if selected.is_empty() {
            anyhow::bail!("at least one repo must be selected");
        }

        let mut entries = Vec::new();
        for idx in selected {
            let name = &all_repos[idx];
            let repo_config = config_mgr.load_repo_config(name)?;

            let repo_path = shellexpand::tilde(&repo_config.path).into_owned();
            let current = git.current_branch(&repo_path).unwrap_or_else(|_| "main".into());
            let target_branch = if let Some(default) = &repo_config.default_target_branch {
                default.clone()
            } else {
                let input = tui::input_optional(&format!("Target branch for {} (default: {})", name, current))?;
                input.unwrap_or(current)
            };

            entries.push(RepoEntry {
                name: name.clone(),
                target_branch: Some(target_branch),
            });
        }
        entries
    };

    let name_gen = NameGenerator::new();
    let existing: Vec<String> = config_mgr.list_workspaces(None::<&[WorkspaceStatus]>)?
        .iter().map(|w| w.name.clone()).collect();
    let name = match &args.name {
        Some(n) => n.clone(),
        None => name_gen.generate_avoiding(&existing),
    };

    let branch = match &args.branch {
        Some(b) => b.clone(),
        None => format!("{}/{}", global.branch_prefix, name),
    };

    let workspace_dir = format!(
        "{}/{}",
        shellexpand::tilde(&global.workspace_root),
        name
    );

    let now = Local::now().to_rfc3339();

    let workspace = WorkspaceConfig {
        title,
        name: name.clone(),
        description,
        branch,
        workspace_dir,
        created_at: now.clone(),
        zellij: ZellijConfig {
            session_mode: Some("standalone".into()),
            ..Default::default()
        },
        repos: repo_entries,
        events: vec![Event {
            action: "created".into(),
            timestamp: now,
            detail: None,
        }],
    };

    config_mgr.save_workspace(&WorkspaceStatus::Pending, &workspace)?;

    let recently = TemplateConfig {
        repos: workspace.repos.iter().map(|r| r.name.clone()).collect(),
        zellij: workspace.zellij.clone(),
    };
    config_mgr.save_template("recently", &recently)?;

    println!("workspace '{}' created (pending)", name);
    println!("  branch: {}", workspace.branch);
    println!("  repos: {}", workspace.repos.iter().map(|r| format!("{}:{}", r.name, r.target_branch.as_deref().unwrap_or("*"))).collect::<Vec<_>>().join(", "));

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
            let names: Vec<String> = pending.iter().map(|w| format!("{} - {}", w.name, w.title)).collect();
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

    for repo_entry in &workspace.repos {
        let repo_config = config_mgr.load_repo_config(&repo_entry.name)?;
        let repo_path = shellexpand::tilde(&repo_config.path).into_owned();

        let target_branch = match &repo_entry.target_branch {
            Some(tb) if git.branch_exists(&repo_path, tb)? => tb.clone(),
            Some(tb) => {
                let current = git.current_branch(&repo_path)?;
                tracing::warn!(
                    "target branch '{}' not found in repo '{}', using current branch '{}'",
                    tb, repo_entry.name, current
                );
                current
            }
            None => {
                let current = git.current_branch(&repo_path)?;
                tracing::warn!(
                    "target branch not configured for repo '{}', using current branch '{}'",
                    repo_entry.name, current
                );
                current
            }
        };

        let worktree_path = format!("{}/{}", ws_dir, repo_entry.name);

        tracing::info!("creating worktree for {} at {}", repo_entry.name, worktree_path);
        git.worktree_add(&repo_path, &workspace.branch, &worktree_path, &target_branch)?;

        let patterns = copy_files::merge_copy_files(&global.copy_files, &repo_config.copy_files);
        if !patterns.is_empty() {
            copy_files::copy_files_to_worktree(
                Path::new(&repo_path),
                Path::new(&worktree_path),
                &patterns,
            )?;
        }

        let hook = repo_config.hooks.post_create.as_ref()
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
    config_mgr.move_workspace(&name, &WorkspaceStatus::Pending, &WorkspaceStatus::InProgress)?;

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
        launch_zellij(&config_mgr, &global, &workspace, &runner)?;
    }

    Ok(())
}

pub fn handle_list(args: &ListArgs) -> Result<()> {
    let config_mgr = ConfigManager::new()?;

    let status_filter: Vec<WorkspaceStatus> = if args.status.is_empty() {
        vec![WorkspaceStatus::Pending, WorkspaceStatus::InProgress]
    } else {
        args.status.iter().map(|s| parse_status(s)).collect::<Result<Vec<_>>>()?
    };

    let workspaces = config_mgr.list_workspaces(Some(status_filter.as_slice()))?;

    if workspaces.is_empty() {
        println!("no workspaces found");
        return Ok(());
    }

    for ws in &workspaces {
        let (status, _) = config_mgr.load_workspace(&ws.name)?;
        let status_str = format_status(&status);
        let repos_str = ws.repos.iter()
            .map(|r| format!("{}:{}", r.name, r.target_branch.as_deref().unwrap_or("*")))
            .collect::<Vec<_>>()
            .join(", ");
        if matches!(status, WorkspaceStatus::InProgress) {
            println!("  {} ({}) - {} [{}] {}", ws.name, status_str, ws.title, repos_str, ws.workspace_dir);
        } else {
            println!("  {} ({}) - {} [{}]", ws.name, status_str, ws.title, repos_str);
        }
    }

    Ok(())
}

fn parse_status(s: &str) -> Result<WorkspaceStatus> {
    match s {
        "pending" => Ok(WorkspaceStatus::Pending),
        "in_progress" => Ok(WorkspaceStatus::InProgress),
        "done" => Ok(WorkspaceStatus::Done),
        "canceled" => Ok(WorkspaceStatus::Canceled),
        _ => anyhow::bail!("invalid status '{}', available: pending, in_progress, done, canceled", s),
    }
}

fn format_status(status: &WorkspaceStatus) -> &'static str {
    match status {
        WorkspaceStatus::Pending => "pending",
        WorkspaceStatus::InProgress => "in_progress",
        WorkspaceStatus::Done => "done",
        WorkspaceStatus::Canceled => "canceled",
    }
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
            let names: Vec<String> = in_progress.iter().map(|w| format!("{} - {}", w.name, w.title)).collect();
            let idx = tui::select_one("Select workspace to open", &names)?;
            in_progress[idx].name.clone()
        }
    };

    let (status, workspace) = config_mgr.load_workspace(&name)?;
    if !matches!(status, WorkspaceStatus::InProgress) {
        anyhow::bail!("workspace '{}' is not in_progress", name);
    }

    launch_zellij(&config_mgr, &global, &workspace, &runner)?;
    Ok(())
}

fn write_default_layout(base_dir: &Path) -> String {
    let content = LayoutRenderer::default_layout().to_string();
    let path = base_dir.join("layouts").join("default.kdl");
    let _ = std::fs::create_dir_all(path.parent().unwrap());
    let _ = std::fs::write(&path, &content);
    content
}

fn launch_zellij(
    config_mgr: &ConfigManager,
    global: &crate::config::global::GlobalConfig,
    workspace: &WorkspaceConfig,
    runner: &RealRunner,
) -> Result<()> {
    if std::env::var("ZELLIJ").is_ok() {
        anyhow::bail!(
            "already inside a zellij session (ZELLIJ is set); cannot start a new session. \
             Use a regular terminal to run 'zootree start'"
        );
    }

    let zellij = ZellijOps::new(runner);

    let layout_name = workspace.zellij.layout.as_deref()
        .or(global.zellij.layout.as_deref())
        .unwrap_or("default");

    let template_content = if layout_name == "default" {
        write_default_layout(&config_mgr.base_dir)
    } else {
        let layout_path = config_mgr.base_dir.join("layouts").join(format!("{}.kdl", layout_name));
        if layout_path.exists() {
            std::fs::read_to_string(&layout_path)?
        } else {
            write_default_layout(&config_mgr.base_dir)
        }
    };

    let ws_dir = shellexpand::tilde(&workspace.workspace_dir).into_owned();
    let mut vars = Vec::new();
    for repo_entry in &workspace.repos {
        let repo_config = config_mgr.load_repo_config(&repo_entry.name)?;
        let lazygit_config = repo_config.lazygit
            .map(|lg| lg.config)
            .unwrap_or_default();

        vars.push(LayoutVar {
            repo_name: repo_entry.name.clone(),
            worktree_path: format!("{}/{}", ws_dir, repo_entry.name),
            branch: workspace.branch.clone(),
            workspace_name: workspace.name.clone(),
            workspace_dir: ws_dir.clone(),
            lazygit_config,
        });
    }

    let rendered = LayoutRenderer::render(&template_content, &vars);

    let layout_dir = config_mgr.base_dir.join("layouts");
    std::fs::create_dir_all(&layout_dir)?;
    let layout_file = layout_dir.join("recently.kdl");
    std::fs::write(&layout_file, &rendered)?;

    let session_name = match workspace.zellij.session_mode.as_deref() {
        Some("shared") => workspace.zellij.session_name.clone()
            .ok_or_else(|| anyhow::anyhow!("shared mode requires session_name"))?,
        _ => format!("zootree-{}", workspace.name),
    };

    match zellij.start_session(&session_name, &layout_file) {
        Ok(()) => {}
        Err(e) => {
            tracing::warn!("start_session failed ({}), trying attach", e);
            zellij.attach_session(&session_name)?;
        }
    }

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
    #[arg(long, help = "Comma-separated repos, optionally with branch: repo1:branch1,repo2")]
    pub repos: Option<String>,
    #[arg(long, help = "Git branch name for worktrees (defaults to <prefix>/<name>)")]
    pub branch: Option<String>,
    #[arg(long, help = "Template name to use for repo selection")]
    pub template: Option<String>,
}

#[derive(Args)]
pub struct ListArgs {
    #[arg(long, help = "Filter by status [available: pending, in_progress, done, canceled]")]
    pub status: Vec<String>,
}

#[derive(Args)]
pub struct StartArgs {
    #[arg(help = "Workspace name to start (interactive if omitted)")]
    pub name: Option<String>,
    #[arg(long, help = "Skip launching zellij session after start")]
    pub no_zellij: bool,
}

#[derive(Args)]
pub struct OpenArgs {
    #[arg(help = "Workspace name to open (interactive if omitted)")]
    pub name: Option<String>,
}

#[derive(Args)]
pub struct DoneArgs {
    #[arg(help = "Workspace name to complete (interactive if omitted)")]
    pub name: Option<String>,
    #[arg(long, help = "Skip merging branches back to target")]
    pub no_merge: bool,
    #[arg(long, help = "Keep worktrees and workspace directory")]
    pub no_clean: bool,
    #[arg(long, help = "Push target branch to remote after merge")]
    pub push: bool,
    #[arg(long, help = "Delete remote feature branch after merge")]
    pub delete_remote: bool,
    #[arg(long, help = "Merge strategy, available: squash(default), rebase, merge")]
    pub strategy: Option<String>,
    #[arg(long, help = "Continue even if steps fail (errors become warnings)")]
    pub force: bool,
    #[arg(long, help = "Skip all hooks (pre_done/pre_remove)")]
    pub skip_hooks: bool,
    #[arg(long, help = "Show what would be done without executing")]
    pub dry_run: bool,
}

#[derive(Args)]
pub struct CancelArgs {
    #[arg(help = "Workspace name to cancel (interactive if omitted)")]
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
            let names: Vec<String> = in_progress.iter().map(|w| format!("{} - {}", w.name, w.title)).collect();
            let idx = tui::select_one("Select workspace to complete", &names)?;
            in_progress[idx].name.clone()
        }
    };

    let (status, mut workspace) = config_mgr.load_workspace(&name)?;
    if !matches!(status, WorkspaceStatus::InProgress) {
        anyhow::bail!("workspace '{}' is not in_progress", name);
    }

    let ws_dir = shellexpand::tilde(&workspace.workspace_dir).into_owned();

    if args.dry_run {
        println!("dry run for workspace '{}':", name);
        if !args.no_merge {
            for repo_entry in &workspace.repos {
                println!("  merge {} -> {}", workspace.branch, repo_entry.target_branch.as_deref().unwrap_or("*"));
            }
        }
        if !args.no_clean {
            println!("  clean worktrees and workspace directory");
        }
        return Ok(());
    }

    // pre_done hook
    if !args.skip_hooks {
        if let Err(e) = hook_engine.execute_if_set(&global.hooks.pre_done, &HookContext {
            workspace: workspace.name.clone(),
            repo: None,
            branch: workspace.branch.clone(),
            target_branch: None,
            worktree_path: None,
            workspace_dir: ws_dir.clone(),
        }) {
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
                    tb, repo_entry.name, current
                );
                current
            }
            None => {
                let current = git.current_branch(&repo_path)?;
                tracing::warn!(
                    "target branch not configured for repo '{}', using current branch '{}'",
                    repo_entry.name, current
                );
                current
            }
        };

        // Check uncommitted changes
        if git.has_uncommitted_changes(&worktree_path)? {
            if !args.force {
                anyhow::bail!(
                    "repo '{}' has uncommitted changes in {}. Commit or stash first, or use --force",
                    repo_entry.name, worktree_path
                );
            }
        }

        // Merge
        if !args.no_merge {
            let strategy = args.strategy.as_deref();
            let message = if workspace.description.is_empty() {
                workspace.title.clone()
            } else {
                format!("{}\n\n{}", workspace.title, workspace.description)
            };
            git.merge(&repo_path, &workspace.branch, &target_branch, strategy, &message)?;
            println!("  merged {} -> {} ({})", workspace.branch, target_branch, repo_entry.name);

            if args.push {
                git.push(&repo_path, &target_branch)?;
                println!("  pushed {} ({})", target_branch, repo_entry.name);
            }

            if args.delete_remote {
                git.delete_remote_branch(&repo_path, &workspace.branch)?;
                println!("  deleted remote branch {} ({})", workspace.branch, repo_entry.name);
            }
        }

        // Clean
        if !args.no_clean {
            let hook = repo_config.hooks.pre_remove.as_ref()
                .or(global.hooks.pre_remove.as_ref());
            if let Some(h) = hook {
                if !args.skip_hooks {
                    if let Err(e) = hook_engine.execute(h, &HookContext {
                        workspace: workspace.name.clone(),
                        repo: Some(repo_entry.name.clone()),
                        branch: workspace.branch.clone(),
                        target_branch: Some(target_branch.clone()),
                        worktree_path: Some(worktree_path.clone()),
                        workspace_dir: ws_dir.clone(),
                    }) {
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
    if !args.no_clean {
        if Path::new(&ws_dir).exists() {
            if let Err(e) = std::fs::remove_dir_all(&ws_dir) {
                warn_or_bail(args.force, e.into(), "failed to remove workspace directory")?;
            }
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
            let in_progress = config_mgr.list_workspaces(Some(&[WorkspaceStatus::InProgress]))?;
            if in_progress.is_empty() {
                anyhow::bail!("no in_progress workspaces");
            }
            let names: Vec<String> = in_progress.iter().map(|w| format!("{} - {}", w.name, w.title)).collect();
            let idx = tui::select_one("Select workspace to cancel", &names)?;
            in_progress[idx].name.clone()
        }
    };

    let (status, mut workspace) = config_mgr.load_workspace(&name)?;
    if !matches!(status, WorkspaceStatus::InProgress) {
        anyhow::bail!("workspace '{}' is not in_progress", name);
    }

    let ws_dir = shellexpand::tilde(&workspace.workspace_dir).into_owned();

    // Confirm if uncommitted changes exist
    if !args.force {
        for repo_entry in &workspace.repos {
            let worktree_path = format!("{}/{}", ws_dir, repo_entry.name);
            if Path::new(&worktree_path).exists() && git.has_uncommitted_changes(&worktree_path)? {
                if !tui::confirm(
                    &format!("repo '{}' has uncommitted changes. Continue?", repo_entry.name),
                    false,
                )? {
                    anyhow::bail!("canceled by user");
                }
            }
        }
    }

    // pre_cancel hook
    if !args.force {
        hook_engine.execute_if_set(&global.hooks.pre_cancel, &HookContext {
            workspace: workspace.name.clone(),
            repo: None,
            branch: workspace.branch.clone(),
            target_branch: None,
            worktree_path: None,
            workspace_dir: ws_dir.clone(),
        })?;
    }

    if !args.no_clean {
        for repo_entry in &workspace.repos {
            let repo_config = config_mgr.load_repo_config(&repo_entry.name)?;
            let repo_path = shellexpand::tilde(&repo_config.path).into_owned();
            let worktree_path = format!("{}/{}", ws_dir, repo_entry.name);

            // pre_remove hook
            let hook = repo_config.hooks.pre_remove.as_ref()
                .or(global.hooks.pre_remove.as_ref());
            if let Some(h) = hook {
                if !args.force {
                    let _ = hook_engine.execute(h, &HookContext {
                        workspace: workspace.name.clone(),
                        repo: Some(repo_entry.name.clone()),
                        branch: workspace.branch.clone(),
                        target_branch: repo_entry.target_branch.clone(),
                        worktree_path: Some(worktree_path.clone()),
                        workspace_dir: ws_dir.clone(),
                    });
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
            std::fs::remove_dir_all(&ws_dir)?;
        }
    }

    // Archive
    let now = Local::now().to_rfc3339();
    workspace.events.push(Event {
        action: "canceled".into(),
        timestamp: now,
        detail: None,
    });
    config_mgr.save_workspace(&WorkspaceStatus::InProgress, &workspace)?;
    config_mgr.move_workspace(&name, &WorkspaceStatus::InProgress, &WorkspaceStatus::Canceled)?;

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
        assert!(msg.contains("use --force to proceed anyway"), "got: {}", msg);
    }
}
