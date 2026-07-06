use zootree::cli::info::{render_once, render_once_with_missing_repos};
use zootree::config::global::{GlobalConfig, ZellijConfig};
use zootree::config::workspace::{Event, RepoEntry, WorkspaceConfig, WorkspaceStatus};

fn base_ws() -> WorkspaceConfig {
    WorkspaceConfig {
        title: "Demo title".into(),
        name: "demo".into(),
        description: String::new(),
        branch: "zootree/demo".into(),
        workspace_dir: "/tmp/demo".into(),
        created_at: "2026-05-10T14:22:00+08:00".into(),
        agent_cli: None,
        zellij: ZellijConfig::default(),
        repos: vec![],
        events: vec![],
    }
}

#[test]
fn render_once_includes_core_fields() {
    let out = render_once(
        &WorkspaceStatus::InProgress,
        &base_ws(),
        &GlobalConfig::default(),
    );
    assert!(out.contains("Workspace: Demo title (demo)"), "{}", out);
    assert!(out.contains("Status:    in_progress"), "{}", out);
    assert!(out.contains("Branch:    zootree/demo"), "{}", out);
    assert!(out.contains("Dir:       /tmp/demo"), "{}", out);
    assert!(out.contains("Repos:\n  (none)"), "{}", out);
}

#[test]
fn render_once_omits_description_when_empty() {
    let out = render_once(
        &WorkspaceStatus::Pending,
        &base_ws(),
        &GlobalConfig::default(),
    );
    assert!(!out.contains("Description:"), "{}", out);
}

#[test]
fn render_once_includes_description_when_present() {
    let mut ws = base_ws();
    ws.description = "line one\nline two".into();
    let out = render_once(&WorkspaceStatus::Pending, &ws, &GlobalConfig::default());
    assert!(
        out.contains("Description:\n  line one\n  line two"),
        "{}",
        out
    );
}

#[test]
fn render_once_lists_repos_with_target_branch() {
    let mut ws = base_ws();
    ws.repos = vec![
        RepoEntry {
            name: "frontend".into(),
            target_branch: Some("main".into()),
        },
        RepoEntry {
            name: "backend".into(),
            target_branch: None,
        },
    ];
    let out = render_once(&WorkspaceStatus::InProgress, &ws, &GlobalConfig::default());
    assert!(out.contains("- frontend"), "{}", out);
    assert!(out.contains("-> main"), "{}", out);
    assert!(out.contains("- backend"), "{}", out);
    assert!(out.contains("-> *"), "{}", out);
}

#[test]
fn render_once_shows_last_five_events() {
    let mut ws = base_ws();
    for i in 0..7 {
        ws.events.push(Event {
            action: format!("step-{}", i),
            timestamp: "2026-05-10T14:22:00+08:00".into(),
            detail: None,
        });
    }
    let out = render_once(&WorkspaceStatus::InProgress, &ws, &GlobalConfig::default());
    assert!(out.contains("Recent events:"), "{}", out);
    assert!(!out.contains("step-0"), "oldest trimmed: {}", out);
    assert!(!out.contains("step-1"), "oldest trimmed: {}", out);
    assert!(out.contains("step-2"), "{}", out);
    assert!(out.contains("step-6"), "{}", out);
}

#[test]
fn render_once_covers_all_statuses() {
    use WorkspaceStatus::*;
    for s in [Pending, InProgress, Done, Canceled] {
        let out = render_once(&s, &base_ws(), &GlobalConfig::default());
        let label = match s {
            Pending => "pending",
            InProgress => "in_progress",
            Done => "done",
            Canceled => "canceled",
        };
        assert!(
            out.contains(&format!("Status:    {}", label)),
            "{}: {}",
            label,
            out
        );
    }
}

#[test]
fn render_once_includes_agent_section_when_configured() {
    let ws = base_ws();
    let global = GlobalConfig {
        agent_cli: Some("claude --skip -- $prompt".into()),
        ..Default::default()
    };
    let out = render_once(&WorkspaceStatus::InProgress, &ws, &global);
    assert!(out.contains("Agent:"), "missing Agent: section:\n{}", out);
    assert!(
        out.contains("claude"),
        "missing claude in command:\n{}",
        out
    );
    assert!(
        !out.contains("Prompt:"),
        "should not include Prompt: when configured:\n{}",
        out
    );
}

#[test]
fn render_once_includes_prompt_section_when_not_configured() {
    let ws = base_ws();
    let global = GlobalConfig::default();
    let out = render_once(&WorkspaceStatus::InProgress, &ws, &global);
    assert!(out.contains("Prompt:"), "missing Prompt: section:\n{}", out);
    assert!(
        !out.contains("Agent:"),
        "should not include Agent: when unconfigured:\n{}",
        out
    );
}

#[test]
fn render_once_shows_agent_section_with_error_on_invalid_template() {
    let ws = base_ws();
    let global = GlobalConfig {
        agent_cli: Some("claude 'unclosed".into()),
        ..Default::default()
    };
    let out = render_once(&WorkspaceStatus::InProgress, &ws, &global);
    assert!(out.contains("Agent:"), "missing Agent: section:\n{}", out);
    assert!(
        out.contains("failed to parse"),
        "missing error message:\n{}",
        out
    );
}

