use std::os::unix::process::ExitStatusExt;
use std::process::{ExitStatus, Output};
use tempfile::TempDir;
use zootree::cli::create_flow::{
    build_repo_draft_entries, create_args_need_wizard, discover_current_repo_candidate,
    draft_from_args, persist_selected_pending_repos, resolve_agent_cli_for_draft,
    workspace_from_draft, AfterCreateMode, CreateDraft, CreateDraftError, CreateWizardLayout,
    CreateWizardOutput, CurrentRepoCandidate, RepoDraftEntry, RepoDraftSource,
};
use zootree::cli::workspace::CreateArgs;
use zootree::config::global::{GlobalConfig, MultiplexerConfig, MultiplexerKind};
use zootree::config::repo::RepoConfig;
use zootree::config::template::TemplateConfig;
use zootree::config::ConfigManager;
use zootree::runner::MockRunner;

fn success_stdout(stdout: &str) -> Output {
    Output {
        status: ExitStatus::from_raw(0),
        stdout: stdout.as_bytes().to_vec(),
        stderr: Vec::new(),
    }
}

fn failure_output(stderr: &str) -> Output {
    Output {
        status: ExitStatus::from_raw(1),
        stdout: Vec::new(),
        stderr: stderr.as_bytes().to_vec(),
    }
}

fn repo_config(path: &str, default_target_branch: Option<&str>) -> RepoConfig {
    RepoConfig {
        path: path.into(),
        default_target_branch: default_target_branch.map(str::to_string),
        copy_files: Vec::new(),
        hooks: Default::default(),
        lazygit: None,
    }
}

fn create_args_with(
    title: Option<&str>,
    repos: Option<&str>,
    template: Option<&str>,
) -> CreateArgs {
    CreateArgs {
        title: title.map(str::to_string),
        name: None,
        description: None,
        repos: repos.map(str::to_string),
        branch: None,
        template: template.map(str::to_string),
        start: false,
        run_agent: None,
    }
}

fn create_args_full() -> CreateArgs {
    CreateArgs {
        title: Some("auth cleanup".into()),
        name: Some("open-reef".into()),
        description: Some("clean up auth flow".into()),
        repos: Some("frontend:release".into()),
        branch: Some("feature/auth-cleanup".into()),
        template: None,
        start: true,
        run_agent: None,
    }
}

#[test]
fn draft_derives_branch_and_workspace_dir_from_name() {
    let global = GlobalConfig {
        workspace_root: "/tmp/zootree-workspaces".into(),
        branch_prefix: "zt".into(),
        ..Default::default()
    };
    let mut draft = CreateDraft::new("auth cleanup", "open-reef", &global);

    assert_eq!(draft.title, "auth cleanup");
    assert_eq!(draft.name, "open-reef");
    assert_eq!(draft.branch, "zt/open-reef");
    assert_eq!(draft.workspace_dir, "/tmp/zootree-workspaces/open-reef");

    draft.set_name("wide-tide", &global);
    assert_eq!(draft.name, "wide-tide");
    assert_eq!(draft.branch, "zt/wide-tide");
    assert_eq!(draft.workspace_dir, "/tmp/zootree-workspaces/wide-tide");
}

#[test]
fn draft_from_args_uses_cli_values_as_initial_draft_values() {
    let tmp = TempDir::new().unwrap();
    let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
    mgr.ensure_dirs().unwrap();
    mgr.save_repo_config("frontend", &repo_config("/repo/frontend", Some("main")))
        .unwrap();
    let runner = MockRunner::new();
    let global = GlobalConfig {
        workspace_root: "/tmp/zootree-workspaces".into(),
        branch_prefix: "zt".into(),
        ..Default::default()
    };

    let draft = draft_from_args(&create_args_full(), &mgr, &runner, &global, None, &[]).unwrap();

    assert_eq!(draft.title, "auth cleanup");
    assert_eq!(draft.description, "clean up auth flow");
    assert_eq!(draft.name, "open-reef");
    assert_eq!(draft.branch, "feature/auth-cleanup");
    assert!(draft.branch_was_edited);
    assert_eq!(draft.workspace_dir, "/tmp/zootree-workspaces/open-reef");
    assert_eq!(draft.after_create, AfterCreateMode::Start);
    assert_eq!(draft.selected_repos().len(), 1);
    assert_eq!(draft.repo("frontend").unwrap().target_branch, "release");
    assert!(draft.repo("frontend").unwrap().selected);
}

