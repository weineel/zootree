use std::os::unix::process::ExitStatusExt;
use std::process::{ExitStatus, Output};
use zootree::config::global::{HooksConfig, MultiplexerConfig};
use zootree::config::repo::RepoConfig;
use zootree::config::workspace::{RepoEntry, WorkspaceConfig, WorkspaceStatus};
use zootree::config::ConfigManager;
use zootree::core::workspace_repository::{AddRepositoryRequest, TerminalUpdate};
use zootree::runner::MockRunner;
use zootree::service::add_repo::add_repo_to_workspace;
use zootree::service::context::CollectingSink;
use zootree::service::create::{create_workspace, CreateRequest};
use zootree::service::prune::{prune_workspaces, PruneRequest, PruneTarget};
use zootree::service::repos::{register_repo, remove_repo, RegisterRepoRequest, RemoveRepoRequest};
use zootree::service::{EnvSnapshot, RequestContext, ServiceEvent};

fn success(stdout: &str) -> Output {
    Output {
        status: ExitStatus::from_raw(0),
        stdout: stdout.as_bytes().to_vec(),
        stderr: Vec::new(),
    }
}

fn setup() -> (tempfile::TempDir, ConfigManager) {
    let tmp = tempfile::tempdir().unwrap();
    let mgr = ConfigManager::with_base_dir(tmp.path().join("config"));
    mgr.ensure_dirs().unwrap();
    (tmp, mgr)
}

fn repo_config(path: &str) -> RepoConfig {
    RepoConfig {
        path: path.into(),
        default_target_branch: Some("main".into()),
        copy_files: Vec::new(),
        hooks: HooksConfig::default(),
        lazygit: None,
    }
}

fn workspace(name: &str, dir: &std::path::Path, repos: &[&str]) -> WorkspaceConfig {
    WorkspaceConfig {
        title: "Title".into(),
        name: name.into(),
        description: String::new(),
        branch: format!("zootree/{name}"),
        workspace_dir: dir.to_string_lossy().into_owned(),
        created_at: "2026-06-29T10:00:00+08:00".into(),
        agent_cli: None,
        multiplexer: MultiplexerConfig::default(),
        multiplexer_state: Default::default(),
        repos: repos
            .iter()
            .map(|r| RepoEntry {
                name: (*r).into(),
                target_branch: Some("main".into()),
            })
            .collect(),
        events: Vec::new(),
    }
}

fn steps(sink: &CollectingSink) -> Vec<String> {
    sink.events()
        .into_iter()
        .filter_map(|e| match e {
            ServiceEvent::Step(s) => Some(s),
            _ => None,
        })
        .collect()
}

// ---- create ----

#[test]
fn create_without_title_errors_and_saves_nothing() {
    let (_tmp, mgr) = setup();
    mgr.save_repo_config("api", &repo_config("/repos/api"))
        .unwrap();
    let sink = CollectingSink::new();
    let ctx = RequestContext::new(&sink, EnvSnapshot::default());

    let err = create_workspace(
        &mgr,
        &MockRunner::new(),
        &ctx,
        &CreateRequest {
            name: Some("calm".into()),
            repos: Some(vec![("api".into(), None)]),
            ..Default::default()
        },
    )
    .unwrap_err();

    assert!(err.to_string().contains("title is required"), "{err:#}");
    assert!(mgr
        .list_workspaces(None::<&[WorkspaceStatus]>)
        .unwrap()
        .is_empty());
    assert!(sink.events().is_empty());
}

#[test]
fn create_without_repos_or_template_errors() {
    let (_tmp, mgr) = setup();
    let sink = CollectingSink::new();
    let ctx = RequestContext::new(&sink, EnvSnapshot::default());

    let err = create_workspace(
        &mgr,
        &MockRunner::new(),
        &ctx,
        &CreateRequest {
            title: Some("T".into()),
            ..Default::default()
        },
    )
    .unwrap_err();

    assert!(err.to_string().contains("repos or a template"), "{err:#}");
}

