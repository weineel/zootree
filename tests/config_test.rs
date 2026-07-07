use std::os::unix::process::ExitStatusExt;
use std::process::{ExitStatus, Output};
use tempfile::TempDir;
use zootree::cli::workspace::{build_repo_entries, parse_repos_arg};
use zootree::config::global::GlobalConfig;
use zootree::config::global::HookValue;
use zootree::config::global::{MultiplexerConfig, MultiplexerKind};
use zootree::config::repo::RepoConfig;
use zootree::config::workspace::{MultiplexerState, WorkspaceConfig, WorkspaceStatus};
use zootree::config::ConfigManager;
use zootree::runner::MockRunner;

fn assert_unknown_field_error(error: toml::de::Error, field: &str) {
    let message = error.to_string();
    assert!(
        message.contains("unknown field") || message.contains(field),
        "unexpected error: {message}"
    );
}

#[test]
fn test_parse_global_config_full() {
    let toml_str = r#"
workspace_root = "~/zootree-workspaces"
branch_prefix = "zootree"
copy_files = [".env"]
agent_cli = "claude --dangerously-skip-permissions -- $prompt"

[multiplexer]
kind = "zellij"

[multiplexer.zellij]
layout = "default"

[hooks]
post_create = "echo hello"

[log]
max_files = 5
"#;
    let config: GlobalConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.multiplexer.kind, MultiplexerKind::Zellij);
    assert_eq!(config.multiplexer.zellij.layout, Some("default".into()));
    assert_eq!(config.workspace_root, "~/zootree-workspaces");
    assert_eq!(config.branch_prefix, "zootree");
    assert_eq!(config.copy_files, vec![".env"]);
    assert_eq!(
        config.hooks.post_create,
        Some(zootree::config::global::HookValue::Simple(
            "echo hello".into()
        ))
    );
    assert_eq!(config.log.max_files, Some(5));
    assert_eq!(
        config.agent_cli.as_deref(),
        Some("claude --dangerously-skip-permissions -- $prompt")
    );
}

#[test]
fn test_parse_global_config_defaults() {
    let toml_str = "";
    let config: GlobalConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.multiplexer.kind, MultiplexerKind::Zellij);
    assert_eq!(config.multiplexer.zellij.layout, Some("default".into()));
    assert_eq!(config.branch_prefix, "zootree");
    assert!(config.copy_files.is_empty());
    assert!(config.agent_cli.is_none());
}

#[test]
fn parse_global_config_defaults_to_zellij_multiplexer() {
    let config: GlobalConfig = toml::from_str("").unwrap();
    let multiplexer: MultiplexerConfig = config.multiplexer;

    assert_eq!(multiplexer.kind, MultiplexerKind::Zellij);
    assert_eq!(multiplexer.zellij.layout.as_deref(), Some("default"));
    assert_eq!(multiplexer.cmux.layout.as_deref(), Some("default"));
}

#[test]
fn parse_global_config_with_cmux_multiplexer() {
    let toml_str = r#"
workspace_root = "~/zootree-workspaces"
branch_prefix = "zootree"

[multiplexer]
kind = "cmux"

[multiplexer.cmux]
layout = "daily"
"#;

    let config: GlobalConfig = toml::from_str(toml_str).unwrap();

    assert_eq!(config.multiplexer.kind, MultiplexerKind::Cmux);
    assert_eq!(config.multiplexer.cmux.layout.as_deref(), Some("daily"));
    assert_eq!(config.multiplexer.zellij.layout.as_deref(), Some("default"));
}