#[test]
fn draft_from_args_maps_run_agent_to_start_and_run_agent_mode() {
    let tmp = TempDir::new().unwrap();
    let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
    mgr.ensure_dirs().unwrap();
    mgr.save_repo_config("frontend", &repo_config("/repo/frontend", Some("main")))
        .unwrap();
    let runner = MockRunner::new();
    let global = GlobalConfig::default();
    let mut args = create_args_with(Some("auth cleanup"), Some("frontend"), None);
    args.run_agent = Some(Some("codex".into()));

    let draft = draft_from_args(&args, &mgr, &runner, &global, None, &[]).unwrap();

    assert_eq!(
        draft.after_create,
        AfterCreateMode::StartAndRunAgent {
            run_agent: Some("codex".into())
        }
    );
}

#[test]
fn draft_from_args_rejects_unknown_repo_arg() {
    let tmp = TempDir::new().unwrap();
    let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
    mgr.ensure_dirs().unwrap();
    mgr.save_repo_config("frontend", &repo_config("/repo/frontend", Some("main")))
        .unwrap();
    let runner = MockRunner::new();
    let global = GlobalConfig::default();
    let args = create_args_with(Some("auth cleanup"), Some("missing"), None);

    let err = draft_from_args(&args, &mgr, &runner, &global, None, &[])
        .unwrap_err()
        .to_string();

    assert!(
        err.contains("repo 'missing' is not registered"),
        "got: {}",
        err
    );
}

#[test]
fn resolve_agent_cli_for_draft_uses_global_default_for_default_run_agent_mode() {
    let global = GlobalConfig {
        agent_cli: Some("codex".into()),
        ..Default::default()
    };

    let agent_cli = resolve_agent_cli_for_draft(
        &AfterCreateMode::StartAndRunAgent { run_agent: None },
        &global,
    )
    .unwrap();

    assert_eq!(agent_cli.as_deref(), Some("codex"));
}

#[test]
fn draft_from_args_prefers_explicit_repos_over_template_selection() {
    let tmp = TempDir::new().unwrap();
    let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
    mgr.ensure_dirs().unwrap();
    mgr.save_repo_config("frontend", &repo_config("/repo/frontend", Some("main")))
        .unwrap();
    mgr.save_repo_config("backend", &repo_config("/repo/backend", Some("develop")))
        .unwrap();
    let runner = MockRunner::new();
    let global = GlobalConfig::default();
    let mut args = create_args_with(Some("auth cleanup"), Some("frontend"), Some("recently"));
    args.name = Some("open-reef".into());

    let draft = draft_from_args(&args, &mgr, &runner, &global, None, &[]).unwrap();

    assert_eq!(draft.repos.len(), 1);
    assert!(draft.repo("frontend").unwrap().selected);
    assert!(draft.repo("backend").is_none());
}

#[test]
fn draft_from_args_explicit_repos_only_touches_requested_repos() {
    let tmp = TempDir::new().unwrap();
    let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
    mgr.ensure_dirs().unwrap();
    mgr.save_repo_config("frontend", &repo_config("/repo/frontend", Some("main")))
        .unwrap();
    mgr.save_repo_config("broken", &repo_config("/repo/broken", None))
        .unwrap();
    let runner = MockRunner::new();
    let global = GlobalConfig::default();
    let args = create_args_with(Some("auth cleanup"), Some("frontend"), None);

    let draft = draft_from_args(
        &args,
        &mgr,
        &runner,
        &global,
        Some(CurrentRepoCandidate::Registered {
            name: "broken".into(),
            current_branch: "feature/current".into(),
        }),
        &[],
    )
    .unwrap();

    assert_eq!(draft.repos.len(), 1);
    assert_eq!(draft.repos[0].name, "frontend");
    assert!(draft.repos[0].selected);
    assert_eq!(draft.repos[0].target_branch, "main");
    assert!(runner.take_calls().is_empty());
}

