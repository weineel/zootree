use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::Local;

use crate::config::global::GlobalConfig;
use crate::config::workspace::{Event, RepoEntry, WorkspaceConfig, WorkspaceStatus};
use crate::config::ConfigManager;
use crate::core::copy_files;
use crate::core::git::GitOps;
use crate::core::hook::{
    HookEngine, HookInvocation, HookOperation, HookStage, RepositoryHookContext,
};
use crate::core::terminal_environment::TerminalEnvironment;
use crate::core::workspace_instruction_index;
use crate::runner::CommandRunner;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddRepositoryRequest {
    pub workspace: String,
    pub repo: String,
    pub target_branch: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalUpdate {
    Updated,
    Absent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddRepositoryResult {
    pub repo: String,
    pub workspace: String,
    pub target_branch: String,
    pub workspace_branch: String,
    pub worktree_path: String,
    pub terminal: TerminalUpdate,
}

pub fn add<R: CommandRunner>(
    config_manager: &ConfigManager,
    global_config: &GlobalConfig,
    runner: &R,
    request: &AddRepositoryRequest,
) -> Result<AddRepositoryResult> {
    let (status, mut workspace) = config_manager.load_workspace(&request.workspace)?;
    if status != WorkspaceStatus::InProgress {
        anyhow::bail!("workspace '{}' is not in_progress", request.workspace);
    }
    let original_workspace = workspace.clone();
    if request.repo.is_empty() {
        anyhow::bail!("repository name must not be empty");
    }
    if let Some(existing) = workspace
        .repos
        .iter()
        .find(|repo| repo.name == request.repo)
    {
        anyhow::bail!(
            "repository '{}' is already in workspace '{}' with target branch '{}'",
            request.repo,
            request.workspace,
            existing
                .target_branch
                .as_deref()
                .unwrap_or("(not recorded)")
        );
    }

    let repo_config = config_manager
        .load_repo_config(&request.repo)
        .with_context(|| {
            format!(
                "registered repository '{}' could not be loaded",
                request.repo
            )
        })?;
    let repo_path = shellexpand::tilde(&repo_config.path).into_owned();
    let git = GitOps::new(runner);
    let target_branch = match request
        .target_branch
        .clone()
        .or_else(|| repo_config.default_target_branch.clone())
    {
        Some(branch) if branch.is_empty() => {
            anyhow::bail!(
                "target branch for repository '{}' must not be empty",
                request.repo
            )
        }
        Some(branch) => branch,
        None => git.current_branch(&repo_path).with_context(|| {
            format!(
                "failed to resolve the current branch for repository '{}'",
                request.repo
            )
        })?,
    };
    if !git.branch_exists(&repo_path, &target_branch)? {
        anyhow::bail!(
            "target branch '{}' does not exist locally in repository '{}'",
            target_branch,
            request.repo
        );
    }
    if git.branch_exists(&repo_path, &workspace.branch)? {
        anyhow::bail!(
            "workspace branch '{}' already exists in repository '{}'; refusing to adopt it",
            workspace.branch,
            request.repo
        );
    }

    let workspace_dir = shellexpand::tilde(&workspace.workspace_dir).into_owned();
    let worktree_path = PathBuf::from(&workspace_dir).join(&request.repo);
    match std::fs::symlink_metadata(&worktree_path) {
        Ok(_) => anyhow::bail!(
            "worktree path '{}' already exists; refusing to adopt or replace it",
            worktree_path.display()
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error).with_context(|| {
                format!(
                    "failed to inspect worktree path '{}'",
                    worktree_path.display()
                )
            })
        }
    }
    let worktree_path = worktree_path.to_string_lossy().into_owned();
    let terminal_environment = TerminalEnvironment::new(config_manager, global_config, runner);
    let prepared_terminal = terminal_environment.prepare_repository_addition(
        &workspace,
        &request.repo,
        &worktree_path,
    )?;

    if let Err(error) = git.worktree_add(
        &repo_path,
        &workspace.branch,
        &worktree_path,
        &target_branch,
    ) {
        return Err(rollback_failed_worktree_add(
            error,
            &git,
            &repo_path,
            &worktree_path,
            &workspace.branch,
        ));
    }

    let operation = (|| -> Result<()> {
        let patterns =
            copy_files::merge_copy_files(&global_config.copy_files, &repo_config.copy_files);
        if !patterns.is_empty() {
            copy_files::copy_files_to_worktree(
                Path::new(&repo_path),
                Path::new(&worktree_path),
                &patterns,
            )?;
        }

        if let Some(invocation) = HookInvocation::for_repository(
            repo_config.hooks.post_create.as_ref(),
            global_config.hooks.post_create.as_ref(),
            HookStage::PostCreate,
            HookOperation::AddRepo,
            WorkspaceStatus::InProgress,
            &workspace,
            RepositoryHookContext {
                name: &request.repo,
                source_dir: &repo_path,
                worktree_path: &worktree_path,
                target_branch: Some(&target_branch),
            },
        ) {
            HookEngine::new(runner).execute(&invocation)?;
        }

        Ok(())
    })();

    if let Err(error) = operation {
        return Err(rollback_error(
            error,
            &git,
            GitRollbackTarget {
                repo_path: &repo_path,
                worktree_path: &worktree_path,
                workspace_branch: &workspace.branch,
                worktree_created: true,
                branch_created: true,
            },
            Vec::new(),
        ));
    }

    workspace.repos.push(RepoEntry {
        name: request.repo.clone(),
        target_branch: Some(target_branch.clone()),
    });
    workspace.events.push(Event {
        action: "repo_added".into(),
        timestamp: Local::now().to_rfc3339(),
        detail: Some(format!(
            "repo={}, target_branch={}",
            request.repo, target_branch
        )),
    });
    if let Err(error) =
        config_manager.save_workspace_atomic(&WorkspaceStatus::InProgress, &workspace)
    {
        return Err(rollback_error(
            error.context("failed to persist added repository"),
            &git,
            GitRollbackTarget {
                repo_path: &repo_path,
                worktree_path: &worktree_path,
                workspace_branch: &workspace.branch,
                worktree_created: true,
                branch_created: true,
            },
            Vec::new(),
        ));
    }
    workspace_instruction_index::sync(&workspace);

    let applied_terminal = match terminal_environment.apply_repository_addition(prepared_terminal) {
        Ok(applied) => applied,
        Err(error) => {
            let residues = restore_workspace_after_failed_add(config_manager, &original_workspace);
            return Err(rollback_error(
                error,
                &git,
                GitRollbackTarget {
                    repo_path: &repo_path,
                    worktree_path: &worktree_path,
                    workspace_branch: &workspace.branch,
                    worktree_created: true,
                    branch_created: true,
                },
                residues,
            ));
        }
    };

    let terminal_state_changed = workspace.multiplexer_state != *applied_terminal.stored_state();
    workspace.multiplexer_state = applied_terminal.stored_state().clone();
    if terminal_state_changed {
        if let Err(error) =
            config_manager.save_workspace_atomic(&WorkspaceStatus::InProgress, &workspace)
        {
            let mut residues = Vec::new();
            if let Err(error) = terminal_environment.rollback_repository_addition(&applied_terminal)
            {
                residues.push(format!("terminal unit cleanup failed: {error:#}"));
            }
            residues.extend(restore_workspace_after_failed_add(
                config_manager,
                &original_workspace,
            ));
            return Err(rollback_error(
                error.context("failed to persist terminal state for added repository"),
                &git,
                GitRollbackTarget {
                    repo_path: &repo_path,
                    worktree_path: &worktree_path,
                    workspace_branch: &workspace.branch,
                    worktree_created: true,
                    branch_created: true,
                },
                residues,
            ));
        }
    }

    Ok(AddRepositoryResult {
        repo: request.repo.clone(),
        workspace: workspace.name,
        target_branch,
        workspace_branch: workspace.branch,
        worktree_path,
        terminal: if applied_terminal.was_updated() {
            TerminalUpdate::Updated
        } else {
            TerminalUpdate::Absent
        },
    })
}

fn restore_workspace_after_failed_add(
    config_manager: &ConfigManager,
    original_workspace: &WorkspaceConfig,
) -> Vec<String> {
    match config_manager.save_workspace_atomic(&WorkspaceStatus::InProgress, original_workspace) {
        Ok(()) => {
            workspace_instruction_index::sync(original_workspace);
            Vec::new()
        }
        Err(error) => vec![format!("workspace config rollback failed: {error:#}")],
    }
}

struct GitRollbackTarget<'a> {
    repo_path: &'a str,
    worktree_path: &'a str,
    workspace_branch: &'a str,
    worktree_created: bool,
    branch_created: bool,
}

