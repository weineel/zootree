pub mod global;

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
}
