use std::collections::BTreeMap;

use crate::config::workspace::WorkspaceConfig;

pub struct LayoutVar {
    pub repo_name: String,
    pub worktree_path: String,
    pub branch: String,
    pub workspace_name: String,
    pub workspace_dir: String,
    pub lazygit_config: String,
    pub overview_agent_cli: String,
    pub repo_agent_cli: String,
}

pub struct LayoutRenderer;

impl LayoutRenderer {
    pub fn replace_vars(template: &str, vars: &LayoutVar) -> String {
        let mut result = template.to_string();

        // Handle empty lazygit_config: remove "-ucf" "$lazygit_config" pair
        if vars.lazygit_config.is_empty() {
            result = result.replace(r#" "-ucf" "$lazygit_config""#, "");
        }

        result = result.replace("$repo_name", &vars.repo_name);
        result = result.replace("$worktree_path", &vars.worktree_path);
        result = result.replace("$branch", &vars.branch);
        result = result.replace("$workspace_name", &vars.workspace_name);
        result = result.replace("$workspace_dir", &vars.workspace_dir);
        result = result.replace("$lazygit_config", &vars.lazygit_config);
        // agent_cli placeholders MUST be substituted last so injected KDL fragments
        // don't get re-processed by the standard variable replacements.
        result = result.replace("$overview_agent_cli", &vars.overview_agent_cli);
        result = result.replace("$repo_agent_cli", &vars.repo_agent_cli);
        result
    }

    pub fn render(template: &str, repos: &[LayoutVar]) -> String {
        let marker = "// @repeat-per-repo";
        let Some(marker_pos) = template.find(marker) else {
            if let Some(vars) = repos.first() {
                return Self::replace_vars(template, vars);
            }
            return template.to_string();
        };

        let before_marker = &template[..marker_pos];
        let after_marker = &template[marker_pos + marker.len()..];
        let after_marker = after_marker.trim_start_matches('\n');

        let tab_block = Self::extract_tab_block(after_marker);
        let after_tab = &after_marker[tab_block.len()..];

        let workspace_vars = repos.first();

        let before = if let Some(vars) = workspace_vars {
            Self::replace_vars(before_marker, vars)
        } else {
            before_marker.to_string()
        };

        let after = if let Some(vars) = workspace_vars {
            Self::replace_vars(after_tab, vars)
        } else {
            after_tab.to_string()
        };

        let mut expanded = String::new();
        for (i, vars) in repos.iter().enumerate() {
            if i > 0 {
                expanded.push('\n');
            }
            expanded.push_str(&Self::replace_vars(tab_block, vars));
        }

        format!("{}\n\n{}{}", before.trim_end_matches('\n'), expanded, after)
    }

    fn extract_tab_block(s: &str) -> &str {
        let mut depth = 0;
        let mut started = false;
        let mut end = 0;

        for (i, ch) in s.char_indices() {
            match ch {
                '{' => {
                    depth += 1;
                    started = true;
                }
                '}' => {
                    depth -= 1;
                    if started && depth == 0 {
                        end = i + 1;
                        break;
                    }
                }
                _ => {}
            }
        }

        &s[..end]
    }

    pub fn default_layout() -> &'static str {
        r#"// 自动生成，修改无效，仅作参考和调试用途
layout {
    default_tab_template {
        pane size=1 borderless=true {
            plugin location="tab-bar"
        }
        children
        pane size=1 borderless=true {
            plugin location="status-bar"
        }
    }

    tab name="overview" {
        pane split_direction="vertical" {
            pane command="zootree" {
                args "info" "$workspace_name" "--watch"
            }
            pane cwd="$workspace_dir" $overview_agent_cli
        }
    }

    // @repeat-per-repo
    tab name="$repo_name" {
        pane split_direction="vertical" {
            pane size="60%" command="lazygit" {
                args "-p" "$worktree_path" "-ucf" "$lazygit_config"
            }
            pane {
                pane size="30%" cwd="$worktree_path"
                pane size="70%" cwd="$worktree_path" $repo_agent_cli
            }
        }
    }
}"#
    }
}

pub fn build_prompt(workspace: &WorkspaceConfig) -> String {
    if workspace.description.is_empty() {
        workspace.title.clone()
    } else {
        format!("{}\n{}", workspace.title, workspace.description)
    }
}

