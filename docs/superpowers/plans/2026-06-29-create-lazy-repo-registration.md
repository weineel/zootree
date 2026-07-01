# Create Lazy Repo Registration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `zootree create` show an unregistered current git repo as a selectable wizard candidate and only register it if the submitted workspace still selects it.

**Architecture:** Move current-repo detection into side-effect-free create-flow helpers, model pending repo registration in `RepoDraftEntry`, and persist selected pending repos immediately before workspace conversion. Keep config schemas unchanged and restrict lazy registration to interactive create flows that need repo selection.

**Tech Stack:** Rust, Clap CLI, ratatui create wizard, TOML-backed `ConfigManager`, `cargo test`.

---

## File Structure

- Modify `src/cli/create_flow.rs`
  - Own create-specific draft types.
  - Add `RepoDraftSource`, `CurrentRepoCandidate`, read-only current repo discovery, pending repo persistence, and pending repo display labels.
  - Keep non-interactive explicit repo validation strict.
- Modify `src/cli/workspace.rs`
  - Stop calling eager repo registration before the wizard.
  - Call read-only discovery and persist selected pending repos only after wizard submit.
  - Remove the old eager registration helper and its unit tests.
- Modify `src/tui_app/create_wizard.rs`
  - Render pending repos with `(new, will register)` in repo selection and review.
  - Add a small render helper that can be unit-tested.
- Modify `tests/create_flow_test.rs`
  - Cover pending draft construction, no write during draft construction, selected pending persistence, deselected pending ignore, submit-time name collision, and strict non-interactive unknown repo behavior.
- Modify `tests/create_wizard_test.rs`
  - Cover pending repo label rendering through public wizard label helpers.

---

### Task 1: Add Pending Repo Draft Metadata

**Files:**
- Modify: `src/cli/create_flow.rs`
- Test: `tests/create_flow_test.rs`

- [ ] **Step 1: Write failing tests for repo draft source metadata**

Add `RepoDraftSource` to the import list in `tests/create_flow_test.rs`:

```rust
use zootree::cli::create_flow::{
    build_repo_draft_entries, create_args_need_wizard, draft_from_args,
    resolve_agent_cli_for_draft, workspace_from_draft, AfterCreateMode, CreateDraft,
    CreateDraftError, CreateWizardLayout, CreateWizardOutput, RepoDraftEntry,
    RepoDraftSource,
};
```

Add these tests near the existing repo draft tests:

```rust
#[test]
fn repo_draft_entry_new_defaults_to_registered_source() {
    let entry = RepoDraftEntry::new("frontend", "main", true);

    assert_eq!(entry.source, RepoDraftSource::Registered);
}

#[test]
fn pending_repo_draft_entry_records_path_and_label() {
    let entry = RepoDraftEntry::pending_registration(
        "zootree",
        "feature/current",
        true,
        "/repo/zootree",
    );

    assert_eq!(entry.name, "zootree");
    assert_eq!(entry.target_branch, "feature/current");
    assert!(entry.selected);
    assert_eq!(
        entry.source,
        RepoDraftSource::PendingRegistration {
            path: "/repo/zootree".into()
        }
    );
    assert!(entry.is_pending_registration());
    assert_eq!(entry.display_name(), "zootree (new, will register)");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```sh
cargo test --test create_flow_test repo_draft_entry_new_defaults_to_registered_source
cargo test --test create_flow_test pending_repo_draft_entry_records_path_and_label
```

Expected: compilation fails because `RepoDraftSource`, `pending_registration`, `is_pending_registration`, and `display_name` do not exist.

- [ ] **Step 3: Implement source metadata on `RepoDraftEntry`**

In `src/cli/create_flow.rs`, replace the current `RepoDraftEntry` definition with:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepoDraftSource {
    Registered,
    PendingRegistration { path: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoDraftEntry {
    pub name: String,
    pub target_branch: String,
    pub selected: bool,
    pub source: RepoDraftSource,
}

impl RepoDraftEntry {
    pub fn new(name: impl Into<String>, target_branch: impl Into<String>, selected: bool) -> Self {
        Self {
            name: name.into(),
            target_branch: target_branch.into(),
            selected,
            source: RepoDraftSource::Registered,
        }
    }

    pub fn pending_registration(
        name: impl Into<String>,
        target_branch: impl Into<String>,
        selected: bool,
        path: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            target_branch: target_branch.into(),
            selected,
            source: RepoDraftSource::PendingRegistration { path: path.into() },
        }
    }

    pub fn is_pending_registration(&self) -> bool {
        matches!(self.source, RepoDraftSource::PendingRegistration { .. })
    }

    pub fn display_name(&self) -> String {
        if self.is_pending_registration() {
            format!("{} (new, will register)", self.name)
        } else {
            self.name.clone()
        }
    }
}
```

