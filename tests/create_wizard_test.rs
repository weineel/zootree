use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use zootree::cli::create_flow::{
    AfterCreateMode, CreateDraft, CreateDraftError, CreateWizardLayout, RepoDraftEntry,
};
use zootree::config::global::GlobalConfig;
use zootree::tui_app::create_wizard::{
    CreateStep, CreateWizardApp, CreateWizardOutcome, CreateWizardPage,
};
use zootree::tui_app::{App, Event};

fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn key_mod(code: KeyCode, modifiers: KeyModifiers) -> Event {
    Event::Key(KeyEvent::new(code, modifiers))
}

fn paste(text: &str) -> Event {
    Event::Paste(text.into())
}

fn draft() -> CreateDraft {
    let global = GlobalConfig::default();
    let mut draft = CreateDraft::new("auth cleanup", "open-reef", &global);
    draft.repos = vec![RepoDraftEntry::new("frontend", "main", true)];
    draft
}

fn buffer_to_string(buf: &ratatui::buffer::Buffer) -> String {
    let mut out = String::new();
    for y in 0..buf.area.height {
        for x in 0..buf.area.width {
            out.push_str(buf[(x, y)].symbol());
        }
        out.push('\n');
    }
    out
}

fn render_to_string(app: &mut CreateWizardApp, width: u16, height: u16) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| <CreateWizardApp as App>::render(app, frame))
        .unwrap();
    buffer_to_string(terminal.backend().buffer())
}

fn jump_to_page(app: &mut CreateWizardApp, page: CreateWizardPage) {
    for _ in 0..32 {
        if app.page() == &page {
            return;
        }
        app.on_event(key(KeyCode::Enter)).unwrap();
    }
    panic!(
        "failed to jump to page {page:?}, current page: {:?}",
        app.page()
    );
}

fn clear_text_field_for_test(app: &mut CreateWizardApp) {
    app.on_event(key_mod(KeyCode::Char('e'), KeyModifiers::CONTROL))
        .unwrap();
    app.on_event(key_mod(KeyCode::Char('u'), KeyModifiers::CONTROL))
        .unwrap();
}

