use std::ffi::OsStr;

use anyhow::Result;
use clap::Args;
use clap_complete::{ArgValueCompleter, CompletionCandidate};

use crate::config::workspace::{WorkspaceConfig, WorkspaceStatus};
use crate::config::ConfigManager;
use crate::core::completers::{complete_single_repo_spec, complete_workspace, WorkspaceFilter};
use crate::core::workspace_repository::{
    add, AddRepositoryRequest, AddRepositoryResult, TerminalUpdate,
};
use crate::runner::RealRunner;
use crate::tui;

#[derive(Args)]
pub struct AddRepoArgs {
    #[arg(
        help = "In-progress workspace name (interactive if omitted)",
        add = ArgValueCompleter::new(|current: &OsStr| complete_workspace(current, WorkspaceFilter::InProgress))
    )]
    pub workspace: Option<String>,
    #[arg(
        long,
        value_name = "REPO[:TARGET_BRANCH]",
        help = "Registered repository and optional target branch (interactive if omitted)",
        add = ArgValueCompleter::new(|current: &OsStr| complete_add_repo(current))
    )]
    pub repo: Option<String>,
}

pub fn handle_add_repo(args: &AddRepoArgs) -> Result<()> {
    let config_manager = ConfigManager::new()?;
    let global_config = config_manager.load_global_config()?;
    let workspace_name = select_workspace(&config_manager, args.workspace.as_deref())?;
    let (status, workspace) = config_manager.load_workspace(&workspace_name)?;
    if status != WorkspaceStatus::InProgress {
        anyhow::bail!("workspace '{}' is not in_progress", workspace_name);
    }
    let (repo, target_branch) = match args.repo.as_deref() {
        Some(repo) => parse_repo_spec(repo)?,
        None => (select_repository(&config_manager, &workspace)?, None),
    };
    let result = add(
        &config_manager,
        &global_config,
        &RealRunner,
        &AddRepositoryRequest {
            workspace: workspace_name,
            repo,
            target_branch,
        },
    )?;
    print_result(&result);
    Ok(())
}

fn select_workspace(config_manager: &ConfigManager, selected: Option<&str>) -> Result<String> {
    if let Some(selected) = selected {
        return Ok(selected.into());
    }
    let workspaces = config_manager.list_workspaces(Some(&[WorkspaceStatus::InProgress]))?;
    if workspaces.is_empty() {
        anyhow::bail!("no in_progress workspaces");
    }
    let items = workspaces
        .iter()
        .map(|workspace| format!("{} - {}", workspace.name, workspace.title))
        .collect::<Vec<_>>();
    let index = tui::select_one("Select workspace to add a repository to", &items)?;
    Ok(workspaces[index].name.clone())
}

fn select_repository(
    config_manager: &ConfigManager,
    workspace: &WorkspaceConfig,
) -> Result<String> {
    let repos = eligible_repository_names(config_manager, workspace)?;
    if repos.is_empty() {
        anyhow::bail!(
            "no registered repositories are available to add to workspace '{}'",
            workspace.name
        );
    }
    let items = repos
        .iter()
        .map(|name| {
            config_manager
                .load_repo_config(name)
                .map(|config| format!("{name} - {}", config.path))
                .unwrap_or_else(|_| name.clone())
        })
        .collect::<Vec<_>>();
    let index = tui::select_one("Select repository to add", &items)?;
    Ok(repos[index].clone())
}

fn eligible_repository_names(
    config_manager: &ConfigManager,
    workspace: &WorkspaceConfig,
) -> Result<Vec<String>> {
    let existing = workspace
        .repos
        .iter()
        .map(|repo| repo.name.as_str())
        .collect::<std::collections::HashSet<_>>();
    Ok(config_manager
        .list_repos()?
        .into_iter()
        .filter(|repo| !existing.contains(repo.as_str()))
        .collect())
}

fn parse_repo_spec(spec: &str) -> Result<(String, Option<String>)> {
    let (repo, target_branch) = match spec.split_once(':') {
        Some((repo, target_branch)) => (repo, Some(target_branch)),
        None => (spec, None),
    };
    if repo.is_empty() {
        anyhow::bail!("repository name must not be empty");
    }
    if repo.contains(',') {
        anyhow::bail!("--repo accepts exactly one registered repository");
    }
    if target_branch.is_some_and(str::is_empty) {
        anyhow::bail!("target branch must not be empty");
    }
    Ok((repo.into(), target_branch.map(str::to_string)))
}

fn complete_add_repo(current: &OsStr) -> Vec<CompletionCandidate> {
    complete_single_repo_spec(current)
}

fn print_result(result: &AddRepositoryResult) {
    println!(
        "repository '{}' added to workspace '{}'",
        result.repo, result.workspace
    );
    println!("  target branch: {}", result.target_branch);
    println!("  workspace branch: {}", result.workspace_branch);
    println!("  worktree: {}", result.worktree_path);
    println!(
        "  terminal: {}",
        match result.terminal {
            TerminalUpdate::Updated => "updated existing environment",
            TerminalUpdate::Absent => "environment absent; skipped",
        }
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::global::{HooksConfig, MultiplexerConfig};
    use crate::config::repo::RepoConfig;
    use crate::config::workspace::RepoEntry;
    use tempfile::TempDir;

    #[test]
    fn repo_spec_accepts_one_optional_target_branch() {
        assert_eq!(
            parse_repo_spec("backend:release/2026").unwrap(),
            ("backend".into(), Some("release/2026".into()))
        );
        assert_eq!(
            parse_repo_spec("backend").unwrap(),
            ("backend".into(), None)
        );
        assert!(parse_repo_spec("backend:").is_err());
        assert!(parse_repo_spec("backend,frontend").is_err());
    }

    #[test]
    fn interactive_repo_candidates_exclude_existing_memberships() {
        let temp = TempDir::new().unwrap();
        let config_manager = ConfigManager::with_base_dir(temp.path().to_path_buf());
        config_manager.ensure_dirs().unwrap();
        for name in ["frontend", "backend"] {
            config_manager
                .save_repo_config(
                    name,
                    &RepoConfig {
                        path: format!("/repos/{name}"),
                        default_target_branch: Some("main".into()),
                        copy_files: Vec::new(),
                        hooks: HooksConfig::default(),
                        lazygit: None,
                    },
                )
                .unwrap();
        }
        let workspace = WorkspaceConfig {
            title: "Test".into(),
            name: "calm-river".into(),
            description: String::new(),
            branch: "zootree/calm-river".into(),
            workspace_dir: "/tmp/calm-river".into(),
            created_at: "2026-08-25T10:00:00+08:00".into(),
            agent_cli: None,
            multiplexer: MultiplexerConfig::default(),
            multiplexer_state: Default::default(),
            repos: vec![RepoEntry {
                name: "frontend".into(),
                target_branch: Some("main".into()),
            }],
            events: Vec::new(),
        };

        assert_eq!(
            eligible_repository_names(&config_manager, &workspace).unwrap(),
            vec!["backend"]
        );
    }
}
