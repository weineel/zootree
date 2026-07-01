use clap::Parser;
use zootree::cli::repo::RepoCommands;
use zootree::cli::{Cli, Commands};

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
