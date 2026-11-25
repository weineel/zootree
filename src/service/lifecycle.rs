//! Done and cancel as service functions.

use crate::config::global::GlobalConfig;
use crate::config::workspace::{Event, RepoEntry, WorkspaceConfig, WorkspaceStatus};
use crate::config::ConfigManager;
use crate::core::git::GitOps;
use crate::core::hook::{
    HookEngine, HookInvocation, HookOperation, HookStage, RepositoryHookContext,
};
use crate::core::terminal_environment::{CallerContext, TerminalEnvironment};
use crate::core::worktree_status::{repo_worktree_statuses, RepoWorktreeStatus};
use crate::runner::{CommandRunner, OutputLine};
use crate::service::{EnvRunner, RequestContext, ServiceEvent};
use anyhow::Result;
use chrono::Local;
use serde::{Deserialize, Serialize};
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
    /// Approve all Source checkout branch switches for this operation.
    pub yes: bool,
    /// Switches approved by the caller after an earlier read-only preflight.
    pub confirmed_switches: Vec<BranchSwitch>,
    /// Integrate only these Workspace repositories (empty: all). Mutually
    /// exclusive with `skip_repos`. Unselected repositories are not
    /// integrated but are still cleaned up.
    pub repos: Vec<String>,
    /// Integrate every Workspace repository except these.
    pub skip_repos: Vec<String>,
}

/// Integrates Workspace repositories without completing the Workspace: no
/// cleanup, no lifecycle change, no hooks, and no progress records.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MergeRequest {
    pub name: String,
    pub push: bool,
    pub strategy: Option<String>,
    pub force: bool,
    pub dry_run: bool,
    pub yes: bool,
    pub confirmed_switches: Vec<BranchSwitch>,
    pub repos: Vec<String>,
    pub skip_repos: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchSwitch {
    pub repo: String,
    pub current: String,
    pub target: String,
    pub workspace_branch: String,
}

impl std::fmt::Display for BranchSwitch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: source checkout {} -> {} (merging workspace branch {})",
            self.repo, self.current, self.target, self.workspace_branch
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchSwitchConfirmationRequired {
    pub switches: Vec<BranchSwitch>,
}

impl std::fmt::Display for BranchSwitchConfirmationRequired {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Source checkouts will stay on their target branches after integration. Confirm these branch switches:")?;
        for switch in &self.switches {
            write!(f, "\n  {switch}")?;
        }
        Ok(())
    }
}

impl std::error::Error for BranchSwitchConfirmationRequired {}

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
    caller: &CallerContext,
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

    let terminal_environment = TerminalEnvironment::with_caller(config_mgr, global, runner, caller);
    terminal_environment.close(workspace).warnings
}

pub(crate) fn archive_canceled_workspace_and_close_with<R: CommandRunner>(
    config_mgr: &ConfigManager,
    global: &GlobalConfig,
    from_status: &WorkspaceStatus,
    workspace: &mut WorkspaceConfig,
    runner: &R,
    caller: &CallerContext,
) -> Result<Vec<String>> {
    archive_canceled_workspace(config_mgr, from_status, workspace)?;
    Ok(close_terminal_environment_with(
        config_mgr, global, workspace, runner, caller,
    ))
}

