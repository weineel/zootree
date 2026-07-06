use std::path::Path;

use crate::config::workspace::WorkspaceConfig;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoWorktreeStatus {
    pub repo_name: String,
    pub worktree_path: String,
    pub exists: bool,
}

pub fn repo_worktree_statuses(
    workspace: &WorkspaceConfig,
    workspace_dir: &str,
) -> Vec<RepoWorktreeStatus> {
    workspace
        .repos
        .iter()
        .map(|repo| {
            let worktree_path = Path::new(workspace_dir).join(&repo.name);
            RepoWorktreeStatus {
                repo_name: repo.name.clone(),
                exists: worktree_path.exists(),
                worktree_path: worktree_path.to_string_lossy().into_owned(),
            }
        })
        .collect()
}

pub fn missing_worktrees(statuses: &[RepoWorktreeStatus]) -> Vec<&RepoWorktreeStatus> {
    statuses.iter().filter(|status| !status.exists).collect()
}

pub fn format_missing_worktrees_error(
    workspace_name: &str,
    statuses: &[RepoWorktreeStatus],
) -> String {
    let missing = missing_worktrees(statuses);
    let details = missing
        .iter()
        .map(|status| format!("{} ({})", status.repo_name, status.worktree_path))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "workspace '{}' is missing worktrees: {}",
        workspace_name, details
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::global::ZellijConfig;
    use crate::config::workspace::{RepoEntry, WorkspaceConfig};

    fn workspace(workspace_dir: &str) -> WorkspaceConfig {
        WorkspaceConfig {
            title: "Demo".into(),
            name: "demo".into(),
            description: String::new(),
            branch: "zootree/demo".into(),
            workspace_dir: workspace_dir.into(),
            created_at: "2026-07-02T10:00:00+08:00".into(),
            agent_cli: None,
            zellij: ZellijConfig::default(),
            repos: vec![
                RepoEntry {
                    name: "frontend".into(),
                    target_branch: Some("main".into()),
                },
                RepoEntry {
                    name: "backend".into(),
                    target_branch: Some("main".into()),
                },
            ],
            events: Vec::new(),
        }
    }

    #[test]
    fn repo_worktree_statuses_reports_existing_and_missing_repos() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("frontend")).unwrap();
        let ws = workspace(&tmp.path().to_string_lossy());

        let statuses = repo_worktree_statuses(&ws, &ws.workspace_dir);

        assert_eq!(statuses[0].repo_name, "frontend");
        assert!(statuses[0].exists);
        assert_eq!(statuses[1].repo_name, "backend");
        assert!(!statuses[1].exists);
    }

    #[test]
    fn repo_worktree_statuses_reports_all_missing_when_workspace_dir_is_absent() {
        let tmp = tempfile::tempdir().unwrap();
        let missing_dir = tmp.path().join("missing");
        let ws = workspace(&missing_dir.to_string_lossy());

        let statuses = repo_worktree_statuses(&ws, &ws.workspace_dir);

        assert_eq!(missing_worktrees(&statuses).len(), 2);
    }

    #[test]
    fn repo_worktree_statuses_normalizes_trailing_slash_in_workspace_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let workspace_dir = format!("{}/", tmp.path().to_string_lossy());
        let ws = workspace(&workspace_dir);

        let statuses = repo_worktree_statuses(&ws, &ws.workspace_dir);

        assert_eq!(
            statuses[0].worktree_path,
            tmp.path().join("frontend").to_string_lossy()
        );
        assert!(!statuses[0].worktree_path.contains("//"));
    }

    #[test]
    fn format_missing_worktrees_error_lists_repo_names_and_paths() {
        let statuses = vec![RepoWorktreeStatus {
            repo_name: "backend".into(),
            worktree_path: "/tmp/demo/backend".into(),
            exists: false,
        }];

        let message = format_missing_worktrees_error("demo", &statuses);

        assert_eq!(
            message,
            "workspace 'demo' is missing worktrees: backend (/tmp/demo/backend)"
        );
    }
}
