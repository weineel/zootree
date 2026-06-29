use crate::cli::workspace::CreateArgs;
use crate::config::global::GlobalConfig;
use crate::config::global::ZellijConfig;
use crate::config::workspace::{Event, RepoEntry, WorkspaceConfig};
use crate::config::ConfigManager;
use crate::core::git::GitOps;
use crate::core::name_gen::NameGenerator;
use crate::runner::CommandRunner;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AfterCreateMode {
    CreateOnly,
    Start,
    StartAndRunAgent { run_agent: Option<String> },
}

impl AfterCreateMode {
    pub fn should_start(&self) -> bool {
        !matches!(self, Self::CreateOnly)
    }

    pub fn run_agent_arg(&self) -> Option<Option<String>> {
        match self {
            Self::CreateOnly | Self::Start => None,
            Self::StartAndRunAgent { run_agent } => Some(run_agent.clone()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreateDraftError {
    TitleRequired,
    TitleSingleLineRequired,
    WorkspaceNameRequired,
    WorkspaceNameSingleLineRequired,
    WorkspaceBranchRequired,
    WorkspaceBranchSingleLineRequired,
    WorkspaceNameExists(String),
    RepoRequired,
    TargetBranchRequired(String),
    TargetBranchSingleLineRequired(String),
    DefaultAgentMissing,
    RunAgentSingleLineRequired,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoDraftEntry {
    pub name: String,
    pub target_branch: String,
    pub selected: bool,
}

impl RepoDraftEntry {
    pub fn new(name: impl Into<String>, target_branch: impl Into<String>, selected: bool) -> Self {
        Self {
            name: name.into(),
            target_branch: target_branch.into(),
            selected,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateDraft {
    pub title: String,
    pub description: String,
    pub name: String,
    pub branch: String,
    pub branch_was_edited: bool,
    pub workspace_dir: String,
    pub repos: Vec<RepoDraftEntry>,
    pub after_create: AfterCreateMode,
}

impl CreateDraft {
    pub fn new(title: impl Into<String>, name: impl Into<String>, global: &GlobalConfig) -> Self {
        let name = name.into();
        Self {
            title: title.into(),
            description: String::new(),
            branch: default_branch(global, &name),
            branch_was_edited: false,
            workspace_dir: default_workspace_dir(global, &name),
            name,
            repos: Vec::new(),
            after_create: AfterCreateMode::CreateOnly,
        }
    }

    pub fn set_name(&mut self, name: impl Into<String>, global: &GlobalConfig) {
        self.name = name.into();
        if !self.branch_was_edited {
            self.branch = default_branch(global, &self.name);
        }
        self.workspace_dir = default_workspace_dir(global, &self.name);
    }

    pub fn set_branch(&mut self, branch: impl Into<String>) {
        self.branch = branch.into();
        self.branch_was_edited = true;
    }

    pub fn repo(&self, name: &str) -> Option<&RepoDraftEntry> {
        self.repos.iter().find(|repo| repo.name == name)
    }

    pub fn toggle_repo(&mut self, name: &str) {
        if let Some(repo) = self.repos.iter_mut().find(|repo| repo.name == name) {
            repo.selected = !repo.selected;
        }
    }

    pub fn apply_template_repos(&mut self, template_repos: &[String]) {
        for repo in &mut self.repos {
            repo.selected = template_repos.iter().any(|name| name == &repo.name);
        }
    }

    pub fn selected_repos(&self) -> Vec<&RepoDraftEntry> {
        self.repos.iter().filter(|repo| repo.selected).collect()
    }

    pub fn validate(
        &self,
        existing_workspaces: &[String],
        global: &GlobalConfig,
    ) -> Vec<CreateDraftError> {
        let mut errors = Vec::new();
        if self.title.trim().is_empty() {
            errors.push(CreateDraftError::TitleRequired);
        }
        if self.name.trim().is_empty() {
            errors.push(CreateDraftError::WorkspaceNameRequired);
        }
        if self.branch.trim().is_empty() {
            errors.push(CreateDraftError::WorkspaceBranchRequired);
        }
        if existing_workspaces.iter().any(|name| name == &self.name) {
            errors.push(CreateDraftError::WorkspaceNameExists(self.name.clone()));
        }
        let selected = self.selected_repos();
        if selected.is_empty() {
            errors.push(CreateDraftError::RepoRequired);
        }
        for repo in selected {
            if repo.target_branch.trim().is_empty() {
                errors.push(CreateDraftError::TargetBranchRequired(repo.name.clone()));
            }
        }
        if matches!(
            self.after_create,
            AfterCreateMode::StartAndRunAgent { run_agent: None }
        ) && global.agent_cli.is_none()
        {
            errors.push(CreateDraftError::DefaultAgentMissing);
        }
        errors
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateWizardOutput {
    pub draft: CreateDraft,
}

pub fn create_args_need_wizard(args: &CreateArgs) -> bool {
    args.title.is_none() || (args.repos.is_none() && args.template.is_none())
}

pub fn draft_from_args<R: CommandRunner>(
    args: &CreateArgs,
    config_mgr: &ConfigManager,
    runner: &R,
    global: &GlobalConfig,
    current_repo: Option<(String, String)>,
    existing_workspaces: &[String],
) -> anyhow::Result<CreateDraft> {
    let name = args.name.clone().unwrap_or_else(|| {
        let name_gen = NameGenerator::new();
        name_gen.generate_avoiding(existing_workspaces)
    });
    let title = args.title.clone().unwrap_or_default();
    let mut draft = CreateDraft::new(title, name, global);
    draft.description = args.description.clone().unwrap_or_default();
    if let Some(branch) = &args.branch {
        draft.set_branch(branch.clone());
    }

    let needs_wizard = create_args_need_wizard(args);
    if let Some(repos_str) = &args.repos {
        draft.repos = build_requested_repo_draft_entries(
            config_mgr,
            runner,
            crate::cli::workspace::parse_repos_arg(repos_str),
        )?;
    } else if let Some(template_name) = &args.template {
        let template = config_mgr.load_template(template_name)?;
        if template.repos.is_empty() {
            anyhow::bail!("template '{}' has no repos", template_name);
        }
        if needs_wizard {
            draft.repos = build_repo_draft_entries(config_mgr, runner, current_repo)?;
            draft.apply_template_repos(&template.repos);
        } else {
            let repos = template
                .repos
                .into_iter()
                .map(|name| (name, None))
                .collect();
            draft.repos = build_requested_repo_draft_entries(config_mgr, runner, repos)?;
        }
    } else {
        draft.repos = build_repo_draft_entries(config_mgr, runner, current_repo)?;
    }

    draft.after_create = match &args.run_agent {
        Some(Some(value)) if !value.is_empty() => AfterCreateMode::StartAndRunAgent {
            run_agent: Some(value.clone()),
        },
        Some(_) => AfterCreateMode::StartAndRunAgent { run_agent: None },
        None if args.start => AfterCreateMode::Start,
        None => AfterCreateMode::CreateOnly,
    };

    Ok(draft)
}

pub fn resolve_agent_cli_for_draft(
    mode: &AfterCreateMode,
    global: &GlobalConfig,
) -> anyhow::Result<Option<String>> {
    match mode {
        AfterCreateMode::CreateOnly | AfterCreateMode::Start => Ok(None),
        AfterCreateMode::StartAndRunAgent {
            run_agent: Some(value),
        } if !value.is_empty() => Ok(Some(value.clone())),
        AfterCreateMode::StartAndRunAgent { .. } => Ok(Some(global.agent_cli.clone().ok_or_else(
            || {
                anyhow::anyhow!(
                    "--run-agent requires agent_cli in global config (~/.config/zootree/config.toml)"
                )
            },
        )?)),
    }
}

pub fn workspace_from_draft(
    draft: &CreateDraft,
    created_at: impl Into<String>,
    agent_cli: Option<String>,
) -> WorkspaceConfig {
    let created_at = created_at.into();
    WorkspaceConfig {
        title: draft.title.clone(),
        name: draft.name.clone(),
        description: draft.description.clone(),
        branch: draft.branch.clone(),
        workspace_dir: draft.workspace_dir.clone(),
        created_at: created_at.clone(),
        agent_cli,
        zellij: ZellijConfig {
            session_mode: Some("standalone".into()),
            ..Default::default()
        },
        repos: draft
            .repos
            .iter()
            .filter(|repo| repo.selected)
            .map(|repo| RepoEntry {
                name: repo.name.clone(),
                target_branch: Some(repo.target_branch.clone()),
            })
            .collect(),
        events: vec![Event {
            action: "created".into(),
            timestamp: created_at,
            detail: None,
        }],
    }
}

pub fn build_repo_draft_entries<R: CommandRunner>(
    config_mgr: &ConfigManager,
    runner: &R,
    current_repo: Option<(String, String)>,
) -> anyhow::Result<Vec<RepoDraftEntry>> {
    let git = GitOps::new(runner);
    let mut repos = Vec::new();
    for name in config_mgr.list_repos()? {
        let config = config_mgr.load_repo_config(&name)?;
        let expanded_path = shellexpand::tilde(&config.path).into_owned();
        let is_current = current_repo
            .as_ref()
            .map(|(current_name, _)| current_name == &name)
            .unwrap_or(false);
        let target_branch = if let Some((_, branch)) = current_repo
            .as_ref()
            .filter(|(current_name, _)| current_name == &name)
        {
            branch.clone()
        } else if let Some(default) = config.default_target_branch {
            default
        } else {
            git.current_branch(&expanded_path)
                .unwrap_or_else(|_| "main".into())
        };
        repos.push(RepoDraftEntry::new(name, target_branch, is_current));
    }
    Ok(repos)
}

fn build_requested_repo_draft_entries<R: CommandRunner>(
    config_mgr: &ConfigManager,
    runner: &R,
    repos: Vec<(String, Option<String>)>,
) -> anyhow::Result<Vec<RepoDraftEntry>> {
    let registered = config_mgr.list_repos()?;
    let git = GitOps::new(runner);
    let mut entries = Vec::new();

    for (name, branch) in repos {
        if !registered.iter().any(|registered| registered == &name) {
            anyhow::bail!("repo '{}' is not registered", name);
        }
        let config = config_mgr.load_repo_config(&name)?;
        let expanded_path = shellexpand::tilde(&config.path).into_owned();
        let target_branch = branch.or(config.default_target_branch).unwrap_or_else(|| {
            git.current_branch(&expanded_path)
                .unwrap_or_else(|_| "main".into())
        });
        entries.push(RepoDraftEntry::new(name, target_branch, true));
    }

    Ok(entries)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateWizardLayout {
    TwoColumn,
    SingleColumn,
    TooNarrow,
}

impl CreateWizardLayout {
    pub fn for_width(width: u16) -> Self {
        if width < 50 {
            Self::TooNarrow
        } else if width >= 100 {
            Self::TwoColumn
        } else {
            Self::SingleColumn
        }
    }
}

fn default_branch(global: &GlobalConfig, name: &str) -> String {
    format!("{}/{}", global.branch_prefix, name)
}

fn default_workspace_dir(global: &GlobalConfig, name: &str) -> String {
    format!("{}/{}", shellexpand::tilde(&global.workspace_root), name)
}
