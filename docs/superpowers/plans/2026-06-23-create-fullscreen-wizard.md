# Create Fullscreen Wizard Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace interactive `zootree create` with a full-screen, reversible wizard while keeping complete CLI-argument flows unchanged.

**Architecture:** Add a focused create-flow layer under `src/cli/create_flow.rs` for draft/default/output logic, then add `src/tui_app/create_wizard.rs` for wizard state and rendering. `src/cli/workspace.rs` remains the command orchestration layer: it decides when to enter the wizard, saves `WorkspaceConfig`, and reuses `handle_start` for after-create actions.

**Tech Stack:** Rust 2021, clap, ratatui 0.30, crossterm 0.29, ratatui-textarea 0.9, existing `ConfigManager`, `GitOps`, `NameGenerator`, and `tui_app::run_app`.

Design spec: `docs/superpowers/specs/2026-06-23-create-fullscreen-wizard-design.md`.

---

## File Structure

| File | Responsibility |
|---|---|
| `src/cli/create_flow.rs` | Create-specific data model, draft defaults, repo/template application, validation, conversion to `WorkspaceConfig`, and after-create decision types. |
| `src/cli/mod.rs` | Export the new `create_flow` module. |
| `src/cli/workspace.rs` | Keep `CreateArgs`, existing command handlers, and create orchestration; delegate create-specific pure logic to `create_flow`; call `CreateWizardApp` only for interactive create. |
| `src/tui_app/create_wizard.rs` | Full-screen create wizard state machine, key handling, responsive layout calculation, and rendering. |
| `src/tui_app/mod.rs` | Export the new `create_wizard` module; no runner rewrite expected. |
| `tests/create_flow_test.rs` | Tests for defaults, repo/template behavior, target branch selection, after-create validation, output conversion, and non-interactive decision helpers. |
| `tests/create_wizard_test.rs` | Tests for wizard state transitions, key handling, validation gates, responsive layout selection, and cancellation outcome. |

Do not change workspace/repo/template config schemas. Do not change `src/tui.rs` public prompt functions.

---

### Task 1: Create Flow Core Types and Pure Draft Behavior

**Files:**
- Create: `src/cli/create_flow.rs`
- Modify: `src/cli/mod.rs`
- Test: `tests/create_flow_test.rs`

- [ ] **Step 1: Write failing tests for pure draft behavior**

Create `tests/create_flow_test.rs` with:

```rust
use zootree::cli::create_flow::{
    AfterCreateMode, CreateDraft, CreateWizardLayout, RepoDraftEntry,
};
use zootree::config::global::GlobalConfig;

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
    assert_eq!(CreateWizardLayout::for_width(120), CreateWizardLayout::TwoColumn);
    assert_eq!(CreateWizardLayout::for_width(80), CreateWizardLayout::SingleColumn);
    assert_eq!(CreateWizardLayout::for_width(40), CreateWizardLayout::TooNarrow);
}
```

- [ ] **Step 2: Run tests and verify they fail**

Run:

```bash
cargo test --test create_flow_test 2>&1 | tee /tmp/zootree-create-flow-task1.log
```

Expected: FAIL with unresolved import `zootree::cli::create_flow`.

- [ ] **Step 3: Add the core create-flow module**

Create `src/cli/create_flow.rs`:

```rust
use crate::config::global::GlobalConfig;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AfterCreateMode {
    CreateOnly,
    Start,
    StartAndRunAgent { run_agent: Option<String> },
}

impl AfterCreateMode {
    pub fn should_start(&self) -> bool {
        !matches!(self, Self::CreateOnly)
    }

    pub fn run_agent_arg(&self) -> Option<Option<String>> {
        match self {
            Self::CreateOnly | Self::Start => None,
            Self::StartAndRunAgent { run_agent } => Some(run_agent.clone()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoDraftEntry {
    pub name: String,
    pub target_branch: String,
    pub selected: bool,
}

impl RepoDraftEntry {
    pub fn new(name: impl Into<String>, target_branch: impl Into<String>, selected: bool) -> Self {
        Self {
            name: name.into(),
            target_branch: target_branch.into(),
            selected,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateDraft {
    pub title: String,
    pub description: String,
    pub name: String,
    pub branch: String,
    pub branch_was_edited: bool,
    pub workspace_dir: String,
    pub repos: Vec<RepoDraftEntry>,
    pub after_create: AfterCreateMode,
}

impl CreateDraft {
    pub fn new(title: impl Into<String>, name: impl Into<String>, global: &GlobalConfig) -> Self {
        let name = name.into();
        Self {
            title: title.into(),
            description: String::new(),
            branch: default_branch(global, &name),
            branch_was_edited: false,
            workspace_dir: default_workspace_dir(global, &name),
            name,
            repos: Vec::new(),
            after_create: AfterCreateMode::CreateOnly,
        }
    }

    pub fn set_name(&mut self, name: impl Into<String>, global: &GlobalConfig) {
        self.name = name.into();
        if !self.branch_was_edited {
            self.branch = default_branch(global, &self.name);
        }
        self.workspace_dir = default_workspace_dir(global, &self.name);
    }

    pub fn set_branch(&mut self, branch: impl Into<String>) {
        self.branch = branch.into();
        self.branch_was_edited = true;
    }

    pub fn repo(&self, name: &str) -> Option<&RepoDraftEntry> {
        self.repos.iter().find(|repo| repo.name == name)
    }

    pub fn toggle_repo(&mut self, name: &str) {
        if let Some(repo) = self.repos.iter_mut().find(|repo| repo.name == name) {
            repo.selected = !repo.selected;
        }
    }

    pub fn apply_template_repos(&mut self, template_repos: &[String]) {
        for repo in &mut self.repos {
            repo.selected = template_repos.iter().any(|name| name == &repo.name);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateWizardLayout {
    TwoColumn,
    SingleColumn,
    TooNarrow,
}

impl CreateWizardLayout {
    pub fn for_width(width: u16) -> Self {
        if width < 50 {
            Self::TooNarrow
        } else if width >= 100 {
            Self::TwoColumn
        } else {
            Self::SingleColumn
        }
    }
}

fn default_branch(global: &GlobalConfig, name: &str) -> String {
    format!("{}/{}", global.branch_prefix, name)
}

fn default_workspace_dir(global: &GlobalConfig, name: &str) -> String {
    format!("{}/{}", shellexpand::tilde(&global.workspace_root), name)
}
```

