use crate::config::global::GlobalConfig;
use crate::core::agent_cli::{AgentCatalog, AgentDefaultKind};
use anyhow::Result;
use clap::{Args, Subcommand};
use std::fmt::Write;

#[derive(Args)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub command: ConfigCommands,
}

#[derive(Subcommand)]
pub enum ConfigCommands {
    #[command(about = "List configured coding agents")]
    Agents {
        #[arg(long, help = "Print the agent catalog as JSON")]
        json: bool,
    },
}

pub fn handle_config_command(command: &ConfigCommands, global: &GlobalConfig) -> Result<()> {
    let catalog = AgentCatalog::from_global(global);
    match command {
        ConfigCommands::Agents { json: true } => {
            println!("{}", serde_json::to_string_pretty(&catalog)?);
            Ok(())
        }
        ConfigCommands::Agents { json: false } => {
            print!("{}", format_agent_catalog(&catalog));
            Ok(())
        }
    }
}

fn format_agent_catalog(catalog: &AgentCatalog) -> String {
    let mut output = String::new();
    match &catalog.default {
        Some(default) => {
            let kind = match default.kind {
                AgentDefaultKind::Alias => "alias",
                AgentDefaultKind::Literal => "literal",
            };
            let _ = writeln!(output, "Default: {} ({})", default.value, kind);
        }
        None => {
            let _ = writeln!(output, "Default: not configured");
        }
    }

    if catalog.aliases.is_empty() {
        let _ = writeln!(output, "Agents: none configured");
    } else {
        let _ = writeln!(output, "Agents:");
        for alias in &catalog.aliases {
            let marker = if alias.is_default { " (default)" } else { "" };
            let _ = writeln!(output, "  {}{} -> {}", alias.name, marker, alias.command);
        }
    }

    output
}
