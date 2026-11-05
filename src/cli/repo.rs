use crate::cli::workspace::CliEventSink;
use crate::config::ConfigManager;
use crate::core::completers::complete_repo;
use crate::core::editor;
use crate::service::repos::{register_repo, remove_repo, RegisterRepoRequest, RemoveRepoRequest};
use crate::service::{EnvSnapshot, RequestContext};
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
    List {
        #[arg(long, help = "Print machine-readable JSON")]
        json: bool,
    },
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
    let sink = CliEventSink;
    let ctx = RequestContext::new(&sink, EnvSnapshot::capture());

    match cmd {
        RepoCommands::Add {
            name,
            path,
            default_target_branch,
        } => {
            register_repo(
                &config_mgr,
                &ctx,
                &RegisterRepoRequest {
                    name: name.clone(),
                    path: path.clone(),
                    default_target_branch: default_target_branch.clone(),
                },
            )?;
            Ok(())
        }
        RepoCommands::List { json: true } => {
            let mut out = Vec::new();
            for name in config_mgr.list_repos()? {
                let config = config_mgr.load_repo_config(&name)?;
                out.push(crate::cli::json_view::repo_entry(&name, &config));
            }
            println!("{}", crate::cli::json_view::to_pretty_json(&out)?);
            Ok(())
        }
        RepoCommands::List { json: false } => {
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
            editor::open_file(&path)?;
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
            remove_repo(&config_mgr, &ctx, &RemoveRepoRequest { name })?;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::global::HooksConfig;
    use crate::config::repo::RepoConfig;
    use crate::service::repos::{repo_add_base_name, resolve_repo_add_name};

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