#[test]
fn create_saves_pending_workspace_and_emits_events() {
    let (_tmp, mgr) = setup();
    mgr.save_repo_config("api", &repo_config("/repos/api"))
        .unwrap();
    let sink = CollectingSink::new();
    let ctx = RequestContext::new(&sink, EnvSnapshot::default());

    let outcome = create_workspace(
        &mgr,
        &MockRunner::new(),
        &ctx,
        &CreateRequest {
            title: Some("Fix".into()),
            name: Some("calm".into()),
            repos: Some(vec![("api".into(), Some("develop".into()))]),
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(outcome.workspace.name, "calm");
    let (status, saved) = mgr.load_workspace("calm").unwrap();
    assert_eq!(status, WorkspaceStatus::Pending);
    assert_eq!(saved.repos[0].target_branch.as_deref(), Some("develop"));
    assert_eq!(mgr.load_template("recently").unwrap().repos, vec!["api"]);
    assert_eq!(
        sink.events().first(),
        Some(&ServiceEvent::Started {
            operation: "create",
            workspace: "calm".into()
        })
    );
    let steps = steps(&sink);
    assert_eq!(steps[0], "workspace 'calm' created (pending)");
    assert_eq!(steps[1], "  branch: zootree/calm");
    assert_eq!(steps[2], "  repos: api:develop");
}

#[test]
fn create_rejects_duplicate_name_and_unregistered_repo() {
    let (tmp, mgr) = setup();
    mgr.save_repo_config("api", &repo_config("/repos/api"))
        .unwrap();
    mgr.save_workspace(
        &WorkspaceStatus::Pending,
        &workspace("calm", &tmp.path().join("ws"), &["api"]),
    )
    .unwrap();
    let sink = CollectingSink::new();
    let ctx = RequestContext::new(&sink, EnvSnapshot::default());
    let request = |name: &str, repo: &str| CreateRequest {
        title: Some("T".into()),
        name: Some(name.into()),
        repos: Some(vec![(repo.into(), None)]),
        ..Default::default()
    };

    let err =
        create_workspace(&mgr, &MockRunner::new(), &ctx, &request("calm", "api")).unwrap_err();
    assert!(err.to_string().contains("already exists"), "{err:#}");
    let err =
        create_workspace(&mgr, &MockRunner::new(), &ctx, &request("other", "nope")).unwrap_err();
    assert!(err.to_string().contains("not registered"), "{err:#}");
}

// ---- add-repo ----

#[test]
fn add_repo_emits_result_steps_and_persists() {
    let (tmp, mgr) = setup();
    mgr.save_repo_config("backend", &repo_config("/repos/backend"))
        .unwrap();
    mgr.save_workspace(
        &WorkspaceStatus::InProgress,
        &workspace("ws", &tmp.path().join("ws"), &["frontend"]),
    )
    .unwrap();
    let runner = MockRunner::new();
    for out in ["refs/heads/main\n", "", "", ""] {
        runner.push_response(success(out));
    }
    let sink = CollectingSink::new();
    let ctx = RequestContext::new(&sink, EnvSnapshot::default());

    let result = add_repo_to_workspace(
        &mgr,
        &Default::default(),
        &runner,
        &ctx,
        &AddRepositoryRequest {
            workspace: "ws".into(),
            repo: "backend".into(),
            target_branch: None,
        },
    )
    .unwrap();

    assert_eq!(result.terminal, TerminalUpdate::Absent);
    let steps = steps(&sink);
    assert_eq!(steps[0], "repository 'backend' added to workspace 'ws'");
    assert_eq!(steps[1], "  target branch: main");
    assert_eq!(steps[4], "  terminal: environment absent; skipped");
    assert_eq!(mgr.load_workspace("ws").unwrap().1.repos.len(), 2);
}

#[test]
fn add_repo_missing_parameters_and_wrong_status_error() {
    let (tmp, mgr) = setup();
    mgr.save_workspace(
        &WorkspaceStatus::Pending,
        &workspace("pend", &tmp.path().join("ws"), &[]),
    )
    .unwrap();
    let sink = CollectingSink::new();
    let ctx = RequestContext::new(&sink, EnvSnapshot::default());
    let runner = MockRunner::new();
    let call = |workspace: &str, repo: &str| {
        add_repo_to_workspace(
            &mgr,
            &Default::default(),
            &runner,
            &ctx,
            &AddRepositoryRequest {
                workspace: workspace.into(),
                repo: repo.into(),
                target_branch: None,
            },
        )
        .unwrap_err()
        .to_string()
    };

    assert!(call("", "api").contains("workspace name is required"));
    assert!(call("pend", "").contains("must not be empty"));
    assert!(call("pend", "api").contains("not in_progress"));
    assert!(steps(&sink).is_empty());
}

// ---- prune ----

#[test]
fn prune_all_removes_archived_workspaces_and_directories() {
    let (tmp, mgr) = setup();
    let done_dir = tmp.path().join("done-ws");
    std::fs::create_dir_all(&done_dir).unwrap();
    mgr.save_workspace(&WorkspaceStatus::Done, &workspace("d", &done_dir, &[]))
        .unwrap();
    mgr.save_workspace(
        &WorkspaceStatus::Canceled,
        &workspace("c", &tmp.path().join("missing"), &[]),
    )
    .unwrap();
    mgr.save_workspace(
        &WorkspaceStatus::InProgress,
        &workspace("live", &tmp.path().join("live"), &[]),
    )
    .unwrap();
    let sink = CollectingSink::new();
    let ctx = RequestContext::new(&sink, EnvSnapshot::default());

    let outcome = prune_workspaces(
        &mgr,
        &ctx,
        &PruneRequest {
            all: true,
            targets: Vec::new(),
        },
    )
    .unwrap();

    assert_eq!(outcome.pruned, vec!["d", "c"]);
    assert!(!done_dir.exists());
    assert!(mgr.load_workspace("live").is_ok());
    assert!(mgr.load_workspace("d").is_err());
    assert_eq!(
        steps(&sink),
        vec![
            format!("  removed directory: {}", done_dir.display()),
            "  pruned: d".to_string(),
            "  pruned: c".to_string(),
            "2 workspace(s) pruned".to_string(),
        ]
    );
}

#[test]
fn prune_reports_empty_nothing_selected_and_unknown_target() {
    let (tmp, mgr) = setup();
    let sink = CollectingSink::new();
    let ctx = RequestContext::new(&sink, EnvSnapshot::default());
    let all = PruneRequest {
        all: true,
        targets: Vec::new(),
    };
    prune_workspaces(&mgr, &ctx, &all).unwrap();
    assert_eq!(steps(&sink), vec!["no archived workspaces to prune"]);

    mgr.save_workspace(
        &WorkspaceStatus::Done,
        &workspace("d", &tmp.path().join("x"), &[]),
    )
    .unwrap();
    prune_workspaces(&mgr, &ctx, &PruneRequest::default()).unwrap();
    assert_eq!(steps(&sink)[1], "nothing selected");
    assert!(mgr.load_workspace("d").is_ok());

    let err = prune_workspaces(
        &mgr,
        &ctx,
        &PruneRequest {
            all: false,
            targets: vec![PruneTarget {
                name: "ghost".into(),
                status: WorkspaceStatus::Done,
            }],
        },
    )
    .unwrap_err();
    assert!(err.to_string().contains("'ghost' not found"));
}

// ---- repo register / remove ----

#[test]
fn register_repo_defaults_name_and_deduplicates() {
    let (tmp, mgr) = setup();
    let repo_dir = tmp.path().join("my-repo");
    std::fs::create_dir_all(&repo_dir).unwrap();
    let sink = CollectingSink::new();
    let ctx = RequestContext::new(&sink, EnvSnapshot::default());
    let request = RegisterRepoRequest {
        name: None,
        path: repo_dir.to_string_lossy().into_owned(),
        default_target_branch: Some("develop".into()),
    };

    let first = register_repo(&mgr, &ctx, &request).unwrap();
    let second = register_repo(&mgr, &ctx, &request).unwrap();

    assert_eq!(first.name, "my-repo");
    assert_eq!(second.name, "my-repo-2");
    let config = mgr.load_repo_config("my-repo").unwrap();
    assert_eq!(config.default_target_branch.as_deref(), Some("develop"));
    assert_eq!(
        steps(&sink)[0],
        format!("repo 'my-repo' registered at {}", first.path)
    );
    assert!(register_repo(&mgr, &ctx, &RegisterRepoRequest::default()).is_err());
}

#[test]
fn remove_repo_deletes_config_and_requires_name() {
    let (_tmp, mgr) = setup();
    mgr.save_repo_config("api", &repo_config("/repos/api"))
        .unwrap();
    let sink = CollectingSink::new();
    let ctx = RequestContext::new(&sink, EnvSnapshot::default());

    let err = remove_repo(&mgr, &ctx, &RemoveRepoRequest::default()).unwrap_err();
    assert!(err.to_string().contains("name is required"));
    remove_repo(&mgr, &ctx, &RemoveRepoRequest { name: "api".into() }).unwrap();

    assert!(mgr.list_repos().unwrap().is_empty());
    assert_eq!(steps(&sink), vec!["repo 'api' removed"]);
    assert!(remove_repo(&mgr, &ctx, &RemoveRepoRequest { name: "api".into() }).is_err());
}
