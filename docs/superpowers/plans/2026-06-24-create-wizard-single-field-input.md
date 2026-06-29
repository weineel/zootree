# Create Wizard Single Field Input Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Refactor `zootree create` fullscreen wizard so each page handles exactly one field/control and all text fields use a `ratatui_textarea::TextArea`-backed editor.

**Architecture:** Replace the current coarse `CreateStep` state machine with a field-level `CreateWizardPage` sequence. Keep create draft and CLI routing in `create_flow`/`workspace` unchanged except where template semi-interactive page construction already requires all repos. Add a focused `WizardTextField` inside `src/tui_app/create_wizard.rs` and reuse current responsive rendering with one active field panel plus draft preview.

**Tech Stack:** Rust 2021, ratatui 0.30, crossterm 0.29, ratatui-textarea 0.9, existing `CreateDraft`, `CreateDraftError`, `CreateWizardLayout`, `CreateWizardApp`, and `tui_app::run_app`.

Design spec: `docs/superpowers/specs/2026-06-24-create-wizard-single-field-input-design.md`.

---

## File Structure

| File | Responsibility |
|---|---|
| `src/tui_app/create_wizard.rs` | Field-level wizard page model, textarea-backed current field editor, page navigation, responsive render, wizard outcome. |
| `tests/create_wizard_test.rs` | Pure state/input/render tests for field pages, textarea behavior, dynamic target branch pages, after-create/run-agent flow. |
| `src/cli/create_flow.rs` | Keep existing draft/default/template semantics; only adjust if page construction requires a missing helper. |
| `tests/create_flow_test.rs` | Keep existing CLI/non-interactive and template helper coverage green; add only if `create_flow` changes. |

Do not change config schemas. Do not modify `src/cli/workspace.rs` unless a compile error proves the public wizard API must change.

---

## Task 1: Add Field-Level Page Model Without Changing Behavior

**Files:**
- Modify: `src/tui_app/create_wizard.rs`
- Test: `tests/create_wizard_test.rs`

- [ ] **Step 1: Write failing tests for field page sequence**

Append these tests to `tests/create_wizard_test.rs`:

```rust
use zootree::tui_app::create_wizard::CreateWizardPage;

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
```

- [ ] **Step 2: Run tests and verify they fail**

Run:

```bash
cargo test --test create_wizard_test
```

Expected: FAIL with unresolved import or missing methods `CreateWizardPage`, `page`, and `page_titles`.

- [ ] **Step 3: Add page model and page builder**

In `src/tui_app/create_wizard.rs`, add the new page enum near `CreateStep`:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreateWizardPage {
    Title,
    Description,
    WorkspaceName,
    WorkspaceBranch,
    Repos,
    TargetBranch { repo_name: String },
    AfterCreate,
    RunAgent,
    Review,
}

impl CreateWizardPage {
    fn title(&self) -> String {
        match self {
            Self::Title => "Workspace: Title".into(),
            Self::Description => "Workspace: Description".into(),
            Self::WorkspaceName => "Workspace: Name".into(),
            Self::WorkspaceBranch => "Workspace: Branch".into(),
            Self::Repos => "Repos".into(),
            Self::TargetBranch { repo_name } => format!("Branches: {repo_name}"),
            Self::AfterCreate => "After create".into(),
            Self::RunAgent => "After create: Run agent".into(),
            Self::Review => "Review".into(),
        }
    }
}
```

Add fields to `CreateWizardApp`:

```rust
pages: Vec<CreateWizardPage>,
page_index: usize,
```

Add helper functions:

```rust
fn build_pages(draft: &CreateDraft) -> Vec<CreateWizardPage> {
    let mut pages = vec![
        CreateWizardPage::Title,
        CreateWizardPage::Description,
        CreateWizardPage::WorkspaceName,
        CreateWizardPage::WorkspaceBranch,
        CreateWizardPage::Repos,
    ];
    pages.extend(
        draft
            .selected_repos()
            .into_iter()
            .map(|repo| CreateWizardPage::TargetBranch {
                repo_name: repo.name.clone(),
            }),
    );
    pages.push(CreateWizardPage::AfterCreate);
    if matches!(draft.after_create, AfterCreateMode::StartAndRunAgent { .. }) {
        pages.push(CreateWizardPage::RunAgent);
    }
    pages.push(CreateWizardPage::Review);
    pages
}