Modify `src/cli/mod.rs`:

```rust
pub mod completions;
pub mod create_flow;
pub mod info;
```

Keep the rest of the module declarations unchanged.

- [ ] **Step 4: Run tests and verify they pass**

Run:

```bash
cargo test --test create_flow_test
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/cli/mod.rs src/cli/create_flow.rs tests/create_flow_test.rs
git commit -m "feat(create): add create draft model"
```

---

### Task 2: Add Repo Defaults, Template Application, and Draft Validation

**Files:**
- Modify: `src/cli/create_flow.rs`
- Test: `tests/create_flow_test.rs`

- [ ] **Step 1: Add failing tests for repo defaults and validation**

Append to `tests/create_flow_test.rs`:

```rust
use zootree::cli::create_flow::{build_repo_draft_entries, CreateDraftError};
use zootree::config::repo::RepoConfig;
use zootree::config::ConfigManager;
use zootree::runner::MockRunner;
use std::os::unix::process::ExitStatusExt;
use std::process::{ExitStatus, Output};
use tempfile::TempDir;

fn success_stdout(stdout: &str) -> Output {
    Output {
        status: ExitStatus::from_raw(0),
        stdout: stdout.as_bytes().to_vec(),
        stderr: Vec::new(),
    }
}

fn repo_config(path: &str, default_target_branch: Option<&str>) -> RepoConfig {
    RepoConfig {
        path: path.into(),
        default_target_branch: default_target_branch.map(str::to_string),
        copy_files: Vec::new(),
        hooks: Default::default(),
        lazygit: None,
        zellij: None,
    }
}

#[test]
fn repo_draft_prefers_current_repo_branch_over_config_default() {
    let tmp = TempDir::new().unwrap();
    let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
    mgr.ensure_dirs().unwrap();
    mgr.save_repo_config("frontend", &repo_config("/repo/frontend", Some("develop"))).unwrap();
    let runner = MockRunner::new();
    runner.push_response(success_stdout("feature/current\n"));

    let repos = build_repo_draft_entries(
        &mgr,
        &runner,
        Some(("frontend".to_string(), "feature/current".to_string())),
    )
    .unwrap();

    let frontend = repos.iter().find(|repo| repo.name == "frontend").unwrap();
    assert!(frontend.selected);
    assert_eq!(frontend.target_branch, "feature/current");
}

#[test]
fn repo_draft_uses_repo_default_for_non_current_repo() {
    let tmp = TempDir::new().unwrap();
    let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
    mgr.ensure_dirs().unwrap();
    mgr.save_repo_config("backend", &repo_config("/repo/backend", Some("develop"))).unwrap();
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
    mgr.save_repo_config("backend", &repo_config("/repo/backend", None)).unwrap();
    let runner = MockRunner::new();
    runner.push_response(success_stdout("main\n"));

    let repos = build_repo_draft_entries(&mgr, &runner, None).unwrap();

    assert_eq!(repos[0].target_branch, "main");
    assert_eq!(
        runner.take_calls()[0].args,
        vec!["-C", "/repo/backend", "branch", "--show-current"]
    );
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
fn draft_validation_rejects_existing_workspace_name() {
    let global = GlobalConfig::default();
    let draft = CreateDraft::new("auth cleanup", "open-reef", &global);

    let errors = draft.validate(&["open-reef".to_string()], &global);

    assert_eq!(errors, vec![CreateDraftError::WorkspaceNameExists("open-reef".into())]);
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
```

- [ ] **Step 2: Run tests and verify they fail**

Run:

```bash
cargo test --test create_flow_test 2>&1 | tee /tmp/zootree-create-flow-task2.log
```

Expected: FAIL with unresolved `build_repo_draft_entries`, `CreateDraftError`, and `CreateDraft::validate`.

- [ ] **Step 3: Implement repo draft defaults and validation**

In `src/cli/create_flow.rs`, add imports:

```rust
use crate::config::ConfigManager;
use crate::core::git::GitOps;
use crate::runner::CommandRunner;
```

Add the error enum:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreateDraftError {
    TitleRequired,
    WorkspaceNameExists(String),
    RepoRequired,
    TargetBranchRequired(String),
    DefaultAgentMissing,
}
```

Add validation to `impl CreateDraft`:

```rust
    pub fn selected_repos(&self) -> Vec<&RepoDraftEntry> {
        self.repos.iter().filter(|repo| repo.selected).collect()
    }

    pub fn validate(&self, existing_workspaces: &[String], global: &GlobalConfig) -> Vec<CreateDraftError> {
        let mut errors = Vec::new();
        if self.title.trim().is_empty() {
            errors.push(CreateDraftError::TitleRequired);
        }
        if existing_workspaces.iter().any(|name| name == &self.name) {
            errors.push(CreateDraftError::WorkspaceNameExists(self.name.clone()));
        }
        let selected = self.selected_repos();
        if selected.is_empty() {
            errors.push(CreateDraftError::RepoRequired);
        }
        for repo in selected {
            if repo.target_branch.trim().is_empty() {
                errors.push(CreateDraftError::TargetBranchRequired(repo.name.clone()));
            }
        }
        if matches!(self.after_create, AfterCreateMode::StartAndRunAgent { run_agent: None })
            && global.agent_cli.is_none()
        {
            errors.push(CreateDraftError::DefaultAgentMissing);
        }
        errors
    }
