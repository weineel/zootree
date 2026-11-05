//! Registering and removing repositories.

use crate::config::global::HooksConfig;
use crate::config::repo::RepoConfig;
use crate::config::ConfigManager;
use crate::core::repo_names::unique_repo_name;
use crate::service::RequestContext;
use anyhow::Result;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RegisterRepoRequest {
    /// Defaults to the directory name of `path`.
    pub name: Option<String>,
    pub path: String,
    pub default_target_branch: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisterRepoOutcome {
    pub name: String,
    pub path: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RemoveRepoRequest {
    pub name: String,
}

pub fn repo_add_base_name(name: &Option<String>, path: &str) -> String {
    name.clone().unwrap_or_else(|| {
        std::path::Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| path.to_string())
    })
}

pub fn resolve_repo_add_name(
    config_mgr: &ConfigManager,
    name: &Option<String>,
    path: &str,
) -> Result<String> {
    let base = repo_add_base_name(name, path);
    unique_repo_name(config_mgr, &base)
}

pub fn register_repo(
    config_mgr: &ConfigManager,
    ctx: &RequestContext<'_>,
    req: &RegisterRepoRequest,
) -> Result<RegisterRepoOutcome> {
    if req.path.is_empty() {
        anyhow::bail!("repository path is required");
    }
    config_mgr.ensure_dirs()?;
    let expanded = shellexpand::tilde(&req.path).into_owned();
    let abs_path =
        std::fs::canonicalize(&expanded).unwrap_or_else(|_| std::path::PathBuf::from(&expanded));

    let repo_name = resolve_repo_add_name(config_mgr, &req.name, &req.path)?;

    let repo_config = RepoConfig {
        path: abs_path.to_string_lossy().into_owned(),
        default_target_branch: req.default_target_branch.clone(),
        copy_files: Vec::new(),
        hooks: HooksConfig::default(),
        lazygit: None,
    };
    config_mgr.save_repo_config(&repo_name, &repo_config)?;
    ctx.step(format!(
        "repo '{}' registered at {}",
        repo_name,
        abs_path.display()
    ));
    Ok(RegisterRepoOutcome {
        name: repo_name,
        path: abs_path.to_string_lossy().into_owned(),
    })
}

pub fn remove_repo(
    config_mgr: &ConfigManager,
    ctx: &RequestContext<'_>,
    req: &RemoveRepoRequest,
) -> Result<()> {
    if req.name.is_empty() {
        anyhow::bail!("repository name is required");
    }
    config_mgr.ensure_dirs()?;
    config_mgr.remove_repo_config(&req.name)?;
    ctx.step(format!("repo '{}' removed", req.name));
    Ok(())
}
