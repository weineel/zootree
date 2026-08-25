use super::{encode_current_state, Activation, AgentIntent, CloseReport};
use crate::config::global::{GlobalConfig, MultiplexerKind};
use crate::config::workspace::WorkspaceConfig;
use crate::core::layout::{build_agent_cli_command, build_prompt, resolve_agent_cli};
use crate::core::multiplexer::herdr::{
    CreatedEnvironment, EnvironmentSpec, HerdrCommands, HerdrWorkspace, RepoSpec,
};
use crate::runner::CommandRunner;
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct HerdrStatePayload {
    pub(super) session: String,
    pub(super) workspace_id: String,
    pub(super) label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum HerdrCallerContext {
    Outside,
    Inside { socket_path: Option<String> },
}

impl HerdrCallerContext {
    pub(super) fn from_env() -> Self {
        if std::env::var_os("HERDR_ENV").as_deref() != Some(std::ffi::OsStr::new("1")) {
            return Self::Outside;
        }
        Self::Inside {
            socket_path: std::env::var("HERDR_SOCKET_PATH").ok(),
        }
    }
}

pub(super) fn activate<R: CommandRunner>(
    global_config: &GlobalConfig,
    runner: &R,
    workspace: &WorkspaceConfig,
    stored_payload: Option<HerdrStatePayload>,
    agent_intent: AgentIntent,
    mut warnings: Vec<String>,
    caller: &HerdrCallerContext,
) -> Result<Activation> {
    let commands = HerdrCommands::new(runner);
    commands.ensure_supported_version()?;
    let session = stored_payload
        .as_ref()
        .map(|payload| payload.session.as_str())
        .unwrap_or(&workspace.multiplexer.herdr.session);
    if session.is_empty() {
        bail!("Herdr named session must not be empty");
    }

    let derived_label = display_label(workspace);
    let existing = if let Some(payload) = &stored_payload {
        let mut existing = commands.get_workspace(session, &payload.workspace_id)?;
        if existing.is_none() {
            warnings.push(format!(
                "stored Herdr workspace '{}' was stale; reconciling by label",
                payload.workspace_id
            ));
            existing = find_unique_workspace(&commands, session, &payload.label)?;
        }
        existing
    } else {
        find_unique_workspace(&commands, session, &derived_label)?
    };

    if let Some(existing) = existing {
        if !matches!(agent_intent, AgentIntent::None) {
            warnings.push(
                "agent request was ignored because the Herdr terminal environment already exists"
                    .into(),
            );
        }
        present_existing(&commands, session, &existing, caller, &mut warnings);
        return activation(session, &existing, warnings);
    }

    let spec = environment_spec(
        global_config,
        workspace,
        session,
        &derived_label,
        &agent_intent,
    )?;
    let created = commands.create_environment(&spec)?;
    if let Some(agent_pane_id) = &created.agent_pane_id {
        name_agent(
            &commands,
            session,
            agent_pane_id,
            &agent_name(&workspace.name),
            &mut warnings,
        );
    }
    present_created(&commands, session, &created, caller, &mut warnings);
    activation(session, &created.workspace, warnings)
}

pub(super) fn close<R: CommandRunner>(
    runner: &R,
    workspace: &WorkspaceConfig,
    stored_payload: Option<HerdrStatePayload>,
    mut warnings: Vec<String>,
) -> CloseReport {
    let commands = HerdrCommands::new(runner);
    let session = stored_payload
        .as_ref()
        .map(|payload| payload.session.as_str())
        .unwrap_or(&workspace.multiplexer.herdr.session);
    if session.is_empty() {
        warnings.push("Herdr named session was empty; terminal environment was not closed".into());
        return CloseReport {
            closed: false,
            warnings,
        };
    }

    let target = (|| -> Result<Option<HerdrWorkspace>> {
        if let Some(payload) = &stored_payload {
            if let Some(existing) = commands.get_workspace(session, &payload.workspace_id)? {
                return Ok(Some(existing));
            }
            return find_unique_workspace(&commands, session, &payload.label);
        }
        find_unique_workspace(&commands, session, &display_label(workspace))
    })();

    let closed = match target {
        Ok(Some(target)) => {
            if let Err(error) = commands.close_workspace(session, &target.id) {
                warnings.push(format!(
                    "failed to close Herdr terminal environment for workspace '{}': {error:#}",
                    workspace.name
                ));
                false
            } else {
                true
            }
        }
        Ok(None) => true,
        Err(error) => {
            warnings.push(format!(
                "failed to inspect Herdr terminal environment for workspace '{}': {error:#}",
                workspace.name
            ));
            false
        }
    };
    CloseReport { closed, warnings }
}

fn find_unique_workspace<R: CommandRunner>(
    commands: &HerdrCommands<'_, R>,
    session: &str,
    label: &str,
) -> Result<Option<HerdrWorkspace>> {
    let mut matches = commands
        .list_workspaces(session)?
        .into_iter()
        .filter(|workspace| workspace.label == label);
    let first = matches.next();
    if matches.next().is_some() {
        bail!(
            "Herdr workspace label '{label}' is ambiguous in named session '{session}'; refusing to guess"
        );
    }
    Ok(first)
}

fn environment_spec(
    global_config: &GlobalConfig,
    workspace: &WorkspaceConfig,
    session: &str,
    label: &str,
    agent_intent: &AgentIntent,
) -> Result<EnvironmentSpec> {
    let workspace_cwd = shellexpand::tilde(&workspace.workspace_dir).into_owned();
    let info_command = shlex::try_join(["zootree", "info", workspace.name.as_str(), "--watch"])?;
    let repos = workspace
        .repos
        .iter()
        .map(|repo| RepoSpec {
            name: repo.name.clone(),
            cwd: std::path::Path::new(&workspace_cwd)
                .join(&repo.name)
                .to_string_lossy()
                .into_owned(),
        })
        .collect();
    let agent_command = resolve_agent_template(global_config, agent_intent)?
        .map(|template| build_agent_cli_command(template, &build_prompt(workspace)))
        .transpose()?;
    Ok(EnvironmentSpec {
        session: session.into(),
        label: label.into(),
        workspace_cwd,
        info_command,
        repos,
        agent_command,
    })
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

fn present_existing<R: CommandRunner>(
    commands: &HerdrCommands<'_, R>,
    session: &str,
    workspace: &HerdrWorkspace,
    caller: &HerdrCallerContext,
    warnings: &mut Vec<String>,
) {
    if let Err(error) = commands.focus_workspace(session, &workspace.id) {
        warnings.push(format!(
            "failed to focus Herdr workspace '{}': {error:#}",
            workspace.id
        ));
    }
    match caller {
        HerdrCallerContext::Outside => {
            if let Err(error) = commands.attach_session(session) {
                warnings.push(format!(
                    "failed to attach Herdr session '{session}': {error:#}; run `herdr session attach {session}`"
                ));
            }
        }
        HerdrCallerContext::Inside {
            socket_path: Some(caller_socket),
        } => match commands.session_socket(session) {
            Ok(Some(target_socket)) if target_socket == *caller_socket => {}
            Ok(_) => warnings.push(format!(
                "Herdr workspace was focused in named session '{session}', but the caller is inside another or unknown Herdr session; detach and run `herdr session attach {session}`"
            )),
            Err(error) => warnings.push(format!(
                "could not verify the caller's Herdr named session: {error:#}; detach and run `herdr session attach {session}`"
            )),
        },
        HerdrCallerContext::Inside { socket_path: None } => {
            warnings.push(format!(
                "Herdr workspace was focused in named session '{session}', but the caller session could not be identified; detach and run `herdr session attach {session}`"
            ));
        }
    }
}

fn present_created<R: CommandRunner>(
    commands: &HerdrCommands<'_, R>,
    session: &str,
    environment: &CreatedEnvironment,
    caller: &HerdrCallerContext,
    warnings: &mut Vec<String>,
) {
    let landing_tab = if environment.agent_pane_id.is_some() && environment.repo_tabs.len() == 1 {
        &environment.repo_tabs[0].tab_id
    } else {
        &environment.overview_tab_id
    };
    if let Err(error) = commands.focus_tab(session, landing_tab) {
        warnings.push(format!(
            "failed to focus Herdr tab '{landing_tab}': {error:#}"
        ));
    }
    if environment.agent_pane_id.is_some() && environment.repo_tabs.len() > 1 {
        if let Err(error) = commands.focus_right_from(
            session,
            &environment.overview_info_pane_id,
            &environment.overview_primary_pane_id,
        ) {
            warnings.push(format!(
                "failed to focus Herdr agent pane '{}': {error:#}",
                environment.overview_primary_pane_id
            ));
        }
    }
    present_existing(commands, session, &environment.workspace, caller, warnings);
}

fn display_label(workspace: &WorkspaceConfig) -> String {
    format!("{} · zootree:{}", workspace.title, workspace.name)
}

fn name_agent<R: CommandRunner>(
    commands: &HerdrCommands<'_, R>,
    session: &str,
    pane_id: &str,
    name: &str,
    warnings: &mut Vec<String>,
) {
    name_agent_with_timeout(
        commands,
        session,
        pane_id,
        name,
        std::time::Duration::from_secs(5),
        warnings,
    );
}

fn name_agent_with_timeout<R: CommandRunner>(
    commands: &HerdrCommands<'_, R>,
    session: &str,
    pane_id: &str,
    name: &str,
    timeout: std::time::Duration,
    warnings: &mut Vec<String>,
) {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        match commands.get_agent(session, pane_id) {
            Ok(true) => {
                if let Err(error) = commands.rename_agent(session, pane_id, name) {
                    warnings.push(format!(
                        "failed to name Herdr agent in pane '{pane_id}' as '{name}': {error:#}"
                    ));
                }
                return;
            }
            Ok(false) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Ok(false) => {
                warnings.push(format!(
                    "Herdr did not detect an agent in pane '{pane_id}' within 5 seconds; the agent command is still running"
                ));
                return;
            }
            Err(error) => {
                warnings.push(format!(
                    "failed to detect Herdr agent in pane '{pane_id}': {error:#}"
                ));
                return;
            }
        }
    }
}

fn agent_name(workspace_name: &str) -> String {
    let normalized: String = workspace_name
        .chars()
        .map(|character| match character {
            'A'..='Z' => character.to_ascii_lowercase(),
            'a'..='z' | '0'..='9' | '-' | '_' => character,
            _ => '-',
        })
        .collect();
    let candidate = format!("zt-{normalized}");
    if candidate.len() <= 32 {
        return candidate;
    }
    let mut hash = 0x811c9dc5_u32;
    for byte in candidate.as_bytes() {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(0x01000193);
    }
    format!("{}-{hash:08x}", &candidate[..23])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::global::MultiplexerConfig;
    use crate::config::workspace::{RepoEntry, StoredTerminalEnvironmentState};
    use crate::runner::MockRunner;
    use std::os::unix::process::ExitStatusExt;
    use std::process::{ExitStatus, Output};

    fn success(stdout: &[u8]) -> Output {
        Output {
            status: ExitStatus::from_raw(0),
            stdout: stdout.to_vec(),
            stderr: Vec::new(),
        }
    }

    #[test]
    fn presentation_inside_another_session_warns_without_nesting_a_client() {
        let runner = MockRunner::new();
        runner.push_response(success(
            br#"{"result":{"type":"workspace_info","workspace":{"workspace_id":"w7","label":"Support Herdr"}}}"#,
        ));
        runner.push_response(success(
            br#"{"sessions":[{"name":"agents","running":true,"socket_path":"/tmp/agents.sock"}]}"#,
        ));
        let commands = HerdrCommands::new(&runner);
        let mut warnings = Vec::new();

        present_existing(
            &commands,
            "agents",
            &HerdrWorkspace {
                id: "w7".into(),
                label: "Support Herdr".into(),
            },
            &HerdrCallerContext::Inside {
                socket_path: Some("/tmp/other.sock".into()),
            },
            &mut warnings,
        );

        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("herdr session attach agents"));
        let calls = runner.take_calls();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[1].args, vec!["session", "list", "--json"]);
    }

    #[test]
    fn presentation_inside_the_target_session_only_focuses() {
        let runner = MockRunner::new();
        runner.push_response(success(
            br#"{"result":{"type":"workspace_info","workspace":{"workspace_id":"w7","label":"Support Herdr"}}}"#,
        ));
        runner.push_response(success(
            br#"{"sessions":[{"name":"agents","running":true,"socket_path":"/tmp/agents.sock"}]}"#,
        ));
        let commands = HerdrCommands::new(&runner);
        let mut warnings = Vec::new();

        present_existing(
            &commands,
            "agents",
            &HerdrWorkspace {
                id: "w7".into(),
                label: "Support Herdr".into(),
            },
            &HerdrCallerContext::Inside {
                socket_path: Some("/tmp/agents.sock".into()),
            },
            &mut warnings,
        );

        assert!(warnings.is_empty());
        let calls = runner.take_calls();
        assert_eq!(calls.len(), 2);
        assert_eq!(
            calls[0].args,
            vec!["--session", "agents", "workspace", "focus", "w7"]
        );
        assert_eq!(calls[1].args, vec!["session", "list", "--json"]);
    }

    #[test]
    fn environment_spec_uses_the_reconciled_stored_session() {
        let workspace = WorkspaceConfig {
            title: "Support Herdr".into(),
            name: "calm-river".into(),
            description: "Exercise Herdr reconciliation".into(),
            branch: "zootree/calm-river".into(),
            workspace_dir: "/tmp/calm-river".into(),
            created_at: "2026-08-12T10:00:00+08:00".into(),
            agent_cli: None,
            multiplexer: MultiplexerConfig::default(),
            multiplexer_state: StoredTerminalEnvironmentState::default(),
            repos: vec![RepoEntry {
                name: "api".into(),
                target_branch: Some("main".into()),
            }],
            events: Vec::new(),
        };

        let spec = environment_spec(
            &GlobalConfig::default(),
            &workspace,
            "stored-session",
            "Support Herdr · zootree:calm-river",
            &AgentIntent::None,
        )
        .unwrap();

        assert_eq!(spec.session, "stored-session");
    }

    #[test]
    fn agent_name_is_normalized_and_bounded() {
        assert_eq!(agent_name("Calm_River"), "zt-calm_river");

        let first = agent_name("A-Very-Long-Workspace-Name-For-Herdr-One");
        let second = agent_name("A-Very-Long-Workspace-Name-For-Herdr-Two");
        assert_eq!(first.len(), 32);
        assert_eq!(second.len(), 32);
        assert_ne!(first, second);
        assert!(first.chars().all(|character| character.is_ascii_lowercase()
            || character.is_ascii_digit()
            || matches!(character, '-' | '_')));
    }

    #[test]
    fn agent_detection_timeout_is_a_warning() {
        let runner = MockRunner::new();
        runner.push_response(Output {
            status: ExitStatus::from_raw(1 << 8),
            stdout: Vec::new(),
            stderr: br#"{"error":{"code":"agent_not_found","message":"not detected"}}"#.to_vec(),
        });
        let commands = HerdrCommands::new(&runner);
        let mut warnings = Vec::new();

        name_agent_with_timeout(
            &commands,
            "agents",
            "w7:p3",
            "zt-calm-river",
            std::time::Duration::ZERO,
            &mut warnings,
        );

        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("within 5 seconds"));
        assert_eq!(runner.take_calls().len(), 1);
    }

    #[test]
    fn agent_rename_failure_is_a_warning_after_detection() {
        let runner = MockRunner::new();
        runner.push_response(success(
            br#"{"result":{"type":"agent_info","agent":{"pane_id":"w7:p3"}}}"#,
        ));
        runner.push_response(Output {
            status: ExitStatus::from_raw(1 << 8),
            stdout: Vec::new(),
            stderr: br#"{"error":{"code":"invalid_name","message":"rename failed"}}"#.to_vec(),
        });
        let commands = HerdrCommands::new(&runner);
        let mut warnings = Vec::new();

        name_agent_with_timeout(
            &commands,
            "agents",
            "w7:p3",
            "zt-calm-river",
            std::time::Duration::ZERO,
            &mut warnings,
        );

        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("rename failed"));
        assert_eq!(runner.take_calls().len(), 2);
    }
}

fn activation(
    session: &str,
    workspace: &HerdrWorkspace,
    warnings: Vec<String>,
) -> Result<Activation> {
    Ok(Activation {
        stored_state: encode_current_state(
            MultiplexerKind::Herdr,
            &HerdrStatePayload {
                session: session.into(),
                workspace_id: workspace.id.clone(),
                label: workspace.label.clone(),
            },
        )?,
        warnings,
    })
}
