use serde::{Deserialize, Serialize};
use super::global::{HooksConfig, ZellijConfig};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LazyGitConfig {
    pub config: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RepoConfig {
    pub path: String,
    pub default_target_branch: Option<String>,
    #[serde(default)]
    pub copy_files: Vec<String>,
    #[serde(default)]
    pub hooks: HooksConfig,
    pub lazygit: Option<LazyGitConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zellij: Option<ZellijConfig>,
}
