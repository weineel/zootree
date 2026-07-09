use super::global::{MultiplexerConfig, MultiplexerKind};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceStatus {
    Pending,
    InProgress,
    Done,
    Canceled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RepoEntry {
    pub name: String,
    pub target_branch: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Event {
    pub action: String,
    pub timestamp: String,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CmuxRepoWorkspaceState {
    pub repo: String,
    pub workspace: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MultiplexerState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<MultiplexerKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cmux_workspace: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cmux_group: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cmux_anchor_workspace: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cmux_repo_workspaces: Vec<CmuxRepoWorkspaceState>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceConfig {
    pub title: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub branch: String,
    pub workspace_dir: String,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_cli: Option<String>,
    #[serde(default)]
    pub multiplexer: MultiplexerConfig,
    #[serde(default, skip_serializing_if = "MultiplexerState::is_empty")]
    pub multiplexer_state: MultiplexerState,
    #[serde(default)]
    pub repos: Vec<RepoEntry>,
    #[serde(default)]
    pub events: Vec<Event>,
}

impl MultiplexerState {
    pub fn is_empty(&self) -> bool {
        self.kind.is_none()
            && self.cmux_workspace.is_none()
            && self.cmux_group.is_none()
            && self.cmux_anchor_workspace.is_none()
            && self.cmux_repo_workspaces.is_empty()
    }

    pub fn has_cmux_group_state(&self) -> bool {
        self.cmux_group.is_some() || self.cmux_anchor_workspace.is_some()
    }
}