#[test]
fn draft_from_args_template_only_touches_template_repos() {
    let tmp = TempDir::new().unwrap();
    let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
    mgr.ensure_dirs().unwrap();
    mgr.save_repo_config("frontend", &repo_config("/repo/frontend", Some("main")))
        .unwrap();
    mgr.save_repo_config("broken", &repo_config("/repo/broken", None))
        .unwrap();
    mgr.save_template(
        "recently",
        &TemplateConfig {
            repos: vec!["frontend".into()],
            multiplexer: Default::default(),
        },
    )
    .unwrap();
    let runner = MockRunner::new();
    let global = GlobalConfig::default();
    let args = create_args_with(Some("auth cleanup"), None, Some("recently"));

    let draft = draft_from_args(
        &args,
        &mgr,
        &runner,
        &global,
        Some(CurrentRepoCandidate::Registered {
            name: "broken".into(),
            current_branch: "feature/current".into(),
        }),
        &[],
    )
    .unwrap();

    assert_eq!(draft.repos.len(), 1);
    assert_eq!(draft.repos[0].name, "frontend");
    assert!(draft.repos[0].selected);
    assert_eq!(draft.repos[0].target_branch, "main");
    assert!(runner.take_calls().is_empty());
}

#[test]
fn draft_from_args_template_carries_multiplexer_snapshot() {
    let tmp = TempDir::new().unwrap();
    let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
    mgr.ensure_dirs().unwrap();
    mgr.save_repo_config("frontend", &repo_config("/repo/frontend", Some("main")))
        .unwrap();
    let multiplexer = MultiplexerConfig {
        kind: MultiplexerKind::Cmux,
        ..Default::default()
    };
    mgr.save_template(
        "recently",
        &TemplateConfig {
            repos: vec!["frontend".into()],
            multiplexer: multiplexer.clone(),
        },
    )
    .unwrap();
    let runner = MockRunner::new();
    let global = GlobalConfig::default();
    let args = create_args_with(Some("auth cleanup"), None, Some("recently"));

    let draft = draft_from_args(&args, &mgr, &runner, &global, None, &[]).unwrap();
    let workspace = workspace_from_draft(
        &draft,
        "2026-06-23T10:00:00+08:00",
        None,
        draft
            .multiplexer
            .clone()
            .unwrap_or_else(|| global.multiplexer.clone()),
    );

    assert_eq!(draft.multiplexer, Some(multiplexer.clone()));
    assert_eq!(workspace.multiplexer, multiplexer);
}

#[test]
fn draft_from_args_missing_title_template_includes_all_repos_with_template_selected() {
    let tmp = TempDir::new().unwrap();
    let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
    mgr.ensure_dirs().unwrap();
    mgr.save_repo_config("frontend", &repo_config("/repo/frontend", Some("main")))
        .unwrap();
    mgr.save_repo_config("backend", &repo_config("/repo/backend", Some("develop")))
        .unwrap();
    mgr.save_repo_config("docs", &repo_config("/repo/docs", Some("main")))
        .unwrap();
    mgr.save_template(
        "recently",
        &TemplateConfig {
            repos: vec!["frontend".into(), "docs".into()],
            multiplexer: Default::default(),
        },
    )
    .unwrap();
    let runner = MockRunner::new();
    let global = GlobalConfig::default();
    let args = create_args_with(None, None, Some("recently"));

    assert!(create_args_need_wizard(&args));
    let draft = draft_from_args(
        &args,
        &mgr,
        &runner,
        &global,
        Some(CurrentRepoCandidate::Registered {
            name: "backend".into(),
            current_branch: "feature/current".into(),
        }),
        &[],
    )
    .unwrap();

    assert_eq!(draft.repos.len(), 3);
    assert!(draft.repo("frontend").unwrap().selected);
    assert!(!draft.repo("backend").unwrap().selected);
    assert!(draft.repo("docs").unwrap().selected);
    assert_eq!(
        draft.repo("backend").unwrap().target_branch,
        "feature/current"
    );
    assert!(runner.take_calls().is_empty());
}

