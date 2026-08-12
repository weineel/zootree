use super::global::MultiplexerConfig;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceStatus {
    Pending,
    InProgress,
    Done,
    Canceled,
}

impl WorkspaceStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            WorkspaceStatus::Pending => "pending",
            WorkspaceStatus::InProgress => "in_progress",
            WorkspaceStatus::Done => "done",
            WorkspaceStatus::Canceled => "canceled",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct WorkspaceWithStatus {
    pub status: WorkspaceStatus,
    pub config: WorkspaceConfig,
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

/// Opaque persisted state for a workspace's terminal environment.
///
/// Workspace persistence deliberately treats the contents as an uninterpreted
/// TOML table. Only `core::terminal_environment` may assign meaning to the
/// stored fields.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(transparent)]
pub struct StoredTerminalEnvironmentState {
    value: toml::Table,
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
    #[serde(
        default,
        skip_serializing_if = "StoredTerminalEnvironmentState::is_empty"
    )]
    pub multiplexer_state: StoredTerminalEnvironmentState,
    #[serde(default)]
    pub repos: Vec<RepoEntry>,
    #[serde(default)]
    pub events: Vec<Event>,
}

impl StoredTerminalEnvironmentState {
    pub fn is_empty(&self) -> bool {
        self.value.is_empty()
    }

    pub(crate) fn from_table(value: toml::Table) -> Self {
        Self { value }
    }

    pub(crate) fn as_table(&self) -> &toml::Table {
        &self.value
    }
}
