# Create Wizard Review/Draft Scroll Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let users scroll both the Review page and Draft summary in `zootree create` when long descriptions overflow the visible wizard areas.

**Architecture:** Keep the change inside `src/tui_app/create_wizard.rs`. Add UI-only scroll state to `CreateWizardApp`, render Review and Draft through scrollable `Paragraph`s, and route Review-page key bindings to either `review_scroll` or `draft_scroll`. Keep `CreateDraft`, persistence, validation, and mouse handling unchanged.

**Tech Stack:** Rust, ratatui `Paragraph::scroll`, crossterm key events, existing `CreateWizardApp` tests with `ratatui::backend::TestBackend`.

---

## File Structure

- Modify: `tests/create_wizard_test.rs`
  - Add focused render/event regression tests for Review scroll, Draft scroll, help text, and unchanged submit behavior.
  - Reuse existing helpers: `key`, `key_mod`, `render_to_string`, `draft`.
- Modify: `src/tui_app/create_wizard.rs`
  - Add `review_scroll`, `draft_scroll`, `last_review_height`, and `last_draft_height` fields.
  - Add scroll helpers for line clamp, page amount, and scroll movement.
  - Render Draft and Review via mutable render helpers that apply scroll.
  - Add Review-page key handling for normal Review scroll and `Alt+...` Draft scroll.
  - Update Review help text.

No new files are required beyond this plan.

---

### Task 1: Add Failing Scroll Tests

**Files:**
- Modify: `tests/create_wizard_test.rs`

- [ ] **Step 1: Add a long-description test helper**

Insert this helper after the existing `draft()` helper:

```rust
fn long_description_draft() -> CreateDraft {
    let mut draft = draft();
    draft.description = (0..30)
        .map(|idx| format!("desc-line-{idx:02}"))
        .collect::<Vec<_>>()
        .join("\n");
    draft
}
```

- [ ] **Step 2: Add Review scroll regression tests**

Insert these tests near the existing Review tests, before `review_step_exposes_all_errors_and_does_not_submit`:

```rust
#[test]
fn review_end_scrolls_long_review_content_into_view() {
    let global = GlobalConfig::default();
    let mut app = CreateWizardApp::new(long_description_draft(), global, Vec::new());
    app.set_step(CreateStep::Review);

    let before = render_to_string(&mut app, 120, 12);
    assert!(
        !before.contains("desc-line-29"),
        "last description line should start below the review viewport:\n{before}"
    );

    app.on_event(key(KeyCode::End)).unwrap();

    let after = render_to_string(&mut app, 120, 12);
    assert!(
        after.contains("desc-line-29"),
        "End should reveal the last description line in review content:\n{after}"
    );
}

#[test]
fn review_page_down_scrolls_review_without_submitting() {
    let global = GlobalConfig::default();
    let mut app = CreateWizardApp::new(long_description_draft(), global, Vec::new());
    app.set_step(CreateStep::Review);

    app.on_event(key(KeyCode::PageDown)).unwrap();

    assert_eq!(app.step(), CreateStep::Review);
    assert!(!app.should_quit());
    assert_eq!(app.outcome(), None);

    app.on_event(key(KeyCode::Enter)).unwrap();

    assert!(app.should_quit());
    match app.outcome() {
        Some(CreateWizardOutcome::Submit(output)) => {
            assert_eq!(output.draft.title, "auth cleanup");
        }
        other => panic!("expected submitted output after Enter, got {:?}", other),
    }
}
```

- [ ] **Step 3: Add Draft scroll regression test**

Insert this test near the tests from Step 2:

```rust
#[test]
fn alt_end_scrolls_long_draft_summary_into_view() {
    let global = GlobalConfig::default();
    let mut app = CreateWizardApp::new(long_description_draft(), global, Vec::new());
    app.set_step(CreateStep::Review);

    let before = render_to_string(&mut app, 120, 12);
    assert!(
        !before.contains("desc-line-29"),
        "last description line should start below the draft viewport:\n{before}"
    );

    app.on_event(key_mod(KeyCode::End, KeyModifiers::ALT))
        .unwrap();

    let after = render_to_string(&mut app, 120, 12);
    assert!(
        after.contains("desc-line-29"),
        "Alt+End should reveal the last description line in the draft summary:\n{after}"
    );
}
```

- [ ] **Step 4: Update the Review help test to expect scroll help**

Replace `review_help_does_not_advertise_missing_movement` with:

```rust
#[test]
fn review_help_advertises_review_and_draft_scroll() {
    let global = GlobalConfig::default();
    let mut app = CreateWizardApp::new(draft(), global, Vec::new());
    app.set_step(CreateStep::Review);

    let out = render_to_string(&mut app, 120, 24);

    assert!(out.contains("enter submit"), "missing submit help:\n{out}");
    assert!(
        out.contains("up/down review"),
        "missing review scroll help:\n{out}"
    );
    assert!(
        out.contains("alt+up/down draft"),
        "missing draft scroll help:\n{out}"
    );
}
```

- [ ] **Step 5: Run the focused tests and verify they fail**

Run:

```bash
cargo test --test create_wizard_test -- --nocapture
```

Expected: FAIL. At least the new scroll tests should fail because `End`, `PageDown`, and `Alt+End` do not scroll Review/Draft yet, and the help text still lacks scroll help.

- [ ] **Step 6: Commit the failing tests**

```bash
git add tests/create_wizard_test.rs
git commit -m "test(create): cover review and draft scroll"
```

---

### Task 2: Add Scroll State and Rendering

**Files:**
- Modify: `src/tui_app/create_wizard.rs`

- [ ] **Step 1: Add scroll fields to `CreateWizardApp`**

Change the struct around the existing state fields to include four UI-only scroll fields:

```rust
pub struct CreateWizardApp {
    draft: CreateDraft,
    global: GlobalConfig,
    existing_workspaces: Vec<String>,
    pages: Vec<CreateWizardPage>,
    page_index: usize,
    repo_cursor: usize,
    repo_filter: WizardTextField,
    repo_filter_active: bool,
    after_create_cursor: usize,
    run_agent_value: Option<String>,
    text_field: Option<WizardTextField>,
    errors: Vec<CreateDraftError>,
    review_scroll: usize,
    draft_scroll: usize,
    last_review_height: u16,
    last_draft_height: u16,
    outcome: Rc<RefCell<Option<CreateWizardOutcome>>>,
}
```

- [ ] **Step 2: Initialize the new fields**

In `CreateWizardApp::with_outcome_handle`, add the new fields before `outcome`:

```rust
            errors: Vec::new(),
            review_scroll: 0,
            draft_scroll: 0,
            last_review_height: 1,
            last_draft_height: 1,
            outcome,
```

- [ ] **Step 3: Add shared scroll helpers**

Add these methods inside `impl CreateWizardApp`, after `clamp_active_cursor`:

```rust
    fn scroll_visible_height(area: Rect) -> u16 {
        area.height.saturating_sub(2).max(1)
    }

    fn max_scroll_for_lines(line_count: usize, visible_height: u16) -> usize {
        line_count
            .saturating_sub(visible_height as usize)
            .min(u16::MAX as usize)
    }

    fn clamp_scroll_to_lines(scroll: &mut usize, line_count: usize, visible_height: u16) {
        *scroll = (*scroll).min(Self::max_scroll_for_lines(line_count, visible_height));
    }

    fn scroll_page_amount(visible_height: u16) -> usize {
        visible_height.saturating_sub(1).max(1) as usize
    }
```

- [ ] **Step 4: Add Review and Draft scroll movement helpers**

Add these methods after the helpers from Step 3:

```rust
    fn review_page_scroll_amount(&self) -> usize {
        Self::scroll_page_amount(self.last_review_height)
    }

    fn draft_page_scroll_amount(&self) -> usize {
        Self::scroll_page_amount(self.last_draft_height)
    }

    fn scroll_review_down(&mut self, amount: usize) {
        self.review_scroll = self.review_scroll.saturating_add(amount);
    }

    fn scroll_review_up(&mut self, amount: usize) {
        self.review_scroll = self.review_scroll.saturating_sub(amount);
    }

    fn scroll_draft_down(&mut self, amount: usize) {
        self.draft_scroll = self.draft_scroll.saturating_add(amount);
    }

    fn scroll_draft_up(&mut self, amount: usize) {
        self.draft_scroll = self.draft_scroll.saturating_sub(amount);
    }
```

- [ ] **Step 5: Replace `summary_paragraph` with a render helper**

Replace the existing `summary_paragraph(&self, compact: bool) -> Paragraph<'static>` method with:

```rust
    fn render_summary(&mut self, frame: &mut ratatui::Frame, area: Rect, compact: bool) {
        let lines = self.summary_lines(compact);
        self.last_draft_height = Self::scroll_visible_height(area);
        Self::clamp_scroll_to_lines(
            &mut self.draft_scroll,
            lines.len(),
            self.last_draft_height,
        );
        let paragraph = Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title("Draft"))
            .wrap(Wrap { trim: false })
            .scroll((self.draft_scroll as u16, 0));
        frame.render_widget(paragraph, area);
    }
```

- [ ] **Step 6: Render the Draft summary through `render_summary`**

