---
status: accepted
---

# Make Hook environments deterministic

zootree treats its `ZOOTREE_*` Hook environment variables as an additive public interface. A typed Hook invocation is the single source of cwd and environment construction, so every lifecycle caller reports the same meanings without inheriting stale zootree context from its parent process.

## Environment contract

Existing variable names and meanings remain compatible. Every invocation receives:

| Variable | Meaning |
| --- | --- |
| `ZOOTREE_HOOK` | Hook stage: `post_create`, `post_start`, `pre_done`, `pre_cancel`, or `pre_remove` |
| `ZOOTREE_OPERATION` | Trigger operation: `start`, `reopen`, `add-repo`, `done`, or `cancel` |
| `ZOOTREE_HOOK_SCOPE` | Execution scope: `workspace` or `repo` |
| `ZOOTREE_HOOK_CONFIG_SCOPE` | Scope of the selected Hook configuration: `global` or `repo` |
| `ZOOTREE_WORKSPACE` | Workspace name |
| `ZOOTREE_WORKSPACE_TITLE` | Persisted Workspace title |
| `ZOOTREE_WORKSPACE_DESCRIPTION` | Persisted Workspace description, including an empty value |
| `ZOOTREE_WORKSPACE_STATUS` | Workspace status already persisted when the Hook starts |
| `ZOOTREE_WORKSPACE_DIR` | Expanded Workspace root directory |
| `ZOOTREE_BRANCH` | Workspace branch |
| `ZOOTREE_VERSION` | Version of the running zootree binary |

A repository-scoped invocation additionally receives:

| Variable | Meaning |
| --- | --- |
| `ZOOTREE_REPO` | Registered repository name |
| `ZOOTREE_REPO_SOURCE_DIR` | Expanded Source checkout directory |
| `ZOOTREE_WORKTREE_PATH` | Workspace repository checkout directory |
| `ZOOTREE_TARGET_BRANCH` | Target branch when one is available to that lifecycle caller |

Workspace-scoped Hooks run with the Workspace root as cwd. Repository-scoped Hooks run with the Workspace repository checkout as cwd.

Before starting the Hook process, zootree removes every official variable in the tables above from the inherited environment, then injects only the values applicable to the current invocation. Unrelated parent variables such as `PATH` and `HOME` remain inherited. An absent optional repository variable therefore means "not applicable or unavailable", never "inherited from the launching shell".

## Invocation matrix

| Hook stage | Trigger operation | Hook scope | Persisted status at execution |
| --- | --- | --- | --- |
| `post_create` | `start` | `repo` | `pending` |
| `post_start` | `start` | `workspace` | `in_progress` |
| `post_create` | `reopen` | `repo` | `done` or `canceled` |
| `post_start` | `reopen` | `workspace` | `in_progress` |
| `post_create` | `add-repo` | `repo` | `in_progress` |
| `pre_done` | `done` | `workspace` | `in_progress` |
| `pre_remove` | `done` | `repo` | `in_progress` |
| `pre_cancel` | `cancel` | `workspace` | `in_progress` |
| `pre_remove` | `cancel` | `repo` | `in_progress` |

`create --start` and `create --run-agent` enter the normal `start` workflow and report `start`. Plain `create`, `open`, and cancellation of a still-pending Workspace do not execute Hooks.

The status variable reports an execution-time fact, not a planned source or destination status. The Trigger operation communicates lifecycle intent because the Hook or a later step may still prevent that transition.

## Hook module interface

The Hook module exposes one typed `HookInvocation` interface with distinct Workspace and repository variants. Hook stages, Trigger operations, and configuration scopes are enums; the module derives cwd, environment variables, and reserved-variable cleanup instead of asking lifecycle callers to assemble string fields.

For a repository invocation, the Hook module receives the repository and Global configuration candidates, applies repository-first/Global-fallback selection, and derives `ZOOTREE_HOOK_CONFIG_SCOPE` from the selected value. Callers cannot separately declare provenance. This replaces the former flat `HookContext` interface without a parallel compatibility path: the pre-1.0 Rust library interface may change, while the CLI, TOML, and existing Hook variables remain compatible.

## Exclusions

The contract does not expose agent commands, Terminal environment state, configuration file paths, process IDs, executable paths, repository indexes or counts, or an aggregate JSON mirror. Indexes and counts would confuse Workspace membership with a conditional Hook batch; JSON would duplicate the public representation. Future context is added through new individual variables.

## Verification

Tests cover every row of the invocation matrix, both repository and Global configuration selection, exact cwd and environment values, removal of stale official parent variables, and absence of repository-only variables from Workspace invocations. Lifecycle tests preserve existing Hook failure, rollback, and partial-success behavior. Both READMEs and the zootree development skill are updated with the implemented contract.
