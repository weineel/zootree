//! Machine-readable views for `list --json`, `info --json` and `repo list --json`.
//!
//! The shapes here are a stable contract for external callers (e.g. Orca).
//! Keep the builders pure so they can be tested without touching the disk.

use serde::Serialize;

use crate::config::global::GlobalConfig;
use crate::config::repo::RepoConfig;
use crate::config::workspace::{Event, WorkspaceConfig, WorkspaceStatus};

#[derive(Debug, Serialize)]
pub struct RepoRefJson {
    pub name: String,
    pub target_branch: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct WorkspaceSummaryJson {
    pub name: String,
    pub title: String,
    pub status: &'static str,
    pub workspace_dir: String,
    pub branch: String,
    pub repos: Vec<RepoRefJson>,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
pub struct WorkspaceDetailJson {
    #[serde(flatten)]
    pub summary: WorkspaceSummaryJson,
    pub description: String,
    pub events: Vec<Event>,
    /// Resolved agent command as argv (alias expanded, `$prompt` substituted).
    /// `None` when no agent is configured; an error string is reported in
    /// `agent_command_error` if the template cannot be parsed.
    pub agent_command: Option<Vec<String>>,
    pub agent_command_error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RepoJson {
    pub name: String,
    pub path: String,
    pub default_target_branch: Option<String>,
}

pub fn workspace_summary(status: &WorkspaceStatus, ws: &WorkspaceConfig) -> WorkspaceSummaryJson {
    WorkspaceSummaryJson {
        name: ws.name.clone(),
        title: ws.title.clone(),
        status: status.as_str(),
        workspace_dir: shellexpand::tilde(&ws.workspace_dir).into_owned(),
        branch: ws.branch.clone(),
        repos: ws
            .repos
            .iter()
            .map(|r| RepoRefJson {
                name: r.name.clone(),
                target_branch: r.target_branch.clone(),
            })
            .collect(),
        created_at: ws.created_at.clone(),
    }
}

pub fn workspace_detail(
    status: &WorkspaceStatus,
    ws: &WorkspaceConfig,
    global: &GlobalConfig,
) -> WorkspaceDetailJson {
    let tpl = ws.agent_cli.as_deref().or(global.agent_cli.as_deref());
    let (agent_command, agent_command_error) = match tpl {
        None => (None, None),
        Some(tpl) => {
            let resolved = crate::core::agent_cli::resolve_agent_cli(tpl, &global.agent_cli_alias);
            let prompt = crate::core::layout::build_prompt(ws);
            match crate::core::layout::build_agent_cli_argv(resolved, &prompt) {
                Ok(argv) => (Some(argv), None),
                Err(e) => (None, Some(format!("{:#}", e))),
            }
        }
    };
    WorkspaceDetailJson {
        summary: workspace_summary(status, ws),
        description: ws.description.clone(),
        events: ws.events.clone(),
        agent_command,
        agent_command_error,
    }
}

pub fn repo_entry(name: &str, config: &RepoConfig) -> RepoJson {
    RepoJson {
        name: name.to_string(),
        path: config.path.clone(),
        default_target_branch: config.default_target_branch.clone(),
    }
}

pub fn to_pretty_json<T: Serialize>(value: &T) -> anyhow::Result<String> {
    Ok(serde_json::to_string_pretty(value)?)
}
