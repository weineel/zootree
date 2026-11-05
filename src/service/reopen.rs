//! Reopen as a service function.

use crate::config::workspace::WorkspaceStatus;
use crate::config::ConfigManager;
use crate::core::reopen::{
    build_reopen_plan, execute_reopen_plan, format_reopen_plan, ReopenLifecyclePlan, ReopenOptions,
    ReopenPrompt, WorktreeAction,
};
use crate::core::terminal_environment::{CallerContext, CloseReport, TerminalEnvironment};
use crate::runner::CommandRunner;
use crate::service::lifecycle::{report_terminal_environment_warnings, require_name};
use crate::service::start::{
    activate_terminal_environment, agent_intent, selected_agent_cli_value, SessionOutcome,
};
use crate::service::{EnvRunner, RequestContext, ServiceEvent};
use anyhow::Result;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReopenRequest {
    pub name: String,
    /// Multiplexer override, base and overwrite choices.
    pub options: ReopenOptions,
    /// See [`crate::service::start::StartRequest::run_agent`].
    pub run_agent: Option<Option<String>>,
    pub skip_hooks: bool,
    pub no_multiplexer: bool,
    pub dry_run: bool,
}

pub(crate) fn require_terminal_environment_closed(report: CloseReport) -> Result<Vec<String>> {
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

fn close_terminal_environment_for_reopen<R: CommandRunner>(
    config_mgr: &ConfigManager,
    global: &crate::config::global::GlobalConfig,
    workspace_name: &str,
    runner: &R,
    caller: &CallerContext,
) -> Result<Vec<String>> {
    let workspace = match config_mgr.load_workspace(workspace_name)? {
        (WorkspaceStatus::Done | WorkspaceStatus::Canceled, workspace) => workspace,
        (status, _) => anyhow::bail!(
            "cannot close terminal environment before reopen because workspace '{}' is {}",
            workspace_name,
            status.as_str()
        ),
    };

    let terminal_environment = TerminalEnvironment::with_caller(config_mgr, global, runner, caller);
    require_terminal_environment_closed(terminal_environment.close(&workspace))
}

/// Reopens a done or canceled Workspace. Questions (remote branch, base,
/// overwrite) go to `prompt`; a Daemon passes a non-interactive one.
pub fn reopen_workspace<R: CommandRunner, P: ReopenPrompt>(
    config_mgr: &ConfigManager,
    runner: &R,
    ctx: &RequestContext<'_>,
    prompt: &mut P,
    req: &ReopenRequest,
) -> Result<SessionOutcome> {
    require_name(&req.name)?;
    let runner = EnvRunner::new(runner, &ctx.env);
    let runner = &runner;
    let global = config_mgr.load_global_config()?;
    let name = req.name.clone();

    let _workspace_lock = config_mgr.lock_workspace(&name)?;
    let mut plan = build_reopen_plan(config_mgr, runner, &name, &req.options, prompt)?;
    ctx.emit(ServiceEvent::Started {
        operation: "reopen",
        workspace: name.clone(),
    });
    plan.apply_current_terminal_config(&global);
    if req.run_agent.is_some() {
        plan.workspace.agent_cli = selected_agent_cli_value(&req.run_agent, &global)?;
    }
    let lifecycle = ReopenLifecyclePlan {
        skip_hooks: req.skip_hooks,
        activate_terminal_environment: !req.no_multiplexer,
        run_agent: req.run_agent.is_some(),
    };
    let formatted = format_reopen_plan(&plan, &lifecycle);
    ctx.step(formatted.strip_suffix('\n').unwrap_or(&formatted));
    if req.dry_run {
        ctx.step("dry run: no changes made");
        return Ok(SessionOutcome {
            workspace: plan.workspace.name,
            dry_run: true,
            attach: None,
        });
    }

    if plan
        .repos
        .iter()
        .any(|repo| matches!(repo.worktree_action, WorktreeAction::Overwrite { .. }))
    {
        let caller = CallerContext::from_vars(|var| ctx.env.get(var).map(str::to_string));
        let warnings = close_terminal_environment_for_reopen(
            config_mgr,
            &global,
            &plan.workspace.name,
            runner,
            &caller,
        )?;
        report_terminal_environment_warnings(ctx, &plan.workspace.name, warnings);
    }

    let workspace = execute_reopen_plan(config_mgr, &global, runner, plan, req.skip_hooks)?;
    let attach = if req.no_multiplexer {
        None
    } else {
        activate_terminal_environment(
            config_mgr,
            &global,
            &workspace,
            runner,
            ctx,
            agent_intent(req.run_agent.clone()),
        )
        .map_err(|error| {
            anyhow::anyhow!(
                "workspace '{}' reopened and remains in_progress, but terminal environment activation failed: {error:#}. Run `zootree open {}` to retry",
                workspace.name,
                workspace.name
            )
        })?
    };
    Ok(SessionOutcome {
        workspace: workspace.name,
        dry_run: false,
        attach,
    })
}