pub(crate) fn archive_done_workspace_and_close_with<R: CommandRunner>(
    config_mgr: &ConfigManager,
    global: &GlobalConfig,
    workspace: &mut WorkspaceConfig,
    runner: &R,
    caller: &CallerContext,
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
        config_mgr, global, workspace, runner, caller,
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

/// Names of the Workspace repositories to integrate.
fn select_repos(
    workspace: &WorkspaceConfig,
    repos: &[String],
    skip_repos: &[String],
) -> Result<Vec<String>> {
    if !repos.is_empty() && !skip_repos.is_empty() {
        anyhow::bail!("--repo and --skip-repo cannot be used together");
    }
    let known: Vec<&str> = workspace.repos.iter().map(|r| r.name.as_str()).collect();
    let unknown: Vec<&str> = repos
        .iter()
        .chain(skip_repos)
        .map(String::as_str)
        .filter(|name| !known.contains(name))
        .collect();
    if !unknown.is_empty() {
        anyhow::bail!(
            "workspace '{}' has no repo(s) {} (repos: {})",
            workspace.name,
            unknown.join(", "),
            known.join(", ")
        );
    }
    Ok(workspace
        .repos
        .iter()
        .map(|r| r.name.clone())
        .filter(|name| {
            if repos.is_empty() {
                !skip_repos.contains(name)
            } else {
                repos.contains(name)
            }
        })
        .collect())
}

/// One Workspace repository, checked and ready for the mutating phases.
struct PreparedRepo {
    name: String,
    repo_path: String,
    worktree_path: String,
    /// Resolved integration target; `None` for repositories not integrated.
    target_branch: Option<String>,
    branch_switch: Option<BranchSwitch>,
    worktree_exists: bool,
    /// Integration already recorded by an earlier `done` attempt.
    already_merged: bool,
    pre_remove_hook: Option<crate::config::global::HookValue>,
}

fn merged_marker(repo: &str, branch_head: &str) -> String {
    format!("{repo}@{branch_head}")
}

fn has_record(workspace: &WorkspaceConfig, action: &str, marker: &str) -> bool {
    workspace
        .events
        .iter()
        .any(|e| e.action == action && e.detail.as_deref() == Some(marker))
}

/// Read-only validation of every repository involved, before any mutation.
/// All problems are collected into one error.
#[allow(clippy::too_many_arguments)]
fn preflight<R: CommandRunner>(
    config_mgr: &ConfigManager,
    ctx: &RequestContext<'_>,
    git: &GitOps<'_, R>,
    workspace: &WorkspaceConfig,
    merge_set: &[String],
    clean: bool,
    force: bool,
    honor_records: bool,
) -> Result<Vec<PreparedRepo>> {
    let ws_dir = expanded_workspace_dir(workspace);
    let mut problems = Vec::new();
    let mut prepared = Vec::new();

    for entry in &workspace.repos {
        let in_merge = merge_set.contains(&entry.name);
        let repo_config = match config_mgr.load_repo_config(&entry.name) {
            Ok(c) => c,
            Err(e) => {
                problems.push(format!("{}: cannot load repo config: {e:#}", entry.name));
                continue;
            }
        };
        let repo_path = shellexpand::tilde(&repo_config.path).into_owned();
        let worktree_path = format!("{}/{}", ws_dir, entry.name);
        let worktree_exists = Path::new(&worktree_path).exists();

        let already_merged = in_merge
            && honor_records
            && git
                .revision(&repo_path, &workspace.branch)
                .map(|head| has_record(workspace, "merged", &merged_marker(&entry.name, &head)))
                .unwrap_or(false);

        if !worktree_exists {
            // Never forced: a repo that should be merged but has no worktree
            // would silently be left out. Leaving it out must be explicit.
            if in_merge && !already_merged {
                problems.push(format!(
                    "{}: worktree {} is missing (use --skip-repo {} to leave it out of the merge)",
                    entry.name, worktree_path, entry.name
                ));
            }
        } else if (in_merge || clean) && !force {
            match git.has_uncommitted_changes(&worktree_path) {
                Ok(true) => problems.push(format!(
                    "{}: uncommitted changes in {} (commit or stash first, or use --force)",
                    entry.name, worktree_path
                )),
                Ok(false) => {}
                Err(e) => problems.push(format!("{}: {e:#}", entry.name)),
            }
        }

        let mut target_branch = None;
        let mut branch_switch = None;
        if in_merge && (already_merged || worktree_exists) {
            match resolve_target(ctx, git, entry, &repo_path, !already_merged) {
                Ok((target, switch_from)) => {
                    if !already_merged {
                        match git.has_tracked_changes(&repo_path) {
                            Ok(true) => problems.push(format!(
                                "{}: source checkout {} has uncommitted changes to tracked files; \
                                 they would end up in the same commit as the merge into '{}'",
                                entry.name, repo_path, target
                            )),
                            Ok(false) => {}
                            Err(e) => problems.push(format!("{}: {e:#}", entry.name)),
                        }
                    }
                    branch_switch = switch_from.map(|current| BranchSwitch {
                        repo: entry.name.clone(),
                        current,
                        target: target.clone(),
                        workspace_branch: workspace.branch.clone(),
                    });
                    target_branch = Some(target);
                }
                Err(e) => problems.push(format!("{}: {e:#}", entry.name)),
            }
        }

        prepared.push(PreparedRepo {
            name: entry.name.clone(),
            repo_path,
            worktree_path,
            target_branch,
            branch_switch,
            worktree_exists,
            already_merged,
            pre_remove_hook: repo_config.hooks.pre_remove.clone(),
        });
    }

    if problems.is_empty() {
        Ok(prepared)
    } else {
        anyhow::bail!(
            "workspace '{}' cannot proceed:\n  {}",
            workspace.name,
            problems.join("\n  ")
        )
    }
}

/// Returns the effective target and the branch we would switch away from.
fn resolve_target<R: CommandRunner>(
    ctx: &RequestContext<'_>,
    git: &GitOps<'_, R>,
    entry: &RepoEntry,
    repo_path: &str,
    will_checkout: bool,
) -> Result<(String, Option<String>)> {
    match &entry.target_branch {
        Some(tb) if git.branch_exists(repo_path, tb)? => {
            let switch_from = if will_checkout {
                Some(git.current_branch(repo_path)?).filter(|current| current != tb)
            } else {
                None
            };
            Ok((tb.clone(), switch_from))
        }
        Some(tb) => {
            let current = git.current_branch(repo_path)?;
            ctx.warn(format!(
                "target branch '{}' not found in repo '{}', using current branch '{}'",
                tb, entry.name, current
            ));
            Ok((current, None))
        }
        None => {
            let current = git.current_branch(repo_path)?;
            ctx.warn(format!(
                "target branch not configured for repo '{}', using current branch '{}'",
                entry.name, current
            ));
            Ok((current, None))
        }
    }
}

fn confirm_branch_switches(
    prepared: &[PreparedRepo],
    yes: bool,
    confirmed: &[BranchSwitch],
) -> Result<()> {
    if yes {
        return Ok(());
    }
    let switches = prepared
        .iter()
        .filter_map(|repo| repo.branch_switch.as_ref());
    if switches.clone().all(|switch| confirmed.contains(switch)) {
        return Ok(());
    }
    Err(BranchSwitchConfirmationRequired {
        switches: switches.cloned().collect(),
    }
    .into())
}

/// Integrates (and optionally pushes) every prepared repository that has a
/// target. With `record`, progress is persisted on the Workspace so a retry
/// skips what already succeeded.
#[allow(clippy::too_many_arguments)]
fn integrate_repos<R: CommandRunner>(
    config_mgr: &ConfigManager,
    ctx: &RequestContext<'_>,
    git: &GitOps<'_, R>,
    workspace: &mut WorkspaceConfig,
    prepared: &[PreparedRepo],
    strategy: Option<&str>,
    push: bool,
    record: bool,
    yes: bool,
) -> Result<()> {
    let message = if workspace.description.is_empty() {
        workspace.title.clone()
    } else {
        format!("{}\n\n{}", workspace.title, workspace.description)
    };
    for repo in prepared {
        let Some(target) = &repo.target_branch else {
            continue;
        };
        if repo.already_merged {
            ctx.step(format!(
                "  skipped {} (already merged {} -> {})",
                repo.name, workspace.branch, target
            ));
        } else if repo.worktree_exists {
            if !yes {
                // The pre_done hook or another Workspace can change the Source checkout
                // after preflight. Stop before modifying even the Workspace worktree.
                let current = git.current_branch(&repo.repo_path)?;
                let original = repo
                    .branch_switch
                    .as_ref()
                    .map_or(target.as_str(), |s| s.current.as_str());
                if current != original && current != *target {
                    anyhow::bail!(
                        "{}: source checkout changed from '{}' to '{}' after preflight; \
                         switching to target '{}' requires a new confirmation. Run the command again.",
                        repo.name, original, current, target
                    );
                }
            }
            git_step(ctx, "merge", || {
                git.merge_with_worktree(
                    &repo.repo_path,
                    Some(&repo.worktree_path),
                    &workspace.branch,
                    target,
                    strategy,
                    &message,
                )
            })?;
            ctx.step(format!(
                "  merged {} -> {} ({})",
                workspace.branch, target, repo.name
            ));
            if record {
                // After the merge: a rebase rewrites the branch head, and a
                // retry compares against the head it will then see.
                let branch_head = git.revision(&repo.repo_path, &workspace.branch)?;
                let marker = merged_marker(&repo.name, &branch_head);
                record_event(config_mgr, workspace, "merged", &marker)?;
            }
        } else {
            continue;
        }

        if push {
            let target_head = git.revision(&repo.repo_path, target)?;
            let pushed = merged_marker(&repo.name, &target_head);
            if record && has_record(workspace, "pushed", &pushed) {
                ctx.step(format!("  skipped push {} ({})", target, repo.name));
                continue;
            }
            git_step(ctx, "push", || git.push(&repo.repo_path, target))?;
            ctx.step(format!("  pushed {} ({})", target, repo.name));
            if record {
                record_event(config_mgr, workspace, "pushed", &pushed)?;
            }
        }
    }
    Ok(())
}

fn record_event(
    config_mgr: &ConfigManager,
    workspace: &mut WorkspaceConfig,
    action: &str,
    marker: &str,
) -> Result<()> {
    workspace.events.push(Event {
        action: action.into(),
        timestamp: Local::now().to_rfc3339(),
        detail: Some(marker.into()),
    });
    config_mgr.save_workspace(&WorkspaceStatus::InProgress, workspace)
}

fn describe_plan(
    ctx: &RequestContext<'_>,
    workspace: &WorkspaceConfig,
    prepared: &[PreparedRepo],
    clean: bool,
) {
    for entry in &workspace.repos {
        let repo = prepared.iter().find(|repo| repo.name == entry.name);
        if let Some(switch) = repo.and_then(|repo| repo.branch_switch.as_ref()) {
            ctx.step(format!("  {switch}"));
        }
        match repo.and_then(|repo| repo.target_branch.as_deref()) {
            Some(_) if repo.is_some_and(|repo| repo.already_merged) => {
                ctx.step(format!("  skip merge of {} (already merged)", entry.name));
            }
            Some(target) => ctx.step(format!(
                "  merge {} -> {} ({})",
                workspace.branch, target, entry.name
            )),
            None => ctx.step(format!("  skip merge of {}", entry.name)),
        }
    }
    if clean {
        ctx.step("  clean worktrees and workspace directory");
    }
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
    let selected = select_repos(&workspace, &req.repos, &req.skip_repos)?;
    let merge_set = if req.no_merge {
        if !req.repos.is_empty() || !req.skip_repos.is_empty() {
            ctx.warn("--no-merge given: --repo/--skip-repo select nothing to merge".to_string());
        }
        Vec::new()
    } else {
        selected
    };
    ctx.emit(ServiceEvent::Started {
        operation: "done",
        workspace: name.clone(),
    });

    let ws_dir = expanded_workspace_dir(&workspace);
    let clean = !req.no_clean;

    // Validate everything before touching anything: a failure here leaves
    // the Workspace exactly as it was, so `done` can simply be run again.
    let prepared = preflight(
        config_mgr, ctx, &git, &workspace, &merge_set, clean, req.force, true,
    )?;

    if req.dry_run {
        ctx.step(format!("dry run for workspace '{}':", name));
        describe_plan(ctx, &workspace, &prepared, clean);
        return Ok(LifecycleOutcome {
            workspace: name,
            dry_run: true,
        });
    }

    confirm_branch_switches(&prepared, req.yes, &req.confirmed_switches)?;

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

    // Phase 1: integrate everything. Nothing is cleaned until all succeeded.
    integrate_repos(
        config_mgr,
        ctx,
        &git,
        &mut workspace,
        &prepared,
        req.strategy.as_deref(),
        req.push,
        true,
        req.yes,
    )?;

    // Phase 2: clean up. Hooks for every repo first, then the removals.
    if clean {
        let cleanable: Vec<&PreparedRepo> = prepared.iter().filter(|p| p.worktree_exists).collect();
        if !req.skip_hooks {
            for repo in &cleanable {
                let entry_target = workspace
                    .repos
                    .iter()
                    .find(|r| r.name == repo.name)
                    .and_then(|r| r.target_branch.as_deref());
                if let Some(invocation) = HookInvocation::for_repository(
                    repo.pre_remove_hook.as_ref(),
                    global.hooks.pre_remove.as_ref(),
                    HookStage::PreRemove,
                    HookOperation::Done,
                    WorkspaceStatus::InProgress,
                    &workspace,
                    RepositoryHookContext {
                        name: &repo.name,
                        source_dir: &repo.repo_path,
                        worktree_path: &repo.worktree_path,
                        target_branch: repo.target_branch.as_deref().or(entry_target),
                    },
                ) {
                    if let Err(e) = run_hook(ctx, &hook_engine, &invocation, "pre_remove") {
                        warn_or_bail(ctx, req.force, e, "pre_remove hook failed")?;
                    }
                }
            }
        }
        for repo in &cleanable {
            if let Err(e) = git.worktree_remove(&repo.repo_path, &repo.worktree_path, false) {
                ctx.warn(format!(
                    "failed to remove worktree '{}': {}",
                    repo.worktree_path, e
                ));
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

    let warnings = archive_done_workspace_and_close_with(
        config_mgr,
        &global,
        &mut workspace,
        runner,
        &ctx.caller(),
    )?;
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

/// Integrates the selected repositories only. Leaves the Workspace in
/// progress, keeps every worktree, runs no hooks, and records nothing.
pub fn merge_workspace<R: CommandRunner>(
    config_mgr: &ConfigManager,
    runner: &R,
    ctx: &RequestContext<'_>,
    req: &MergeRequest,
) -> Result<LifecycleOutcome> {
    require_name(&req.name)?;
    let runner = EnvRunner::new(runner, &ctx.env);
    let runner = &runner;
    config_mgr.load_global_config()?;
    let log_git = |line: &OutputLine| ctx.log_line("git", line);
    let git = GitOps::with_output(runner, &log_git);
    let name = req.name.clone();

    let _workspace_lock = config_mgr.lock_workspace(&name)?;
    let (status, mut workspace) = config_mgr.load_workspace(&name)?;
    if !matches!(status, WorkspaceStatus::InProgress) {
        anyhow::bail!("workspace '{}' is not in_progress", name);
    }
    let merge_set = select_repos(&workspace, &req.repos, &req.skip_repos)?;
    ctx.emit(ServiceEvent::Started {
        operation: "merge",
        workspace: name.clone(),
    });

    // Repositories outside the selection are not touched at all, not even
    // their configuration is read.
    let mut scoped = workspace.clone();
    scoped.repos.retain(|r| merge_set.contains(&r.name));
    let prepared = preflight(
        config_mgr, ctx, &git, &scoped, &merge_set, false, req.force, false,
    )?;

    if req.dry_run {
        ctx.step(format!("dry run for workspace '{}':", name));
        describe_plan(ctx, &workspace, &prepared, false);
        return Ok(LifecycleOutcome {
            workspace: name,
            dry_run: true,
        });
    }

    confirm_branch_switches(&prepared, req.yes, &req.confirmed_switches)?;

    integrate_repos(
        config_mgr,
        ctx,
        &git,
        &mut workspace,
        &prepared,
        req.strategy.as_deref(),
        req.push,
        false,
        req.yes,
    )?;

    ctx.emit(ServiceEvent::Finished {
        operation: "merge",
        workspace: name.clone(),
        summary: format!("workspace '{}' merged", name),
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
        // Hooks for every repo first, so a failing hook cannot leave some
        // worktrees removed and others not.
        let mut removals = Vec::new();
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
            removals.push((repo_path, worktree_path));
        }

        for (repo_path, worktree_path) in removals {
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
        &ctx.caller(),
    )?;
    report_terminal_environment_warnings(ctx, &workspace.name, warnings);

    finish(ctx)
}