```

Add repo-default helper:

```rust
pub fn build_repo_draft_entries<R: CommandRunner>(
    config_mgr: &ConfigManager,
    runner: &R,
    current_repo: Option<(String, String)>,
) -> anyhow::Result<Vec<RepoDraftEntry>> {
    let git = GitOps::new(runner);
    let mut repos = Vec::new();
    for name in config_mgr.list_repos()? {
        let config = config_mgr.load_repo_config(&name)?;
        let expanded_path = shellexpand::tilde(&config.path).into_owned();
        let is_current = current_repo
            .as_ref()
            .map(|(current_name, _)| current_name == &name)
            .unwrap_or(false);
        let target_branch = if let Some((_, branch)) = current_repo
            .as_ref()
            .filter(|(current_name, _)| current_name == &name)
        {
            branch.clone()
        } else if let Some(default) = config.default_target_branch {
            default
        } else {
            git.current_branch(&expanded_path).unwrap_or_else(|_| "main".into())
        };
        repos.push(RepoDraftEntry::new(name, target_branch, is_current));
    }
    Ok(repos)
}
```

- [ ] **Step 4: Run focused tests**

Run:

```bash
cargo test --test create_flow_test
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/cli/create_flow.rs tests/create_flow_test.rs
git commit -m "feat(create): derive repo defaults for create draft"
```

---

### Task 3: Convert Draft to Workspace and Preserve Existing Non-Interactive Semantics

**Files:**
- Modify: `src/cli/create_flow.rs`
- Modify: `src/cli/workspace.rs`
- Test: `tests/create_flow_test.rs`

- [ ] **Step 1: Add failing tests for output conversion and non-interactive routing**

Append to `tests/create_flow_test.rs`:

```rust
use zootree::cli::create_flow::{
    create_args_need_wizard, workspace_from_draft, CreateWizardOutput,
};
use zootree::cli::workspace::CreateArgs;

fn create_args_with(title: Option<&str>, repos: Option<&str>, template: Option<&str>) -> CreateArgs {
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
    assert!(create_args_need_wizard(&create_args_with(None, Some("frontend"), None)));
    assert!(create_args_need_wizard(&create_args_with(Some("auth cleanup"), None, None)));
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

    let workspace = workspace_from_draft(&output.draft, "2026-06-23T10:00:00+08:00", Some("codex".into()));

    assert_eq!(workspace.title, "auth cleanup");
    assert_eq!(workspace.name, "open-reef");
    assert_eq!(workspace.description, "clean up auth flow");
    assert_eq!(workspace.branch, "zt/open-reef");
    assert_eq!(workspace.workspace_dir, "/tmp/zootree-workspaces/open-reef");
    assert_eq!(workspace.agent_cli.as_deref(), Some("codex"));
    assert_eq!(workspace.zellij.session_mode.as_deref(), Some("standalone"));
    assert_eq!(workspace.repos.len(), 1);
    assert_eq!(workspace.repos[0].name, "frontend");
    assert_eq!(workspace.repos[0].target_branch.as_deref(), Some("main"));
    assert_eq!(workspace.events[0].action, "created");
}
```

- [ ] **Step 2: Run tests and verify they fail**

Run:

```bash
cargo test --test create_flow_test 2>&1 | tee /tmp/zootree-create-flow-task3.log
```

Expected: FAIL with unresolved `create_args_need_wizard`, `workspace_from_draft`, and `CreateWizardOutput`.

- [ ] **Step 3: Implement conversion helpers**

In `src/cli/create_flow.rs`, add imports:

```rust
use crate::cli::workspace::CreateArgs;
use crate::config::global::ZellijConfig;
use crate::config::workspace::{Event, RepoEntry, WorkspaceConfig};
```

Add:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateWizardOutput {
    pub draft: CreateDraft,
}

pub fn create_args_need_wizard(args: &CreateArgs) -> bool {
    args.title.is_none() || (args.repos.is_none() && args.template.is_none())
}

pub fn workspace_from_draft(
    draft: &CreateDraft,
    created_at: impl Into<String>,
    agent_cli: Option<String>,
) -> WorkspaceConfig {
    let created_at = created_at.into();
    WorkspaceConfig {
        title: draft.title.clone(),
        name: draft.name.clone(),
        description: draft.description.clone(),
        branch: draft.branch.clone(),
        workspace_dir: draft.workspace_dir.clone(),
        created_at: created_at.clone(),
        agent_cli,
        zellij: ZellijConfig {
            session_mode: Some("standalone".into()),
            ..Default::default()
        },
        repos: draft
            .repos
            .iter()
            .filter(|repo| repo.selected)
            .map(|repo| RepoEntry {
                name: repo.name.clone(),
                target_branch: Some(repo.target_branch.clone()),
            })
            .collect(),
        events: vec![Event {
            action: "created".into(),
            timestamp: created_at,
            detail: None,
        }],
    }
}
```

- [ ] **Step 4: Run tests**

Run:

```bash
cargo test --test create_flow_test
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/cli/create_flow.rs tests/create_flow_test.rs
git commit -m "feat(create): convert create draft to workspace"
```

---

### Task 4: Add CreateWizardApp State Machine Without Rendering Dependence

**Files:**
- Create: `src/tui_app/create_wizard.rs`
- Modify: `src/tui_app/mod.rs`
- Test: `tests/create_wizard_test.rs`

- [ ] **Step 1: Write failing state-machine tests**

Create `tests/create_wizard_test.rs`:

