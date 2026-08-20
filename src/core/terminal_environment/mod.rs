mod cmux;
mod herdr;
mod zellij;

use crate::config::global::{GlobalConfig, MultiplexerKind};
use crate::config::workspace::{StoredTerminalEnvironmentState, WorkspaceConfig};
use crate::config::ConfigManager;
use crate::runner::CommandRunner;
use anyhow::Result;
use serde::{Deserialize, Serialize};

const CURRENT_STATE_VERSION: u64 = 1;

/// Describes whether activation should place an agent in a newly created
/// terminal environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentIntent {
    None,
    Default,
    Override(String),
}

/// The observable result of successfully activating a terminal environment.
#[derive(Debug, Clone, PartialEq)]
pub struct Activation {
    pub stored_state: StoredTerminalEnvironmentState,
    pub warnings: Vec<String>,
}

/// Best-effort cleanup information returned when closing a terminal
/// environment.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CloseReport {
    pub warnings: Vec<String>,
}

/// Stable synchronous lifecycle facade for a workspace's terminal environment.
///
/// Workspace callers use only this adapter-neutral facade; adapter-specific
/// reconciliation, layout preparation, and command translation remain behind
/// it.
pub struct TerminalEnvironment<'a, R: CommandRunner> {
    config_manager: &'a ConfigManager,
    global_config: &'a GlobalConfig,
    runner: &'a R,
    in_zellij: bool,
    herdr_caller: herdr::HerdrCallerContext,
}

impl<'a, R: CommandRunner> TerminalEnvironment<'a, R> {
    pub fn new(
        config_manager: &'a ConfigManager,
        global_config: &'a GlobalConfig,
        runner: &'a R,
    ) -> Self {
        Self {
            config_manager,
            global_config,
            runner,
            in_zellij: crate::core::multiplexer::zellij::is_inside_zellij(),
            herdr_caller: herdr::HerdrCallerContext::from_env(),
        }
    }

