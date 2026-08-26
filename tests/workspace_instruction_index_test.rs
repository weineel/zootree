use std::fs;

use tempfile::TempDir;
use zootree::config::global::MultiplexerConfig;
use zootree::config::workspace::{RepoEntry, WorkspaceConfig};
use zootree::core::workspace_instruction_index;

fn workspace(temp: &TempDir, repos: &[&str]) -> WorkspaceConfig {
    WorkspaceConfig {
        title: "Instruction indexes".into(),
        name: "calm-river".into(),
        description: String::new(),
        branch: "zootree/calm-river".into(),
        workspace_dir: temp.path().to_string_lossy().into_owned(),
        created_at: "2026-08-26T10:00:00+08:00".into(),
        agent_cli: None,
        multiplexer: MultiplexerConfig::default(),
        multiplexer_state: Default::default(),
        repos: repos
            .iter()
            .map(|name| RepoEntry {
                name: (*name).into(),
                target_branch: Some("main".into()),
            })
            .collect(),
        events: Vec::new(),
    }
}

#[test]
fn sync_writes_agents_index_in_workspace_membership_order() {
    let temp = TempDir::new().unwrap();
    let workspace = workspace(&temp, &["frontend", "backend", "docs"]);
    for repo in ["frontend", "backend", "docs"] {
        fs::create_dir(temp.path().join(repo)).unwrap();
    }
    fs::write(temp.path().join("frontend/AGENTS.md"), "frontend rules").unwrap();
    fs::write(temp.path().join("backend/AGENTS.md"), "backend rules").unwrap();

    workspace_instruction_index::sync(&workspace);

    assert_eq!(
        fs::read_to_string(temp.path().join("AGENTS.md")).unwrap(),
        "# Workspace repository instructions\n\n\
- For work in `frontend/`, read and follow `frontend/AGENTS.md`.\n\
- For work in `backend/`, read and follow `backend/AGENTS.md`.\n"
    );
}

#[test]
fn sync_writes_claude_imports_for_existing_repo_files_only() {
    let temp = TempDir::new().unwrap();
    let workspace = workspace(&temp, &["frontend", "backend", "docs"]);
    for repo in ["frontend", "backend", "docs"] {
        fs::create_dir(temp.path().join(repo)).unwrap();
    }
    fs::write(temp.path().join("frontend/CLAUDE.md"), "frontend rules").unwrap();
    fs::write(temp.path().join("docs/CLAUDE.md"), "docs rules").unwrap();

    workspace_instruction_index::sync(&workspace);

    assert_eq!(
        fs::read_to_string(temp.path().join("CLAUDE.md")).unwrap(),
        "@frontend/CLAUDE.md\n@docs/CLAUDE.md\n"
    );
}

#[test]
fn sync_replaces_existing_indexes_with_empty_files_when_only_nested_sources_exist() {
    let temp = TempDir::new().unwrap();
    let workspace = workspace(&temp, &["frontend"]);
    fs::create_dir_all(temp.path().join("frontend/packages/app")).unwrap();
    fs::write(
        temp.path().join("frontend/packages/app/AGENTS.md"),
        "nested rules",
    )
    .unwrap();
    fs::write(temp.path().join("AGENTS.md"), "manual workspace rules").unwrap();
    fs::write(temp.path().join("CLAUDE.md"), "manual workspace rules").unwrap();

    workspace_instruction_index::sync(&workspace);

    assert_eq!(fs::read(temp.path().join("AGENTS.md")).unwrap(), b"");
    assert_eq!(fs::read(temp.path().join("CLAUDE.md")).unwrap(), b"");
}

#[test]
fn sync_continues_with_claude_when_agents_replace_fails() {
    let temp = TempDir::new().unwrap();
    let workspace = workspace(&temp, &["frontend"]);
    fs::create_dir_all(temp.path().join("frontend")).unwrap();
    fs::write(temp.path().join("frontend/CLAUDE.md"), "frontend rules").unwrap();
    fs::create_dir(temp.path().join("AGENTS.md")).unwrap();

    workspace_instruction_index::sync(&workspace);

    assert_eq!(
        fs::read_to_string(temp.path().join("CLAUDE.md")).unwrap(),
        "@frontend/CLAUDE.md\n"
    );
}
