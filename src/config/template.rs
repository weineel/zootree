use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TemplateConfig {
    #[serde(default)]
    pub repos: Vec<String>,
    pub layout: Option<String>,
    pub session_mode: Option<String>,
}