    #[cfg(test)]
    fn with_zellij_context(
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
            herdr_caller: herdr::HerdrCallerContext::from_env(),
        }
    }

    #[cfg(test)]
    fn with_herdr_context(
        config_manager: &'a ConfigManager,
        global_config: &'a GlobalConfig,
        runner: &'a R,
        herdr_caller: herdr::HerdrCallerContext,
    ) -> Self {
        Self {
            config_manager,
            global_config,
            runner,
            in_zellij: false,
            herdr_caller,
        }
    }

    pub fn activate(
        &self,
        workspace: &WorkspaceConfig,
        agent_intent: AgentIntent,
    ) -> Result<Activation> {
        let decoded = decode_stored_state(&workspace.multiplexer_state);
        match selected_adapter(workspace, &decoded) {
            MultiplexerKind::Herdr => {
                let (stored_payload, warnings) = herdr_state(decoded);
                herdr::activate(
                    self.global_config,
                    self.runner,
                    workspace,
                    stored_payload,
                    agent_intent,
                    warnings,
                    &self.herdr_caller,
                )
            }
            MultiplexerKind::Cmux => {
                let (stored_payload, warnings) = cmux_state(decoded);
                cmux::activate(
                    self.config_manager,
                    self.global_config,
                    self.runner,
                    workspace,
                    stored_payload,
                    agent_intent,
                    warnings,
                )
            }
            MultiplexerKind::Zellij => {
                let (stored_payload, warnings) = zellij_state(decoded);
                zellij::ZellijAdapter::new(
                    self.config_manager,
                    self.global_config,
                    self.runner,
                    self.in_zellij,
                )
                .activate(workspace, stored_payload, agent_intent, warnings)
            }
        }
    }

    pub fn close(&self, workspace: &WorkspaceConfig) -> CloseReport {
        let decoded = decode_stored_state(&workspace.multiplexer_state);
        match selected_adapter(workspace, &decoded) {
            MultiplexerKind::Herdr => {
                let (stored_payload, warnings) = herdr_state(decoded);
                herdr::close(self.runner, workspace, stored_payload, warnings)
            }
            MultiplexerKind::Cmux => {
                let (stored_payload, warnings) = cmux_state(decoded);
                cmux::close(self.runner, workspace, stored_payload, warnings)
            }
            MultiplexerKind::Zellij => {
                let (stored_payload, warnings) = zellij_state(decoded);
                zellij::ZellijAdapter::new(
                    self.config_manager,
                    self.global_config,
                    self.runner,
                    self.in_zellij,
                )
                .close(workspace, stored_payload, warnings)
            }
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CurrentStateEnvelope {
    version: u64,
    adapter: MultiplexerKind,
    #[serde(default)]
    payload: toml::Table,
}

enum DecodedStoredState {
    Empty,
    Legacy(LegacyMultiplexerState),
    Current {
        adapter: MultiplexerKind,
        payload: toml::Table,
    },
    UnknownVersion(u64),
    Corrupt(String),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyMultiplexerState {
    #[serde(default)]
    kind: Option<MultiplexerKind>,
    #[serde(default)]
    cmux_workspace: Option<String>,
    #[serde(default)]
    cmux_group: Option<String>,
    #[serde(default)]
    #[serde(rename = "cmux_anchor_workspace")]
    _cmux_anchor_workspace: Option<String>,
    #[serde(default)]
    cmux_repo_workspaces: Vec<CmuxRepoWorkspaceRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct CmuxRepoWorkspaceRef {
    repo: String,
    workspace: String,
}

fn selected_adapter(workspace: &WorkspaceConfig, decoded: &DecodedStoredState) -> MultiplexerKind {
    match decoded {
        DecodedStoredState::Legacy(state) => state
            .kind
            .clone()
            .unwrap_or_else(|| workspace.multiplexer.kind.clone()),
        DecodedStoredState::Current { adapter, .. } => adapter.clone(),
        DecodedStoredState::Empty
        | DecodedStoredState::UnknownVersion(_)
        | DecodedStoredState::Corrupt(_) => workspace.multiplexer.kind.clone(),
    }
}

fn cmux_state(decoded: DecodedStoredState) -> (Option<cmux::CmuxStatePayload>, Vec<String>) {
    match decoded {
        DecodedStoredState::Empty => (None, Vec::new()),
        DecodedStoredState::Legacy(state) => {
            let mut warnings = Vec::new();
            if state.cmux_group.is_none() && state.cmux_workspace.is_some() {
                warnings.push(
                    "legacy cmux workspace state did not identify a group; reconciling by name"
                        .into(),
                );
            }
            let payload = state.cmux_group.map(|group| cmux::CmuxStatePayload {
                group,
                repo_workspaces: state.cmux_repo_workspaces,
            });
            (payload, warnings)
        }
        DecodedStoredState::Current { adapter, payload } => {
            debug_assert_eq!(adapter, MultiplexerKind::Cmux);
            match toml::Value::Table(payload).try_into::<cmux::CmuxStatePayload>() {
                Ok(payload) if !payload.group.is_empty() => (Some(payload), Vec::new()),
                Ok(_) => (
                    None,
                    vec!["stored cmux state had an empty group ref; reconciling by name".into()],
                ),
                Err(error) => (
                    None,
                    vec![format!(
                        "stored cmux state was corrupt ({error}); reconciling by name"
                    )],
                ),
            }
        }
        DecodedStoredState::UnknownVersion(version) => (
            None,
            vec![format!(
                "terminal environment state version {version} is unknown; reconciling cmux by name"
            )],
        ),
        DecodedStoredState::Corrupt(reason) => (
            None,
            vec![format!(
                "terminal environment state was corrupt ({reason}); reconciling cmux by name"
            )],
        ),
    }
}

fn zellij_state(decoded: DecodedStoredState) -> (Option<zellij::ZellijStatePayload>, Vec<String>) {
    match decoded {
        DecodedStoredState::Empty | DecodedStoredState::Legacy(_) => (None, Vec::new()),
        DecodedStoredState::Current { adapter, payload } => {
            debug_assert_eq!(adapter, MultiplexerKind::Zellij);
            match toml::Value::Table(payload).try_into::<zellij::ZellijStatePayload>() {
                Ok(payload) if !payload.session.is_empty() => (Some(payload), Vec::new()),
                Ok(_) => (
                    None,
                    vec![
                        "stored Zellij state had an empty session name; reconciling by display name"
                            .into(),
                    ],
                ),
                Err(error) => (
                    None,
                    vec![format!(
                        "stored Zellij state was corrupt ({error}); reconciling by display name"
                    )],
                ),
            }
        }
        DecodedStoredState::UnknownVersion(version) => (
            None,
            vec![format!(
                "terminal environment state version {version} is unknown; reconciling Zellij by display name"
            )],
        ),
        DecodedStoredState::Corrupt(reason) => (
            None,
            vec![format!(
                "terminal environment state was corrupt ({reason}); reconciling Zellij by display name"
            )],
        ),
    }
}

fn herdr_state(decoded: DecodedStoredState) -> (Option<herdr::HerdrStatePayload>, Vec<String>) {
    match decoded {
        DecodedStoredState::Empty | DecodedStoredState::Legacy(_) => (None, Vec::new()),
        DecodedStoredState::Current { adapter, payload } => {
            debug_assert_eq!(adapter, MultiplexerKind::Herdr);
            match toml::Value::Table(payload).try_into::<herdr::HerdrStatePayload>() {
                Ok(payload)
                    if !payload.session.is_empty()
                        && !payload.workspace_id.is_empty()
                        && !payload.label.is_empty() =>
                {
                    (Some(payload), Vec::new())
                }
                Ok(_) => (
                    None,
                    vec![
                        "stored Herdr state had an empty field; reconciling by derived label"
                            .into(),
                    ],
                ),
                Err(error) => (
                    None,
                    vec![format!(
                        "stored Herdr state was corrupt ({error}); reconciling by derived label"
                    )],
                ),
            }
        }
        DecodedStoredState::UnknownVersion(version) => (
            None,
            vec![format!(
                "terminal environment state version {version} is unknown; reconciling Herdr by derived label"
            )],
        ),
        DecodedStoredState::Corrupt(reason) => (
            None,
            vec![format!(
                "terminal environment state was corrupt ({reason}); reconciling Herdr by derived label"
            )],
        ),
    }
}

fn encode_current_state<T: Serialize>(
    adapter: MultiplexerKind,
    payload: &T,
) -> Result<StoredTerminalEnvironmentState> {
    let payload = toml::Value::try_from(payload)?
        .try_into::<toml::Table>()
        .map_err(|_| anyhow::anyhow!("terminal environment payload must serialize as a table"))?;
    let envelope = CurrentStateEnvelope {
        version: CURRENT_STATE_VERSION,
        adapter,
        payload,
    };
    let table = toml::Value::try_from(envelope)?
        .try_into::<toml::Table>()
        .map_err(|_| anyhow::anyhow!("terminal environment state must serialize as a table"))?;
    Ok(StoredTerminalEnvironmentState::from_table(table))
}

fn decode_stored_state(state: &StoredTerminalEnvironmentState) -> DecodedStoredState {
    if state.is_empty() {
        return DecodedStoredState::Empty;
    }

    let table = state.as_table();
    if let Some(version) = table.get("version") {
        let Some(version) = version
            .as_integer()
            .and_then(|value| u64::try_from(value).ok())
        else {
            return DecodedStoredState::Corrupt(
                "state envelope version must be a non-negative integer".into(),
            );
        };
        if version != CURRENT_STATE_VERSION {
            return DecodedStoredState::UnknownVersion(version);
        }

        return match toml::Value::Table(table.clone()).try_into::<CurrentStateEnvelope>() {
            Ok(envelope) => DecodedStoredState::Current {
                adapter: envelope.adapter,
                payload: envelope.payload,
            },
            Err(error) => DecodedStoredState::Corrupt(error.to_string()),
        };
    }

    match toml::Value::Table(table.clone()).try_into::<LegacyMultiplexerState>() {
        Ok(legacy) => DecodedStoredState::Legacy(legacy),
        Err(error) => DecodedStoredState::Corrupt(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::global::MultiplexerConfig;
    use crate::config::repo::RepoConfig;
    use crate::config::workspace::RepoEntry;
    use crate::runner::MockRunner;
    use std::collections::BTreeMap;
    use std::os::unix::process::ExitStatusExt;
    use std::process::{ExitStatus, Output};
    use tempfile::TempDir;

    fn stored_state(toml_source: &str) -> StoredTerminalEnvironmentState {
        toml::from_str(toml_source).unwrap()
    }

    fn success_output(stdout: &[u8]) -> Output {
        Output {
            status: ExitStatus::from_raw(0),
            stdout: stdout.to_vec(),
            stderr: Vec::new(),
        }
    }

    fn failure_output(stderr: &[u8]) -> Output {
        Output {
            status: ExitStatus::from_raw(1 << 8),
            stdout: Vec::new(),
            stderr: stderr.to_vec(),
        }
    }

    fn zellij_workspace(repo_names: &[&str]) -> WorkspaceConfig {
        WorkspaceConfig {
            title: "Zellij terminal environment".into(),
            name: "calm-river".into(),
            description: "Exercise Zellij reconciliation".into(),
            branch: "zootree/calm-river".into(),
            workspace_dir: "/tmp/calm-river".into(),
            created_at: "2026-07-21T10:00:00+08:00".into(),
            agent_cli: None,
            multiplexer: MultiplexerConfig::default(),
            multiplexer_state: StoredTerminalEnvironmentState::default(),
            repos: repo_names
                .iter()
                .map(|name| RepoEntry {
                    name: (*name).into(),
                    target_branch: Some("main".into()),
                })
                .collect(),
            events: Vec::new(),
        }
    }

    fn setup_zellij_config(repo_names: &[&str]) -> (TempDir, ConfigManager) {
        let temp = TempDir::new().unwrap();
        let config_manager = ConfigManager::with_base_dir(temp.path().to_path_buf());
        config_manager.ensure_dirs().unwrap();
        for repo_name in repo_names {
            config_manager
                .save_repo_config(
                    repo_name,
                    &RepoConfig {
                        path: format!("/repo/{repo_name}"),
                        default_target_branch: Some("main".into()),
                        copy_files: Vec::new(),
                        hooks: Default::default(),
                        lazygit: None,
                    },
                )
                .unwrap();
        }
        (temp, config_manager)
    }

    fn state_session(state: &StoredTerminalEnvironmentState) -> Option<&str> {
        state
            .as_table()
            .get("payload")
            .and_then(toml::Value::as_table)
            .and_then(|payload| payload.get("session"))
            .and_then(toml::Value::as_str)
    }

    fn stored_herdr_workspace() -> WorkspaceConfig {
        let mut workspace = zellij_workspace(&["api"]);
        workspace.title = "Support Herdr".into();
        workspace.multiplexer.kind = MultiplexerKind::Herdr;
        workspace.multiplexer.herdr.session = "agents".into();
        workspace.multiplexer_state = stored_state(
            r#"
version = 1
adapter = "herdr"

[payload]
session = "agents"
workspace_id = "w7"
label = "Support Herdr · zootree:calm-river"
"#,
        );
        workspace
    }

    #[test]
    fn current_envelope_is_private_but_recognized() {
        let state = stored_state(
            r#"
version = 1
adapter = "cmux"

[payload]
group = "workspace_group:2"
"#,
        );

        match decode_stored_state(&state) {
            DecodedStoredState::Current { adapter, payload } => {
                assert_eq!(adapter, MultiplexerKind::Cmux);
                assert_eq!(
                    payload.get("group").and_then(toml::Value::as_str),
                    Some("workspace_group:2")
                );
            }
            _ => panic!("expected current state envelope"),
        }
    }

    #[test]
    fn private_legacy_decoder_accepts_existing_shape() {
        let state = stored_state(
            r#"
kind = "cmux"
cmux_workspace = "workspace:3"
cmux_anchor_workspace = "workspace:4"
"#,
        );

        match decode_stored_state(&state) {
            DecodedStoredState::Legacy(legacy) => {
                assert_eq!(legacy.kind, Some(MultiplexerKind::Cmux));
            }
            _ => panic!("expected private legacy state"),
        }
    }

    #[test]
    fn herdr_facade_inside_target_session_returns_state_without_attaching() {
        let (_temp, config_manager) = setup_zellij_config(&["api"]);
        let global_config = GlobalConfig::default();
        let runner = MockRunner::new();
        runner.push_response(success_output(b"herdr 0.8.0\n"));
        runner.push_response(success_output(
            br#"{"result":{"type":"workspace_info","workspace":{"workspace_id":"w7","label":"Support Herdr"}}}"#,
        ));
        runner.push_response(success_output(
            br#"{"result":{"type":"workspace_info","workspace":{"workspace_id":"w7","label":"Support Herdr"}}}"#,
        ));
        runner.push_response(success_output(
            br#"{"sessions":[{"name":"agents","running":true,"socket_path":"/tmp/agents.sock"}]}"#,
        ));

        let activation = TerminalEnvironment::with_herdr_context(
            &config_manager,
            &global_config,
            &runner,
            herdr::HerdrCallerContext::Inside {
                socket_path: Some("/tmp/agents.sock".into()),
            },
        )
        .activate(&stored_herdr_workspace(), AgentIntent::None)
        .unwrap();

        assert!(activation.warnings.is_empty());
        assert_eq!(state_session(&activation.stored_state), Some("agents"));
        let calls = runner.take_calls();
        assert_eq!(calls.len(), 4);
        assert_eq!(calls[3].args, vec!["session", "list", "--json"]);
    }

    #[test]
    fn herdr_facade_inside_other_session_returns_state_and_attach_warning() {
        let (_temp, config_manager) = setup_zellij_config(&["api"]);
        let global_config = GlobalConfig::default();
        let runner = MockRunner::new();
        runner.push_response(success_output(b"herdr 0.8.0\n"));
        runner.push_response(success_output(
            br#"{"result":{"type":"workspace_info","workspace":{"workspace_id":"w7","label":"Support Herdr"}}}"#,
        ));
        runner.push_response(success_output(
            br#"{"result":{"type":"workspace_info","workspace":{"workspace_id":"w7","label":"Support Herdr"}}}"#,
        ));
        runner.push_response(success_output(
            br#"{"sessions":[{"name":"agents","running":true,"socket_path":"/tmp/agents.sock"}]}"#,
        ));

        let activation = TerminalEnvironment::with_herdr_context(
            &config_manager,
            &global_config,
            &runner,
            herdr::HerdrCallerContext::Inside {
                socket_path: Some("/tmp/other.sock".into()),
            },
        )
        .activate(&stored_herdr_workspace(), AgentIntent::None)
        .unwrap();

        assert_eq!(state_session(&activation.stored_state), Some("agents"));
        assert_eq!(activation.warnings.len(), 1);
        assert!(activation.warnings[0].contains("herdr session attach agents"));
        assert_eq!(runner.take_calls().len(), 4);
    }

    #[test]
    fn zellij_activate_creates_foreground_with_default_layout_and_canonical_state() {
        let (_temp, config_manager) = setup_zellij_config(&["api"]);
        let global_config = GlobalConfig {
            agent_cli: Some("codex --prompt $prompt".into()),
            ..GlobalConfig::default()
        };
        let runner = MockRunner::new();
        runner.push_response(success_output(b"other-session\n"));
        runner.push_response(success_output(b""));
        let workspace = zellij_workspace(&["api"]);

        let activation = TerminalEnvironment::with_zellij_context(
            &config_manager,
            &global_config,
            &runner,
            false,
        )
        .activate(&workspace, AgentIntent::Default)
        .unwrap();

        assert_eq!(
            state_session(&activation.stored_state),
            Some("zootree-calm-river")
        );
        assert!(activation.warnings.is_empty());
        assert!(config_manager.base_dir.join("layouts/default.kdl").exists());
        let rendered =
            std::fs::read_to_string(config_manager.base_dir.join("layouts/recently.kdl")).unwrap();
        assert!(rendered.contains(r#"command="codex""#));
        let calls = runner.take_calls();
        assert_eq!(calls[0].args, vec!["list-sessions"]);
        assert_eq!(
            calls[1].args,
            vec![
                "--new-session-with-layout",
                config_manager
                    .base_dir
                    .join("layouts/recently.kdl")
                    .to_string_lossy()
                    .as_ref(),
                "--session",
                "zootree-calm-river"
            ]
        );
    }

    #[test]
    fn zellij_activate_reuses_stored_session_without_preparing_layout() {
        let (_temp, config_manager) = setup_zellij_config(&[]);
        let global_config = GlobalConfig::default();
        let runner = MockRunner::new();
        runner.push_response(success_output(b"renamed-session [Created 1m ago]\n"));
        runner.push_response(success_output(b""));
        let mut workspace = zellij_workspace(&["missing-repo-config"]);
        workspace.multiplexer_state = stored_state(
            r#"
version = 1
adapter = "zellij"

[payload]
session = "renamed-session"
"#,
        );

        let activation = TerminalEnvironment::with_zellij_context(
            &config_manager,
            &global_config,
            &runner,
            false,
        )
        .activate(&workspace, AgentIntent::Default)
        .unwrap();

        assert_eq!(
            state_session(&activation.stored_state),
            Some("renamed-session")
        );
        assert!(activation.warnings[0].contains("agent request was ignored"));
        assert!(!config_manager
            .base_dir
            .join("layouts/recently.kdl")
            .exists());
        let calls = runner.take_calls();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[1].args, vec!["attach", "renamed-session"]);
    }

    #[test]
    fn zellij_activate_recovers_stale_state_by_display_name() {
        let (_temp, config_manager) = setup_zellij_config(&[]);
        let global_config = GlobalConfig::default();
        let runner = MockRunner::new();
        runner.push_response(success_output(b"zootree-calm-river\n"));
        runner.push_response(success_output(b"zootree-calm-river\n"));
        runner.push_response(success_output(b""));
        let mut workspace = zellij_workspace(&[]);
        workspace.multiplexer_state = stored_state(
            r#"
version = 1
adapter = "zellij"

[payload]
session = "stale-session"
"#,
        );

        let activation = TerminalEnvironment::with_zellij_context(
            &config_manager,
            &global_config,
            &runner,
            false,
        )
        .activate(&workspace, AgentIntent::None)
        .unwrap();

        assert_eq!(
            state_session(&activation.stored_state),
            Some("zootree-calm-river")
        );
        assert!(activation.warnings[0].contains("stale-session"));
        let calls = runner.take_calls();
        assert_eq!(calls[0].args, vec!["list-sessions"]);
        assert_eq!(calls[1].args, vec!["list-sessions"]);
        assert_eq!(calls[2].args, vec!["attach", "zootree-calm-river"]);
    }

    #[test]
    fn zellij_activate_creates_in_background_inside_zellij() {
        let (_temp, config_manager) = setup_zellij_config(&["api"]);
        let global_config = GlobalConfig::default();
        let runner = MockRunner::new();
        runner.push_response(success_output(b""));
        runner.push_response(success_output(b""));
        let workspace = zellij_workspace(&["api"]);

        TerminalEnvironment::with_zellij_context(&config_manager, &global_config, &runner, true)
            .activate(&workspace, AgentIntent::None)
            .unwrap();

        let calls = runner.take_calls();
        assert_eq!(
            calls[1].args,
            vec![
                "-l",
                config_manager
                    .base_dir
                    .join("layouts/recently.kdl")
                    .to_string_lossy()
                    .as_ref(),
                "attach",
                "--create-background",
                "zootree-calm-river"
            ]
        );
        assert!(calls[1].env_remove.iter().any(|key| key == "ZELLIJ"));
        assert!(calls[1]
            .env_remove
            .iter()
            .any(|key| key == "ZELLIJ_SESSION_NAME"));
    }

    #[test]
    fn zellij_activate_renders_custom_layout_and_agent_alias_only_when_creating() {
        let (_temp, config_manager) = setup_zellij_config(&["api"]);
        std::fs::write(
            config_manager.base_dir.join("layouts/focused.kdl"),
            r#"layout { pane cwd="$worktree_path" $repo_agent_cli }"#,
        )
        .unwrap();
        let global_config = GlobalConfig {
            agent_cli_alias: BTreeMap::from([(
                "fast".into(),
                "codex --model gpt-5 --prompt $prompt".into(),
            )]),
            ..GlobalConfig::default()
        };
        let runner = MockRunner::new();
        runner.push_response(success_output(b""));
        runner.push_response(success_output(b""));
        let mut workspace = zellij_workspace(&["api"]);
        workspace.multiplexer.zellij.layout = Some("focused".into());

        TerminalEnvironment::with_zellij_context(&config_manager, &global_config, &runner, false)
            .activate(&workspace, AgentIntent::Override("fast".into()))
            .unwrap();

        let rendered =
            std::fs::read_to_string(config_manager.base_dir.join("layouts/recently.kdl")).unwrap();
        assert!(rendered.contains("/tmp/calm-river/api"));
        assert!(rendered.contains(r#"command="codex""#));
        assert!(rendered.contains(r#""--model" "gpt-5""#));
    }

    #[test]
    fn zellij_activate_is_idempotent_after_creation() {
        let (_temp, config_manager) = setup_zellij_config(&["api"]);
        let global_config = GlobalConfig::default();
        let first_runner = MockRunner::new();
        first_runner.push_response(success_output(b""));
        first_runner.push_response(success_output(b""));
        let mut workspace = zellij_workspace(&["api"]);
        let first = TerminalEnvironment::with_zellij_context(
            &config_manager,
            &global_config,
            &first_runner,
            true,
        )
        .activate(&workspace, AgentIntent::None)
        .unwrap();
        workspace.multiplexer_state = first.stored_state;

        let second_runner = MockRunner::new();
        second_runner.push_response(success_output(b"zootree-calm-river\n"));
        let second = TerminalEnvironment::with_zellij_context(
            &config_manager,
            &global_config,
            &second_runner,
            true,
        )
        .activate(&workspace, AgentIntent::None)
        .unwrap();

        assert_eq!(
            state_session(&second.stored_state),
            Some("zootree-calm-river")
        );
        assert_eq!(second_runner.take_calls().len(), 1);
    }

    #[test]
    fn zellij_activate_rejects_ambiguous_display_name() {
        let (_temp, config_manager) = setup_zellij_config(&[]);
        let global_config = GlobalConfig::default();
        let runner = MockRunner::new();
        runner.push_response(success_output(
            b"zootree-calm-river\nzootree-calm-river [Created 1m ago]\n",
        ));

        let error = TerminalEnvironment::with_zellij_context(
            &config_manager,
            &global_config,
            &runner,
            false,
        )
        .activate(&zellij_workspace(&[]), AgentIntent::None)
        .unwrap_err();

        assert!(error.to_string().contains("ambiguous"));
        assert_eq!(runner.take_calls().len(), 1);
    }

    #[test]
    fn zellij_close_deletes_existing_stored_session() {
        let (_temp, config_manager) = setup_zellij_config(&[]);
        let global_config = GlobalConfig::default();
        let runner = MockRunner::new();
        runner.push_response(success_output(b"renamed-session\n"));
        runner.push_response(success_output(b""));
        let mut workspace = zellij_workspace(&[]);
        workspace.multiplexer_state = stored_state(
            r#"
version = 1
adapter = "zellij"

[payload]
session = "renamed-session"
"#,
        );

        let report = TerminalEnvironment::with_zellij_context(
            &config_manager,
            &global_config,
            &runner,
            false,
        )
        .close(&workspace);

        assert!(report.warnings.is_empty());
        let calls = runner.take_calls();
        assert_eq!(calls[0].args, vec!["list-sessions"]);
        assert_eq!(
            calls[1].args,
            vec!["delete-session", "--force", "renamed-session"]
        );
    }

    #[test]
    fn zellij_close_treats_missing_target_as_success() {
        let (_temp, config_manager) = setup_zellij_config(&[]);
        let global_config = GlobalConfig::default();
        let runner = MockRunner::new();
        runner.push_response(success_output(b"other-session\n"));

        let report = TerminalEnvironment::with_zellij_context(
            &config_manager,
            &global_config,
            &runner,
            false,
        )
        .close(&zellij_workspace(&[]));

        assert!(report.warnings.is_empty());
        assert_eq!(runner.take_calls().len(), 1);
    }

    #[test]
    fn zellij_close_reports_list_and_delete_failures_as_warnings() {
        let (_temp, config_manager) = setup_zellij_config(&[]);
        let global_config = GlobalConfig::default();

        let list_runner = MockRunner::new();
        list_runner.push_response(failure_output(b"socket unavailable"));
        let report = TerminalEnvironment::with_zellij_context(
            &config_manager,
            &global_config,
            &list_runner,
            false,
        )
        .close(&zellij_workspace(&[]));
        assert_eq!(report.warnings.len(), 1);
        assert!(report.warnings[0].contains("socket unavailable"));

        let delete_runner = MockRunner::new();
        delete_runner.push_response(success_output(b"zootree-calm-river\n"));
        delete_runner.push_response(failure_output(b"permission denied"));
        let report = TerminalEnvironment::with_zellij_context(
            &config_manager,
            &global_config,
            &delete_runner,
            false,
        )
        .close(&zellij_workspace(&[]));
        assert_eq!(report.warnings.len(), 1);
        assert!(report.warnings[0].contains("permission denied"));
    }
}
