use crate::config::global::GlobalConfig;
use crate::config::name::validate_config_name;
use crate::config::workspace::Event;
use crate::config::workspace::{WorkspaceConfig, WorkspaceStatus};
use crate::config::ConfigManager;
use crate::core::copy_files;
use crate::core::git::GitOps;
use crate::core::hook::{HookContext, HookEngine};
use crate::runner::CommandRunner;
use anyhow::{bail, Context, Result};
use chrono::Local;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReopenBase {
    Current,
    Branch(String),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReopenSources {
    current_default: bool,
    per_repo: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReopenOptions {
    pub sources: ReopenSources,
    pub overwrite_repos: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskBranchSource {
    Local,
    Remote(String),
    Base { revision: String, display: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorktreeAction {
    Reuse,
    Create,
    Overwrite { registered: bool },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoReopenPlan {
    pub repo_name: String,
    pub repo_path: String,
    pub worktree_path: String,
    pub target_branch: Option<String>,
    pub branch_source: TaskBranchSource,
    pub worktree_action: WorktreeAction,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReopenPlan {
    pub from_status: WorkspaceStatus,
    pub workspace: WorkspaceConfig,
    pub repos: Vec<RepoReopenPlan>,
    archived_workspace: WorkspaceConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReopenLifecyclePlan {
    pub skip_hooks: bool,
    pub activate_terminal_environment: bool,
    pub run_agent: bool,
}

impl Default for ReopenLifecyclePlan {
    fn default() -> Self {
        Self {
            skip_hooks: false,
            activate_terminal_environment: true,
            run_agent: false,
        }
    }
}

impl ReopenPlan {
    pub fn apply_current_terminal_config(&mut self, global: &GlobalConfig) {
        self.workspace.multiplexer = global.multiplexer.clone();
        self.workspace.multiplexer_state = Default::default();
    }
}

pub trait ReopenPrompt {
    fn is_interactive(&self) -> bool;
    fn choose_remote(&mut self, repo: &str, branches: &[String]) -> Result<String>;
    fn choose_base(&mut self, repo: &str, current: &str) -> Result<ReopenBase>;
    fn confirm_overwrite(
        &mut self,
        repo: &str,
        path: &str,
        valid_worktree: bool,
        dirty: bool,
    ) -> Result<bool>;
}

pub struct NonInteractiveReopenPrompt;

impl ReopenPrompt for NonInteractiveReopenPrompt {
    fn is_interactive(&self) -> bool {
        false
    }

    fn choose_remote(&mut self, repo: &str, _branches: &[String]) -> Result<String> {
        bail!("repo '{}' requires an explicit remote branch", repo)
    }

    fn choose_base(&mut self, repo: &str, _current: &str) -> Result<ReopenBase> {
        bail!("repo '{}' requires an explicit --from source", repo)
    }

    fn confirm_overwrite(
        &mut self,
        repo: &str,
        path: &str,
        _valid_worktree: bool,
        _dirty: bool,
    ) -> Result<bool> {
        bail!(
            "repo '{}' requires --overwrite before replacing '{}'",
            repo,
            path
        )
    }
}

impl ReopenSources {
    pub fn parse(values: &[String]) -> Result<Self> {
        let mut sources = Self::default();
        for value in values {
            if value == "current" {
                sources.current_default = true;
                continue;
            }

            let Some((repo, branch)) = value.split_once(':') else {
                bail!(
                    "invalid --from '{}': expected 'current' or REPO:BRANCH",
                    value
                );
            };
            validate_config_name("repo", repo)?;
            if branch.is_empty() || branch.contains(['\r', '\n']) {
                bail!("invalid --from '{}': branch must be a single line", value);
            }
            if let Some(existing) = sources.per_repo.insert(repo.into(), branch.into()) {
                if existing != branch {
                    bail!(
                        "conflicting --from values for repo '{}': '{}' and '{}'",
                        repo,
                        existing,
                        branch
                    );
                }
            }
        }
        Ok(sources)
    }

    pub fn for_repo(&self, repo: &str) -> Option<ReopenBase> {
        self.per_repo
            .get(repo)
            .cloned()
            .map(ReopenBase::Branch)
            .or_else(|| self.current_default.then_some(ReopenBase::Current))
    }

    fn explicit_branch(&self, repo: &str) -> Option<&str> {
        self.per_repo.get(repo).map(String::as_str)
    }

    fn repo_names(&self) -> impl Iterator<Item = &str> {
        self.per_repo.keys().map(String::as_str)
    }
}

pub fn build_reopen_plan<R: CommandRunner, P: ReopenPrompt>(
    config_manager: &ConfigManager,
    runner: &R,
    name: &str,
    options: &ReopenOptions,
    prompt: &mut P,
) -> Result<ReopenPlan> {
    let (from_status, workspace) = config_manager.load_workspace(name)?;
    if !matches!(
        from_status,
        WorkspaceStatus::Done | WorkspaceStatus::Canceled
    ) {
        bail!(
            "workspace '{}' is {}, expected done or canceled",
            name,
            from_status.as_str()
        );
    }

    let workspace_repo_names = workspace
        .repos
        .iter()
        .map(|repo| repo.name.as_str())
        .collect::<BTreeSet<_>>();
    for repo in options.sources.repo_names() {
        if !workspace_repo_names.contains(repo) {
            bail!(
                "--from references repo '{}' outside workspace '{}'",
                repo,
                workspace.name
            );
        }
    }
    for repo in &options.overwrite_repos {
        validate_config_name("repo", repo)?;
        if !workspace_repo_names.contains(repo.as_str()) {
            bail!(
                "--overwrite references repo '{}' outside workspace '{}'",
                repo,
                workspace.name
            );
        }
    }

    let git = GitOps::new(runner);
    let workspace_dir = PathBuf::from(shellexpand::tilde(&workspace.workspace_dir).into_owned());
    let mut repos = Vec::with_capacity(workspace.repos.len());
    for repo_entry in &workspace.repos {
        let repo_config = config_manager.load_repo_config(&repo_entry.name)?;
        let repo_path = shellexpand::tilde(&repo_config.path).into_owned();
        let worktree_path = workspace_dir.join(&repo_entry.name);
        ensure_not_registered_source(&worktree_path, Path::new(&repo_path), &repo_entry.name)?;

        let branch_source = if git.branch_exists(&repo_path, &workspace.branch)? {
            if options.sources.explicit_branch(&repo_entry.name).is_some() {
                bail!(
                    "repo '{}' already has recoverable task branch '{}'; --from is not applicable",
                    repo_entry.name,
                    workspace.branch
                );
            }
            TaskBranchSource::Local
        } else {
            let remote_branches = git.remote_branches(&repo_path, &workspace.branch)?;
            if remote_branches.is_empty() {
                resolve_missing_task_branch(
                    &git,
                    &repo_path,
                    &repo_entry.name,
                    options.sources.for_repo(&repo_entry.name),
                    prompt,
                )?
            } else {
                let origin = format!("origin/{}", workspace.branch);
                let selected = if remote_branches.iter().any(|branch| branch == &origin) {
                    if options.sources.explicit_branch(&repo_entry.name).is_some() {
                        bail!(
                            "repo '{}' already has recoverable remote task branch '{}'; --from is not applicable",
                            repo_entry.name,
                            origin
                        );
                    }
                    origin
                } else if remote_branches.len() == 1 {
                    if options.sources.explicit_branch(&repo_entry.name).is_some() {
                        bail!(
                            "repo '{}' already has recoverable remote task branch '{}'; --from is not applicable",
                            repo_entry.name,
                            remote_branches[0]
                        );
                    }
                    remote_branches[0].clone()
                } else if let Some(explicit) = options.sources.explicit_branch(&repo_entry.name) {
                    if remote_branches.iter().any(|branch| branch == explicit) {
                        explicit.to_string()
                    } else {
                        bail!(
                            "--from for repo '{}' must select one of: {}",
                            repo_entry.name,
                            remote_branches.join(", ")
                        );
                    }
                } else {
                    prompt.choose_remote(&repo_entry.name, &remote_branches)?
                };
                TaskBranchSource::Remote(selected)
            }
        };
        let worktrees = git.worktrees(&repo_path)?;
        let worktree_path_string = worktree_path.to_string_lossy().into_owned();
        let target_worktree = worktrees
            .iter()
            .find(|worktree| same_path(Path::new(&worktree.path), &worktree_path));
        let valid_worktree = target_worktree
            .is_some_and(|worktree| worktree.branch.as_deref() == Some(&workspace.branch));
        if worktrees.iter().any(|worktree| {
            worktree.branch.as_deref() == Some(&workspace.branch)
                && !same_path(Path::new(&worktree.path), &worktree_path)
        }) {
            let occupied = worktrees
                .iter()
                .find(|worktree| {
                    worktree.branch.as_deref() == Some(&workspace.branch)
                        && !same_path(Path::new(&worktree.path), &worktree_path)
                })
                .expect("occupied worktree was just found");
            bail!(
                "task branch '{}' for repo '{}' is checked out at '{}'",
                workspace.branch,
                repo_entry.name,
                occupied.path
            );
        }

        let occupied = std::fs::symlink_metadata(&worktree_path).is_ok();
        let overwrite = options.overwrite_repos.contains(&repo_entry.name);
        let worktree_action = if !occupied {
            WorktreeAction::Create
        } else if overwrite {
            WorktreeAction::Overwrite {
                registered: target_worktree.is_some(),
            }
        } else if prompt.is_interactive() {
            let dirty = valid_worktree && git.has_uncommitted_changes(&worktree_path_string)?;
            if prompt.confirm_overwrite(
                &repo_entry.name,
                &worktree_path_string,
                valid_worktree,
                dirty,
            )? {
                WorktreeAction::Overwrite {
                    registered: target_worktree.is_some(),
                }
            } else if valid_worktree {
                WorktreeAction::Reuse
            } else {
                bail!(
                    "repo '{}' cannot reuse occupied path '{}'",
                    repo_entry.name,
                    worktree_path_string
                );
            }
        } else if valid_worktree {
            WorktreeAction::Reuse
        } else {
            bail!(
                "repo '{}' requires --overwrite before replacing '{}'",
                repo_entry.name,
                worktree_path_string
            );
        };

        repos.push(RepoReopenPlan {
            repo_name: repo_entry.name.clone(),
            repo_path,
            worktree_path: worktree_path_string,
            target_branch: repo_entry.target_branch.clone(),
            branch_source,
            worktree_action,
        });
    }

    Ok(ReopenPlan {
        from_status,
        archived_workspace: workspace.clone(),
        workspace,
        repos,
    })
}

pub fn execute_reopen_plan<R: CommandRunner>(
    config_manager: &ConfigManager,
    global: &GlobalConfig,
    runner: &R,
    mut plan: ReopenPlan,
    skip_hooks: bool,
) -> Result<WorkspaceConfig> {
    plan.apply_current_terminal_config(global);
    let git = GitOps::new(runner);
    let hook_engine = HookEngine::new(runner);
    let workspace_dir =
        PathBuf::from(shellexpand::tilde(&plan.workspace.workspace_dir).into_owned());
    let workspace_dir_created = !workspace_dir.exists();
    std::fs::create_dir_all(&workspace_dir).with_context(|| {
        format!(
            "failed to create workspace directory '{}'",
            workspace_dir.display()
        )
    })?;
    let mut created = Vec::new();

    let recovery = (|| -> Result<()> {
        for repo in &plan.repos {
            if matches!(repo.worktree_action, WorktreeAction::Reuse) {
                continue;
            }
            validate_destructive_target(&workspace_dir, repo)?;
            if let WorktreeAction::Overwrite { registered } = repo.worktree_action {
                if registered {
                    git.worktree_remove(&repo.repo_path, &repo.worktree_path, true)?;
                } else {
                    remove_occupied_path(Path::new(&repo.worktree_path))?;
                }
            }

            match &repo.branch_source {
                TaskBranchSource::Local => git.worktree_add_existing(
                    &repo.repo_path,
                    &plan.workspace.branch,
                    &repo.worktree_path,
                )?,
                TaskBranchSource::Remote(remote) => git.worktree_add_tracking(
                    &repo.repo_path,
                    &plan.workspace.branch,
                    &repo.worktree_path,
                    remote,
                )?,
                TaskBranchSource::Base { revision, .. } => git.worktree_add(
                    &repo.repo_path,
                    &plan.workspace.branch,
                    &repo.worktree_path,
                    revision,
                )?,
            }
            created.push((repo.repo_path.clone(), repo.worktree_path.clone()));

            let repo_config = config_manager.load_repo_config(&repo.repo_name)?;
            let patterns =
                copy_files::merge_copy_files(&global.copy_files, &repo_config.copy_files);
            if !patterns.is_empty() {
                copy_files::copy_files_to_worktree(
                    Path::new(&repo.repo_path),
                    Path::new(&repo.worktree_path),
                    &patterns,
                )?;
            }

            if !skip_hooks {
                let hook = repo_config
                    .hooks
                    .post_create
                    .as_ref()
                    .or(global.hooks.post_create.as_ref());
                if let Some(hook) = hook {
                    hook_engine.execute(
                        hook,
                        &HookContext {
                            workspace: plan.workspace.name.clone(),
                            repo: Some(repo.repo_name.clone()),
                            branch: plan.workspace.branch.clone(),
                            target_branch: repo.target_branch.clone(),
                            worktree_path: Some(repo.worktree_path.clone()),
                            workspace_dir: workspace_dir.to_string_lossy().into_owned(),
                        },
                    )?;
                }
            }
        }
        Ok(())
    })();

    if let Err(error) = recovery {
        return Err(rollback_reopen(
            error,
            &git,
            &created,
            workspace_dir_created.then_some(&workspace_dir),
        ));
    }

    let archived_workspace = plan.archived_workspace.clone();
    plan.workspace.events.push(Event {
        action: "reopened".into(),
        timestamp: Local::now().to_rfc3339(),
        detail: Some(format!("from {}", plan.from_status.as_str())),
    });
    if let Err(error) = config_manager.save_workspace(&plan.from_status, &plan.workspace) {
        let error = restore_archived_workspace(
            config_manager,
            &plan.from_status,
            &archived_workspace,
            error,
        );
        return Err(rollback_reopen(
            error,
            &git,
            &created,
            workspace_dir_created.then_some(&workspace_dir),
        ));
    }
    if let Err(error) = config_manager.move_workspace(
        &plan.workspace.name,
        &plan.from_status,
        &WorkspaceStatus::InProgress,
    ) {
        let error = restore_archived_workspace(
            config_manager,
            &plan.from_status,
            &archived_workspace,
            error,
        );
        return Err(rollback_reopen(
            error,
            &git,
            &created,
            workspace_dir_created.then_some(&workspace_dir),
        ));
    }

    if !skip_hooks {
        hook_engine
            .execute_if_set(
                &global.hooks.post_start,
                &HookContext {
                    workspace: plan.workspace.name.clone(),
                    repo: None,
                    branch: plan.workspace.branch.clone(),
                    target_branch: None,
                    worktree_path: None,
                    workspace_dir: workspace_dir.to_string_lossy().into_owned(),
                },
            )
            .map_err(|error| {
                anyhow::anyhow!(
                    "workspace '{}' reopened and remains in_progress, but post_start hook failed: {error:#}",
                    plan.workspace.name
                )
            })?;
    }

    Ok(plan.workspace)
}

fn restore_archived_workspace(
    config_manager: &ConfigManager,
    status: &WorkspaceStatus,
    workspace: &WorkspaceConfig,
    error: anyhow::Error,
) -> anyhow::Error {
    match config_manager.save_workspace(status, workspace) {
        Ok(()) => error,
        Err(restore_error) => anyhow::anyhow!(
            "reopen state transition failed: {error:#}; failed to restore archived workspace config: {restore_error:#}"
        ),
    }
}

pub fn format_reopen_plan(plan: &ReopenPlan, lifecycle: &ReopenLifecyclePlan) -> String {
    let mut output = format!(
        "reopen '{}' from {}:\n",
        plan.workspace.name,
        plan.from_status.as_str()
    );
    for repo in &plan.repos {
        let source = match &repo.branch_source {
            TaskBranchSource::Local => "local task branch".to_string(),
            TaskBranchSource::Remote(branch) => format!("remote task branch {branch}"),
            TaskBranchSource::Base { display, .. } => format!("base {display}"),
        };
        let action = match repo.worktree_action {
            WorktreeAction::Reuse => "reuse worktree".to_string(),
            WorktreeAction::Create => format!("create worktree from {source}"),
            WorktreeAction::Overwrite { registered: true } => {
                format!("overwrite registered worktree from {source}")
            }
            WorktreeAction::Overwrite { registered: false } => {
                format!("overwrite occupied path from {source}")
            }
        };
        output.push_str(&format!(
            "  {}: {} ({})\n",
            repo.repo_name, action, repo.worktree_path
        ));
    }
    let multiplexer = plan.workspace.multiplexer.kind.as_str();
    output.push_str(&format!(
        "  terminal config: current global config ({multiplexer})\n"
    ));
    if plan
        .repos
        .iter()
        .any(|repo| matches!(repo.worktree_action, WorktreeAction::Overwrite { .. }))
    {
        output.push_str("  terminal before recovery: close before overwriting worktrees\n");
    } else {
        output.push_str("  terminal before recovery: preserve existing environment\n");
    }
    let post_create = if lifecycle.skip_hooks {
        "skip post_create hooks"
    } else {
        "run post_create hooks"
    };
    output.push_str(&format!(
        "  worktree setup: copy files; {post_create} (new and overwritten worktrees only)\n"
    ));
    output.push_str(&format!(
        "  state: append reopened event and move {} -> in_progress\n",
        plan.from_status.as_str()
    ));
    output.push_str(if lifecycle.skip_hooks {
        "  post_start: skip\n"
    } else {
        "  post_start: run\n"
    });
    let activation = match (lifecycle.activate_terminal_environment, lifecycle.run_agent) {
        (true, true) => "activate with requested agent",
        (true, false) => "activate without an agent request",
        (false, true) => "skip activation; requested agent will not be launched",
        (false, false) => "skip activation",
    };
    output.push_str(&format!("  terminal after recovery: {activation}\n"));
    output
}

fn same_path(left: &Path, right: &Path) -> bool {
    lexically_normalized(left) == lexically_normalized(right)
}

fn lexically_normalized(path: &Path) -> PathBuf {
    use std::path::Component;

    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::Normal(part) => normalized.push(part),
        }
    }
    normalized
}

fn ensure_not_registered_source(target: &Path, source: &Path, repo_name: &str) -> Result<()> {
    if same_path(target, source) {
        bail!(
            "refusing to reopen repo '{}' over its registered source path '{}'",
            repo_name,
            source.display()
        );
    }

    let target_metadata = match std::fs::symlink_metadata(target) {
        Ok(metadata) => Some(metadata),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    if target_metadata
        .as_ref()
        .is_some_and(|metadata| metadata.file_type().is_symlink())
    {
        return Ok(());
    }
    if target_metadata.is_none() {
        return Ok(());
    }

    let resolved_source = match std::fs::canonicalize(source) {
        Ok(path) => path,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(error).with_context(|| {
                format!(
                    "failed to resolve registered source path '{}' for repo '{}'",
                    source.display(),
                    repo_name
                )
            })
        }
    };
    let resolved_target = std::fs::canonicalize(target)?;
    if same_path(&resolved_target, &resolved_source) {
        bail!(
            "refusing to reopen repo '{}' over its registered source path '{}'",
            repo_name,
            source.display()
        );
    }
    Ok(())
}

fn validate_destructive_target(workspace_dir: &Path, repo: &RepoReopenPlan) -> Result<()> {
    let target = Path::new(&repo.worktree_path);
    if target
        .parent()
        .is_none_or(|parent| !same_path(parent, workspace_dir))
    {
        bail!(
            "refusing to modify unsafe worktree path '{}' for repo '{}'",
            repo.worktree_path,
            repo.repo_name
        );
    }
    ensure_not_registered_source(target, Path::new(&repo.repo_path), &repo.repo_name)?;
    Ok(())
}

fn remove_occupied_path(path: &Path) -> Result<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || metadata.is_file() {
        std::fs::remove_file(path)?;
    } else if metadata.is_dir() {
        std::fs::remove_dir_all(path)?;
    } else {
        bail!("unsupported occupied path '{}'", path.display());
    }
    Ok(())
}

fn rollback_reopen<R: CommandRunner>(
    error: anyhow::Error,
    git: &GitOps<'_, R>,
    created: &[(String, String)],
    workspace_dir: Option<&PathBuf>,
) -> anyhow::Error {
    let mut failures = Vec::new();
    for (repo_path, worktree_path) in created.iter().rev() {
        if let Err(rollback_error) = git.worktree_remove(repo_path, worktree_path, true) {
            failures.push(format!("{}: {rollback_error:#}", worktree_path));
        }
    }
    if let Some(workspace_dir) = workspace_dir {
        match std::fs::remove_dir(workspace_dir) {
            Ok(()) => {}
            Err(remove_error)
                if matches!(
                    remove_error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::DirectoryNotEmpty
                ) => {}
            Err(remove_error) => {
                failures.push(format!("{}: {}", workspace_dir.display(), remove_error))
            }
        }
    }
    if failures.is_empty() {
        error
    } else {
        anyhow::anyhow!(
            "reopen failed: {error:#}; rollback failed: {}",
            failures.join("; ")
        )
    }
}

fn resolve_missing_task_branch<R: CommandRunner, P: ReopenPrompt>(
    git: &GitOps<'_, R>,
    repo_path: &str,
    repo_name: &str,
    selected: Option<ReopenBase>,
    prompt: &mut P,
) -> Result<TaskBranchSource> {
    if let Some(ReopenBase::Branch(branch)) = selected {
        if !git.branch_ref_exists(repo_path, &branch)? {
            bail!("base branch '{}' not found in repo '{}'", branch, repo_name);
        }
        return Ok(TaskBranchSource::Base {
            revision: branch.clone(),
            display: branch,
        });
    }

    let current_branch = git.current_branch(repo_path)?;
    let current = if current_branch == "HEAD" {
        let short = git.short_revision(repo_path, "HEAD")?;
        TaskBranchSource::Base {
            revision: "HEAD".into(),
            display: format!("HEAD at {short}"),
        }
    } else {
        TaskBranchSource::Base {
            revision: current_branch.clone(),
            display: current_branch,
        }
    };

    match selected {
        Some(ReopenBase::Current) => Ok(current),
        Some(ReopenBase::Branch(_)) => unreachable!("explicit branches return before lookup"),
        None => {
            let display = match &current {
                TaskBranchSource::Base { display, .. } => display,
                _ => unreachable!("current source is always a base"),
            };
            match prompt.choose_base(repo_name, display)? {
                ReopenBase::Current => Ok(current),
                ReopenBase::Branch(branch) => {
                    if !git.branch_ref_exists(repo_path, &branch)? {
                        bail!("base branch '{}' not found in repo '{}'", branch, repo_name);
                    }
                    Ok(TaskBranchSource::Base {
                        revision: branch.clone(),
                        display: branch,
                    })
                }
            }
        }
    }
}