#[test]
fn test_parse_repo_config() {
    let toml_str = r#"
path = "~/projects/frontend"
default_target_branch = "develop"
copy_files = [".env.local", ".vscode/settings.json"]

[hooks]
post_create = "npm install"

[hooks.pre_remove]
file = "~/.config/zootree/hooks/cleanup.sh"

[lazygit]
config = "~/projects/frontend/.lazygit.yml"
"#;
    let config: RepoConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.path, "~/projects/frontend");
    assert_eq!(config.default_target_branch, Some("develop".into()));
    assert_eq!(
        config.copy_files,
        vec![".env.local", ".vscode/settings.json"]
    );
    assert_eq!(
        config.hooks.post_create,
        Some(HookValue::Simple("npm install".into()))
    );
    assert_eq!(
        config.hooks.pre_remove,
        Some(HookValue::File {
            file: "~/.config/zootree/hooks/cleanup.sh".into()
        })
    );
    assert_eq!(
        config.lazygit.as_ref().unwrap().config,
        "~/projects/frontend/.lazygit.yml"
    );
}

#[test]
fn test_parse_repo_config_minimal() {
    let toml_str = r#"
path = "~/projects/backend"
"#;
    let config: RepoConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.path, "~/projects/backend");
    assert!(config.default_target_branch.is_none());
    assert!(config.copy_files.is_empty());
}

#[test]
fn test_parse_workspace_config() {
    let toml_str = r#"
title = "用户认证功能"
name = "calm-river"
description = "前后端联调 OAuth2 登录"
branch = "zootree/calm-river"
workspace_dir = "~/zootree-workspaces/calm-river"
created_at = "2026-04-28T10:30:00+08:00"

[[repos]]
name = "frontend"
target_branch = "develop"

[[repos]]
name = "backend"
target_branch = "develop"

[[events]]
action = "created"
timestamp = "2026-04-28T10:30:00+08:00"
"#;
    let config: WorkspaceConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.title, "用户认证功能");
    assert_eq!(config.name, "calm-river");
    assert_eq!(config.repos.len(), 2);
    assert_eq!(config.repos[0].name, "frontend");
    assert_eq!(config.repos[0].target_branch, Some("develop".into()));
    assert_eq!(config.events.len(), 1);
    assert!(config.agent_cli.is_none());
}

#[test]
fn test_parse_workspace_config_with_agent_cli() {
    let toml_str = r#"
title = "用户认证功能"
name = "calm-river"
description = "前后端联调 OAuth2 登录"
branch = "zootree/calm-river"
workspace_dir = "~/zootree-workspaces/calm-river"
created_at = "2026-04-28T10:30:00+08:00"
agent_cli = "codexd_brainstorming"
"#;
    let config: WorkspaceConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.agent_cli.as_deref(), Some("codexd_brainstorming"));
}

#[test]
fn parse_workspace_config_with_multiplexer_state() {
    let toml_str = r#"
title = "用户认证功能"
name = "calm-river"
description = "前后端联调 OAuth2 登录"
branch = "zootree/calm-river"
workspace_dir = "~/zootree-workspaces/calm-river"
created_at = "2026-04-28T10:30:00+08:00"

[multiplexer]
kind = "cmux"

[multiplexer.cmux]
layout = "wide"

[multiplexer_state]
kind = "cmux"
cmux_workspace = "workspace:3"
"#;

    let config: WorkspaceConfig = toml::from_str(toml_str).unwrap();

    assert_eq!(config.multiplexer.kind, MultiplexerKind::Cmux);
    assert_eq!(config.multiplexer.cmux.layout.as_deref(), Some("wide"));
    assert_eq!(config.multiplexer_state.kind, Some(MultiplexerKind::Cmux));
    assert_eq!(
        config.multiplexer_state.cmux_workspace.as_deref(),
        Some("workspace:3")
    );
}

#[test]
fn old_zellij_global_config_is_rejected() {
    let toml_str = r#"
[zellij]
layout = "custom"
"#;
    assert_unknown_field_error(
        toml::from_str::<GlobalConfig>(toml_str).unwrap_err(),
        "zellij",
    );
}

#[test]
fn old_zellij_workspace_config_is_rejected() {
    let toml_str = r#"
title = "用户认证功能"
name = "calm-river"
description = "前后端联调 OAuth2 登录"
branch = "zootree/calm-river"
workspace_dir = "~/zootree-workspaces/calm-river"
created_at = "2026-04-28T10:30:00+08:00"

[zellij]
layout = "custom"
"#;
    assert_unknown_field_error(
        toml::from_str::<WorkspaceConfig>(toml_str).unwrap_err(),
        "zellij",
    );
}

