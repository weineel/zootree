use crate::config::global::GlobalConfig;
use serde::Serialize;
use std::collections::BTreeMap;

/// Resolve an agent_cli value against the alias map (single level).
///
/// If `value` is a key in `alias_map`, returns the alias's template; otherwise
/// returns `value` unchanged so it can be used as a literal command string.
pub fn resolve_agent_cli<'a>(value: &'a str, alias_map: &'a BTreeMap<String, String>) -> &'a str {
    alias_map.get(value).map(String::as_str).unwrap_or(value)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentDefaultKind {
    Alias,
    Literal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentDefault {
    pub value: String,
    pub kind: AgentDefaultKind,
    pub command: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentAlias {
    pub name: String,
    pub command: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentCatalog {
    pub default: Option<AgentDefault>,
    pub aliases: Vec<AgentAlias>,
}

impl AgentCatalog {
    pub fn from_global(global: &GlobalConfig) -> Self {
        let default_alias = global
            .agent_cli
            .as_deref()
            .filter(|value| global.agent_cli_alias.contains_key(*value));
        let default = global.agent_cli.as_deref().map(|value| AgentDefault {
            value: value.to_string(),
            kind: if default_alias.is_some() {
                AgentDefaultKind::Alias
            } else {
                AgentDefaultKind::Literal
            },
            command: resolve_agent_cli(value, &global.agent_cli_alias).to_string(),
        });

        let aliases = default_alias
            .into_iter()
            .chain(
                global
                    .agent_cli_alias
                    .keys()
                    .map(String::as_str)
                    .filter(|name| Some(*name) != default_alias),
            )
            .map(|name| AgentAlias {
                name: name.to_string(),
                command: global.agent_cli_alias[name].clone(),
                is_default: Some(name) == default_alias,
            })
            .collect();

        Self { default, aliases }
    }
}
