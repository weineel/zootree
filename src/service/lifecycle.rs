//! Done and cancel as service functions.

use crate::config::global::GlobalConfig;
use crate::config::workspace::{Event, RepoEntry, WorkspaceConfig, WorkspaceStatus};
use crate::config::ConfigManager;
use crate::core::git::GitOps;
use crate::core::hook::{
    HookEngine, HookInvocation, HookOperation, HookStage, RepositoryHookContext,
};
use crate::core::terminal_environment::TerminalEnvironment;
use crate::core::worktree_status::{
    ensure_required_worktrees_exist, repo_worktree_statuses, RepoWorktreeStatus,
};
use crate::runner::{CommandRunner, OutputLine};
use crate::service::{EnvRunner, RequestContext, ServiceEvent};
use anyhow::Result;
use chrono::Local;
use std::path::Path;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DoneRequest {
    pub name: String,
    pub no_merge: bool,
    pub no_clean: bool,
    pub push: bool,
    /// "squash", "rebase" or "merge"; `None` uses the git layer default.
    pub strategy: Option<String>,
    pub force: bool,
    pub skip_hooks: bool,
    pub dry_run: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CancelRequest {
    pub name: String,
    pub no_clean: bool,
    pub force: bool,
    pub skip_hooks: bool,
    /// The caller already confirmed continuing despite uncommitted changes.
    pub confirmed_uncommitted: bool,
}

/// Returned by [`cancel_workspace`] when repos have uncommitted changes and the
/// request neither forces nor carries a confirmation. The caller decides how to
/// ask the user and retries with `confirmed_uncommitted`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UncommittedConfirmationRequired {
    pub repos: Vec<String>,
}

impl std::fmt::Display for UncommittedConfirmationRequired {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "repo(s) {} have uncommitted changes; confirmation required (use --force to proceed anyway)",
            self.repos.join(", ")
        )
    }
}

impl std::error::Error for UncommittedConfirmationRequired {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifecycleOutcome {
    pub workspace: String,
    /// Done: nothing was changed (dry run).
    pub dry_run: bool,
}

const CANCELABLE_STATUSES: &[WorkspaceStatus] =
    &[WorkspaceStatus::Pending, WorkspaceStatus::InProgress];

pub fn cancel_candidate_statuses() -> &'static [WorkspaceStatus] {
    CANCELABLE_STATUSES
}

pub(crate) fn is_cancelable_status(status: &WorkspaceStatus) -> bool {
    CANCELABLE_STATUSES.contains(status)
}

pub(crate) fn warn_or_bail(
    ctx: &RequestContext<'_>,
    force: bool,
    err: anyhow::Error,
    context: &str,
) -> Result<()> {
    if force {
        ctx.warn(format!("{}: {:#}", context, err));
        Ok(())
    } else {
        Err(err.context(format!("{} (use --force to proceed anyway)", context)))
    }
}

/// Runs a hook, emitting step start/end events and one log event per output
/// line. The error (and its complete stderr) is returned unchanged so callers
/// keep their `warn_or_bail` semantics.
fn run_hook<R: CommandRunner>(
    ctx: &RequestContext<'_>,
    engine: &HookEngine<'_, R>,
    invocation: &HookInvocation<'_>,
    step: &str,
) -> Result<()> {
    ctx.emit(ServiceEvent::StepStarted(step.to_string()));
    let result = engine.execute_streaming(invocation, &mut |line| ctx.log_line(step, &line));
    ctx.emit(ServiceEvent::StepFinished {
        name: step.to_string(),
        ok: result.is_ok(),
    });
    result
}

/// Runs a git step, emitting step start/end events around it. The `GitOps`
/// in the service functions forwards its output lines as log events.
fn git_step<T>(ctx: &RequestContext<'_>, step: &str, f: impl FnOnce() -> Result<T>) -> Result<T> {
    ctx.emit(ServiceEvent::StepStarted(step.to_string()));
    let result = f();
    ctx.emit(ServiceEvent::StepFinished {
        name: step.to_string(),
        ok: result.is_ok(),
    });
    result
}