fn clamp_page_index(&mut self) {
    if self.pages.is_empty() {
        self.page_index = 0;
    } else {
        self.page_index = self.page_index.min(self.pages.len() - 1);
    }
}
```

Initialize `pages` and `page_index` in `with_outcome_handle`:

```rust
let pages = Self::build_pages(&draft);
Self {
    pages,
    page_index: 0,
    // keep existing fields unchanged for now
}
```

Add public test helpers:

```rust
pub fn page(&self) -> &CreateWizardPage {
    &self.pages[self.page_index]
}

pub fn page_titles(&self) -> Vec<String> {
    self.pages.iter().map(CreateWizardPage::title).collect()
}
```

Keep current `CreateStep` behavior intact in this task so existing tests still pass.

- [ ] **Step 4: Run focused tests**

Run:

```bash
cargo test --test create_wizard_test
```

Expected: PASS.

- [ ] **Step 5: Run existing wizard tests**

Run:

```bash
cargo test --test create_wizard_test
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/tui_app/create_wizard.rs tests/create_wizard_test.rs
git commit -m "feat(create): add wizard field page model"
```

---

## Task 2: Add TextArea-Backed Wizard Text Field

**Files:**
- Modify: `src/tui_app/create_wizard.rs`
- Test: `tests/create_wizard_test.rs`

- [ ] **Step 1: Write failing textarea behavior tests**

Append these tests to `tests/create_wizard_test.rs`:

```rust
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
    assert!(app.errors().iter().any(|err| matches!(err, CreateDraftError::TitleRequired)));
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
```

- [ ] **Step 2: Run tests and verify they fail**

Run:

```bash
cargo test --test create_wizard_test
```

Expected: FAIL because current wizard text editing still uses hand-written string mutation and does not drive page-level textarea commits.

- [ ] **Step 3: Implement `WizardTextField`**

In `src/tui_app/create_wizard.rs`, add imports:

```rust
use ratatui_textarea::{TextArea, WrapMode};
```

Add these private types:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WizardTextKind {
    SingleLine,
    Multiline,
}

struct WizardTextField {
    textarea: TextArea<'static>,
    kind: WizardTextKind,
}

impl WizardTextField {
    fn new(text: impl Into<String>, kind: WizardTextKind) -> Self {
        let text = text.into();
        let mut textarea = if text.is_empty() {
            TextArea::default()
        } else {
            TextArea::from(text.lines().map(str::to_string).collect::<Vec<_>>())
        };
        textarea.set_cursor_line_style(Style::default());
        textarea.set_wrap_mode(WrapMode::WordOrGlyph);
        Self { textarea, kind }
    }

    fn text(&self) -> String {
        self.textarea.lines().join("\n")
    }

    fn handle_key(&mut self, key: crossterm::event::KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        match key.code {
            KeyCode::Char('u') if ctrl => self.textarea.delete_line_by_head(),
            KeyCode::Enter if alt || shift => self.textarea.insert_newline(),
            _ => {
                let _ = self.textarea.input(key);
            }
        }
    }

    fn handle_paste(&mut self, text: &str) {
        self.textarea.insert_str(text);
    }

    fn paragraph(&self, title: impl Into<String>) -> Paragraph<'static> {
        Paragraph::new(self.textarea.lines().join("\n"))
            .block(Block::default().borders(Borders::ALL).title(title.into()))
            .wrap(Wrap { trim: false })
    }
}
```

Add `text_field: Option<WizardTextField>` to `CreateWizardApp`.

Add:

```rust
fn page_text_kind(page: &CreateWizardPage) -> Option<WizardTextKind> {
    match page {
        CreateWizardPage::Description => Some(WizardTextKind::Multiline),
        CreateWizardPage::Title
        | CreateWizardPage::WorkspaceName
        | CreateWizardPage::WorkspaceBranch
        | CreateWizardPage::TargetBranch { .. }
        | CreateWizardPage::RunAgent => Some(WizardTextKind::SingleLine),
        _ => None,
    }
}
```

Add `enter_page`:

```rust
fn enter_page(&mut self, page_index: usize) {
    self.page_index = page_index.min(self.pages.len().saturating_sub(1));
    self.errors.clear();
    self.text_field = self.current_page_text().map(|(text, kind)| WizardTextField::new(text, kind));
}
```

Add `current_page_text` for Title and Description first:

```rust
fn current_page_text(&self) -> Option<(String, WizardTextKind)> {
    match self.page() {
        CreateWizardPage::Title => Some((self.draft.title.clone(), WizardTextKind::SingleLine)),
        CreateWizardPage::Description => Some((self.draft.description.clone(), WizardTextKind::Multiline)),
        _ => None,
    }
}
```

Initialize text field in `with_outcome_handle` after constructing `Self`:

```rust
let mut app = Self { /* fields */ };
app.enter_page(0);
app
```

Add single-line validation helper:

```rust
fn clean_single_line(field: &'static str, text: String) -> Result<String, CreateDraftError> {
    let cleaned = text.trim().to_string();
    if cleaned.is_empty() {
        return Err(match field {
            "title" => CreateDraftError::TitleRequired,
            _ => CreateDraftError::TargetBranchRequired(field.into()),
        });
    }
    if cleaned.contains('\n') {
        return Err(match field {
            "title" => CreateDraftError::TitleRequired,
            _ => CreateDraftError::TargetBranchRequired(field.into()),
        });
    }
    Ok(cleaned)
}
```

Add `commit_current_page` handling Title and Description:

```rust
fn commit_current_page(&mut self) -> bool {
    match self.page() {
        CreateWizardPage::Title => {
            let text = self.text_field.as_ref().map(WizardTextField::text).unwrap_or_default();
            match Self::clean_single_line("title", text) {
                Ok(title) => self.draft.title = title,
                Err(err) => {
                    self.errors = vec![err];
                    return false;
                }
            }
        }
        CreateWizardPage::Description => {
            self.draft.description = self.text_field.as_ref().map(WizardTextField::text).unwrap_or_default();
        }
        _ => return self.validate_current_step(),
    }
    true
}
```

Update `submit_or_advance` to prefer page-level commit:

```rust
fn submit_or_advance(&mut self) {
    if !self.commit_current_page() {
        return;
    }
    if matches!(self.page(), CreateWizardPage::Review) {
        // existing submit
    } else {
        self.enter_page(self.page_index + 1);
    }
}
```

Route text keys:

```rust
fn handle_text_key(&mut self, key: crossterm::event::KeyEvent) -> bool {
    if let Some(field) = &mut self.text_field {
        field.handle_key(key);
        self.errors.clear();
        return true;
    }
    false
}
```

In `on_event`, before movement keys but after Ctrl+C/Esc/Enter:

```rust
KeyCode::Enter => self.submit_or_advance(),
KeyCode::Esc => self.cancel_or_back(),
_ if self.handle_text_key(key) => {}
```

For `Event::Paste`, call `text_field.handle_paste` when present.

- [ ] **Step 4: Run focused tests**

Run:

```bash
cargo test --test create_wizard_test
```

Expected: PASS.

- [ ] **Step 5: Run full wizard tests**

Run:

```bash
cargo test --test create_wizard_test
```

Expected: PASS. Replace old `CreateStep` assertions with page-level assertions in the same test file when the new field-page assertions cover the same behavior.

- [ ] **Step 6: Commit**

```bash
git add src/tui_app/create_wizard.rs tests/create_wizard_test.rs
git commit -m "feat(create): add wizard textarea field"
```

---