#[test]
fn wizard_layout_thresholds_match_width_contract() {
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
fn render_wide_layout_shows_step_summary_and_title() {
    let global = GlobalConfig::default();
    let mut app = CreateWizardApp::new(draft(), global, Vec::new());

    let out = render_to_string(&mut app, 120, 24);

    assert!(out.contains("Workspace: Title"), "missing page:\n{}", out);
    assert!(out.contains("Draft"), "missing summary:\n{}", out);
    assert!(out.contains("auth cleanup"), "missing title:\n{}", out);
}

#[test]
fn title_page_render_shows_textarea_and_summary() {
    let global = GlobalConfig::default();
    let mut draft = draft();
    draft.description = "fix login redirects".into();
    let mut app = CreateWizardApp::new(draft, global, Vec::new());

    let out = render_to_string(&mut app, 120, 24);

    assert!(
        out.contains("Workspace: Title"),
        "missing title page:\n{out}"
    );
    assert!(out.contains("auth cleanup"), "missing title text:\n{out}");
    assert!(
        out.contains("workspace_dir:"),
        "derived workspace_dir should remain visible:\n{out}"
    );
    assert!(
        out.contains("description: fix login redirects"),
        "description should remain visible in draft preview:\n{out}"
    );
}

#[test]
fn render_title_page_shows_one_text_input_and_draft_preview() {
    let global = GlobalConfig::default();
    let mut app = CreateWizardApp::new(draft(), global, Vec::new());

    let out = render_to_string(&mut app, 120, 24);

    assert!(out.contains("Workspace: Title"), "{out}");
    assert!(out.contains("Draft"), "{out}");
    assert!(out.contains("auth cleanup"), "{out}");
    assert!(!out.contains("Workspace info"), "{out}");
    assert!(!out.contains("Workspace: Description"), "{out}");
    assert!(out.contains("description:"), "{out}");
}

#[test]
fn workspace_name_page_render_shows_textarea_and_derived_context() {
    let global = GlobalConfig {
        workspace_root: "/tmp/zootree-workspaces".into(),
        branch_prefix: "zt".into(),
        ..Default::default()
    };
    let mut draft = CreateDraft::new("auth cleanup", "open-reef", &global);
    draft.repos = vec![RepoDraftEntry::new("frontend", "main", true)];
    let mut app = CreateWizardApp::new(draft, global, Vec::new());
    jump_to_page(&mut app, CreateWizardPage::WorkspaceName);

    let out = render_to_string(&mut app, 120, 24);

    assert!(
        out.contains("Workspace: Name"),
        "workspace name should render as its own textarea page:\n{out}"
    );
    assert!(
        out.contains("zt/open-reef"),
        "missing derived branch:\n{out}"
    );
    assert!(
        out.contains("/tmp/zootree-workspaces/open-reef"),
        "missing derived workspace_dir:\n{out}"
    );
}

#[test]
fn render_name_page_shows_derived_workspace_dir_as_read_only_context() {
    let global = GlobalConfig {
        workspace_root: "/tmp/zootree-workspaces".into(),
        branch_prefix: "zt".into(),
        ..Default::default()
    };
    let mut draft = CreateDraft::new("auth cleanup", "open-reef", &global);
    draft.repos = vec![RepoDraftEntry::new("frontend", "main", true)];
    let mut app = CreateWizardApp::new(draft, global, Vec::new());

    jump_to_page(&mut app, CreateWizardPage::WorkspaceName);
    let out = render_to_string(&mut app, 120, 24);

    assert!(out.contains("Workspace: Name"), "{out}");
    assert!(out.contains("derived branch: zt/open-reef"), "{out}");
    assert!(
        out.contains("workspace_dir: /tmp/zootree-workspaces/open-reef"),
        "{out}"
    );
}

#[test]
fn workspace_branch_page_render_shows_textarea_and_derived_context() {
    let global = GlobalConfig {
        workspace_root: "/tmp/zootree-workspaces".into(),
        branch_prefix: "zt".into(),
        ..Default::default()
    };
    let mut app = CreateWizardApp::new(draft(), global, Vec::new());
    jump_to_page(&mut app, CreateWizardPage::WorkspaceBranch);

    let out = render_to_string(&mut app, 120, 24);

    assert!(
        out.contains("Workspace: Branch"),
        "workspace branch should render as its own textarea page:\n{out}"
    );
    assert!(
        out.contains("/tmp/zootree-workspaces/open-reef"),
        "missing derived workspace_dir:\n{out}"
    );
}

#[test]
fn render_narrow_layout_shows_compact_summary_and_step() {
    let global = GlobalConfig::default();
    let mut app = CreateWizardApp::new(draft(), global, Vec::new());

    let out = render_to_string(&mut app, 80, 24);

    assert!(out.contains("Workspace: Title"), "missing page:\n{}", out);
    assert!(out.contains("repos: 1"), "missing repo count:\n{}", out);
}

#[test]
fn render_too_narrow_layout_asks_user_to_resize() {
    let global = GlobalConfig::default();
    let mut app = CreateWizardApp::new(draft(), global, Vec::new());

    let out = render_to_string(&mut app, 40, 12);

    assert!(
        out.contains("resize to at least 50 columns"),
        "missing resize message:\n{}",
        out
    );
}

#[test]
fn field_pages_start_with_workspace_fields_then_repos() {
    let global = GlobalConfig::default();
    let app = CreateWizardApp::new(draft(), global, Vec::new());

    assert_eq!(app.page(), &CreateWizardPage::Title);
    assert_eq!(
        app.page_titles(),
        vec![
            "Workspace: Title",
            "Workspace: Description",
            "Workspace: Name",
            "Workspace: Branch",
            "Repos",
            "Branches: frontend",
            "After create",
            "Review",
        ]
    );
}

#[test]
fn title_page_uses_textarea_editing_and_enter_commits() {
    let global = GlobalConfig::default();
    let mut draft = draft();
    draft.title.clear();
    let mut app = CreateWizardApp::new(draft, global, Vec::new());

    app.on_event(key(KeyCode::Char('a'))).unwrap();
    app.on_event(key(KeyCode::Char('b'))).unwrap();
    app.on_event(key_mod(KeyCode::Char('a'), KeyModifiers::CONTROL))
        .unwrap();
    app.on_event(key(KeyCode::Char('X'))).unwrap();
    app.on_event(key_mod(KeyCode::Char('e'), KeyModifiers::CONTROL))
        .unwrap();
    app.on_event(key(KeyCode::Char('c'))).unwrap();
    app.on_event(key(KeyCode::Enter)).unwrap();

    assert_eq!(app.draft().title, "Xabc");
    assert_eq!(app.page(), &CreateWizardPage::Description);
}

#[test]
fn textarea_shift_enter_inserts_newline_but_single_line_title_rejects_it() {
    let global = GlobalConfig::default();
    let mut draft = draft();
    draft.title.clear();
    let mut app = CreateWizardApp::new(draft, global, Vec::new());

    app.on_event(key(KeyCode::Char('a'))).unwrap();
    app.on_event(key_mod(KeyCode::Enter, KeyModifiers::SHIFT))
        .unwrap();
    app.on_event(key(KeyCode::Char('b'))).unwrap();
    app.on_event(key(KeyCode::Enter)).unwrap();

    assert_eq!(app.page(), &CreateWizardPage::Title);
    assert!(app
        .errors()
        .iter()
        .any(|err| matches!(err, CreateDraftError::TitleSingleLineRequired)));
}

#[test]
fn title_page_rejects_leading_or_trailing_newline() {
    for text in ["abc\n", "\nabc"] {
        let global = GlobalConfig::default();
        let mut draft = draft();
        draft.title.clear();
        let mut app = CreateWizardApp::new(draft, global, Vec::new());

        app.on_event(paste(text)).unwrap();
        app.on_event(key(KeyCode::Enter)).unwrap();

        assert_eq!(app.page(), &CreateWizardPage::Title, "text: {text:?}");
        assert!(app
            .errors()
            .iter()
            .any(|err| matches!(err, CreateDraftError::TitleSingleLineRequired)));
    }
}

#[test]
fn title_page_rejects_initial_leading_or_trailing_newline() {
    for text in ["abc\n", "\nabc"] {
        let global = GlobalConfig::default();
        let mut draft = draft();
        draft.title = text.into();
        let mut app = CreateWizardApp::new(draft, global, Vec::new());

        app.on_event(key(KeyCode::Enter)).unwrap();

        assert_eq!(app.page(), &CreateWizardPage::Title, "text: {text:?}");
        assert!(app
            .errors()
            .iter()
            .any(|err| matches!(err, CreateDraftError::TitleSingleLineRequired)));
    }
}

#[test]
fn text_pages_keep_navigation_keys_for_textarea_input() {
    let global = GlobalConfig::default();
    let mut app = CreateWizardApp::new(draft(), global, Vec::new());

    app.on_event(key(KeyCode::Enter)).unwrap();
    assert_eq!(app.page(), &CreateWizardPage::Description);

    for event in [
        key(KeyCode::Down),
        key(KeyCode::Up),
        key_mod(KeyCode::Char('n'), KeyModifiers::CONTROL),
        key_mod(KeyCode::Char('p'), KeyModifiers::CONTROL),
        key(KeyCode::Tab),
        key(KeyCode::BackTab),
    ] {
        app.on_event(event).unwrap();
        assert_eq!(app.page(), &CreateWizardPage::Description);
    }

    clear_text_field_for_test(&mut app);
    app.on_event(paste("details")).unwrap();
    app.on_event(key(KeyCode::Enter)).unwrap();
    assert_eq!(app.draft().description, "details");
    assert_eq!(app.page(), &CreateWizardPage::WorkspaceName);
}

#[test]
fn title_navigation_commits_buffer_and_preserves_it_when_returning() {
    let global = GlobalConfig::default();
    let mut draft = draft();
    draft.title.clear();
    let mut app = CreateWizardApp::new(draft, global, Vec::new());

    app.on_event(paste("New title")).unwrap();
    app.on_event(key(KeyCode::Enter)).unwrap();

    assert_eq!(app.page(), &CreateWizardPage::Description);
    assert_eq!(app.draft().title, "New title");

    app.on_event(key(KeyCode::Esc)).unwrap();

    assert_eq!(app.page(), &CreateWizardPage::Title);
    app.on_event(key(KeyCode::Enter)).unwrap();
    assert_eq!(app.draft().title, "New title");
    assert_eq!(app.page(), &CreateWizardPage::Description);
}

#[test]
fn invalid_title_navigation_does_not_leave_page() {
    let global = GlobalConfig::default();
    let mut draft = draft();
    draft.title.clear();
    let mut app = CreateWizardApp::new(draft, global, Vec::new());

    app.on_event(key_mod(KeyCode::Enter, KeyModifiers::SHIFT))
        .unwrap();
    app.on_event(key(KeyCode::Enter)).unwrap();

    assert_eq!(app.page(), &CreateWizardPage::Title);
    assert!(app
        .errors()
        .iter()
        .any(|err| matches!(err, CreateDraftError::TitleRequired)));
}

#[test]
fn description_navigation_commits_multiline_buffer_and_preserves_it_when_returning() {
    let global = GlobalConfig::default();
    let mut app = CreateWizardApp::new(draft(), global, Vec::new());

    app.on_event(key(KeyCode::Enter)).unwrap();
    assert_eq!(app.page(), &CreateWizardPage::Description);
    app.on_event(paste("line one\nline two")).unwrap();
    app.on_event(key(KeyCode::Enter)).unwrap();

    assert_eq!(app.page(), &CreateWizardPage::WorkspaceName);
    assert_eq!(app.draft().description, "line one\nline two");

    app.on_event(key(KeyCode::Esc)).unwrap();

    assert_eq!(app.page(), &CreateWizardPage::Description);
    app.on_event(key(KeyCode::Enter)).unwrap();
    assert_eq!(app.draft().description, "line one\nline two");
    assert_eq!(app.page(), &CreateWizardPage::WorkspaceName);
}

#[test]
fn workspace_name_existing_error_stays_until_name_is_unique() {
    let global = GlobalConfig::default();
    let mut app = CreateWizardApp::new(draft(), global, vec!["open-reef".into()]);

    app.on_event(key(KeyCode::Enter)).unwrap();
    app.on_event(key(KeyCode::Enter)).unwrap();
    assert_eq!(app.page(), &CreateWizardPage::WorkspaceName);

    app.on_event(key(KeyCode::Enter)).unwrap();
    assert_eq!(app.page(), &CreateWizardPage::WorkspaceName);
    assert_eq!(
        app.errors(),
        &[CreateDraftError::WorkspaceNameExists("open-reef".into())]
    );

    clear_text_field_for_test(&mut app);
    app.on_event(paste("open-reef-2")).unwrap();
    app.on_event(key(KeyCode::Enter)).unwrap();
    assert_eq!(app.page(), &CreateWizardPage::WorkspaceBranch);

    app.on_event(key(KeyCode::Esc)).unwrap();
    assert_eq!(app.page(), &CreateWizardPage::WorkspaceName);
}

#[test]
fn workspace_name_unique_value_can_enter_branch_and_repos() {
    let global = GlobalConfig::default();
    let mut app = CreateWizardApp::new(draft(), global, vec!["open-reef".into()]);

    app.on_event(key(KeyCode::Enter)).unwrap();
    app.on_event(key(KeyCode::Enter)).unwrap();
    assert_eq!(app.page(), &CreateWizardPage::WorkspaceName);

    clear_text_field_for_test(&mut app);
    app.on_event(paste("open-reef-2")).unwrap();
    app.on_event(key(KeyCode::Enter)).unwrap();
    assert_eq!(app.page(), &CreateWizardPage::WorkspaceBranch);

    app.on_event(key(KeyCode::Enter)).unwrap();
    assert_eq!(app.page(), &CreateWizardPage::Repos);
}

#[test]
fn description_esc_commits_multiline_buffer_before_going_back() {
    let global = GlobalConfig::default();
    let mut app = CreateWizardApp::new(draft(), global, Vec::new());

    app.on_event(key(KeyCode::Enter)).unwrap();
    assert_eq!(app.page(), &CreateWizardPage::Description);
    app.on_event(paste("line one\nline two")).unwrap();
    app.on_event(key(KeyCode::Esc)).unwrap();

    assert_eq!(app.page(), &CreateWizardPage::Title);
    assert_eq!(app.draft().description, "line one\nline two");

    app.on_event(key(KeyCode::Enter)).unwrap();

    assert_eq!(app.page(), &CreateWizardPage::Description);
    app.on_event(key(KeyCode::Enter)).unwrap();
    assert_eq!(app.draft().description, "line one\nline two");
}

#[test]
fn invalid_title_first_page_esc_keeps_cancel_semantics() {
    let global = GlobalConfig::default();
    let mut draft = draft();
    draft.title.clear();
    let mut app = CreateWizardApp::new(draft, global, Vec::new());

    app.on_event(key_mod(KeyCode::Enter, KeyModifiers::SHIFT))
        .unwrap();
    app.on_event(key(KeyCode::Esc)).unwrap();

    assert_eq!(app.outcome(), Some(CreateWizardOutcome::Cancelled));
}

#[test]
fn title_textarea_page_renders_validation_errors() {
    let global = GlobalConfig::default();
    let mut draft = draft();
    draft.title.clear();
    let mut app = CreateWizardApp::new(draft, global, Vec::new());

    app.on_event(paste("AlphaLine\nBetaLine")).unwrap();
    app.on_event(key(KeyCode::Enter)).unwrap();
    let out = render_to_string(&mut app, 120, 24);

    assert!(out.contains("AlphaLine"), "missing textarea text:\n{out}");
    assert!(out.contains("BetaLine"), "missing textarea newline:\n{out}");
    assert!(
        out.contains("title must be a single line"),
        "missing single-line error:\n{out}"
    );
}

#[test]
fn description_page_accepts_multiline_paste_and_enter_commits() {
    let global = GlobalConfig::default();
    let mut app = CreateWizardApp::new(draft(), global, Vec::new());

    app.on_event(key(KeyCode::Enter)).unwrap(); // Title -> Description
    app.on_event(paste("line one\nline two")).unwrap();
    app.on_event(key(KeyCode::Enter)).unwrap();

    assert_eq!(app.draft().description, "line one\nline two");
    assert_eq!(app.page(), &CreateWizardPage::WorkspaceName);
}

#[test]
fn description_shift_enter_inserts_newline_without_submitting() {
    let global = GlobalConfig::default();
    let mut app = CreateWizardApp::new(draft(), global, Vec::new());

    app.on_event(key(KeyCode::Enter)).unwrap(); // Title -> Description
    app.on_event(paste("line one")).unwrap();
    app.on_event(key_mod(KeyCode::Enter, KeyModifiers::SHIFT))
        .unwrap();
    app.on_event(paste("line two")).unwrap();

    assert_eq!(app.page(), &CreateWizardPage::Description);
    assert_eq!(app.draft().description, "");

    app.on_event(key(KeyCode::Enter)).unwrap();

    assert_eq!(app.draft().description, "line one\nline two");
    assert_eq!(app.page(), &CreateWizardPage::WorkspaceName);
}

#[test]
fn run_agent_page_is_conditional_on_after_create_mode() {
    let global = GlobalConfig {
        agent_cli: Some("codex".into()),
        ..Default::default()
    };
    let mut draft = draft();
    draft.after_create = AfterCreateMode::StartAndRunAgent { run_agent: None };
    let app = CreateWizardApp::new(draft, global, Vec::new());

    assert!(app
        .page_titles()
        .iter()
        .any(|title| title == "After create: Run agent"));
}

#[test]
fn target_branch_pages_refresh_when_repo_selection_changes() {
    let global = GlobalConfig::default();
    let mut draft = draft();
    draft
        .repos
        .push(RepoDraftEntry::new("backend", "main", false));
    let mut app = CreateWizardApp::new(draft, global, Vec::new());

    app.set_step(CreateStep::Repos);
    assert!(app
        .page_titles()
        .contains(&"Branches: frontend".to_string()));
    assert!(!app.page_titles().contains(&"Branches: backend".to_string()));

    app.on_event(key(KeyCode::Down)).unwrap();
    app.on_event(key(KeyCode::Char(' '))).unwrap();

    assert!(app.page_titles().contains(&"Branches: backend".to_string()));

    app.on_event(key(KeyCode::Up)).unwrap();
    app.on_event(key(KeyCode::Char(' '))).unwrap();

    assert!(!app
        .page_titles()
        .contains(&"Branches: frontend".to_string()));
    assert!(app.page_titles().contains(&"Branches: backend".to_string()));
}

#[test]
fn run_agent_page_refreshes_when_after_create_mode_changes() {
    let global = GlobalConfig {
        agent_cli: Some("codex".into()),
        ..Default::default()
    };
    let mut app = CreateWizardApp::new(draft(), global, Vec::new());

    app.set_step(CreateStep::AfterCreate);
    assert!(!app
        .page_titles()
        .contains(&"After create: Run agent".to_string()));

    app.on_event(key(KeyCode::Down)).unwrap();
    app.on_event(key(KeyCode::Down)).unwrap();

    assert!(app
        .page_titles()
        .contains(&"After create: Run agent".to_string()));

    app.on_event(key(KeyCode::Up)).unwrap();

    assert!(!app
        .page_titles()
        .contains(&"After create: Run agent".to_string()));
}

#[test]
fn repo_selection_rebuilds_target_branch_pages() {
    let global = GlobalConfig::default();
    let mut draft = draft();
    draft
        .repos
        .push(RepoDraftEntry::new("backend", "main", false));
    let mut app = CreateWizardApp::new(draft, global, Vec::new());

    app.set_step(CreateStep::Repos);

    assert_eq!(
        app.page_titles()
            .into_iter()
            .filter(|title| title.starts_with("Branches: "))
            .collect::<Vec<_>>(),
        vec!["Branches: frontend"]
    );

    app.on_event(key(KeyCode::Down)).unwrap();
    app.on_event(key(KeyCode::Char(' '))).unwrap();

    assert_eq!(
        app.page_titles()
            .into_iter()
            .filter(|title| title.starts_with("Branches: "))
            .collect::<Vec<_>>(),
        vec!["Branches: frontend", "Branches: backend"]
    );
}

#[test]
fn target_branch_page_commits_textarea_to_repo() {
    let global = GlobalConfig::default();
    let mut app = CreateWizardApp::new(draft(), global, Vec::new());

    jump_to_page(
        &mut app,
        CreateWizardPage::TargetBranch {
            repo_name: "frontend".into(),
        },
    );
    clear_text_field_for_test(&mut app);
    app.on_event(paste("feature/current")).unwrap();
    app.on_event(key(KeyCode::Enter)).unwrap();

    assert_eq!(
        app.draft().repo("frontend").unwrap().target_branch,
        "feature/current"
    );
    assert_eq!(app.page(), &CreateWizardPage::AfterCreate);
}

#[test]
fn repos_filter_limits_visible_repos_but_keeps_space_selection() {
    let global = GlobalConfig::default();
    let mut draft = draft();
    draft
        .repos
        .push(RepoDraftEntry::new("backend", "main", false));
    let mut app = CreateWizardApp::new(draft, global, Vec::new());
    app.set_step(CreateStep::Repos);

    app.on_event(key(KeyCode::Tab)).unwrap();
    app.on_event(paste("back")).unwrap();
    app.on_event(key(KeyCode::Char(' '))).unwrap();

    assert!(app.draft().repo("backend").unwrap().selected);
}

#[test]
fn repos_filter_supports_textarea_cursor_shortcuts() {
    let global = GlobalConfig::default();
    let mut draft = draft();
    draft
        .repos
        .push(RepoDraftEntry::new("prefix-backend-tail", "main", false));
    let mut app = CreateWizardApp::new(draft, global, Vec::new());
    app.set_step(CreateStep::Repos);

    app.on_event(key(KeyCode::Tab)).unwrap();
    app.on_event(paste("backend")).unwrap();
    app.on_event(key_mod(KeyCode::Char('a'), KeyModifiers::CONTROL))
        .unwrap();
    app.on_event(paste("prefix-")).unwrap();
    app.on_event(key_mod(KeyCode::Char('e'), KeyModifiers::CONTROL))
        .unwrap();
    app.on_event(paste("-tail")).unwrap();

    let out = render_to_string(&mut app, 120, 24);

    assert!(
        out.contains("filter [active]: prefix-backend-tail"),
        "filter shortcuts did not edit text:\n{out}"
    );
    assert!(
        out.contains("prefix-backend-tail"),
        "matching repo should remain visible:\n{out}"
    );
}

#[test]
fn repos_filter_render_shows_filter_and_only_visible_repos() {
    let global = GlobalConfig::default();
    let mut draft = draft();
    draft
        .repos
        .push(RepoDraftEntry::new("backend", "main", false));
    let mut app = CreateWizardApp::new(draft, global, Vec::new());
    app.set_step(CreateStep::Repos);

    app.on_event(key(KeyCode::Tab)).unwrap();
    app.on_event(paste("back")).unwrap();

    let out = render_to_string(&mut app, 120, 24);

    assert!(
        out.contains("filter [active]: back"),
        "missing filter:\n{out}"
    );
    assert!(out.contains("backend"), "missing matching repo:\n{out}");
    assert!(
        !out.contains("> [x] frontend") && !out.contains("  [x] frontend"),
        "hidden repo should not render in visible list:\n{out}"
    );
}

#[test]
fn repos_page_low_height_keeps_cursor_repo_visible() {
    let global = GlobalConfig::default();
    let mut draft = draft();
    draft.repos = (0..8)
        .map(|idx| RepoDraftEntry::new(format!("repo-{idx}"), "main", true))
        .collect();
    let mut app = CreateWizardApp::new(draft, global, Vec::new());
    app.set_step(CreateStep::Repos);

    for _ in 0..5 {
        app.on_event(key(KeyCode::Down)).unwrap();
    }

    let out = render_to_string(&mut app, 80, 16);

    assert!(
        out.contains("> [x] repo-5"),
        "cursor repo should remain visible in low-height layout:\n{out}"
    );
}

#[test]
fn target_branch_page_rejects_empty_or_multiline_values() {
    for (text, expected_error) in [
        (
            "",
            CreateDraftError::TargetBranchRequired("frontend".into()),
        ),
        (
            "feature\ncurrent",
            CreateDraftError::TargetBranchSingleLineRequired("frontend".into()),
        ),
    ] {
        let global = GlobalConfig::default();
        let mut app = CreateWizardApp::new(draft(), global, Vec::new());

        jump_to_page(
            &mut app,
            CreateWizardPage::TargetBranch {
                repo_name: "frontend".into(),
            },
        );
        clear_text_field_for_test(&mut app);
        app.on_event(paste(text)).unwrap();
        app.on_event(key(KeyCode::Enter)).unwrap();

        assert_eq!(
            app.page(),
            &CreateWizardPage::TargetBranch {
                repo_name: "frontend".into()
            },
            "text: {text:?}"
        );
        assert_eq!(app.errors(), &[expected_error]);
    }
}

#[test]
fn target_branch_error_state_can_go_back_to_repos() {
    let global = GlobalConfig::default();
    let mut invalid = draft();
    invalid.repos = vec![RepoDraftEntry::new("frontend", "", true)];
    let mut app = CreateWizardApp::new(invalid, global, Vec::new());

    jump_to_page(
        &mut app,
        CreateWizardPage::TargetBranch {
            repo_name: "frontend".into(),
        },
    );
    app.on_event(key(KeyCode::Enter)).unwrap();
    assert_eq!(
        app.errors(),
        &[CreateDraftError::TargetBranchRequired("frontend".into())]
    );

    app.on_event(key(KeyCode::Esc)).unwrap();

    assert_eq!(app.page(), &CreateWizardPage::Repos);
}

#[test]
fn target_branch_error_state_goes_directly_to_repos_from_later_repo() {
    let global = GlobalConfig::default();
    let mut invalid = draft();
    invalid.repos = vec![
        RepoDraftEntry::new("frontend", "main", true),
        RepoDraftEntry::new("backend", "", true),
    ];
    let mut app = CreateWizardApp::new(invalid, global, Vec::new());

    jump_to_page(
        &mut app,
        CreateWizardPage::TargetBranch {
            repo_name: "backend".into(),
        },
    );
    app.on_event(key(KeyCode::Enter)).unwrap();
    assert_eq!(
        app.errors(),
        &[CreateDraftError::TargetBranchRequired("backend".into())]
    );

    app.on_event(key(KeyCode::Esc)).unwrap();

    assert_eq!(app.page(), &CreateWizardPage::Repos);
}

#[test]
fn target_branch_esc_commits_valid_buffer_before_going_back() {
    let global = GlobalConfig::default();
    let mut app = CreateWizardApp::new(draft(), global, Vec::new());

    jump_to_page(
        &mut app,
        CreateWizardPage::TargetBranch {
            repo_name: "frontend".into(),
        },
    );
    clear_text_field_for_test(&mut app);
    app.on_event(paste("feature/current")).unwrap();
    app.on_event(key(KeyCode::Esc)).unwrap();

    assert_eq!(app.page(), &CreateWizardPage::Repos);
    assert_eq!(
        app.draft().repo("frontend").unwrap().target_branch,
        "feature/current"
    );
}

#[test]
fn target_branch_page_uses_repo_name_for_validation_errors() {
    let global = GlobalConfig::default();
    let mut draft = CreateDraft::new("auth cleanup", "open-reef", &global);
    draft.repos = vec![RepoDraftEntry::new("title", "main", true)];
    let mut app = CreateWizardApp::new(draft, global, Vec::new());

    jump_to_page(
        &mut app,
        CreateWizardPage::TargetBranch {
            repo_name: "title".into(),
        },
    );
    clear_text_field_for_test(&mut app);
    app.on_event(key(KeyCode::Enter)).unwrap();

    assert_eq!(
        app.errors(),
        &[CreateDraftError::TargetBranchRequired("title".into())]
    );
}

#[test]
fn target_branch_page_title_includes_repo_name() {
    assert_eq!(
        CreateWizardPage::TargetBranch {
            repo_name: "frontend".into(),
        }
        .title(),
        "Branches: frontend"
    );
}

#[test]
fn target_branch_page_help_matches_text_input_mode() {
    let global = GlobalConfig::default();
    let mut app = CreateWizardApp::new(draft(), global, Vec::new());

    jump_to_page(
        &mut app,
        CreateWizardPage::TargetBranch {
            repo_name: "frontend".into(),
        },
    );
    let out = render_to_string(&mut app, 120, 24);

    assert!(
        out.contains("type branch name"),
        "missing text help:\n{out}"
    );
    assert!(
        !out.contains("j/k move") && !out.contains("tab edit field"),
        "stale movement help:\n{out}"
    );
}

#[test]
fn run_agent_help_matches_text_input_mode() {
    let global = GlobalConfig {
        agent_cli: Some("codex".into()),
        ..Default::default()
    };
    let mut draft = draft();
    draft.after_create = AfterCreateMode::StartAndRunAgent { run_agent: None };
    let mut app = CreateWizardApp::new(draft, global, Vec::new());

    jump_to_page(&mut app, CreateWizardPage::RunAgent);
    let out = render_to_string(&mut app, 120, 24);

    assert!(
        out.contains("type agent command"),
        "missing text help:\n{out}"
    );
    assert!(
        !out.contains("j/k move") && !out.contains("tab edit field"),
        "stale movement help:\n{out}"
    );
}

#[test]
fn enter_advances_through_valid_steps() {
    let global = GlobalConfig::default();
    let mut app = CreateWizardApp::new(draft(), global, Vec::new());

    assert_eq!(app.page(), &CreateWizardPage::Title);
    app.on_event(key(KeyCode::Enter)).unwrap();
    assert_eq!(app.page(), &CreateWizardPage::Description);
    app.on_event(key(KeyCode::Enter)).unwrap();
    assert_eq!(app.page(), &CreateWizardPage::WorkspaceName);
    app.on_event(key(KeyCode::Enter)).unwrap();
    assert_eq!(app.page(), &CreateWizardPage::WorkspaceBranch);
    app.on_event(key(KeyCode::Enter)).unwrap();
    assert_eq!(app.page(), &CreateWizardPage::Repos);
    app.on_event(key(KeyCode::Enter)).unwrap();
    assert_eq!(
        app.page(),
        &CreateWizardPage::TargetBranch {
            repo_name: "frontend".into()
        }
    );
    app.on_event(key(KeyCode::Enter)).unwrap();
    assert_eq!(app.page(), &CreateWizardPage::AfterCreate);
    app.on_event(key(KeyCode::Enter)).unwrap();
    assert_eq!(app.page(), &CreateWizardPage::Review);
}

#[test]
fn esc_goes_back_and_first_step_cancels() {
    let global = GlobalConfig::default();
    let mut app = CreateWizardApp::new(draft(), global, Vec::new());

    app.set_step(CreateStep::Review);
    app.on_event(key(KeyCode::Esc)).unwrap();
    assert_eq!(app.page(), &CreateWizardPage::AfterCreate);
    app.on_event(key(KeyCode::Esc)).unwrap();
    assert_eq!(
        app.page(),
        &CreateWizardPage::TargetBranch {
            repo_name: "frontend".into()
        }
    );
    app.on_event(key(KeyCode::Esc)).unwrap();
    assert_eq!(app.page(), &CreateWizardPage::Repos);
    app.on_event(key(KeyCode::Esc)).unwrap();
    assert_eq!(app.page(), &CreateWizardPage::WorkspaceBranch);
    app.on_event(key(KeyCode::Esc)).unwrap();
    assert_eq!(app.page(), &CreateWizardPage::WorkspaceName);
    app.on_event(key(KeyCode::Esc)).unwrap();
    assert_eq!(app.page(), &CreateWizardPage::Description);
    app.on_event(key(KeyCode::Esc)).unwrap();
    assert_eq!(app.page(), &CreateWizardPage::Title);
    app.on_event(key(KeyCode::Esc)).unwrap();

    assert!(app.should_quit());
    assert_eq!(app.outcome(), Some(CreateWizardOutcome::Cancelled));
}

#[test]
fn ctrl_c_interrupts_from_any_step() {
    let steps = [
        CreateStep::Info,
        CreateStep::Repos,
        CreateStep::Branches,
        CreateStep::AfterCreate,
        CreateStep::Review,
    ];
    for step in steps {
        let global = GlobalConfig::default();
        let mut app = CreateWizardApp::new(draft(), global, Vec::new());
        app.set_step(step);

        app.on_event(key_mod(KeyCode::Char('c'), KeyModifiers::CONTROL))
            .unwrap();

        assert!(app.should_quit(), "step {:?} should quit", step);
        assert_eq!(app.outcome(), Some(CreateWizardOutcome::Cancelled));
    }
}

#[test]
fn invalid_step_stays_put_and_exposes_errors() {
    let global = GlobalConfig::default();
    let invalid = CreateDraft::new("", "open-reef", &global);
    let mut app = CreateWizardApp::new(invalid, global, Vec::new());

    app.on_event(key(KeyCode::Enter)).unwrap();

    assert_eq!(app.step(), CreateStep::Info);
    assert_eq!(app.errors(), &[CreateDraftError::TitleRequired]);
}

#[test]
fn title_page_buffers_title_until_enter_commits() {
    let global = GlobalConfig::default();
    let mut invalid = CreateDraft::new("", "open-reef", &global);
    invalid.repos = vec![RepoDraftEntry::new("frontend", "main", true)];
    let mut app = CreateWizardApp::new(invalid, global, Vec::new());

    app.on_event(key(KeyCode::Enter)).unwrap();
    assert_eq!(app.errors(), &[CreateDraftError::TitleRequired]);

    app.on_event(key(KeyCode::Char('A'))).unwrap();
    app.on_event(paste("uth")).unwrap();

    assert_eq!(app.draft().title, "");
    assert!(app.errors().is_empty());

    app.on_event(key(KeyCode::Enter)).unwrap();

    assert_eq!(app.draft().title, "Auth");
    assert_eq!(app.page(), &CreateWizardPage::Description);
}

#[test]
fn workspace_name_page_updates_workspace_dir_and_auto_branch() {
    let global = GlobalConfig {
        workspace_root: "/tmp/zootree-workspaces".into(),
        branch_prefix: "zt".into(),
        ..Default::default()
    };
    let mut draft = CreateDraft::new("auth cleanup", "open", &global);
    draft.repos = vec![RepoDraftEntry::new("frontend", "main", true)];
    let mut app = CreateWizardApp::new(draft, global, Vec::new());
    jump_to_page(&mut app, CreateWizardPage::WorkspaceName);

    clear_text_field_for_test(&mut app);
    app.on_event(paste("wide-tide")).unwrap();
    app.on_event(key(KeyCode::Enter)).unwrap();

    assert_eq!(app.draft().name, "wide-tide");
    assert_eq!(app.draft().branch, "zt/wide-tide");
    assert_eq!(
        app.draft().workspace_dir,
        "/tmp/zootree-workspaces/wide-tide"
    );
    assert_eq!(app.page(), &CreateWizardPage::WorkspaceBranch);
}

#[test]
fn workspace_branch_page_marks_branch_as_edited() {
    let global = GlobalConfig {
        workspace_root: "/tmp/zootree-workspaces".into(),
        branch_prefix: "zt".into(),
        ..Default::default()
    };
    let mut draft = CreateDraft::new("auth cleanup", "open", &global);
    draft.repos = vec![RepoDraftEntry::new("frontend", "main", true)];
    let mut app = CreateWizardApp::new(draft, global, Vec::new());
    jump_to_page(&mut app, CreateWizardPage::WorkspaceBranch);

    clear_text_field_for_test(&mut app);
    app.on_event(paste("feature/manual")).unwrap();
    app.on_event(key(KeyCode::Enter)).unwrap();

    assert_eq!(app.draft().branch, "feature/manual");
    assert!(app.draft().branch_was_edited);
    assert_eq!(app.page(), &CreateWizardPage::Repos);
}

#[test]
fn workspace_branch_page_rejects_empty_or_multiline_values() {
    for (text, expected_error) in [
        ("", CreateDraftError::WorkspaceBranchRequired),
        (
            "feature\nmanual",
            CreateDraftError::WorkspaceBranchSingleLineRequired,
        ),
    ] {
        let global = GlobalConfig::default();
        let mut draft = CreateDraft::new("auth cleanup", "open", &global);
        draft.repos = vec![RepoDraftEntry::new("frontend", "main", true)];
        let mut app = CreateWizardApp::new(draft, global, Vec::new());
        jump_to_page(&mut app, CreateWizardPage::WorkspaceBranch);

        clear_text_field_for_test(&mut app);
        app.on_event(paste(text)).unwrap();
        app.on_event(key(KeyCode::Enter)).unwrap();

        assert_eq!(app.page(), &CreateWizardPage::WorkspaceBranch);
        assert_eq!(app.errors(), &[expected_error]);
    }
}

#[test]
fn workspace_name_page_rejects_empty_or_multiline_values() {
    for (text, expected_error) in [
        ("", CreateDraftError::WorkspaceNameRequired),
        (
            "wide\ntide",
            CreateDraftError::WorkspaceNameSingleLineRequired,
        ),
    ] {
        let global = GlobalConfig::default();
        let mut draft = CreateDraft::new("auth cleanup", "open", &global);
        draft.repos = vec![RepoDraftEntry::new("frontend", "main", true)];
        let mut app = CreateWizardApp::new(draft, global, Vec::new());
        jump_to_page(&mut app, CreateWizardPage::WorkspaceName);

        clear_text_field_for_test(&mut app);
        app.on_event(paste(text)).unwrap();
        app.on_event(key(KeyCode::Enter)).unwrap();

        assert_eq!(app.page(), &CreateWizardPage::WorkspaceName);
        assert_eq!(app.errors(), &[expected_error]);
    }
}

#[test]
fn workspace_name_page_rejects_existing_workspace_name() {
    let global = GlobalConfig::default();
    let mut app = CreateWizardApp::new(draft(), global, vec!["open-reef".into()]);

    jump_to_page(&mut app, CreateWizardPage::WorkspaceName);
    app.on_event(key(KeyCode::Enter)).unwrap();

    assert_eq!(app.page(), &CreateWizardPage::WorkspaceName);
    assert_eq!(
        app.errors(),
        &[CreateDraftError::WorkspaceNameExists("open-reef".into())]
    );
}

#[test]
fn workspace_text_pages_keep_tab_inside_input() {
    let global = GlobalConfig {
        workspace_root: "/tmp/zootree-workspaces".into(),
        branch_prefix: "zt".into(),
        ..Default::default()
    };
    let mut draft = CreateDraft::new("auth cleanup", "open", &global);
    draft.repos = vec![RepoDraftEntry::new("frontend", "main", true)];
    let mut app = CreateWizardApp::new(draft, global, Vec::new());

    app.on_event(key(KeyCode::Enter)).unwrap();
    app.on_event(key(KeyCode::Enter)).unwrap();
    assert_eq!(app.page(), &CreateWizardPage::WorkspaceName);

    app.on_event(key(KeyCode::Tab)).unwrap();
    app.on_event(paste("-manual")).unwrap();

    assert_eq!(app.page(), &CreateWizardPage::WorkspaceName);
    assert_eq!(app.draft().title, "auth cleanup");
    assert_eq!(app.draft().name, "open");
    assert_eq!(app.draft().branch, "zt/open");
    assert!(!app.draft().branch_was_edited);
}

#[test]
fn low_height_text_page_keeps_errors_visible_before_context() {
    let global = GlobalConfig::default();
    let mut app = CreateWizardApp::new(draft(), global, Vec::new());

    jump_to_page(
        &mut app,
        CreateWizardPage::TargetBranch {
            repo_name: "frontend".into(),
        },
    );
    clear_text_field_for_test(&mut app);
    app.on_event(key(KeyCode::Enter)).unwrap();

    let out = render_to_string(&mut app, 120, 4);

    assert!(
        out.contains("target branch is required for frontend"),
        "missing low-height error:\n{out}"
    );
}

#[test]
fn review_help_does_not_advertise_missing_movement() {
    let global = GlobalConfig::default();
    let mut app = CreateWizardApp::new(draft(), global, Vec::new());
    app.set_step(CreateStep::Review);

    let out = render_to_string(&mut app, 120, 24);

    assert!(out.contains("enter submit"), "missing review help:\n{out}");
    assert!(
        !out.contains("j/k move") && !out.contains("up/down"),
        "stale review movement help:\n{out}"
    );
}

#[test]
fn workspace_name_and_branch_pages_buffer_until_submit() {
    let global = GlobalConfig {
        workspace_root: "/tmp/zootree-workspaces".into(),
        branch_prefix: "zt".into(),
        ..Default::default()
    };
    let mut draft = CreateDraft::new("auth cleanup", "open", &global);
    draft.repos = vec![RepoDraftEntry::new("frontend", "main", true)];
    let mut app = CreateWizardApp::new(draft, global, Vec::new());

    app.on_event(key(KeyCode::Enter)).unwrap();
    app.on_event(key(KeyCode::Enter)).unwrap();
    assert_eq!(app.page(), &CreateWizardPage::WorkspaceName);

    app.on_event(paste("-reeg")).unwrap();
    app.on_event(key(KeyCode::Backspace)).unwrap();
    app.on_event(key(KeyCode::Char('f'))).unwrap();

    assert_eq!(app.draft().title, "auth cleanup");
    assert_eq!(app.draft().name, "open");
    assert_eq!(app.draft().workspace_dir, "/tmp/zootree-workspaces/open");
    assert_eq!(app.draft().branch, "zt/open");
    assert!(!app.draft().branch_was_edited);

    app.on_event(key(KeyCode::Enter)).unwrap();

    assert_eq!(app.draft().name, "open-reef");
    assert_eq!(
        app.draft().workspace_dir,
        "/tmp/zootree-workspaces/open-reef"
    );
    assert_eq!(app.draft().branch, "zt/open-reef");
    assert!(!app.draft().branch_was_edited);

    assert_eq!(app.page(), &CreateWizardPage::WorkspaceBranch);
    app.on_event(paste("-manual")).unwrap();

    assert_eq!(app.draft().branch, "zt/open-reef");
    assert!(!app.draft().branch_was_edited);

    app.on_event(key(KeyCode::Enter)).unwrap();

    assert_eq!(app.draft().branch, "zt/open-reef-manual");
    assert!(app.draft().branch_was_edited);
}

#[test]
fn repos_step_exposes_only_repo_required() {
    let global = GlobalConfig::default();
    let mut invalid = draft();
    invalid.repos = vec![RepoDraftEntry::new("frontend", "main", false)];
    let mut app = CreateWizardApp::new(invalid, global, Vec::new());
    app.set_step(CreateStep::Repos);

    app.on_event(key(KeyCode::Enter)).unwrap();

    assert_eq!(app.step(), CreateStep::Repos);
    assert_eq!(app.errors(), &[CreateDraftError::RepoRequired]);

    app.on_event(key(KeyCode::Char(' '))).unwrap();

    assert!(app.draft().repo("frontend").unwrap().selected);
    assert!(!app.errors().contains(&CreateDraftError::RepoRequired));
}

#[test]
fn repos_error_state_can_go_back_to_info() {
    let global = GlobalConfig::default();
    let mut invalid = draft();
    invalid.repos = vec![RepoDraftEntry::new("frontend", "main", false)];
    let mut app = CreateWizardApp::new(invalid, global, Vec::new());
    app.set_step(CreateStep::Repos);

    app.on_event(key(KeyCode::Enter)).unwrap();
    assert_eq!(app.errors(), &[CreateDraftError::RepoRequired]);

    app.on_event(key(KeyCode::Esc)).unwrap();

    assert_eq!(app.page(), &CreateWizardPage::WorkspaceBranch);
}

#[test]
fn branches_step_exposes_only_target_branch_errors() {
    let global = GlobalConfig::default();
    let mut invalid = draft();
    invalid.repos = vec![RepoDraftEntry::new("frontend", "", true)];
    let mut app = CreateWizardApp::new(invalid, global, Vec::new());
    app.set_step(CreateStep::Branches);

    app.on_event(key(KeyCode::Enter)).unwrap();

    assert_eq!(app.step(), CreateStep::Branches);
    assert_eq!(
        app.errors(),
        &[CreateDraftError::TargetBranchRequired("frontend".into())]
    );
}

#[test]
fn branches_error_state_can_go_back_to_repos() {
    let global = GlobalConfig::default();
    let mut app = CreateWizardApp::new(draft(), global, Vec::new());
    app.set_step(CreateStep::Branches);

    app.on_event(key(KeyCode::Esc)).unwrap();

    assert_eq!(app.page(), &CreateWizardPage::Repos);
}

#[test]
fn branches_step_edits_selected_repo_target_branch() {
    let global = GlobalConfig::default();
    let mut invalid = draft();
    invalid.repos = vec![RepoDraftEntry::new("frontend", "", true)];
    let mut app = CreateWizardApp::new(invalid, global, Vec::new());
    app.set_step(CreateStep::Branches);

    app.on_event(key(KeyCode::Enter)).unwrap();
    assert_eq!(
        app.errors(),
        &[CreateDraftError::TargetBranchRequired("frontend".into())]
    );

    app.on_event(key(KeyCode::Char('f'))).unwrap();
    app.on_event(paste("eag")).unwrap();
    app.on_event(key(KeyCode::Backspace)).unwrap();
    app.on_event(paste("ture/current")).unwrap();
    assert!(app.errors().is_empty());

    app.on_event(key(KeyCode::Enter)).unwrap();

    assert_eq!(
        app.draft().repo("frontend").unwrap().target_branch,
        "feature/current"
    );
    assert_eq!(app.step(), CreateStep::AfterCreate);
}

#[test]
fn target_branch_textarea_keeps_jk_as_branch_text() {
    let global = GlobalConfig::default();
    let mut draft = draft();
    draft
        .repos
        .push(RepoDraftEntry::new("backend", "develop", true));
    let mut app = CreateWizardApp::new(draft, global, Vec::new());
    app.set_step(CreateStep::Branches);

    app.on_event(key(KeyCode::Char('j'))).unwrap();
    app.on_event(key(KeyCode::Char('k'))).unwrap();
    app.on_event(paste("-paste")).unwrap();
    app.on_event(key(KeyCode::Enter)).unwrap();

    assert_eq!(
        app.draft().repo("frontend").unwrap().target_branch,
        "mainjk-paste"
    );
}

#[test]
fn after_create_step_defers_default_agent_missing_to_run_agent_page() {
    let global = GlobalConfig::default();
    let mut invalid = draft();
    invalid.after_create = AfterCreateMode::StartAndRunAgent { run_agent: None };
    let mut app = CreateWizardApp::new(invalid, global, Vec::new());
    app.set_step(CreateStep::AfterCreate);

    app.on_event(key(KeyCode::Enter)).unwrap();

    assert_eq!(app.page(), &CreateWizardPage::RunAgent);
    assert!(app.errors().is_empty());

    app.on_event(key(KeyCode::Esc)).unwrap();
    app.on_event(key(KeyCode::Up)).unwrap();

    assert_eq!(app.after_create_cursor(), 1);
    assert_eq!(app.draft().after_create, AfterCreateMode::Start);
    assert!(!app
        .errors()
        .contains(&CreateDraftError::DefaultAgentMissing));
}

#[test]
fn review_step_exposes_all_errors_and_does_not_submit() {
    let global = GlobalConfig::default();
    let mut invalid = CreateDraft::new("", "open-reef", &global);
    invalid.repos = vec![RepoDraftEntry::new("frontend", "", true)];
    invalid.after_create = AfterCreateMode::StartAndRunAgent { run_agent: None };
    let mut app = CreateWizardApp::new(invalid, global, vec!["open-reef".into()]);
    app.set_step(CreateStep::Review);

    app.on_event(key(KeyCode::Enter)).unwrap();

    assert_eq!(app.step(), CreateStep::Review);
    assert_eq!(
        app.errors(),
        &[
            CreateDraftError::TitleRequired,
            CreateDraftError::WorkspaceNameExists("open-reef".into()),
            CreateDraftError::TargetBranchRequired("frontend".into()),
            CreateDraftError::DefaultAgentMissing,
        ]
    );
    assert!(!app.should_quit());
    assert_eq!(app.outcome(), None);
}

#[test]
fn review_error_state_can_go_back_to_after_create() {
    let global = GlobalConfig::default();
    let mut invalid = CreateDraft::new("", "open-reef", &global);
    invalid.repos = vec![RepoDraftEntry::new("frontend", "", true)];
    invalid.after_create = AfterCreateMode::StartAndRunAgent { run_agent: None };
    let mut app = CreateWizardApp::new(invalid, global, vec!["open-reef".into()]);
    app.set_step(CreateStep::Review);

    app.on_event(key(KeyCode::Enter)).unwrap();
    assert_eq!(app.step(), CreateStep::Review);
    assert!(!app.errors().is_empty());

    app.on_event(key(KeyCode::Esc)).unwrap();

    assert_eq!(app.page(), &CreateWizardPage::RunAgent);
}

#[test]
fn review_enter_submits_output() {
    let global = GlobalConfig::default();
    let mut app = CreateWizardApp::new(draft(), global, Vec::new());

    app.set_step(CreateStep::Review);
    app.on_event(key(KeyCode::Enter)).unwrap();

    assert!(app.should_quit());
    match app.outcome() {
        Some(CreateWizardOutcome::Submit(output)) => {
            assert_eq!(output.draft.title, "auth cleanup");
        }
        other => panic!("expected submitted output, got {:?}", other),
    }
}

#[test]
fn repo_list_movement_aliases_change_repo_cursor() {
    let global = GlobalConfig::default();
    let mut draft = draft();
    draft
        .repos
        .push(RepoDraftEntry::new("backend", "main", true));
    let mut app = CreateWizardApp::new(draft, global, Vec::new());
    app.set_step(CreateStep::Repos);

    assert_eq!(app.repo_cursor(), 0);
    app.on_event(key(KeyCode::Down)).unwrap();
    assert_eq!(app.repo_cursor(), 1);
    app.on_event(key(KeyCode::Up)).unwrap();
    assert_eq!(app.repo_cursor(), 0);
    app.on_event(key(KeyCode::Char('j'))).unwrap();
    assert_eq!(app.repo_cursor(), 1);
    app.on_event(key(KeyCode::Char('k'))).unwrap();
    assert_eq!(app.repo_cursor(), 0);
    app.on_event(key_mod(KeyCode::Char('n'), KeyModifiers::CONTROL))
        .unwrap();
    assert_eq!(app.repo_cursor(), 1);
    app.on_event(key_mod(KeyCode::Char('p'), KeyModifiers::CONTROL))
        .unwrap();
    assert_eq!(app.repo_cursor(), 0);
}

#[test]
fn space_toggles_repo_on_repos_step() {
    let global = GlobalConfig::default();
    let mut draft = draft();
    draft
        .repos
        .push(RepoDraftEntry::new("backend", "main", true));
    let mut app = CreateWizardApp::new(draft, global, Vec::new());
    app.set_step(CreateStep::Repos);

    app.on_event(key(KeyCode::Down)).unwrap();
    app.on_event(key(KeyCode::Char(' '))).unwrap();

    assert!(app.draft().repo("frontend").unwrap().selected);
    assert!(!app.draft().repo("backend").unwrap().selected);
}

#[test]
fn repo_and_branch_cursor_movement_wraps() {
    let global = GlobalConfig::default();
    let mut draft = draft();
    draft
        .repos
        .push(RepoDraftEntry::new("backend", "develop", true));
    let mut app = CreateWizardApp::new(draft, global, Vec::new());

    app.set_step(CreateStep::Repos);
    app.on_event(key(KeyCode::Up)).unwrap();
    assert_eq!(app.repo_cursor(), 1);
    app.on_event(key(KeyCode::Down)).unwrap();
    assert_eq!(app.repo_cursor(), 0);

    app.set_step(CreateStep::Branches);
    assert_eq!(
        app.page(),
        &CreateWizardPage::TargetBranch {
            repo_name: "frontend".into()
        }
    );
    app.on_event(key(KeyCode::Enter)).unwrap();
    assert_eq!(
        app.page(),
        &CreateWizardPage::TargetBranch {
            repo_name: "backend".into()
        }
    );
}

#[test]
fn target_branch_pages_rebuild_after_repo_deselection_before_entering_branches() {
    let global = GlobalConfig::default();
    let mut draft = draft();
    draft
        .repos
        .push(RepoDraftEntry::new("backend", "develop", true));
    draft.repos.push(RepoDraftEntry::new("docs", "main", true));
    let mut app = CreateWizardApp::new(draft, global, Vec::new());
    app.set_step(CreateStep::Branches);

    app.on_event(key(KeyCode::Esc)).unwrap();
    assert_eq!(app.step(), CreateStep::Repos);
    app.on_event(key(KeyCode::Down)).unwrap();
    app.on_event(key(KeyCode::Down)).unwrap();
    assert_eq!(app.repo_cursor(), 2);
    app.on_event(key(KeyCode::Char(' '))).unwrap();
    assert!(!app.draft().repo("docs").unwrap().selected);

    app.on_event(key(KeyCode::Enter)).unwrap();

    assert_eq!(app.step(), CreateStep::Branches);
    assert_eq!(
        app.page_titles()
            .into_iter()
            .filter(|title| title.starts_with("Branches: "))
            .collect::<Vec<_>>(),
        vec!["Branches: frontend", "Branches: backend"]
    );
}

#[test]
fn after_create_mode_can_cycle_to_run_agent() {
    let global = GlobalConfig {
        agent_cli: Some("codex".into()),
        ..Default::default()
    };
    let mut app = CreateWizardApp::new(draft(), global, Vec::new());
    app.set_step(CreateStep::AfterCreate);

    app.on_event(key(KeyCode::Right)).unwrap();
    app.on_event(key(KeyCode::Right)).unwrap();

    assert_eq!(app.after_create_cursor(), 2);
    assert_eq!(
        app.draft().after_create,
        AfterCreateMode::StartAndRunAgent { run_agent: None }
    );
}

#[test]
fn after_create_movement_selects_modes_and_validates_agent_only_for_run_agent() {
    let global = GlobalConfig::default();
    let mut app = CreateWizardApp::new(draft(), global, Vec::new());
    app.set_step(CreateStep::AfterCreate);

    assert_eq!(app.after_create_cursor(), 0);
    assert_eq!(app.draft().after_create, AfterCreateMode::CreateOnly);
    app.on_event(key(KeyCode::Down)).unwrap();
    assert_eq!(app.after_create_cursor(), 1);
    assert_eq!(app.draft().after_create, AfterCreateMode::Start);
    app.on_event(key(KeyCode::Enter)).unwrap();
    assert_eq!(app.step(), CreateStep::Review);
    assert!(app.errors().is_empty());

    let global = GlobalConfig::default();
    let mut app = CreateWizardApp::new(draft(), global, Vec::new());
    app.set_step(CreateStep::AfterCreate);
    app.on_event(key_mod(KeyCode::Char('n'), KeyModifiers::CONTROL))
        .unwrap();
    app.on_event(key_mod(KeyCode::Char('n'), KeyModifiers::CONTROL))
        .unwrap();
    assert_eq!(app.after_create_cursor(), 2);
    assert_eq!(
        app.draft().after_create,
        AfterCreateMode::StartAndRunAgent { run_agent: None }
    );
    app.on_event(key(KeyCode::Enter)).unwrap();
    assert_eq!(app.page(), &CreateWizardPage::RunAgent);

    app.on_event(key(KeyCode::Enter)).unwrap();
    assert_eq!(app.page(), &CreateWizardPage::RunAgent);
    assert_eq!(app.errors(), &[CreateDraftError::DefaultAgentMissing]);

    app.on_event(key(KeyCode::Esc)).unwrap();
    app.on_event(key(KeyCode::Up)).unwrap();
    assert_eq!(app.after_create_cursor(), 1);
    assert_eq!(app.draft().after_create, AfterCreateMode::Start);
    app.on_event(key(KeyCode::Enter)).unwrap();
    assert_eq!(app.step(), CreateStep::Review);

    let global = GlobalConfig {
        agent_cli: Some("codex".into()),
        ..Default::default()
    };
    let mut app = CreateWizardApp::new(draft(), global, Vec::new());
    app.set_step(CreateStep::AfterCreate);
    app.on_event(key(KeyCode::Char('j'))).unwrap();
    app.on_event(key(KeyCode::Char('j'))).unwrap();
    assert_eq!(app.after_create_cursor(), 2);
    app.on_event(key(KeyCode::Enter)).unwrap();
    assert_eq!(app.page(), &CreateWizardPage::RunAgent);
    app.on_event(key(KeyCode::Enter)).unwrap();
    assert_eq!(app.step(), CreateStep::Review);
}

#[test]
fn removing_run_agent_page_keeps_page_and_step_in_sync_before_enter() {
    let global = GlobalConfig {
        agent_cli: Some("codex".into()),
        ..Default::default()
    };
    let mut draft = draft();
    draft.after_create = AfterCreateMode::StartAndRunAgent { run_agent: None };
    let mut app = CreateWizardApp::new(draft, global, Vec::new());
    app.set_step(CreateStep::AfterCreate);

    app.on_event(key(KeyCode::Enter)).unwrap();
    assert_eq!(app.page(), &CreateWizardPage::RunAgent);

    app.on_event(key(KeyCode::Esc)).unwrap();
    app.on_event(key(KeyCode::Up)).unwrap();

    assert_eq!(app.draft().after_create, AfterCreateMode::Start);
    assert_eq!(app.page(), &CreateWizardPage::AfterCreate);
    assert_eq!(app.step(), CreateStep::AfterCreate);

    app.on_event(key(KeyCode::Enter)).unwrap();

    assert_eq!(app.page(), &CreateWizardPage::Review);
    assert_eq!(app.outcome(), None);
}

#[test]
fn after_create_run_agent_mode_inserts_run_agent_page() {
    let global = GlobalConfig {
        agent_cli: Some("codex".into()),
        ..Default::default()
    };
    let mut app = CreateWizardApp::new(draft(), global, Vec::new());
    jump_to_page(&mut app, CreateWizardPage::AfterCreate);

    app.on_event(key(KeyCode::Down)).unwrap();
    app.on_event(key(KeyCode::Down)).unwrap();
    app.on_event(key(KeyCode::Enter)).unwrap();

    assert_eq!(app.page(), &CreateWizardPage::RunAgent);
}

#[test]
fn run_agent_page_empty_uses_global_default_and_advances_to_review() {
    let global = GlobalConfig {
        agent_cli: Some("codex".into()),
        ..Default::default()
    };
    let mut draft = draft();
    draft.after_create = AfterCreateMode::StartAndRunAgent { run_agent: None };
    let mut app = CreateWizardApp::new(draft, global, Vec::new());
    jump_to_page(&mut app, CreateWizardPage::RunAgent);

    clear_text_field_for_test(&mut app);
    app.on_event(key(KeyCode::Enter)).unwrap();

    assert_eq!(app.page(), &CreateWizardPage::Review);
    assert_eq!(
        app.draft().after_create,
        AfterCreateMode::StartAndRunAgent { run_agent: None }
    );
}

#[test]
fn run_agent_page_literal_commits_to_draft() {
    let global = GlobalConfig::default();
    let mut draft = draft();
    draft.after_create = AfterCreateMode::StartAndRunAgent { run_agent: None };
    let mut app = CreateWizardApp::new(draft, global, Vec::new());
    jump_to_page(&mut app, CreateWizardPage::RunAgent);

    clear_text_field_for_test(&mut app);
    app.on_event(paste("codex")).unwrap();
    app.on_event(key(KeyCode::Enter)).unwrap();

    assert_eq!(app.page(), &CreateWizardPage::Review);
    assert_eq!(
        app.draft().after_create,
        AfterCreateMode::StartAndRunAgent {
            run_agent: Some("codex".into())
        }
    );
}

#[test]
fn run_agent_page_rejects_empty_without_global_default() {
    let global = GlobalConfig::default();
    let mut draft = draft();
    draft.after_create = AfterCreateMode::StartAndRunAgent { run_agent: None };
    let mut app = CreateWizardApp::new(draft, global, Vec::new());
    jump_to_page(&mut app, CreateWizardPage::RunAgent);

    clear_text_field_for_test(&mut app);
    app.on_event(key(KeyCode::Enter)).unwrap();

    assert_eq!(app.page(), &CreateWizardPage::RunAgent);
    assert_eq!(app.errors(), &[CreateDraftError::DefaultAgentMissing]);
}

#[test]
fn run_agent_page_rejects_multiline_value() {
    let global = GlobalConfig {
        agent_cli: Some("codex".into()),
        ..Default::default()
    };
    let mut draft = draft();
    draft.after_create = AfterCreateMode::StartAndRunAgent { run_agent: None };
    let mut app = CreateWizardApp::new(draft, global, Vec::new());
    jump_to_page(&mut app, CreateWizardPage::RunAgent);

    clear_text_field_for_test(&mut app);
    app.on_event(paste("codex\nagent")).unwrap();
    app.on_event(key(KeyCode::Enter)).unwrap();

    assert_eq!(app.page(), &CreateWizardPage::RunAgent);
    assert_eq!(
        app.errors(),
        &[CreateDraftError::RunAgentSingleLineRequired]
    );

    let out = render_to_string(&mut app, 120, 24);
    assert!(
        out.contains("run-agent must be a single line"),
        "missing single-line error:\n{out}"
    );
    assert!(
        !out.contains("configure a default agent"),
        "misleading default-agent error:\n{out}"
    );
}

#[test]
fn run_agent_esc_commits_valid_buffer_before_going_back() {
    let global = GlobalConfig::default();
    let mut draft = draft();
    draft.after_create = AfterCreateMode::StartAndRunAgent { run_agent: None };
    let mut app = CreateWizardApp::new(draft, global, Vec::new());
    jump_to_page(&mut app, CreateWizardPage::RunAgent);

    clear_text_field_for_test(&mut app);
    app.on_event(paste("codex")).unwrap();
    app.on_event(key(KeyCode::Esc)).unwrap();

    assert_eq!(app.page(), &CreateWizardPage::AfterCreate);
    assert_eq!(
        app.draft().after_create,
        AfterCreateMode::StartAndRunAgent {
            run_agent: Some("codex".into())
        }
    );

    app.on_event(key(KeyCode::Enter)).unwrap();
    assert_eq!(app.page(), &CreateWizardPage::RunAgent);
    app.on_event(key(KeyCode::Enter)).unwrap();
    assert_eq!(app.page(), &CreateWizardPage::Review);
}

#[test]
fn run_agent_failed_empty_submit_preserves_existing_literal() {
    let global = GlobalConfig::default();
    let mut draft = draft();
    draft.after_create = AfterCreateMode::StartAndRunAgent {
        run_agent: Some("codex".into()),
    };
    let mut app = CreateWizardApp::new(draft, global, Vec::new());
    jump_to_page(&mut app, CreateWizardPage::RunAgent);

    clear_text_field_for_test(&mut app);
    app.on_event(key(KeyCode::Enter)).unwrap();

    assert_eq!(app.page(), &CreateWizardPage::RunAgent);
    assert_eq!(app.errors(), &[CreateDraftError::DefaultAgentMissing]);
    assert_eq!(
        app.draft().after_create,
        AfterCreateMode::StartAndRunAgent {
            run_agent: Some("codex".into())
        }
    );
}

#[test]
fn run_agent_page_keeps_jk_as_text() {
    let global = GlobalConfig::default();
    let mut draft = draft();
    draft.after_create = AfterCreateMode::StartAndRunAgent { run_agent: None };
    let mut app = CreateWizardApp::new(draft, global, Vec::new());
    jump_to_page(&mut app, CreateWizardPage::RunAgent);

    clear_text_field_for_test(&mut app);
    app.on_event(key(KeyCode::Char('j'))).unwrap();
    app.on_event(key(KeyCode::Char('k'))).unwrap();
    app.on_event(paste("-agent")).unwrap();
    app.on_event(key(KeyCode::Enter)).unwrap();

    assert_eq!(
        app.draft().after_create,
        AfterCreateMode::StartAndRunAgent {
            run_agent: Some("jk-agent".into())
        }
    );
}

#[test]
fn after_create_tab_does_not_edit_run_agent_inline() {
    let global = GlobalConfig::default();
    let mut app = CreateWizardApp::new(draft(), global, Vec::new());
    app.set_step(CreateStep::AfterCreate);

    app.on_event(key(KeyCode::Char('j'))).unwrap();
    app.on_event(key(KeyCode::Char('j'))).unwrap();
    assert_eq!(app.after_create_cursor(), 2);

    app.on_event(key(KeyCode::Tab)).unwrap();
    app.on_event(key(KeyCode::Char('c'))).unwrap();
    app.on_event(paste("odex")).unwrap();

    assert_eq!(
        app.draft().after_create,
        AfterCreateMode::StartAndRunAgent { run_agent: None }
    );
}

#[test]
fn after_create_preserves_explicit_run_agent_without_global_default() {
    let global = GlobalConfig::default();
    let mut draft = draft();
    draft.after_create = AfterCreateMode::StartAndRunAgent {
        run_agent: Some("codex".into()),
    };
    let mut app = CreateWizardApp::new(draft, global, Vec::new());
    app.set_step(CreateStep::AfterCreate);

    assert_eq!(app.after_create_cursor(), 2);
    app.on_event(key(KeyCode::Up)).unwrap();
    assert_eq!(app.after_create_cursor(), 1);
    assert_eq!(app.draft().after_create, AfterCreateMode::Start);
    app.on_event(key(KeyCode::Down)).unwrap();
    assert_eq!(app.after_create_cursor(), 2);
    app.on_event(key(KeyCode::Enter)).unwrap();

    assert_eq!(app.page(), &CreateWizardPage::RunAgent);
    app.on_event(key(KeyCode::Enter)).unwrap();
    assert_eq!(app.step(), CreateStep::Review);
    assert!(app.errors().is_empty());
    assert_eq!(
        app.draft().after_create,
        AfterCreateMode::StartAndRunAgent {
            run_agent: Some("codex".into())
        }
    );
}
