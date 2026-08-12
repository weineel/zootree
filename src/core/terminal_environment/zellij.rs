use super::{encode_current_state, Activation, AgentIntent, CloseReport};
use crate::config::global::{GlobalConfig, MultiplexerKind};
use crate::config::workspace::WorkspaceConfig;
use crate::config::ConfigManager;
use crate::core::layout::{
    build_agent_cli_kdl, build_prompt, resolve_agent_cli, LayoutRenderer, LayoutVar,
};
use crate::core::multiplexer::zellij::{SessionLookup, ZellijCommands};
use crate::runner::CommandRunner;
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct ZellijStatePayload {
    pub(super) session: String,
}

pub(super) struct ZellijAdapter<'a, R: CommandRunner> {
    config_manager: &'a ConfigManager,
    global_config: &'a GlobalConfig,
    runner: &'a R,
    in_zellij: bool,
}

impl<'a, R: CommandRunner> ZellijAdapter<'a, R> {
    pub(super) fn new(
        config_manager: &'a ConfigManager,
        global_config: &'a GlobalConfig,
        runner: &'a R,
        in_zellij: bool,
    ) -> Self {
        Self {
            config_manager,
            global_config,
            runner,
            in_zellij,
        }
    }

    pub(super) fn activate(
        &self,
        workspace: &WorkspaceConfig,
        stored_payload: Option<ZellijStatePayload>,
        agent_intent: AgentIntent,
        mut warnings: Vec<String>,
    ) -> Result<Activation> {
        let zellij = ZellijCommands::new(self.runner, self.in_zellij);
        let display_name = deterministic_display_name(workspace);
        let stored_session = stored_payload
            .as_ref()
            .map(|payload| payload.session.as_str())
            .filter(|session| !session.is_empty());

        if let Some(session) = stored_session {
            match zellij.lookup_session(session)? {
                SessionLookup::Unique => {
                    warn_if_agent_was_not_placed(&agent_intent, &mut warnings);
                    zellij.activate_existing(session, &workspace.name)?;
                    return activation(session, warnings);
                }
                SessionLookup::Ambiguous => {
                    bail!(
                        "stored Zellij session '{}' is ambiguous; refusing to guess",
                        session
                    )
                }
                SessionLookup::NotFound => warnings.push(format!(
                    "stored Zellij session '{session}' was stale; reconciling by display name"
                )),
            }
        }

        if stored_session != Some(display_name.as_str()) {
            match zellij.lookup_session(&display_name)? {
                SessionLookup::Unique => {
                    warn_if_agent_was_not_placed(&agent_intent, &mut warnings);
                    zellij.activate_existing(&display_name, &workspace.name)?;
                    return activation(&display_name, warnings);
                }
                SessionLookup::Ambiguous => {
                    bail!(
                        "Zellij session '{}' is ambiguous; refusing to guess or create another session",
                        display_name
                    )
                }
                SessionLookup::NotFound => {}
            }
        }

        let layout_file = prepare_layout(
            self.config_manager,
            self.global_config,
            workspace,
            &agent_intent,
            &mut warnings,
        )?;
        zellij.create_session(&display_name, &workspace.name, &layout_file)?;
        activation(&display_name, warnings)
    }

    pub(super) fn close(
        &self,
        workspace: &WorkspaceConfig,
        stored_payload: Option<ZellijStatePayload>,
        mut warnings: Vec<String>,
    ) -> CloseReport {
        let zellij = ZellijCommands::new(self.runner, self.in_zellij);
        let display_name = deterministic_display_name(workspace);
        let stored_session = stored_payload
            .as_ref()
            .map(|payload| payload.session.as_str())
            .filter(|session| !session.is_empty());

        if let Some(session) = stored_session {
            match close_named_session(&zellij, session, workspace, &mut warnings) {
                CloseLookupOutcome::Closed | CloseLookupOutcome::Failed => {
                    return CloseReport { warnings };
                }
                CloseLookupOutcome::NotFound if session == display_name => {
                    return CloseReport { warnings };
                }
                CloseLookupOutcome::NotFound => {}
            }
        }

        close_named_session(&zellij, &display_name, workspace, &mut warnings);
        CloseReport { warnings }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CloseLookupOutcome {
    Closed,
    NotFound,
    Failed,
}

fn close_named_session<R: CommandRunner>(
    zellij: &ZellijCommands<'_, R>,
    session: &str,
    workspace: &WorkspaceConfig,
    warnings: &mut Vec<String>,
) -> CloseLookupOutcome {
    match zellij.lookup_session(session) {
        Ok(SessionLookup::NotFound) => CloseLookupOutcome::NotFound,
        Ok(SessionLookup::Ambiguous) => {
            warnings.push(format!(
                "Zellij session '{session}' is ambiguous; terminal environment for workspace '{}' was not closed",
                workspace.name
            ));
            CloseLookupOutcome::Failed
        }
        Ok(SessionLookup::Unique) => match zellij.delete_session_checked(session) {
            Ok(()) => CloseLookupOutcome::Closed,
            Err(error) => {
                warnings.push(format!(
                    "failed to close Zellij terminal environment for workspace '{}': {error:#}",
                    workspace.name
                ));
                CloseLookupOutcome::Failed
            }
        },
        Err(error) => {
            warnings.push(format!(
                "failed to inspect Zellij terminal environment for workspace '{}': {error:#}",
                workspace.name
            ));
            CloseLookupOutcome::Failed
        }
    }
}

fn activation(session: &str, warnings: Vec<String>) -> Result<Activation> {
    Ok(Activation {
        stored_state: encode_current_state(
            MultiplexerKind::Zellij,
            &ZellijStatePayload {
                session: session.into(),
            },
        )?,
        warnings,
    })
}

fn deterministic_display_name(workspace: &WorkspaceConfig) -> String {
    format!("zootree-{}", workspace.name)
}

fn warn_if_agent_was_not_placed(agent_intent: &AgentIntent, warnings: &mut Vec<String>) {
    if !matches!(agent_intent, AgentIntent::None) {
        warnings.push(
            "agent request was ignored because the Zellij terminal environment already exists"
                .into(),
        );
    }
}

fn resolve_agent_template<'a>(
    global_config: &'a GlobalConfig,
    agent_intent: &'a AgentIntent,
) -> Result<Option<&'a str>> {
    let requested = match agent_intent {
        AgentIntent::None => return Ok(None),
        AgentIntent::Default => global_config.agent_cli.as_deref().ok_or_else(|| {
            anyhow::anyhow!(
                "--run-agent requires agent_cli in global config (~/.config/zootree/config.toml)"
            )
        })?,
        AgentIntent::Override(command) if command.is_empty() => {
            bail!("agent_cli override is empty")
        }
        AgentIntent::Override(command) => command,
    };