fn expanded_workspace_dir(workspace: &WorkspaceConfig) -> String {
    shellexpand::tilde(&workspace.workspace_dir).into_owned()
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum CancelRepoWorktreeDecision {
    Proceed,
    SkipMissing {
        repo_name: String,
        worktree_path: String,
    },
}

pub(crate) fn cancel_repo_worktree_decision(
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

pub(crate) fn archive_canceled_workspace(
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

pub(crate) fn close_terminal_environment_with<R: CommandRunner>(
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

pub(crate) fn archive_canceled_workspace_and_close_with<R: CommandRunner>(
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

pub(crate) fn archive_done_workspace_and_close_with<R: CommandRunner>(
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

pub(crate) fn report_terminal_environment_warnings(
    ctx: &RequestContext<'_>,
    workspace_name: &str,
    warnings: Vec<String>,
) {
    for warning in warnings {
        ctx.warn(format!(
            "terminal environment for workspace '{}': {}",
            workspace_name, warning
        ));
    }
}

pub(crate) fn require_name(name: &str) -> Result<()> {
    if name.trim().is_empty() {
        anyhow::bail!("workspace name is required");
    }
    Ok(())
}

pub fn done_workspace<R: CommandRunner>(
    config_mgr: &ConfigManager,
    runner: &R,
    ctx: &RequestContext<'_>,
    req: &DoneRequest,
) -> Result<LifecycleOutcome> {
    require_name(&req.name)?;
    let runner = EnvRunner::new(runner, &ctx.env);
    let runner = &runner;
    let global = config_mgr.load_global_config()?;
    let log_git = |line: &OutputLine| ctx.log_line("git", line);
    let git = GitOps::with_output(runner, &log_git);
    let hook_engine = HookEngine::new(runner);
    let name = req.name.clone();

    // One lifecycle mutation per Workspace at a time, across processes (ADR-0006).
    let _workspace_lock = config_mgr.lock_workspace(&name)?;
    let (status, mut workspace) = config_mgr.load_workspace(&name)?;
    if !matches!(status, WorkspaceStatus::InProgress) {
        anyhow::bail!("workspace '{}' is not in_progress", name);
    }
    ctx.emit(ServiceEvent::Started {
        operation: "done",
        workspace: name.clone(),
    });

    let ws_dir = expanded_workspace_dir(&workspace);

    if req.dry_run {
        ctx.step(format!("dry run for workspace '{}':", name));
        if !req.no_merge {
            for repo_entry in &workspace.repos {
                ctx.step(format!(
                    "  merge {} -> {}",
                    workspace.branch,
                    repo_entry.target_branch.as_deref().unwrap_or("*")
                ));
            }
        }
        if !req.no_clean {
            ctx.step("  clean worktrees and workspace directory");
        }
        return Ok(LifecycleOutcome {
            workspace: name,
            dry_run: true,
        });
    }

    ensure_required_worktrees_exist(&workspace)?;

    // pre_done hook
    if !req.skip_hooks {
        if let Some(invocation) = HookInvocation::for_workspace(
            global.hooks.pre_done.as_ref(),
            HookStage::PreDone,
            HookOperation::Done,
            WorkspaceStatus::InProgress,
            &workspace,
        ) {
            if let Err(e) = run_hook(ctx, &hook_engine, &invocation, "pre_done") {
                warn_or_bail(ctx, req.force, e, "pre_done hook failed")?;
            }
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
                ctx.warn(format!(
                    "target branch '{}' not found in repo '{}', using current branch '{}'",
                    tb, repo_entry.name, current
                ));
                current
            }
            None => {
                let current = git.current_branch(&repo_path)?;
                ctx.warn(format!(
                    "target branch not configured for repo '{}', using current branch '{}'",
                    repo_entry.name, current
                ));
                current
            }
        };

        // Check uncommitted changes
        if git.has_uncommitted_changes(&worktree_path)? && !req.force {
            anyhow::bail!(
                "repo '{}' has uncommitted changes in {}. Commit or stash first, or use --force",
                repo_entry.name,
                worktree_path
            );
        }

        // Merge
        if !req.no_merge {
            let message = if workspace.description.is_empty() {
                workspace.title.clone()
            } else {
                format!("{}\n\n{}", workspace.title, workspace.description)
            };
            git_step(ctx, "merge", || {
                git.merge_with_worktree(
                    &repo_path,
                    Some(&worktree_path),
                    &workspace.branch,
                    &target_branch,
                    req.strategy.as_deref(),
                    &message,
                )
            })?;
            ctx.step(format!(
                "  merged {} -> {} ({})",
                workspace.branch, target_branch, repo_entry.name
            ));

            if req.push {
                git_step(ctx, "push", || git.push(&repo_path, &target_branch))?;
                ctx.step(format!("  pushed {} ({})", target_branch, repo_entry.name));
            }
        }

        // Clean
        if !req.no_clean {
            if !req.skip_hooks {
                if let Some(invocation) = HookInvocation::for_repository(
                    repo_config.hooks.pre_remove.as_ref(),
                    global.hooks.pre_remove.as_ref(),
                    HookStage::PreRemove,
                    HookOperation::Done,
                    WorkspaceStatus::InProgress,
                    &workspace,
                    RepositoryHookContext {
                        name: &repo_entry.name,
                        source_dir: &repo_path,
                        worktree_path: &worktree_path,
                        target_branch: Some(&target_branch),
                    },
                ) {
                    if let Err(e) = run_hook(ctx, &hook_engine, &invocation, "pre_remove") {
                        warn_or_bail(ctx, req.force, e, "pre_remove hook failed")?;
                    }
                }
            }

            if let Err(e) = git.worktree_remove(&repo_path, &worktree_path, false) {
                ctx.warn(format!(
                    "failed to remove worktree '{}': {}",
                    worktree_path, e
                ));
            }
        }
    }

    // Remove workspace directory
    if !req.no_clean && Path::new(&ws_dir).exists() {
        if let Err(e) = std::fs::remove_dir_all(&ws_dir) {
            warn_or_bail(
                ctx,
                req.force,
                e.into(),
                "failed to remove workspace directory",
            )?;
        }
    }

    let warnings =
        archive_done_workspace_and_close_with(config_mgr, &global, &mut workspace, runner)?;
    report_terminal_environment_warnings(ctx, &workspace.name, warnings);

    ctx.emit(ServiceEvent::Finished {
        operation: "done",
        workspace: name.clone(),
        summary: format!("workspace '{}' completed", name),
    });
    Ok(LifecycleOutcome {
        workspace: name,
        dry_run: false,
    })
}

pub fn cancel_workspace<R: CommandRunner>(
    config_mgr: &ConfigManager,
    runner: &R,
    ctx: &RequestContext<'_>,
    req: &CancelRequest,
) -> Result<LifecycleOutcome> {
    require_name(&req.name)?;
    let runner = EnvRunner::new(runner, &ctx.env);
    let runner = &runner;
    let global = config_mgr.load_global_config()?;
    let log_git = |line: &OutputLine| ctx.log_line("git", line);
    let git = GitOps::with_output(runner, &log_git);
    let hook_engine = HookEngine::new(runner);
    let name = req.name.clone();

    // One lifecycle mutation per Workspace at a time, across processes (ADR-0006).
    let _workspace_lock = config_mgr.lock_workspace(&name)?;
    let (status, mut workspace) = config_mgr.load_workspace(&name)?;
    if !is_cancelable_status(&status) {
        anyhow::bail!("workspace '{}' is not active", name);
    }
    ctx.emit(ServiceEvent::Started {
        operation: "cancel",
        workspace: name.clone(),
    });

    let finish = |ctx: &RequestContext<'_>| {
        ctx.emit(ServiceEvent::Finished {
            operation: "cancel",
            workspace: name.clone(),
            summary: format!("workspace '{}' canceled", name),
        });
        Ok(LifecycleOutcome {
            workspace: name.clone(),
            dry_run: false,
        })
    };

    if matches!(status, WorkspaceStatus::Pending) {
        archive_canceled_workspace(config_mgr, &status, &mut workspace)?;
        return finish(ctx);
    }

    let ws_dir = expanded_workspace_dir(&workspace);
    let worktree_statuses = repo_worktree_statuses(&workspace, &ws_dir);

    // Uncommitted changes need a decision from the caller; never prompt here.
    if !req.force && !req.confirmed_uncommitted {
        let mut dirty = Vec::new();
        for repo_entry in &workspace.repos {
            let worktree_path = format!("{}/{}", ws_dir, repo_entry.name);
            if matches!(
                cancel_repo_worktree_decision(repo_entry, &worktree_path, &worktree_statuses),
                CancelRepoWorktreeDecision::Proceed
            ) && git.has_uncommitted_changes(&worktree_path)?
            {
                dirty.push(repo_entry.name.clone());
            }
        }
        if !dirty.is_empty() {
            return Err(UncommittedConfirmationRequired { repos: dirty }.into());
        }
    }

    // pre_cancel hook
    if !req.skip_hooks {
        if let Some(invocation) = HookInvocation::for_workspace(
            global.hooks.pre_cancel.as_ref(),
            HookStage::PreCancel,
            HookOperation::Cancel,
            WorkspaceStatus::InProgress,
            &workspace,
        ) {
            if let Err(e) = run_hook(ctx, &hook_engine, &invocation, "pre_cancel") {
                warn_or_bail(ctx, req.force, e, "pre_cancel hook failed")?;
            }
        }
    }

    if !req.no_clean {
        for repo_entry in &workspace.repos {
            let worktree_path = format!("{}/{}", ws_dir, repo_entry.name);
            match cancel_repo_worktree_decision(repo_entry, &worktree_path, &worktree_statuses) {
                CancelRepoWorktreeDecision::Proceed => {}
                CancelRepoWorktreeDecision::SkipMissing {
                    repo_name,
                    worktree_path,
                } => {
                    ctx.step(format!(
                        "  warning: missing worktree: {} ({})",
                        repo_name, worktree_path
                    ));
                    continue;
                }
            }
            let repo_config = config_mgr.load_repo_config(&repo_entry.name)?;
            let repo_path = shellexpand::tilde(&repo_config.path).into_owned();

            // pre_remove hook
            if !req.skip_hooks {
                if let Some(invocation) = HookInvocation::for_repository(
                    repo_config.hooks.pre_remove.as_ref(),
                    global.hooks.pre_remove.as_ref(),
                    HookStage::PreRemove,
                    HookOperation::Cancel,
                    WorkspaceStatus::InProgress,
                    &workspace,
                    RepositoryHookContext {
                        name: &repo_entry.name,
                        source_dir: &repo_path,
                        worktree_path: &worktree_path,
                        target_branch: repo_entry.target_branch.as_deref(),
                    },
                ) {
                    if let Err(e) = run_hook(ctx, &hook_engine, &invocation, "pre_remove") {
                        warn_or_bail(ctx, req.force, e, "pre_remove hook failed")?;
                    }
                }
            }

            if Path::new(&worktree_path).exists() {
                if let Err(e) = git.worktree_remove(&repo_path, &worktree_path, req.force) {
                    ctx.warn(format!(
                        "failed to remove worktree '{}': {}",
                        worktree_path, e
                    ));
                }
            }
        }

        if Path::new(&ws_dir).exists() {
            if let Err(e) = std::fs::remove_dir_all(&ws_dir) {
                warn_or_bail(
                    ctx,
                    req.force,
                    e.into(),
                    "failed to remove workspace directory",
                )?;
            }
        }
    }

    let warnings = archive_canceled_workspace_and_close_with(
        config_mgr,
        &global,
        &status,
        &mut workspace,
        runner,
    )?;
    report_terminal_environment_warnings(ctx, &workspace.name, warnings);

    finish(ctx)
}