```rust
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use zootree::cli::create_flow::{AfterCreateMode, CreateDraft, RepoDraftEntry};
use zootree::config::global::GlobalConfig;
use zootree::tui_app::create_wizard::{CreateStep, CreateWizardApp, CreateWizardOutcome};
use zootree::tui_app::{App, Event};

fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn key_mod(code: KeyCode, modifiers: KeyModifiers) -> Event {
    Event::Key(KeyEvent::new(code, modifiers))
}

fn draft() -> CreateDraft {
    let global = GlobalConfig::default();
    let mut draft = CreateDraft::new("auth cleanup", "open-reef", &global);
    draft.repos = vec![RepoDraftEntry::new("frontend", "main", true)];
    draft
}

#[test]
fn enter_advances_through_valid_steps() {
    let global = GlobalConfig::default();
    let mut app = CreateWizardApp::new(draft(), global, Vec::new());

    assert_eq!(app.step(), CreateStep::Info);
    app.on_event(key(KeyCode::Enter)).unwrap();
    assert_eq!(app.step(), CreateStep::Repos);
    app.on_event(key(KeyCode::Enter)).unwrap();
    assert_eq!(app.step(), CreateStep::Branches);
}

#[test]
fn esc_goes_back_and_first_step_cancels() {
    let global = GlobalConfig::default();
    let mut app = CreateWizardApp::new(draft(), global, Vec::new());

    app.on_event(key(KeyCode::Enter)).unwrap();
    assert_eq!(app.step(), CreateStep::Repos);
    app.on_event(key(KeyCode::Esc)).unwrap();
    assert_eq!(app.step(), CreateStep::Info);
    app.on_event(key(KeyCode::Esc)).unwrap();

    assert!(app.should_quit());
    assert_eq!(app.outcome(), Some(CreateWizardOutcome::Cancelled));
}

#[test]
fn ctrl_c_interrupts_from_any_step() {
    let global = GlobalConfig::default();
    let mut app = CreateWizardApp::new(draft(), global, Vec::new());

    app.on_event(key_mod(KeyCode::Char('c'), KeyModifiers::CONTROL)).unwrap();

    assert!(app.should_quit());
    assert_eq!(app.outcome(), Some(CreateWizardOutcome::Cancelled));
}

#[test]
fn invalid_step_stays_put_and_exposes_errors() {
    let global = GlobalConfig::default();
    let mut invalid = CreateDraft::new("", "open-reef", &global);
    invalid.repos = vec![RepoDraftEntry::new("frontend", "main", true)];
    let mut app = CreateWizardApp::new(invalid, global, Vec::new());

    app.on_event(key(KeyCode::Enter)).unwrap();

    assert_eq!(app.step(), CreateStep::Info);
    assert!(!app.errors().is_empty());
}

#[test]
fn review_enter_submits_output() {
    let global = GlobalConfig::default();
    let mut app = CreateWizardApp::new(draft(), global, Vec::new());

    app.set_step(CreateStep::Review);
    app.on_event(key(KeyCode::Enter)).unwrap();

    assert!(app.should_quit());
    match app.outcome() {
        Some(CreateWizardOutcome::Submitted(output)) => {
            assert_eq!(output.draft.title, "auth cleanup");
        }
        other => panic!("expected submitted output, got {:?}", other),
    }
}

#[test]
fn space_toggles_repo_on_repos_step() {
    let global = GlobalConfig::default();
    let mut app = CreateWizardApp::new(draft(), global, Vec::new());
    app.set_step(CreateStep::Repos);

    app.on_event(key(KeyCode::Char(' '))).unwrap();

    assert!(!app.draft().repo("frontend").unwrap().selected);
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

    assert_eq!(
        app.draft().after_create,
        AfterCreateMode::StartAndRunAgent { run_agent: None }
    );
}
```

- [ ] **Step 2: Run tests and verify they fail**

Run:

```bash
cargo test --test create_wizard_test 2>&1 | tee /tmp/zootree-create-wizard-task4.log
```

Expected: FAIL with unresolved module `zootree::tui_app::create_wizard`.

- [ ] **Step 3: Implement minimal state machine**

Create `src/tui_app/create_wizard.rs`:

```rust
use std::cell::RefCell;
use std::rc::Rc;

use crossterm::event::{KeyCode, KeyEventKind, KeyModifiers};

use crate::cli::create_flow::{
    AfterCreateMode, CreateDraft, CreateDraftError, CreateWizardOutput,
};
use crate::config::global::GlobalConfig;
use crate::tui_app::{run_app, App, CancelledByUser, Event};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateStep {
    Info,
    Repos,
    Branches,
    AfterCreate,
    Review,
}

impl CreateStep {
    fn next(self) -> Self {
        match self {
            Self::Info => Self::Repos,
            Self::Repos => Self::Branches,
            Self::Branches => Self::AfterCreate,
            Self::AfterCreate => Self::Review,
            Self::Review => Self::Review,
        }
    }

    fn prev(self) -> Self {
        match self {
            Self::Info => Self::Info,
            Self::Repos => Self::Info,
            Self::Branches => Self::Repos,
            Self::AfterCreate => Self::Branches,
            Self::Review => Self::AfterCreate,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreateWizardOutcome {
    Submitted(CreateWizardOutput),
    Cancelled,
}

pub struct CreateWizardApp {
    draft: CreateDraft,
    global: GlobalConfig,
    existing_workspaces: Vec<String>,
    step: CreateStep,
    repo_cursor: usize,
    errors: Vec<CreateDraftError>,
    outcome: Rc<RefCell<Option<CreateWizardOutcome>>>,
}

impl CreateWizardApp {
    pub fn new(draft: CreateDraft, global: GlobalConfig, existing_workspaces: Vec<String>) -> Self {
        Self::with_outcome_handle(
            draft,
            global,
            existing_workspaces,
            Rc::new(RefCell::new(None)),
        )
    }

    pub fn with_outcome_handle(
        draft: CreateDraft,
        global: GlobalConfig,
        existing_workspaces: Vec<String>,
        outcome: Rc<RefCell<Option<CreateWizardOutcome>>>,
    ) -> Self {
        Self {
            draft,
            global,
            existing_workspaces,
            step: CreateStep::Info,
            repo_cursor: 0,
            errors: Vec::new(),
            outcome,
        }
    }

    pub fn step(&self) -> CreateStep {
        self.step
    }

    pub fn set_step(&mut self, step: CreateStep) {
        self.step = step;
        self.errors.clear();
    }

    pub fn draft(&self) -> &CreateDraft {
        &self.draft
    }

    pub fn errors(&self) -> &[CreateDraftError] {
        &self.errors
    }

    pub fn outcome(&self) -> Option<CreateWizardOutcome> {
        self.outcome.borrow().clone()
    }

    fn validate_current_step(&mut self) -> bool {
        self.errors = self.draft.validate(&self.existing_workspaces, &self.global);
        match self.step {
            CreateStep::Info => !self.errors.iter().any(|err| {
                matches!(err, CreateDraftError::TitleRequired | CreateDraftError::WorkspaceNameExists(_))
            }),
            CreateStep::Repos => !self.errors.iter().any(|err| matches!(err, CreateDraftError::RepoRequired)),
            CreateStep::Branches => !self.errors.iter().any(|err| matches!(err, CreateDraftError::TargetBranchRequired(_))),
            CreateStep::AfterCreate => !self.errors.iter().any(|err| matches!(err, CreateDraftError::DefaultAgentMissing)),
            CreateStep::Review => self.errors.is_empty(),
        }
    }

    fn submit_or_advance(&mut self) {
        if !self.validate_current_step() {
            return;
        }
        if self.step == CreateStep::Review {
            *self.outcome.borrow_mut() = Some(CreateWizardOutcome::Submitted(CreateWizardOutput {
                draft: self.draft.clone(),
            }));
        } else {
            self.step = self.step.next();
            self.errors.clear();
        }
    }

    fn cancel_or_back(&mut self) {
        if self.step == CreateStep::Info {
            *self.outcome.borrow_mut() = Some(CreateWizardOutcome::Cancelled);
        } else {
            self.step = self.step.prev();
            self.errors.clear();
        }
    }

    fn toggle_current_repo(&mut self) {
        if self.draft.repos.is_empty() {
            return;
        }
        let idx = self.repo_cursor.min(self.draft.repos.len() - 1);
        self.draft.repos[idx].selected = !self.draft.repos[idx].selected;
    }

    fn move_repo_cursor(&mut self, delta: isize) {
        if self.draft.repos.is_empty() {
            self.repo_cursor = 0;
            return;
        }
        let len = self.draft.repos.len() as isize;
        self.repo_cursor = (self.repo_cursor as isize + delta).rem_euclid(len) as usize;
    }

    fn cycle_after_create(&mut self, delta: isize) {
        let idx = match self.draft.after_create {
            AfterCreateMode::CreateOnly => 0,
            AfterCreateMode::Start => 1,
            AfterCreateMode::StartAndRunAgent { .. } => 2,
        };
        let next = (idx + delta).rem_euclid(3);
        self.draft.after_create = match next {
            0 => AfterCreateMode::CreateOnly,
            1 => AfterCreateMode::Start,
            _ => AfterCreateMode::StartAndRunAgent { run_agent: None },
        };
    }
}

impl App for CreateWizardApp {
    fn on_event(&mut self, event: Event) -> anyhow::Result<()> {
        let Event::Key(key) = event else {
            return Ok(());
        };
        if key.kind != KeyEventKind::Press {
            return Ok(());
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char('c') if ctrl => {
                *self.outcome.borrow_mut() = Some(CreateWizardOutcome::Cancelled);
            }
            KeyCode::Enter => self.submit_or_advance(),
            KeyCode::Esc => self.cancel_or_back(),
            KeyCode::Char(' ') if self.step == CreateStep::Repos => self.toggle_current_repo(),
            KeyCode::Down | KeyCode::Char('j') if self.step == CreateStep::Repos => self.move_repo_cursor(1),
            KeyCode::Up | KeyCode::Char('k') if self.step == CreateStep::Repos => self.move_repo_cursor(-1),
            KeyCode::Char('n') if ctrl && self.step == CreateStep::Repos => self.move_repo_cursor(1),
            KeyCode::Char('p') if ctrl && self.step == CreateStep::Repos => self.move_repo_cursor(-1),
            KeyCode::Right if self.step == CreateStep::AfterCreate => self.cycle_after_create(1),
            KeyCode::Left if self.step == CreateStep::AfterCreate => self.cycle_after_create(-1),
            _ => {}
        }
        Ok(())
    }

    fn render(&mut self, frame: &mut ratatui::Frame) {
        let _ = frame;
    }

    fn should_quit(&self) -> bool {
        self.outcome.borrow().is_some()
    }
}

pub fn run_create_wizard(
    draft: CreateDraft,
    global: GlobalConfig,
    existing_workspaces: Vec<String>,
) -> anyhow::Result<CreateWizardOutput> {
    let outcome = Rc::new(RefCell::new(None));
    let app = CreateWizardApp::with_outcome_handle(
        draft,
        global,
        existing_workspaces,
        outcome.clone(),
    );
    run_app(app)?;
    match outcome.borrow().clone() {
        Some(CreateWizardOutcome::Submitted(output)) => Ok(output),
        Some(CreateWizardOutcome::Cancelled) | None => Err(CancelledByUser.into()),
    }
}
```

Modify `src/tui_app/mod.rs` near the top:

```rust
pub mod create_wizard;
pub mod info;
pub mod prompt;
```

- [ ] **Step 4: Run focused tests**

Run:

```bash
cargo test --test create_wizard_test
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/tui_app/mod.rs src/tui_app/create_wizard.rs tests/create_wizard_test.rs
git commit -m "feat(create): add create wizard state machine"
```

---

### Task 5: Render the Full-Screen Wizard Responsively

**Files:**
- Modify: `src/tui_app/create_wizard.rs`
- Test: `tests/create_wizard_test.rs`

- [ ] **Step 1: Add failing tests for layout and render smoke behavior**

Append to `tests/create_wizard_test.rs`:

```rust
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use zootree::cli::create_flow::CreateWizardLayout;

fn render_app_to_string(app: &mut CreateWizardApp, width: u16, height: u16) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| app.render(frame)).unwrap();
    format!("{:?}", terminal.backend().buffer())
}

#[test]
fn render_wide_layout_shows_draft_summary() {
    let global = GlobalConfig::default();
    let mut app = CreateWizardApp::new(draft(), global, Vec::new());

    let rendered = render_app_to_string(&mut app, 120, 30);

    assert!(rendered.contains("Workspace info"));
    assert!(rendered.contains("Draft"));
    assert!(rendered.contains("auth cleanup"));
}

#[test]
fn render_narrow_layout_shows_compact_summary() {
    let global = GlobalConfig::default();
    let mut app = CreateWizardApp::new(draft(), global, Vec::new());

    let rendered = render_app_to_string(&mut app, 80, 24);

    assert!(rendered.contains("Workspace info"));
    assert!(rendered.contains("repos: 1"));
}

#[test]
fn render_too_narrow_shows_resize_message() {
    let global = GlobalConfig::default();
    let mut app = CreateWizardApp::new(draft(), global, Vec::new());

    let rendered = render_app_to_string(&mut app, 40, 20);

    assert!(rendered.contains("resize to at least 50 columns"));
}

#[test]
fn app_reports_layout_for_width() {
    assert_eq!(CreateWizardLayout::for_width(120), CreateWizardLayout::TwoColumn);
    assert_eq!(CreateWizardLayout::for_width(80), CreateWizardLayout::SingleColumn);
    assert_eq!(CreateWizardLayout::for_width(40), CreateWizardLayout::TooNarrow);
}
```

