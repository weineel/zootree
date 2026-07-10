# Review backlog workspace split design

## Context

A static review produced ten follow-up items for zootree. They span several
independent areas:

- config file name/path safety
- zellij KDL layout rendering
- workspace start failure recovery
- inline TUI selection rendering
- create wizard structure
- log configuration behavior
- terminal cleanup
- copy file semantics
- external command status handling
- workspace listing and completion stability

These should not be implemented as one large change. zootree already manages
isolated workspaces well, and development tasks can be created with
full non-interactive `zootree create` commands that include `--title`,
`--description`, repo source, and `--run-agent codexd`. Independent work should
run in separate zootree workspaces. Items with shared behavior or shared call
paths should be kept in the same workspace to avoid conflicting implementations.

## Goals

- Convert the review backlog into independently executable zootree workspaces.
- Use `cz-conventional-changelog` task titles for every workspace.
- Keep related review items together when they share the same runtime behavior.
- Make the first implementation batch small enough to run in parallel.
- Leave lower-priority structural refactoring out of the first bugfix batch.

## Non-goals

- Do not implement any review item in this design step.
- Do not create the zootree workspaces from this spec alone.
- Do not refactor `CreateWizardApp` while the higher-priority bugfix work is in
  progress.
- Do not change the review item priorities unless code exploration shows that a
  claimed issue is not real in the current checkout.

## Title Convention

Every zootree task title follows the `cz-conventional-changelog` shape:

```text
<type>(<scope>): <subject>
```

Use common Conventional Commit types such as `fix`, `feat`, `refactor`, `docs`,
`test`, and `chore`. This is an open source project, so the scope should be the
source module, command, or behavior boundary that a contributor can understand
from the repository. Use scopes such as `config`, `layout`, `tui`, `workspace`,
`log`, `repo`, `docs`, or `test`. Do not use internal project identifiers,
customer identifiers, organization-specific shorthand, or `other` as the scope.
If no meaningful scope exists, omit the scope.

Examples:

```text
fix(config): validate config-backed names
fix(layout): escape zellij kdl variables
refactor(tui): split create wizard app
```

## Workspace Breakdown

### 1. `fix(config): validate config-backed names`

Create command:

```bash
zootree create \
  --title "fix(config): validate config-backed names" \
  --name config-name-validation \
  --branch zootree/config-name-validation \
  --repos zootree:main \
  --description "$(cat <<'EOF'
Validate names that zootree uses to derive config-backed file paths.

Scope:
- Add shared slug validation for repo, workspace, and template names used to
  build config file paths.
- Apply that validation consistently in ConfigManager load/save/remove/move
  paths and in call sites that accept user-provided names.

Do not implement KDL variable escaping, start rollback, copy_files behavior, log
configuration, or workspace listing changes in this workspace.

Expected verification:
- cargo test --test config_test
- cargo fmt --check
- cargo clippy --all-targets -- -D warnings
EOF
)" \
  --run-agent codexd
```

Review items covered:

- Name/path safety validation.

This is the highest-priority first workspace because it protects the on-disk
config boundary and has narrow tests.

### 2. `fix(layout): escape zellij kdl variables`

Create command:

```bash
zootree create \
  --title "fix(layout): escape zellij kdl variables" \
  --name layout-kdl-escaping \
  --branch zootree/layout-kdl-escaping \
  --repos zootree:main \
  --description "$(cat <<'EOF'
Escape zellij KDL layout variables from the review backlog.

Scope:
- Escape ordinary zellij KDL string variables before replacing
  $repo_name, $worktree_path, $branch, $workspace_name, $workspace_dir, and
  $lazygit_config.
- Keep agent_cli KDL fragment substitution last and preserve existing agent_cli
  escaping behavior.

Do not implement config name validation, start rollback, copy_files behavior,
log configuration, or workspace listing changes in this workspace.

Expected verification:
- cargo test --test layout_test
- cargo test --test start_agent_test
- cargo fmt --check
EOF
)" \
  --run-agent codexd
```

Review items covered:

- Zellij KDL layout variable escaping.

This workspace can run in parallel with config name validation because it is
limited to layout rendering behavior.