fn rollback_failed_worktree_add<R: CommandRunner>(
    primary: anyhow::Error,
    git: &GitOps<'_, R>,
    repo_path: &str,
    worktree_path: &str,
    workspace_branch: &str,
) -> anyhow::Error {
    let mut residues = Vec::new();
    let worktree_path_created = match std::fs::symlink_metadata(worktree_path) {
        Ok(_) => true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => {
            residues.push(format!(
                "failed to inspect worktree path '{worktree_path}' after git worktree add failed: {error}"
            ));
            false
        }
    };
    let registered_worktree_created = if worktree_path_created {
        false
    } else {
        match git.worktree_registered_for_branch(repo_path, worktree_path, workspace_branch) {
            Ok(created) => created,
            Err(error) => {
                residues.push(format!(
                    "failed to inspect Git worktree registration for '{worktree_path}' after git worktree add failed: {error:#}"
                ));
                false
            }
        }
    };
    let branch_created = match git.branch_exists(repo_path, workspace_branch) {
        Ok(created) => created,
        Err(error) => {
            residues.push(format!(
                "failed to inspect workspace branch '{workspace_branch}' after git worktree add failed: {error:#}"
            ));
            false
        }
    };

    rollback_error(
        primary,
        git,
        GitRollbackTarget {
            repo_path,
            worktree_path,
            workspace_branch,
            worktree_created: worktree_path_created || registered_worktree_created,
            branch_created,
        },
        residues,
    )
}

