# Design: Fix Cleanup Order After Zellij Session Kill

Date: 2026-05-07

## Problem

When `zootree done` or `zootree cancel` is run from inside a zellij pane, the call to `kill_session` terminates the current pane's process. The archive steps (`save_workspace` + `move_workspace`) that follow never execute, leaving the workspace in `in_progress` state permanently.

Affected code:
- `src/cli/workspace.rs` `handle_done` (lines 634–653): kill before archive
- `src/cli/workspace.rs` `handle_cancel` (lines 751–770): kill before archive
- `src/core/zellij.rs` `kill_session` (lines 58–64): two-step kill+delete

## Fix

### 1. Reorder operations in `handle_done` and `handle_cancel`

Change the execution order from:

```
git ops → clean worktrees → kill_session → archive (save + move)
```

to:

```
git ops → clean worktrees → archive (save + move) → kill_session
```

This ensures the workspace state is persisted before the process is potentially killed by the zellij kill. Works correctly for both cases:
- **Inside the session**: archive completes, then `kill_session` kills the pane (process dies, but state is already saved)
- **Outside the session**: archive completes, then `kill_session` kills the remote session (process continues normally)

### 2. Simplify `kill_session` in `ZellijOps`

Replace the two-step `kill-session` + `delete-session --force` with a single `delete-session --force`:

```rust
// before
let _ = self.zellij(vec!["kill-session".into(), session_name.into()]);
self.zellij(vec!["delete-session".into(), "--force".into(), session_name.into()])?;

// after
let _ = self.zellij(vec!["delete-session".into(), "--force".into(), session_name.into()]);
```

`zellij delete-session --force` already means "kill if running, then delete". The separate `kill-session` step is redundant. The return value uses `let _` to ignore errors, consistent with how callers already handle `kill_session` failures (warn log, don't abort).

## Files Changed

| File | Change |
|------|--------|
| `src/cli/workspace.rs` | Move archive block before `kill_session` call in `handle_done` and `handle_cancel` |
| `src/core/zellij.rs` | Simplify `kill_session` to single `delete-session --force` call |

## Testing

- Add/update unit tests in `tests/` for `handle_done` and `handle_cancel` using `MockRunner` to verify archive steps are called before `kill-session`/`delete-session` commands
- Manually verify: run `zootree done` from inside a zellij pane and confirm workspace moves to `done` status