- [ ] **Step 4: Run targeted tests**

Run:

```sh
cargo test --test create_flow_test repo_draft_entry_new_defaults_to_registered_source
cargo test --test create_flow_test pending_repo_draft_entry_records_path_and_label
```

Expected: both tests pass.

- [ ] **Step 5: Commit**

```sh
git add src/cli/create_flow.rs tests/create_flow_test.rs
git commit -m "feat(create): track pending repo draft source"
```

---

### Task 2: Replace Eager Registration With Read-Only Current Repo Discovery

**Files:**
- Modify: `src/cli/create_flow.rs`
- Modify: `src/cli/workspace.rs`
- Test: `tests/create_flow_test.rs`

- [ ] **Step 1: Write failing tests for read-only current repo discovery**

Extend the import in `tests/create_flow_test.rs` to include `CurrentRepoCandidate` and `discover_current_repo_candidate`:

```rust
use zootree::cli::create_flow::{
    build_repo_draft_entries, create_args_need_wizard, discover_current_repo_candidate,
    draft_from_args, resolve_agent_cli_for_draft, workspace_from_draft, AfterCreateMode,
    CreateDraft, CreateDraftError, CreateWizardLayout, CreateWizardOutput,
    CurrentRepoCandidate, RepoDraftEntry, RepoDraftSource,
};
```

Add these tests near the repo draft tests:

```rust
#[test]
fn discover_current_repo_candidate_returns_pending_without_writing_config() {
    let tmp = TempDir::new().unwrap();
    let mgr = ConfigManager::with_base_dir(tmp.path().join("config"));
    mgr.ensure_dirs().unwrap();
    let repo_root = tmp.path().join("zootree");
    std::fs::create_dir(&repo_root).unwrap();
    let runner = MockRunner::new();
    runner.push_response(success_stdout(&format!("{}\n", repo_root.display())));
    runner.push_response(success_stdout("feature/current\n"));

    let candidate = discover_current_repo_candidate(&mgr, &runner, &repo_root)
        .unwrap()
        .unwrap();

    assert_eq!(
        candidate,
        CurrentRepoCandidate::PendingRegistration {
            name: "zootree".into(),
            path: repo_root.canonicalize().unwrap().to_string_lossy().into_owned(),
            current_branch: "feature/current".into(),
        }
    );
    assert!(mgr.list_repos().unwrap().is_empty());
}

#[test]
fn discover_current_repo_candidate_reuses_registered_repo_for_same_path() {
    let tmp = TempDir::new().unwrap();
    let mgr = ConfigManager::with_base_dir(tmp.path().join("config"));
    mgr.ensure_dirs().unwrap();
    let repo_root = tmp.path().join("zootree");
    std::fs::create_dir(&repo_root).unwrap();
    mgr.save_repo_config(
        "custom",
        &repo_config(&repo_root.to_string_lossy(), Some("develop")),
    )
    .unwrap();
    let runner = MockRunner::new();
    runner.push_response(success_stdout(&format!("{}\n", repo_root.display())));
    runner.push_response(success_stdout("feature/current\n"));

    let candidate = discover_current_repo_candidate(&mgr, &runner, &repo_root)
        .unwrap()
        .unwrap();

    assert_eq!(
        candidate,
        CurrentRepoCandidate::Registered {
            name: "custom".into(),
            current_branch: "feature/current".into(),
        }
    );
    assert_eq!(mgr.list_repos().unwrap(), vec!["custom"]);
}

#[test]
fn discover_current_repo_candidate_uses_collision_safe_pending_name() {
    let tmp = TempDir::new().unwrap();
    let mgr = ConfigManager::with_base_dir(tmp.path().join("config"));
    mgr.ensure_dirs().unwrap();
    let existing_root = tmp.path().join("existing");
    let repo_root = tmp.path().join("zootree");
    std::fs::create_dir(&existing_root).unwrap();
    std::fs::create_dir(&repo_root).unwrap();
    mgr.save_repo_config(
        "zootree",
        &repo_config(&existing_root.to_string_lossy(), None),
    )
    .unwrap();
    let runner = MockRunner::new();
    runner.push_response(success_stdout(&format!("{}\n", repo_root.display())));
    runner.push_response(success_stdout("feature/current\n"));

    let candidate = discover_current_repo_candidate(&mgr, &runner, &repo_root)
        .unwrap()
        .unwrap();

    assert_eq!(
        candidate,
        CurrentRepoCandidate::PendingRegistration {
            name: "zootree-2".into(),
            path: repo_root.canonicalize().unwrap().to_string_lossy().into_owned(),
            current_branch: "feature/current".into(),
        }
    );
    assert_eq!(mgr.list_repos().unwrap(), vec!["zootree"]);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```sh