fn kdl_escape(s: &str) -> String {
    s.replace('\\', r"\\")
        .replace('"', r#"\""#)
        .replace('\n', r"\n")
        .replace('\r', r"\r")
        .replace('\t', r"\t")
}

pub fn build_agent_cli_kdl(agent_cli_tpl: &str, prompt: &str) -> anyhow::Result<String> {
    let tokens = shlex::split(agent_cli_tpl)
        .ok_or_else(|| anyhow::anyhow!("failed to parse agent_cli: {}", agent_cli_tpl))?;
    if tokens.is_empty() {
        anyhow::bail!("agent_cli is empty");
    }

    let substituted: Vec<String> = tokens
        .into_iter()
        .map(|t| t.replace("$prompt", prompt))
        .collect();

    let command = kdl_escape(&substituted[0]);
    let args = &substituted[1..];

    if args.is_empty() {
        Ok(format!(r#"command="{}""#, command))
    } else {
        let escaped_args: Vec<String> = args
            .iter()
            .map(|a| format!(r#""{}""#, kdl_escape(a)))
            .collect();
        Ok(format!(
            "command=\"{}\" {{\n    args {}\n}}",
            command,
            escaped_args.join(" ")
        ))
    }
}

pub fn build_agent_cli_command(agent_cli_tpl: &str, prompt: &str) -> anyhow::Result<String> {
    let tokens = shlex::split(agent_cli_tpl)
        .ok_or_else(|| anyhow::anyhow!("failed to parse agent_cli: {}", agent_cli_tpl))?;
    if tokens.is_empty() {
        anyhow::bail!("agent_cli is empty");
    }

    let substituted: Vec<String> = tokens
        .into_iter()
        .map(|t| t.replace("$prompt", prompt))
        .collect();

    shlex::try_join(substituted.iter().map(String::as_str))
        .map_err(|e| anyhow::anyhow!("failed to join agent_cli: {}", e))
}

/// Resolve an agent_cli value against the alias map (single level).
///
/// If `value` is a key in `alias_map`, returns the alias's template; otherwise
/// returns `value` unchanged so it can be used as a literal command string.
pub fn resolve_agent_cli<'a>(value: &'a str, alias_map: &'a BTreeMap<String, String>) -> &'a str {
    alias_map.get(value).map(String::as_str).unwrap_or(value)
}

/// Resolved alias info: which alias key was hit, and the alias's raw template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AliasInfo {
    pub name: String,
    pub template: String,
}

/// Display-ready agent command, plus optional alias provenance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentDisplay {
    /// Shell-quoted command line with `$prompt` substituted.
    pub command: String,
    /// `Some(..)` when `agent_cli_tpl` was a key in the alias map.
    pub alias: Option<AliasInfo>,
}

/// Resolved display of agent_cli with `$prompt` substituted, suitable for showing in `zootree info`.
///
/// Resolves `agent_cli_tpl` against `alias_map` (single level, same semantics as
/// [`resolve_agent_cli`]) before parsing — so when `agent_cli_tpl` is an alias
/// key, the displayed command reflects the alias's underlying template.
///
/// - Returns `None` when `agent_cli_tpl` is `None` (caller should fall back to displaying `build_prompt`).
/// - Returns `Some(Err(..))` when the (possibly alias-resolved) template fails to parse via shlex,
///   is empty, or fails to re-join after substitution.
/// - Returns `Some(Ok(display))` where `display.command` is a single shell-quoted command string;
///   `display.alias` is `Some(..)` if the input matched an alias key.
pub fn build_agent_cli_display(
    agent_cli_tpl: Option<&str>,
    alias_map: &BTreeMap<String, String>,
    workspace: &WorkspaceConfig,
) -> Option<anyhow::Result<AgentDisplay>> {
    let tpl = agent_cli_tpl?;
    let alias = alias_map.get(tpl).map(|template| AliasInfo {
        name: tpl.to_string(),
        template: template.clone(),
    });
    let resolved: &str = alias.as_ref().map(|a| a.template.as_str()).unwrap_or(tpl);
    let prompt = build_prompt(workspace);

    let result = (|| -> anyhow::Result<String> {
        let tokens = shlex::split(resolved)
            .ok_or_else(|| anyhow::anyhow!("failed to parse agent_cli: {}", resolved))?;
        if tokens.is_empty() {
            anyhow::bail!("agent_cli is empty");
        }
        let substituted: Vec<String> = tokens
            .into_iter()
            .map(|t| t.replace("$prompt", &prompt))
            .collect();
        let joined = shlex::try_join(substituted.iter().map(|s| s.as_str()))
            .map_err(|e| anyhow::anyhow!("failed to join agent_cli: {}", e))?;
        Ok(joined)
    })();

    Some(result.map(|command| AgentDisplay { command, alias }))
}