## Task 3: Migrate Workspace Text Pages

**Files:**
- Modify: `src/tui_app/create_wizard.rs`
- Test: `tests/create_wizard_test.rs`

- [ ] **Step 1: Write failing tests for name and branch pages**

Append these tests:

```rust
#[test]
fn workspace_name_page_updates_workspace_dir_and_auto_branch() {
    let global = GlobalConfig {
        workspace_root: "/tmp/zootree-workspaces".into(),
        branch_prefix: "zt".into(),
        ..Default::default()
    };
    let mut app = CreateWizardApp::new(draft(), global, Vec::new());

    app.jump_to_page(CreateWizardPage::WorkspaceName);
    app.clear_text_field_for_test();
    app.on_event(paste("wide-tide")).unwrap();
    app.on_event(key(KeyCode::Enter)).unwrap();

    assert_eq!(app.draft().name, "wide-tide");
    assert_eq!(app.draft().branch, "zt/wide-tide");
    assert_eq!(app.draft().workspace_dir, "/tmp/zootree-workspaces/wide-tide");
}

#[test]
fn workspace_branch_page_marks_branch_as_edited() {
    let global = GlobalConfig::default();
    let mut app = CreateWizardApp::new(draft(), global, Vec::new());

    app.jump_to_page(CreateWizardPage::WorkspaceBranch);
    app.clear_text_field_for_test();
    app.on_event(paste("feature/manual")).unwrap();
    app.on_event(key(KeyCode::Enter)).unwrap();

    assert_eq!(app.draft().branch, "feature/manual");
    assert!(app.draft().branch_was_edited);
}
```

Add test-only helpers under `#[cfg(test)]` if direct test access is needed:

```rust
#[cfg(test)]
pub fn jump_to_page(&mut self, page: CreateWizardPage) {
    let idx = self.pages.iter().position(|p| p == &page).expect("page exists");
    self.enter_page(idx);
}

#[cfg(test)]
pub fn clear_text_field_for_test(&mut self) {
    if let Some(field) = &mut self.text_field {
        *field = WizardTextField::new("", field.kind);
    }
}
```

- [ ] **Step 2: Run tests and verify they fail**

Run:

```bash
cargo test --test create_wizard_test
```

Expected: FAIL because `WorkspaceName` and `WorkspaceBranch` are not committed from the textarea yet.

- [ ] **Step 3: Implement WorkspaceName and WorkspaceBranch commit**

Extend `current_page_text`:

```rust
CreateWizardPage::WorkspaceName => {
    Some((self.draft.name.clone(), WizardTextKind::SingleLine))
}
CreateWizardPage::WorkspaceBranch => {
    Some((self.draft.branch.clone(), WizardTextKind::SingleLine))
}
```

Extend `commit_current_page`:

```rust
CreateWizardPage::WorkspaceName => {
    let text = self.text_field.as_ref().map(WizardTextField::text).unwrap_or_default();
    let name = text.trim().to_string();
    if name.is_empty() || name.contains('\n') {
        self.errors = vec![CreateDraftError::WorkspaceNameRequired];
        return false;
    }
    self.draft.set_name(name, &self.global);
}
CreateWizardPage::WorkspaceBranch => {
    let text = self.text_field.as_ref().map(WizardTextField::text).unwrap_or_default();
    let branch = text.trim().to_string();
    if branch.is_empty() || branch.contains('\n') {
        self.errors = vec![CreateDraftError::TargetBranchRequired("workspace".into())];
        return false;
    }
    self.draft.set_branch(branch);
}
```

- [ ] **Step 4: Add `WorkspaceNameRequired`**

In `src/cli/create_flow.rs`, update:

```rust
pub enum CreateDraftError {
    TitleRequired,
    WorkspaceNameRequired,
    WorkspaceNameExists(String),
    RepoRequired,
    TargetBranchRequired(String),
    DefaultAgentMissing,
}
```

In `CreateDraft::validate`:

```rust
if self.name.trim().is_empty() {
    errors.push(CreateDraftError::WorkspaceNameRequired);
}
```

