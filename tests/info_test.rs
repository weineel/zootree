use zootree::cli::info::render_once;
use zootree::config::global::ZellijConfig;
use zootree::config::workspace::{Event, RepoEntry, WorkspaceConfig, WorkspaceStatus};

fn base_ws() -> WorkspaceConfig {
    WorkspaceConfig {
        title: "Demo title".into(),
        name: "demo".into(),
        description: String::new(),
        branch: "zootree/demo".into(),
        workspace_dir: "/tmp/demo".into(),
        created_at: "2026-05-10T14:22:00+08:00".into(),
        zellij: ZellijConfig::default(),
        repos: vec![],
        events: vec![],
    }
}

#[test]
fn render_once_includes_core_fields() {
    let out = render_once(&WorkspaceStatus::InProgress, &base_ws());
    assert!(out.contains("Workspace: Demo title (demo)"), "{}", out);
    assert!(out.contains("Status:    in_progress"), "{}", out);
    assert!(out.contains("Branch:    zootree/demo"), "{}", out);
    assert!(out.contains("Dir:       /tmp/demo"), "{}", out);
    assert!(out.contains("Repos:\n  (none)"), "{}", out);
}

#[test]
fn render_once_omits_description_when_empty() {
    let out = render_once(&WorkspaceStatus::Pending, &base_ws());
    assert!(!out.contains("Description:"), "{}", out);
}

#[test]
fn render_once_includes_description_when_present() {
    let mut ws = base_ws();
    ws.description = "line one\nline two".into();
    let out = render_once(&WorkspaceStatus::Pending, &ws);
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
    let out = render_once(&WorkspaceStatus::InProgress, &ws);
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
    let out = render_once(&WorkspaceStatus::InProgress, &ws);
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
        let out = render_once(&s, &base_ws());
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
