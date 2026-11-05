//! Workspace creation as a service function.
//!
//! The interactive wizard stays in the CLI adapter: it produces a
//! [`CreateDraft`], and [`create_workspace_from_draft`] validates and persists
//! it. [`create_workspace`] builds the draft from a plain request and fails
//! with an error when required parameters are missing.

use crate::cli::create_flow::{
    build_requested_repo_draft_entries, persist_selected_pending_repos,
    resolve_agent_cli_for_draft, workspace_from_draft, AfterCreateMode, CreateDraft,
    CreateDraftError,
};
use crate::config::global::MultiplexerKind;
use crate::config::template::TemplateConfig;
use crate::config::workspace::{WorkspaceConfig, WorkspaceStatus};
use crate::config::ConfigManager;
use crate::core::name_gen::NameGenerator;
use crate::runner::CommandRunner;
use crate::service::{RequestContext, ServiceEvent};
use anyhow::Result;
use chrono::Local;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CreateRequest {
    /// Required.
    pub title: Option<String>,
    /// Generated when absent.
    pub name: Option<String>,
    pub description: Option<String>,
    pub branch: Option<String>,
    pub multiplexer: Option<MultiplexerKind>,
    /// `(repo, target branch)` pairs. Either `repos` or `template` is required.
    pub repos: Option<Vec<(String, Option<String>)>>,
    pub template: Option<String>,
    pub start: bool,
    /// `Some(None)` means "run the default agent CLI".
    pub run_agent: Option<Option<String>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CreateOutcome {
    pub workspace: WorkspaceConfig,
    /// What the caller should do after creation (start is the caller's job).
    pub after_create: AfterCreateMode,
}

pub fn format_draft_errors(errors: &[CreateDraftError]) -> String {
    errors
        .iter()
        .map(|error| match error {
            CreateDraftError::TitleRequired => "title is required".to_string(),
            CreateDraftError::TitleSingleLineRequired => "title must be a single line".to_string(),
            CreateDraftError::WorkspaceNameRequired => "workspace name is required".to_string(),
            CreateDraftError::WorkspaceNameInvalid(name) => {
                format!("workspace name '{name}' must use only ASCII letters, numbers, '-' and '_'")
            }
            CreateDraftError::WorkspaceNameSingleLineRequired => {
                "workspace name must be a single line".to_string()
            }
            CreateDraftError::WorkspaceBranchRequired => "workspace branch is required".to_string(),
            CreateDraftError::WorkspaceBranchSingleLineRequired => {
                "workspace branch must be a single line".to_string()
            }
            CreateDraftError::WorkspaceNameExists(name) => {
                format!("workspace name '{}' already exists", name)
            }
            CreateDraftError::RepoRequired => "at least one repo must be selected".to_string(),
            CreateDraftError::TargetBranchRequired(repo) => {
                format!("target branch for repo '{}' is required", repo)
            }
            CreateDraftError::TargetBranchSingleLineRequired(repo) => {
                format!("target branch for repo '{}' must be a single line", repo)
            }
            CreateDraftError::DefaultAgentMissing => {
                "--run-agent requires agent_cli in global config (~/.config/zootree/config.toml)"
                    .to_string()
            }
            CreateDraftError::RunAgentSingleLineRequired => {
                "run-agent must be a single line".to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// Creates a pending Workspace from a plain request. Missing title, or missing
/// repos and template, is an error: the service never prompts.
pub fn create_workspace<R: CommandRunner>(
    config_mgr: &ConfigManager,
    runner: &R,
    ctx: &RequestContext<'_>,
    req: &CreateRequest,
) -> Result<CreateOutcome> {
    config_mgr.ensure_dirs()?;
    let global = config_mgr.load_global_config()?;
    let existing = existing_workspace_names(config_mgr)?;

    let name = req
        .name
        .clone()
        .unwrap_or_else(|| NameGenerator::new().generate_avoiding(&existing));
    let mut draft = CreateDraft::new(req.title.clone().unwrap_or_default(), name, &global);
    if let Some(kind) = &req.multiplexer {
        let mut multiplexer = global.multiplexer.clone();
        multiplexer.kind = kind.clone();
        draft.multiplexer = Some(multiplexer);
    }
    draft.description = req.description.clone().unwrap_or_default();
    if let Some(branch) = &req.branch {
        draft.set_branch(branch.clone());
    }

    let requested = if let Some(repos) = &req.repos {
        repos.clone()
    } else if let Some(template_name) = &req.template {
        let template = config_mgr.load_template(template_name)?;
        if template.repos.is_empty() {
            anyhow::bail!("template '{}' has no repos", template_name);
        }
        template
            .repos
            .into_iter()
            .map(|name| (name, None))
            .collect()
    } else {
        anyhow::bail!("either repos or a template is required");
    };
    draft.repos = build_requested_repo_draft_entries(config_mgr, runner, requested)?;

    draft.after_create = match &req.run_agent {
        Some(Some(value)) if !value.is_empty() => AfterCreateMode::StartAndRunAgent {
            run_agent: Some(value.clone()),
        },
        Some(_) => AfterCreateMode::StartAndRunAgent { run_agent: None },
        None if req.start => AfterCreateMode::Start,
        None => AfterCreateMode::CreateOnly,
    };

    create_workspace_from_draft(config_mgr, ctx, draft)
}

/// Validates and persists a finished draft (from a request or from the wizard).
pub fn create_workspace_from_draft(
    config_mgr: &ConfigManager,
    ctx: &RequestContext<'_>,
    mut draft: CreateDraft,
) -> Result<CreateOutcome> {
    config_mgr.ensure_dirs()?;
    let global = config_mgr.load_global_config()?;
    let existing = existing_workspace_names(config_mgr)?;
    let errors = draft.validate(&existing, &global);
    if !errors.is_empty() {
        anyhow::bail!("invalid create options: {}", format_draft_errors(&errors));
    }

    persist_selected_pending_repos(config_mgr, &mut draft)?;
    let agent_cli = resolve_agent_cli_for_draft(&draft.after_create, &global)?;
    let multiplexer = draft
        .multiplexer
        .clone()
        .unwrap_or_else(|| global.multiplexer.clone());
    let workspace = workspace_from_draft(&draft, Local::now().to_rfc3339(), agent_cli, multiplexer);
    let name = workspace.name.clone();

    ctx.emit(ServiceEvent::Started {
        operation: "create",
        workspace: name.clone(),
    });
    config_mgr.save_workspace(&WorkspaceStatus::Pending, &workspace)?;
    save_recently_template(config_mgr, &workspace)?;

    ctx.step(format!("workspace '{}' created (pending)", name));
    ctx.step(format!("  branch: {}", workspace.branch));
    ctx.step(format!(
        "  repos: {}",
        workspace
            .repos
            .iter()
            .map(|r| format!("{}:{}", r.name, r.target_branch.as_deref().unwrap_or("*")))
            .collect::<Vec<_>>()
            .join(", ")
    ));

    Ok(CreateOutcome {
        workspace,
        after_create: draft.after_create,
    })
}

fn existing_workspace_names(config_mgr: &ConfigManager) -> Result<Vec<String>> {
    Ok(config_mgr
        .list_workspaces(None::<&[WorkspaceStatus]>)?
        .iter()
        .map(|w| w.name.clone())
        .collect())
}

fn save_recently_template(config_mgr: &ConfigManager, workspace: &WorkspaceConfig) -> Result<()> {
    let recently = TemplateConfig {
        repos: workspace.repos.iter().map(|r| r.name.clone()).collect(),
        multiplexer: workspace.multiplexer.clone(),
    };
    config_mgr.save_template("recently", &recently)
}
