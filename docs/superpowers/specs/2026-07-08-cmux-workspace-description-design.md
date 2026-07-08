# cmux workspace description design

## Context

zootree can launch workspaces with cmux. The current cmux path creates a workspace with a stable cmux name:

```text
cmux workspace create --name zootree-<workspace_name> --cwd <workspace_dir> --layout <json> --focus true
```

This stable name is used for lookup and close fallback. cmux also supports a separate workspace description field through `new-workspace --description <text>` / `workspace create --description <text>`. zootree should use that field to show the zootree workspace title in cmux while keeping the stable `zootree-<name>` identity unchanged.

cmux also supports collapsible workspace groups through `cmux workspace-group` and supports adding a newly created workspace to a group with `new-workspace --group <id|ref>`. Group support is out of scope for this change.

## Goals

- When zootree creates a cmux workspace, set the cmux description to `WorkspaceConfig.title`.
- Preserve the existing cmux workspace name format `zootree-<workspace.name>`.
- Keep cmux lookup, select, and close behavior based on the stable name or persisted workspace ref.
- Keep zellij behavior unchanged.

## Non-Goals

- Do not introduce cmux workspace group configuration.
- Do not rename existing cmux workspaces on `open`.
- Do not update the cmux description when selecting an already-existing workspace.
- Do not change the zellij layout/session behavior.

## Design

Add a `description: String` field to `MultiplexerLaunch`.

Both launch preparation paths set it from the zootree workspace title:

```rust
description: workspace.title.clone()
```

`CmuxMultiplexer::launch_and_capture_workspace` adds this field to the create command:

```text
cmux workspace create \
  --name zootree-<workspace_name> \
  --description <workspace.title> \
  --cwd <workspace_dir> \
  --layout <json> \
  --focus true
```

The name remains the stable zootree identity. The description is display metadata for cmux.

`open` keeps its current behavior:

- If `identity.cmux_workspace` is present and `cmux workspace select <ref>` succeeds, return `Attached` and do not modify description.
- If selecting the persisted ref fails, recreate the workspace with the current launch data, including `--description <workspace.title>`.
- If no persisted ref exists, create the workspace with the current launch data.

## Error Handling

No new error branch is needed. `--description` is part of the cmux create command. If cmux rejects the argument or creation fails, the existing `cmux workspace create` error path surfaces stderr.

Empty titles should not occur for valid zootree workspaces, but the code can pass an empty string without special handling.

## Testing

Update `tests/cmux_test.rs`:

- `launch_invokes_cmux_new_workspace` should assert that the create command includes `--description <title>`.
- `launch_or_open_recreates_when_persisted_workspace_ref_cannot_be_selected` should assert that the recreate command includes `--description <title>`.

The test helper that builds `MultiplexerLaunch` should include a non-name title value so the assertion proves the description comes from title metadata, not from `zootree-<name>`.

No integration test with real cmux is required; the project already tests external commands through `MockRunner`.

## Acceptance Criteria

- cmux workspace creation receives `--description` with the zootree workspace title.
- Existing cmux `--name zootree-<workspace.name>` behavior is unchanged.
- Existing persisted-ref `open` path still only selects the workspace.
- Recreate-after-missing-ref path creates with the description.
- Targeted cmux tests pass.
