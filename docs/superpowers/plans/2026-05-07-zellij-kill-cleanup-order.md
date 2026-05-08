# Zellij Kill Cleanup Order Fix Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the bug where `zootree done/cancel` run from inside a zellij pane leave the workspace in `in_progress` state because `kill_session` terminates the process before archive steps run.

**Architecture:** Two-part fix — (1) simplify `ZellijOps::kill_session` to a single `delete-session --force` call; (2) reorder operations in `handle_done` and `handle_cancel` so archive (save + move) happens before `kill_session`. No new abstractions needed.

**Tech Stack:** Rust, `anyhow`, `MockRunner` for unit tests, `std::os::unix::process::ExitStatusExt` for constructing test outputs.

---

## File Map

| File | Change |
|------|--------|
| `src/core/zellij.rs` | Simplify `kill_session` body |
| `src/cli/workspace.rs` | Reorder archive block before `kill_session` in `handle_done` and `handle_cancel` |
| `tests/zellij_test.rs` | New file — unit tests for `ZellijOps::kill_session` |

---

### Task 1: Simplify `ZellijOps::kill_session`

**Files:**
- Modify: `src/core/zellij.rs:58-64`
- Test: `tests/zellij_test.rs` (new)

- [ ] **Step 1: Write the failing test**

Create `tests/zellij_test.rs`:

```rust
use zootree::core::zellij::ZellijOps;
use zootree::runner::MockRunner;
use std::os::unix::process::ExitStatusExt;
use std::process::{ExitStatus, Output};

fn success_output() -> Output {
    Output {
        status: ExitStatus::from_raw(0),
        stdout: Vec::new(),
        stderr: Vec::new(),
    }
}

#[test]
fn test_kill_session_calls_delete_force_only() {
    let runner = MockRunner::new();
    runner.push_response(success_output()); // one response for delete-session --force
    let zellij = ZellijOps::new(&runner);

    zellij.kill_session("zootree-test-ws").unwrap();

    let calls = runner.take_calls();
    assert_eq!(calls.len(), 1, "expected exactly one zellij call");
    assert_eq!(calls[0].program, "zellij");
    assert_eq!(
        calls[0].args,
        vec!["delete-session", "--force", "zootree-test-ws"]
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test test_kill_session_calls_delete_force_only 2>&1
```

Expected: FAIL — current implementation makes 2 calls (`kill-session` + `delete-session`), so `assert_eq!(calls.len(), 1)` fails.

- [ ] **Step 3: Simplify `kill_session` in `src/core/zellij.rs`**

Replace lines 58–64:

```rust
pub fn kill_session(&self, session_name: &str) -> Result<()> {
    info!("killing zellij session: {}", session_name);
    let _ = self.zellij(vec!["delete-session".into(), "--force".into(), session_name.into()]);
    Ok(())
}
```

- [ ] **Step 4: Run test to verify it passes**

```bash
cargo test test_kill_session_calls_delete_force_only 2>&1
```

Expected: PASS.

- [ ] **Step 5: Run full test suite**

```bash
cargo test 2>&1
```

Expected: all existing tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/core/zellij.rs tests/zellij_test.rs
git commit -m "fix: simplify kill_session to single delete-session --force call"
```

---

### Task 2: Reorder archive before kill in `handle_done`

**Files:**
- Modify: `src/cli/workspace.rs:634-656`

The current order in `handle_done` (lines 634–656):

```rust
// Kill zellij session          ← line 634
let session_name = ...;
if let Some(sn) = &session_name {
    if let Err(e) = zellij.kill_session(sn) {
        tracing::warn!(...);
    }
}

// Archive                       ← line 645
let now = Local::now().to_rfc3339();
workspace.events.push(Event { action: "done".into(), timestamp: now, detail: None });
config_mgr.save_workspace(&WorkspaceStatus::InProgress, &workspace)?;
config_mgr.move_workspace(&name, &WorkspaceStatus::InProgress, &WorkspaceStatus::Done)?;

println!("workspace '{}' completed", name);
```

- [ ] **Step 1: Move the archive block before the kill block**

Edit `src/cli/workspace.rs` so the section after the clean loop reads:

```rust
    // Archive
    let now = Local::now().to_rfc3339();
    workspace.events.push(Event {
        action: "done".into(),
        timestamp: now,
        detail: None,
    });
    config_mgr.save_workspace(&WorkspaceStatus::InProgress, &workspace)?;
    config_mgr.move_workspace(&name, &WorkspaceStatus::InProgress, &WorkspaceStatus::Done)?;

    // Kill zellij session
    let session_name = match workspace.zellij.session_mode.as_deref() {
        Some("shared") => workspace.zellij.session_name.clone(),
        _ => Some(format!("zootree-{}", workspace.name)),
    };
    if let Some(sn) = &session_name {
        if let Err(e) = zellij.kill_session(sn) {
            tracing::warn!("failed to kill zellij session '{}': {}", sn, e);
        }
    }

    println!("workspace '{}' completed", name);
    Ok(())
```

- [ ] **Step 2: Build to check for compile errors**

```bash
cargo build 2>&1
```

Expected: no errors.

- [ ] **Step 3: Run test suite**

```bash
cargo test 2>&1
```

Expected: all tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/cli/workspace.rs
git commit -m "fix: archive workspace state before killing zellij session in handle_done"
```

---

### Task 3: Reorder archive before kill in `handle_cancel`

**Files:**
- Modify: `src/cli/workspace.rs:751-773`

The current order in `handle_cancel` (lines 751–773):

```rust
// Kill zellij session          ← line 751
let session_name = ...;
if let Some(sn) = &session_name {
    if let Err(e) = zellij.kill_session(sn) {
        tracing::warn!(...);
    }
}

// Archive                       ← line 762
let now = Local::now().to_rfc3339();
workspace.events.push(Event { action: "canceled".into(), timestamp: now, detail: None });
config_mgr.save_workspace(&WorkspaceStatus::InProgress, &workspace)?;
config_mgr.move_workspace(&name, &WorkspaceStatus::InProgress, &WorkspaceStatus::Canceled)?;

println!("workspace '{}' canceled", name);
```

- [ ] **Step 1: Move the archive block before the kill block**

Edit `src/cli/workspace.rs` so the section after the clean block reads:

```rust
    // Archive
    let now = Local::now().to_rfc3339();
    workspace.events.push(Event {
        action: "canceled".into(),
        timestamp: now,
        detail: None,
    });
    config_mgr.save_workspace(&WorkspaceStatus::InProgress, &workspace)?;
    config_mgr.move_workspace(&name, &WorkspaceStatus::InProgress, &WorkspaceStatus::Canceled)?;

    // Kill zellij session
    let session_name = match workspace.zellij.session_mode.as_deref() {
        Some("shared") => workspace.zellij.session_name.clone(),
        _ => Some(format!("zootree-{}", workspace.name)),
    };
    if let Some(sn) = &session_name {
        if let Err(e) = zellij.kill_session(sn) {
            tracing::warn!("failed to kill zellij session '{}': {}", sn, e);
        }
    }

    println!("workspace '{}' canceled", name);
    Ok(())
```

- [ ] **Step 2: Build to check for compile errors**

```bash
cargo build 2>&1
```

Expected: no errors.

- [ ] **Step 3: Run full test suite**

```bash
cargo test 2>&1
```

Expected: all tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/cli/workspace.rs
git commit -m "fix: archive workspace state before killing zellij session in handle_cancel"
```
