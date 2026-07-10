# Review Backlog Workspace Split Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Create and verify zootree workspaces that delegate the review backlog into independent, Conventional Commit-scoped development tasks.

**Architecture:** This is an orchestration plan, not a code implementation plan for the review fixes themselves. It uses the committed design spec as the source of truth, verifies the current repo registration and workspace state, then creates zootree workspaces with full non-interactive `zootree create ... --run-agent codexd` commands. Code-level implementation planning happens inside each generated workspace.

**Tech Stack:** zootree CLI, zootree registered repo config, git, shell, Rust project conventions documented in `skills/zootree-dev/SKILL.md`.

## Global Constraints

- Always reply in Chinese for this repository.
- This project is open source; task titles use `cz-conventional-changelog` with module or behavior scopes such as `config`, `layout`, `tui`, `workspace`, and `log`.
- Do not use internal project identifiers, customer identifiers, organization-specific shorthand, or `other` as the scope.
- Independent review items become independent zootree workspaces.
- Review items with shared call chains or strong dependencies stay in the same zootree workspace.
- Development tasks that should start an agent use complete non-interactive `zootree create ... --run-agent codexd` commands with `--title`, `--description`, `--name`, `--branch`, and `--repos` or `--template`.
- The source design is `docs/superpowers/specs/2026-07-10-review-backlog-workspace-split-design.md`.

---

## File Structure

- `AGENTS.md` already contains the project-level instruction for Chinese responses, Conventional Commit-scoped task titles, and zootree workspace splitting rules. No change is planned unless execution reveals that an instruction is inaccurate.
- `docs/superpowers/specs/2026-07-10-review-backlog-workspace-split-design.md` is the approved design and remains the source of truth for workspace titles, descriptions, and execution order.
- `docs/superpowers/plans/2026-07-10-review-backlog-workspace-split.md` is this execution plan and records the exact commands used to create the delegated workspaces.
- zootree runtime config under `~/.config/zootree/` is touched only through the zootree CLI.
- zootree workspaces are created under the user's configured zootree workspace directory by the zootree CLI.

---

### Task 1: Verify orchestration baseline

**Files:**
- Read: `AGENTS.md`
- Read: `docs/superpowers/specs/2026-07-10-review-backlog-workspace-split-design.md`
- Read: `src/cli/workspace.rs`
- Runtime config through CLI: `~/.config/zootree/repos/zootree.toml`

**Interfaces:**
- Consumes: committed design spec and `zootree create` CLI options.
- Produces: verified local prerequisites for creating the delegated workspaces.

- [ ] **Step 1: Verify the working tree is clean**

Run:

```bash
git status --short
```

Expected: no output. If there is output, stop and inspect it before creating workspaces:

```bash
git status --short
git diff --stat
git diff --cached --stat
```

- [ ] **Step 2: Verify the design commit is present**

Run:

```bash
git log --oneline -5
```

Expected: output includes:

```text
c9ff0a7 docs: split review backlog workspaces
```

- [ ] **Step 3: Verify the `zootree` repo is registered**

Run:

```bash
zootree repo list | rg 'zootree -> .*/project/weineel/zootree'
```

Expected: command exits 0 and prints one line for the registered `zootree` repo.

- [ ] **Step 4: Register the repo if Step 3 fails**

Run this only if Step 3 fails:

```bash
zootree repo add --name zootree /Users/lijufeng/project/weineel/zootree --default-target-branch main
```

Expected: output contains:

```text
repo 'zootree' registered
```

- [ ] **Step 5: Verify the first-batch workspace names are unused**

Run:

```bash
zootree list --oneline | rg '^(config-name-validation|layout-kdl-escaping|inline-prompt-scroll|log-config-behavior|workspace-list-api)\b'
```

Expected: no output and exit code 1. If any names are printed, stop and decide whether to reuse, cancel, or rename those workspaces before continuing.

- [ ] **Step 6: Commit any baseline-only documentation change**

Run:

```bash
git status --short
```

Expected: no output. There should be no baseline commit in this task because this plan does not modify source files during execution.

---

### Task 2: Create the first parallel batch

**Files:**
- Read: `docs/superpowers/specs/2026-07-10-review-backlog-workspace-split-design.md`
- Runtime config through CLI: zootree pending workspace configs

**Interfaces:**
- Consumes: verified `zootree` repo registration from Task 1.
- Produces: five pending or started zootree workspaces with `codexd` agents launched by `--run-agent codexd`.

- [ ] **Step 1: Create config name validation workspace**

Run:

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

Expected: output contains:

```text
workspace 'config-name-validation' created
```

- [ ] **Step 2: Create layout escaping workspace**

Run:

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

Expected: output contains:

```text
workspace 'layout-kdl-escaping' created
```

- [ ] **Step 3: Create inline prompt scroll workspace**

Run:

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

Expected: output contains:

```text
workspace 'inline-prompt-scroll' created
```