#[test]
fn manual_branch_is_not_overwritten_when_name_changes() {
    let global = GlobalConfig::default();
    let mut draft = CreateDraft::new("auth cleanup", "open-reef", &global);

    draft.set_branch("feature/auth-cleanup");
    draft.set_name("wide-tide", &global);

    assert_eq!(draft.name, "wide-tide");
    assert_eq!(draft.branch, "feature/auth-cleanup");
}

#[test]
fn apply_template_replaces_selection_but_keeps_manual_edit_possible() {
    let global = GlobalConfig::default();
    let mut draft = CreateDraft::new("auth cleanup", "open-reef", &global);
    draft.repos = vec![
        RepoDraftEntry::new("frontend", "main", true),
        RepoDraftEntry::new("backend", "develop", false),
        RepoDraftEntry::new("docs", "main", false),
    ];

    draft.apply_template_repos(&["backend".to_string(), "docs".to_string()]);

    assert!(!draft.repo("frontend").unwrap().selected);
    assert!(draft.repo("backend").unwrap().selected);
    assert!(draft.repo("docs").unwrap().selected);

    draft.toggle_repo("frontend");
    assert!(draft.repo("frontend").unwrap().selected);
}

#[test]
fn after_create_modes_map_to_start_and_agent_flags() {
    assert!(!AfterCreateMode::CreateOnly.should_start());
    assert!(AfterCreateMode::Start.should_start());
    assert!(AfterCreateMode::StartAndRunAgent { run_agent: None }.should_start());

    assert_eq!(AfterCreateMode::CreateOnly.run_agent_arg(), None);
    assert_eq!(AfterCreateMode::Start.run_agent_arg(), None);
    assert_eq!(
        AfterCreateMode::StartAndRunAgent {
            run_agent: Some("codex".into())
        }
        .run_agent_arg(),
        Some(Some("codex".into()))
    );
    assert_eq!(
        AfterCreateMode::StartAndRunAgent { run_agent: None }.run_agent_arg(),
        Some(None)
    );
}

#[test]
fn layout_mode_uses_double_column_only_when_wide_enough() {
    assert_eq!(
        CreateWizardLayout::for_width(120),
        CreateWizardLayout::TwoColumn
    );
    assert_eq!(
        CreateWizardLayout::for_width(80),
        CreateWizardLayout::SingleColumn
    );
    assert_eq!(
        CreateWizardLayout::for_width(40),
        CreateWizardLayout::TooNarrow
    );
}

#[test]
fn layout_mode_covers_threshold_boundaries() {
    assert_eq!(
        CreateWizardLayout::for_width(49),
        CreateWizardLayout::TooNarrow
    );
    assert_eq!(
        CreateWizardLayout::for_width(50),
        CreateWizardLayout::SingleColumn
    );
    assert_eq!(
        CreateWizardLayout::for_width(99),
        CreateWizardLayout::SingleColumn
    );
    assert_eq!(
        CreateWizardLayout::for_width(100),
        CreateWizardLayout::TwoColumn
    );
}

#[test]
fn repo_draft_prefers_current_repo_branch_over_config_default() {
    let tmp = TempDir::new().unwrap();
    let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
    mgr.ensure_dirs().unwrap();
    mgr.save_repo_config("frontend", &repo_config("/repo/frontend", Some("develop")))
        .unwrap();
    let runner = MockRunner::new();

    let repos = build_repo_draft_entries(
        &mgr,
        &runner,
        Some(CurrentRepoCandidate::Registered {
            name: "frontend".to_string(),
            current_branch: "feature/current".to_string(),
        }),
    )
    .unwrap();

    let frontend = repos.iter().find(|repo| repo.name == "frontend").unwrap();
    assert!(frontend.selected);
    assert_eq!(frontend.target_branch, "feature/current");
    assert!(runner.take_calls().is_empty());
}