cargo test --test create_flow_test discover_current_repo_candidate
```

Expected: compilation fails because `CurrentRepoCandidate` and `discover_current_repo_candidate` do not exist.

- [ ] **Step 3: Implement read-only discovery in `src/cli/create_flow.rs`**

Add imports near the top of `src/cli/create_flow.rs`:

```rust
use std::collections::HashSet;
use std::path::{Path, PathBuf};
```

Add these types and helpers after `CreateWizardOutput`:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CurrentRepoCandidate {
    Registered {
        name: String,
        current_branch: String,
    },
    PendingRegistration {
        name: String,
        path: String,
        current_branch: String,
    },
}

impl CurrentRepoCandidate {
    fn name(&self) -> &str {
        match self {
            Self::Registered { name, .. } | Self::PendingRegistration { name, .. } => name,
        }
    }

    fn current_branch(&self) -> &str {
        match self {
            Self::Registered { current_branch, .. }
            | Self::PendingRegistration { current_branch, .. } => current_branch,
        }
    }
}

fn canonical_or_original(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn repo_name_from_path(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "repo".into())
}

fn unique_repo_name(config_mgr: &ConfigManager, base: &str) -> anyhow::Result<String> {
    let existing: HashSet<String> = config_mgr.list_repos()?.into_iter().collect();
    if !existing.contains(base) {
        return Ok(base.to_string());
    }

    for i in 2.. {
        let candidate = format!("{}-{}", base, i);
        if !existing.contains(&candidate) {
            return Ok(candidate);
        }
    }

    unreachable!("unbounded repo name search should always return")
}

fn registered_repo_for_path(
    config_mgr: &ConfigManager,
    repo_root: &Path,
) -> anyhow::Result<Option<String>> {
    let repo_root = canonical_or_original(repo_root);
    for name in config_mgr.list_repos()? {
        let config = config_mgr.load_repo_config(&name)?;
        let expanded = shellexpand::tilde(&config.path).into_owned();
        if canonical_or_original(Path::new(&expanded)) == repo_root {
            return Ok(Some(name));
        }
    }
    Ok(None)
}

pub fn discover_current_repo_candidate<R: CommandRunner>(
    config_mgr: &ConfigManager,
    runner: &R,
    cwd: &Path,
) -> anyhow::Result<Option<CurrentRepoCandidate>> {
    let git = GitOps::new(runner);
    let cwd = cwd.to_string_lossy().into_owned();
    let root = match git.repo_root(&cwd) {
        Ok(root) => PathBuf::from(root),
        Err(_) => return Ok(None),
    };
    let root = canonical_or_original(&root);
    let root_str = root.to_string_lossy().into_owned();
    let current_branch = git
        .current_branch(&root_str)
        .unwrap_or_else(|_| "main".into());

    if let Some(name) = registered_repo_for_path(config_mgr, &root)? {
        return Ok(Some(CurrentRepoCandidate::Registered {
            name,
            current_branch,
        }));
    }

    let base = repo_name_from_path(&root);
    let name = unique_repo_name(config_mgr, &base)?;
    Ok(Some(CurrentRepoCandidate::PendingRegistration {
        name,
        path: root_str,
        current_branch,
    }))
}
```