Change `render_two_column` and `render_single_column` to take `&mut self`, and use `render_summary`:

```rust
    fn render_two_column(&mut self, frame: &mut ratatui::Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(3), Constraint::Length(1)])
            .split(area);
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(62), Constraint::Percentage(38)])
            .split(chunks[0]);

        self.render_step(frame, columns[0]);
        self.render_summary(frame, columns[1], false);
        frame.render_widget(self.help_paragraph(), chunks[1]);
    }

    fn render_single_column(&mut self, frame: &mut ratatui::Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(8),
                Constraint::Min(3),
                Constraint::Length(1),
            ])
            .split(area);

        self.render_summary(frame, chunks[0], true);
        self.render_step(frame, chunks[1]);
        frame.render_widget(self.help_paragraph(), chunks[2]);
    }
```

- [ ] **Step 7: Add a dedicated scrollable Review renderer**

Add this method near `page_paragraph`:

```rust
    fn render_review_page(&mut self, frame: &mut ratatui::Frame, area: Rect) {
        let title = self.page_title();
        let mut lines = vec![Line::from(Span::styled(
            title.clone(),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ))];
        lines.push(Line::from(""));
        lines.extend(self.page_content_lines());
        lines.extend(self.error_lines());

        self.last_review_height = Self::scroll_visible_height(area);
        Self::clamp_scroll_to_lines(
            &mut self.review_scroll,
            lines.len(),
            self.last_review_height,
        );

        let paragraph = Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title(title))
            .wrap(Wrap { trim: false })
            .scroll((self.review_scroll as u16, 0));
        frame.render_widget(paragraph, area);
    }
```

- [ ] **Step 8: Route Review rendering through the new helper**

Change `render_step` to take `&mut self` and route Review separately:

```rust
    fn render_step(&mut self, frame: &mut ratatui::Frame, area: Rect) {
        if let Some(field) = &self.text_field {
            self.render_text_page(frame, area, field);
        } else if self.page() == &CreateWizardPage::Repos {
            frame.render_widget(self.repos_page_paragraph(area), area);
        } else if self.page() == &CreateWizardPage::Review {
            self.render_review_page(frame, area);
        } else {
            frame.render_widget(self.page_paragraph(), area);
        }
    }
```

- [ ] **Step 9: Run focused tests and verify scroll tests still fail on events only if key handling is missing**

Run:

```bash
cargo test --test create_wizard_test review_end_scrolls_long_review_content_into_view -- --nocapture
cargo test --test create_wizard_test alt_end_scrolls_long_draft_summary_into_view -- --nocapture
```

Expected: FAIL until Task 3 adds key handling. If compilation fails, fix borrow or signature mismatches before continuing; the expected signatures are `render_two_column(&mut self, ...)`, `render_single_column(&mut self, ...)`, and `render_step(&mut self, ...)`.

- [ ] **Step 10: Commit rendering support**

```bash
git add src/tui_app/create_wizard.rs
git commit -m "feat(create): render scrollable review and draft"
```

---

### Task 3: Add Review-Page Scroll Key Handling and Help Text

**Files:**
- Modify: `src/tui_app/create_wizard.rs`

- [ ] **Step 1: Capture `alt` in event handling**

In `impl App for CreateWizardApp`, after the existing `ctrl` assignment, add:

```rust
        let alt = key.modifiers.contains(KeyModifiers::ALT);
```

The nearby code should become:

```rust
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let step = self.step();
```

- [ ] **Step 2: Add Draft scroll key branches before generic movement**

In the `match key.code` block, insert these branches after the repo-filter editing branches and before the existing generic `KeyCode::Down | KeyCode::Char('j')` branch:

```rust
            KeyCode::Down | KeyCode::Char('j') if step == CreateStep::Review && alt => {
                self.scroll_draft_down(1);
            }
            KeyCode::Up | KeyCode::Char('k') if step == CreateStep::Review && alt => {
                self.scroll_draft_up(1);
            }
            KeyCode::PageDown if step == CreateStep::Review && alt => {
                self.scroll_draft_down(self.draft_page_scroll_amount());
            }
            KeyCode::PageUp if step == CreateStep::Review && alt => {
                self.scroll_draft_up(self.draft_page_scroll_amount());
            }
            KeyCode::Home if step == CreateStep::Review && alt => {
                self.draft_scroll = 0;
            }
            KeyCode::End if step == CreateStep::Review && alt => {
                self.draft_scroll = usize::MAX;
            }
```

- [ ] **Step 3: Add Review scroll key branches before generic movement**

Immediately after the Draft scroll branches from Step 2, insert:

```rust
            KeyCode::Down | KeyCode::Char('j') if step == CreateStep::Review => {
                self.scroll_review_down(1);
            }
            KeyCode::Up | KeyCode::Char('k') if step == CreateStep::Review => {
                self.scroll_review_up(1);
            }
            KeyCode::PageDown if step == CreateStep::Review => {
                self.scroll_review_down(self.review_page_scroll_amount());
            }
            KeyCode::PageUp if step == CreateStep::Review => {
                self.scroll_review_up(self.review_page_scroll_amount());
            }
            KeyCode::Home if step == CreateStep::Review => {
                self.review_scroll = 0;
            }
            KeyCode::End if step == CreateStep::Review => {
                self.review_scroll = usize::MAX;
            }
```

This placement matters: text pages are still handled earlier by `self.text_field.is_some() && self.handle_text_key(key)`, and repo-filter editing is still handled before Review scroll.

- [ ] **Step 4: Update Review help text**

Replace the existing Review help arm:

```rust
            CreateWizardPage::Review => {
                "enter submit · esc back · ctrl+c abort"
            }
```

with:

```rust
            CreateWizardPage::Review => {
                "enter submit · esc back · up/down review · alt+up/down draft · pg/home/end scroll · ctrl+c abort"
            }
```

- [ ] **Step 5: Run the focused scroll/help tests**

Run:

```bash
cargo test --test create_wizard_test review_end_scrolls_long_review_content_into_view -- --nocapture
cargo test --test create_wizard_test alt_end_scrolls_long_draft_summary_into_view -- --nocapture
cargo test --test create_wizard_test review_page_down_scrolls_review_without_submitting -- --nocapture
cargo test --test create_wizard_test review_help_advertises_review_and_draft_scroll -- --nocapture
```

Expected: PASS.

- [ ] **Step 6: Run existing text-entry guard tests**

Run:

```bash
cargo test --test create_wizard_test target_branch_textarea_keeps_jk_as_branch_text -- --nocapture
cargo test --test create_wizard_test description_shift_enter_inserts_newline_without_submitting -- --nocapture
```

Expected: PASS. This verifies `j` / `k` still go to text fields and `Alt` / `Shift` Enter behavior is not broken.

- [ ] **Step 7: Commit key handling**

```bash
git add src/tui_app/create_wizard.rs tests/create_wizard_test.rs
git commit -m "feat(create): add review and draft scroll keys"
```

---

### Task 4: Full Verification

**Files:**
- Verify: `src/tui_app/create_wizard.rs`
- Verify: `tests/create_wizard_test.rs`
- Verify: `docs/superpowers/specs/2026-07-02-create-wizard-review-draft-scroll-design.md`

- [ ] **Step 1: Format check**

Run:

```bash
cargo fmt --check
```

Expected: PASS. If it fails, run `cargo fmt`, inspect the diff, and re-run `cargo fmt --check`.

- [ ] **Step 2: Run create wizard test file**

Run:

```bash
cargo test --test create_wizard_test
```

Expected: PASS.

- [ ] **Step 3: Run all tests**

Run:

```bash
cargo test
```

Expected: PASS.

- [ ] **Step 4: Check patch hygiene**

Run:

```bash
git diff --check
```

Expected: PASS with no output.

- [ ] **Step 5: Review final diff**

Run:

```bash
git diff --stat HEAD~3..HEAD
git diff HEAD~3..HEAD -- src/tui_app/create_wizard.rs tests/create_wizard_test.rs
```

Expected: Diff is limited to `src/tui_app/create_wizard.rs` and `tests/create_wizard_test.rs`; no changes to `CreateDraft`, config files, shared mouse/event framework, or persistence.

- [ ] **Step 6: Commit any formatting-only follow-up if needed**

If `cargo fmt` changed files after Task 3, commit those formatting changes:

```bash
git add src/tui_app/create_wizard.rs tests/create_wizard_test.rs
git commit -m "style(create): format review scroll changes"
```

If `cargo fmt --check` already passed and there are no changes, skip this step.

---

## Self-Review

- Spec coverage: Task 2 implements UI-only scroll state and scrollable rendering for Review/Draft. Task 3 implements Review and Draft key bindings plus help text. Task 4 verifies no mouse/event framework changes and no data-model changes.
- Red-flag scan: The plan contains concrete paths, commands, expected results, and code snippets for each code-changing step.
- Type consistency: The plan uses existing names from the current checkout: `CreateWizardApp`, `CreateStep::Review`, `CreateWizardPage::Review`, `summary_lines`, `page_content_lines`, `KeyModifiers::ALT`, `Paragraph::scroll`, `render_to_string`, `key_mod`, and `CreateWizardOutcome::Submit`.
