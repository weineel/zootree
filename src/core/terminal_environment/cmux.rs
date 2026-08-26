use super::{encode_current_state, Activation, AgentIntent, CloseReport, CmuxRepoWorkspaceRef};
use crate::config::global::{GlobalConfig, MultiplexerKind};
use crate::config::workspace::WorkspaceConfig;
use crate::config::ConfigManager;
use crate::core::cmux_layout::{
    default_cmux_anchor_layout, default_cmux_repo_layout, render_cmux_anchor_layout,
    render_cmux_repo_layout, CmuxLayoutVar,
};
use crate::core::layout::{build_agent_cli_command, build_prompt, resolve_agent_cli};
use crate::core::multiplexer::cmux::{
    CmuxCommands, DeleteResult, FocusResult, GroupLookup, GroupSpec, RepoWorkspaceSpec,
};
use crate::runner::CommandRunner;
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct CmuxStatePayload {
    pub(super) group: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) repo_workspaces: Vec<CmuxRepoWorkspaceRef>,
}

pub(super) struct PreparedRepositoryAddition {
    group: String,
    repo_name: String,
    spec: RepoWorkspaceSpec,
    payload: CmuxStatePayload,
}

pub(super) struct AppliedRepositoryAddition {
    group: String,
    workspace: String,
    pub(super) stored_state: crate::config::workspace::StoredTerminalEnvironmentState,
}

