# Create Wizard Module Split Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Split `CreateWizardApp` into focused modules while preserving every existing create wizard behavior and public import path.

**Architecture:** Convert the single `create_wizard.rs` file into a directory module with a stable `mod.rs` facade. Keep `CreateWizardApp` as the sole state owner, distribute inherent `impl` blocks by responsibility using `pub(super)` interfaces, and preserve `tests/create_wizard_test.rs` as the behavior contract throughout the extraction.

**Tech Stack:** Rust 2021, ratatui 0.29, crossterm 0.28, ratatui-textarea, Cargo integration tests.

## Global Constraints

- Preserve the current keyboard contract, layout contract, and persisted `CreateDraft` model.
- Preserve the public path `zootree::tui_app::create_wizard::*` used by integration tests and callers.
- Do not weaken or replace existing `tests/create_wizard_test.rs` assertions.
- Do not mix unrelated behavior fixes into the refactor.
- Prefer `pub(super)` for cross-module implementation details; do not expand the public API merely to support the split.
- After adding or renaming source modules, update `skills/zootree-dev/SKILL.md` to match the real tree.

## File Structure

- Create `src/tui_app/create_wizard/mod.rs`: stable facade, public outcome, app runner, module declarations, and public re-exports.
- Create `src/tui_app/create_wizard/state.rs`: app fields, public state types, constructor/accessors, page construction, and page synchronization.
- Create `src/tui_app/create_wizard/text_field.rs`: text field kind and `ratatui-textarea` adapter.
- Create `src/tui_app/create_wizard/repo_page.rs`: repo labels, filtering, selection, cursor movement, and visible window calculation.
- Create `src/tui_app/create_wizard/navigation.rs`: validation, commit, navigation, cursor dispatch, event handling, and `App` lifecycle methods excluding rendering.
- Create `src/tui_app/create_wizard/render.rs`: responsive rendering, formatting, errors/help, and scroll calculations/tests.
- Delete `src/tui_app/create_wizard.rs` after all code has moved.
- Modify `skills/zootree-dev/SKILL.md`: document the new create wizard module tree and responsibility split.
- Keep `tests/create_wizard_test.rs` unchanged unless a compile-only import correction is strictly required; no behavior assertion changes are allowed.

---

### Task 1: Establish Baseline And Extract State/Text Foundations

**Files:**
- Create: `src/tui_app/create_wizard/mod.rs`
- Create: `src/tui_app/create_wizard/state.rs`
- Create: `src/tui_app/create_wizard/text_field.rs`
- Modify/Delete: `src/tui_app/create_wizard.rs`
- Test: `tests/create_wizard_test.rs`

**Interfaces:**
- Produces: `pub use state::{CreateStep, CreateWizardApp, CreateWizardPage};`
- Produces: `pub(super) enum WizardTextKind { SingleLine, Multiline }`
- Produces: `pub(super) struct WizardTextField` with `new`, `text`, `handle_key`, and `handle_paste`.
- Preserves: `CreateWizardApp::{new, with_outcome_handle, step, set_step, page, page_titles, draft, repo_cursor, after_create_cursor, errors, outcome}`.

- [ ] **Step 1: Record the green behavior baseline**

Run:

```bash
cargo test --test create_wizard_test
```

Expected: all existing create wizard integration tests pass before file movement.

- [ ] **Step 2: Convert the file module to a directory facade**

Move the current implementation under `src/tui_app/create_wizard/mod.rs` without changing code, then confirm Rust still resolves `pub mod create_wizard;` from `src/tui_app/mod.rs`.

Run:

```bash
cargo test --test create_wizard_test
```

Expected: the same test count passes with no import changes in `tests/create_wizard_test.rs`.

- [ ] **Step 3: Extract the text adapter mechanically**

Move `WizardTextKind`, `WizardTextField`, and its complete implementation into `text_field.rs`. Expose only these parent-module interfaces:

```rust
pub(super) enum WizardTextKind {
    SingleLine,
    Multiline,
}

pub(super) struct WizardTextField {
    textarea: TextArea<'static>,
}

impl WizardTextField {
    pub(super) fn new(text: impl Into<String>, _kind: WizardTextKind) -> Self;
    pub(super) fn text(&self) -> String;
    pub(super) fn handle_key(&mut self, key: crossterm::event::KeyEvent);
    pub(super) fn handle_paste(&mut self, text: &str);
}
```

Do not alter key matching, cursor initialization, wrapping mode, or newline behavior.

- [ ] **Step 4: Extract state definitions and synchronization**

Move `CreateStep`, `CreateWizardPage`, its `title` method, `CreateWizardApp` fields, constructors/accessors, and these helpers into `state.rs`:

```rust
pub(super) fn build_pages(draft: &CreateDraft) -> Vec<CreateWizardPage>;
pub(super) fn page_text_kind(page: &CreateWizardPage) -> Option<WizardTextKind>;
pub(super) fn step_for_page(page: &CreateWizardPage) -> CreateStep;
pub(super) fn first_page_index_for_step(&self, step: CreateStep) -> usize;
pub(super) fn enter_page(&mut self, page_index: usize);
pub(super) fn sync_current_page_state(&mut self, reset_editing: bool);
pub(super) fn current_page_text(&self) -> Option<(String, WizardTextKind)>;
pub(super) fn clamp_page_index(&mut self);
pub(super) fn refresh_pages(&mut self);
pub(super) fn repo_mut(&mut self, name: &str) -> Option<&mut RepoDraftEntry>;
```

Keep every field private to the create wizard module family by using `pub(super)` only where sibling `impl` modules require access.

- [ ] **Step 5: Verify foundation extraction**

Run:

```bash
cargo fmt --all
cargo test --test create_wizard_test
git diff --check
```

Expected: formatting succeeds, all create wizard tests pass, and there are no whitespace errors.

- [ ] **Step 6: Commit the foundation extraction**

```bash
git add src/tui_app/create_wizard.rs src/tui_app/create_wizard
git commit -m "refactor(tui): extract create wizard state"
```

### Task 2: Extract Repo Page And Navigation Behavior

**Files:**
- Create: `src/tui_app/create_wizard/repo_page.rs`
- Create: `src/tui_app/create_wizard/navigation.rs`
- Modify: `src/tui_app/create_wizard/mod.rs`
- Modify: `src/tui_app/create_wizard/state.rs`
- Test: `tests/create_wizard_test.rs`

**Interfaces:**
- Produces: public facade functions `repo_list_label` and `review_repo_label` with unchanged signatures.
- Produces: sibling methods for repo filtering, visible indices, selection, cursor movement, and active cursor clamping.
- Produces: `impl App for CreateWizardApp::on_event` and `should_quit` with unchanged event ordering.
- Consumes: state and text-field interfaces from Task 1.

- [ ] **Step 1: Extract repo labels and repo-page mechanics**

Move the following behavior unchanged into `repo_page.rs`:

```rust
pub fn repo_list_label(repo: &RepoDraftEntry, selected: bool, focused: bool) -> String;
pub fn review_repo_label(repo: &RepoDraftEntry) -> String;

impl CreateWizardApp {
    pub(super) fn toggle_current_repo(&mut self);
    pub(super) fn move_repo_cursor(&mut self, delta: isize);
    pub(super) fn repo_filter_text(&self) -> String;
    pub(super) fn visible_repo_indices(&self) -> Vec<usize>;
    pub(super) fn selected_repo_count(&self) -> usize;
    pub(super) fn clamp_active_cursor(&mut self);
    pub(super) fn repo_window_start(visible_count: usize, cursor: usize, capacity: usize) -> usize;
}
```

Re-export both public label helpers from `mod.rs`. Preserve filtering case conversion, cursor wrapping, dynamic page refresh, and selection semantics exactly.

- [ ] **Step 2: Extract validation and page commit behavior**

Move step error filtering, validation refresh, single-line cleaning, target-branch cleaning, `commit_current_page`, and `commit_text_page_if_present` into `navigation.rs`. Preserve the exact `CreateDraftError` mapping and mutation order.

- [ ] **Step 3: Extract navigation and selection behavior**

Move `should_commit_before_back`, `submit_or_advance`, `cancel_or_back`, info-page movement, after-create mode conversion/movement, and active cursor dispatch into `navigation.rs`. Do not change page refresh timing or Review submission validation.

- [ ] **Step 4: Extract event handling without reordering match arms**

Move `handle_text_key` and the `App::on_event`/`should_quit` implementation into `navigation.rs`. Keep the existing match-arm order verbatim so text fields, repo filters, Review scrolling, global cancellation, and aliases retain precedence.

- [ ] **Step 5: Verify navigation extraction**

Run:

```bash
cargo fmt --all
cargo test --test create_wizard_test
git diff --check
```

Expected: all keyboard, validation, repo, and submission tests pass unchanged.

- [ ] **Step 6: Commit repo and navigation extraction**

```bash
git add src/tui_app/create_wizard
git commit -m "refactor(tui): extract create wizard navigation"
```

### Task 3: Extract Rendering And Scroll Behavior

**Files:**
- Create: `src/tui_app/create_wizard/render.rs`
- Modify: `src/tui_app/create_wizard/mod.rs`
- Modify: `src/tui_app/create_wizard/state.rs`
- Test: `src/tui_app/create_wizard/render.rs`
- Test: `tests/create_wizard_test.rs`

