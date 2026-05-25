# Review Fixes Design

## Context

Code review found four project-level issues:

- `done --strategy rebase` currently rebases the target branch onto the workspace branch, which can rewrite target history.
- `create --template <name>` loads the template but does not use its repo list.
- `hooks.file = "~/.config/..."` is documented and accepted by config parsing, but hook execution does not expand `~`.
- README documents commands and flags that the CLI does not expose.

This spec keeps the fix narrow. It corrects existing behavior and documentation without adding new product surface such as remote branch deletion or extra template commands.

## Goals

- Make `done --strategy rebase` safe and aligned with the intent of completing a workspace.
- Make `create --template <name>` actually create from the selected template.
- Make file hooks work with tilde-prefixed paths.
- Keep English and Chinese README command references consistent with the real CLI.
- Preserve existing behavior for explicit `--repos`, squash merge, normal merge, simple hooks, and inline hooks.

## Non-Goals

- Do not implement `done --delete-remote`.
- Do not implement `template show` or `template delete`.
- Do not redesign the workspace lifecycle or template file format.
- Do not change the default merge strategy.

## Design

### Rebase Strategy

`done --strategy rebase` will keep its CLI shape but change its command sequence.

Current unsafe sequence:

1. Checkout target branch.
2. Run `git rebase <workspace_branch>`.

New sequence:

1. Checkout the workspace branch.
2. Run `git rebase <target_branch>`.
3. Checkout the target branch.
4. Run `git merge --ff-only <workspace_branch>`.

This replays workspace commits on top of the current target branch, then advances target only if the result is a fast-forward. If rebase conflicts or fast-forward fails, the command returns an error. The workspace must not be cleaned or archived after that failure.

`squash` and `merge` keep their current behavior. `--push` remains after a successful merge only and still pushes the target branch.

### Template-Based Create

When `zootree create --template <name>` is used without `--repos`, `handle_create` will use the loaded template's `repos` list directly.

Implementation shape:

- Extract the existing repo-entry construction into a small helper that accepts a list of repo names plus optional per-repo branch overrides.
- Use that helper from both the explicit `--repos` path and the new template path.
- Keep the TUI selection path separate so template behavior can be tested without driving terminal prompts.

Rules:

- `--repos` remains the most explicit source. If both `--repos` and `--template` are passed, `--repos` wins.
- A template with an empty repo list is an error.
- A template that references an unregistered repo is an error from `load_repo_config`.
- Target branch resolution in the shared helper matches the current explicit `--repos` path:
  - use the repo-specific branch from input when present;
  - otherwise use `default_target_branch`;
  - otherwise read the current branch;
  - if current branch cannot be read, fall back to `main`.
- Interactive repo selection remains only for the case where neither `--repos` nor `--template` is provided.

This makes saved templates deterministic and non-interactive by default.

### Hook File Paths

`HookValue::File { file }` will expand `~` before execution.

Only file hook paths are changed:

- `File { file }`: pass `shellexpand::tilde(file)` to `sh`.
- `Simple(cmd)`: keep `sh -c <cmd>`.
- `Inline { inline }`: keep `sh -c <inline>`.

This matches examples that already use `~/.config/zootree/hooks/...` and avoids changing shell command semantics for simple and inline hooks.

### README Alignment

The command reference in both `README.md` and `README.zh-CN.md` will be adjusted to match the current CLI surface.

Changes:

- Remove `done --delete-remote`.
- Replace the template section with the implemented commands:
  - `zootree template list`
  - `zootree template save <name> --from <workspace>`

No new CLI commands are added in this scope.

## Error Handling

- Rebase conflicts and fast-forward failures propagate as command errors.
- Template loading errors propagate.
- Empty template repo lists return a direct user-facing error.
- Missing repo configs referenced by a template propagate from existing config loading.
- Hook execution failures keep current behavior; only path resolution changes.

## Tests

Add or update focused tests:

- `GitOps::merge` with `Some("rebase")` emits:
  1. `git checkout <workspace_branch>`
  2. `git rebase <target_branch>`
  3. `git checkout <target_branch>`
  4. `git merge --ff-only <workspace_branch>`
- The shared repo-entry helper resolves explicit branches, repo defaults, and current-branch fallback.
- The template path passes template repo names into that helper and errors on empty templates.
- Template with no repos errors.
- File hooks expand `~` before execution.
- README cleanup can be verified by searching for removed command references.

Run:

```sh
cargo test
cargo clippy --all-targets -- -D warnings
rg "delete-remote|template show|template delete" README.md README.zh-CN.md
```

The final `rg` should produce no matches.
