# cmux group workspaces design

## Context

zootree currently supports cmux as a terminal multiplexer through a one-to-one model: one zootree workspace creates one cmux workspace. That is enough for a single rendered layout, but it does not match cmux's workspace group workflow for multi-repo development.

cmux groups are represented by an anchor workspace. The group header in the sidebar is the anchor workspace's sidebar representation, and `cmux workspace-group delete <group>` deletes the group and closes every workspace inside it. This fits zootree's lifecycle: one zootree workspace should own one cmux group, and `zootree done` / `zootree cancel` should delete that whole group.

The new design changes only cmux behavior. zellij remains unchanged.

## Goals

- Represent every cmux-backed zootree workspace as one cmux workspace group.
- Use the zootree workspace title as the cmux group name.
- Create one anchor cmux workspace for group-level context and one cmux workspace per repo.
- Put `zootree info <workspace> --watch` in the anchor workspace.
- Keep repo workspaces focused on repo-local work: lazygit plus shells.
- Preserve existing `--run-agent` semantics:
  - Single repo: agent runs in the repo workspace.
  - Multiple repos: agent runs in the anchor workspace.
- Delete the cmux group when `zootree done` or `zootree cancel` archives the zootree workspace.
- Persist cmux group and workspace refs so open and cleanup are deterministic.

## Non-Goals

- Do not change zellij session behavior.
- Do not introduce a broad cmux multi-template configuration model in this change.
- Do not preserve the old single-cmux-workspace launch path for non-default cmux layouts.
- Do not migrate zellij KDL layouts to cmux layouts.
- Do not run an agent in every repo workspace.
- Do not create an extra overview workspace beyond the required anchor workspace.
- Do not make `--no-clean` preserve the cmux group; it should continue to affect git worktrees and workspace directory cleanup only.

## Architecture

The cmux model changes from:

```text
zootree workspace -> cmux workspace
```

to:

```text
zootree workspace -> cmux workspace group
                   -> anchor cmux workspace
                   -> repo cmux workspace for repo[0]
                   -> repo cmux workspace for repo[1]
                   -> ...
```

The anchor workspace owns the group header. When a user selects the group header in cmux, they see the anchor workspace layout. Repo workspaces remain separate selectable workspaces inside the same group.

For three repos, zootree creates one cmux group with four cmux workspaces: one anchor plus three repo workspaces. For one repo, zootree creates one cmux group with two cmux workspaces: one anchor plus one repo workspace.

`src/cli/workspace.rs` should keep the existing high-level lifecycle boundary:

- `launch_multiplexer(...)`
- `close_multiplexer(...)`
- `prepare_multiplexer_launch(...)`

The cmux implementation can add group-aware helpers under `src/core/multiplexer/cmux.rs`, but cmux command sequencing should not be scattered through create/start/open/done/cancel handlers.

## Lifecycle

### Start

For cmux-backed workspaces, `zootree start` should:

1. Create worktrees and run existing hooks as it does today.
2. Build cmux launch data for the anchor workspace and each repo workspace.
3. Create the anchor workspace.
4. Create a workspace group named with `WorkspaceConfig.title`, using the anchor workspace.
5. Create each repo workspace and add it to the group.
6. Persist cmux refs in `workspace.multiplexer_state` only after the full group has been created successfully.

If any cmux creation step fails, `start` should return an error and should not save partial cmux state. zootree does not need complex rollback in this change; partially created cmux objects can be cleaned manually from cmux.

### Open

`zootree open` should prefer existing cmux state:

1. If `cmux_group` exists, focus/select the group.
2. If that fails, or `cmux_group` is missing, look up a group by exact title match.
3. If exactly one group matches, focus it and update persisted state when enough refs are available.
4. If no group matches, or multiple groups match, recreate the whole group.

The fallback match must be exact and unique. zootree should not select or delete the first fuzzy match.

### Done / Cancel

`zootree done` and `zootree cancel` should keep the existing archive and git cleanup behavior. After archiving:

1. Prefer `cmux workspace-group delete <cmux_group>`.
2. If no persisted group ref exists, look up by exact workspace title.
3. Delete only if exactly one group matches.
4. If lookup has no match or multiple matches, warn and skip cmux deletion.

cmux close/delete failures should follow current multiplexer cleanup behavior: warn, but do not roll back workspace archival.

## Layout

This change reworks the built-in default cmux layout. Non-default cmux layouts are not compatible with the group model yet, because the old template format describes one cmux workspace containing every repo. In group mode, zootree needs separate anchor and repo workspace layouts.

For this change, cmux `layout = "default"` is the only supported group-aware layout. If a workspace selects a non-default cmux layout, zootree should return a clear unsupported-layout error instead of silently falling back to the old single-workspace path. A future change can introduce first-class multi-template cmux configuration.

### Anchor Workspace Layout

Anchor workspace layout:

```text
horizontal split
├── left: zootree info <workspace_name> --watch
└── right:
    ├── multi repo + --run-agent: agent command
    └── otherwise: shell
```

The anchor workspace cwd is the zootree workspace directory. The `zootree info` terminal also runs from the zootree workspace directory.

### Repo Workspace Layout

Repo workspace layout:

```text
horizontal split
├── left: lazygit -p <worktree_path> [-ucf <lazygit_config>]
└── right vertical split
    ├── top: shell in <worktree_path>
    └── bottom:
        ├── single repo + --run-agent: agent command in <worktree_path>
        └── otherwise: shell in <worktree_path>
```

