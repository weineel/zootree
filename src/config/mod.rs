pub mod global;
pub mod repo;

use std::path::PathBuf;
use anyhow::Result;

pub struct ConfigManager {
    pub base_dir: PathBuf,
}

impl ConfigManager {
    pub fn new() -> Result<Self> {
        let base_dir = dirs::config_dir()
            .ok_or_else(|| anyhow::anyhow!("cannot find config directory"))?
            .join("zootree");
        Ok(Self { base_dir })
    }

    pub fn with_base_dir(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    pub fn ensure_dirs(&self) -> Result<()> {
        let dirs = [
            "repos", "layouts", "templates",
            "workspaces/pending", "workspaces/in_progress",
            "workspaces/archived/done", "workspaces/archived/canceled",
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
                    if entry.path().extension().map_or(false, |e| e == "toml") {
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
}
