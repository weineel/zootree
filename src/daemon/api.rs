//! Read-only API bodies. Synchronous on purpose: the handlers run these on
//! `spawn_blocking`. Bodies reuse the published `--json` views unchanged.

use super::error::ApiError;
use crate::cli::json_view;
use crate::config::workspace::WorkspaceStatus;
use crate::config::{name, ConfigManager};
use crate::core::agent_cli::AgentCatalog;
use serde::Serialize;
use serde_json::Value;

fn to_value<T: Serialize>(value: &T) -> Result<Value, ApiError> {
    serde_json::to_value(value).map_err(|e| ApiError::internal(format!("{e:#}")))
}

fn internal(e: anyhow::Error) -> ApiError {
    ApiError::internal(format!("{e:#}"))
}

/// Parses `status=a&status=b,c` from a raw query string. Unknown parameters
/// are ignored. No filter means pending + in_progress, like `list`.
pub fn parse_status_filter(query: Option<&str>) -> Result<Vec<WorkspaceStatus>, ApiError> {
    let mut out = Vec::new();
    for pair in query.unwrap_or("").split('&') {
        let Some(("status", values)) = pair.split_once('=') else {
            continue;
        };
        for value in values.split(',').filter(|v| !v.is_empty()) {
            let status = match value {
                "pending" => WorkspaceStatus::Pending,
                "in_progress" => WorkspaceStatus::InProgress,
                "done" => WorkspaceStatus::Done,
                "canceled" => WorkspaceStatus::Canceled,
                other => {
                    return Err(ApiError::bad_request(format!(
                        "unknown status {other:?}; expected pending, in_progress, done or canceled"
                    )))
                }
            };
            if !out.contains(&status) {
                out.push(status);
            }
        }
    }
    if out.is_empty() {
        out = vec![WorkspaceStatus::Pending, WorkspaceStatus::InProgress];
    }
    Ok(out)
}

pub fn list_workspaces(mgr: &ConfigManager, filter: &[WorkspaceStatus]) -> Result<Value, ApiError> {
    let entries = mgr
        .list_workspaces_with_status(Some(filter))
        .map_err(internal)?;
    let out: Vec<_> = entries
        .iter()
        .map(|e| json_view::workspace_summary(&e.status, &e.config))
        .collect();
    to_value(&out)
}

/// 400 for a malformed name, 404 for an absent Workspace.
pub fn require_workspace(mgr: &ConfigManager, workspace: &str) -> Result<(), ApiError> {
    if !name::is_config_name(workspace) {
        return Err(ApiError::bad_request(format!(
            "invalid workspace name {workspace:?}"
        )));
    }
    // With the name validated, a locate failure can only mean "absent".
    if mgr.locate_workspace_config(workspace).is_err() {
        return Err(ApiError::not_found(format!(
            "workspace '{workspace}' not found"
        )));
    }
    Ok(())
}

pub fn workspace_detail(mgr: &ConfigManager, workspace: &str) -> Result<Value, ApiError> {
    require_workspace(mgr, workspace)?;
    let (status, config) = mgr.load_workspace(workspace).map_err(internal)?;
    let global = mgr.load_global_config().unwrap_or_default();
    to_value(&json_view::workspace_detail(&status, &config, &global))
}

pub fn list_repos(mgr: &ConfigManager) -> Result<Value, ApiError> {
    let mut out = Vec::new();
    for repo in mgr.list_repos().map_err(internal)? {
        let config = mgr.load_repo_config(&repo).map_err(internal)?;
        out.push(json_view::repo_entry(&repo, &config));
    }
    to_value(&out)
}

pub fn agent_catalog(mgr: &ConfigManager) -> Result<Value, ApiError> {
    let global = mgr.load_global_config().map_err(internal)?;
    to_value(&AgentCatalog::from_global(&global))
}
