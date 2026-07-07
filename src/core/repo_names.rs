use std::collections::HashSet;

use anyhow::Result;

use crate::config::ConfigManager;

pub fn unique_repo_name(config_mgr: &ConfigManager, base: &str) -> Result<String> {
    let existing: HashSet<String> = config_mgr.list_repos()?.into_iter().collect();
    if !existing.contains(base) {
        return Ok(base.to_string());
    }

    for i in 2.. {
        let candidate = format!("{}-{}", base, i);
        if !existing.contains(&candidate) {
            return Ok(candidate);
        }
    }

    unreachable!("unbounded repo name search should always return")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::global::HooksConfig;
    use crate::config::repo::RepoConfig;

    fn repo_config(path: &str) -> RepoConfig {
        RepoConfig {
            path: path.into(),
            default_target_branch: None,
            copy_files: Vec::new(),
            hooks: HooksConfig::default(),
            lazygit: None,
        }
    }

    #[test]
    fn unique_repo_name_returns_base_when_unused() {
        let tmp = tempfile::tempdir().unwrap();
        let mgr = ConfigManager::with_base_dir(tmp.path().join("config"));
        mgr.ensure_dirs().unwrap();

        let name = unique_repo_name(&mgr, "zootree").unwrap();

        assert_eq!(name, "zootree");
    }

    #[test]
    fn unique_repo_name_appends_two_when_base_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let mgr = ConfigManager::with_base_dir(tmp.path().join("config"));
        mgr.ensure_dirs().unwrap();
        mgr.save_repo_config("zootree", &repo_config("/repo/one"))
            .unwrap();

        let name = unique_repo_name(&mgr, "zootree").unwrap();

        assert_eq!(name, "zootree-2");
    }

    #[test]
    fn unique_repo_name_skips_existing_suffixes() {
        let tmp = tempfile::tempdir().unwrap();
        let mgr = ConfigManager::with_base_dir(tmp.path().join("config"));
        mgr.ensure_dirs().unwrap();
        mgr.save_repo_config("zootree", &repo_config("/repo/one"))
            .unwrap();
        mgr.save_repo_config("zootree-2", &repo_config("/repo/two"))
            .unwrap();

        let name = unique_repo_name(&mgr, "zootree").unwrap();

        assert_eq!(name, "zootree-3");
    }
}
