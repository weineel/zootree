use serde::{Deserialize, Serialize};

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
pub struct GlobalConfig {
    #[serde(default = "default_zellij_layout")]
    pub zellij_layout: String,
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
}

fn default_zellij_layout() -> String { "default".into() }
fn default_workspace_root() -> String { "~/zootree-workspaces".into() }
fn default_branch_prefix() -> String { "zootree".into() }

impl Default for GlobalConfig {
    fn default() -> Self {
        Self {
            zellij_layout: default_zellij_layout(),
            workspace_root: default_workspace_root(),
            branch_prefix: default_branch_prefix(),
            copy_files: Vec::new(),
            hooks: HooksConfig::default(),
            log: LogConfig::default(),
        }
    }
}
