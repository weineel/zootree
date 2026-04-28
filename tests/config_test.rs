use zootree::config::global::GlobalConfig;

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
