use zootree::cli::workspace::parse_repos_arg;
use zootree::config::global::GlobalConfig;
use zootree::config::repo::RepoConfig;
use zootree::config::global::HookValue;

#[test]
fn test_parse_global_config_full() {
    let toml_str = r#"
default_layout = "default"
workspace_root = "~/zootree-workspaces"
branch_prefix = "zootree"
copy_files = [".env"]

[hooks]
post_create = "echo hello"

[log]
max_files = 5
"#;
    let config: GlobalConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.default_layout, "default");
    assert_eq!(config.workspace_root, "~/zootree-workspaces");
    assert_eq!(config.branch_prefix, "zootree");
    assert_eq!(config.copy_files, vec![".env"]);
    assert_eq!(config.hooks.post_create, Some(zootree::config::global::HookValue::Simple("echo hello".into())));
    assert_eq!(config.log.max_files, Some(5));
}

#[test]
fn test_parse_global_config_defaults() {
    let toml_str = "";
    let config: GlobalConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.default_layout, "default");
    assert_eq!(config.branch_prefix, "zootree");
    assert!(config.copy_files.is_empty());
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
    assert_eq!(config.copy_files, vec![".env.local", ".vscode/settings.json"]);
    assert_eq!(config.hooks.post_create, Some(HookValue::Simple("npm install".into())));
    assert_eq!(config.hooks.pre_remove, Some(HookValue::File { file: "~/.config/zootree/hooks/cleanup.sh".into() }));
    assert_eq!(config.lazygit.as_ref().unwrap().config, "~/projects/frontend/.lazygit.yml");
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

use zootree::config::workspace::{WorkspaceConfig, WorkspaceStatus, RepoEntry, Event};

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
    assert_eq!(config.repos[0].target_branch, "develop");
    assert_eq!(config.events.len(), 1);
}

#[test]
fn test_parse_template_config() {
    let toml_str = r#"
repos = ["frontend", "backend", "shared-lib"]
layout = "default"
session_mode = "standalone"
"#;
    let config: zootree::config::template::TemplateConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.repos, vec!["frontend", "backend", "shared-lib"]);
    assert_eq!(config.layout, Some("default".into()));
}

#[test]
fn test_parse_repos_arg() {
    let result = parse_repos_arg("frontend:develop,backend,shared-lib:main");
    assert_eq!(result, vec![
        ("frontend".into(), Some("develop".into())),
        ("backend".into(), None),
        ("shared-lib".into(), Some("main".into())),
    ]);
}

#[test]
fn test_parse_repos_arg_single() {
    let result = parse_repos_arg("frontend:develop");
    assert_eq!(result, vec![("frontend".into(), Some("develop".into()))]);
}
