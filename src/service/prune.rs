//! Pruning archived (done / canceled) Workspaces.

use crate::config::workspace::{WorkspaceConfig, WorkspaceStatus};
use crate::config::ConfigManager;
use crate::service::RequestContext;
use anyhow::Result;
use std::path::Path;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PruneRequest {
    /// Prune every archived Workspace; `targets` is ignored.
    pub all: bool,
    /// Archived Workspaces to prune when `all` is false.
    pub targets: Vec<PruneTarget>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PruneTarget {
    pub name: String,
    pub status: WorkspaceStatus,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PruneOutcome {
    pub pruned: Vec<String>,
}

/// All archived Workspaces, done first and then canceled.
pub fn archived_workspaces(
    config_mgr: &ConfigManager,
) -> Result<Vec<(WorkspaceStatus, WorkspaceConfig)>> {
    let mut archived = Vec::new();
    for status in [WorkspaceStatus::Done, WorkspaceStatus::Canceled] {
        let workspaces = config_mgr.list_workspaces(Some(std::slice::from_ref(&status)))?;
        for ws in workspaces {
            archived.push((status.clone(), ws));
        }
    }
    Ok(archived)
}

pub fn prune_workspaces(
    config_mgr: &ConfigManager,
    ctx: &RequestContext<'_>,
    req: &PruneRequest,
) -> Result<PruneOutcome> {
    let archived = archived_workspaces(config_mgr)?;
    if archived.is_empty() {
        ctx.step("no archived workspaces to prune");
        return Ok(PruneOutcome::default());
    }

    let to_prune: Vec<(WorkspaceStatus, WorkspaceConfig)> = if req.all {
        archived
    } else {
        let mut selected = Vec::new();
        for target in &req.targets {
            let found = archived
                .iter()
                .find(|(status, ws)| *status == target.status && ws.name == target.name)
                .ok_or_else(|| anyhow::anyhow!("archived workspace '{}' not found", target.name))?;
            selected.push(found.clone());
        }
        selected
    };

    if to_prune.is_empty() {
        ctx.step("nothing selected");
        return Ok(PruneOutcome::default());
    }

    let mut pruned = Vec::new();
    for (status, ws) in &to_prune {
        let ws_dir = shellexpand::tilde(&ws.workspace_dir).into_owned();

        if Path::new(&ws_dir).exists() {
            std::fs::remove_dir_all(&ws_dir)?;
            ctx.step(format!("  removed directory: {}", ws_dir));
        }

        config_mgr.delete_workspace_config(&ws.name, status)?;
        ctx.step(format!("  pruned: {}", ws.name));
        pruned.push(ws.name.clone());
    }

    ctx.step(format!("{} workspace(s) pruned", to_prune.len()));
    Ok(PruneOutcome { pruned })
}
