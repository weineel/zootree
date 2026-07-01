# Cancel Active Workspace Design

## Goal

`zootree cancel` should cancel any active workspace, not only workspaces already in progress.

Active means a workspace that has not reached a terminal archive state. In the current status model, active is:

- `pending`
- `in_progress`

Terminal states remain:

- `done`
- `canceled`

`done` and `canceled` workspaces must not be canceled again.

## Current Behavior

The CLI already hints that cancel is active-state aware:

- `CancelArgs.name` completion uses `WorkspaceFilter::Active`.
- README completion docs say `zootree cancel <TAB>` lists pending or in-progress workspaces.

The implementation is narrower:

- Interactive `zootree cancel` only lists `in_progress`.
- Named `zootree cancel <name>` rejects `pending` with `workspace '<name>' is not in_progress`.
- The archive step always moves from `in_progress` to `canceled`.

This makes pending workspace completion/documentation misleading and prevents canceling a workspace that has been created but not started.

## Design

### Status Semantics

Add a single source of truth for cancelable statuses in the workspace CLI layer. It can be a helper such as:

```rust
fn is_active_status(status: &WorkspaceStatus) -> bool
```

or a helper that returns the status slice used by cancel. The important contract is that `Pending | InProgress` are accepted and `Done | Canceled` are rejected.

For named cancel, load the workspace and reject non-active statuses with:

```text
workspace '<name>' is not active
```

For interactive cancel, list pending and in-progress workspaces. This should align with the existing `WorkspaceFilter::Active` completion behavior.

### Pending Cancel Flow

When canceling a pending workspace:

1. Load the pending workspace config.
2. Append an event with `action = "canceled"`.
3. Save the updated config in the pending location.
4. Move it from `workspaces/pending` to `workspaces/archived/canceled`.
5. Print `workspace '<name>' canceled`.

Pending cancel must not run worktree/session cleanup:

- no uncommitted-change checks
- no `pre_cancel`
- no `pre_remove`
- no `git worktree remove`
- no workspace directory removal
- no zellij session kill

The workspace has not been started, so these operations either have nothing to operate on or rely on in-progress runtime assumptions.

### In-Progress Cancel Flow

Canceling an in-progress workspace keeps the current behavior:

1. Confirm or force through uncommitted changes.
2. Run `pre_cancel` unless skipped.
3. Run per-repo `pre_remove` unless skipped.
4. Remove worktrees and workspace directory unless `--no-clean`.
5. Append `canceled` event.
6. Move the config from `in_progress` to `archived/canceled`.
7. Kill the zellij session if present.
8. Print `workspace '<name>' canceled`.

The archive step should use the loaded source status rather than a hard-coded `InProgress` where practical, while still only allowing the active statuses above.

## Error Handling

- Missing workspace keeps the existing `workspace '<name>' not found` behavior from `ConfigManager::load_workspace`.
- `done` or already `canceled` workspaces fail before hooks or cleanup with `workspace '<name>' is not active`.
- Pending cancel ignores `--no-clean`, `--force`, and `--skip-hooks` because there is no runtime cleanup path to affect.
- In-progress cancel preserves existing `--no-clean`, `--force`, and `--skip-hooks` behavior.

## Tests

Add targeted tests around the cancel state boundary.

1. Pending cancel archives the workspace:
   - create a pending workspace in an isolated temp config
   - run cancel
   - assert `workspaces/pending/<name>.toml` is gone
   - assert `workspaces/archived/canceled/<name>.toml` exists
   - assert the archived config contains a `canceled` event

2. Terminal workspaces cannot be canceled:
   - save a `done` workspace and assert cancel fails with `not active`
   - save a `canceled` workspace and assert cancel fails with `not active`

3. Interactive candidates use active statuses:
   - prefer extracting the candidate-status selection into a small helper and test it returns pending plus in-progress
   - keep the existing completion active test as the shell-completion coverage

## Documentation

Update README and README.zh-CN command docs:

- English: `zootree cancel [name] # Cancel an active workspace`
- Chinese: `zootree cancel [name] # 取消 active 工作空间`

Where status terms are explained, document `active = pending or in-progress`.

## Verification

Expected verification after implementation:

```bash
cargo test cancel
cargo test --test completions_test workspace_completer_filters_active
cargo check
```

If the final implementation touches broader CLI helpers, run the relevant full test file instead of only name-filtered tests.
