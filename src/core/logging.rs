use std::path::PathBuf;

use crate::config::{global::GlobalConfig, ConfigManager};

pub const LOG_FILE_NAME: &str = "zootree.log";

pub fn default_log_dir(config_mgr: &ConfigManager) -> PathBuf {
    config_mgr.base_dir.join("logs")
}

pub fn resolve_log_dir(config_mgr: &ConfigManager, global: &GlobalConfig) -> PathBuf {
    global
        .log
        .dir
        .as_deref()
        .map(expand_log_dir)
        .unwrap_or_else(|| default_log_dir(config_mgr))
}

pub fn resolve_log_file_path(config_mgr: &ConfigManager, global: &GlobalConfig) -> PathBuf {
    resolve_log_dir(config_mgr, global).join(LOG_FILE_NAME)
}

fn expand_log_dir(dir: &str) -> PathBuf {
    PathBuf::from(shellexpand::tilde(dir).into_owned())
}
