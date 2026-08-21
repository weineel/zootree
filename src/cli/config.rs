use crate::config::global::GlobalConfig;
use crate::config::ConfigManager;
use crate::core::agent_cli::{AgentCatalog, AgentDefaultKind};
use crate::core::editor;
use anyhow::Result;
use clap::{Args, Subcommand};
use std::fmt::Write as FmtWrite;
use std::io::Write as IoWrite;

#[derive(Args)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub command: ConfigCommands,
}

#[derive(Subcommand)]
pub enum ConfigCommands {
    #[command(about = "Show the global config file path")]
    Path,
    #[command(about = "Show the global config file contents")]
    Show,
    #[command(about = "Edit the global config file")]
    Edit,
    #[command(about = "List configured coding agents")]
    Agents {
        #[arg(long, help = "Print the agent catalog as JSON")]
        json: bool,
    },
}

pub fn handle_bootstrap_command(
    command: &ConfigCommands,
    config_mgr: &ConfigManager,
) -> Option<Result<()>> {
    match command {
        ConfigCommands::Path => Some(handle_config_path(config_mgr)),
        ConfigCommands::Show => Some(handle_config_show(config_mgr)),
        ConfigCommands::Edit => Some(handle_config_edit(config_mgr)),
        ConfigCommands::Agents { .. } => None,
    }
}

fn handle_config_path(config_mgr: &ConfigManager) -> Result<()> {
    println!("{}", config_mgr.global_config_path().display());
    Ok(())
}

fn handle_config_show(config_mgr: &ConfigManager) -> Result<()> {
    let path = config_mgr.global_config_path();
    let Some(content) = config_mgr.read_global_config_source()? else {
        anyhow::bail!(
            "global config file not found: {}\nrun `zootree config edit` to create it",
            path.display()
        );
    };
    std::io::stdout().write_all(&content)?;
    Ok(())
}

fn handle_config_edit(config_mgr: &ConfigManager) -> Result<()> {
    let path = config_mgr.ensure_global_config_file()?;
    editor::open_file(&path)?;
    config_mgr.parse_global_config_file()?;
    Ok(())
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
        _ => anyhow::bail!("config recovery command must run during bootstrap"),
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

    for alias in &catalog.aliases {
        let marker = if alias.is_default { " (default)" } else { "" };
        let _ = writeln!(output, "  {}{} -> {}", alias.name, marker, alias.command);
    }

    output
}
