use anyhow::Result;
use glob::glob;
use std::path::Path;
use tracing::{info, warn};

pub fn merge_copy_files(global: &[String], repo: &[String]) -> Vec<String> {
    let mut merged = global.to_vec();
    for f in repo {
        if !merged.contains(f) {
            merged.push(f.clone());
        }
    }
    merged
}

pub fn copy_files_to_worktree(
    repo_path: &Path,
    worktree_path: &Path,
    patterns: &[String],
) -> Result<()> {
    for pattern in patterns {
        let full_pattern = repo_path.join(pattern);
        let pattern_str = full_pattern.to_string_lossy();

        let matches: Vec<_> = glob(&pattern_str)?.filter_map(|r| r.ok()).collect();

        if matches.is_empty() {
            let plain_path = repo_path.join(pattern);
            if plain_path.exists() {
                let relative = plain_path.strip_prefix(repo_path)?;
                let dest = worktree_path.join(relative);
                if let Some(parent) = dest.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::copy(&plain_path, &dest)?;
                info!("copied {} -> {}", plain_path.display(), dest.display());
            } else {
                warn!("copy_files pattern '{}' matched nothing", pattern);
            }
            continue;
        }

        for source in matches {
            let relative = source.strip_prefix(repo_path)?;
            let dest = worktree_path.join(relative);
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(&source, &dest)?;
            info!("copied {} -> {}", source.display(), dest.display());
        }
    }
    Ok(())
}
