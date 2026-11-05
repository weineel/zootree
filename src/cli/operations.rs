use crate::cli::remote::{self, Connection};
use crate::config::ConfigManager;
use anyhow::Result;
use clap::{Args, Subcommand};
use std::time::{Duration, Instant};

#[derive(Args)]
pub struct OperationsArgs {
    #[command(subcommand)]
    pub command: OperationsCommands,
}

#[derive(Subcommand)]
pub enum OperationsCommands {
    #[command(about = "Ask the daemon to stop a running operation (best effort)")]
    Cancel {
        #[arg(help = "Operation id, as printed when a command is interrupted (op-...)")]
        id: String,
    },
}

pub fn handle_operations_command(
    command: &OperationsCommands,
    config_mgr: &ConfigManager,
) -> Result<()> {
    match command {
        OperationsCommands::Cancel { id } => handle_cancel(id, config_mgr),
    }
}

fn handle_cancel(id: &str, config_mgr: &ConfigManager) -> Result<()> {
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        anyhow::bail!("invalid operation id {id:?}");
    }
    let client = match remote::discover(&config_mgr.base_dir) {
        Connection::Daemon(client) => client,
        Connection::Local { notice } => {
            if let Some(notice) = notice {
                eprintln!("{notice}");
            }
            anyhow::bail!(
                "no compatible zootree daemon is running; operations only exist inside a daemon (see `zootree daemon status`)"
            );
        }
    };
    let answer = client.cancel_operation(id)?;
    if answer["outcome"] == "already_finished" {
        println!(
            "operation {id} already finished ({})",
            answer["status"].as_str().unwrap_or("?")
        );
        return Ok(());
    }
    println!("cancel requested for operation {id}");
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        let view = client.operation(id)?;
        let status = view["status"].as_str().unwrap_or("running");
        if status != "running" {
            println!("operation {id} {status}");
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    println!("operation {id} is still stopping");
    Ok(())
}
