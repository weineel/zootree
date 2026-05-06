use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkspaceConfig {
    pub title: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub branch: String,
    pub workspace_dir: String,
    pub created_at: String,
    pub layout: Option<String>,
    #[serde(default = "default_session_mode")]
    pub session_mode: String,
    pub session_name: Option<String>,
    #[serde(default)]
    pub repos: Vec<RepoEntry>,
    #[serde(default)]
    pub events: Vec<Event>,
}

fn default_session_mode() -> String {
    "standalone".into()
}
