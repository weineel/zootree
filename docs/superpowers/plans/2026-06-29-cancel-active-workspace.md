# Cancel Active Workspace Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `zootree cancel` cancel active workspaces, where active means `pending` or `in_progress`.

**Architecture:** Keep the behavior inside `src/cli/workspace.rs`, where cancel is already implemented. Add private helpers for cancelable statuses and canceled-archive movement so status semantics are tested without terminal prompts or real zellij/git cleanup. Keep pending cancel as an early return before the in-progress cleanup path.

**Tech Stack:** Rust, clap derive, existing `ConfigManager`, existing `WorkspaceConfig` / `WorkspaceStatus`, Cargo unit tests.

---

## File Structure

- Modify `src/cli/workspace.rs`
  - Add private `CANCELABLE_STATUSES`.
  - Add private `cancel_candidate_statuses()`.
  - Add private `is_cancelable_status()`.
  - Add private `archive_canceled_workspace()`.
  - Update `handle_cancel()` to use active statuses for interactive selection and named-workspace validation.
  - Add unit tests in the existing `#[cfg(test)] mod tests`.
- Modify `README.md`
  - Document cancel as active-workspace cancellation.
  - Add a short definition of active if the command section has no nearby explanation.
- Modify `README.zh-CN.md`
  - Mirror the README update in Chinese.

No new public API or module is needed. The helper functions stay private because they are implementation details of the workspace CLI.

---

### Task 1: Add cancelable-status tests and helpers

**Files:**
- Modify: `src/cli/workspace.rs`

- [ ] **Step 1: Add failing tests for cancelable status semantics**

In `src/cli/workspace.rs`, inside the existing `#[cfg(test)] mod tests`, add these tests after `render_list_cards_shows_none_when_repos_empty`:

```rust
    #[test]
    fn cancel_candidate_statuses_are_pending_and_in_progress() {
        assert_eq!(
            cancel_candidate_statuses(),
            &[WorkspaceStatus::Pending, WorkspaceStatus::InProgress]
        );
    }

    #[test]
    fn is_cancelable_status_accepts_only_active_statuses() {
        assert!(is_cancelable_status(&WorkspaceStatus::Pending));
        assert!(is_cancelable_status(&WorkspaceStatus::InProgress));
        assert!(!is_cancelable_status(&WorkspaceStatus::Done));
        assert!(!is_cancelable_status(&WorkspaceStatus::Canceled));
    }
```

- [ ] **Step 2: Run the tests and verify they fail**

Run:

```bash
cargo test cancel_candidate_statuses_are_pending_and_in_progress
cargo test is_cancelable_status_accepts_only_active_statuses
```

Expected: both fail to compile with errors that `cancel_candidate_statuses` and `is_cancelable_status` are not found.

- [ ] **Step 3: Add the private helpers**

In `src/cli/workspace.rs`, insert these helpers after `render_list_cards` and before `handle_open`:

```rust
const CANCELABLE_STATUSES: &[WorkspaceStatus] =
    &[WorkspaceStatus::Pending, WorkspaceStatus::InProgress];

fn cancel_candidate_statuses() -> &'static [WorkspaceStatus] {
    CANCELABLE_STATUSES
}

fn is_cancelable_status(status: &WorkspaceStatus) -> bool {
    CANCELABLE_STATUSES.contains(status)
}
```

- [ ] **Step 4: Run the helper tests and verify they pass**

Run:

```bash
cargo test cancel_candidate_statuses_are_pending_and_in_progress
cargo test is_cancelable_status_accepts_only_active_statuses
```

Expected: both tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/cli/workspace.rs
git commit -m "test: define cancelable workspace statuses"
```

---

### Task 2: Add pending cancel archive behavior

**Files:**
- Modify: `src/cli/workspace.rs`

- [ ] **Step 1: Add failing archive and terminal-state tests**

In `src/cli/workspace.rs`, inside the existing `#[cfg(test)] mod tests`, add these tests after `is_cancelable_status_accepts_only_active_statuses`:

```rust
    fn test_workspace(name: &str) -> WorkspaceConfig {
        WorkspaceConfig {
            title: format!("{} title", name),
            name: name.into(),
            description: String::new(),
            branch: format!("zootree/{}", name),
            workspace_dir: format!("/tmp/{}", name),
            created_at: "2026-06-29T10:00:00+08:00".into(),
            agent_cli: None,
            zellij: ZellijConfig::default(),
            repos: Vec::new(),
            events: Vec::new(),
        }
    }

    #[test]
    fn archive_canceled_workspace_moves_pending_to_canceled_with_event() {
        let tmp = tempfile::tempdir().unwrap();
        let config_mgr = ConfigManager::with_base_dir(tmp.path().join("config"));
        config_mgr.ensure_dirs().unwrap();
        let mut workspace = test_workspace("pending-cancel");
        config_mgr
            .save_workspace(&WorkspaceStatus::Pending, &workspace)
            .unwrap();

        archive_canceled_workspace(&config_mgr, &WorkspaceStatus::Pending, &mut workspace).unwrap();

        assert!(
            !config_mgr
                .base_dir
                .join("workspaces/pending/pending-cancel.toml")
                .exists()
        );
        assert!(
            config_mgr
                .base_dir
                .join("workspaces/archived/canceled/pending-cancel.toml")
                .exists()
        );
        let (status, archived) = config_mgr.load_workspace("pending-cancel").unwrap();
        assert_eq!(status, WorkspaceStatus::Canceled);
        assert_eq!(
            archived.events.last().map(|event| event.action.as_str()),
            Some("canceled")
        );
    }

    #[test]
    fn terminal_statuses_are_rejected_before_cancel_archive() {
        for status in [WorkspaceStatus::Done, WorkspaceStatus::Canceled] {
            assert!(
                !is_cancelable_status(&status),
                "terminal status should not be cancelable: {:?}",
                status
            );
        }
    }
```

- [ ] **Step 2: Run the archive test and verify it fails**

Run:

```bash
cargo test archive_canceled_workspace_moves_pending_to_canceled_with_event
```

Expected: compile fails because `archive_canceled_workspace` does not exist.

- [ ] **Step 3: Add the archive helper**

In `src/cli/workspace.rs`, insert this helper after `is_cancelable_status`:

```rust
fn archive_canceled_workspace(
    config_mgr: &ConfigManager,
    from_status: &WorkspaceStatus,
    workspace: &mut WorkspaceConfig,
) -> Result<()> {
    let now = Local::now().to_rfc3339();
    workspace.events.push(Event {
        action: "canceled".into(),
        timestamp: now,
        detail: None,
    });
    config_mgr.save_workspace(from_status, workspace)?;
    config_mgr.move_workspace(&workspace.name, from_status, &WorkspaceStatus::Canceled)?;
    Ok(())
}
```

- [ ] **Step 4: Update `handle_cancel` to use active statuses and pending early return**

In `src/cli/workspace.rs`, replace the name-selection block at the start of `handle_cancel`:

```rust
    let name = match &args.name {
        Some(n) => n.clone(),
        None => {
            let in_progress = config_mgr.list_workspaces(Some(&[WorkspaceStatus::InProgress]))?;
            if in_progress.is_empty() {
                anyhow::bail!("no in_progress workspaces");
            }
            let names: Vec<String> = in_progress
                .iter()
                .map(|w| format!("{} - {}", w.name, w.title))
                .collect();
            let idx = tui::select_one("Select workspace to cancel", &names)?;
            in_progress[idx].name.clone()
        }
    };

    let (status, mut workspace) = config_mgr.load_workspace(&name)?;
    if !matches!(status, WorkspaceStatus::InProgress) {
        anyhow::bail!("workspace '{}' is not in_progress", name);
    }
```

with:

