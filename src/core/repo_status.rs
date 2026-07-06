use std::path::Path;

use crate::config::workspace::RepoEntry;
use crate::config::ConfigManager;

pub fn missing_registered_repo_names(
    config_mgr: &ConfigManager,
    repos: &[RepoEntry],
) -> Vec<String> {
    repos
        .iter()
        .filter_map(|repo| match config_mgr.load_repo_config(&repo.name) {
            Ok(config) => {
                let path = shellexpand::tilde(&config.path).into_owned();
                (!Path::new(&path).exists()).then(|| repo.name.clone())
            }
            Err(_) => Some(repo.name.clone()),
        })
        .collect()
}