In `src/tui_app/create_wizard.rs::error_belongs_to_step`:

```rust
CreateWizardPage::WorkspaceName => matches!(
    err,
    CreateDraftError::WorkspaceNameRequired | CreateDraftError::WorkspaceNameExists(_)
),
```

In `error_message`:

```rust
CreateDraftError::WorkspaceNameRequired => "workspace name is required".into(),
```

- [ ] **Step 5: Update render to show one text field page**

Replace Info multi-row rendering with page-specific rendering. For text pages, render only one textarea block and derived read-only metadata:

```rust
fn step_content_lines(&self) -> Vec<Line<'static>> {
    match self.page() {
        CreateWizardPage::WorkspaceName => vec![
            Line::from("Editing workspace name"),
            Line::from(format!("derived branch: {}", self.draft.branch)),
            Line::from(format!("derived workspace_dir: {}", self.draft.workspace_dir)),
        ],
        CreateWizardPage::WorkspaceBranch => vec![
            Line::from("Editing workspace branch"),
            Line::from(format!("workspace_dir: {}", self.draft.workspace_dir)),
        ],
        // keep other pages
    }
}
```

Keep textarea rendering in this task consistent with `WizardTextField::paragraph`; do not introduce a second text rendering path.

- [ ] **Step 6: Run tests**

Run:

```bash
cargo test --test create_wizard_test
cargo test --test create_flow_test
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add src/tui_app/create_wizard.rs src/cli/create_flow.rs tests/create_wizard_test.rs tests/create_flow_test.rs
git commit -m "feat(create): migrate workspace field pages"
```

---

## Task 4: Migrate Repos Page and Dynamic Target Branch Pages

**Files:**
- Modify: `src/tui_app/create_wizard.rs`
- Test: `tests/create_wizard_test.rs`

- [ ] **Step 1: Write failing tests for dynamic target branch pages**

Append:

```rust
#[test]
fn repo_selection_rebuilds_target_branch_pages() {
    let global = GlobalConfig::default();
    let mut draft = draft();
    draft.repos.push(RepoDraftEntry::new("backend", "develop", false));
    let mut app = CreateWizardApp::new(draft, global, Vec::new());

    assert_eq!(
        app.page_titles()
            .into_iter()
            .filter(|title| title.starts_with("Branches:"))
            .collect::<Vec<_>>(),
        vec!["Branches: frontend"]
    );

    app.jump_to_page(CreateWizardPage::Repos);
    app.on_event(key(KeyCode::Down)).unwrap();
    app.on_event(key(KeyCode::Char(' '))).unwrap();

    assert_eq!(
        app.page_titles()
            .into_iter()
            .filter(|title| title.starts_with("Branches:"))
            .collect::<Vec<_>>(),
        vec!["Branches: frontend", "Branches: backend"]
    );
}

#[test]
fn target_branch_page_commits_textarea_to_repo() {
    let global = GlobalConfig::default();
    let mut app = CreateWizardApp::new(draft(), global, Vec::new());

    app.jump_to_page(CreateWizardPage::TargetBranch {
        repo_name: "frontend".into(),
    });
    app.clear_text_field_for_test();
    app.on_event(paste("feature/current")).unwrap();
    app.on_event(key(KeyCode::Enter)).unwrap();

    assert_eq!(
        app.draft().repo("frontend").unwrap().target_branch,
        "feature/current"
    );
}
```

- [ ] **Step 2: Run tests and verify they fail**

Run:

```bash
cargo test --test create_wizard_test
```

Expected: FAIL because repo selection does not rebuild `pages` and target branch pages are not textarea-backed.

- [ ] **Step 3: Rebuild pages after repo selection changes**

Add helper:

```rust
fn rebuild_pages_preserving_current(&mut self) {
    let current = self.pages.get(self.page_index).cloned();
    self.pages = Self::build_pages(&self.draft);
    self.page_index = current
        .and_then(|page| self.pages.iter().position(|candidate| candidate == &page))
        .unwrap_or_else(|| self.page_index.min(self.pages.len().saturating_sub(1)));
    self.enter_page(self.page_index);
}
```