### 3. `fix(tui): keep inline prompt cursor visible`

Create command:

```bash
zootree create \
  --title "fix(tui): keep inline prompt cursor visible" \
  --name inline-prompt-scroll \
  --branch zootree/inline-prompt-scroll \
  --repos zootree:main \
  --description "$(cat <<'EOF'
Fix inline select and multiselect rendering when there are more than eight
visible options.

Scope:
- Ensure the selected cursor row is always visible after moving beyond the
  first eight items.
- Apply the same scrolling behavior to SelectPromptState and
  MultiSelectPromptState rendering.
- Keep existing filter, fuzzy match highlighting, selection order, and keyboard
  behavior unchanged.
- Prefer pure state/rendering tests using the existing ratatui test approach.

Do not refactor CreateWizardApp in this workspace.

Expected verification:
- cargo test --test tui_app_test
- cargo test
EOF
)" \
  --run-agent codexd
```

Review items covered:

- Inline select/multiselect cursor visibility after the eighth item.

This workspace is independent of config, start, logging, and listing changes.

### 4. `fix(workspace): rollback failed start operations`

Create command:

```bash
zootree create \
  --title "fix(workspace): rollback failed start operations" \
  --name start-failure-rollback \
  --branch zootree/start-failure-rollback \
  --repos zootree:main \
  --description "$(cat <<'EOF'
Improve zootree start reliability and related failure semantics.

Scope:
- Add a start plan or rollback helper for handle_start so failures after
  creating directories, worktrees, copied files, or post_create hooks do not
  leave silent half-created state.
- Track created worktrees and remove them on failure where it is safe to do so.
- Make copy_files glob iteration errors explicit instead of silently dropping
  them.
- Define and implement copy_files directory semantics: either skip directories
  with a warning or support recursive copy. Keep the selected behavior explicit
  in tests.
- Distinguish "branch does not exist" from git command/repository failure in
  GitOps::branch_exists.
- Check zellij list-sessions exit status so zellij command failure is not
  treated as "session does not exist".

Do not change slug validation or KDL variable escaping here unless the
`fix(config): validate config-backed names` and `fix(layout): escape zellij kdl
variables` workspaces have already been merged and this branch is rebased on
them.

Expected verification:
- focused tests for rollback planning or helper behavior
- focused copy_files tests for glob errors and directory matches
- focused git/zellij command status tests
- cargo test
EOF
)" \
  --run-agent codexd
```

Review items covered:

- `handle_start` failure rollback.
- `copy_files` directory and glob error handling.
- External command status handling for git branch checks and zellij sessions.

This workspace is intentionally larger because all covered items affect startup
failure behavior. It should start after `fix(config): validate config-backed
names` is merged or rebased, because name/path validation can affect start
assumptions.

### 5. `fix(log): honor documented log configuration`

Create command:

```bash
zootree create \
  --title "fix(log): honor documented log configuration" \
  --name log-config-behavior \
  --branch zootree/log-config-behavior \
  --repos zootree:main \
  --description "$(cat <<'EOF'
Make zootree log configuration behavior match the documented config surface.

Scope:
- Decide the supported behavior for [log] fields currently exposed in
  GlobalConfig and README examples.
- At minimum, implement or remove documentation for fields that currently have
  no runtime effect.
- Prefer implementing configurable log directory if it can be done without
  changing CLI startup order.
- If max_files or max_size cannot be supported by the current tracing appender,
  document the limitation or remove those fields from public examples.
- Keep `zootree logs` aligned with the actual log path.

Expected verification:
- config parsing tests for supported log fields
- tests or small helpers for resolving the log path without writing to the real
  user config directory
- cargo test
EOF
)" \
  --run-agent codexd
```

Review items covered:

- `[log]` configuration currently not matching runtime behavior or README
  examples.

This workspace is independent and can run in the first parallel batch.

### 6. `refactor(workspace): stabilize workspace listing api`

Create command:

