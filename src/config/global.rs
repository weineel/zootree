use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum HookValue {
    Simple(String),
    File { file: String },
    Inline { inline: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct HooksConfig {
    pub post_create: Option<HookValue>,
    pub pre_remove: Option<HookValue>,
    pub post_start: Option<HookValue>,
    pub pre_done: Option<HookValue>,
    pub pre_cancel: Option<HookValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LogConfig {
    pub dir: Option<String>,
    pub max_files: Option<u32>,
    pub max_size: Option<String>,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            dir: None,
            max_files: Some(5),
            max_size: Some("10MB".into()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ZellijConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layout: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_name: Option<String>,
}

impl Default for ZellijConfig {
    fn default() -> Self {
        Self {
            layout: Some("default".into()),
            session_mode: None,
            session_name: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GlobalConfig {
    #[serde(default)]
    pub zellij: ZellijConfig,
    #[serde(default = "default_workspace_root")]
    pub workspace_root: String,
    #[serde(default = "default_branch_prefix")]
    pub branch_prefix: String,
    #[serde(default)]
    pub copy_files: Vec<String>,
    #[serde(default)]
    pub hooks: HooksConfig,
    #[serde(default)]
    pub log: LogConfig,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_cli: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub agent_cli_alias: BTreeMap<String, String>,
}

fn default_workspace_root() -> String {
    "~/zootree-workspaces".into()
}
fn default_branch_prefix() -> String {
    "zootree".into()
}

impl Default for GlobalConfig {
    fn default() -> Self {
        Self {
            zellij: ZellijConfig::default(),
            workspace_root: default_workspace_root(),
            branch_prefix: default_branch_prefix(),
            copy_files: Vec::new(),
            hooks: HooksConfig::default(),
            log: LogConfig::default(),
            agent_cli: None,
            agent_cli_alias: BTreeMap::new(),
        }
    }
}
