use serde::{Deserialize, Serialize};
use super::global::ZellijConfig;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TemplateConfig {
    #[serde(default)]
    pub repos: Vec<String>,
    #[serde(default)]
    pub zellij: ZellijConfig,
}