pub(super) fn prepare_repository_addition<R: CommandRunner>(
    config_manager: &ConfigManager,
    runner: &R,
    workspace: &WorkspaceConfig,
    repo_name: &str,
    worktree_path: &str,
    stored_payload: Option<CmuxStatePayload>,
    _warnings: Vec<String>,
) -> Result<Option<PreparedRepositoryAddition>> {
    let cmux = CmuxCommands::new(runner);
    let stored_group = stored_payload
        .as_ref()
        .map(|payload| payload.group.as_str())
        .filter(|group| !group.is_empty());
    let group =
        match cmux.lookup_group_without_focus(deterministic_group_name(workspace), stored_group)? {
            GroupLookup::Found(group) => group,
            GroupLookup::NotFound => return Ok(None),
            GroupLookup::Ambiguous => bail!(
                "cmux group '{}' is ambiguous; refusing to guess",
                deterministic_group_name(workspace)
            ),
        };
    let workspace_name = repo_workspace_name(workspace, repo_name);
    if cmux
        .workspace_names()?
        .iter()
        .any(|name| name == &workspace_name)
    {
        bail!("cmux already contains a workspace named '{workspace_name}'; refusing to adopt it");
    }
    let layout_name = workspace
        .multiplexer
        .cmux
        .layout
        .as_deref()
        .unwrap_or("default");
    if layout_name != "default" {
        bail!(
            "incremental cmux repository addition currently supports only layout = \"default\"; workspace '{}' selected '{}'",
            workspace.name,
            layout_name
        );
    }
    let repo_config = config_manager.load_repo_config(repo_name)?;
    let vars = CmuxLayoutVar {
        repo_name: repo_name.into(),
        worktree_path: worktree_path.into(),
        branch: workspace.branch.clone(),
        workspace_name: workspace.name.clone(),
        workspace_dir: shellexpand::tilde(&workspace.workspace_dir).into_owned(),
        lazygit_config: repo_config
            .lazygit
            .map(|config| config.config)
            .unwrap_or_default(),
        overview_agent_command: String::new(),
        repo_agent_command: String::new(),
    };
    let repo_workspaces = if stored_group == Some(group.as_str()) {
        stored_payload
            .map(|payload| payload.repo_workspaces)
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    Ok(Some(PreparedRepositoryAddition {
        group: group.clone(),
        repo_name: repo_name.into(),
        spec: RepoWorkspaceSpec {
            repo_name: repo_name.into(),
            workspace_name,
            description: repo_name.into(),
            cwd: worktree_path.into(),
            layout: render_cmux_repo_layout(default_cmux_repo_layout(), &vars, None)?,
        },
        payload: CmuxStatePayload {
            group,
            repo_workspaces,
        },
    }))
}

pub(super) fn apply_repository_addition<R: CommandRunner>(
    runner: &R,
    mut prepared: PreparedRepositoryAddition,
) -> Result<AppliedRepositoryAddition> {
    let workspace =
        CmuxCommands::new(runner).create_repo_workspace(&prepared.spec, &prepared.group)?;
    prepared.payload.repo_workspaces.push(CmuxRepoWorkspaceRef {
        repo: prepared.repo_name,
        workspace: workspace.clone(),
    });
    let stored_state = encode_current_state(MultiplexerKind::Cmux, &prepared.payload)?;
    Ok(AppliedRepositoryAddition {
        group: prepared.group,
        workspace,
        stored_state,
    })
}

pub(super) fn rollback_repository_addition<R: CommandRunner>(
    runner: &R,
    applied: &AppliedRepositoryAddition,
) -> Result<()> {
    CmuxCommands::new(runner)
        .close_repo_workspace(&applied.workspace)
        .map_err(|error| {
            anyhow::anyhow!(
                "failed to close cmux repo workspace '{}' in group '{}': {error:#}",
                applied.workspace,
                applied.group
            )
        })
}

pub(super) fn activate<R: CommandRunner>(
    config_manager: &ConfigManager,
    global_config: &GlobalConfig,
    runner: &R,
    workspace: &WorkspaceConfig,
    stored_payload: Option<CmuxStatePayload>,
    agent_intent: AgentIntent,
    mut warnings: Vec<String>,
) -> Result<Activation> {
    let cmux = CmuxCommands::new(runner);
    let group_name = deterministic_group_name(workspace);
    let stored_group = stored_payload
        .as_ref()
        .map(|payload| payload.group.as_str())
        .filter(|group| !group.is_empty());

    match cmux.focus_group_or_find(group_name, stored_group)? {
        FocusResult::FocusedExisting => {
            warn_if_agent_was_not_placed(&agent_intent, &mut warnings);
            let payload =
                stored_payload.expect("focused stored cmux group requires stored payload");
            return Ok(Activation {
                stored_state: encode_current_state(MultiplexerKind::Cmux, &payload)?,
                warnings,
            });
        }
        FocusResult::FocusedFound(group) => {
            if let Some(stale_group) = stored_group {
                warnings.push(format!(
                    "stored cmux group '{stale_group}' was stale; adopted '{group}' by name"
                ));
            }
            warn_if_agent_was_not_placed(&agent_intent, &mut warnings);
            return Ok(Activation {
                stored_state: encode_current_state(
                    MultiplexerKind::Cmux,
                    &CmuxStatePayload {
                        group,
                        repo_workspaces: Vec::new(),
                    },
                )?,
                warnings,
            });
        }
        FocusResult::Ambiguous => {
            bail!(
                "cmux group '{}' is ambiguous; refusing to guess or create another group",
                group_name
            )
        }
        FocusResult::NotFound => {
            if let Some(stale_group) = stored_group {
                warnings.push(format!(
                    "stored cmux group '{stale_group}' was stale; creating a new terminal environment"
                ));
            }
        }
    }

    let spec = prepare_group_spec(config_manager, global_config, workspace, &agent_intent)?;
    let created = cmux.create_group_environment(&spec)?;
    let payload = CmuxStatePayload {
        group: created.group,
        repo_workspaces: created
            .repo_workspaces
            .into_iter()
            .map(|created| CmuxRepoWorkspaceRef {
                repo: created.repo,
                workspace: created.workspace,
            })
            .collect(),
    };
    Ok(Activation {
        stored_state: encode_current_state(MultiplexerKind::Cmux, &payload)?,
        warnings,
    })
}

pub(super) fn close<R: CommandRunner>(
    runner: &R,
    workspace: &WorkspaceConfig,
    stored_payload: Option<CmuxStatePayload>,
    mut warnings: Vec<String>,
) -> CloseReport {
    let cmux = CmuxCommands::new(runner);
    let stored_group = stored_payload
        .as_ref()
        .map(|payload| payload.group.as_str())
        .filter(|group| !group.is_empty());

    let closed = match cmux.delete_group(deterministic_group_name(workspace), stored_group) {
        Ok(
            DeleteResult::Deleted { stored_ref_failure }
            | DeleteResult::NotFound { stored_ref_failure },
        ) => {
            if let Some(failure) = stored_ref_failure {
                warnings.push(format!("{failure}; completed close fallback by name"));
            }
            true
        }
        Ok(DeleteResult::Ambiguous { stored_ref_failure }) => {
            if let Some(failure) = stored_ref_failure {
                warnings.push(format!("{failure}; attempted close fallback by name"));
            }
            warnings.push(format!(
                "cmux group '{}' is ambiguous; terminal environment was not closed",
                deterministic_group_name(workspace)
            ));
            false
        }
        Err(error) => {
            warnings.push(format!(
                "failed to close cmux terminal environment for workspace '{}': {error:#}",
                workspace.name
            ));
            false
        }
    };

    CloseReport { closed, warnings }
}

fn deterministic_group_name(workspace: &WorkspaceConfig) -> &str {
    &workspace.title
}

fn display_name(workspace: &WorkspaceConfig) -> String {
    format!("zootree-{}", workspace.name)
}

fn repo_workspace_name(workspace: &WorkspaceConfig, repo_name: &str) -> String {
    format!("{}-{repo_name}", display_name(workspace))
}

fn warn_if_agent_was_not_placed(agent_intent: &AgentIntent, warnings: &mut Vec<String>) {
    if !matches!(agent_intent, AgentIntent::None) {
        warnings.push(
            "agent request was ignored because the cmux terminal environment already exists".into(),
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

fn prepare_group_spec(
    config_manager: &ConfigManager,
    global_config: &GlobalConfig,
    workspace: &WorkspaceConfig,
    agent_intent: &AgentIntent,
) -> Result<GroupSpec> {
    let layout_name = workspace
        .multiplexer
        .cmux
        .layout
        .as_deref()
        .unwrap_or("default");
    if layout_name != "default" {
        bail!(
            "group-aware cmux currently supports only layout = \"default\"; workspace '{}' selected '{}'",
            workspace.name,
            layout_name
        );
    }

    let workspace_dir = shellexpand::tilde(&workspace.workspace_dir).into_owned();
    let agent_template = resolve_agent_template(global_config, agent_intent)?;
    let prompt = build_prompt(workspace);
    let agent_command = agent_template
        .map(|template| build_agent_cli_command(template, &prompt))
        .transpose()?;
    let single_repo = workspace.repos.len() == 1;
    let mut vars = Vec::with_capacity(workspace.repos.len());
    for repo_entry in &workspace.repos {
        let repo_config = config_manager.load_repo_config(&repo_entry.name)?;
        let lazygit_config = repo_config
            .lazygit
            .map(|lazygit| lazygit.config)
            .unwrap_or_default();
        vars.push(CmuxLayoutVar {
            repo_name: repo_entry.name.clone(),
            worktree_path: format!("{workspace_dir}/{}", repo_entry.name),
            branch: workspace.branch.clone(),
            workspace_name: workspace.name.clone(),
            workspace_dir: workspace_dir.clone(),
            lazygit_config,
            overview_agent_command: String::new(),
            repo_agent_command: String::new(),
        });
    }

    let anchor_agent = (!single_repo).then_some(agent_command.as_deref()).flatten();
    let repo_agent = single_repo.then_some(agent_command.as_deref()).flatten();
    let anchor_layout =
        render_cmux_anchor_layout(default_cmux_anchor_layout(), &vars, anchor_agent)?;
    let repo_workspaces = vars
        .iter()
        .map(|repo| {
            Ok(RepoWorkspaceSpec {
                repo_name: repo.repo_name.clone(),
                workspace_name: repo_workspace_name(workspace, &repo.repo_name),
                description: repo.repo_name.clone(),
                cwd: repo.worktree_path.clone().into(),
                layout: render_cmux_repo_layout(default_cmux_repo_layout(), repo, repo_agent)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(GroupSpec {
        group_name: deterministic_group_name(workspace).into(),
        anchor_name: display_name(workspace),
        anchor_description: workspace.title.clone(),
        anchor_cwd: workspace_dir.into(),
        anchor_layout,
        repo_workspaces,
    })
}
