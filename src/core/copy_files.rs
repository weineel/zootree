use anyhow::{Context, Result};
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

        let mut matched = false;
        for entry in glob(&pattern_str)
            .with_context(|| format!("invalid copy_files pattern '{}'", pattern))?
        {
            let source = entry.with_context(|| {
                format!(
                    "copy_files pattern '{}' failed while reading matches",
                    pattern
                )
            })?;
            matched = true;
            copy_one(repo_path, worktree_path, &source)?;
        }

        if !matched {
            let plain_path = repo_path.join(pattern);
            if plain_path.exists() {
                copy_one(repo_path, worktree_path, &plain_path)?;
            } else {
                warn!("copy_files pattern '{}' matched nothing", pattern);
            }
        }
    }
    Ok(())
}

fn copy_one(repo_path: &Path, worktree_path: &Path, source: &Path) -> Result<()> {
    if source.is_dir() {
        warn!(
            "copy_files path '{}' is a directory; skipping (recursive copy is not supported)",
            source.display()
        );
        return Ok(());
    }

    let relative = source.strip_prefix(repo_path)?;
    let dest = worktree_path.join(relative);
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(source, &dest)?;
    info!("copied {} -> {}", source.display(), dest.display());
    Ok(())
}
