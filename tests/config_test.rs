use std::os::unix::process::ExitStatusExt;
use std::process::{ExitStatus, Output};
use tempfile::TempDir;
use zootree::cli::workspace::{build_repo_entries, parse_repos_arg};
use zootree::config::global::GlobalConfig;
use zootree::config::global::HookValue;
use zootree::config::global::{LogConfig, MultiplexerConfig, MultiplexerKind};
use zootree::config::repo::RepoConfig;
use zootree::config::workspace::{
    CmuxRepoWorkspaceState, MultiplexerState, WorkspaceConfig, WorkspaceStatus,
};
use zootree::config::ConfigManager;
use zootree::core::logging::{resolve_log_dir, resolve_log_file_path};
use zootree::runner::MockRunner;

fn test_repo_config(path: &str) -> RepoConfig {
    RepoConfig {
        path: path.into(),
        default_target_branch: None,
        copy_files: Vec::new(),
        hooks: Default::default(),
        lazygit: None,
    }
}

fn test_workspace_config(name: &str) -> WorkspaceConfig {
    WorkspaceConfig {
        title: "config path validation".into(),
        name: name.into(),
        description: String::new(),
        branch: format!("zootree/{name}"),
        workspace_dir: format!("~/zootree-workspaces/{name}"),
        created_at: "2026-07-10T10:00:00+08:00".into(),
        agent_cli: None,
        multiplexer: MultiplexerConfig::default(),
        multiplexer_state: MultiplexerState::default(),
        repos: Vec::new(),
        events: Vec::new(),
    }
}

fn test_template_config() -> zootree::config::template::TemplateConfig {
    zootree::config::template::TemplateConfig {
        repos: vec!["frontend".into()],
        multiplexer: MultiplexerConfig::default(),
    }
}

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
dir = "~/zootree-logs"
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
    assert_eq!(config.log.dir.as_deref(), Some("~/zootree-logs"));
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
    assert_eq!(config.log, LogConfig::default());
    assert!(config.agent_cli.is_none());
}

#[test]
fn log_path_defaults_to_config_manager_logs_dir() {
    let temp = TempDir::new().unwrap();
    let mgr = ConfigManager::with_base_dir(temp.path().join("zootree"));
    let config = GlobalConfig::default();

    assert_eq!(resolve_log_dir(&mgr, &config), mgr.base_dir.join("logs"));
    assert_eq!(
        resolve_log_file_path(&mgr, &config),
        mgr.base_dir.join("logs/zootree.log")
    );
}

