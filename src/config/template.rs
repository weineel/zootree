use super::global::ZellijConfig;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TemplateConfig {
    #[serde(default)]
    pub repos: Vec<String>,
    #[serde(default)]
    pub zellij: ZellijConfig,
}
