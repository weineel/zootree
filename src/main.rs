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
        Commands::Template(args) => {
            zootree::cli::template::handle_template_command(&args.command)?;
        }
        Commands::Prune(args) => {
            zootree::cli::prune::handle_prune(&args)?;
        }
        Commands::Logs => {
            let config_dir = dirs::config_dir()
                .ok_or_else(|| anyhow::anyhow!("cannot find config directory"))?
                .join("zootree/logs/zootree.log");
            if config_dir.exists() {
                let status = std::process::Command::new("tail")
                    .args(["-f", "-n", "100"])
                    .arg(&config_dir)
                    .status()?;
                if !status.success() {
                    anyhow::bail!("tail exited with error");
                }
            } else {
                println!("no log file found at {}", config_dir.display());
            }
        }
    }

    Ok(())
}