#[test]
fn list_workspaces_fails_fast_on_legacy_zellij_workspace_config() {
    let tmp = TempDir::new().unwrap();
    let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
    mgr.ensure_dirs().unwrap();
    let path = tmp.path().join("workspaces/pending/calm-river.toml");
    std::fs::write(
        &path,
        r#"
title = "用户认证功能"
name = "calm-river"
description = ""
branch = "zootree/calm-river"
workspace_dir = "~/zootree-workspaces/calm-river"
created_at = "2026-04-28T10:30:00+08:00"

[zellij]
layout = "custom"
"#,
    )
    .unwrap();

    let err = mgr
        .list_workspaces(Some(&[WorkspaceStatus::Pending]))
        .unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("failed to parse workspace config") && msg.contains("calm-river.toml"),
        "unexpected error: {msg}"
    );
}

#[test]
fn old_zellij_repo_config_is_rejected() {
    let toml_str = r#"
path = "~/projects/frontend"

[zellij]
layout = "custom"
"#;
    assert_unknown_field_error(
        toml::from_str::<RepoConfig>(toml_str).unwrap_err(),
        "zellij",
    );
}

#[test]
fn repo_multiplexer_config_is_rejected() {
    let toml_str = r#"
path = "~/projects/frontend"

[multiplexer]
kind = "cmux"
"#;
    assert_unknown_field_error(
        toml::from_str::<RepoConfig>(toml_str).unwrap_err(),
        "multiplexer",
    );
}

#[test]
fn old_zellij_template_config_is_rejected() {
    let toml_str = r#"
repos = ["frontend"]

[zellij]
layout = "custom"
"#;
    assert_unknown_field_error(
        toml::from_str::<zootree::config::template::TemplateConfig>(toml_str).unwrap_err(),
        "zellij",
    );
}

#[test]
fn unknown_multiplexer_field_is_rejected() {
    let toml_str = r#"
[multiplexer]
kind = "zellij"
session_name = "shared-session"
"#;
    assert_unknown_field_error(
        toml::from_str::<GlobalConfig>(toml_str).unwrap_err(),
        "session_name",
    );
}

#[test]
fn zellij_session_mode_is_rejected() {
    let toml_str = r#"
[multiplexer]
kind = "zellij"

[multiplexer.zellij]
layout = "default"
session_mode = "shared"
"#;
    assert_unknown_field_error(
        toml::from_str::<GlobalConfig>(toml_str).unwrap_err(),
        "session_mode",
    );
}

#[test]
fn zellij_session_name_is_rejected() {
    let toml_str = r#"
[multiplexer]
kind = "zellij"

[multiplexer.zellij]
layout = "default"
session_name = "shared-session"
"#;
    assert_unknown_field_error(
        toml::from_str::<GlobalConfig>(toml_str).unwrap_err(),
        "session_name",
    );
}

#[test]
fn cmux_unknown_field_is_rejected() {
    let toml_str = r#"
[multiplexer]
kind = "cmux"

[multiplexer.cmux]
layout = "default"
workspace = "workspace:3"
"#;
    assert_unknown_field_error(
        toml::from_str::<GlobalConfig>(toml_str).unwrap_err(),
        "workspace",
    );
}

#[test]
fn multiplexer_state_unknown_field_is_rejected() {
    let toml_str = r#"
title = "用户认证功能"
name = "calm-river"
description = "前后端联调 OAuth2 登录"
branch = "zootree/calm-river"
workspace_dir = "~/zootree-workspaces/calm-river"
created_at = "2026-04-28T10:30:00+08:00"

[multiplexer_state]
kind = "cmux"
zellij_session = "old-session"
"#;
    assert_unknown_field_error(
        toml::from_str::<WorkspaceConfig>(toml_str).unwrap_err(),
        "zellij_session",
    );
}

