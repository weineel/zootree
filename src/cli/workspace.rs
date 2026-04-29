use clap::Args;
use crate::config::ConfigManager;
use crate::config::workspace::{WorkspaceConfig, WorkspaceStatus, RepoEntry, Event};
use crate::config::template::TemplateConfig;
use crate::core::name_gen::NameGenerator;
use crate::tui;
use anyhow::Result;
use chrono::Local;

pub fn parse_repos_arg(repos_str: &str) -> Vec<(String, Option<String>)> {
    repos_str.split(',')
        .map(|s| {
            let s = s.trim();
            if let Some((name, branch)) = s.split_once(':') {
                (name.to_string(), Some(branch.to_string()))
            } else {
                (s.to_string(), None)
            }
        })
        .collect()
}

pub fn handle_create(args: &CreateArgs) -> Result<()> {
    let config_mgr = ConfigManager::new()?;
    config_mgr.ensure_dirs()?;
    let global = config_mgr.load_global_config()?;

    let title = match &args.title {
        Some(t) => t.clone(),
        None => tui::input_required("Title")?,
    };

    let description = match &args.description {
        Some(d) => d.clone(),
        None => tui::input_optional("Description (optional)")?.unwrap_or_default(),
    };

    let repo_entries = if let Some(repos_str) = &args.repos {
        let parsed = parse_repos_arg(repos_str);
        let mut entries = Vec::new();
        for (name, branch) in parsed {
            let repo_config = config_mgr.load_repo_config(&name)?;
            let target_branch = branch
                .or(repo_config.default_target_branch.clone())
                .ok_or_else(|| anyhow::anyhow!("target branch required for repo '{}'", name))?;
            entries.push(RepoEntry { name, target_branch });
        }
        entries
    } else {
        let _template_repos = if let Some(tmpl_name) = &args.template {
            let tmpl = config_mgr.load_template(tmpl_name)?;
            Some(tmpl.repos)
        } else {
            None
        };

        let all_repos = config_mgr.list_repos()?;
        if all_repos.is_empty() {
            anyhow::bail!("no repos registered. Use 'zootree repo add' first.");
        }

        let selected = tui::select_multi("Select repos", &all_repos)?;
        if selected.is_empty() {
            anyhow::bail!("at least one repo must be selected");
        }

        let mut entries = Vec::new();
        for idx in selected {
            let name = &all_repos[idx];
            let repo_config = config_mgr.load_repo_config(name)?;

            let target_branch = if let Some(default) = &repo_config.default_target_branch {
                default.clone()
            } else {
                tui::input_required(&format!("Target branch for {}", name))?
            };

            entries.push(RepoEntry {
                name: name.clone(),
                target_branch,
            });
        }
        entries
    };

    let name_gen = NameGenerator::new();
    let existing: Vec<String> = config_mgr.list_workspaces(None)?
        .iter().map(|w| w.name.clone()).collect();
    let name = match &args.name {
        Some(n) => n.clone(),
        None => name_gen.generate_avoiding(&existing),
    };

    let branch = match &args.branch {
        Some(b) => b.clone(),
        None => format!("{}/{}", global.branch_prefix, name),
    };

    let workspace_dir = format!(
        "{}/{}",
        shellexpand::tilde(&global.workspace_root),
        name
    );

    let now = Local::now().to_rfc3339();

    let workspace = WorkspaceConfig {
        title,
        name: name.clone(),
        description,
        branch,
        workspace_dir,
        created_at: now.clone(),
        layout: None,
        session_mode: "standalone".into(),
        session_name: None,
        repos: repo_entries,
        events: vec![Event {
            action: "created".into(),
            timestamp: now,
            detail: None,
        }],
    };

    config_mgr.save_workspace(&WorkspaceStatus::Pending, &workspace)?;

    let recently = TemplateConfig {
        repos: workspace.repos.iter().map(|r| r.name.clone()).collect(),
        layout: workspace.layout.clone(),
        session_mode: Some(workspace.session_mode.clone()),
    };
    config_mgr.save_template("recently", &recently)?;

    println!("workspace '{}' created (pending)", name);
    println!("  branch: {}", workspace.branch);
    println!("  repos: {}", workspace.repos.iter().map(|r| format!("{}:{}", r.name, r.target_branch)).collect::<Vec<_>>().join(", "));

    Ok(())
}

#[derive(Args)]
pub struct CreateArgs {
    #[arg(long)]
    pub title: Option<String>,
    #[arg(long)]
    pub name: Option<String>,
    #[arg(long)]
    pub description: Option<String>,
    #[arg(long)]
    pub repos: Option<String>,
    #[arg(long)]
    pub branch: Option<String>,
    #[arg(long)]
    pub template: Option<String>,
}

#[derive(Args)]
pub struct ListArgs {
    #[arg(long)]
    pub status: Option<String>,
}

#[derive(Args)]
pub struct StartArgs {
    pub name: Option<String>,
    #[arg(long)]
    pub no_zellij: bool,
}

#[derive(Args)]
pub struct OpenArgs {
    pub name: Option<String>,
}

#[derive(Args)]
pub struct DoneArgs {
    pub name: Option<String>,
    #[arg(long)]
    pub no_merge: bool,
    #[arg(long)]
    pub no_clean: bool,
    #[arg(long)]
    pub push: bool,
    #[arg(long)]
    pub delete_remote: bool,
    #[arg(long)]
    pub strategy: Option<String>,
    #[arg(long)]
    pub force: bool,
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Args)]
pub struct CancelArgs {
    pub name: Option<String>,
    #[arg(long)]
    pub no_clean: bool,
    #[arg(long)]
    pub force: bool,
}
