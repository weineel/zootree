use std::collections::BTreeMap;

use zootree::cli::json_view::{repo_entry, workspace_detail, workspace_summary};
use zootree::config::global::{GlobalConfig, MultiplexerConfig};
use zootree::config::repo::RepoConfig;
use zootree::config::workspace::{Event, RepoEntry, WorkspaceConfig, WorkspaceStatus};
use zootree::core::layout::build_agent_cli_argv;

fn ws() -> WorkspaceConfig {
    WorkspaceConfig {
        title: "feat: demo".into(),
        name: "demo".into(),
        description: "line one\nline two".into(),
        branch: "zootree/demo".into(),
        workspace_dir: "/tmp/demo".into(),
        created_at: "2026-05-10T14:22:00+08:00".into(),
        agent_cli: None,
        multiplexer: MultiplexerConfig::default(),
        multiplexer_state: Default::default(),
        repos: vec![RepoEntry {
            name: "api".into(),
            target_branch: Some("main".into()),
        }],
        events: vec![Event {
            action: "created".into(),
            timestamp: "2026-05-10T14:22:00+08:00".into(),
            detail: None,
        }],
    }
}

fn global_with_alias() -> GlobalConfig {
    let mut aliases = BTreeMap::new();
    aliases.insert("claude".to_string(), "claude -- $prompt".to_string());
    GlobalConfig {
        agent_cli: Some("claude".into()),
        agent_cli_alias: aliases,
        ..GlobalConfig::default()
    }
}

#[test]
fn summary_has_stable_fields() {
    let v = serde_json::to_value(workspace_summary(&WorkspaceStatus::InProgress, &ws())).unwrap();
    assert_eq!(v["name"], "demo");
    assert_eq!(v["status"], "in_progress");
    assert_eq!(v["workspace_dir"], "/tmp/demo");
    assert_eq!(v["repos"][0]["name"], "api");
    assert_eq!(v["repos"][0]["target_branch"], "main");
    assert!(v.get("description").is_none());
}

#[test]
fn detail_resolves_alias_into_argv_with_prompt() {
    let v = serde_json::to_value(workspace_detail(
        &WorkspaceStatus::Pending,
        &ws(),
        &global_with_alias(),
    ))
    .unwrap();
    assert_eq!(v["description"], "line one\nline two");
    assert_eq!(v["events"][0]["action"], "created");
    assert_eq!(
        v["agent_command"],
        serde_json::json!(["claude", "--", "feat: demo\nline one\nline two"])
    );
    assert!(v["agent_command_error"].is_null());
}

#[test]
fn detail_prefers_workspace_agent_over_global() {
    let mut w = ws();
    w.agent_cli = Some("codex $prompt".into());
    let v = serde_json::to_value(workspace_detail(
        &WorkspaceStatus::Pending,
        &w,
        &global_with_alias(),
    ))
    .unwrap();
    assert_eq!(v["agent_command"][0], "codex");
}

#[test]
fn detail_without_agent_reports_null() {
    let v = serde_json::to_value(workspace_detail(
        &WorkspaceStatus::Pending,
        &ws(),
        &GlobalConfig::default(),
    ))
    .unwrap();
    assert!(v["agent_command"].is_null());
    assert!(v["agent_command_error"].is_null());
}

#[test]
fn detail_reports_unparsable_template() {
    let mut w = ws();
    w.agent_cli = Some("claude 'unterminated".into());
    let v = serde_json::to_value(workspace_detail(
        &WorkspaceStatus::Pending,
        &w,
        &GlobalConfig::default(),
    ))
    .unwrap();
    assert!(v["agent_command"].is_null());
    assert!(v["agent_command_error"].as_str().unwrap().contains("parse"));
}

#[test]
fn argv_keeps_prompt_as_one_token() {
    let argv = build_agent_cli_argv("a $prompt b", "x y\nz").unwrap();
    assert_eq!(argv, vec!["a", "x y\nz", "b"]);
}

#[test]
fn repo_json_has_name_path_and_default_branch() {
    let cfg = RepoConfig {
        path: "/src/api".into(),
        default_target_branch: Some("main".into()),
        copy_files: vec![],
        hooks: Default::default(),
        lazygit: None,
    };
    let v = serde_json::to_value(repo_entry("api", &cfg)).unwrap();
    assert_eq!(
        v,
        serde_json::json!({"name":"api","path":"/src/api","default_target_branch":"main"})
    );
}