#[test]
fn render_once_includes_alias_annotation_and_alias_section() {
    use std::collections::BTreeMap;
    let ws = base_ws();
    let mut alias_map = BTreeMap::new();
    alias_map.insert("safe".to_string(), "claude --skip -- $prompt".to_string());
    let global = GlobalConfig {
        agent_cli: Some("safe".into()),
        agent_cli_alias: alias_map,
        ..Default::default()
    };
    let out = render_once(&WorkspaceStatus::InProgress, &ws, &global);

    assert!(out.contains("Agent:"), "missing Agent: section:\n{}", out);
    assert!(
        out.contains("(via alias: safe)"),
        "missing alias annotation:\n{}",
        out
    );
    assert!(
        out.contains("Alias:\n  safe = claude --skip -- $prompt"),
        "missing single-line Alias section:\n{}",
        out
    );
}

#[test]
fn render_once_prefers_workspace_agent_over_global_default() {
    use std::collections::BTreeMap;
    let mut ws = base_ws();
    ws.agent_cli = Some("codexd_brainstorming".into());

    let mut alias_map = BTreeMap::new();
    alias_map.insert("claude".to_string(), "claude -- $prompt".to_string());
    alias_map.insert(
        "codexd_brainstorming".to_string(),
        "codexd brainstorming -- $prompt".to_string(),
    );
    let global = GlobalConfig {
        agent_cli: Some("claude".into()),
        agent_cli_alias: alias_map,
        ..Default::default()
    };
    let out = render_once(&WorkspaceStatus::InProgress, &ws, &global);

    assert!(out.contains("Agent:"), "missing Agent: section:\n{}", out);
    assert!(
        out.contains("codexd brainstorming"),
        "workspace agent should be shown:\n{}",
        out
    );
    assert!(
        out.contains("(via alias: codexd_brainstorming)"),
        "workspace agent alias should be annotated:\n{}",
        out
    );
    assert!(
        !out.contains("claude --"),
        "global default agent should not be shown:\n{}",
        out
    );
}

#[test]
fn render_once_omits_alias_section_for_literal_template() {
    let ws = base_ws();
    let global = GlobalConfig {
        agent_cli: Some("claude --skip -- $prompt".into()),
        ..Default::default()
    };
    let out = render_once(&WorkspaceStatus::InProgress, &ws, &global);

    assert!(out.contains("Agent:"), "missing Agent: section:\n{}", out);
    assert!(
        !out.contains("via alias:"),
        "should not include alias annotation:\n{}",
        out
    );
    assert!(
        !out.contains("Alias:"),
        "should not include Alias section:\n{}",
        out
    );
}

#[test]
fn render_once_omits_alias_section_on_parse_error() {
    use std::collections::BTreeMap;
    let ws = base_ws();
    let mut alias_map = BTreeMap::new();
    alias_map.insert("broken".to_string(), "claude 'unclosed".to_string());
    let global = GlobalConfig {
        agent_cli: Some("broken".into()),
        agent_cli_alias: alias_map,
        ..Default::default()
    };
    let out = render_once(&WorkspaceStatus::InProgress, &ws, &global);

    assert!(out.contains("Agent:"), "missing Agent: section:\n{}", out);
    assert!(out.contains("failed to parse"), "missing error:\n{}", out);
    assert!(
        !out.contains("via alias:"),
        "should not show alias annotation on parse error:\n{}",
        out
    );
    assert!(
        !out.contains("Alias:"),
        "should not show Alias section on parse error:\n{}",
        out
    );
}

#[test]
fn render_once_marks_missing_in_progress_repo_worktree() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir(tmp.path().join("frontend")).unwrap();
    let mut ws = base_ws();
    ws.workspace_dir = tmp.path().to_string_lossy().into_owned();
    ws.repos = vec![
        RepoEntry {
            name: "frontend".into(),
            target_branch: Some("main".into()),
        },
        RepoEntry {
            name: "backend".into(),
            target_branch: Some("main".into()),
        },
    ];

    let out = render_once(&WorkspaceStatus::InProgress, &ws, &GlobalConfig::default());

    assert!(out.contains("frontend"), "{out}");
    assert!(
        out.contains(&format!("{}/frontend", ws.workspace_dir)),
        "{out}"
    );
    assert!(
        out.contains(&format!("{}/backend (missing)", ws.workspace_dir)),
        "{out}"
    );
}

#[test]
fn render_once_marks_missing_registered_repo_path() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir(tmp.path().join("zootree-2")).unwrap();
    let mut ws = base_ws();
    ws.workspace_dir = tmp.path().to_string_lossy().into_owned();
    ws.repos = vec![RepoEntry {
        name: "zootree-2".into(),
        target_branch: Some("zootree/safe-fire".into()),
    }];

    let out = render_once_with_missing_repos(
        &WorkspaceStatus::InProgress,
        &ws,
        &GlobalConfig::default(),
        &["zootree-2".to_string()],
    );

    assert!(
        out.contains(&format!("{}/zootree-2 (missing)", ws.workspace_dir)),
        "{out}"
    );
}

#[test]
fn render_once_omits_worktree_paths_for_non_in_progress_workspace() {
    let mut ws = base_ws();
    ws.workspace_dir = "/tmp/demo".into();
    ws.repos = vec![RepoEntry {
        name: "frontend".into(),
        target_branch: Some("main".into()),
    }];

    let out = render_once(&WorkspaceStatus::Pending, &ws, &GlobalConfig::default());

    assert!(out.contains("  - frontend"), "{out}");
    assert!(!out.contains("/tmp/demo/frontend"), "{out}");
}
