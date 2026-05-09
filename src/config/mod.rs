pub mod global;
pub mod repo;
pub mod template;
pub mod workspace;

use anyhow::Result;
use std::path::PathBuf;

pub struct ConfigManager {
    pub base_dir: PathBuf,
}

impl ConfigManager {
    pub fn new() -> Result<Self> {
        let base_dir = dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("cannot find home directory"))?
            .join(".config")
            .join("zootree");
        Ok(Self { base_dir })
    }

    pub fn with_base_dir(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    pub fn ensure_dirs(&self) -> Result<()> {
        let dirs = [
            "repos",
            "layouts",
            "templates",
            "workspaces/pending",
            "workspaces/in_progress",
            "workspaces/archived/done",
            "workspaces/archived/canceled",
            "logs",
        ];
        for d in dirs {
            std::fs::create_dir_all(self.base_dir.join(d))?;
        }
        Ok(())
    }

    pub fn load_global_config(&self) -> Result<global::GlobalConfig> {
        let path = self.base_dir.join("config.toml");
        if path.exists() {
            let content = std::fs::read_to_string(&path)?;
            Ok(toml::from_str(&content)?)
        } else {
            Ok(global::GlobalConfig::default())
        }
    }

    pub fn save_global_config(&self, config: &global::GlobalConfig) -> Result<()> {
        let path = self.base_dir.join("config.toml");
        let content = toml::to_string_pretty(config)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    pub fn repos_dir(&self) -> PathBuf {
        self.base_dir.join("repos")
    }

    pub fn load_repo_config(&self, name: &str) -> Result<repo::RepoConfig> {
        let path = self.repos_dir().join(format!("{}.toml", name));
        let content = std::fs::read_to_string(&path)?;
        Ok(toml::from_str(&content)?)
    }

    pub fn save_repo_config(&self, name: &str, config: &repo::RepoConfig) -> Result<()> {
        let path = self.repos_dir().join(format!("{}.toml", name));
        let content = toml::to_string_pretty(config)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    pub fn list_repos(&self) -> Result<Vec<String>> {
        let dir = self.repos_dir();
        let mut names = Vec::new();
        if dir.exists() {
            for entry in std::fs::read_dir(dir)? {
                let entry = entry?;
                if let Some(name) = entry.path().file_stem() {
                    if entry.path().extension().is_some_and(|e| e == "toml") {
                        names.push(name.to_string_lossy().into_owned());
                    }
                }
            }
        }
        names.sort();
        Ok(names)
    }

    pub fn remove_repo_config(&self, name: &str) -> Result<()> {
        let path = self.repos_dir().join(format!("{}.toml", name));
        std::fs::remove_file(path)?;
        Ok(())
    }

    fn workspace_status_dir(&self, status: &workspace::WorkspaceStatus) -> PathBuf {
        match status {
            workspace::WorkspaceStatus::Pending => self.base_dir.join("workspaces/pending"),
            workspace::WorkspaceStatus::InProgress => self.base_dir.join("workspaces/in_progress"),
            workspace::WorkspaceStatus::Done => self.base_dir.join("workspaces/archived/done"),
            workspace::WorkspaceStatus::Canceled => {
                self.base_dir.join("workspaces/archived/canceled")
            }
        }
    }

    pub fn save_workspace(
        &self,
        status: &workspace::WorkspaceStatus,
        config: &workspace::WorkspaceConfig,
    ) -> Result<()> {
        let dir = self.workspace_status_dir(status);
        let path = dir.join(format!("{}.toml", config.name));
        let content = toml::to_string_pretty(config)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    pub fn load_workspace(
        &self,
        name: &str,
    ) -> Result<(workspace::WorkspaceStatus, workspace::WorkspaceConfig)> {
        use workspace::WorkspaceStatus::*;
        for status in [Pending, InProgress, Done, Canceled] {
            let path = self
                .workspace_status_dir(&status)
                .join(format!("{}.toml", name));
            if path.exists() {
                let content = std::fs::read_to_string(&path)?;
                let config: workspace::WorkspaceConfig = toml::from_str(&content)?;
                return Ok((status, config));
            }
        }
        anyhow::bail!("workspace '{}' not found", name)
    }

    pub fn move_workspace(
        &self,
        name: &str,
        from: &workspace::WorkspaceStatus,
        to: &workspace::WorkspaceStatus,
    ) -> Result<()> {
        let from_path = self
            .workspace_status_dir(from)
            .join(format!("{}.toml", name));
        let to_path = self.workspace_status_dir(to).join(format!("{}.toml", name));
        std::fs::rename(from_path, to_path)?;
        Ok(())
    }

    pub fn list_workspaces(
        &self,
        status: Option<&[workspace::WorkspaceStatus]>,
    ) -> Result<Vec<workspace::WorkspaceConfig>> {
        use workspace::WorkspaceStatus::*;
        let statuses = match status {
            Some(s) => s.to_vec(),
            None => vec![Pending, InProgress, Done, Canceled],
        };
        let mut workspaces = Vec::new();
        for s in statuses {
            let dir = self.workspace_status_dir(&s);
            if dir.exists() {
                for entry in std::fs::read_dir(&dir)? {
                    let entry = entry?;
                    if entry.path().extension().is_some_and(|e| e == "toml") {
                        let content = std::fs::read_to_string(entry.path())?;
                        if let Ok(config) = toml::from_str(&content) {
                            workspaces.push(config);
                        }
                    }
                }
            }
        }
        Ok(workspaces)
    }

    pub fn delete_workspace_config(
        &self,
        name: &str,
        status: &workspace::WorkspaceStatus,
    ) -> Result<()> {
        let path = self
            .workspace_status_dir(status)
            .join(format!("{}.toml", name));
        std::fs::remove_file(path)?;
        Ok(())
    }

    fn templates_dir(&self) -> PathBuf {
        self.base_dir.join("templates")
    }

    pub fn load_template(&self, name: &str) -> Result<template::TemplateConfig> {
        let path = self.templates_dir().join(format!("{}.toml", name));
        let content = std::fs::read_to_string(&path)?;
        Ok(toml::from_str(&content)?)
    }

    pub fn save_template(&self, name: &str, config: &template::TemplateConfig) -> Result<()> {
        let path = self.templates_dir().join(format!("{}.toml", name));
        let content = toml::to_string_pretty(config)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    pub fn list_templates(&self) -> Result<Vec<String>> {
        let dir = self.templates_dir();
        let mut names = Vec::new();
        if dir.exists() {
            for entry in std::fs::read_dir(dir)? {
                let entry = entry?;
                if let Some(name) = entry.path().file_stem() {
                    if entry.path().extension().is_some_and(|e| e == "toml") {
                        names.push(name.to_string_lossy().into_owned());
                    }
                }
            }
        }
        names.sort();
        Ok(names)
    }
}
