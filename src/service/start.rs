//! Start and open as service functions, plus the shared terminal activation
//! step that reopen uses too.
//!
//! Foreground attach is a client-local action: service functions never run
//! interactive commands. They return an [`AttachRequest`] describing what to
//! attach to, and the caller (the CLI today, a Daemon client later) performs it.

use crate::config::global::{GlobalConfig, MultiplexerKind};
use crate::config::workspace::{
    Event, StoredTerminalEnvironmentState, WorkspaceConfig, WorkspaceStatus,
};
use crate::config::{ConfigManager, WorkspaceLock};
use crate::core::copy_files;
use crate::core::git::GitOps;
use crate::core::hook::{
    HookEngine, HookInvocation, HookOperation, HookStage, RepositoryHookContext,
};
use crate::core::terminal_environment::{AgentIntent, CallerContext, TerminalEnvironment};
use crate::core::workspace_instruction_index;
use crate::core::worktree_status::ensure_required_worktrees_exist;
use crate::runner::{CommandRunner, CommandSpec};
use crate::service::lifecycle::{report_terminal_environment_warnings, require_name};
use crate::service::{EnvRunner, RequestContext, ServiceEvent};
use anyhow::Result;
use chrono::Local;
use std::cell::RefCell;
#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;
#[cfg(windows)]
use std::os::windows::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Output};
use std::time::Duration;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StartRequest {
    pub name: String,
    pub multiplexer: Option<MultiplexerKind>,
    /// Start the Workspace without touching the terminal environment.
    pub no_multiplexer: bool,
    /// `None`: no agent; `Some(None)` or an empty alias: the configured
    /// default agent; `Some(Some(command))`: that command.
    pub run_agent: Option<Option<String>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OpenRequest {
    pub name: String,
}

/// An interactive command the caller must run in its own terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachRequest {
    pub program: String,
    pub args: Vec<String>,
}

impl AttachRequest {
    /// Runs the attach in the current terminal. A Herdr attach failure is
    /// non-fatal and comes back as a warning line; any other failure is an
    /// error.
    pub fn run<R: CommandRunner>(&self, runner: &R) -> Result<Option<String>> {
        let herdr = self.program == "herdr";
        let result = runner
            .run_interactive(&CommandSpec {
                program: self.program.clone(),
                args: self.args.clone(),
                cwd: None,
                env: Default::default(),
                env_remove: Vec::new(),
            })
            .and_then(|status| {
                if status.success() {
                    return Ok(());
                }
                let reason = status
                    .code()
                    .map(|code| format!("exit code {code}"))
                    .unwrap_or_else(|| "terminated by signal".into());
                if herdr {
                    anyhow::bail!("Herdr session attach failed with {reason}")
                }
                anyhow::bail!("{} exited with {}", self.program, reason)
            });
        match result {
            Ok(()) => Ok(None),
            Err(error) if herdr => {
                let session = self.args.get(2).map(String::as_str).unwrap_or_default();
                Ok(Some(format!(
                    "failed to attach Herdr session '{session}': {error:#}; run `herdr session attach {session}`"
                )))
            }
            Err(error) => Err(error),
        }
    }
}

/// Result of start, open and reopen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionOutcome {
    pub workspace: String,
    /// Reopen: nothing was changed (dry run).
    pub dry_run: bool,
    /// What the caller still has to attach to, if anything.
    pub attach: Option<AttachRequest>,
}

/// Runner decorator that records interactive commands instead of running them.
pub(crate) struct DeferInteractive<'a, R: CommandRunner> {
    inner: &'a R,
    deferred: RefCell<Vec<CommandSpec>>,
}

impl<'a, R: CommandRunner> DeferInteractive<'a, R> {
    pub(crate) fn new(inner: &'a R) -> Self {
        Self {
            inner,
            deferred: RefCell::new(Vec::new()),
        }
    }

    pub(crate) fn take_attach(&self) -> Option<AttachRequest> {
        self.deferred.borrow_mut().pop().map(|spec| AttachRequest {
            program: spec.program,
            args: spec.args,
        })
    }
}