Call it after toggling a repo:

```rust
fn toggle_current_repo(&mut self) {
    // existing selected toggle
    self.clamp_active_cursor();
    self.rebuild_pages_preserving_current();
    self.refresh_current_step_errors();
}
```

- [ ] **Step 4: Implement target branch textarea commit**

Extend `current_page_text`:

```rust
CreateWizardPage::TargetBranch { repo_name } => self
    .draft
    .repo(repo_name)
    .map(|repo| (repo.target_branch.clone(), WizardTextKind::SingleLine)),
```

Add mutable lookup:

```rust
fn repo_mut(&mut self, name: &str) -> Option<&mut RepoDraftEntry> {
    self.draft.repos.iter_mut().find(|repo| repo.name == name)
}
```

Extend `commit_current_page`:

```rust
CreateWizardPage::TargetBranch { repo_name } => {
    let text = self.text_field.as_ref().map(WizardTextField::text).unwrap_or_default();
    let branch = text.trim().to_string();
    if branch.is_empty() || branch.contains('\n') {
        self.errors = vec![CreateDraftError::TargetBranchRequired(repo_name.clone())];
        return false;
    }
    if let Some(repo) = self.repo_mut(repo_name) {
        repo.target_branch = branch;
    }
}
```

- [ ] **Step 5: Add filter input state for Repos**

Add optional repo filter field:

```rust
repo_filter: WizardTextField,
repo_filter_active: bool,
```

Initialize with:

```rust
repo_filter: WizardTextField::new("", WizardTextKind::SingleLine),
repo_filter_active: false,
```

Add:

```rust
fn repo_filter_text(&self) -> String {
    self.repo_filter.text()
}

fn visible_repo_indices(&self) -> Vec<usize> {
    let filter = self.repo_filter_text().to_lowercase();
    self.draft
        .repos
        .iter()
        .enumerate()
        .filter(|(_, repo)| filter.is_empty() || repo.name.to_lowercase().contains(&filter))
        .map(|(idx, _)| idx)
        .collect()
}

fn selected_visible_repo_index(&self) -> Option<usize> {
    self.visible_repo_indices().get(self.repo_cursor).copied()
}
```

Key behavior:

- `Tab` on `Repos` toggles filter active.
- When filter active, printable chars and paste go to `repo_filter`.
- `repo_cursor` is the cursor inside `visible_repo_indices()`, not a raw index into `draft.repos`.
- Movement keys clamp `repo_cursor` against `visible_repo_indices().len()`.
- `Space` calls `selected_visible_repo_index()` and toggles that raw repo index.
- When the filter changes, clamp `repo_cursor` immediately so it never points outside the visible list.

Tests:

```rust
#[test]
fn repos_filter_limits_visible_repos_but_keeps_space_selection() {
    let global = GlobalConfig::default();
    let mut draft = draft();
    draft.repos.push(RepoDraftEntry::new("backend", "develop", false));
    let mut app = CreateWizardApp::new(draft, global, Vec::new());

    app.jump_to_page(CreateWizardPage::Repos);
    app.on_event(key(KeyCode::Tab)).unwrap();
    app.on_event(paste("back")).unwrap();
    app.on_event(key(KeyCode::Char(' '))).unwrap();

    assert!(app.draft().repo("backend").unwrap().selected);
}
```

- [ ] **Step 6: Run tests**

Run:

```bash
cargo test --test create_wizard_test
cargo test --test create_flow_test
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add src/tui_app/create_wizard.rs tests/create_wizard_test.rs
git commit -m "feat(create): add dynamic branch field pages"
```

---

## Task 5: Migrate AfterCreate and Conditional RunAgent Page

**Files:**
- Modify: `src/tui_app/create_wizard.rs`
- Test: `tests/create_wizard_test.rs`