#[test]
fn repo_draft_appends_pending_current_repo_selected_by_default() {
    let tmp = TempDir::new().unwrap();
    let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
    mgr.ensure_dirs().unwrap();
    mgr.save_repo_config("backend", &repo_config("/repo/backend", Some("develop")))
        .unwrap();
    let runner = MockRunner::new();

    let repos = build_repo_draft_entries(
        &mgr,
        &runner,
        Some(CurrentRepoCandidate::PendingRegistration {
            name: "zootree".into(),
            path: "/repo/zootree".into(),
            current_branch: "feature/current".into(),
        }),
    )
    .unwrap();

    assert_eq!(repos.len(), 2);
    assert_eq!(repos[0].name, "backend");
    assert_eq!(repos[0].source, RepoDraftSource::Registered);
    assert!(!repos[0].selected);
    assert_eq!(repos[1].name, "zootree");
    assert_eq!(
        repos[1].source,
        RepoDraftSource::PendingRegistration {
            path: "/repo/zootree".into()
        }
    );
    assert!(repos[1].selected);
    assert_eq!(repos[1].target_branch, "feature/current");
}

#[test]
fn draft_from_args_includes_pending_current_repo_for_interactive_repo_selection() {
    let tmp = TempDir::new().unwrap();
    let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
    mgr.ensure_dirs().unwrap();
    let runner = MockRunner::new();
    let global = GlobalConfig::default();
    let args = create_args_with(Some("auth cleanup"), None, None);

    let draft = draft_from_args(
        &args,
        &mgr,
        &runner,
        &global,
        Some(CurrentRepoCandidate::PendingRegistration {
            name: "zootree".into(),
            path: "/repo/zootree".into(),
            current_branch: "feature/current".into(),
        }),
        &[],
    )
    .unwrap();

    assert_eq!(draft.repos.len(), 1);
    assert_eq!(draft.repos[0].name, "zootree");
    assert!(draft.repos[0].selected);
    assert!(draft.repos[0].is_pending_registration());
}

#[test]
fn repo_draft_entry_new_defaults_to_registered_source() {
    let entry = RepoDraftEntry::new("frontend", "main", true);

    assert_eq!(entry.source, RepoDraftSource::Registered);
}

#[test]
fn pending_repo_draft_entry_records_path_and_label() {
    let entry =
        RepoDraftEntry::pending_registration("zootree", "feature/current", true, "/repo/zootree");

    assert_eq!(entry.name, "zootree");
    assert_eq!(entry.target_branch, "feature/current");
    assert!(entry.selected);
    assert_eq!(
        entry.source,
        RepoDraftSource::PendingRegistration {
            path: "/repo/zootree".into()
        }
    );
    assert!(entry.is_pending_registration());
    assert_eq!(entry.display_name(), "zootree (new, will register)");
}

#[test]
fn discover_current_repo_candidate_returns_pending_without_writing_config() {
    let tmp = TempDir::new().unwrap();
    let mgr = ConfigManager::with_base_dir(tmp.path().join("config"));
    mgr.ensure_dirs().unwrap();
    let repo_root = tmp.path().join("zootree");
    std::fs::create_dir(&repo_root).unwrap();
    let runner = MockRunner::new();
    runner.push_response(success_stdout(&format!("{}\n", repo_root.display())));
    runner.push_response(success_stdout("feature/current\n"));

    let candidate = discover_current_repo_candidate(&mgr, &runner, &repo_root)
        .unwrap()
        .unwrap();

    assert_eq!(
        candidate,
        CurrentRepoCandidate::PendingRegistration {
            name: "zootree".into(),
            path: repo_root
                .canonicalize()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            current_branch: "feature/current".into(),
        }
    );
    assert!(mgr.list_repos().unwrap().is_empty());
}