#[test]
fn log_path_uses_configured_log_dir_without_creating_it() {
    let temp = TempDir::new().unwrap();
    let mgr = ConfigManager::with_base_dir(temp.path().join("zootree"));
    let custom_dir = temp.path().join("custom-logs");
    let mut config = GlobalConfig::default();
    config.log.dir = Some(custom_dir.to_string_lossy().into_owned());

    assert_eq!(resolve_log_dir(&mgr, &config), custom_dir);
    assert_eq!(
        resolve_log_file_path(&mgr, &config),
        temp.path().join("custom-logs/zootree.log")
    );
    assert!(
        !temp.path().join("custom-logs").exists(),
        "path resolution should not create log directories"
    );
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
fn parse_workspace_config_with_cmux_group_state() {
    let toml_str = r#"
title = "用户认证功能"
name = "calm-river"
description = "前后端联调 OAuth2 登录"
branch = "zootree/calm-river"
workspace_dir = "~/zootree-workspaces/calm-river"
created_at = "2026-04-28T10:30:00+08:00"

[multiplexer]
kind = "cmux"

[multiplexer_state]
kind = "cmux"
cmux_group = "workspace_group:2"
cmux_anchor_workspace = "workspace:4"

[[multiplexer_state.cmux_repo_workspaces]]
repo = "frontend"
workspace = "workspace:5"

[[multiplexer_state.cmux_repo_workspaces]]
repo = "backend"
workspace = "workspace:6"
"#;

    let config: WorkspaceConfig = toml::from_str(toml_str).unwrap();

    assert_eq!(config.multiplexer_state.kind, Some(MultiplexerKind::Cmux));
    assert_eq!(
        config.multiplexer_state.cmux_group.as_deref(),
        Some("workspace_group:2")
    );
    assert_eq!(
        config.multiplexer_state.cmux_anchor_workspace.as_deref(),
        Some("workspace:4")
    );
    assert_eq!(config.multiplexer_state.cmux_repo_workspaces.len(), 2);
    assert_eq!(
        config.multiplexer_state.cmux_repo_workspaces[0].repo,
        "frontend"
    );
    assert_eq!(
        config.multiplexer_state.cmux_repo_workspaces[0].workspace,
        "workspace:5"
    );
    assert_eq!(
        config.multiplexer_state.cmux_repo_workspaces[1].repo,
        "backend"
    );
    assert_eq!(
        config.multiplexer_state.cmux_repo_workspaces[1].workspace,
        "workspace:6"
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

fn workspace_config(name: &str, title: &str) -> WorkspaceConfig {
    WorkspaceConfig {
        title: title.into(),
        name: name.into(),
        description: String::new(),
        branch: format!("zootree/{}", name),
        workspace_dir: format!("~/zootree-workspaces/{}", name),
        created_at: "2026-04-28T10:30:00+08:00".into(),
        agent_cli: None,
        multiplexer: MultiplexerConfig::default(),
        multiplexer_state: MultiplexerState::default(),
        repos: Vec::new(),
        events: Vec::new(),
    }
}

#[test]
fn list_workspaces_returns_status_order_then_name_order() {
    let tmp = TempDir::new().unwrap();
    let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
    mgr.ensure_dirs().unwrap();

    mgr.save_workspace(
        &WorkspaceStatus::InProgress,
        &workspace_config("beta", "Beta"),
    )
    .unwrap();
    mgr.save_workspace(&WorkspaceStatus::Pending, &workspace_config("zulu", "Zulu"))
        .unwrap();
    mgr.save_workspace(
        &WorkspaceStatus::InProgress,
        &workspace_config("alpha", "Alpha"),
    )
    .unwrap();
    mgr.save_workspace(&WorkspaceStatus::Pending, &workspace_config("echo", "Echo"))
        .unwrap();

    let workspaces = mgr
        .list_workspaces(Some(&[
            WorkspaceStatus::Pending,
            WorkspaceStatus::InProgress,
        ]))
        .unwrap();
    let names: Vec<_> = workspaces.into_iter().map(|ws| ws.name).collect();

    assert_eq!(names, vec!["echo", "zulu", "alpha", "beta"]);
}

#[test]
fn list_workspaces_with_status_returns_status_and_config_once() {
    let tmp = TempDir::new().unwrap();
    let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
    mgr.ensure_dirs().unwrap();

    mgr.save_workspace(
        &WorkspaceStatus::InProgress,
        &workspace_config("beta", "Beta"),
    )
    .unwrap();
    mgr.save_workspace(
        &WorkspaceStatus::Pending,
        &workspace_config("alpha", "Alpha"),
    )
    .unwrap();

    let entries = mgr
        .list_workspaces_with_status(Some(&[
            WorkspaceStatus::Pending,
            WorkspaceStatus::InProgress,
        ]))
        .unwrap();
    let got: Vec<_> = entries
        .into_iter()
        .map(|entry| (entry.status, entry.config.name))
        .collect();

    assert_eq!(
        got,
        vec![
            (WorkspaceStatus::Pending, "alpha".into()),
            (WorkspaceStatus::InProgress, "beta".into())
        ]
    );
}

#[test]
fn config_manager_rejects_invalid_repo_names_used_for_paths() {
    let tmp = TempDir::new().unwrap();
    let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
    mgr.ensure_dirs().unwrap();
    let config = test_repo_config("/repo/frontend");

    let err = mgr.save_repo_config("../outside", &config).unwrap_err();
    assert!(
        err.to_string().contains("invalid repo name"),
        "unexpected error: {err}"
    );
    assert!(
        !tmp.path().join("outside.toml").exists(),
        "invalid repo name should not escape the repos directory"
    );

    for invalid in ["", "front/end", "front\\end", "front end", ".hidden"] {
        let err = mgr.load_repo_config(invalid).unwrap_err();
        assert!(
            err.to_string().contains("invalid repo name"),
            "name {invalid:?} produced unexpected error: {err}"
        );
    }

    let err = mgr.remove_repo_config("front/end").unwrap_err();
    assert!(
        err.to_string().contains("invalid repo name"),
        "unexpected error: {err}"
    );
}

#[test]
fn config_manager_rejects_invalid_workspace_names_used_for_paths() {
    let tmp = TempDir::new().unwrap();
    let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
    mgr.ensure_dirs().unwrap();
    let workspace = test_workspace_config("../outside");

    let err = mgr
        .save_workspace(&WorkspaceStatus::Pending, &workspace)
        .unwrap_err();
    assert!(
        err.to_string().contains("invalid workspace name"),
        "unexpected error: {err}"
    );
    assert!(
        !tmp.path().join("outside.toml").exists(),
        "invalid workspace name should not escape the workspaces directory"
    );

    for invalid in ["", "../outside", "open/reef", "open reef", ".hidden"] {
        let err = mgr.load_workspace(invalid).unwrap_err();
        assert!(
            err.to_string().contains("invalid workspace name"),
            "name {invalid:?} produced unexpected error: {err}"
        );
    }

    let err = mgr
        .move_workspace(
            "open/reef",
            &WorkspaceStatus::Pending,
            &WorkspaceStatus::InProgress,
        )
        .unwrap_err();
    assert!(
        err.to_string().contains("invalid workspace name"),
        "unexpected error: {err}"
    );

    let err = mgr
        .delete_workspace_config("open/reef", &WorkspaceStatus::Pending)
        .unwrap_err();
    assert!(
        err.to_string().contains("invalid workspace name"),
        "unexpected error: {err}"
    );
}

#[test]
fn config_manager_rejects_invalid_template_names_used_for_paths() {
    let tmp = TempDir::new().unwrap();
    let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
    mgr.ensure_dirs().unwrap();
    let template = test_template_config();

    let err = mgr.save_template("../outside", &template).unwrap_err();
    assert!(
        err.to_string().contains("invalid template name"),
        "unexpected error: {err}"
    );
    assert!(
        !tmp.path().join("outside.toml").exists(),
        "invalid template name should not escape the templates directory"
    );

    for invalid in ["", "../outside", "daily/work", "daily work", ".hidden"] {
        let err = mgr.load_template(invalid).unwrap_err();
        assert!(
            err.to_string().contains("invalid template name"),
            "name {invalid:?} produced unexpected error: {err}"
        );
    }
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
            cmux_group: None,
            cmux_anchor_workspace: None,
            cmux_repo_workspaces: Vec::new(),
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
fn group_aware_multiplexer_state_is_serialized_without_legacy_workspace_ref() {
    let config = WorkspaceConfig {
        title: "Group cmux".into(),
        name: "calm-river".into(),
        description: String::new(),
        branch: "zootree/calm-river".into(),
        workspace_dir: "~/zootree-workspaces/calm-river".into(),
        created_at: "2026-04-28T10:30:00+08:00".into(),
        agent_cli: None,
        multiplexer: MultiplexerConfig::default(),
        multiplexer_state: MultiplexerState {
            kind: Some(MultiplexerKind::Cmux),
            cmux_workspace: None,
            cmux_group: Some("workspace_group:2".into()),
            cmux_anchor_workspace: None,
            cmux_repo_workspaces: vec![
                CmuxRepoWorkspaceState {
                    repo: "frontend".into(),
                    workspace: "workspace:5".into(),
                },
                CmuxRepoWorkspaceState {
                    repo: "backend".into(),
                    workspace: "workspace:6".into(),
                },
            ],
        },
        repos: Vec::new(),
        events: Vec::new(),
    };

    let serialized = toml::to_string(&config).unwrap();

    assert!(serialized.contains("cmux_group = \"workspace_group:2\""));
    assert!(!serialized.contains("cmux_anchor_workspace"));
    assert!(serialized.contains("[[multiplexer_state.cmux_repo_workspaces]]"));
    assert!(serialized.contains("repo = \"frontend\""));
    assert!(serialized.contains("workspace = \"workspace:5\""));
    assert!(serialized.contains("repo = \"backend\""));
    assert!(serialized.contains("workspace = \"workspace:6\""));
    assert!(
        !serialized.contains("cmux_workspace"),
        "new group-aware state should not write legacy cmux_workspace: {serialized}"
    );

    let round_tripped: WorkspaceConfig = toml::from_str(&serialized).unwrap();
    assert_eq!(
        round_tripped.multiplexer_state.cmux_group.as_deref(),
        Some("workspace_group:2")
    );
    assert!(round_tripped
        .multiplexer_state
        .cmux_anchor_workspace
        .is_none());
    assert_eq!(
        round_tripped.multiplexer_state.cmux_repo_workspaces.len(),
        2
    );
    assert_eq!(
        round_tripped.multiplexer_state.cmux_repo_workspaces[0].repo,
        "frontend"
    );
    assert_eq!(
        round_tripped.multiplexer_state.cmux_repo_workspaces[0].workspace,
        "workspace:5"
    );
    assert_eq!(
        round_tripped.multiplexer_state.cmux_repo_workspaces[1].repo,
        "backend"
    );
    assert_eq!(
        round_tripped.multiplexer_state.cmux_repo_workspaces[1].workspace,
        "workspace:6"
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
fn build_repo_entries_rejects_invalid_repo_names_before_loading_config() {
    let tmp = TempDir::new().unwrap();
    let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
    mgr.ensure_dirs().unwrap();
    let runner = MockRunner::new();

    let err =
        build_repo_entries(&mgr, &runner, vec![("../outside".to_string(), None)]).unwrap_err();

    assert!(
        err.to_string().contains("invalid repo name"),
        "unexpected error: {err}"
    );
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