- [ ] **Step 2: Run tests and verify they fail**

Run:

```bash
cargo test --test create_wizard_test 2>&1 | tee /tmp/zootree-create-wizard-task5.log
```

Expected: FAIL because `render()` is blank.

- [ ] **Step 3: Implement render helpers**

In `src/tui_app/create_wizard.rs`, add imports:

```rust
use crate::cli::create_flow::CreateWizardLayout;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
```

Replace the blank `render` implementation with:

```rust
    fn render(&mut self, frame: &mut ratatui::Frame) {
        let area = frame.area();
        match CreateWizardLayout::for_width(area.width) {
            CreateWizardLayout::TooNarrow => render_too_narrow(frame, area),
            CreateWizardLayout::SingleColumn => self.render_single_column(frame, area),
            CreateWizardLayout::TwoColumn => self.render_two_column(frame, area),
        }
    }
```

Add helper methods to `impl CreateWizardApp`:

```rust
    fn render_two_column(&self, frame: &mut ratatui::Frame, area: Rect) {
        let vertical = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(area);
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(65), Constraint::Percentage(35)])
            .split(vertical[0]);
        frame.render_widget(self.step_paragraph(), columns[0]);
        frame.render_widget(self.summary_paragraph(false), columns[1]);
        frame.render_widget(self.help_paragraph(), vertical[1]);
    }

    fn render_single_column(&self, frame: &mut ratatui::Frame, area: Rect) {
        let vertical = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(1), Constraint::Length(1)])
            .split(area);
        frame.render_widget(self.compact_summary(), vertical[0]);
        frame.render_widget(self.step_paragraph(), vertical[1]);
        frame.render_widget(self.help_paragraph(), vertical[2]);
    }

    fn step_title(&self) -> &'static str {
        match self.step {
            CreateStep::Info => "Workspace info",
            CreateStep::Repos => "Repos",
            CreateStep::Branches => "Target branches",
            CreateStep::AfterCreate => "After create",
            CreateStep::Review => "Review",
        }
    }

    fn step_paragraph(&self) -> Paragraph<'_> {
        let mut lines = vec![
            Line::from(Span::styled(self.step_title(), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))),
            Line::from(""),
        ];
        match self.step {
            CreateStep::Info => {
                lines.push(Line::from(format!("title: {}", self.draft.title)));
                lines.push(Line::from(format!("description: {}", first_line_or_empty(&self.draft.description))));
                lines.push(Line::from(format!("name: {}", self.draft.name)));
                lines.push(Line::from(format!("branch: {}", self.draft.branch)));
            }
            CreateStep::Repos => {
                for (idx, repo) in self.draft.repos.iter().enumerate() {
                    let cursor = if idx == self.repo_cursor { ">" } else { " " };
                    let check = if repo.selected { "[x]" } else { "[ ]" };
                    lines.push(Line::from(format!("{} {} {}", cursor, check, repo.name)));
                }
            }
            CreateStep::Branches => {
                for repo in self.draft.repos.iter().filter(|repo| repo.selected) {
                    lines.push(Line::from(format!("{}: {}", repo.name, repo.target_branch)));
                }
            }
            CreateStep::AfterCreate => {
                lines.push(Line::from(format!("mode: {}", after_create_label(&self.draft.after_create))));
            }
            CreateStep::Review => {
                lines.extend(self.summary_lines());
            }
        }
        if !self.errors.is_empty() {
            lines.push(Line::from(""));
            for err in &self.errors {
                lines.push(Line::from(Span::styled(format!("error: {:?}", err), Style::default().fg(Color::Red))));
            }
        }
        Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title(self.step_title()))
            .wrap(Wrap { trim: false })
    }

    fn summary_lines(&self) -> Vec<Line<'_>> {
        vec![
            Line::from(format!("title: {}", self.draft.title)),
            Line::from(format!("name: {}", self.draft.name)),
            Line::from(format!("branch: {}", self.draft.branch)),
            Line::from(format!("workspace: {}", self.draft.workspace_dir)),
            Line::from(format!("repos: {}", self.draft.selected_repos().len())),
            Line::from(format!("after create: {}", after_create_label(&self.draft.after_create))),
        ]
    }

    fn summary_paragraph(&self, compact: bool) -> Paragraph<'_> {
        let lines = if compact {
            vec![Line::from(format!(
                "{} · repos: {} · {}",
                self.draft.name,
                self.draft.selected_repos().len(),
                after_create_label(&self.draft.after_create)
            ))]
        } else {
            self.summary_lines()
        };
        Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title("Draft"))
            .wrap(Wrap { trim: false })
    }

    fn compact_summary(&self) -> Paragraph<'_> {
        self.summary_paragraph(true)
    }

    fn help_paragraph(&self) -> Paragraph<'_> {
        Paragraph::new("tab fields · enter next/confirm · esc back/cancel · ctrl+c quit · j/k move")
            .style(Style::default().fg(Color::DarkGray))
    }
```

Add free functions:

```rust
fn render_too_narrow(frame: &mut ratatui::Frame, area: Rect) {
    frame.render_widget(
        Paragraph::new("terminal too narrow; resize to at least 50 columns")
            .block(Block::default().borders(Borders::ALL).title("zootree create")),
        area,
    );
}

fn first_line_or_empty(s: &str) -> &str {
    s.lines().next().unwrap_or("")
}

fn after_create_label(mode: &AfterCreateMode) -> &'static str {
    match mode {
        AfterCreateMode::CreateOnly => "create only",
        AfterCreateMode::Start => "create and start",
        AfterCreateMode::StartAndRunAgent { .. } => "create, start and run agent",
    }
}
```

- [ ] **Step 4: Run focused tests**

Run:

```bash
cargo test --test create_wizard_test
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/tui_app/create_wizard.rs tests/create_wizard_test.rs
git commit -m "feat(create): render create wizard layout"
```

---

### Task 6: Integrate Wizard Into `handle_create`

**Files:**
- Modify: `src/cli/workspace.rs`
- Modify: `src/cli/create_flow.rs`
- Test: `tests/create_flow_test.rs`