#[test]
fn test_parse_template_config() {
    let toml_str = r#"
repos = ["frontend", "backend", "shared-lib"]

[multiplexer]
kind = "zellij"

[multiplexer.zellij]
layout = "default"
"#;
    let config: zootree::config::template::TemplateConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.repos, vec!["frontend", "backend", "shared-lib"]);
    assert_eq!(config.multiplexer.kind, MultiplexerKind::Zellij);
    assert_eq!(config.multiplexer.zellij.layout, Some("default".into()));
}

#[test]
fn empty_multiplexer_state_is_not_serialized() {
    let config = WorkspaceConfig {
        title: "用户认证功能".into(),
        name: "calm-river".into(),
        description: "前后端联调 OAuth2 登录".into(),
        branch: "zootree/calm-river".into(),
        workspace_dir: "~/zootree-workspaces/calm-river".into(),
        created_at: "2026-04-28T10:30:00+08:00".into(),
        agent_cli: None,
        multiplexer: MultiplexerConfig::default(),
        multiplexer_state: MultiplexerState::default(),
        repos: Vec::new(),
        events: Vec::new(),
    };

    let serialized = toml::to_string(&config).unwrap();

    assert!(
        !serialized.contains("[multiplexer_state]"),
        "empty multiplexer_state should be skipped, got: {serialized}"
    );
}

#[test]
fn cmux_workspace_state_serializes_and_round_trips() {
    let config = WorkspaceConfig {
        title: "用户认证功能".into(),
        name: "calm-river".into(),
        description: "前后端联调 OAuth2 登录".into(),
        branch: "zootree/calm-river".into(),
        workspace_dir: "~/zootree-workspaces/calm-river".into(),
        created_at: "2026-04-28T10:30:00+08:00".into(),
        agent_cli: None,
        multiplexer: MultiplexerConfig::default(),
        multiplexer_state: MultiplexerState {
            kind: Some(MultiplexerKind::Cmux),
            cmux_workspace: Some("workspace:3".into()),
        },
        repos: Vec::new(),
        events: Vec::new(),
    };

    let serialized = toml::to_string(&config).unwrap();

    assert!(
        serialized.contains("[multiplexer_state]"),
        "non-empty multiplexer_state should be serialized, got: {serialized}"
    );
    assert!(
        serialized.contains("cmux_workspace = \"workspace:3\""),
        "cmux workspace should be serialized, got: {serialized}"
    );

    let round_tripped: WorkspaceConfig = toml::from_str(&serialized).unwrap();

    assert_eq!(
        round_tripped.multiplexer_state.cmux_workspace.as_deref(),
        Some("workspace:3")
    );
    assert_eq!(
        round_tripped.multiplexer_state.kind,
        Some(MultiplexerKind::Cmux)
    );
}

#[test]
fn test_parse_repos_arg() {
    let result = parse_repos_arg("frontend:develop,backend,shared-lib:main");
    assert_eq!(
        result,
        vec![
            ("frontend".into(), Some("develop".into())),
            ("backend".into(), None),
            ("shared-lib".into(), Some("main".into())),
        ]
    );
}

#[test]
fn test_parse_repos_arg_single() {
    let result = parse_repos_arg("frontend:develop");
    assert_eq!(result, vec![("frontend".into(), Some("develop".into()))]);
}

fn success_branch_output(branch: &str) -> Output {
    Output {
        status: ExitStatus::from_raw(0),
        stdout: format!("{}\n", branch).into_bytes(),
        stderr: Vec::new(),
    }
}

#[test]
fn build_repo_entries_prefers_explicit_branch() {
    let tmp = TempDir::new().unwrap();
    let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
    mgr.ensure_dirs().unwrap();
    mgr.save_repo_config(
        "frontend",
        &RepoConfig {
            path: "/repo/frontend".into(),
            default_target_branch: Some("develop".into()),
            copy_files: Vec::new(),
            hooks: Default::default(),
            lazygit: None,
        },
    )
    .unwrap();
    let runner = MockRunner::new();

    let entries = build_repo_entries(
        &mgr,
        &runner,
        vec![("frontend".to_string(), Some("release".to_string()))],
    )
    .unwrap();

    assert_eq!(entries[0].name, "frontend");
    assert_eq!(entries[0].target_branch.as_deref(), Some("release"));
    assert!(runner.take_calls().is_empty());
}