```bash
zootree create \
  --title "refactor(workspace): stabilize workspace listing api" \
  --name workspace-list-api \
  --branch zootree/workspace-list-api \
  --repos zootree:main \
  --description "$(cat <<'EOF'
Make workspace listing and completion stable and avoid redundant config reads.

Scope:
- Make ConfigManager workspace listing return stable ordering.
- Add or adjust an API that returns workspace status together with
  WorkspaceConfig.
- Use the status-aware API in completion so complete_workspace_with does not
  call load_workspace again for each candidate.
- Preserve existing list output format except for deterministic ordering.

Expected verification:
- config tests for stable ordering
- completion tests proving help text still includes status
- cargo test --test config_test
- cargo test --test completions_test
- cargo test
EOF
)" \
  --run-agent codexd
```

Review items covered:

- Workspace list ordering.
- Completion duplicate reads for workspace status.

This workspace can run in parallel with the first batch. It may conflict
slightly with `fix(config): validate config-backed names` in
`src/config/mod.rs`; merge order should be managed carefully.

### 7. `refactor(tui): split create wizard app`

Create command:

```bash
zootree create \
  --title "refactor(tui): split create wizard app" \
  --name create-wizard-refactor \
  --branch zootree/create-wizard-refactor \
  --repos zootree:main \
  --description "$(cat <<'EOF'
Refactor CreateWizardApp into smaller modules without changing behavior.

Scope:
- Split state, navigation, rendering, repo page behavior, and text field
  behavior into focused units.
- Preserve the current keyboard contract, layout contract, and persisted draft
  model.
- Keep all existing create_wizard tests meaningful after the split.
- Do not mix unrelated bugfixes into this refactor.

Expected verification:
- cargo test --test create_wizard_test
- cargo test
EOF
)" \
  --run-agent codexd
```

Review items covered:

- `CreateWizardApp` is too large and carries several responsibilities.

This is deliberately not in the first implementation batch. It should wait
until the bugfix work has landed so the refactor does not become the integration
point for unrelated changes.

## Review Item Mapping

| Review item | Workspace |
| --- | --- |
| Name/path safety validation | `fix(config): validate config-backed names` |
| Zellij KDL layout escaping | `fix(layout): escape zellij kdl variables` |
| `handle_start` rollback | `fix(workspace): rollback failed start operations` |
| Inline select/multiselect scroll | `fix(tui): keep inline prompt cursor visible` |
| `CreateWizardApp` split | `refactor(tui): split create wizard app` |
| `[log]` config behavior | `fix(log): honor documented log configuration` |
| TUI terminal cleanup RAII guard | deferred, unless it becomes necessary while touching TUI runtime code |
| `copy_files` errors and directories | `fix(workspace): rollback failed start operations` |
| External command status checks | `fix(workspace): rollback failed start operations` |
| Workspace list ordering and completion reads | `refactor(workspace): stabilize workspace listing api` |

The TUI terminal cleanup RAII guard is not assigned to the first batch. It is a
valid cleanup item, but it touches the TUI runtime session lifecycle and is less
connected to the high-priority bugfixes above. It should get a separate
workspace if it becomes a near-term priority.

## Recommended Execution Order

First parallel batch:

1. `fix(config): validate config-backed names`
2. `fix(layout): escape zellij kdl variables`
3. `fix(tui): keep inline prompt cursor visible`
4. `fix(log): honor documented log configuration`
5. `refactor(workspace): stabilize workspace listing api`

Second batch:

1. `fix(workspace): rollback failed start operations`

Start this after `fix(config): validate config-backed names` is merged or
rebased.

Later refactor:

1. `refactor(tui): split create wizard app`

Start this only after the first bugfix wave is complete.

Optional later cleanup:

1. `refactor(tui): add terminal session guard`

Create this only if terminal cleanup becomes a priority.

## Acceptance Criteria

- The review backlog is represented as clear zootree workspace tasks.
- Every task title uses the configured `cz-conventional-changelog` style.
- Independent tasks can be created with full non-interactive
  `zootree create ... --run-agent codexd` commands.
- Dependent review items are grouped into the same workspace.
- The first implementation batch can run in parallel without knowingly touching
  the same behavioral surface, except for manageable `ConfigManager` merge
  overlap between config safety and listing API work.
- Lower-priority refactoring is explicitly deferred.