- [ ] **Step 4: Remove eager helper from `src/cli/workspace.rs`**

Remove the private create-registration items from `src/cli/workspace.rs` because
the create-flow module now owns them. Delete the contiguous block that starts at:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
struct CurrentRepoDefault {
    name: String,
    current_branch: String,
}
```

and ends after the closing brace of:

```rust
fn ensure_current_repo_registered<R: crate::runner::CommandRunner>(
    config_mgr: &ConfigManager,
    runner: &R,
    cwd: &Path,
) -> Result<Option<CurrentRepoDefault>>
```

The deleted block includes these helper functions:

```text
canonical_or_original
repo_name_from_path
unique_repo_name
registered_repo_for_path
ensure_current_repo_registered
```

Also remove the now-unused import:

```rust
use std::collections::HashSet;
```

Do not remove `Path` or `PathBuf`; later code in `workspace.rs` still uses both.

Remove these eager-registration test functions from the internal `workspace.rs`
test module:

```text
ensure_current_repo_registered_adds_unregistered_git_repo
ensure_current_repo_registered_reuses_existing_repo_config
ensure_current_repo_registered_avoids_name_collision
```

- [ ] **Step 5: Run targeted tests**

Run:

```sh
cargo test --test create_flow_test discover_current_repo_candidate
```

Expected: the three discovery tests pass and no repo config is written during discovery.

- [ ] **Step 6: Commit**

```sh
git add src/cli/create_flow.rs src/cli/workspace.rs tests/create_flow_test.rs
git commit -m "feat(create): discover current repo without registering"
```

---

### Task 3: Build Repo Draft Entries From Current Repo Candidate

**Files:**
- Modify: `src/cli/create_flow.rs`
- Modify: `src/cli/workspace.rs`
- Test: `tests/create_flow_test.rs`

- [ ] **Step 1: Write failing tests for pending candidate draft construction**

Add these tests near the existing `repo_draft_prefers_current_repo_branch_over_config_default` test:

```rust
#[test]
fn repo_draft_appends_pending_current_repo_selected_by_default() {
    let tmp = TempDir::new().unwrap();
    let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
    mgr.ensure_dirs().unwrap();
    mgr.save_repo_config("backend", &repo_config("/repo/backend", Some("develop")))
        .unwrap();
    let runner = MockRunner::new();

    let repos = build_repo_draft_entries(
        &mgr,
        &runner,
        Some(CurrentRepoCandidate::PendingRegistration {
            name: "zootree".into(),
            path: "/repo/zootree".into(),
            current_branch: "feature/current".into(),
        }),
    )
    .unwrap();

    assert_eq!(repos.len(), 2);
    assert_eq!(repos[0].name, "backend");
    assert_eq!(repos[0].source, RepoDraftSource::Registered);
    assert!(!repos[0].selected);
    assert_eq!(repos[1].name, "zootree");
    assert_eq!(
        repos[1].source,
        RepoDraftSource::PendingRegistration {
            path: "/repo/zootree".into()
        }
    );
    assert!(repos[1].selected);
    assert_eq!(repos[1].target_branch, "feature/current");
}

