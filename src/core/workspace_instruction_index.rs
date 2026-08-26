use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::config::workspace::WorkspaceConfig;

const AGENTS_FILE: &str = "AGENTS.md";
const CLAUDE_FILE: &str = "CLAUDE.md";

pub fn sync(workspace: &WorkspaceConfig) {
    let workspace_dir = expanded_workspace_dir(workspace);
    let indexes = [
        (AGENTS_FILE, render_agents_index(&workspace_dir, workspace)),
        (CLAUDE_FILE, render_claude_index(&workspace_dir, workspace)),
    ];

    for (file_name, content) in indexes {
        let path = workspace_dir.join(file_name);
        if let Err(error) = replace_atomically(&path, content.as_bytes()) {
            tracing::warn!(
                "failed to update workspace instruction index '{}': {:#}",
                path.display(),
                error
            );
        }
    }
}

fn expanded_workspace_dir(workspace: &WorkspaceConfig) -> PathBuf {
    let path = PathBuf::from(shellexpand::tilde(&workspace.workspace_dir).into_owned());
    if path.is_absolute() {
        path
    } else {
        std::env::current_dir()
            .map(|current_dir| current_dir.join(&path))
            .unwrap_or(path)
    }
}

fn render_agents_index(workspace_dir: &Path, workspace: &WorkspaceConfig) -> String {
    let entries = workspace
        .repos
        .iter()
        .filter(|repo| workspace_dir.join(&repo.name).join(AGENTS_FILE).is_file())
        .map(|repo| {
            format!(
                "- For work in `{0}/`, read and follow `{0}/{AGENTS_FILE}`.",
                repo.name
            )
        })
        .collect::<Vec<_>>();

    if entries.is_empty() {
        String::new()
    } else {
        format!(
            "# Workspace repository instructions\n\n{}\n",
            entries.join("\n")
        )
    }
}

fn render_claude_index(workspace_dir: &Path, workspace: &WorkspaceConfig) -> String {
    let entries = workspace
        .repos
        .iter()
        .filter(|repo| workspace_dir.join(&repo.name).join(CLAUDE_FILE).is_file())
        .map(|repo| format!("@{}/{CLAUDE_FILE}", repo.name))
        .collect::<Vec<_>>();

    if entries.is_empty() {
        String::new()
    } else {
        format!("{}\n", entries.join("\n"))
    }
}

fn replace_atomically(path: &Path, content: &[u8]) -> Result<()> {
    let file_name = path
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("workspace instruction index path has no file name"))?
        .to_string_lossy();
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let temporary_path = path.with_file_name(format!(
        ".{file_name}.{}.{}.tmp",
        std::process::id(),
        suffix
    ));

    let write_result = (|| -> Result<()> {
        let mut temporary_file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary_path)
            .with_context(|| {
                format!(
                    "failed to create temporary workspace instruction index '{}'",
                    temporary_path.display()
                )
            })?;
        temporary_file.write_all(content).with_context(|| {
            format!(
                "failed to write temporary workspace instruction index '{}'",
                temporary_path.display()
            )
        })?;
        temporary_file.sync_all().with_context(|| {
            format!(
                "failed to sync temporary workspace instruction index '{}'",
                temporary_path.display()
            )
        })?;
        std::fs::rename(&temporary_path, path).with_context(|| {
            format!(
                "failed to replace workspace instruction index '{}' atomically",
                path.display()
            )
        })?;
        Ok(())
    })();

    if let Err(error) = write_result {
        return match std::fs::remove_file(&temporary_path) {
            Ok(()) => Err(error),
            Err(cleanup_error) if cleanup_error.kind() == std::io::ErrorKind::NotFound => {
                Err(error)
            }
            Err(cleanup_error) => Err(anyhow::anyhow!(
                "{error:#}; additionally failed to remove temporary workspace instruction index '{}': {cleanup_error}",
                temporary_path.display()
            )),
        };
    }

    Ok(())
}
