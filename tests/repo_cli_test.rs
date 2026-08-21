use clap::Parser;
use std::process::Command;
use tempfile::TempDir;
use zootree::cli::repo::RepoCommands;
use zootree::cli::{Cli, Commands};
use zootree::config::global::HooksConfig;
use zootree::config::repo::RepoConfig;
use zootree::config::ConfigManager;

#[test]
fn repo_delete_alias_parses_as_remove() {
    let cli = Cli::try_parse_from(["zootree", "repo", "delete", "frontend"]).unwrap();

    let Commands::Repo(args) = cli.command else {
        panic!("expected repo command");
    };
    let RepoCommands::Remove { name } = args.command else {
        panic!("expected delete alias to parse as remove");
    };

    assert_eq!(name.as_deref(), Some("frontend"));
}

#[test]
fn repo_edit_uses_visual_before_editor() {
    let home = TempDir::new().unwrap();
    let manager = ConfigManager::with_base_dir(home.path().join(".config/zootree"));
    manager.ensure_dirs().unwrap();
    manager
        .save_repo_config(
            "frontend",
            &RepoConfig {
                path: "/repo/frontend".into(),
                default_target_branch: None,
                copy_files: Vec::new(),
                hooks: HooksConfig::default(),
                lazygit: None,
            },
        )
        .unwrap();
    let repo_config_path = manager.repo_config_path("frontend").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_zootree"))
        .env("HOME", home.path())
        .env("VISUAL", "/bin/rm")
        .env("EDITOR", "/usr/bin/false")
        .args(["repo", "edit", "frontend"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!repo_config_path.exists());
}
