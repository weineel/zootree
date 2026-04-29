use anyhow::Result;
use clap::Parser;
use zootree::cli::{Cli, Commands};

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Repo(_args) => {
            println!("repo command");
        }
        Commands::Create(_args) => {
            println!("create workspace");
        }
        Commands::List(_args) => {
            println!("list workspaces");
        }
        Commands::Start(_args) => {
            println!("start workspace");
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
