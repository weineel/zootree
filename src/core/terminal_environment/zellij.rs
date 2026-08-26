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
use std::io::Write;

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

pub(super) struct PreparedRepositoryAddition {
    session: String,
    repo_name: String,
    layout: String,
    stored_state: crate::config::workspace::StoredTerminalEnvironmentState,
}

pub(super) struct AppliedRepositoryAddition {
    session: String,
    tab_id: String,
    pub(super) stored_state: crate::config::workspace::StoredTerminalEnvironmentState,
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
                CloseLookupOutcome::Closed => {
                    return CloseReport {
                        closed: true,
                        warnings,
                    };
                }
                CloseLookupOutcome::Failed => {
                    return CloseReport {
                        closed: false,
                        warnings,
                    };
                }
                CloseLookupOutcome::NotFound if session == display_name => {
                    return CloseReport {
                        closed: true,
                        warnings,
                    };
                }
                CloseLookupOutcome::NotFound => {}
            }
        }

        let outcome = close_named_session(&zellij, &display_name, workspace, &mut warnings);
        CloseReport {
            closed: !matches!(outcome, CloseLookupOutcome::Failed),
            warnings,
        }
    }

    pub(super) fn prepare_repository_addition(
        &self,
        workspace: &WorkspaceConfig,
        repo_name: &str,
        worktree_path: &str,
        stored_payload: Option<ZellijStatePayload>,
        _warnings: Vec<String>,
    ) -> Result<Option<PreparedRepositoryAddition>> {
        let zellij = ZellijCommands::new(self.runner, self.in_zellij);
        let display_name = deterministic_display_name(workspace);
        let stored_session = stored_payload
            .as_ref()
            .map(|payload| payload.session.as_str())
            .filter(|session| !session.is_empty());

        let session = if let Some(session) = stored_session {
            match zellij.lookup_session(session)? {
                SessionLookup::Unique => Some(session.to_string()),
                SessionLookup::Ambiguous => {
                    bail!("stored Zellij session '{session}' is ambiguous; refusing to guess")
                }
                SessionLookup::NotFound if session == display_name => None,
                SessionLookup::NotFound => match zellij.lookup_session(&display_name)? {
                    SessionLookup::Unique => Some(display_name.clone()),
                    SessionLookup::Ambiguous => bail!(
                        "Zellij session '{}' is ambiguous; refusing to guess",
                        display_name
                    ),
                    SessionLookup::NotFound => None,
                },
            }
        } else {
            match zellij.lookup_session(&display_name)? {
                SessionLookup::Unique => Some(display_name.clone()),
                SessionLookup::Ambiguous => bail!(
                    "Zellij session '{}' is ambiguous; refusing to guess",
                    display_name
                ),
                SessionLookup::NotFound => None,
            }
        };

        let Some(session) = session else {
            return Ok(None);
        };
        if zellij
            .tab_names(&session)?
            .iter()
            .any(|name| name == repo_name)
        {
            bail!(
                "Zellij session '{session}' already contains a tab named '{repo_name}'; refusing to adopt it"
            );
        }

        let layout_name = workspace
            .multiplexer
            .zellij
            .layout
            .as_deref()
            .unwrap_or("default");
        let template = if layout_name == "default" {
            LayoutRenderer::default_layout().to_string()
        } else {
            let path = self
                .config_manager
                .base_dir
                .join("layouts")
                .join(format!("{layout_name}.kdl"));
            std::fs::read_to_string(&path).map_err(|error| {
                anyhow::anyhow!(
                    "failed to read Zellij layout '{}' at {}: {error}",
                    layout_name,
                    path.display()
                )
            })?
        };
        let repo_config = self.config_manager.load_repo_config(repo_name)?;
        let vars = LayoutVar {
            repo_name: repo_name.into(),
            worktree_path: worktree_path.into(),
            branch: workspace.branch.clone(),
            workspace_name: workspace.name.clone(),
            workspace_dir: shellexpand::tilde(&workspace.workspace_dir).into_owned(),
            lazygit_config: repo_config
                .lazygit
                .map(|config| config.config)
                .unwrap_or_default(),
            overview_agent_cli: String::new(),
            repo_agent_cli: String::new(),
        };
        let tab = LayoutRenderer::render_single_repo_tab(&template, &vars)?;
        let layout = format!("layout {{\n{tab}\n}}\n");
        let stored_state = encode_current_state(
            MultiplexerKind::Zellij,
            &ZellijStatePayload {
                session: session.clone(),
            },
        )?;
        Ok(Some(PreparedRepositoryAddition {
            session,
            repo_name: repo_name.into(),
            layout,
            stored_state,
        }))
    }

    pub(super) fn apply_repository_addition(
        &self,
        prepared: PreparedRepositoryAddition,
    ) -> Result<AppliedRepositoryAddition> {
        let layout_dir = self.config_manager.base_dir.join("layouts");
        std::fs::create_dir_all(&layout_dir)?;
        let layout_path = layout_dir.join(format!(
            ".zootree-add-repo-{}-{}.kdl",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        let mut layout_file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&layout_path)?;
        if let Err(error) = layout_file.write_all(prepared.layout.as_bytes()) {
            let cleanup = std::fs::remove_file(&layout_path);
            return match cleanup {
                Ok(()) => Err(error.into()),
                Err(cleanup_error) => Err(anyhow::anyhow!(
                    "failed to write temporary Zellij layout {}: {error}; cleanup also failed: {cleanup_error}",
                    layout_path.display()
                )),
            };
        }
        drop(layout_file);
        let result = ZellijCommands::new(self.runner, self.in_zellij).create_tab(
            &prepared.session,
            &layout_path,
            &prepared.repo_name,
        );
        let cleanup = std::fs::remove_file(&layout_path);
        let tab_id = match (result, cleanup) {
            (Ok(tab_id), Ok(())) => tab_id,
            (Ok(tab_id), Err(error)) => {
                let rollback = ZellijCommands::new(self.runner, self.in_zellij)
                    .close_tab(&prepared.session, &tab_id);
                return match rollback {
                    Ok(()) => Err(anyhow::anyhow!(
                        "failed to remove temporary Zellij layout {}: {error}",
                        layout_path.display()
                    )),
                    Err(rollback_error) => Err(anyhow::anyhow!(
                        "failed to remove temporary Zellij layout {}: {error}; additionally failed to roll back Zellij tab '{tab_id}': {rollback_error:#}",
                        layout_path.display()
                    )),
                };
            }
            (Err(error), Ok(())) => return Err(error),
            (Err(error), Err(cleanup_error)) => {
                return Err(anyhow::anyhow!(
                    "{error:#}; additionally failed to remove temporary Zellij layout {}: {cleanup_error}",
                    layout_path.display()
                ))
            }
        };
        Ok(AppliedRepositoryAddition {
            session: prepared.session,
            tab_id,
            stored_state: prepared.stored_state,
        })
    }

    pub(super) fn rollback_repository_addition(
        &self,
        applied: &AppliedRepositoryAddition,
    ) -> Result<()> {
        ZellijCommands::new(self.runner, self.in_zellij)
            .close_tab(&applied.session, &applied.tab_id)
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
