//! Adding a registered repository to an in-progress Workspace.

use crate::config::global::GlobalConfig;
use crate::config::ConfigManager;
use crate::core::workspace_repository::{
    add, AddRepositoryRequest, AddRepositoryResult, TerminalUpdate,
};
use crate::runner::CommandRunner;
use crate::service::{EnvRunner, RequestContext, ServiceEvent};
use anyhow::Result;

/// Takes the Workspace lock, adds the repository and reports the result as
/// events. Interactive workspace and repository selection is the caller's job.
pub fn add_repo_to_workspace<R: CommandRunner>(
    config_mgr: &ConfigManager,
    global: &GlobalConfig,
    runner: &R,
    ctx: &RequestContext<'_>,
    req: &AddRepositoryRequest,
) -> Result<AddRepositoryResult> {
    if req.workspace.is_empty() {
        anyhow::bail!("workspace name is required");
    }
    if req.repo.is_empty() {
        anyhow::bail!("repository name must not be empty");
    }
    let runner = EnvRunner::new(runner, &ctx.env);
    let _workspace_lock = config_mgr.lock_workspace(&req.workspace)?;
    ctx.emit(ServiceEvent::Started {
        operation: "add-repo",
        workspace: req.workspace.clone(),
    });
    let result = add(config_mgr, global, &runner, req)?;

    ctx.step(format!(
        "repository '{}' added to workspace '{}'",
        result.repo, result.workspace
    ));
    ctx.step(format!("  target branch: {}", result.target_branch));
    ctx.step(format!("  workspace branch: {}", result.workspace_branch));
    ctx.step(format!("  worktree: {}", result.worktree_path));
    ctx.step(format!(
        "  terminal: {}",
        match result.terminal {
            TerminalUpdate::Updated => "updated existing environment",
            TerminalUpdate::Absent => "environment absent; skipped",
        }
    ));
    Ok(result)
}
