use crate::config::template::TemplateConfig;
use crate::config::ConfigManager;
use anyhow::Result;
use clap::{Args, Subcommand};

#[derive(Args)]
pub struct TemplateArgs {
    #[command(subcommand)]
    pub command: TemplateCommands,
}

#[derive(Subcommand)]
pub enum TemplateCommands {
    #[command(about = "List saved templates")]
    List,
    #[command(about = "Save a workspace as a template")]
    Save {
        #[arg(help = "Name for the new template")]
        name: String,
        #[arg(long, help = "Workspace name to save as template")]
        from: String,
    },
}

pub fn handle_template_command(cmd: &TemplateCommands) -> Result<()> {
    let config_mgr = ConfigManager::new()?;
    config_mgr.ensure_dirs()?;

    match cmd {
        TemplateCommands::List => {
            let templates = config_mgr.list_templates()?;
            if templates.is_empty() {
                println!("no templates found");
            } else {
                for name in &templates {
                    let tmpl = config_mgr.load_template(name)?;
                    println!("  {} — repos: {}", name, tmpl.repos.join(", "));
                }
            }
            Ok(())
        }
        TemplateCommands::Save { name, from } => {
            let (_, workspace) = config_mgr.load_workspace(from)?;
            let tmpl = TemplateConfig {
                repos: workspace.repos.iter().map(|r| r.name.clone()).collect(),
                zellij: workspace.zellij.clone(),
            };
            config_mgr.save_template(name, &tmpl)?;
            println!("template '{}' saved from workspace '{}'", name, from);
            Ok(())
        }
    }
}
