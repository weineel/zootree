---
status: accepted
---

# Add Workspace repositories transactionally

An in-progress Workspace gains one registered repository through a logical transaction spanning membership, its new worktree, repository setup, and any existing Terminal environment. The operation reports success only when those representations agree, and otherwise rolls back only artifacts it created.

## CLI contract

The public entry point is:

```text
zootree add-repo [workspace] [--repo <repo-name>[:<target-branch>]]
```

- `workspace` must be `in_progress`; when omitted, the user selects one interactively.
- `--repo` accepts one registered repository name, never a path; when omitted, the user selects among registered repositories not already in the Workspace.
- `zootree repo add` remains exclusively responsible for global repository registration.
- A repository already in the Workspace fails validation and reports its recorded Target branch. `add-repo` is not a worktree or Terminal environment repair command.
- The new Workspace repository is appended without reordering existing repositories. The same order drives display, terminal placement, and later `done` or `cancel` processing.
- The automatic `recently` template remains a creation-time snapshot and is not changed.

The Target branch resolves from the explicit suffix, then the registered repository's default, then its current branch. Resolution errors and a locally missing Target branch fail before side effects; zootree never guesses `main` or silently substitutes another branch.

An existing same-named Workspace branch or any existing filesystem entry at `<workspace_dir>/<repo-name>` is an ownership conflict. zootree never adopts, resets, deletes, clears, or moves those objects. Only a Workspace branch and worktree created by the current transaction are eligible for rollback.

## Transaction

`core::workspace_repository` owns the complete transaction behind a small `add` interface. It performs these steps:

1. Load and validate the in-progress Workspace, registered repository, Target branch, Workspace branch, worktree path, and terminal addition plan.
2. Create the Workspace branch and worktree from the Target branch.
3. Apply the existing merged `copy_files` behavior.
4. Execute the existing repository-first, global-fallback `post_create` hook with complete repository context.
5. Append the membership and a `repo_added` event, then atomically replace the Workspace config.
6. Synchronize the disposable Workspace instruction indexes from the committed membership. Index write failures only warn and do not interrupt the transaction.
7. Apply the prepared terminal addition when an existing Terminal environment was found.
8. When the terminal outcome changes the stored state, atomically replace the Workspace config again with that state.

The event detail is `repo=<name>, target_branch=<branch>`. A failed or rolled-back attempt appends neither membership nor event. A reconciled Terminal environment returns normalized opaque state; verified absence performs no terminal work and preserves the existing opaque state for normal future reconciliation. `add-repo` never triggers the Workspace-level global `post_start`.

Rollback proceeds in reverse order and continues safe cleanup after individual failures:

1. Remove only the adapter-native terminal unit created by the operation.
2. If membership was already persisted, restore the previous Workspace config and synchronize its instruction indexes again.
3. Force-remove only its new worktree.
4. After the worktree is gone, force-delete only its new Workspace branch.

The final error aggregates the initiating failure and every cleanup residue. These internal force operations need no user-facing `--force` because preflight established ownership of every eligible target.

## Terminal environment behavior

Terminal environment lookup retains the stored-reference then deterministic-name reconciliation order but never creates an environment for `add-repo`:

- A unique match is prepared for mutation and its state is normalized after successful application.
- Verified absence skips terminal work; a later `zootree open <workspace>` creates the complete environment from updated membership.
- Ambiguity or terminal inspection failure aborts the transaction. Inability to inspect is never treated as absence.
- An existing adapter-native unit with the new repository's deterministic name is an ownership conflict; zootree never adopts or replaces it.

When an environment exists, the operation appends exactly one unit at the end:

- Zellij: one repo tab.
- cmux: one repo workspace in the existing group.
- Herdr: one repo tab in the existing Herdr workspace.

Zellij renders only the single valid `// @repeat-per-repo` tab block from the selected layout. An existing Zellij environment using a custom layout without exactly one such block fails terminal preflight rather than falling back to the default layout. When no Terminal environment exists, incremental layout validation is unnecessary because `open` uses the normal complete-layout path.

The operation never closes, rebuilds, repairs, or rearranges other terminal units. It starts or moves no agent and accepts no AgentIntent, even when a second repository changes a Workspace from single-repo to multi-repo. After success, the new repo unit becomes the target environment's focus, but `add-repo` never attaches or opens a terminal client.

The Terminal environment module exposes a crate-private, adapter-neutral prepare/apply/rollback interface. Opaque prepared and applied values hide session, group, tab, workspace, and rollback references from `core::workspace_repository`, CLI callers, and user output.

## Constraints and consequences

This transaction assumes one lifecycle mutation at a time per Workspace. Cross-command locking, revisions, and concurrent mutation support require a separate design spanning every mutating Workspace command.

Success output reports the repository, Workspace, Target branch, Workspace branch, worktree path, and whether the Terminal environment was updated or absent. Adapter runtime references remain hidden. This deliberately differs from `start` partial success because an incremental membership change can be isolated and reversed without invalidating the already-running Workspace.

## Verification

Tests cover CLI parsing, completion and interactive selection; success with no Terminal environment and with each adapter; every ownership and layout preflight; stored-state recovery, absence, ambiguity and inspection failure; exact adapter commands, focus and rollback references; every transaction failure point and cleanup failure aggregation; event persistence; and downstream `info`, `list`, `done`, and `cancel` behavior.

Implementation updates both READMEs and `skills/zootree-dev/SKILL.md`, then passes `cargo fmt --check`, `cargo clippy -- -D warnings`, and `cargo test`. Tests use `MockRunner` and isolated `ConfigManager` directories rather than mutating a real user Terminal environment.
