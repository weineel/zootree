use crate::config::workspace::WorkspaceStatus;
use crate::config::ConfigManager;
use clap_complete::CompletionCandidate;
use std::ffi::OsStr;

#[derive(Debug, Clone, Copy)]
pub enum WorkspaceFilter {
    Pending,
    InProgress,
    Active, // pending or in_progress
    Any,
}

impl WorkspaceFilter {
    fn statuses(&self) -> &'static [WorkspaceStatus] {
        match self {
            WorkspaceFilter::Pending => &[WorkspaceStatus::Pending],
            WorkspaceFilter::InProgress => &[WorkspaceStatus::InProgress],
            WorkspaceFilter::Active => &[WorkspaceStatus::Pending, WorkspaceStatus::InProgress],
            WorkspaceFilter::Any => &[
                WorkspaceStatus::Pending,
                WorkspaceStatus::InProgress,
                WorkspaceStatus::Done,
                WorkspaceStatus::Canceled,
            ],
        }
    }
}

pub fn complete_workspace_with(
    mgr: &ConfigManager,
    current: &OsStr,
    filter: WorkspaceFilter,
) -> Vec<CompletionCandidate> {
    let prefix = current.to_string_lossy();
    let Ok(workspaces) = mgr.list_workspaces(Some(filter.statuses())) else {
        return vec![];
    };
    workspaces
        .into_iter()
        .filter(|ws| ws.name.starts_with(prefix.as_ref()))
        .map(|ws| {
            let status = mgr
                .load_workspace(&ws.name)
                .map(|(s, _)| format!("{:?}", s).to_lowercase())
                .unwrap_or_default();
            let help = if status.is_empty() {
                ws.title.clone()
            } else {
                format!("{} ({})", ws.title, status)
            };
            CompletionCandidate::new(ws.name).help(Some(help.into()))
        })
        .collect()
}

pub fn complete_workspace(current: &OsStr, filter: WorkspaceFilter) -> Vec<CompletionCandidate> {
    let Ok(mgr) = ConfigManager::new() else {
        return vec![];
    };
    complete_workspace_with(&mgr, current, filter)
}

pub fn complete_repo_with(mgr: &ConfigManager, current: &OsStr) -> Vec<CompletionCandidate> {
    let prefix = current.to_string_lossy();
    let Ok(names) = mgr.list_repos() else {
        return vec![];
    };
    names
        .into_iter()
        .filter(|n| n.starts_with(prefix.as_ref()))
        .map(|name| {
            let help = mgr
                .load_repo_config(&name)
                .map(|c| c.path)
                .unwrap_or_default();
            let mut cand = CompletionCandidate::new(&name);
            if !help.is_empty() {
                cand = cand.help(Some(help.into()));
            }
            cand
        })
        .collect()
}

pub fn complete_repo(current: &OsStr) -> Vec<CompletionCandidate> {
    let Ok(mgr) = ConfigManager::new() else {
        return vec![];
    };
    complete_repo_with(&mgr, current)
}

pub fn complete_template_with(mgr: &ConfigManager, current: &OsStr) -> Vec<CompletionCandidate> {
    let prefix = current.to_string_lossy();
    let Ok(names) = mgr.list_templates() else {
        return vec![];
    };
    names
        .into_iter()
        .filter(|n| n.starts_with(prefix.as_ref()))
        .map(|name| {
            let help = mgr
                .load_template(&name)
                .map(|t| t.repos.join(", "))
                .unwrap_or_default();
            let mut cand = CompletionCandidate::new(&name);
            if !help.is_empty() {
                cand = cand.help(Some(help.into()));
            }
            cand
        })
        .collect()
}

pub fn complete_template(current: &OsStr) -> Vec<CompletionCandidate> {
    let Ok(mgr) = ConfigManager::new() else {
        return vec![];
    };
    complete_template_with(&mgr, current)
}

pub fn complete_repos_list_with(mgr: &ConfigManager, current: &OsStr) -> Vec<CompletionCandidate> {
    let raw = current.to_string_lossy();
    // Split on the last comma to find the segment being edited.
    let (prefix, segment) = match raw.rfind(',') {
        Some(idx) => (&raw[..=idx], &raw[idx + 1..]),
        None => ("", raw.as_ref()),
    };

    // If the segment already contains ':', user is typing a branch name; don't suggest.
    if segment.contains(':') {
        return vec![];
    }

    let Ok(names) = mgr.list_repos() else {
        return vec![];
    };
    names
        .into_iter()
        .filter(|n| n.starts_with(segment))
        .map(|name| {
            let help = mgr
                .load_repo_config(&name)
                .map(|c| c.path)
                .unwrap_or_default();
            let value = format!("{}{}", prefix, name);
            let mut cand = CompletionCandidate::new(value);
            if !help.is_empty() {
                cand = cand.help(Some(help.into()));
            }
            cand
        })
        .collect()
}

pub fn complete_repos_list(current: &OsStr) -> Vec<CompletionCandidate> {
    let Ok(mgr) = ConfigManager::new() else {
        return vec![];
    };
    complete_repos_list_with(&mgr, current)
}