#[test]
fn discover_current_repo_candidate_reuses_registered_repo_for_same_path() {
    let tmp = TempDir::new().unwrap();
    let mgr = ConfigManager::with_base_dir(tmp.path().join("config"));
    mgr.ensure_dirs().unwrap();
    let repo_root = tmp.path().join("zootree");
    std::fs::create_dir(&repo_root).unwrap();
    mgr.save_repo_config(
        "custom",
        &repo_config(&repo_root.to_string_lossy(), Some("develop")),
    )
    .unwrap();
    let runner = MockRunner::new();
    runner.push_response(success_stdout(&format!("{}\n", repo_root.display())));
    runner.push_response(success_stdout("feature/current\n"));

    let candidate = discover_current_repo_candidate(&mgr, &runner, &repo_root)
        .unwrap()
        .unwrap();

    assert_eq!(
        candidate,
        CurrentRepoCandidate::Registered {
            name: "custom".into(),
            current_branch: "feature/current".into(),
        }
    );
    assert_eq!(mgr.list_repos().unwrap(), vec!["custom"]);
}

#[test]
fn discover_current_repo_candidate_uses_collision_safe_pending_name() {
    let tmp = TempDir::new().unwrap();
    let mgr = ConfigManager::with_base_dir(tmp.path().join("config"));
    mgr.ensure_dirs().unwrap();
    let existing_root = tmp.path().join("existing");
    let repo_root = tmp.path().join("zootree");
    std::fs::create_dir(&existing_root).unwrap();
    std::fs::create_dir(&repo_root).unwrap();
    mgr.save_repo_config(
        "zootree",
        &repo_config(&existing_root.to_string_lossy(), None),
    )
    .unwrap();
    let runner = MockRunner::new();
    runner.push_response(success_stdout(&format!("{}\n", repo_root.display())));
    runner.push_response(success_stdout("feature/current\n"));

    let candidate = discover_current_repo_candidate(&mgr, &runner, &repo_root)
        .unwrap()
        .unwrap();

    assert_eq!(
        candidate,
        CurrentRepoCandidate::PendingRegistration {
            name: "zootree-2".into(),
            path: repo_root
                .canonicalize()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            current_branch: "feature/current".into(),
        }
    );
    assert_eq!(mgr.list_repos().unwrap(), vec!["zootree"]);
}

#[test]
fn repo_draft_uses_repo_default_for_non_current_repo() {
    let tmp = TempDir::new().unwrap();
    let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
    mgr.ensure_dirs().unwrap();
    mgr.save_repo_config("backend", &repo_config("/repo/backend", Some("develop")))
        .unwrap();
    let runner = MockRunner::new();

    let repos = build_repo_draft_entries(&mgr, &runner, None).unwrap();

    assert_eq!(repos[0].name, "backend");
    assert!(!repos[0].selected);
    assert_eq!(repos[0].target_branch, "develop");
    assert!(runner.take_calls().is_empty());
}

#[test]
fn repo_draft_falls_back_to_current_branch_when_no_default() {
    let tmp = TempDir::new().unwrap();
    let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
    mgr.ensure_dirs().unwrap();
    mgr.save_repo_config("backend", &repo_config("/repo/backend", None))
        .unwrap();
    let runner = MockRunner::new();
    runner.push_response(success_stdout("main\n"));

    let repos = build_repo_draft_entries(&mgr, &runner, None).unwrap();

    assert_eq!(repos[0].target_branch, "main");
    assert_eq!(
        runner.take_calls()[0].args,
        vec!["-C", "/repo/backend", "rev-parse", "--abbrev-ref", "HEAD"]
    );
}

#[test]
fn repo_draft_falls_back_to_main_when_current_branch_lookup_fails() {
    let tmp = TempDir::new().unwrap();
    let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
    mgr.ensure_dirs().unwrap();
    mgr.save_repo_config("backend", &repo_config("/repo/backend", None))
        .unwrap();
    let runner = MockRunner::new();
    runner.push_response(failure_output("not a git repository"));

    let repos = build_repo_draft_entries(&mgr, &runner, None).unwrap();

    assert_eq!(repos[0].target_branch, "main");
}