impl<R: CommandRunner> CommandRunner for DeferInteractive<'_, R> {
    fn run(&self, spec: &CommandSpec) -> Result<Output> {
        self.inner.run(spec)
    }

    fn run_interactive(&self, spec: &CommandSpec) -> Result<ExitStatus> {
        self.deferred.borrow_mut().push(CommandSpec {
            program: spec.program.clone(),
            args: spec.args.clone(),
            cwd: spec.cwd.clone(),
            env: spec.env.clone(),
            env_remove: spec.env_remove.clone(),
        });
        Ok(ExitStatus::from_raw(0))
    }
}

pub fn selected_agent_cli_value(
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

pub(crate) fn agent_intent(run_agent: Option<Option<String>>) -> AgentIntent {
    match run_agent {
        None => AgentIntent::None,
        Some(None) => AgentIntent::Default,
        Some(Some(command)) if command.is_empty() => AgentIntent::Default,
        Some(Some(command)) => AgentIntent::Override(command),
    }
}

/// How long persisting terminal state waits for another process's lifecycle
/// mutation of the same Workspace.
const TERMINAL_STATE_LOCK_WAIT: Duration = Duration::from_secs(5);

/// Persists the terminal state produced by an activation. The activation ran
/// on a copy loaded earlier, and the Workspace lock was not held meanwhile (a
/// foreground attach must never hold it), so another process may have moved
/// the Workspace on. Under the lock this reloads the latest configuration,
/// requires it to still be `in_progress` and writes back only
/// `multiplexer_state`; every other field keeps the latest stored value.
/// `held` is the caller's own lock when it already holds one (reopen).
pub(crate) fn persist_terminal_state(
    config_mgr: &ConfigManager,
    name: &str,
    stored_state: StoredTerminalEnvironmentState,
    held: Option<&WorkspaceLock>,
) -> Result<()> {
    let _lock = match held {
        Some(_) => None,
        None => Some(config_mgr.lock_workspace_for(name, TERMINAL_STATE_LOCK_WAIT)?),
    };
    let (status, mut latest) = config_mgr.load_workspace(name)?;
    if !matches!(status, WorkspaceStatus::InProgress) {
        anyhow::bail!(
            "workspace '{name}' is {} now (it changed while its terminal environment was being activated); terminal state not saved",
            status.as_str()
        );
    }
    latest.multiplexer_state = stored_state;
    config_mgr.save_workspace(&WorkspaceStatus::InProgress, &latest)
}

/// Activates the Workspace's terminal environment with the request's caller
/// context, persists the resulting state (see [`persist_terminal_state`]),
/// reports notices and warnings through the context and returns the attach
/// the caller still has to perform.
pub(crate) fn activate_terminal_environment<R: CommandRunner>(
    config_mgr: &ConfigManager,
    global: &GlobalConfig,
    workspace: &WorkspaceConfig,
    runner: &R,
    ctx: &RequestContext<'_>,
    agent_intent: AgentIntent,
    held_lock: Option<&WorkspaceLock>,
) -> Result<Option<AttachRequest>> {
    let deferred = DeferInteractive::new(runner);
    let caller = CallerContext::from_vars(|name| ctx.env.get(name).map(str::to_string));
    let terminal_environment =
        TerminalEnvironment::with_caller(config_mgr, global, &deferred, &caller);
    let activation = terminal_environment.activate(workspace, agent_intent)?;
    persist_terminal_state(
        config_mgr,
        &workspace.name,
        activation.stored_state,
        held_lock,
    )?;
    for notice in activation.notices {
        ctx.step(notice);
    }
    report_terminal_environment_warnings(ctx, &workspace.name, activation.warnings);
    Ok(deferred.take_attach())
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

fn start_workspace_state<R: CommandRunner>(
    config_mgr: &ConfigManager,
    global: &GlobalConfig,
    runner: &R,
    ctx: &RequestContext<'_>,
    req: &StartRequest,
) -> Result<WorkspaceConfig> {
    let git = GitOps::new(runner);
    let hook_engine = HookEngine::new(runner);
    let name = req.name.clone();

    // One lifecycle mutation per Workspace at a time, across processes (ADR-0006).
    let _workspace_lock = config_mgr.lock_workspace(&name)?;
    let (status, mut workspace) = config_mgr.load_workspace(&name)?;
    if !matches!(status, WorkspaceStatus::Pending) {
        anyhow::bail!("workspace '{}' is not in pending state", name);
    }
    ctx.emit(ServiceEvent::Started {
        operation: "start",
        workspace: name.clone(),
    });

    if let Some(kind) = &req.multiplexer {
        workspace.multiplexer.kind = kind.clone();
    }

    let ws_dir = shellexpand::tilde(&workspace.workspace_dir).into_owned();
    let ws_dir_path = PathBuf::from(&ws_dir);
    let created_workspace_dir = !ws_dir_path.exists();
    std::fs::create_dir_all(&ws_dir)?;
    let mut rollback = StartRollback::new(created_workspace_dir.then_some(ws_dir_path));

    let prepare_result = (|| -> Result<()> {
        if req.run_agent.is_some() {
            workspace.agent_cli = selected_agent_cli_value(&req.run_agent, global)?;
        }

        for repo_entry in &workspace.repos {
            let repo_config = config_mgr.load_repo_config(&repo_entry.name)?;
            let repo_path = shellexpand::tilde(&repo_config.path).into_owned();

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

            if let Some(invocation) = HookInvocation::for_repository(
                repo_config.hooks.post_create.as_ref(),
                global.hooks.post_create.as_ref(),
                HookStage::PostCreate,
                HookOperation::Start,
                WorkspaceStatus::Pending,
                &workspace,
                RepositoryHookContext {
                    name: &repo_entry.name,
                    source_dir: &repo_path,
                    worktree_path: &worktree_path,
                    target_branch: Some(&target_branch),
                },
            ) {
                hook_engine.execute(&invocation)?;
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
    workspace_instruction_index::sync(&workspace);

    if let Some(invocation) = HookInvocation::for_workspace(
        global.hooks.post_start.as_ref(),
        HookStage::PostStart,
        HookOperation::Start,
        WorkspaceStatus::InProgress,
        &workspace,
    ) {
        hook_engine.execute(&invocation)?;
    }

    Ok(workspace)
}

/// Starts a pending Workspace: creates its worktrees, moves it to in_progress
/// and (unless `no_multiplexer`) activates its terminal environment. A failure
/// before the state commit rolls the created worktrees back; a failure to
/// activate leaves the Workspace in_progress so `open` can retry.
pub fn start_workspace<R: CommandRunner>(
    config_mgr: &ConfigManager,
    runner: &R,
    ctx: &RequestContext<'_>,
    req: &StartRequest,
) -> Result<SessionOutcome> {
    require_name(&req.name)?;
    let runner = EnvRunner::new(runner, &ctx.env);
    let runner = &runner;
    let global = config_mgr.load_global_config()?;

    let workspace = start_workspace_state(config_mgr, &global, runner, ctx, req)?;
    let mut attach = None;
    if !req.no_multiplexer {
        attach = activate_terminal_environment(
            config_mgr,
            &global,
            &workspace,
            runner,
            ctx,
            agent_intent(req.run_agent.clone()),
            None,
        )
        .map_err(|error| {
            anyhow::anyhow!(
                "workspace '{}' started and remains in_progress, but terminal environment activation failed: {:#}. Run `zootree open {}` to retry",
                workspace.name,
                error,
                workspace.name
            )
        })?;
    }

    Ok(SessionOutcome {
        workspace: workspace.name,
        dry_run: false,
        attach,
    })
}

/// Activates the terminal environment of an in_progress Workspace.
pub fn open_workspace<R: CommandRunner>(
    config_mgr: &ConfigManager,
    runner: &R,
    ctx: &RequestContext<'_>,
    req: &OpenRequest,
) -> Result<SessionOutcome> {
    require_name(&req.name)?;
    let runner = EnvRunner::new(runner, &ctx.env);
    let runner = &runner;
    let global = config_mgr.load_global_config()?;

    let (status, workspace) = config_mgr.load_workspace(&req.name)?;
    if !matches!(status, WorkspaceStatus::InProgress) {
        anyhow::bail!("workspace '{}' is not in_progress", req.name);
    }
    ctx.emit(ServiceEvent::Started {
        operation: "open",
        workspace: req.name.clone(),
    });

    ensure_required_worktrees_exist(&workspace)?;

    let attach = activate_terminal_environment(
        config_mgr,
        &global,
        &workspace,
        runner,
        ctx,
        AgentIntent::None,
        None,
    )?;
    Ok(SessionOutcome {
        workspace: workspace.name,
        dry_run: false,
        attach,
    })
}