- [ ] **Step 1: Write failing tests for conditional RunAgent page**

Append:

```rust
#[test]
fn after_create_run_agent_mode_inserts_run_agent_page() {
    let global = GlobalConfig {
        agent_cli: Some("codex".into()),
        ..Default::default()
    };
    let mut app = CreateWizardApp::new(draft(), global, Vec::new());

    app.jump_to_page(CreateWizardPage::AfterCreate);
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

    app.jump_to_page(CreateWizardPage::RunAgent);
    app.clear_text_field_for_test();
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

    app.jump_to_page(CreateWizardPage::RunAgent);
    app.clear_text_field_for_test();
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
```

- [ ] **Step 2: Run tests and verify they fail**

Run:

```bash
cargo test --test create_wizard_test
```

Expected: FAIL because `RunAgent` is not fully page-driven yet.

- [ ] **Step 3: Rebuild pages after after-create mode changes**

Update `move_after_create_cursor` and commit logic:

```rust
fn set_after_create_from_cursor(&mut self) {
    self.draft.after_create = self.mode_for_after_create_cursor(self.after_create_cursor);
    self.rebuild_pages_preserving_current();
    self.refresh_current_step_errors();
}
```

In `commit_current_page` for `CreateWizardPage::AfterCreate`:

```rust
CreateWizardPage::AfterCreate => {
    self.draft.after_create = self.mode_for_after_create_cursor(self.after_create_cursor);
    self.rebuild_pages_preserving_current();
}
```

After commit, next page should naturally be `RunAgent` if it exists, otherwise `Review`.

- [ ] **Step 4: Implement RunAgent textarea commit**

Extend `current_page_text`:

```rust
CreateWizardPage::RunAgent => Some((
    self.run_agent_value.clone().unwrap_or_default(),
    WizardTextKind::SingleLine,
)),
```

Extend `commit_current_page`:

```rust
CreateWizardPage::RunAgent => {
    let text = self.text_field.as_ref().map(WizardTextField::text).unwrap_or_default();
    let value = text.trim().to_string();
    if value.contains('\n') {
        self.errors = vec![CreateDraftError::DefaultAgentMissing];
        return false;
    }
    self.run_agent_value = if value.is_empty() { None } else { Some(value) };
    self.draft.after_create = AfterCreateMode::StartAndRunAgent {
        run_agent: self.run_agent_value.clone(),
    };
    if self.run_agent_value.is_none() && self.global.agent_cli.is_none() {
        self.errors = vec![CreateDraftError::DefaultAgentMissing];
        return false;
    }
}
```

- [ ] **Step 5: Run tests**

Run:

```bash
cargo test --test create_wizard_test
cargo test --test create_flow_test
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/tui_app/create_wizard.rs tests/create_wizard_test.rs
git commit -m "feat(create): add conditional run agent field page"
```

---

## Task 6: Replace Rendering and Remove Old Step/Edit Mode

**Files:**
- Modify: `src/tui_app/create_wizard.rs`
- Test: `tests/create_wizard_test.rs`

- [ ] **Step 1: Write failing render smoke tests for single field pages**

Append:

```rust
#[test]
fn render_title_page_shows_one_text_input_and_draft_preview() {
    let global = GlobalConfig::default();
    let mut app = CreateWizardApp::new(draft(), global, Vec::new());

    let out = render_to_string(&mut app, 120, 24);

    assert!(out.contains("Workspace: Title"), "{out}");
    assert!(out.contains("Draft"), "{out}");
    assert!(out.contains("auth cleanup"), "{out}");
    assert!(!out.contains("Workspace: Description"), "{out}");
}

#[test]
fn render_name_page_shows_derived_workspace_dir_as_read_only_context() {
    let global = GlobalConfig::default();
    let mut app = CreateWizardApp::new(draft(), global, Vec::new());

    app.jump_to_page(CreateWizardPage::WorkspaceName);
    let out = render_to_string(&mut app, 120, 24);

    assert!(out.contains("Workspace: Name"), "{out}");
    assert!(out.contains("workspace_dir"), "{out}");
}
```