#[test]
fn draft_validation_reports_all_blocking_field_errors() {
    let global = GlobalConfig::default();
    let mut draft = CreateDraft::new("", "open-reef", &global);
    draft.repos = vec![RepoDraftEntry::new("frontend", "", true)];

    let errors = draft.validate(&[], &global);

    assert!(errors.contains(&CreateDraftError::TitleRequired));
    assert!(errors.contains(&CreateDraftError::TargetBranchRequired("frontend".into())));
}

#[test]
fn draft_validation_requires_workspace_name() {
    let global = GlobalConfig::default();
    let mut draft = CreateDraft::new("auth cleanup", "   ", &global);
    draft.repos = vec![RepoDraftEntry::new("frontend", "main", true)];

    let errors = draft.validate(&[], &global);

    assert_eq!(errors, vec![CreateDraftError::WorkspaceNameRequired]);
}

#[test]
fn draft_validation_requires_workspace_branch() {
    let global = GlobalConfig::default();
    let mut draft = CreateDraft::new("auth cleanup", "open-reef", &global);
    draft.branch = "   ".into();
    draft.repos = vec![RepoDraftEntry::new("frontend", "main", true)];

    let errors = draft.validate(&[], &global);

    assert_eq!(errors, vec![CreateDraftError::WorkspaceBranchRequired]);
}

#[test]
fn draft_validation_requires_at_least_one_repo_even_when_repo_list_is_empty() {
    let global = GlobalConfig::default();
    let draft = CreateDraft::new("auth cleanup", "open-reef", &global);

    let errors = draft.validate(&[], &global);

    assert_eq!(errors, vec![CreateDraftError::RepoRequired]);
}

#[test]
fn draft_validation_rejects_existing_workspace_name() {
    let global = GlobalConfig::default();
    let mut draft = CreateDraft::new("auth cleanup", "open-reef", &global);
    draft.repos = vec![RepoDraftEntry::new("frontend", "main", true)];

    let errors = draft.validate(&["open-reef".to_string()], &global);

    assert_eq!(
        errors,
        vec![CreateDraftError::WorkspaceNameExists("open-reef".into())]
    );
}

#[test]
fn draft_validation_rejects_default_agent_when_global_agent_missing() {
    let global = GlobalConfig::default();
    let mut draft = CreateDraft::new("auth cleanup", "open-reef", &global);
    draft.repos = vec![RepoDraftEntry::new("frontend", "main", true)];
    draft.after_create = AfterCreateMode::StartAndRunAgent { run_agent: None };

    let errors = draft.validate(&[], &global);

    assert_eq!(errors, vec![CreateDraftError::DefaultAgentMissing]);
}

#[test]
fn complete_title_and_repos_args_do_not_need_wizard() {
    let args = create_args_with(Some("auth cleanup"), Some("frontend:main"), None);
    assert!(!create_args_need_wizard(&args));
}

#[test]
fn complete_title_and_template_args_do_not_need_wizard() {
    let args = create_args_with(Some("auth cleanup"), None, Some("recently"));
    assert!(!create_args_need_wizard(&args));
}

#[test]
fn missing_title_or_repo_source_needs_wizard() {
    assert!(create_args_need_wizard(&create_args_with(
        None,
        Some("frontend"),
        None
    )));
    assert!(create_args_need_wizard(&create_args_with(
        Some("auth cleanup"),
        None,
        None
    )));
}