fn rollback_error<R: CommandRunner>(
    primary: anyhow::Error,
    git: &GitOps<'_, R>,
    git_target: GitRollbackTarget<'_>,
    mut residues: Vec<String>,
) -> anyhow::Error {
    if git_target.worktree_created {
        match git.worktree_remove(git_target.repo_path, git_target.worktree_path, true) {
            Ok(()) => {
                if git_target.branch_created {
                    if let Err(error) = git.delete_local_branch(
                        git_target.repo_path,
                        git_target.workspace_branch,
                        true,
                    ) {
                        residues.push(format!("workspace branch cleanup failed: {error:#}"));
                    }
                }
            }
            Err(error) => {
                let retained = if git_target.branch_created {
                    format!(
                        "; workspace branch '{}' was retained",
                        git_target.workspace_branch
                    )
                } else {
                    String::new()
                };
                residues.push(format!("worktree cleanup failed: {error:#}{retained}"));
            }
        }
    } else if git_target.branch_created {
        if let Err(error) =
            git.delete_local_branch(git_target.repo_path, git_target.workspace_branch, true)
        {
            residues.push(format!("workspace branch cleanup failed: {error:#}"));
        }
    }

    if residues.is_empty() {
        primary
    } else {
        anyhow::anyhow!("{primary:#}; rollback residue: {}", residues.join("; "))
    }
}