- [ ] **Step 1: Add failing tests for wizard initial draft from args**

Append to `tests/create_flow_test.rs`:

```rust
use zootree::cli::create_flow::{draft_from_args, resolve_agent_cli_for_draft};

#[test]
fn draft_from_args_uses_cli_values_as_initial_values() {
    let tmp = TempDir::new().unwrap();
    let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
    mgr.ensure_dirs().unwrap();
    let global = GlobalConfig {
        workspace_root: "/tmp/zootree-workspaces".into(),
        branch_prefix: "zt".into(),
        ..Default::default()
    };
    let runner = MockRunner::new();
    let args = CreateArgs {
        title: Some("auth cleanup".into()),
        name: Some("manual-name".into()),
        description: Some("desc".into()),
        repos: None,
        branch: Some("feature/manual".into()),
        template: None,
        start: true,
        run_agent: None,
    };

    let draft = draft_from_args(&args, &mgr, &runner, &global, None, &["open-reef".into()]).unwrap();

    assert_eq!(draft.title, "auth cleanup");
    assert_eq!(draft.description, "desc");
    assert_eq!(draft.name, "manual-name");
    assert_eq!(draft.branch, "feature/manual");
    assert!(draft.branch_was_edited);
    assert_eq!(draft.after_create, AfterCreateMode::Start);
}

#[test]
fn draft_from_args_maps_run_agent_to_after_create_mode() {
    let tmp = TempDir::new().unwrap();
    let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
    mgr.ensure_dirs().unwrap();
    let global = GlobalConfig::default();
    let runner = MockRunner::new();
    let args = CreateArgs {
        title: Some("auth cleanup".into()),
        name: None,
        description: None,
        repos: None,
        branch: None,
        template: None,
        start: false,
        run_agent: Some(Some("codex".into())),
    };

    let draft = draft_from_args(&args, &mgr, &runner, &global, None, &[]).unwrap();

    assert_eq!(
        draft.after_create,
        AfterCreateMode::StartAndRunAgent {
            run_agent: Some("codex".into())
        }
    );
}

#[test]
fn draft_from_args_rejects_unknown_repo_argument() {
    let tmp = TempDir::new().unwrap();
    let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
    mgr.ensure_dirs().unwrap();
    let global = GlobalConfig::default();
    let runner = MockRunner::new();
    let args = CreateArgs {
        title: Some("auth cleanup".into()),
        name: None,
        description: None,
        repos: Some("missing".into()),
        branch: None,
        template: None,
        start: false,
        run_agent: None,
    };

    let err = draft_from_args(&args, &mgr, &runner, &global, None, &[]).unwrap_err();

    assert!(err.to_string().contains("repo 'missing' is not registered"));
}

#[test]
fn resolve_agent_cli_for_draft_uses_global_default_for_empty_run_agent() {
    let global = GlobalConfig {
        agent_cli: Some("codex -- $prompt".into()),
        ..Default::default()
    };
    let mode = AfterCreateMode::StartAndRunAgent { run_agent: None };

    assert_eq!(
        resolve_agent_cli_for_draft(&mode, &global).unwrap(),
        Some("codex -- $prompt".into())
    );
}
```

- [ ] **Step 2: Run tests and verify they fail**

Run:

```bash
cargo test --test create_flow_test 2>&1 | tee /tmp/zootree-create-flow-task6.log
```

Expected: FAIL with unresolved `draft_from_args` and `resolve_agent_cli_for_draft`.

- [ ] **Step 3: Implement draft-from-args and agent resolution**

In `src/cli/create_flow.rs`, add:

```rust
use crate::core::name_gen::NameGenerator;
```

Implement:

```rust
pub fn draft_from_args<R: CommandRunner>(
    args: &CreateArgs,
    config_mgr: &ConfigManager,
    runner: &R,
    global: &GlobalConfig,
    current_repo: Option<(String, String)>,
    existing_workspaces: &[String],
) -> anyhow::Result<CreateDraft> {
    let name = args.name.clone().unwrap_or_else(|| {
        NameGenerator::new().generate_avoiding(existing_workspaces)
    });
    let mut draft = CreateDraft::new(args.title.clone().unwrap_or_default(), name, global);
    draft.description = args.description.clone().unwrap_or_default();
    if let Some(branch) = &args.branch {
        draft.set_branch(branch.clone());
    }
    draft.repos = build_repo_draft_entries(config_mgr, runner, current_repo)?;
    if let Some(repos_str) = &args.repos {
        let selected: std::collections::HashMap<String, Option<String>> =
            crate::cli::workspace::parse_repos_arg(repos_str).into_iter().collect();
        for requested in selected.keys() {
            if !draft.repos.iter().any(|repo| &repo.name == requested) {
                anyhow::bail!("repo '{}' is not registered", requested);
            }
        }
        for repo in &mut draft.repos {
            if let Some(branch) = selected.get(&repo.name) {
                repo.selected = true;
                if let Some(branch) = branch {
                    repo.target_branch = branch.clone();
                }
            } else {
                repo.selected = false;
            }
        }
    }
    if let Some(template_name) = &args.template {
        let template = config_mgr.load_template(template_name)?;
        for requested in &template.repos {
            if !draft.repos.iter().any(|repo| &repo.name == requested) {
                anyhow::bail!(
                    "template '{}' references unregistered repo '{}'",
                    template_name,
                    requested
                );
            }
        }
        draft.apply_template_repos(&template.repos);
    }
    draft.after_create = match &args.run_agent {
        Some(Some(value)) if !value.is_empty() => AfterCreateMode::StartAndRunAgent {
            run_agent: Some(value.clone()),
        },
        Some(_) => AfterCreateMode::StartAndRunAgent { run_agent: None },
        None if args.start => AfterCreateMode::Start,
        None => AfterCreateMode::CreateOnly,
    };
    Ok(draft)
}

pub fn resolve_agent_cli_for_draft(
    mode: &AfterCreateMode,
    global: &GlobalConfig,
) -> anyhow::Result<Option<String>> {
    match mode {
        AfterCreateMode::CreateOnly | AfterCreateMode::Start => Ok(None),
        AfterCreateMode::StartAndRunAgent { run_agent: Some(value) } => Ok(Some(value.clone())),
        AfterCreateMode::StartAndRunAgent { run_agent: None } => Ok(Some(
            global.agent_cli.clone().ok_or_else(|| {
                anyhow::anyhow!(
                    "--run-agent requires agent_cli in global config (~/.config/zootree/config.toml)"
                )
            })?,
        )),
    }
}
```