#[test]
fn draft_from_args_includes_pending_current_repo_for_interactive_repo_selection() {
    let tmp = TempDir::new().unwrap();
    let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
    mgr.ensure_dirs().unwrap();
    let runner = MockRunner::new();
    let global = GlobalConfig::default();
    let args = create_args_with(Some("auth cleanup"), None, None);

    let draft = draft_from_args(
        &args,
        &mgr,
        &runner,
        &global,
        Some(CurrentRepoCandidate::PendingRegistration {
            name: "zootree".into(),
            path: "/repo/zootree".into(),
            current_branch: "feature/current".into(),
        }),
        &[],
    )
    .unwrap();

    assert_eq!(draft.repos.len(), 1);
    assert_eq!(draft.repos[0].name, "zootree");
    assert!(draft.repos[0].selected);
    assert!(draft.repos[0].is_pending_registration());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```sh
cargo test --test create_flow_test repo_draft_appends_pending_current_repo_selected_by_default
cargo test --test create_flow_test draft_from_args_includes_pending_current_repo_for_interactive_repo_selection
```

Expected: compilation fails because `build_repo_draft_entries` and `draft_from_args` still accept `Option<(String, String)>`.

- [ ] **Step 3: Update `draft_from_args` and `build_repo_draft_entries` signatures**

In `src/cli/create_flow.rs`, change the `draft_from_args` parameter:

```rust
current_repo: Option<CurrentRepoCandidate>,
```

Update `build_repo_draft_entries` signature:

```rust
pub fn build_repo_draft_entries<R: CommandRunner>(
    config_mgr: &ConfigManager,
    runner: &R,
    current_repo: Option<CurrentRepoCandidate>,
) -> anyhow::Result<Vec<RepoDraftEntry>> {
    let git = GitOps::new(runner);
    let mut repos = Vec::new();
    for name in config_mgr.list_repos()? {
        let config = config_mgr.load_repo_config(&name)?;
        let expanded_path = shellexpand::tilde(&config.path).into_owned();
        let is_current = current_repo
            .as_ref()
            .map(|candidate| candidate.name() == name)
            .unwrap_or(false);
        let target_branch = if is_current {
            current_repo
                .as_ref()
                .map(|candidate| candidate.current_branch().to_string())
                .unwrap_or_else(|| "main".into())
        } else if let Some(default) = config.default_target_branch {
            default
        } else {
            git.current_branch(&expanded_path)
                .unwrap_or_else(|_| "main".into())
        };
        repos.push(RepoDraftEntry::new(name, target_branch, is_current));
    }

    if let Some(CurrentRepoCandidate::PendingRegistration {
        name,
        path,
        current_branch,
    }) = current_repo
    {
        repos.push(RepoDraftEntry::pending_registration(
            name,
            current_branch,
            true,
            path,
        ));
    }

    Ok(repos)
}
```

The existing calls inside `draft_from_args` stay structurally the same after the parameter type change:

```rust
draft.repos = build_repo_draft_entries(config_mgr, runner, current_repo)?;
```

- [ ] **Step 4: Update existing tests to pass `CurrentRepoCandidate`**

Replace existing test arguments like:

```rust
Some(("frontend".to_string(), "feature/current".to_string()))
```

with:

```rust
Some(CurrentRepoCandidate::Registered {
    name: "frontend".to_string(),
    current_branch: "feature/current".to_string(),
})
```

Replace `None` current-repo arguments with unchanged `None`.

- [ ] **Step 5: Update `handle_create` to pass read-only discovery**

In `src/cli/workspace.rs`, update the create-flow import:

```rust
use crate::cli::create_flow::{
    create_args_need_wizard, discover_current_repo_candidate, draft_from_args,
    resolve_agent_cli_for_draft, workspace_from_draft, AfterCreateMode, CreateDraftError,
    CreateWizardOutput,
};
```

Replace the current eager-registration block in `handle_create` with:

```rust
let current_repo = if needs_wizard && needs_repo_selection {
    discover_current_repo_candidate(&config_mgr, &runner, &std::env::current_dir()?)?
} else {
    None
};
```

- [ ] **Step 6: Run targeted tests**

Run:

```sh
cargo test --test create_flow_test repo_draft
cargo test --test create_flow_test draft_from_args
```

Expected: create-flow draft tests pass.

- [ ] **Step 7: Commit**

```sh
git add src/cli/create_flow.rs src/cli/workspace.rs tests/create_flow_test.rs
git commit -m "feat(create): include pending current repo in draft"
```

---

### Task 4: Persist Selected Pending Repos At Submit Time

**Files:**
- Modify: `src/cli/create_flow.rs`
- Modify: `src/cli/workspace.rs`
- Test: `tests/create_flow_test.rs`

- [ ] **Step 1: Write failing tests for pending repo persistence**

Add these tests near `workspace_from_draft_matches_existing_create_shape`:

```rust
#[test]
fn persist_selected_pending_repos_writes_selected_repo_config() {
    let tmp = TempDir::new().unwrap();
    let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
    mgr.ensure_dirs().unwrap();
    let global = GlobalConfig::default();
    let mut draft = CreateDraft::new("auth cleanup", "open-reef", &global);
    draft.repos = vec![RepoDraftEntry::pending_registration(
        "zootree",
        "feature/current",
        true,
        "/repo/zootree",
    )];

    persist_selected_pending_repos(&mgr, &mut draft).unwrap();

    let config = mgr.load_repo_config("zootree").unwrap();
    assert_eq!(config.path, "/repo/zootree");
    assert!(config.default_target_branch.is_none());
    assert!(config.copy_files.is_empty());
    assert!(config.lazygit.is_none());
    assert!(config.zellij.is_none());
    assert_eq!(draft.repos[0].source, RepoDraftSource::Registered);
}

#[test]
fn persist_selected_pending_repos_ignores_deselected_pending_repo() {
    let tmp = TempDir::new().unwrap();
    let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
    mgr.ensure_dirs().unwrap();
    let global = GlobalConfig::default();
    let mut draft = CreateDraft::new("auth cleanup", "open-reef", &global);
    draft.repos = vec![RepoDraftEntry::pending_registration(
        "zootree",
        "feature/current",
        false,
        "/repo/zootree",
    )];

    persist_selected_pending_repos(&mgr, &mut draft).unwrap();

    assert!(mgr.list_repos().unwrap().is_empty());
    assert!(draft.repos[0].is_pending_registration());
    let workspace = workspace_from_draft(&draft, "2026-06-29T10:00:00+08:00", None);
    assert!(workspace.repos.is_empty());
}

#[test]
fn persist_selected_pending_repos_resolves_submit_time_name_collision() {
    let tmp = TempDir::new().unwrap();
    let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
    mgr.ensure_dirs().unwrap();
    mgr.save_repo_config("zootree", &repo_config("/repo/other", None))
        .unwrap();
    let global = GlobalConfig::default();
    let mut draft = CreateDraft::new("auth cleanup", "open-reef", &global);
    draft.repos = vec![RepoDraftEntry::pending_registration(
        "zootree",
        "feature/current",
        true,
        "/repo/zootree",
    )];

    persist_selected_pending_repos(&mgr, &mut draft).unwrap();

    assert_eq!(draft.repos[0].name, "zootree-2");
    assert_eq!(draft.repos[0].source, RepoDraftSource::Registered);
    assert!(mgr.load_repo_config("zootree").is_ok());
    let new_config = mgr.load_repo_config("zootree-2").unwrap();
    assert_eq!(new_config.path, "/repo/zootree");
    let workspace = workspace_from_draft(&draft, "2026-06-29T10:00:00+08:00", None);
    assert_eq!(workspace.repos[0].name, "zootree-2");
}
```

Extend the import to include:

```rust
persist_selected_pending_repos,
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```sh
cargo test --test create_flow_test persist_selected_pending_repos
```

Expected: compilation fails because `persist_selected_pending_repos` does not exist.

- [ ] **Step 3: Implement pending persistence**

In `src/cli/create_flow.rs`, add this import:

```rust
use crate::config::global::HooksConfig;
use crate::config::repo::RepoConfig;
```

Add this function after `build_requested_repo_draft_entries`:

```rust
pub fn persist_selected_pending_repos(
    config_mgr: &ConfigManager,
    draft: &mut CreateDraft,
) -> anyhow::Result<()> {
    for repo in draft.repos.iter_mut().filter(|repo| repo.selected) {
        let RepoDraftSource::PendingRegistration { path } = repo.source.clone() else {
            continue;
        };

        let available_name = unique_repo_name(config_mgr, &repo.name)?;
        repo.name = available_name;
        let repo_config = RepoConfig {
            path,
            default_target_branch: None,
            copy_files: Vec::new(),
            hooks: HooksConfig::default(),
            lazygit: None,
            zellij: None,
        };
        config_mgr.save_repo_config(&repo.name, &repo_config)?;
        repo.source = RepoDraftSource::Registered;
    }

    Ok(())
}
```

- [ ] **Step 4: Call persistence before workspace conversion**

In `src/cli/workspace.rs`, include `persist_selected_pending_repos` in the create-flow import:

```rust
use crate::cli::create_flow::{
    create_args_need_wizard, discover_current_repo_candidate, draft_from_args,
    persist_selected_pending_repos, resolve_agent_cli_for_draft, workspace_from_draft,
    AfterCreateMode, CreateDraftError, CreateWizardOutput,
};
```

Change the output binding in `handle_create` from immutable to mutable:

```rust
let mut output = if needs_wizard {
    run_create_wizard(draft, global.clone(), existing.clone())?
} else {
    let errors = draft.validate(&existing, &global);
    if !errors.is_empty() {
        anyhow::bail!("invalid create options: {}", format_draft_errors(&errors));
    }
    CreateWizardOutput { draft }
};
```

Add pending persistence immediately before resolving agent and converting the workspace:

```rust
persist_selected_pending_repos(&config_mgr, &mut output.draft)?;
let agent_cli = resolve_agent_cli_for_draft(&output.draft.after_create, &global)?;
let workspace = workspace_from_draft(&output.draft, Local::now().to_rfc3339(), agent_cli);
```

- [ ] **Step 5: Run targeted tests**

Run:

```sh
cargo test --test create_flow_test persist_selected_pending_repos
```

Expected: the three persistence tests pass.

- [ ] **Step 6: Commit**

```sh
git add src/cli/create_flow.rs src/cli/workspace.rs tests/create_flow_test.rs
git commit -m "feat(create): persist selected pending repos on submit"
```

---

### Task 5: Render Pending Repo Marker In Wizard

**Files:**
- Modify: `src/tui_app/create_wizard.rs`
- Test: `tests/create_wizard_test.rs`

- [ ] **Step 1: Write failing tests for pending labels**

In `tests/create_wizard_test.rs`, add imports if missing:

```rust
use zootree::cli::create_flow::RepoDraftEntry;
```

Add these tests near existing create wizard rendering or state tests:

```rust
#[test]
fn repo_list_label_marks_pending_registration() {
    let repo = RepoDraftEntry::pending_registration(
        "zootree",
        "feature/current",
        true,
        "/repo/zootree",
    );

    assert_eq!(
        zootree::tui_app::create_wizard::repo_list_label(&repo, true, true),
        "> [x] zootree (new, will register)"
    );
}

#[test]
fn review_repo_label_marks_pending_registration() {
    let repo = RepoDraftEntry::pending_registration(
        "zootree",
        "feature/current",
        true,
        "/repo/zootree",
    );

    assert_eq!(
        zootree::tui_app::create_wizard::review_repo_label(&repo),
        "- zootree (new, will register) -> feature/current"
    );
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```sh
cargo test --test create_wizard_test repo_list_label_marks_pending_registration
cargo test --test create_wizard_test review_repo_label_marks_pending_registration
```

Expected: compilation fails because `repo_list_label` and `review_repo_label` do not exist.

- [ ] **Step 3: Add wizard label helpers**

In `src/tui_app/create_wizard.rs`, add these public helpers near the top-level types:

```rust
pub fn repo_list_label(
    repo: &crate::cli::create_flow::RepoDraftEntry,
    selected: bool,
    focused: bool,
) -> String {
    let cursor = if focused { ">" } else { " " };
    let selected = if selected { "[x]" } else { "[ ]" };
    format!("{cursor} {selected} {}", repo.display_name())
}

pub fn review_repo_label(repo: &crate::cli::create_flow::RepoDraftEntry) -> String {
    format!("- {} -> {}", repo.display_name(), repo.target_branch)
}
```

Replace both repo-list rendering sites. In `repos_page_paragraph`, replace:

```rust
let cursor = if visible_idx == self.repo_cursor {
    ">"
} else {
    " "
};
let selected = if repo.selected { "[x]" } else { "[ ]" };
lines.push(Line::from(format!("{cursor} {selected} {}", repo.name)));
```

with:

```rust
lines.push(Line::from(repo_list_label(
    repo,
    repo.selected,
    visible_idx == self.repo_cursor,
)));
```

In `page_content_lines` for `CreateWizardPage::Repos`, replace the equivalent cursor and selected block with:

```rust
Line::from(repo_list_label(
    repo,
    repo.selected,
    visible_idx == self.repo_cursor,
))
```

In `page_content_lines` for `CreateWizardPage::Review`, replace:

```rust
lines.push(Line::from(format!(
    "- {} -> {}",
    repo.name, repo.target_branch
)));
```

with:

```rust
lines.push(Line::from(review_repo_label(repo)));
```

- [ ] **Step 4: Run targeted tests**

Run:

```sh
cargo test --test create_wizard_test repo_list_label_marks_pending_registration
cargo test --test create_wizard_test review_repo_label_marks_pending_registration
```

Expected: both tests pass.

- [ ] **Step 5: Commit**

```sh
git add src/tui_app/create_wizard.rs tests/create_wizard_test.rs
git commit -m "feat(create): mark pending repos in wizard"
```

---

### Task 6: Regression And Full Verification

**Files:**
- Modify: only files changed by earlier tasks if failures require fixes.
- Test: `tests/create_flow_test.rs`, `tests/create_wizard_test.rs`, full suite.

- [ ] **Step 1: Run formatting**

Run:

```sh
cargo fmt
cargo fmt --check
```

Expected: `cargo fmt --check` exits successfully with no diff.

- [ ] **Step 2: Run create-flow tests**

Run:

```sh
cargo test --test create_flow_test
```

Expected: all create-flow tests pass.

- [ ] **Step 3: Run create-wizard tests**

Run:

```sh
cargo test --test create_wizard_test
```

Expected: all create-wizard tests pass.

- [ ] **Step 4: Run full test suite**

Run:

```sh
cargo test
```

Expected: all tests pass.

- [ ] **Step 5: Inspect final diff**

Run:

```sh
git diff --stat
git diff -- src/cli/create_flow.rs src/cli/workspace.rs src/tui_app/create_wizard.rs tests/create_flow_test.rs tests/create_wizard_test.rs
```

Expected: diff is limited to lazy current-repo registration, pending repo display, and tests. No repo or workspace config schema changes are present.

- [ ] **Step 6: Commit verification fixes if any were needed**

If Step 1 through Step 5 required code changes, commit them:

```sh
git add src/cli/create_flow.rs src/cli/workspace.rs src/tui_app/create_wizard.rs tests/create_flow_test.rs tests/create_wizard_test.rs
git commit -m "test(create): verify lazy repo registration"
```

If no fixes were needed, do not create an empty commit.

---

## Manual Smoke Check

Run this after automated tests pass:

```sh
tmp_home="$(mktemp -d)"
repo_dir="$(mktemp -d)/zootree-smoke"
git init "$repo_dir"
git -C "$repo_dir" checkout -b feature/current
HOME="$tmp_home" cargo run -- create
```

In the wizard:

1. Confirm the repo list shows `zootree-smoke (new, will register)`.
2. Press Space to deselect it and submit with another registered repo if one exists in the test HOME.
3. Confirm no `"$tmp_home/.config/zootree/repos/zootree-smoke.toml"` exists.
4. Repeat with the repo selected.
5. Confirm the repo config exists and the created workspace references that repo.

Clean up:

```sh
rm -rf "$tmp_home"
```

---

## Self-Review

- Spec coverage:
  - Pending current repo appears in wizard: Task 2 and Task 3.
  - No write during draft construction or cancel: Task 2 tests no write; Task 4 only persists after submit.
  - Deselecting pending repo avoids config write: Task 4.
  - Selected pending repo persists before workspace save: Task 4.
  - Pending marker in repo list and review: Task 5.
  - Non-interactive unknown repo remains strict: Task 3 keeps explicit repo path on registered repo lookup, existing test remains.
- Placeholder scan:
  - No unresolved marker words or unspecified implementation steps remain.
- Type consistency:
  - `RepoDraftSource`, `CurrentRepoCandidate`, `discover_current_repo_candidate`, `persist_selected_pending_repos`, `repo_list_label`, and `review_repo_label` are introduced before later tasks use them.
