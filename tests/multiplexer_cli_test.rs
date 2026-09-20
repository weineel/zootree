use clap::{CommandFactory, Parser};
use zootree::cli::Cli;

#[test]
fn lifecycle_commands_accept_backend_selection_and_deferred_activation() {
    for command in ["create", "start", "reopen"] {
        for kind in ["zellij", "cmux", "herdr"] {
            let mut args = vec!["zootree", command, "--multiplexer", kind];
            if command != "create" {
                args.push("--no-multiplexer");
            }
            assert!(Cli::try_parse_from(&args).is_ok(), "{args:?}");
        }
        assert!(Cli::try_parse_from(["zootree", command, "--multiplexer", "invalid"]).is_err());
    }
    assert!(Cli::try_parse_from(["zootree", "open", "example", "--multiplexer", "cmux"]).is_err());
}

#[test]
fn backend_selection_exposes_fixed_values_for_help_and_completion() {
    let command = Cli::command();
    for name in ["create", "start", "reopen"] {
        let subcommand = command.find_subcommand(name).unwrap();
        let arg = subcommand
            .get_arguments()
            .find(|arg| arg.get_id() == "multiplexer")
            .unwrap();
        let values: Vec<_> = arg
            .get_value_parser()
            .possible_values()
            .unwrap()
            .map(|value| value.get_name().to_owned())
            .collect();
        assert_eq!(values, ["zellij", "cmux", "herdr"]);
    }
}

#[test]
fn create_persists_global_settings_and_cli_choice_without_editing_global_or_template() {
    use std::process::Command;
    use tempfile::TempDir;
    use zootree::config::{
        global::{GlobalConfig, MultiplexerConfig, MultiplexerKind},
        repo::RepoConfig,
        template::TemplateConfig,
        workspace::WorkspaceStatus,
        ConfigManager,
    };

    let home = TempDir::new().unwrap();
    let manager = ConfigManager::with_base_dir(home.path().join(".config/zootree"));
    manager.ensure_dirs().unwrap();
    let mut global = GlobalConfig {
        workspace_root: home
            .path()
            .join("workspaces")
            .to_string_lossy()
            .into_owned(),
        ..Default::default()
    };
    global.multiplexer.kind = MultiplexerKind::Herdr;
    global.multiplexer.herdr.session = "team".into();
    global.multiplexer.zellij.layout = Some("personal".into());
    manager.save_global_config(&global).unwrap();
    let repo_path = home.path().join("repo");
    std::fs::create_dir(&repo_path).unwrap();
    manager
        .save_repo_config(
            "api",
            &RepoConfig {
                path: repo_path.to_string_lossy().into_owned(),
                default_target_branch: Some("main".into()),
                copy_files: Vec::new(),
                hooks: Default::default(),
                lazygit: None,
            },
        )
        .unwrap();
    let template = TemplateConfig {
        repos: vec!["api".into()],
        multiplexer: MultiplexerConfig::default(),
    };
    manager.save_template("legacy", &template).unwrap();
    let original_global = std::fs::read(manager.global_config_path()).unwrap();

    for (name, override_kind, expected_kind) in [
        ("default-choice", None, MultiplexerKind::Herdr),
        ("explicit-choice", Some("cmux"), MultiplexerKind::Cmux),
    ] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_zootree"));
        command.env("HOME", home.path()).args([
            "create",
            "--name",
            name,
            "--branch",
            name,
            "--title",
            "feat(workspace): test backend choice",
            "--template",
            "legacy",
        ]);
        if let Some(kind) = override_kind {
            command.args(["--multiplexer", kind]);
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let (status, workspace) = manager.load_workspace(name).unwrap();
        assert_eq!(status, WorkspaceStatus::Pending);
        assert_eq!(workspace.multiplexer.kind, expected_kind);
        assert_eq!(workspace.multiplexer.herdr.session, "team");
        assert_eq!(
            workspace.multiplexer.zellij.layout.as_deref(),
            Some("personal")
        );
    }
    assert_eq!(
        std::fs::read(manager.global_config_path()).unwrap(),
        original_global
    );
    assert_eq!(
        manager.load_template("legacy").unwrap().multiplexer,
        template.multiplexer
    );
}