Repo workspaces do not run `zootree info`. Multi-repo agent runs only in the anchor workspace. Single-repo agent runs in that repo workspace's bottom-right terminal.

## State Model

`WorkspaceConfig.multiplexer_state` should become group-aware:

```toml
[multiplexer_state]
kind = "cmux"
cmux_group = "workspace_group:1"
cmux_anchor_workspace = "workspace:4"

[[multiplexer_state.cmux_repo_workspaces]]
repo = "frontend"
workspace = "workspace:5"

[[multiplexer_state.cmux_repo_workspaces]]
repo = "backend"
workspace = "workspace:6"
```

Recommended Rust shape:

```rust
pub struct MultiplexerState {
    pub kind: Option<MultiplexerKind>,
    pub cmux_workspace: Option<String>,
    pub cmux_group: Option<String>,
    pub cmux_anchor_workspace: Option<String>,
    pub cmux_repo_workspaces: Vec<CmuxRepoWorkspaceState>,
}

pub struct CmuxRepoWorkspaceState {
    pub repo: String,
    pub workspace: String,
}
```

`cmux_workspace` stays readable for compatibility with existing config files from the previous cmux implementation. New saves should not write `cmux_workspace` once group-aware state is available.

If old state has only `cmux_workspace`, `open` should try title-based group lookup first. If lookup fails, it should recreate the new group structure.

## cmux Commands

Use the canonical `cmux workspace ...` commands where practical:

- `cmux workspace create --name <name> --description <description> --cwd <cwd> --layout <json> --focus true`
- `cmux workspace select <workspace>`
- `cmux workspace close <workspace>`

Use group commands for group lifecycle:

- `cmux workspace-group create --name <workspace_title> --from <anchor_workspace>`
- `cmux workspace-group add --group <group> --workspace <repo_workspace>`
- `cmux workspace-group focus <group>`
- `cmux workspace-group list --json`
- `cmux workspace-group delete <group>`

If the exact JSON output shape from `workspace-group list --json` is inconvenient, parsing should be isolated inside `CmuxMultiplexer` so CLI lifecycle code remains independent of cmux output details.

## Error Handling

- cmux creation errors should surface stderr with context, matching the existing cmux command wrapper style.
- Non-default cmux layouts should fail early with a clear message that group-aware cmux currently supports only `layout = "default"`.
- Group lookup by title must require exactly one match.
- Duplicate group titles should warn and skip destructive operations.
- `done` / `cancel` should not fail solely because group deletion fails.
- `open` may recreate the group if persisted refs are stale or missing.
- `--run-agent` command parsing errors should continue to use the existing agent CLI parsing error path.

## Testing

### cmux command tests

Extend `tests/cmux_test.rs` to cover:

- Creating an anchor workspace.
- Creating a workspace group from the anchor workspace.
- Creating repo workspaces.
- Adding repo workspaces to the group.
- Returning a complete captured state: group ref, anchor workspace ref, repo workspace refs.
- Focusing a persisted group on open.
- Recreating the group if persisted focus fails.
- Deleting a persisted group on close.
- Falling back to exact unique title lookup when group ref is missing.
- Skipping deletion when title lookup has zero or multiple matches.

### cmux layout tests

Extend `tests/cmux_layout_test.rs` or split focused helpers to cover:

- Anchor layout contains `zootree info <workspace> --watch`.
- Multi-repo `--run-agent` appears in the anchor layout.
- Multi-repo repo layouts do not contain the agent command.
- Single-repo `--run-agent` appears in the repo layout.
- Repo layout launches `lazygit -p <worktree_path>`.
- Repo-specific lazygit config adds `-ucf <config>`.
- Rendered JSON is valid and has no unresolved variables or empty command fields.
- Non-default cmux layout selection returns a clear unsupported-layout error.

### config tests

Extend `tests/config_test.rs` to cover:

- New group-aware `multiplexer_state` TOML parsing.
- Round-trip serialization of `cmux_group`, `cmux_anchor_workspace`, and repo workspace refs.
- Empty group-aware state is skipped.
- Old `cmux_workspace` still parses.
- New serialization does not write `cmux_workspace` when only group-aware state is set.

### workspace lifecycle tests

Keep tests mostly helper-level:

- `start` persists group-aware state only after successful cmux launch.
- `open` uses group state before falling back.
- `done` / `cancel` route through generic multiplexer close.
- zellij behavior remains unchanged.

## Documentation

Update:

- `README.md`
- `README.zh-CN.md`
- `skills/zootree-usage/SKILL.md`

If implementation changes module structure, lifecycle conventions, or state schema, update:

- `skills/zootree-dev/SKILL.md`

## Acceptance Criteria

- cmux-backed zootree workspaces create one cmux group per zootree workspace.
- The cmux group name is the zootree workspace title.
- The group contains one anchor workspace and one workspace per repo.
- Anchor layout shows `zootree info` on the left.
- Multi-repo `--run-agent` runs in the anchor workspace.
- Single-repo `--run-agent` runs in the repo workspace.
- Repo workspaces run lazygit on the left and shells on the right.
- Non-default cmux layouts fail clearly until group-aware multi-template configuration exists.
- `zootree open` can focus an existing group from persisted state.
- `zootree done` and `zootree cancel` delete the cmux group when it can be identified safely.
- Tests cover command sequencing, layout rendering, config serialization, and lifecycle integration.
