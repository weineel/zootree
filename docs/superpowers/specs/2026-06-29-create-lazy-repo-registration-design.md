# zootree create lazy repo registration design

## Context

`zootree create` currently detects the current git repository before opening the
full-screen create wizard. When the current repository is not registered, the
create path immediately writes a repo config so the repository can appear in the
wizard repo list.

That creates an unwanted side effect: opening or canceling the wizard can leave a
new repo config behind, even when the user did not ultimately choose that repo
for the workspace.

The desired behavior is to make the unregistered current repository selectable
in the wizard first, and only create the repo config if the user submits the
wizard with that repo selected.

## Goals

- Show the current unregistered git repository in the interactive create repo
  list.
- Preselect that repository by default, matching the existing current-repo
  behavior.
- Mark the repository as new in the repo list and review page, for example
  `(new, will register)`.
- Defer writing `repos/<name>.toml` until final create submit.
- Avoid writing repo config when the wizard is canceled or the new repo is
  deselected.
- Preserve existing behavior for registered repos, target branch defaults, and
  fully non-interactive create calls.

## Non-goals

- Do not change the repo config schema.
- Do not change the workspace config schema.
- Do not make non-interactive `--repos missing` auto-register unknown repos.
- Do not add a general-purpose repo discovery feature outside `zootree create`.
- Do not redesign the create wizard UI beyond the small pending-repo marker.

## Recommended Approach

Represent the unregistered current repo as a draft repo entry with source
metadata.

Extend the create draft model so each `RepoDraftEntry` can distinguish a normal
registered repo from a repo pending registration. A pending repo carries the
resolved git root path and the repo name that would be registered on submit.

This keeps the create flow aligned with the current wizard architecture:

1. Build a draft with no persistent side effects.
2. Let the wizard edit and validate that draft.
3. On submit, materialize only the selected draft state.

## Data Model

Add source metadata to `RepoDraftEntry`, such as:

```rust
pub enum RepoDraftSource {
    Registered,
    PendingRegistration { path: String },
}

pub struct RepoDraftEntry {
    pub name: String,
    pub target_branch: String,
    pub selected: bool,
    pub source: RepoDraftSource,
}
```

Existing callers can continue using a constructor that defaults to
`RepoDraftSource::Registered`. Pending current-repo construction should use an
explicit constructor or helper so tests make the side effect boundary obvious.

## Current Repo Discovery

Replace the current eager registration helper with a read-only discovery helper.

The helper should:

1. Run `git rev-parse --show-toplevel` from the current directory.
2. Return `None` when the current directory is not inside a git repository.
3. Canonicalize the detected root for comparison.
4. If a registered repo already points at that root, return the registered repo
   name and current branch.
5. If no registered repo points at that root, derive a unique name from the
   root directory basename and return a pending registration candidate.

The helper must not call `ConfigManager::save_repo_config`.

The current branch rule stays unchanged:

- current repo candidates use the repository's current branch;
- non-current registered repos use `default_target_branch` first;
- registered repos without a default fall back to their current branch, then
  `main` if branch lookup fails.

## Draft Construction

`draft_from_args` should receive the current repo candidate and build the repo
list as follows:

- Registered repos continue to come from `ConfigManager::list_repos()`.
- If the current repo candidate is registered, that repo is marked selected and
  gets the current branch target.
- If the current repo candidate is pending, append or insert it into the repo
  draft list, mark it selected, and set its target branch to the current branch.
- The pending entry must participate in filtering, toggling, validation, target
  branch editing, review, and `workspace_from_draft` just like registered repos.

For this change, sorting can remain simple and predictable: keep existing
registered repos in their existing sorted order, then add the pending current
repo if it is not already registered. This avoids reshuffling existing lists.

## Submit Behavior

Before saving the workspace, the create submit path should persist selected
pending repo entries.

For each selected pending repo:

1. Confirm the pending repo name is still available.
2. If the name is now taken, generate the next unique name and update the draft
   entry before workspace conversion.
3. Write `RepoConfig` using the pending root path and existing default values:
   `default_target_branch: None`, empty `copy_files`, default hooks, no lazygit,
   and no repo-level zellij override.
4. Continue with workspace conversion and save.

If repo config persistence fails, create should fail before saving the workspace.
That prevents a workspace from referencing a repo config that does not exist.

Unselected pending repos are ignored and never written.

## UI Behavior

The repo selection page should show pending repos with a compact marker:

```text
[x] zootree (new, will register)
```

The review page should use the same marker:

```text
- zootree (new, will register) -> main
```

Registered repos remain visually unchanged.

## CLI Behavior

This change only applies to interactive create flows that need repo selection.

Fully non-interactive calls remain strict:

- `zootree create --title t --repos missing` still fails with
  `repo 'missing' is not registered`.
- `zootree create --title t --template recently` still uses the registered repos
  in the template and does not auto-register an unrelated current repo.

When create enters the wizard because repo selection is missing, the pending
current repo can appear as a selectable candidate.

## Error Handling

Wizard validation remains field-level:

- at least one repo must be selected;
- selected repos need a non-empty single-line target branch.

System-level errors still bubble through `anyhow`:

- git discovery failures other than "not a git repo" when relevant;
- repo config read or write failures;
- workspace save failures;
- start-after-create failures.

Canceling the wizard must not save workspace config or pending repo config.

## Testing

Add focused tests around pure draft behavior and the submit side effect boundary:

1. An unregistered current git repo appears in the draft repo list, is selected
   by default, has pending source metadata, and does not write repo config during
   draft construction.
2. A registered current repo remains selected and uses the current branch.
3. A selected pending repo is saved before the workspace is saved.
4. A deselected pending repo is not saved and is not included in the workspace.
5. A pending repo name collision at submit time is resolved by assigning a fresh
   unique name and using that name in the workspace.
6. `--repos missing` remains an error in non-interactive create.
7. The repo selection and review render helpers include the pending marker.

Run:

```sh
cargo fmt --check
cargo test create_flow
cargo test
```

## Acceptance Criteria

- Running `zootree create` inside an unregistered git repo opens the wizard with
  that repo visible, selected, and marked as new.
- Canceling the wizard leaves no new repo config.
- Deselecting the pending repo and submitting leaves no new repo config.
- Submitting with the pending repo selected writes the repo config and creates a
  workspace referencing it.
- Existing registered repo behavior is unchanged.
- Non-interactive unknown repo arguments still fail.
