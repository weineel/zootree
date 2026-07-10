use crate::config::global::HooksConfig;
use crate::config::repo::RepoConfig;
use crate::config::ConfigManager;
use crate::core::completers::complete_repo;
use crate::core::repo_names::unique_repo_name;
use crate::tui;
use anyhow::Result;
use clap::{Args, Subcommand};
use clap_complete::ArgValueCompleter;
use std::path::Path;

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

fn repo_add_base_name(name: &Option<String>, path: &str) -> String {
    name.clone().unwrap_or_else(|| {
        std::path::Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| path.to_string())
    })
}

fn resolve_repo_add_name(
    config_mgr: &ConfigManager,
    name: &Option<String>,
    path: &str,
) -> Result<String> {
    let base = repo_add_base_name(name, path);
    unique_repo_name(config_mgr, &base)
}

fn format_repo_list_entry(name: &str, path: &str) -> String {
    let expanded = shellexpand::tilde(path).into_owned();
    let missing = if Path::new(&expanded).exists() {
        ""
    } else {
        " (missing)"
    };
    format!("  {} -> {}{}", name, path, missing)
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

            let repo_name = resolve_repo_add_name(&config_mgr, name, path)?;

            let repo_config = RepoConfig {
                path: abs_path.to_string_lossy().into_owned(),
                default_target_branch: default_target_branch.clone(),
                copy_files: Vec::new(),
                hooks: HooksConfig::default(),
                lazygit: None,
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
                    println!("{}", format_repo_list_entry(name, &config.path));
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
            let path = config_mgr.repo_config_path(&name)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::global::HooksConfig;

    fn repo_config(path: &str) -> RepoConfig {
        RepoConfig {
            path: path.into(),
            default_target_branch: None,
            copy_files: Vec::new(),
            hooks: HooksConfig::default(),
            lazygit: None,
        }
    }

    #[test]
    fn repo_add_name_from_input_prefers_explicit_name() {
        let name = repo_add_base_name(&Some("custom".into()), "/tmp/zootree");

        assert_eq!(name, "custom");
    }

    #[test]
    fn repo_add_name_from_input_uses_path_basename() {
        let name = repo_add_base_name(&None, "/tmp/zootree");

        assert_eq!(name, "zootree");
    }

    #[test]
    fn repo_add_unique_name_appends_suffix_for_duplicate_base() {
        let tmp = tempfile::tempdir().unwrap();
        let mgr = ConfigManager::with_base_dir(tmp.path().join("config"));
        mgr.ensure_dirs().unwrap();
        mgr.save_repo_config("zootree", &repo_config("/repo/one"))
            .unwrap();

        let name = resolve_repo_add_name(&mgr, &None, "/tmp/zootree").unwrap();

        assert_eq!(name, "zootree-2");
    }

    #[test]
    fn repo_add_unique_name_skips_existing_suffixes() {
        let tmp = tempfile::tempdir().unwrap();
        let mgr = ConfigManager::with_base_dir(tmp.path().join("config"));
        mgr.ensure_dirs().unwrap();
        mgr.save_repo_config("zootree", &repo_config("/repo/one"))
            .unwrap();
        mgr.save_repo_config("zootree-2", &repo_config("/repo/two"))
            .unwrap();

        let name = resolve_repo_add_name(&mgr, &None, "/tmp/zootree").unwrap();

        assert_eq!(name, "zootree-3");
    }

    #[test]
    fn repo_list_entry_keeps_existing_path_unmarked() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().to_string_lossy();

        let line = format_repo_list_entry("zootree", &path);

        assert_eq!(line, format!("  zootree -> {}", path));
    }

    #[test]
    fn repo_list_entry_marks_missing_path() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("missing");
        let path = missing.to_string_lossy();

        let line = format_repo_list_entry("zootree", &path);

        assert_eq!(line, format!("  zootree -> {} (missing)", path));
    }
}