**Interfaces:**
- Produces: `CreateWizardApp::render_frame`, called by the single `impl App for CreateWizardApp` retained in `navigation.rs`.
- Produces: render helpers and wrapping-aware scroll helpers scoped to sibling modules.
- Consumes: state access and repo labels/windowing from Tasks 1-2.

- [ ] **Step 1: Move responsive layout and page rendering**

Move `render_too_narrow`, `render_two_column`, `render_single_column`, `render_summary`, `render_step`, `render_text_page`, `page_paragraph`, `render_review_page`, and `repos_page_paragraph` into `render.rs`. Preserve every `Constraint`, block title, border, wrap, scroll, and width threshold.

- [ ] **Step 2: Move render-only content formatting**

Move `page_title`, `after_create_label`, summary/description lines, page content/context, error messages, and help text into `render.rs`. Keep all strings and ordering unchanged.

- [ ] **Step 3: Move scroll calculations and scroll mutation helpers**

Move visible dimensions, wrapped line counting, clamp logic, page amounts, and Review/Draft up/down helpers into `render.rs`. Move the two existing unit tests alongside those helpers without changing assertions:

```rust
#[test]
fn clamp_scroll_accounts_for_wrapped_lines();

#[test]
fn clamp_scroll_keeps_empty_line_at_top_when_visible();
```

- [ ] **Step 4: Attach rendering through the existing App trait**

Implement only `fn render(&mut self, frame: &mut ratatui::Frame)` in the `App` implementation located in `render.rs`, dispatching on `CreateWizardLayout::for_width(area.width)` exactly as before. Rust permits separate inherent impl blocks but only one trait impl, so keep a single `impl App for CreateWizardApp` in `navigation.rs` and delegate its `render` method to `self.render_frame(frame)` implemented in `render.rs`:

```rust
pub(super) fn render_frame(&mut self, frame: &mut ratatui::Frame) {
    let area = frame.area();
    match CreateWizardLayout::for_width(area.width) {
        CreateWizardLayout::TooNarrow => self.render_too_narrow(frame, area),
        CreateWizardLayout::TwoColumn => self.render_two_column(frame, area),
        CreateWizardLayout::SingleColumn => self.render_single_column(frame, area),
    }
}
```

- [ ] **Step 5: Verify rendering extraction**

Run:

```bash
cargo fmt --all
cargo test --test create_wizard_test
cargo test --lib tui_app::create_wizard
git diff --check
```

Expected: integration rendering/viewport tests and moved unit tests all pass.

- [ ] **Step 6: Commit rendering extraction**

```bash
git add src/tui_app/create_wizard
git commit -m "refactor(tui): extract create wizard rendering"
```

### Task 4: Finalize Facade, Documentation, And Full Verification

**Files:**
- Modify: `src/tui_app/create_wizard/mod.rs`
- Modify: `skills/zootree-dev/SKILL.md`
- Verify: `tests/create_wizard_test.rs`

**Interfaces:**
- Preserves: `CreateWizardOutcome` and `run_create_wizard` under `tui_app::create_wizard`.
- Preserves: public re-exports for `CreateStep`, `CreateWizardApp`, `CreateWizardPage`, `repo_list_label`, and `review_repo_label`.

- [ ] **Step 1: Reduce mod.rs to the stable facade**

Keep only module declarations, public re-exports, `CreateWizardOutcome`, and `run_create_wizard` in `mod.rs`. The runner must retain the existing outcome handle and cancellation conversion:

```rust
pub fn run_create_wizard(
    draft: CreateDraft,
    global: GlobalConfig,
    existing_workspaces: Vec<String>,
) -> anyhow::Result<CreateWizardOutput>;
```

Confirm `tests/create_wizard_test.rs` still imports the same symbols from the same path.

- [ ] **Step 2: Update the zootree development skill architecture**

Replace the single create wizard file entry with the real module directory and concise responsibility descriptions in `skills/zootree-dev/SKILL.md`. Verify the tree with:

```bash
find src/tui_app/create_wizard -type f -name '*.rs' | sort
```

- [ ] **Step 3: Run required verification**

Run:

```bash
cargo fmt --check
cargo test --test create_wizard_test
cargo test
git diff --check
```

Expected: every command exits successfully with no test failures or formatting errors.

- [ ] **Step 4: Inspect scope and public API preservation**

Run:

```bash
git diff --stat HEAD~3
git diff HEAD~3 -- tests/create_wizard_test.rs src/cli/create_flow.rs
rg -n "pub use|pub fn run_create_wizard|pub enum CreateWizardOutcome" src/tui_app/create_wizard/mod.rs
```

Expected: no unrelated create-flow behavior change, no weakened integration tests, and the facade exposes the original public symbols.

- [ ] **Step 5: Commit documentation and final cleanup**

```bash
git add src/tui_app/create_wizard skills/zootree-dev/SKILL.md
git commit -m "docs(tui): document create wizard modules"
```
