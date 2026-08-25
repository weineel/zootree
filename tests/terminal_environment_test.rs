use std::collections::BTreeMap;
use std::os::unix::process::ExitStatusExt;
use std::process::{ExitStatus, Output};

use tempfile::TempDir;
use zootree::config::global::{GlobalConfig, MultiplexerConfig, MultiplexerKind};
use zootree::config::repo::RepoConfig;
use zootree::config::workspace::{RepoEntry, StoredTerminalEnvironmentState, WorkspaceConfig};
use zootree::config::ConfigManager;
use zootree::core::terminal_environment::{AgentIntent, TerminalEnvironment};
use zootree::runner::MockRunner;

fn success_output(stdout: &[u8]) -> Output {
    Output {
        status: ExitStatus::from_raw(0),
        stdout: stdout.to_vec(),
        stderr: Vec::new(),
    }
}

fn failure_output(stderr: &[u8]) -> Output {
    Output {
        status: ExitStatus::from_raw(1),
        stdout: Vec::new(),
        stderr: stderr.to_vec(),
    }
}

fn workspace(repo_names: &[&str]) -> WorkspaceConfig {
    let multiplexer = MultiplexerConfig {
        kind: MultiplexerKind::Cmux,
        ..MultiplexerConfig::default()
    };
    WorkspaceConfig {
        title: "Terminal environment API".into(),
        name: "calm-river".into(),
        description: "Exercise cmux reconciliation".into(),
        branch: "zootree/calm-river".into(),
        workspace_dir: "/tmp/calm-river".into(),
        created_at: "2026-07-21T10:00:00+08:00".into(),
        agent_cli: None,
        multiplexer,
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

fn stored_state(source: &str) -> StoredTerminalEnvironmentState {
    toml::from_str(source).unwrap()
}

fn state_table(state: &StoredTerminalEnvironmentState) -> toml::Table {
    toml::Value::try_from(state)
        .unwrap()
        .as_table()
        .unwrap()
        .clone()
}

fn state_group(state: &StoredTerminalEnvironmentState) -> Option<String> {
    state_table(state)
        .get("payload")
        .and_then(toml::Value::as_table)
        .and_then(|payload| payload.get("group"))
        .and_then(toml::Value::as_str)
        .map(str::to_string)
}

fn state_payload_string(state: &StoredTerminalEnvironmentState, key: &str) -> Option<String> {
    state_table(state)
        .get("payload")
        .and_then(toml::Value::as_table)
        .and_then(|payload| payload.get(key))
        .and_then(toml::Value::as_str)
        .map(str::to_string)
}

fn herdr_workspace(repo_names: &[&str]) -> WorkspaceConfig {
    let mut workspace = workspace(repo_names);
    workspace.title = "Support Herdr".into();
    workspace.description = "Exercise Herdr reconciliation".into();
    workspace.multiplexer.kind = MultiplexerKind::Herdr;
    workspace.multiplexer.herdr.session = "agents".into();
    workspace
}

fn setup_config(repo_names: &[&str]) -> (TempDir, ConfigManager) {
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

fn push_single_repo_create_responses(runner: &MockRunner) {
    runner.push_response(success_output(br#"{"groups":[]}"#));
    runner.push_response(success_output(b"workspace:4\n"));
    runner.push_response(success_output(b"workspace_group:2\n"));
    runner.push_response(success_output(
        br#"{"groups":[{"name":"Terminal environment API","ref":"workspace_group:2","anchor_workspace_ref":"workspace:99"}]}"#,
    ));
    runner.push_response(success_output(b"workspace:7\n"));
    runner.push_response(success_output(b""));
    runner.push_response(success_output(b""));
}

fn command_arg<'a>(args: &'a [String], name: &str) -> &'a str {
    let index = args.iter().position(|arg| arg == name).unwrap();
    &args[index + 1]
}

#[test]
fn herdr_activate_reuses_stored_workspace_without_rebuilding_topology() {
    let (_temp, config_manager) = setup_config(&["api"]);
    let global_config = GlobalConfig::default();
    let runner = MockRunner::new();
    runner.push_response(success_output(b"herdr 0.8.0\n"));
    runner.push_response(success_output(
        br#"{"result":{"type":"workspace_info","workspace":{"workspace_id":"w7","label":"Renamed in Herdr"}}}"#,
    ));
    runner.push_response(success_output(
        br#"{"result":{"type":"workspace_info","workspace":{"workspace_id":"w7","label":"Renamed in Herdr"}}}"#,
    ));
    runner.push_response(success_output(b""));
    let mut workspace = herdr_workspace(&["api"]);
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

    let activation = TerminalEnvironment::new(&config_manager, &global_config, &runner)
        .activate(&workspace, AgentIntent::Default)
        .unwrap();

    assert_eq!(
        state_payload_string(&activation.stored_state, "workspace_id").as_deref(),
        Some("w7")
    );
    assert_eq!(
        state_payload_string(&activation.stored_state, "label").as_deref(),
        Some("Renamed in Herdr")
    );
    assert!(activation.warnings[0].contains("agent request was ignored"));
    let calls = runner.take_calls();
    assert_eq!(calls.len(), 4);
    assert_eq!(calls[0].args, vec!["--version"]);
    assert_eq!(
        calls[1].args,
        vec!["--session", "agents", "workspace", "get", "w7"]
    );
    assert_eq!(
        calls[2].args,
        vec!["--session", "agents", "workspace", "focus", "w7"]
    );
    assert_eq!(calls[3].args, vec!["session", "attach", "agents"]);
}

#[test]
fn herdr_activate_recovers_a_stale_id_by_unique_stored_label() {
    let (_temp, config_manager) = setup_config(&["api"]);
    let global_config = GlobalConfig::default();
    let runner = MockRunner::new();
    runner.push_response(success_output(b"herdr 0.8.0\n"));
    runner.push_response(failure_output(
        br#"{"error":{"code":"workspace_not_found","message":"workspace missing"}}"#,
    ));
    runner.push_response(success_output(
        br#"{"result":{"type":"workspace_list","workspaces":[{"workspace_id":"w9","label":"Original label"}]}}"#,
    ));
    runner.push_response(success_output(
        br#"{"result":{"type":"workspace_info","workspace":{"workspace_id":"w9","label":"Original label"}}}"#,
    ));
    runner.push_response(success_output(b""));
    let mut workspace = herdr_workspace(&["api"]);
    workspace.multiplexer.herdr.session = "new-default".into();
    workspace.multiplexer_state = stored_state(
        r#"
version = 1
adapter = "herdr"

[payload]
session = "stored-session"
workspace_id = "w7"
label = "Original label"
"#,
    );

    let activation = TerminalEnvironment::new(&config_manager, &global_config, &runner)
        .activate(&workspace, AgentIntent::None)
        .unwrap();

    assert_eq!(
        state_payload_string(&activation.stored_state, "session").as_deref(),
        Some("stored-session")
    );
    assert_eq!(
        state_payload_string(&activation.stored_state, "workspace_id").as_deref(),
        Some("w9")
    );
    assert!(activation.warnings[0].contains("was stale"));
    let calls = runner.take_calls();
    assert!(calls[1..4]
        .iter()
        .all(|call| call.args.get(1).map(String::as_str) == Some("stored-session")));
}

#[test]
fn herdr_stale_stored_identity_does_not_adopt_the_current_derived_label() {
    let (_temp, config_manager) = setup_config(&["api"]);
    let global_config = GlobalConfig::default();
    let runner = MockRunner::new();
    runner.push_response(success_output(b"herdr 0.8.0\n"));
    runner.push_response(failure_output(
        br#"{"error":{"code":"workspace_not_found","message":"workspace missing"}}"#,
    ));
    runner.push_response(success_output(
        br#"{"result":{"type":"workspace_list","workspaces":[]}}"#,
    ));
    runner.push_response(failure_output(
        br#"{"error":{"code":"invalid_request","message":"stop before mutation"}}"#,
    ));
    let mut workspace = herdr_workspace(&["api"]);
    workspace.multiplexer_state = stored_state(
        r#"
version = 1
adapter = "herdr"

[payload]
session = "agents"
workspace_id = "w7"
label = "Original label"
"#,
    );

    let error = TerminalEnvironment::new(&config_manager, &global_config, &runner)
        .activate(&workspace, AgentIntent::None)
        .unwrap_err();

    assert!(error.to_string().contains("stop before mutation"));
    let calls = runner.take_calls();
    assert_eq!(calls.len(), 4);
    assert_eq!(
        calls[2].args,
        vec!["--session", "agents", "workspace", "list"]
    );
    assert_eq!(
        calls[3].args[..4],
        ["--session", "agents", "workspace", "create"]
    );
}

#[test]
fn herdr_activate_rejects_an_ambiguous_label_without_creating() {
    let (_temp, config_manager) = setup_config(&["api"]);
    let global_config = GlobalConfig::default();
    let runner = MockRunner::new();
    runner.push_response(success_output(b"herdr 0.8.0\n"));
    runner.push_response(success_output(
        r#"{"result":{"type":"workspace_list","workspaces":[{"workspace_id":"w7","label":"Support Herdr · zootree:calm-river"},{"workspace_id":"w8","label":"Support Herdr · zootree:calm-river"}]}}"#
            .as_bytes(),
    ));

    let error = TerminalEnvironment::new(&config_manager, &global_config, &runner)
        .activate(&herdr_workspace(&["api"]), AgentIntent::None)
        .unwrap_err();

    assert!(error.to_string().contains("ambiguous"));
    let calls = runner.take_calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(
        calls[1].args,
        vec!["--session", "agents", "workspace", "list"]
    );
}

#[test]
fn herdr_activate_rejects_an_older_version_before_session_access() {
    let (_temp, config_manager) = setup_config(&["api"]);
    let global_config = GlobalConfig::default();
    let runner = MockRunner::new();
    runner.push_response(success_output(b"herdr 0.7.9\n"));

    let error = TerminalEnvironment::new(&config_manager, &global_config, &runner)
        .activate(&herdr_workspace(&["api"]), AgentIntent::None)
        .unwrap_err();

    assert!(error.to_string().contains("requires Herdr 0.8.0+"));
    assert_eq!(runner.take_calls().len(), 1);
}

#[test]
fn herdr_activate_creates_single_repo_environment_and_returns_canonical_state() {
    let (_temp, config_manager) = setup_config(&["api"]);
    let global_config = GlobalConfig::default();
    let runner = MockRunner::new();
    runner.push_response(success_output(b"herdr 0.8.0\n"));
    runner.push_response(success_output(
        br#"{"result":{"type":"workspace_list","workspaces":[]}}"#,
    ));
    runner.push_response(success_output(
        r#"{"result":{"type":"workspace_created","workspace":{"workspace_id":"w7","label":"Support Herdr · zootree:calm-river"},"tab":{"tab_id":"w7:t1"},"root_pane":{"pane_id":"w7:p1"}}}"#
            .as_bytes(),
    ));
    runner.push_response(success_output(
        br#"{"result":{"type":"tab_info","tab":{"tab_id":"w7:t1"}}}"#,
    ));
    runner.push_response(success_output(
        br#"{"result":{"type":"pane_info","pane":{"pane_id":"w7:p2"}}}"#,
    ));
    runner.push_response(success_output(b""));
    runner.push_response(success_output(
        br#"{"result":{"type":"tab_created","tab":{"tab_id":"w7:t2"},"root_pane":{"pane_id":"w7:p3"}}}"#,
    ));
    runner.push_response(success_output(
        br#"{"result":{"type":"pane_info","pane":{"pane_id":"w7:p4"}}}"#,
    ));
    runner.push_response(success_output(
        br#"{"result":{"type":"pane_info","pane":{"pane_id":"w7:p5"}}}"#,
    ));
    runner.push_response(success_output(
        br#"{"result":{"type":"tab_info","tab":{"tab_id":"w7:t1"}}}"#,
    ));
    runner.push_response(success_output(
        r#"{"result":{"type":"workspace_info","workspace":{"workspace_id":"w7","label":"Support Herdr · zootree:calm-river"}}}"#
            .as_bytes(),
    ));
    runner.push_response(success_output(b""));
    let workspace = herdr_workspace(&["api"]);

    let activation = TerminalEnvironment::new(&config_manager, &global_config, &runner)
        .activate(&workspace, AgentIntent::None)
        .unwrap();

    let state = state_table(&activation.stored_state);
    assert_eq!(
        state.get("adapter").and_then(toml::Value::as_str),
        Some("herdr")
    );
    assert_eq!(
        state_payload_string(&activation.stored_state, "session").as_deref(),
        Some("agents")
    );
    assert_eq!(
        state_payload_string(&activation.stored_state, "workspace_id").as_deref(),
        Some("w7")
    );
    assert!(activation.warnings.is_empty());
    let calls = runner.take_calls();
    assert_eq!(calls[0].args, vec!["--version"]);
    assert_eq!(
        calls[1].args,
        vec!["--session", "agents", "workspace", "list"]
    );
    assert_eq!(
        calls[5].args,
        vec![
            "--session",
            "agents",
            "pane",
            "run",
            "w7:p1",
            "zootree info calm-river --watch",
        ]
    );
    assert_eq!(
        calls[9].args,
        vec!["--session", "agents", "tab", "focus", "w7:t1"]
    );
    assert_eq!(
        calls[10].args,
        vec!["--session", "agents", "workspace", "focus", "w7"]
    );
    assert_eq!(calls[11].args, vec!["session", "attach", "agents"]);
}

#[test]
fn herdr_close_uses_stored_session_and_workspace_id_without_version_gate() {
    let (_temp, config_manager) = setup_config(&[]);
    let global_config = GlobalConfig::default();
    let runner = MockRunner::new();
    runner.push_response(success_output(
        br#"{"result":{"type":"workspace_info","workspace":{"workspace_id":"w7","label":"Support Herdr"}}}"#,
    ));
    runner.push_response(success_output(br#"{"result":{"type":"ok"}}"#));
    let mut workspace = herdr_workspace(&[]);
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

    let report =
        TerminalEnvironment::new(&config_manager, &global_config, &runner).close(&workspace);

    assert!(report.closed);
    assert!(report.warnings.is_empty());
    let calls = runner.take_calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(
        calls[0].args,
        vec!["--session", "agents", "workspace", "get", "w7"]
    );
    assert_eq!(
        calls[1].args,
        vec!["--session", "agents", "workspace", "close", "w7"]
    );
}

#[test]
fn herdr_close_recovers_a_stale_id_by_unique_stored_label() {
    let (_temp, config_manager) = setup_config(&[]);
    let global_config = GlobalConfig::default();
    let runner = MockRunner::new();
    runner.push_response(failure_output(
        br#"{"error":{"code":"workspace_not_found","message":"workspace missing"}}"#,
    ));
    runner.push_response(success_output(
        br#"{"result":{"type":"workspace_list","workspaces":[{"workspace_id":"w9","label":"Original label"}]}}"#,
    ));
    runner.push_response(success_output(br#"{"result":{"type":"ok"}}"#));
    let mut workspace = herdr_workspace(&[]);
    workspace.multiplexer_state = stored_state(
        r#"
version = 1
adapter = "herdr"

[payload]
session = "stored-session"
workspace_id = "w7"
label = "Original label"
"#,
    );

    let report =
        TerminalEnvironment::new(&config_manager, &global_config, &runner).close(&workspace);

    assert!(report.closed);
    assert!(report.warnings.is_empty());
    let calls = runner.take_calls();
    assert_eq!(calls.len(), 3);
    assert_eq!(
        calls[2].args,
        vec!["--session", "stored-session", "workspace", "close", "w9"]
    );
}

#[test]
fn herdr_close_reports_ambiguous_and_malformed_responses_as_warnings() {
    let (_temp, config_manager) = setup_config(&[]);
    let global_config = GlobalConfig::default();

    let runner = MockRunner::new();
    runner.push_response(success_output(
        r#"{"result":{"type":"workspace_list","workspaces":[{"workspace_id":"w7","label":"Support Herdr · zootree:calm-river"},{"workspace_id":"w8","label":"Support Herdr · zootree:calm-river"}]}}"#
            .as_bytes(),
    ));
    let report = TerminalEnvironment::new(&config_manager, &global_config, &runner)
        .close(&herdr_workspace(&[]));
    assert!(!report.closed);
    assert_eq!(report.warnings.len(), 1);
    assert!(report.warnings[0].contains("ambiguous"));
    assert_eq!(runner.take_calls().len(), 1);

    let runner = MockRunner::new();
    runner.push_response(success_output(
        br#"{"result":{"type":"workspace_info","workspace":{"workspace_id":"w7","label":"Support Herdr"}}}"#,
    ));
    runner.push_response(success_output(br#"{"result":{}}"#));
    let mut workspace = herdr_workspace(&[]);
    workspace.multiplexer_state = stored_state(
        r#"
version = 1
adapter = "herdr"

[payload]
session = "agents"
workspace_id = "w7"
label = "Support Herdr"
"#,
    );
    let report =
        TerminalEnvironment::new(&config_manager, &global_config, &runner).close(&workspace);
    assert!(!report.closed);
    assert_eq!(report.warnings.len(), 1);
    assert!(report.warnings[0].contains("malformed JSON"));
}

#[test]
fn herdr_activate_runs_and_names_single_repo_agent_in_primary_pane() {
    let (_temp, config_manager) = setup_config(&["api"]);
    let global_config = GlobalConfig {
        agent_cli_alias: BTreeMap::from([(
            "fast".into(),
            "codex --model gpt-5 --prompt $prompt".into(),
        )]),
        ..GlobalConfig::default()
    };
    let runner = MockRunner::new();
    runner.push_response(success_output(b"herdr 0.8.0\n"));
    runner.push_response(success_output(
        br#"{"result":{"type":"workspace_list","workspaces":[]}}"#,
    ));
    runner.push_response(success_output(
        r#"{"result":{"type":"workspace_created","workspace":{"workspace_id":"w7","label":"Support Herdr · zootree:calm-river"},"tab":{"tab_id":"w7:t1"},"root_pane":{"pane_id":"w7:p1"}}}"#
            .as_bytes(),
    ));
    runner.push_response(success_output(
        br#"{"result":{"type":"tab_info","tab":{"tab_id":"w7:t1"}}}"#,
    ));
    runner.push_response(success_output(
        br#"{"result":{"type":"pane_info","pane":{"pane_id":"w7:p2"}}}"#,
    ));
    runner.push_response(success_output(b""));
    runner.push_response(success_output(br#"{"result":{"type":"tab_created","tab":{"tab_id":"w7:t2"},"root_pane":{"pane_id":"w7:p3"}}}"#));
    runner.push_response(success_output(
        br#"{"result":{"type":"pane_info","pane":{"pane_id":"w7:p4"}}}"#,
    ));
    runner.push_response(success_output(
        br#"{"result":{"type":"pane_info","pane":{"pane_id":"w7:p5"}}}"#,
    ));
    runner.push_response(success_output(b""));
    runner.push_response(success_output(
        br#"{"result":{"type":"agent_info","agent":{"pane_id":"w7:p3","agent":"codex"}}}"#,
    ));
    runner.push_response(success_output(
        br#"{"result":{"type":"agent_info","agent":{"pane_id":"w7:p3","agent":"codex","name":"zt-calm-river"}}}"#,
    ));
    runner.push_response(success_output(
        br#"{"result":{"type":"tab_info","tab":{"tab_id":"w7:t2"}}}"#,
    ));
    runner.push_response(success_output(
        r#"{"result":{"type":"workspace_info","workspace":{"workspace_id":"w7","label":"Support Herdr · zootree:calm-river"}}}"#
            .as_bytes(),
    ));
    runner.push_response(success_output(b""));
    let workspace = herdr_workspace(&["api"]);

    let activation = TerminalEnvironment::new(&config_manager, &global_config, &runner)
        .activate(&workspace, AgentIntent::Override("fast".into()))
        .unwrap();

    assert!(activation.warnings.is_empty());
    let calls = runner.take_calls();
    assert_eq!(
        calls[9].args,
        vec![
            "--session",
            "agents",
            "pane",
            "run",
            "w7:p3",
            "codex --model gpt-5 --prompt 'Support Herdr\nExercise Herdr reconciliation'",
        ]
    );
    assert_eq!(
        calls[10].args,
        vec!["--session", "agents", "agent", "get", "w7:p3"]
    );
    assert_eq!(
        calls[11].args,
        vec![
            "--session",
            "agents",
            "agent",
            "rename",
            "w7:p3",
            "zt-calm-river",
        ]
    );
    assert_eq!(
        calls[12].args,
        vec!["--session", "agents", "tab", "focus", "w7:t2"]
    );
    assert_eq!(
        calls[13].args,
        vec!["--session", "agents", "workspace", "focus", "w7"]
    );
}

#[test]
fn activate_reuses_current_cmux_state_and_ignores_agent_request() {
    let (_temp, config_manager) = setup_config(&["api"]);
    let global_config = GlobalConfig::default();
    let runner = MockRunner::new();
    runner.push_response(success_output(b""));
    let mut workspace = workspace(&["api"]);
    workspace.multiplexer_state = stored_state(
        r#"
version = 1
adapter = "cmux"

[payload]
group = "workspace_group:2"

[[payload.repo_workspaces]]
repo = "api"
workspace = "workspace:4"
"#,
    );

    let activation = TerminalEnvironment::new(&config_manager, &global_config, &runner)
        .activate(&workspace, AgentIntent::Default)
        .unwrap();

    assert_eq!(
        state_group(&activation.stored_state).as_deref(),
        Some("workspace_group:2")
    );
    assert_eq!(activation.warnings.len(), 1);
    assert!(activation.warnings[0].contains("agent request was ignored"));
    let calls = runner.take_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0].args,
        vec!["workspace-group", "focus", "workspace_group:2"]
    );
}

#[test]
fn activate_recovers_stale_state_by_unique_group_name() {
    let (_temp, config_manager) = setup_config(&["api"]);
    let global_config = GlobalConfig::default();
    let runner = MockRunner::new();
    runner.push_response(failure_output(b"group not found"));
    runner.push_response(success_output(
        br#"{"groups":[{"name":"Terminal environment API","ref":"workspace_group:7"}]}"#,
    ));
    runner.push_response(success_output(b""));
    let mut workspace = workspace(&["api"]);
    workspace.multiplexer_state = stored_state(
        r#"
version = 1
adapter = "cmux"

[payload]
group = "workspace_group:2"
"#,
    );

    let activation = TerminalEnvironment::new(&config_manager, &global_config, &runner)
        .activate(&workspace, AgentIntent::None)
        .unwrap();

    assert_eq!(
        state_group(&activation.stored_state).as_deref(),
        Some("workspace_group:7")
    );
    assert!(activation.warnings[0].contains("was stale"));
    let calls = runner.take_calls();
    assert_eq!(calls[1].args, vec!["workspace-group", "list", "--json"]);
    assert_eq!(
        calls[2].args,
        vec!["workspace-group", "focus", "workspace_group:7"]
    );
}

#[test]
fn activate_adopts_unique_group_when_unknown_state_version_is_ignored() {
    let (_temp, config_manager) = setup_config(&["api"]);
    let global_config = GlobalConfig::default();
    let runner = MockRunner::new();
    runner.push_response(success_output(
        br#"{"groups":[{"name":"Terminal environment API","ref":"workspace_group:8"}]}"#,
    ));
    runner.push_response(success_output(b""));
    let mut workspace = workspace(&["api"]);
    workspace.multiplexer_state = stored_state(
        r#"
version = 99
adapter = "future"

[payload]
identity = "future:1"
"#,
    );

    let activation = TerminalEnvironment::new(&config_manager, &global_config, &runner)
        .activate(&workspace, AgentIntent::None)
        .unwrap();

    assert_eq!(
        state_group(&activation.stored_state).as_deref(),
        Some("workspace_group:8")
    );
    assert!(activation.warnings[0].contains("version 99 is unknown"));
}

#[test]
fn activate_warns_and_recovers_when_current_cmux_payload_is_corrupt() {
    let (_temp, config_manager) = setup_config(&["api"]);
    let global_config = GlobalConfig::default();
    let runner = MockRunner::new();
    runner.push_response(success_output(
        br#"{"groups":[{"name":"Terminal environment API","ref":"workspace_group:9"}]}"#,
    ));
    runner.push_response(success_output(b""));
    let mut workspace = workspace(&["api"]);
    workspace.multiplexer_state = stored_state(
        r#"
version = 1
adapter = "cmux"

[payload]
unexpected = "field"
"#,
    );

    let activation = TerminalEnvironment::new(&config_manager, &global_config, &runner)
        .activate(&workspace, AgentIntent::None)
        .unwrap();

    assert_eq!(
        state_group(&activation.stored_state).as_deref(),
        Some("workspace_group:9")
    );
    assert!(activation.warnings[0].contains("stored cmux state was corrupt"));
}

#[test]
fn activate_creates_group_and_returns_canonical_state() {
    let (_temp, config_manager) = setup_config(&["api"]);
    let global_config = GlobalConfig {
        agent_cli: Some("codex --prompt $prompt".into()),
        ..GlobalConfig::default()
    };
    let runner = MockRunner::new();
    push_single_repo_create_responses(&runner);
    let workspace = workspace(&["api"]);

    let activation = TerminalEnvironment::new(&config_manager, &global_config, &runner)
        .activate(&workspace, AgentIntent::Default)
        .unwrap();

    let state = state_table(&activation.stored_state);
    assert_eq!(
        state.get("version").and_then(toml::Value::as_integer),
        Some(1)
    );
    assert_eq!(
        state.get("adapter").and_then(toml::Value::as_str),
        Some("cmux")
    );
    assert_eq!(
        state_group(&activation.stored_state).as_deref(),
        Some("workspace_group:2")
    );
    assert!(!state.contains_key("cmux_group"));
    assert!(activation.warnings.is_empty());
    let calls = runner.take_calls();
    assert_eq!(calls[0].args, vec!["workspace-group", "list", "--json"]);
    assert_eq!(
        calls[2].args,
        vec![
            "workspace-group",
            "create",
            "--name",
            "Terminal environment API",
            "--from",
            "workspace:4"
        ]
    );
    assert!(command_arg(&calls[1].args, "--layout").contains("codex"));
    assert!(!command_arg(&calls[4].args, "--layout").contains("codex"));
}

#[test]
fn activate_places_override_alias_in_multi_repo_anchor_only() {
    let (_temp, config_manager) = setup_config(&["api", "web"]);
    let global_config = GlobalConfig {
        agent_cli_alias: BTreeMap::from([(
            "fast".into(),
            "codex --model gpt-5 --prompt $prompt".into(),
        )]),
        ..GlobalConfig::default()
    };
    let runner = MockRunner::new();
    push_single_repo_create_responses(&runner);
    runner.push_response(success_output(b"workspace:5\n"));
    let workspace = workspace(&["api", "web"]);

    TerminalEnvironment::new(&config_manager, &global_config, &runner)
        .activate(&workspace, AgentIntent::Override("fast".into()))
        .unwrap();

    let calls = runner.take_calls();
    assert!(!command_arg(&calls[1].args, "--layout").contains("codex"));
    assert!(command_arg(&calls[4].args, "--layout").contains("codex"));
    assert!(!command_arg(&calls[7].args, "--layout").contains("codex"));
}

#[test]
fn activate_rejects_ambiguous_name_without_creating() {
    let (_temp, config_manager) = setup_config(&["api"]);
    let global_config = GlobalConfig::default();
    let runner = MockRunner::new();
    runner.push_response(success_output(
        br#"{"groups":[
            {"name":"Terminal environment API","ref":"workspace_group:7"},
            {"name":"Terminal environment API","ref":"workspace_group:8"}
        ]}"#,
    ));

    let error = TerminalEnvironment::new(&config_manager, &global_config, &runner)
        .activate(&workspace(&["api"]), AgentIntent::None)
        .unwrap_err();

    assert!(error.to_string().contains("ambiguous"));
    let calls = runner.take_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].args, vec!["workspace-group", "list", "--json"]);
}

#[test]
fn activate_rejects_non_default_cmux_layout_before_creation() {
    let (_temp, config_manager) = setup_config(&["api"]);
    let global_config = GlobalConfig::default();
    let runner = MockRunner::new();
    runner.push_response(success_output(br#"{"groups":[]}"#));
    let mut workspace = workspace(&["api"]);
    workspace.multiplexer.cmux.layout = Some("wide".into());

    let error = TerminalEnvironment::new(&config_manager, &global_config, &runner)
        .activate(&workspace, AgentIntent::None)
        .unwrap_err();

    assert!(error
        .to_string()
        .contains("group-aware cmux currently supports only layout = \"default\""));
    assert_eq!(runner.take_calls().len(), 1);
}

#[test]
fn activate_rolls_back_created_workspace_when_group_creation_is_invalid() {
    let (_temp, config_manager) = setup_config(&["api"]);
    let global_config = GlobalConfig::default();
    let runner = MockRunner::new();
    runner.push_response(success_output(br#"{"groups":[]}"#));
    runner.push_response(success_output(b"workspace:4\n"));
    runner.push_response(success_output(b"created group without ref\n"));
    runner.push_response(success_output(b""));

    let error = TerminalEnvironment::new(&config_manager, &global_config, &runner)
        .activate(&workspace(&["api"]), AgentIntent::None)
        .unwrap_err();

    assert!(error.to_string().contains("did not return a group ref"));
    let calls = runner.take_calls();
    assert_eq!(
        calls.last().unwrap().args,
        vec!["workspace", "close", "workspace:4"]
    );
}

#[test]
fn legacy_cmux_group_state_is_migrated_after_successful_activate() {
    let (_temp, config_manager) = setup_config(&["api"]);
    let global_config = GlobalConfig::default();
    let runner = MockRunner::new();
    runner.push_response(success_output(b""));
    let mut workspace = workspace(&["api"]);
    workspace.multiplexer_state = stored_state(
        r#"
kind = "cmux"
cmux_group = "workspace_group:2"

[[cmux_repo_workspaces]]
repo = "api"
workspace = "workspace:4"
"#,
    );

    let activation = TerminalEnvironment::new(&config_manager, &global_config, &runner)
        .activate(&workspace, AgentIntent::None)
        .unwrap();

    let state = state_table(&activation.stored_state);
    assert_eq!(
        state.get("version").and_then(toml::Value::as_integer),
        Some(1)
    );
    assert_eq!(
        state_group(&activation.stored_state).as_deref(),
        Some("workspace_group:2")
    );
    assert!(!state.contains_key("kind"));
    assert!(!state.contains_key("cmux_group"));
}

#[test]
fn close_uses_stored_group_and_missing_target_is_success() {
    let (_temp, config_manager) = setup_config(&[]);
    let global_config = GlobalConfig::default();
    let runner = MockRunner::new();
    runner.push_response(success_output(b""));
    let mut stored_workspace = workspace(&[]);
    stored_workspace.multiplexer_state = stored_state(
        r#"
version = 1
adapter = "cmux"

[payload]
group = "workspace_group:2"
"#,
    );

    let report =
        TerminalEnvironment::new(&config_manager, &global_config, &runner).close(&stored_workspace);

    assert!(report.warnings.is_empty());
    assert_eq!(
        runner.take_calls()[0].args,
        vec!["workspace-group", "delete", "workspace_group:2"]
    );

    let runner = MockRunner::new();
    runner.push_response(success_output(br#"{"groups":[]}"#));
    let report =
        TerminalEnvironment::new(&config_manager, &global_config, &runner).close(&workspace(&[]));
    assert!(report.warnings.is_empty());

    let runner = MockRunner::new();
    runner.push_response(success_output(
        br#"{"groups":[{"name":"Terminal environment API","ref":"workspace_group:7"}]}"#,
    ));
    runner.push_response(success_output(b""));
    let report =
        TerminalEnvironment::new(&config_manager, &global_config, &runner).close(&workspace(&[]));
    assert!(report.warnings.is_empty());
    let calls = runner.take_calls();
    assert_eq!(calls[0].args, vec!["workspace-group", "list", "--json"]);
    assert_eq!(
        calls[1].args,
        vec!["workspace-group", "delete", "workspace_group:7"]
    );
}

#[test]
fn close_warns_when_stored_delete_fails_before_unique_name_fallback_succeeds() {
    let (_temp, config_manager) = setup_config(&[]);
    let global_config = GlobalConfig::default();
    let runner = MockRunner::new();
    runner.push_response(failure_output(b"stored group not found"));
    runner.push_response(success_output(
        br#"{"groups":[{"name":"Terminal environment API","ref":"workspace_group:7"}]}"#,
    ));
    runner.push_response(success_output(b""));
    let mut workspace = workspace(&[]);
    workspace.multiplexer_state = stored_state(
        r#"
version = 1
adapter = "cmux"

[payload]
group = "workspace_group:2"
"#,
    );

    let report =
        TerminalEnvironment::new(&config_manager, &global_config, &runner).close(&workspace);

    assert!(report.closed);
    assert_eq!(report.warnings.len(), 1);
    assert!(report.warnings[0].contains("stored cmux group 'workspace_group:2'"));
    assert!(report.warnings[0].contains("completed close fallback by name"));
    let calls = runner.take_calls();
    assert_eq!(
        calls[0].args,
        vec!["workspace-group", "delete", "workspace_group:2"]
    );
    assert_eq!(calls[1].args, vec!["workspace-group", "list", "--json"]);
    assert_eq!(
        calls[2].args,
        vec!["workspace-group", "delete", "workspace_group:7"]
    );
}

#[test]
fn close_reports_ambiguous_and_command_failures_as_warnings() {
    let (_temp, config_manager) = setup_config(&[]);
    let global_config = GlobalConfig::default();
    let runner = MockRunner::new();
    runner.push_response(success_output(
        br#"{"groups":[
            {"name":"Terminal environment API","ref":"workspace_group:7"},
            {"name":"Terminal environment API","ref":"workspace_group:8"}
        ]}"#,
    ));

    let report =
        TerminalEnvironment::new(&config_manager, &global_config, &runner).close(&workspace(&[]));
    assert!(!report.closed);
    assert_eq!(report.warnings.len(), 1);
    assert!(report.warnings[0].contains("ambiguous"));

    let runner = MockRunner::new();
    runner.push_response(failure_output(b"delete failed"));
    runner.push_response(failure_output(b"list failed"));
    let mut workspace = workspace(&[]);
    workspace.multiplexer_state = stored_state(
        r#"
version = 1
adapter = "cmux"

[payload]
group = "workspace_group:2"
"#,
    );
    let report =
        TerminalEnvironment::new(&config_manager, &global_config, &runner).close(&workspace);
    assert!(!report.closed);
    assert_eq!(report.warnings.len(), 1);
    assert!(report.warnings[0].contains("failed to close cmux"));
}
