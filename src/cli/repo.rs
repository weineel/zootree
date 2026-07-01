use crate::config::global::HooksConfig;
use crate::config::repo::RepoConfig;
use crate::config::ConfigManager;
use crate::core::completers::complete_repo;
use crate::tui;
use anyhow::Result;
use clap::{Args, Subcommand};
use clap_complete::ArgValueCompleter;

#[derive(Args)]
pub struct RepoArgs {
    #[command(subcommand)]
    pub command: RepoCommands,
}

#[derive(Subcommand)]
pub enum RepoCommands {
    #[command(about = "Register a new repository")]
    Add {
        #[arg(long, help = "Custom name for the repo (defaults to directory name)")]
        name: Option<String>,
        #[arg(help = "Path to the git repository")]
        path: String,
        #[arg(long, help = "Default target branch for merging (e.g. main, develop)")]
        default_target_branch: Option<String>,
    },
    #[command(about = "List registered repositories")]
    List,
    #[command(about = "Edit a repository config file")]
    Edit {
        #[arg(
            help = "Name of the repo to edit (interactive if omitted)",
            add = ArgValueCompleter::new(|c: &std::ffi::OsStr| complete_repo(c))
        )]
        name: Option<String>,
    },
    #[command(about = "Unregister a repository", visible_alias = "delete")]
    Remove {
        #[arg(
            help = "Name of the repo to remove (interactive if omitted)",
            add = ArgValueCompleter::new(|c: &std::ffi::OsStr| complete_repo(c))
        )]
        name: Option<String>,
    },
}

pub fn handle_repo_command(cmd: &RepoCommands) -> Result<()> {
    let config_mgr = ConfigManager::new()?;
    config_mgr.ensure_dirs()?;

    match cmd {
        RepoCommands::Add {
            name,
            path,
            default_target_branch,
        } => {
            let expanded = shellexpand::tilde(path).into_owned();
            let abs_path = std::fs::canonicalize(&expanded)
                .unwrap_or_else(|_| std::path::PathBuf::from(&expanded));

            let repo_name = name.clone().unwrap_or_else(|| {
                std::path::Path::new(path)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.clone())
            });

            let repo_config = RepoConfig {
                path: abs_path.to_string_lossy().into_owned(),
                default_target_branch: default_target_branch.clone(),
                copy_files: Vec::new(),
                hooks: HooksConfig::default(),
                lazygit: None,
                zellij: None,
            };
            config_mgr.save_repo_config(&repo_name, &repo_config)?;
            println!("repo '{}' registered at {}", repo_name, abs_path.display());
            Ok(())
        }
        RepoCommands::List => {
            let repos = config_mgr.list_repos()?;
            if repos.is_empty() {
                println!("no repos registered");
            } else {
                for name in &repos {
                    let config = config_mgr.load_repo_config(name)?;
                    println!("  {} -> {}", name, config.path);
                }
            }
            Ok(())
        }
        RepoCommands::Edit { name } => {
            let name = match name {
                Some(n) => n.clone(),
                None => {
                    let repos = config_mgr.list_repos()?;
                    if repos.is_empty() {
                        anyhow::bail!("no repos registered");
                    }
                    let idx = tui::select_one("Select repo to edit", &repos)?;
                    repos[idx].clone()
                }
            };
            let path = config_mgr.repos_dir().join(format!("{}.toml", name));
            let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".into());
            std::process::Command::new(&editor).arg(&path).status()?;
            Ok(())
        }
        RepoCommands::Remove { name } => {
            let name = match name {
                Some(n) => n.clone(),
                None => {
                    let repos = config_mgr.list_repos()?;
                    if repos.is_empty() {
                        anyhow::bail!("no repos registered");
                    }
                    let idx = tui::select_one("Select repo to remove", &repos)?;
                    repos[idx].clone()
                }
            };
            config_mgr.remove_repo_config(&name)?;
            println!("repo '{}' removed", name);
            Ok(())
        }
    }
}
