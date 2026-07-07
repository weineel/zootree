use super::global::MultiplexerConfig;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TemplateConfig {
    #[serde(default)]
    pub repos: Vec<String>,
    #[serde(default)]
    pub multiplexer: MultiplexerConfig,
}
