use anyhow::Result;
use clap::Parser;
use zootree::cli::{Cli, Commands};

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Repo(args) => {
            zootree::cli::repo::handle_repo_command(&args.command)?;
        }
        Commands::Create(args) => {
            zootree::cli::workspace::handle_create(&args)?;
        }
        Commands::List(_args) => {
            println!("list workspaces");
        }
        Commands::Start(args) => {
            zootree::cli::workspace::handle_start(&args)?;
        }
        Commands::Open(_args) => {
            println!("open workspace");
        }
        Commands::Done(_args) => {
            println!("done workspace");
        }
        Commands::Cancel(_args) => {
            println!("cancel workspace");
        }
        Commands::Template(_args) => {
            println!("template command");
        }
        Commands::Prune(_args) => {
            println!("prune");
        }
        Commands::Logs => {
            println!("logs");
        }
    }

    Ok(())
}