#[test]
fn workspace_from_draft_matches_existing_create_shape() {
    let global = GlobalConfig {
        workspace_root: "/tmp/zootree-workspaces".into(),
        branch_prefix: "zt".into(),
        ..Default::default()
    };
    let mut draft = CreateDraft::new("auth cleanup", "open-reef", &global);
    draft.description = "clean up auth flow".into();
    draft.repos = vec![
        RepoDraftEntry::new("frontend", "main", true),
        RepoDraftEntry::new("backend", "develop", false),
    ];
    draft.after_create = AfterCreateMode::StartAndRunAgent {
        run_agent: Some("codex".into()),
    };
    let output = CreateWizardOutput { draft };
    let multiplexer = MultiplexerConfig {
        kind: MultiplexerKind::Cmux,
        ..Default::default()
    };

    let workspace = workspace_from_draft(
        &output.draft,
        "2026-06-23T10:00:00+08:00",
        Some("codex".into()),
        multiplexer.clone(),
    );

    assert_eq!(workspace.title, "auth cleanup");
    assert_eq!(workspace.name, "open-reef");
    assert_eq!(workspace.description, "clean up auth flow");
    assert_eq!(workspace.branch, "zt/open-reef");
    assert_eq!(workspace.workspace_dir, "/tmp/zootree-workspaces/open-reef");
    assert_eq!(workspace.agent_cli.as_deref(), Some("codex"));
    assert_eq!(workspace.multiplexer, multiplexer);
    assert!(workspace.multiplexer_state.is_empty());
    assert_eq!(workspace.repos.len(), 1);
    assert_eq!(workspace.repos[0].name, "frontend");
    assert_eq!(workspace.repos[0].target_branch.as_deref(), Some("main"));
    assert_eq!(workspace.events[0].action, "created");
}

#[test]
fn persist_selected_pending_repos_writes_selected_repo_config() {
    let tmp = TempDir::new().unwrap();
    let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
    mgr.ensure_dirs().unwrap();
    let global = GlobalConfig::default();
    let mut draft = CreateDraft::new("auth cleanup", "open-reef", &global);
    draft.repos = vec![RepoDraftEntry::pending_registration(
        "zootree",
        "feature/current",
        true,
        "/repo/zootree",
    )];

    persist_selected_pending_repos(&mgr, &mut draft).unwrap();

    let config = mgr.load_repo_config("zootree").unwrap();
    assert_eq!(config.path, "/repo/zootree");
    assert!(config.default_target_branch.is_none());
    assert!(config.copy_files.is_empty());
    assert!(config.lazygit.is_none());
    assert_eq!(draft.repos[0].source, RepoDraftSource::Registered);
}

#[test]
fn persist_selected_pending_repos_ignores_deselected_pending_repo() {
    let tmp = TempDir::new().unwrap();
    let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
    mgr.ensure_dirs().unwrap();
    let global = GlobalConfig::default();
    let mut draft = CreateDraft::new("auth cleanup", "open-reef", &global);
    draft.repos = vec![RepoDraftEntry::pending_registration(
        "zootree",
        "feature/current",
        false,
        "/repo/zootree",
    )];

    persist_selected_pending_repos(&mgr, &mut draft).unwrap();

    assert!(mgr.list_repos().unwrap().is_empty());
    assert!(draft.repos[0].is_pending_registration());
    let workspace = workspace_from_draft(
        &draft,
        "2026-06-29T10:00:00+08:00",
        None,
        MultiplexerConfig::default(),
    );
    assert!(workspace.repos.is_empty());
}

#[test]
fn persist_selected_pending_repos_resolves_submit_time_name_collision() {
    let tmp = TempDir::new().unwrap();
    let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
    mgr.ensure_dirs().unwrap();
    mgr.save_repo_config("zootree", &repo_config("/repo/other", None))
        .unwrap();
    let global = GlobalConfig::default();
    let mut draft = CreateDraft::new("auth cleanup", "open-reef", &global);
    draft.repos = vec![RepoDraftEntry::pending_registration(
        "zootree",
        "feature/current",
        true,
        "/repo/zootree",
    )];

    persist_selected_pending_repos(&mgr, &mut draft).unwrap();

    assert_eq!(draft.repos[0].name, "zootree-2");
    assert_eq!(draft.repos[0].source, RepoDraftSource::Registered);
    assert!(mgr.load_repo_config("zootree").is_ok());
    let new_config = mgr.load_repo_config("zootree-2").unwrap();
    assert_eq!(new_config.path, "/repo/zootree");
    let workspace = workspace_from_draft(
        &draft,
        "2026-06-29T10:00:00+08:00",
        None,
        MultiplexerConfig::default(),
    );
    assert_eq!(workspace.repos[0].name, "zootree-2");
}