```rust
    let name = match &args.name {
        Some(n) => n.clone(),
        None => {
            let active = config_mgr.list_workspaces(Some(cancel_candidate_statuses()))?;
            if active.is_empty() {
                anyhow::bail!("no active workspaces");
            }
            let names: Vec<String> = active
                .iter()
                .map(|w| format!("{} - {}", w.name, w.title))
                .collect();
            let idx = tui::select_one("Select workspace to cancel", &names)?;
            active[idx].name.clone()
        }
    };

    let (status, mut workspace) = config_mgr.load_workspace(&name)?;
    if !is_cancelable_status(&status) {
        anyhow::bail!("workspace '{}' is not active", name);
    }

    if matches!(status, WorkspaceStatus::Pending) {
        archive_canceled_workspace(&config_mgr, &status, &mut workspace)?;
        println!("workspace '{}' canceled", name);
        return Ok(());
    }
```

- [ ] **Step 5: Replace the hard-coded in-progress archive block**

Near the end of `handle_cancel`, replace:

```rust
    // Archive
    let now = Local::now().to_rfc3339();
    workspace.events.push(Event {
        action: "canceled".into(),
        timestamp: now,
        detail: None,
    });
    config_mgr.save_workspace(&WorkspaceStatus::InProgress, &workspace)?;
    config_mgr.move_workspace(
        &name,
        &WorkspaceStatus::InProgress,
        &WorkspaceStatus::Canceled,
    )?;
```

with:

```rust
    // Archive
    archive_canceled_workspace(&config_mgr, &status, &mut workspace)?;
```

- [ ] **Step 6: Run the cancel-focused tests**

Run:

```bash
cargo test cancel
```

Expected: all matching tests pass, including the new archive and status tests.

- [ ] **Step 7: Commit**

```bash
git add src/cli/workspace.rs
git commit -m "feat: allow canceling active workspaces"
```

---

### Task 3: Update README docs and run final verification

**Files:**
- Modify: `README.md`
- Modify: `README.zh-CN.md`

- [ ] **Step 1: Update English README cancel wording**

In `README.md`, replace the command summary line:

```markdown
zootree cancel [name]                # Cancel a workspace
```

with:

```markdown
zootree cancel [name]                # Cancel an active workspace (pending or in-progress)
```

- [ ] **Step 2: Update Chinese README cancel wording**

In `README.zh-CN.md`, replace the command summary line:

```markdown
zootree cancel [name]                # 取消工作空间
```

with:

```markdown
zootree cancel [name]                # 取消 active 工作空间（pending 或 in-progress）
```

- [ ] **Step 3: Run targeted verification**

Run:

```bash
cargo test cancel
cargo test --test completions_test workspace_completer_filters_active
cargo check
```

Expected:

- `cargo test cancel` passes.
- `cargo test --test completions_test workspace_completer_filters_active` passes.
- `cargo check` finishes successfully.

- [ ] **Step 4: Check docs diff**

Run:

```bash
git diff -- README.md README.zh-CN.md src/cli/workspace.rs
```

Expected:

- `handle_cancel` now uses active statuses for interactive candidates.
- pending cancel archives immediately before cleanup/hook logic.
- in-progress cancel keeps cleanup/hook behavior.
- README files describe active workspace cancellation.

- [ ] **Step 5: Commit**

```bash
git add README.md README.zh-CN.md
git commit -m "docs: describe cancel active workspace"
```

---

## Final Verification

After all tasks are complete, run:

```bash
git status --short
git log --oneline -n 5
cargo test cancel
cargo test --test completions_test workspace_completer_filters_active
cargo check
```

Expected final state:

- working tree is clean
- recent commits include the implementation and docs commits
- all verification commands pass

## Self-Review Notes

- Spec coverage: Task 1 covers the single source of truth for active statuses. Task 2 covers pending archive, terminal rejection, interactive active candidates, and preserving in-progress cleanup. Task 3 covers README updates and verification.
- No public API is introduced.
- The plan intentionally tests pending archive through a private helper instead of invoking `handle_cancel` directly, because `handle_cancel` constructs `ConfigManager::new()` and real command runners.
