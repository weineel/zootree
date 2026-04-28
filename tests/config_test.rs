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