#[test]
fn build_repo_entries_uses_repo_default_branch() {
    let tmp = TempDir::new().unwrap();
    let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
    mgr.ensure_dirs().unwrap();
    mgr.save_repo_config(
        "frontend",
        &RepoConfig {
            path: "/repo/frontend".into(),
            default_target_branch: Some("develop".into()),
            copy_files: Vec::new(),
            hooks: Default::default(),
            lazygit: None,
        },
    )
    .unwrap();
    let runner = MockRunner::new();

    let entries = build_repo_entries(&mgr, &runner, vec![("frontend".to_string(), None)]).unwrap();

    assert_eq!(entries[0].target_branch.as_deref(), Some("develop"));
    assert!(runner.take_calls().is_empty());
}

#[test]
fn build_repo_entries_falls_back_to_current_branch() {
    let tmp = TempDir::new().unwrap();
    let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
    mgr.ensure_dirs().unwrap();
    mgr.save_repo_config(
        "frontend",
        &RepoConfig {
            path: "/repo/frontend".into(),
            default_target_branch: None,
            copy_files: Vec::new(),
            hooks: Default::default(),
            lazygit: None,
        },
    )
    .unwrap();
    let runner = MockRunner::new();
    runner.push_response(success_branch_output("mainline"));

    let entries = build_repo_entries(&mgr, &runner, vec![("frontend".to_string(), None)]).unwrap();

    assert_eq!(entries[0].target_branch.as_deref(), Some("mainline"));
    let calls = runner.take_calls();
    assert_eq!(
        calls[0].args,
        vec!["-C", "/repo/frontend", "rev-parse", "--abbrev-ref", "HEAD"]
    );
}

#[test]
fn test_parse_global_config_agent_cli_alias() {
    let toml_str = r#"
agent_cli = "claude"

[agent_cli_alias]
claude = "claude --dangerously-skip-permissions -- $prompt"
gemini = "gemini chat -- $prompt"
"#;
    let config: GlobalConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.agent_cli.as_deref(), Some("claude"));
    assert_eq!(config.agent_cli_alias.len(), 2);
    assert_eq!(
        config.agent_cli_alias.get("claude").map(String::as_str),
        Some("claude --dangerously-skip-permissions -- $prompt")
    );
    assert_eq!(
        config.agent_cli_alias.get("gemini").map(String::as_str),
        Some("gemini chat -- $prompt")
    );
}

#[test]
fn test_parse_global_config_agent_cli_alias_default_empty() {
    let toml_str = "";
    let config: GlobalConfig = toml::from_str(toml_str).unwrap();
    assert!(config.agent_cli_alias.is_empty());
}

#[test]
fn test_serialize_global_config_agent_cli_alias_empty_omitted() {
    let cfg = GlobalConfig {
        agent_cli: Some("claude -- $prompt".into()),
        ..GlobalConfig::default()
    };
    let s = toml::to_string(&cfg).unwrap();
    assert!(
        !s.contains("agent_cli_alias"),
        "empty map should be skipped during serialization, got: {}",
        s
    );
}

#[test]
fn workspace_status_value_enum_parses_kebab_case() {
    use clap::ValueEnum;
    use zootree::config::workspace::WorkspaceStatus;

    assert_eq!(
        WorkspaceStatus::from_str("pending", false).unwrap(),
        WorkspaceStatus::Pending
    );
    assert_eq!(
        WorkspaceStatus::from_str("in-progress", false).unwrap(),
        WorkspaceStatus::InProgress
    );
    assert_eq!(
        WorkspaceStatus::from_str("done", false).unwrap(),
        WorkspaceStatus::Done
    );
    assert_eq!(
        WorkspaceStatus::from_str("canceled", false).unwrap(),
        WorkspaceStatus::Canceled
    );
}