- [ ] **Step 2: Run tests and verify they fail**

Run:

```bash
cargo test --test create_wizard_test
```

Expected: FAIL if old multi-field render still leaks.

- [ ] **Step 3: Render by `CreateWizardPage`**

Replace `step_title` with:

```rust
fn page_title(&self) -> String {
    self.page().title()
}
```

Change `step_paragraph` to render `page_title`.

For text pages, render the `WizardTextField` content as the only active editor:

```rust
fn text_field_lines(&self) -> Vec<Line<'static>> {
    self.text_field
        .as_ref()
        .map(|field| field.text().lines().map(|line| Line::from(line.to_string())).collect())
        .unwrap_or_else(Vec::new)
}
```

For `WorkspaceName`, append read-only derived lines after the text field:

```rust
Line::from(format!("derived branch: {}", self.draft.branch)),
Line::from(format!("workspace_dir: {}", self.draft.workspace_dir)),
```

For `Repos`, render filter text and visible repo list.

For `TargetBranch`, render current branch text field and repo name.

For `AfterCreate`, render mode list.

For `RunAgent`, render current run-agent text field and default hint.

For `Review`, render full summary.

- [ ] **Step 4: Remove old `CreateStep` as state owner**

Delete or stop using:

- `CreateStep` as the primary state machine.
- `step: CreateStep`.
- `info_cursor`.
- `branch_editing`.
- `after_create_editing`.
- old `edit_active_field` string mutation path.

Keep any compatibility test helpers only if they are still meaningful. Update tests from `app.step()` to `app.page()` where the new page model is the source of truth.

- [ ] **Step 5: Run focused tests**

Run:

```bash
cargo test --test create_wizard_test
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/tui_app/create_wizard.rs tests/create_wizard_test.rs
git commit -m "refactor(create): render wizard field pages"
```

---

## Task 7: Preserve CLI Semantics and Final Verification

**Files:**
- Modify only if verification exposes behavior issues.

- [ ] **Step 1: Run focused create tests**

Run:

```bash
cargo test --test create_flow_test
cargo test --test create_wizard_test
```

Expected: PASS.

- [ ] **Step 2: Run full verification**

Run:

```bash
cargo fmt --check
cargo test
cargo build
cargo clippy --all-targets -- -D warnings
```

Expected: all commands exit 0.

- [ ] **Step 3: Run isolated CLI smoke**

Run:

```bash
tmp_home=$(mktemp -d /tmp/zootree-smoke-home.XXXXXX)
repo_dir=$(mktemp -d /tmp/zootree-smoke-repo.XXXXXX)
HOME="$tmp_home" target/debug/zootree repo add --name smoke --default-target-branch main "$repo_dir"
HOME="$tmp_home" target/debug/zootree create --title "smoke create" --name smoke-create --repos smoke:main --branch zt/smoke-create
test -f "$tmp_home/.config/zootree/workspaces/pending/smoke-create.toml"
test -f "$tmp_home/.config/zootree/templates/recently.toml"
rm -rf "$tmp_home" "$repo_dir"
```

Expected: command exits 0 and prints created workspace output.

- [ ] **Step 4: Request final code review**

Dispatch a final reviewer over the full implementation range. Include these checks:

- Field pages are one-field-one-page.
- Text fields use textarea-backed editing.
- `Enter` submits; `Alt+Enter` / `Shift+Enter` newline.
- Repos filter and list selection both work.
- Target branch pages update dynamically.
- RunAgent page is conditional.
- CLI non-interactive behavior is unchanged.

- [ ] **Step 5: Commit verification fixes when review finds issues**

When verification or review finds issues, fix them with focused failing tests first, then commit:

```bash
git add src/tui_app/create_wizard.rs tests/create_wizard_test.rs src/cli/create_flow.rs tests/create_flow_test.rs
git commit -m "fix(create): polish single field wizard"
```

When there are no fixes, do not create an empty commit.