    Ok(Some(resolve_agent_cli(
        requested,
        &global_config.agent_cli_alias,
    )))
}

fn prepare_layout(
    config_manager: &ConfigManager,
    global_config: &GlobalConfig,
    workspace: &WorkspaceConfig,
    agent_intent: &AgentIntent,
    warnings: &mut Vec<String>,
) -> Result<std::path::PathBuf> {
    let layout_name = workspace
        .multiplexer
        .zellij
        .layout
        .as_deref()
        .unwrap_or("default");
    let layout_dir = config_manager.base_dir.join("layouts");
    std::fs::create_dir_all(&layout_dir)?;
    let template_content = if layout_name == "default" {
        let content = LayoutRenderer::default_layout().to_string();
        std::fs::write(layout_dir.join("default.kdl"), &content)?;
        content
    } else {
        let layout_path = layout_dir.join(format!("{layout_name}.kdl"));
        if !layout_path.exists() {
            bail!(
                "zellij layout '{}' not found at {}",
                layout_name,
                layout_path.display()
            );
        }
        std::fs::read_to_string(layout_path)?
    };

    let workspace_dir = shellexpand::tilde(&workspace.workspace_dir).into_owned();
    let agent_template = resolve_agent_template(global_config, agent_intent)?;
    let (overview_agent_cli, first_repo_agent_cli) =
        build_agent_fragments(workspace, agent_template)?;
    let mut vars = Vec::with_capacity(workspace.repos.len());
    for (index, repo_entry) in workspace.repos.iter().enumerate() {
        let repo_config = config_manager.load_repo_config(&repo_entry.name)?;
        let lazygit_config = repo_config
            .lazygit
            .map(|lazygit| lazygit.config)
            .unwrap_or_default();
        vars.push(LayoutVar {
            repo_name: repo_entry.name.clone(),
            worktree_path: format!("{workspace_dir}/{}", repo_entry.name),
            branch: workspace.branch.clone(),
            workspace_name: workspace.name.clone(),
            workspace_dir: workspace_dir.clone(),
            lazygit_config,
            overview_agent_cli: overview_agent_cli.clone(),
            repo_agent_cli: if index == 0 {
                first_repo_agent_cli.clone()
            } else {
                String::new()
            },
        });
    }

    let rendered_layout = LayoutRenderer::render(&template_content, &vars);
    if !matches!(agent_intent, AgentIntent::None)
        && !template_content.contains("$overview_agent_cli")
        && !template_content.contains("$repo_agent_cli")
    {
        warnings.push(format!(
            "agent request was ignored because Zellij layout '{layout_name}' contains no agent placeholder"
        ));
    }

    let layout_file = layout_dir.join("recently.kdl");
    std::fs::write(&layout_file, &rendered_layout)?;

    Ok(layout_file)
}

fn build_agent_fragments(
    workspace: &WorkspaceConfig,
    agent_template: Option<&str>,
) -> Result<(String, String)> {
    match agent_template {
        None => Ok((String::new(), String::new())),
        Some(template) => {
            let kdl = build_agent_cli_kdl(template, &build_prompt(workspace))?;
            if workspace.repos.len() == 1 {
                Ok((String::new(), kdl))
            } else {
                Ok((kdl, String::new()))
            }
        }
    }
}