- [ ] **Step 4: Refactor `handle_create` to use helpers and wizard**

In `src/cli/workspace.rs`, add imports:

```rust
use crate::cli::create_flow::{
    create_args_need_wizard, draft_from_args, resolve_agent_cli_for_draft, workspace_from_draft,
    AfterCreateMode, CreateWizardOutput,
};
use crate::tui_app::create_wizard::run_create_wizard;
```

Refactor `handle_create` so it follows this shape:

```rust
pub fn handle_create(args: &CreateArgs) -> Result<()> {
    let config_mgr = ConfigManager::new()?;
    config_mgr.ensure_dirs()?;
    let global = config_mgr.load_global_config()?;
    let runner = RealRunner;
    let needs_wizard = create_args_need_wizard(args);

    let current_repo = if needs_wizard {
        ensure_current_repo_registered(&config_mgr, &runner, &std::env::current_dir()?)?
    } else {
        None
    };
    let current_repo_tuple = current_repo
        .as_ref()
        .map(|repo| (repo.name.clone(), repo.current_branch.clone()));
    let existing: Vec<String> = config_mgr
        .list_workspaces(None::<&[WorkspaceStatus]>)?
        .iter()
        .map(|w| w.name.clone())
        .collect();

    let mut draft = draft_from_args(
        args,
        &config_mgr,
        &runner,
        &global,
        current_repo_tuple,
        &existing,
    )?;

    let output = if needs_wizard {
        run_create_wizard(draft, global.clone(), existing.clone())?
    } else {
        let errors = draft.validate(&existing, &global);
        if !errors.is_empty() {
            anyhow::bail!("invalid create arguments: {:?}", errors);
        }
        CreateWizardOutput { draft }
    };

    let agent_cli = resolve_agent_cli_for_draft(&output.draft.after_create, &global)?;
    let now = Local::now().to_rfc3339();
    let workspace = workspace_from_draft(&output.draft, now, agent_cli);

    save_created_workspace(&config_mgr, &workspace)?;
    print_created_workspace(&workspace);
    start_after_create_if_needed(&output.draft.after_create, &workspace.name)?;

    Ok(())
}
```

Extract these private helpers in `workspace.rs`:

```rust
fn save_created_workspace(config_mgr: &ConfigManager, workspace: &WorkspaceConfig) -> Result<()> {
    config_mgr.save_workspace(&WorkspaceStatus::Pending, workspace)?;
    let recently = TemplateConfig {
        repos: workspace.repos.iter().map(|r| r.name.clone()).collect(),
        zellij: workspace.zellij.clone(),
    };
    config_mgr.save_template("recently", &recently)?;
    Ok(())
}

fn print_created_workspace(workspace: &WorkspaceConfig) {
    println!("workspace '{}' created (pending)", workspace.name);
    println!("  branch: {}", workspace.branch);
    println!(
        "  repos: {}",
        workspace
            .repos
            .iter()
            .map(|r| format!("{}:{}", r.name, r.target_branch.as_deref().unwrap_or("*")))
            .collect::<Vec<_>>()
            .join(", ")
    );
}

fn start_after_create_if_needed(mode: &AfterCreateMode, name: &str) -> Result<()> {
    if mode.should_start() {
        let run_agent = match mode {
            AfterCreateMode::CreateOnly | AfterCreateMode::Start => None,
            AfterCreateMode::StartAndRunAgent { run_agent } => Some(run_agent.clone()),
        };
        let start_args = StartArgs {
            name: Some(name.to_string()),
            no_zellij: false,
            run_agent,
        };
        handle_start(&start_args)?;
    }
    Ok(())
}
```

Remove the old inline `tui::input_required`, `tui::input_optional`, and `tui::select_multi` create path from `handle_create`. Keep `parse_repos_arg`, `build_repo_entries`, and template helpers if other tests still use them.

- [ ] **Step 5: Run focused tests**

Run:

```bash
cargo test --test create_flow_test
cargo test --test create_wizard_test
cargo test --test config_test build_repo_entries
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/cli/create_flow.rs src/cli/workspace.rs tests/create_flow_test.rs
git commit -m "feat(create): route interactive create through wizard draft"
```

---

### Task 7: Manual Verification and Full Test Sweep

**Files:**
- Modify only if verification exposes compile or behavior issues.

- [ ] **Step 1: Run formatting and full tests**

Run:

```bash
cargo fmt --check
cargo test
```

Expected: PASS.

If `cargo fmt --check` fails, run:

```bash
cargo fmt
git diff --check
```

Then rerun:

```bash
cargo fmt --check
cargo test
```

- [ ] **Step 2: Build the binary**

Run:

```bash
cargo build
```

Expected: PASS.

- [ ] **Step 3: Manual smoke test non-interactive path**

Use an existing registered repo name from:

```bash
zootree repo list
```

Then run:

```bash
target/debug/zootree create --title "wizard smoke noninteractive" --repos <repo-name>
```

Expected:
- Does not open full-screen wizard.
- Prints `workspace '<name>' created (pending)`.
- Prints branch and repo summary.

If this creates a real pending workspace, remove it through the normal project workflow after confirming with the user or use a temporary `ZOOTREE_CONFIG_DIR` only if the app already supports that variable. Do not delete real config files manually.

- [ ] **Step 4: Manual smoke test full-screen path**

Run:

```bash
target/debug/zootree create
```

Expected:
- Opens alternate-screen full-screen wizard.
- `Enter` advances valid steps.
- `Esc` backs up; `Esc` on first step exits with `aborted`.
- Current repo is selected when running inside a registered git repo.
- Width below 100 columns uses single-column layout.
- Width below 50 columns shows resize message.

- [ ] **Step 5: Final commit for verification fixes**

If Task 7 required any code changes:

```bash
git add <changed-files>
git commit -m "fix(create): polish wizard verification issues"
```

If Task 7 required no code changes, do not create an empty commit.