- [ ] **Step 4: Create log config behavior workspace**

Run:

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

Expected: output contains:

```text
workspace 'log-config-behavior' created
```

- [ ] **Step 5: Create workspace listing API workspace**

Run:

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

Expected: output contains:

```text
workspace 'workspace-list-api' created
```

- [ ] **Step 6: Verify the first batch is visible**

Run:

```bash
zootree list --oneline | rg '^(config-name-validation|layout-kdl-escaping|inline-prompt-scroll|log-config-behavior|workspace-list-api)\b'
```

Expected: five matching lines, one for each first-batch workspace.

---

### Task 3: Gate and create start rollback workspace

**Files:**
- Read: `docs/superpowers/specs/2026-07-10-review-backlog-workspace-split-design.md`
- Runtime config through CLI: zootree workspace configs

**Interfaces:**
- Consumes: first-batch completion state, especially `fix(config): validate config-backed names` and `fix(layout): escape zellij kdl variables`.
- Produces: one zootree workspace for start failure recovery after prerequisite safety work is merged or rebased into the target branch.

- [ ] **Step 1: Verify prerequisite workspaces no longer need pending integration**

Run:

```bash
zootree list --oneline | rg '^(config-name-validation|layout-kdl-escaping)\b'
```

Expected: either no output because both workspaces have been completed/archived, or lines that show both are no longer active implementation blockers. If either workspace is still actively changing `src/config/mod.rs` or `src/core/layout.rs`, stop before creating `start-failure-rollback`.

- [ ] **Step 2: Create start failure rollback workspace**

Run:

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

Expected: output contains:

```text
workspace 'start-failure-rollback' created
```

- [ ] **Step 3: Verify the start rollback workspace is visible**

Run:

```bash
zootree list --oneline | rg '^start-failure-rollback\b'
```

Expected: one matching line for `start-failure-rollback`.

---

### Task 4: Create later create wizard refactor workspace

**Files:**
- Read: `docs/superpowers/specs/2026-07-10-review-backlog-workspace-split-design.md`
- Runtime config through CLI: zootree workspace configs

**Interfaces:**
- Consumes: completion of the first bugfix wave.
- Produces: one behavior-preserving refactor workspace for `CreateWizardApp`.

- [ ] **Step 1: Verify bugfix wave is no longer active**

Run:

```bash
zootree list --oneline | rg '^(config-name-validation|layout-kdl-escaping|inline-prompt-scroll|log-config-behavior|workspace-list-api|start-failure-rollback)\b'
```

Expected: no active blockers remain for the create wizard refactor. If active workspaces are printed, finish or intentionally pause them before creating `create-wizard-refactor`.

- [ ] **Step 2: Create create wizard refactor workspace**

Run:

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

Expected: output contains:

```text
workspace 'create-wizard-refactor' created
```

- [ ] **Step 3: Verify the refactor workspace is visible**

Run:

```bash
zootree list --oneline | rg '^create-wizard-refactor\b'
```

Expected: one matching line for `create-wizard-refactor`.

---

### Task 5: Final orchestration verification

**Files:**
- Read: `AGENTS.md`
- Read: `docs/superpowers/specs/2026-07-10-review-backlog-workspace-split-design.md`
- Read: `docs/superpowers/plans/2026-07-10-review-backlog-workspace-split.md`
- Runtime config through CLI: zootree workspace configs

**Interfaces:**
- Consumes: created workspace records from Tasks 2 through 4.
- Produces: final confidence that the review backlog has been delegated according to the approved split.

- [ ] **Step 1: Verify all expected workspace names exist when all planned workspaces have been created**

Run:

```bash
zootree list --oneline | rg '^(config-name-validation|layout-kdl-escaping|inline-prompt-scroll|log-config-behavior|workspace-list-api|start-failure-rollback|create-wizard-refactor)\b'
```

Expected: seven matching lines after all creation tasks have run. If only the first batch has been created, expected output is the five first-batch lines from Task 2.

- [ ] **Step 2: Verify no generated task title uses `other` scope**

Run:

```bash
zootree list --oneline | rg '\((other)\)'
```

Expected: no output and exit code 1.

- [ ] **Step 3: Verify documentation still has no stale internal scope**

Run:

```bash
rg -n 'fix[(]other[)]|refactor[(]other[)]|P[B]I|config-layout-safet[y]' AGENTS.md docs/superpowers/specs/2026-07-10-review-backlog-workspace-split-design.md docs/superpowers/plans/2026-07-10-review-backlog-workspace-split.md
```

Expected: no output and exit code 1.

- [ ] **Step 4: Verify the source checkout remains clean**

Run:

```bash
git status --short
```

Expected: no output after this plan file has been committed.

- [ ] **Step 5: Commit this plan file**

Run:

```bash
git add docs/superpowers/plans/2026-07-10-review-backlog-workspace-split.md
git commit -m "docs: plan review backlog workspace split"
```

Expected: commit succeeds with one new plan file.
