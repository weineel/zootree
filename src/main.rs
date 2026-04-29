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
        Commands::List(args) => {
            zootree::cli::workspace::handle_list(&args)?;
        }
        Commands::Start(args) => {
            zootree::cli::workspace::handle_start(&args)?;
        }
        Commands::Open(args) => {
            zootree::cli::workspace::handle_open(&args)?;
        }
        Commands::Done(args) => {
            zootree::cli::workspace::handle_done(&args)?;
        }
        Commands::Cancel(args) => {
            zootree::cli::workspace::handle_cancel(&args)?;
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
