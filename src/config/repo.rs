use serde::{Deserialize, Serialize};
use super::global::HooksConfig;

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
    pub layout: Option<String>,
}
