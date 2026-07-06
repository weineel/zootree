# Worktree existence checks and repo name collision design

## Context

`zootree` stores workspace state in config files, but commands that display or
operate on workspaces often assume the workspace worktree directories still
exist on disk. When a user removes a workspace directory manually, display
commands still show the configured path as if it were usable, and execution
commands can fail later inside git or zellij with errors that do not explain
which worktree is missing.

`zootree repo add` also derives the repo name from the path or `--name`. Today
that name maps directly to `<config>/repos/<name>.toml`, so a duplicate name can
overwrite the previous repo config. The create flow already has collision-safe
pending repo registration with `name-2`, `name-3`, etc. `repo add` should use the
same rule.

## Goals

- For commands that display in-progress workspace paths, show when repo
  worktree directories are missing.
- For commands that depend on repo worktree directories, fail before launching
  zellij or running git operations when required worktrees are missing.
- Keep pending and archived workspaces simple: only in-progress workspaces need
  worktree existence status.
- Make `zootree repo add` collision-safe by assigning a unique repo name instead
  of overwriting an existing repo config.
- Reuse one repo-name collision rule across `repo add` and create-flow pending
  repo registration.

## Non-goals

- Do not auto-recreate missing worktrees.
- Do not move workspace configs between statuses based on missing directories.
- Do not change `ConfigManager` into a runtime filesystem status provider.
- Do not add JSON output or new command flags.
- Do not expand `prune`; it already handles archived workspace directory
  deletion with an existence check.

## Worktree Status Model

Add a small runtime model near the workspace command code:

```rust
struct RepoWorktreeStatus {
    repo_name: String,
    worktree_path: String,
    exists: bool,
}
```

The helper should accept a `WorkspaceConfig` and expanded `workspace_dir`, then
return one item per `RepoEntry`. It only calls `Path::exists()`. It must not
write config, call git, or inspect repository state.

This keeps config loading and runtime filesystem checks separate. It also gives
formatters and command handlers one shared source for "which repo worktrees are
missing".

## Display Behavior

### `zootree list`

For in-progress workspaces, `list` should check every repo worktree path:

```text
<workspace_dir>/<repo_name>
```

Default card output keeps the existing fields and adds a compact missing marker
only when at least one repo worktree is absent:

```text
live-clay  [in_progress]  zootree/live-clay
  title: Fix worktree checks
  repos: zootree:main, docs:main
  dir:   /Users/lijufeng/zootree-workspaces/live-clay
  missing worktrees: docs
```

`--oneline` preserves its current shape and appends a short marker only when
needed:

```text
  live-clay (in_progress) - Fix worktree checks [zootree:main, docs:main] /Users/lijufeng/zootree-workspaces/live-clay [missing: docs]
```

Pending, done, and canceled workspaces do not show worktree existence markers in
`list`, because they are not expected to have active worktree directories.

### `zootree info`

`info` should keep the existing top-level `Dir` field and add per-repo worktree
status in the repo section for in-progress workspaces. A missing repo should be
obvious without changing the meaning of the repo target branch:

```text
Repos:
  - zootree         -> main  /Users/.../live-clay/zootree
  - docs            -> main  /Users/.../live-clay/docs (missing)
```

For non-in-progress workspaces, `info` can continue to show repo names and target
branches without worktree status.

## Execution Behavior

### `zootree open`

Before rendering or launching the zellij layout, `open` should check all repo
worktrees for the in-progress workspace. If any are missing, return a clear
error and do not write `recently.kdl` or invoke zellij:

```text
workspace 'live-clay' is missing worktrees: docs (/Users/.../live-clay/docs)
```

### `zootree done`

Before pre-done hooks, merge checks, uncommitted-change checks, or cleanup,
`done` should check all repo worktrees. If any are missing, return the same
clear error. `--force` should not bypass this check, because merge and
uncommitted-change behavior depends on actual worktree contents.

`--dry-run` should not fail on missing worktrees; it should remain a planning
view and include the configured operations only.

### `zootree cancel`

For an in-progress workspace, `cancel` should use the shared worktree status.

- Existing worktrees keep current behavior: check uncommitted changes, run
  `pre_remove` when configured, then call `git worktree remove`.
- Missing worktrees skip uncommitted-change checks, `pre_remove`, and
  `git worktree remove`.
- Missing worktrees should be reported with a warning line before archiving the
  workspace.

Pending workspace cancel behavior remains unchanged because pending workspaces do
not have worktrees.

## Repo Name Collision Behavior

Extract the existing unique repo name logic from create flow into a reusable
helper. The rule is:

- If `base` is unused, return `base`.
- If `base` exists, try `base-2`.
- If `base-2` exists, try `base-3`, and so on.

`zootree repo add` should call this helper after deriving the base name from
`--name` or the path basename. It should save the repo config under the resolved
unique name and print the actual name:

```text
repo 'zootree-2' registered at /Users/lijufeng/project/zootree
```

Create-flow pending repo registration should continue to use the same helper so
interactive create and explicit `repo add` cannot diverge.

## Testing

Add focused tests around pure helpers and formatters:

- Worktree status helper returns existing and missing repo paths.
- Worktree status helper reports all repos missing when the workspace directory
  is absent.
- `render_list_cards` includes `missing worktrees` for an in-progress workspace
  with missing repo worktrees.
- `render_list_oneline` appends `[missing: ...]` for missing repo worktrees.
- `render_once` in `info` marks missing in-progress repo worktrees.
- `repo add` resolves a duplicate base name to `base-2`.
- `repo add` resolves `base` plus existing `base-2` to `base-3`.
- Existing create-flow collision tests continue to pass after the helper move.

For command behavior that touches git or zellij, prefer small helper tests where
possible. If command-level coverage is added, keep it isolated with temporary
config directories and `MockRunner`.

## Acceptance Criteria

- `zootree list` still renders existing in-progress workspaces normally when all
  repo worktrees exist.
- `zootree list` and `zootree info` visibly mark missing in-progress repo
  worktrees.
- `zootree open <name>` fails before zellij launch when any repo worktree is
  missing.
- `zootree done <name>` fails before hooks or git operations when any repo
  worktree is missing, except `--dry-run`.
- `zootree cancel <name>` can archive an in-progress workspace even when some
  repo worktrees are already gone, while skipping per-repo cleanup for missing
  worktrees.
- `zootree repo add <path>` never overwrites an existing repo config because of a
  duplicate derived name.
- `zootree repo add --name <name> <path>` also uses the same unique-name suffix
  rule.
