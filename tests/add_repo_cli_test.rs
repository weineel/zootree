use clap::Parser;
use zootree::cli::{Cli, Commands};

#[test]
fn add_repo_cli_parses_workspace_and_repo_target() {
    let cli = Cli::try_parse_from([
        "zootree",
        "add-repo",
        "calm-river",
        "--repo",
        "backend:release/2026",
    ])
    .unwrap();

    let Commands::AddRepo(args) = cli.command else {
        panic!("expected add-repo command");
    };
    assert_eq!(args.workspace.as_deref(), Some("calm-river"));
    assert_eq!(args.repo.as_deref(), Some("backend:release/2026"));
}

#[test]
fn add_repo_cli_allows_interactive_arguments_to_be_omitted() {
    let cli = Cli::try_parse_from(["zootree", "add-repo"]).unwrap();

    let Commands::AddRepo(args) = cli.command else {
        panic!("expected add-repo command");
    };
    assert!(args.workspace.is_none());
    assert!(args.repo.is_none());
}
