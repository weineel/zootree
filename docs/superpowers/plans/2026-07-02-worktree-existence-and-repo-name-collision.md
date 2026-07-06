# Worktree Existence and Repo Name Collision Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Mark missing in-progress repo worktrees in display commands, fail early in commands that require those worktrees, and make `zootree repo add` duplicate names collision-safe.

**Architecture:** Add two small core helpers: one for unique repo names and one for runtime repo worktree status. Keep `ConfigManager` as config-only storage, and let CLI handlers decide whether to display, fail, or skip based on the runtime status.

**Tech Stack:** Rust, anyhow, std::path, existing `ConfigManager`, existing CLI modules, cargo tests.

---

## File Structure

- Create `src/core/repo_names.rs`: reusable `unique_repo_name` helper used by create flow and `repo add`.
- Create `src/core/worktree_status.rs`: reusable runtime status model for `<workspace_dir>/<repo_name>` paths.
- Modify `src/core/mod.rs`: export the two new core modules.
- Modify `src/cli/create_flow.rs`: remove the private `unique_repo_name` and import the shared helper.
- Modify `src/cli/repo.rs`: resolve duplicate `repo add` names before saving config.
- Modify `src/cli/workspace.rs`: attach worktree status to list formatting and enforce missing-worktree behavior in `open`, `done`, and `cancel`.
- Modify `src/cli/info.rs`: show per-repo worktree paths and `(missing)` for in-progress workspaces.
- Modify `README.md` and `README.zh-CN.md`: document repo name suffix behavior.
- Modify `skills/zootree-dev/SKILL.md`: add the new core files to the architecture tree because this repo requires skill docs to match structural code changes.

---

### Task 1: Extract Unique Repo Name Helper

**Files:**
- Create: `src/core/repo_names.rs`
- Modify: `src/core/mod.rs`
- Modify: `src/cli/create_flow.rs`
- Test: `src/core/repo_names.rs`

- [ ] **Step 1: Add failing tests for reusable unique repo names**

Create `src/core/repo_names.rs` with the helper tests first:

```rust
use std::collections::HashSet;

use anyhow::Result;

use crate::config::ConfigManager;

pub fn unique_repo_name(config_mgr: &ConfigManager, base: &str) -> Result<String> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::global::HooksConfig;
    use crate::config::repo::RepoConfig;

    fn repo_config(path: &str) -> RepoConfig {
        RepoConfig {
            path: path.into(),
            default_target_branch: None,
            copy_files: Vec::new(),
            hooks: HooksConfig::default(),
            lazygit: None,
            zellij: None,
        }
    }

    #[test]
    fn unique_repo_name_returns_base_when_unused() {
        let tmp = tempfile::tempdir().unwrap();
        let mgr = ConfigManager::with_base_dir(tmp.path().join("config"));
        mgr.ensure_dirs().unwrap();

        let name = unique_repo_name(&mgr, "zootree").unwrap();

        assert_eq!(name, "zootree");
    }

    #[test]
    fn unique_repo_name_appends_two_when_base_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let mgr = ConfigManager::with_base_dir(tmp.path().join("config"));
        mgr.ensure_dirs().unwrap();
        mgr.save_repo_config("zootree", &repo_config("/repo/one")).unwrap();

        let name = unique_repo_name(&mgr, "zootree").unwrap();

        assert_eq!(name, "zootree-2");
    }

    #[test]
    fn unique_repo_name_skips_existing_suffixes() {
        let tmp = tempfile::tempdir().unwrap();
        let mgr = ConfigManager::with_base_dir(tmp.path().join("config"));
        mgr.ensure_dirs().unwrap();
        mgr.save_repo_config("zootree", &repo_config("/repo/one")).unwrap();
        mgr.save_repo_config("zootree-2", &repo_config("/repo/two")).unwrap();

        let name = unique_repo_name(&mgr, "zootree").unwrap();

        assert_eq!(name, "zootree-3");
    }
}
```

- [ ] **Step 2: Export the module and run tests to verify the new helper builds**

Modify `src/core/mod.rs`:

```rust
pub mod completers;
pub mod copy_files;
pub mod git;
pub mod hook;
pub mod layout;
pub mod name_gen;
pub mod repo_names;
pub mod zellij;
```

Run: `cargo test unique_repo_name`

Expected: PASS for the three `unique_repo_name_*` tests.

- [ ] **Step 3: Replace create-flow private helper with the shared helper**

In `src/cli/create_flow.rs`, remove this import:

```rust
use std::collections::HashSet;
```

Add this import near the existing core imports:

```rust
use crate::core::repo_names::unique_repo_name;
```

Delete the private function currently shaped like:

```rust
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
```

No call sites need to change because the imported helper has the same name and signature.

- [ ] **Step 4: Verify create-flow collision tests still pass**

Run: `cargo test --test create_flow_test collision`

Expected: PASS, including:

```text
discover_current_repo_candidate_uses_collision_safe_pending_name
persist_selected_pending_repos_resolves_submit_time_name_collision
```

- [ ] **Step 5: Commit**

```bash
git add src/core/mod.rs src/core/repo_names.rs src/cli/create_flow.rs
git commit -m "refactor: share repo name collision helper"
```

---

### Task 2: Use Unique Names in `zootree repo add`

**Files:**
- Modify: `src/cli/repo.rs`
- Test: `src/cli/repo.rs`

- [ ] **Step 1: Add failing unit tests for repo add name resolution**

At the bottom of `src/cli/repo.rs`, add:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::global::HooksConfig;

    fn repo_config(path: &str) -> RepoConfig {
        RepoConfig {
            path: path.into(),
            default_target_branch: None,
            copy_files: Vec::new(),
            hooks: HooksConfig::default(),
            lazygit: None,
            zellij: None,
        }
    }

    #[test]
    fn repo_add_name_from_input_prefers_explicit_name() {
        let name = repo_add_base_name(&Some("custom".into()), "/tmp/zootree");

        assert_eq!(name, "custom");
    }

    #[test]
    fn repo_add_name_from_input_uses_path_basename() {
        let name = repo_add_base_name(&None, "/tmp/zootree");

        assert_eq!(name, "zootree");
    }

    #[test]
    fn repo_add_unique_name_appends_suffix_for_duplicate_base() {
        let tmp = tempfile::tempdir().unwrap();
        let mgr = ConfigManager::with_base_dir(tmp.path().join("config"));
        mgr.ensure_dirs().unwrap();
        mgr.save_repo_config("zootree", &repo_config("/repo/one")).unwrap();

        let name = resolve_repo_add_name(&mgr, &None, "/tmp/zootree").unwrap();

        assert_eq!(name, "zootree-2");
    }

    #[test]
    fn repo_add_unique_name_skips_existing_suffixes() {
        let tmp = tempfile::tempdir().unwrap();
        let mgr = ConfigManager::with_base_dir(tmp.path().join("config"));
        mgr.ensure_dirs().unwrap();
        mgr.save_repo_config("zootree", &repo_config("/repo/one")).unwrap();
        mgr.save_repo_config("zootree-2", &repo_config("/repo/two")).unwrap();

        let name = resolve_repo_add_name(&mgr, &None, "/tmp/zootree").unwrap();

        assert_eq!(name, "zootree-3");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail on missing helpers**

Run: `cargo test repo_add_`

Expected: FAIL with errors that mention missing functions:

```text
cannot find function `repo_add_base_name`
cannot find function `resolve_repo_add_name`
```

- [ ] **Step 3: Implement repo add name helpers and wire `repo add`**

In `src/cli/repo.rs`, add the shared helper import:

```rust
use crate::core::repo_names::unique_repo_name;
```

Add these private helpers above `handle_repo_command`:

```rust
fn repo_add_base_name(name: &Option<String>, path: &str) -> String {
    name.clone().unwrap_or_else(|| {
        std::path::Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| path.to_string())
    })
}

fn resolve_repo_add_name(
    config_mgr: &ConfigManager,
    name: &Option<String>,
    path: &str,
) -> Result<String> {
    let base = repo_add_base_name(name, path);
    unique_repo_name(config_mgr, &base)
}
```

In the `RepoCommands::Add` branch, replace the current `repo_name` derivation:

```rust
let repo_name = name.clone().unwrap_or_else(|| {
    std::path::Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.clone())
});
```

with:

```rust
let repo_name = resolve_repo_add_name(&config_mgr, name, path)?;
```

Keep the existing save and print line, because it already prints the resolved `repo_name`.

- [ ] **Step 4: Verify repo add tests pass**

Run: `cargo test repo_add_`

Expected: PASS for all `repo_add_*` tests.

- [ ] **Step 5: Commit**

```bash
git add src/cli/repo.rs
git commit -m "feat: avoid repo add name collisions"
```

---

### Task 3: Add Runtime Repo Worktree Status Helper

**Files:**
- Create: `src/core/worktree_status.rs`
- Modify: `src/core/mod.rs`
- Test: `src/core/worktree_status.rs`

- [ ] **Step 1: Add helper and tests**

Create `src/core/worktree_status.rs`:

```rust
use std::path::Path;

use crate::config::workspace::WorkspaceConfig;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoWorktreeStatus {
    pub repo_name: String,
    pub worktree_path: String,
    pub exists: bool,
}

pub fn repo_worktree_statuses(
    workspace: &WorkspaceConfig,
    workspace_dir: &str,
) -> Vec<RepoWorktreeStatus> {
    workspace
        .repos
        .iter()
        .map(|repo| {
            let worktree_path = format!("{}/{}", workspace_dir, repo.name);
            RepoWorktreeStatus {
                repo_name: repo.name.clone(),
                exists: Path::new(&worktree_path).exists(),
                worktree_path,
            }
        })
        .collect()
}

pub fn missing_worktrees(statuses: &[RepoWorktreeStatus]) -> Vec<&RepoWorktreeStatus> {
    statuses.iter().filter(|status| !status.exists).collect()
}

pub fn format_missing_worktrees_error(workspace_name: &str, statuses: &[RepoWorktreeStatus]) -> String {
    let missing = missing_worktrees(statuses);
    let details = missing
        .iter()
        .map(|status| format!("{} ({})", status.repo_name, status.worktree_path))
        .collect::<Vec<_>>()
        .join(", ");
    format!("workspace '{}' is missing worktrees: {}", workspace_name, details)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::global::ZellijConfig;
    use crate::config::workspace::{RepoEntry, WorkspaceConfig};

    fn workspace(workspace_dir: &str) -> WorkspaceConfig {
        WorkspaceConfig {
            title: "Demo".into(),
            name: "demo".into(),
            description: String::new(),
            branch: "zootree/demo".into(),
            workspace_dir: workspace_dir.into(),
            created_at: "2026-07-02T10:00:00+08:00".into(),
            agent_cli: None,
            zellij: ZellijConfig::default(),
            repos: vec![
                RepoEntry {
                    name: "frontend".into(),
                    target_branch: Some("main".into()),
                },
                RepoEntry {
                    name: "backend".into(),
                    target_branch: Some("main".into()),
                },
            ],
            events: Vec::new(),
        }
    }

    #[test]
    fn repo_worktree_statuses_reports_existing_and_missing_repos() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("frontend")).unwrap();
        let ws = workspace(&tmp.path().to_string_lossy());

        let statuses = repo_worktree_statuses(&ws, &ws.workspace_dir);

        assert_eq!(statuses[0].repo_name, "frontend");
        assert!(statuses[0].exists);
        assert_eq!(statuses[1].repo_name, "backend");
        assert!(!statuses[1].exists);
    }

    #[test]
    fn repo_worktree_statuses_reports_all_missing_when_workspace_dir_is_absent() {
        let tmp = tempfile::tempdir().unwrap();
        let missing_dir = tmp.path().join("missing");
        let ws = workspace(&missing_dir.to_string_lossy());

        let statuses = repo_worktree_statuses(&ws, &ws.workspace_dir);

        assert_eq!(missing_worktrees(&statuses).len(), 2);
    }

    #[test]
    fn format_missing_worktrees_error_lists_repo_names_and_paths() {
        let statuses = vec![RepoWorktreeStatus {
            repo_name: "backend".into(),
            worktree_path: "/tmp/demo/backend".into(),
            exists: false,
        }];

        let message = format_missing_worktrees_error("demo", &statuses);

        assert_eq!(
            message,
            "workspace 'demo' is missing worktrees: backend (/tmp/demo/backend)"
        );
    }
}
```

- [ ] **Step 2: Export the module**

Modify `src/core/mod.rs`:

```rust
pub mod completers;
pub mod copy_files;
pub mod git;
pub mod hook;
pub mod layout;
pub mod name_gen;
pub mod repo_names;
pub mod worktree_status;
pub mod zellij;
```

- [ ] **Step 3: Run helper tests**

Run: `cargo test worktree_status`

Expected: PASS for the helper tests in `src/core/worktree_status.rs`.

- [ ] **Step 4: Commit**

```bash
git add src/core/mod.rs src/core/worktree_status.rs
git commit -m "feat: add repo worktree status helper"
```

---

### Task 4: Mark Missing Worktrees in `zootree list`

**Files:**
- Modify: `src/cli/workspace.rs`
- Test: `src/cli/workspace.rs`

- [ ] **Step 1: Add failing formatter tests**

In `src/cli/workspace.rs` test module, add this helper:

```rust
fn missing_worktree(repo_name: &str, worktree_path: &str) -> RepoWorktreeStatus {
    RepoWorktreeStatus {
        repo_name: repo_name.into(),
        worktree_path: worktree_path.into(),
        exists: false,
    }
}
```

Add these tests:

```rust
#[test]
fn render_list_cards_shows_missing_worktrees_for_in_progress_workspace() {
    let mut item = list_workspace(
        WorkspaceStatus::InProgress,
        "live-clay",
        "Fix worktree checks",
        "zootree/live-clay",
        "/tmp/live-clay",
        vec![repo("zootree", Some("main")), repo("docs", Some("main"))],
    );
    item.worktrees = vec![missing_worktree("docs", "/tmp/live-clay/docs")];

    let out = render_list_cards(&[item]);

    assert!(out.contains("  missing worktrees: docs"), "{out}");
}

#[test]
fn render_list_oneline_shows_missing_worktrees_for_in_progress_workspace() {
    let mut item = list_workspace(
        WorkspaceStatus::InProgress,
        "live-clay",
        "Fix worktree checks",
        "zootree/live-clay",
        "/tmp/live-clay",
        vec![repo("zootree", Some("main")), repo("docs", Some("main"))],
    );
    item.worktrees = vec![missing_worktree("docs", "/tmp/live-clay/docs")];

    let out = render_list_oneline(&[item]);

    assert!(
        out.contains("/tmp/live-clay [missing: docs]"),
        "{out}"
    );
}
```

- [ ] **Step 2: Run formatter tests to verify they fail**

Run: `cargo test render_list_`

Expected: FAIL because `ListWorkspaceItem` has no `worktrees` field and `RepoWorktreeStatus` is not imported.

- [ ] **Step 3: Add worktree status to list items**

In `src/cli/workspace.rs`, add imports:

```rust
use crate::core::worktree_status::{
    missing_worktrees, repo_worktree_statuses, RepoWorktreeStatus,
};
```

Change `ListWorkspaceItem`:

```rust
#[derive(Debug, Clone, PartialEq)]
struct ListWorkspaceItem {
    status: WorkspaceStatus,
    workspace: WorkspaceConfig,
    worktrees: Vec<RepoWorktreeStatus>,
}
```

In `handle_list`, replace the item construction with:

```rust
let mut items = Vec::with_capacity(workspaces.len());
for ws in workspaces {
    let (status, _) = config_mgr.load_workspace(&ws.name)?;
    let worktrees = if matches!(status, WorkspaceStatus::InProgress) {
        let ws_dir = shellexpand::tilde(&ws.workspace_dir).into_owned();
        repo_worktree_statuses(&ws, &ws_dir)
    } else {
        Vec::new()
    };
    items.push(ListWorkspaceItem {
        status,
        workspace: ws,
        worktrees,
    });
}
```

Add this formatter helper near `format_repo_targets`:

```rust
fn format_missing_worktree_names(worktrees: &[RepoWorktreeStatus]) -> Option<String> {
    let names = missing_worktrees(worktrees)
        .iter()
        .map(|status| status.repo_name.as_str())
        .collect::<Vec<_>>();
    if names.is_empty() {
        None
    } else {
        Some(names.join(", "))
    }
}
```

In `render_list_oneline`, change the in-progress branch to:

```rust
let missing = format_missing_worktree_names(&item.worktrees)
    .map(|names| format!(" [missing: {}]", names))
    .unwrap_or_default();
out.push_str(&format!(
    "  {} ({}) - {} [{}] {}{}\n",
    ws.name, status_str, ws.title, repos_str, ws.workspace_dir, missing
));
```

In `render_list_cards`, after the `dir:` line, add:

```rust
if let Some(names) = format_missing_worktree_names(&item.worktrees) {
    out.push_str(&format!("  missing worktrees: {}\n", names));
}
```

Update the `list_workspace` test helper to initialize `worktrees: Vec::new()`:

```rust
ListWorkspaceItem {
    status,
    workspace: WorkspaceConfig {
        title: title.into(),
        name: name.into(),
        description: String::new(),
        branch: branch.into(),
        workspace_dir: workspace_dir.into(),
        created_at: "2026-06-23T10:00:00+08:00".into(),
        agent_cli: None,
        zellij: ZellijConfig::default(),
        repos,
        events: Vec::new(),
    },
    worktrees: Vec::new(),
}
```

- [ ] **Step 4: Verify list formatter tests pass**

Run: `cargo test render_list_`

Expected: PASS for all list formatter tests, including existing legacy `--oneline` tests.

- [ ] **Step 5: Commit**

```bash
git add src/cli/workspace.rs
git commit -m "feat: mark missing worktrees in list"
```

---

### Task 5: Mark Missing Worktrees in `zootree info`

**Files:**
- Modify: `src/cli/info.rs`
- Test: `tests/info_test.rs`

- [ ] **Step 1: Add failing info tests**

Append to `tests/info_test.rs`:

```rust
#[test]
fn render_once_marks_missing_in_progress_repo_worktree() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir(tmp.path().join("frontend")).unwrap();
    let mut ws = base_ws();
    ws.workspace_dir = tmp.path().to_string_lossy().into_owned();
    ws.repos = vec![
        RepoEntry {
            name: "frontend".into(),
            target_branch: Some("main".into()),
        },
        RepoEntry {
            name: "backend".into(),
            target_branch: Some("main".into()),
        },
    ];

    let out = render_once(&WorkspaceStatus::InProgress, &ws, &GlobalConfig::default());

    assert!(out.contains("frontend"), "{out}");
    assert!(out.contains(&format!("{}/frontend", ws.workspace_dir)), "{out}");
    assert!(out.contains(&format!("{}/backend (missing)", ws.workspace_dir)), "{out}");
}

#[test]
fn render_once_omits_worktree_paths_for_non_in_progress_workspace() {
    let mut ws = base_ws();
    ws.workspace_dir = "/tmp/demo".into();
    ws.repos = vec![RepoEntry {
        name: "frontend".into(),
        target_branch: Some("main".into()),
    }];

    let out = render_once(&WorkspaceStatus::Pending, &ws, &GlobalConfig::default());

    assert!(out.contains("  - frontend"), "{out}");
    assert!(!out.contains("/tmp/demo/frontend"), "{out}");
}
```

- [ ] **Step 2: Run info tests to verify they fail**

Run: `cargo test --test info_test render_once_`

Expected: FAIL for `render_once_marks_missing_in_progress_repo_worktree` because repo rows do not include worktree paths or `(missing)`.

- [ ] **Step 3: Implement in-progress repo worktree rendering**

In `src/cli/info.rs`, add:

```rust
use crate::core::worktree_status::repo_worktree_statuses;
```

Replace the repo loop in `render_once`:

```rust
for r in &ws.repos {
    let target = r.target_branch.as_deref().unwrap_or("*");
    let _ = writeln!(out, "  - {:<15} -> {}", r.name, target);
}
```

with:

```rust
let worktrees = if matches!(status, WorkspaceStatus::InProgress) {
    let ws_dir = shellexpand::tilde(&ws.workspace_dir).into_owned();
    repo_worktree_statuses(ws, &ws_dir)
} else {
    Vec::new()
};

for r in &ws.repos {
    let target = r.target_branch.as_deref().unwrap_or("*");
    if let Some(worktree) = worktrees.iter().find(|status| status.repo_name == r.name) {
        let missing = if worktree.exists { "" } else { " (missing)" };
        let _ = writeln!(
            out,
            "  - {:<15} -> {}  {}{}",
            r.name, target, worktree.worktree_path, missing
        );
    } else {
        let _ = writeln!(out, "  - {:<15} -> {}", r.name, target);
    }
}
```

- [ ] **Step 4: Verify info tests pass**

Run: `cargo test --test info_test`

Expected: PASS for all info tests.

- [ ] **Step 5: Commit**

```bash
git add src/cli/info.rs tests/info_test.rs
git commit -m "feat: mark missing worktrees in info"
```

---

### Task 6: Fail or Skip Before Worktree-Dependent Execution

**Files:**
- Modify: `src/cli/workspace.rs`
- Test: `src/cli/workspace.rs`

- [ ] **Step 1: Add failing helper tests for execution checks**

In the `src/cli/workspace.rs` test module, add:

```rust
#[test]
fn ensure_required_worktrees_exist_allows_existing_worktrees() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir(tmp.path().join("zootree")).unwrap();
    let ws = test_workspace("live-clay");
    let mut ws = WorkspaceConfig {
        workspace_dir: tmp.path().to_string_lossy().into_owned(),
        repos: vec![repo("zootree", Some("main"))],
        ..ws
    };

    let result = ensure_required_worktrees_exist(&ws);

    assert!(result.is_ok());
    ws.repos.clear();
}

#[test]
fn ensure_required_worktrees_exist_reports_missing_worktrees() {
    let tmp = tempfile::tempdir().unwrap();
    let ws = test_workspace("live-clay");
    let ws = WorkspaceConfig {
        workspace_dir: tmp.path().to_string_lossy().into_owned(),
        repos: vec![repo("zootree", Some("main"))],
        ..ws
    };

    let err = ensure_required_worktrees_exist(&ws).unwrap_err();

    assert!(
        err.to_string()
            .contains("workspace 'live-clay' is missing worktrees: zootree"),
        "{err:#}"
    );
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test ensure_required_worktrees_exist`

Expected: FAIL because `ensure_required_worktrees_exist` is not defined.

- [ ] **Step 3: Add execution helper**

In `src/cli/workspace.rs`, extend the worktree status import:

```rust
use crate::core::worktree_status::{
    format_missing_worktrees_error, missing_worktrees, repo_worktree_statuses, RepoWorktreeStatus,
};
```

Add this helper near `warn_or_bail`:

```rust
fn expanded_workspace_dir(workspace: &WorkspaceConfig) -> String {
    shellexpand::tilde(&workspace.workspace_dir).into_owned()
}

fn ensure_required_worktrees_exist(workspace: &WorkspaceConfig) -> Result<()> {
    let ws_dir = expanded_workspace_dir(workspace);
    let statuses = repo_worktree_statuses(workspace, &ws_dir);
    if missing_worktrees(&statuses).is_empty() {
        Ok(())
    } else {
        anyhow::bail!("{}", format_missing_worktrees_error(&workspace.name, &statuses))
    }
}
```

Replace existing `shellexpand::tilde(&workspace.workspace_dir).into_owned()` calls in `handle_done` and `handle_cancel` with `expanded_workspace_dir(&workspace)` when the local variable is named `ws_dir`.

- [ ] **Step 4: Guard `open` before launching zellij**

In `handle_open`, after the status check and before `launch_zellij`, add:

```rust
ensure_required_worktrees_exist(&workspace)?;
```

The `handle_open` body should keep the existing status check:

```rust
if !matches!(status, WorkspaceStatus::InProgress) {
    anyhow::bail!("workspace '{}' is not in_progress", name);
}

ensure_required_worktrees_exist(&workspace)?;

launch_zellij(
    &config_mgr,
    &global,
    &workspace,
    &runner,
    None,
    crate::core::zellij::is_inside_zellij(),
)?;
```

- [ ] **Step 5: Guard `done` after dry-run and before hooks**

In `handle_done`, keep the current `--dry-run` block before the required-worktree check. Immediately after the dry-run `return Ok(())` block, add:

```rust
ensure_required_worktrees_exist(&workspace)?;
```

This preserves the spec requirement that `--dry-run` does not fail on missing worktrees.

- [ ] **Step 6: Make `cancel` skip missing repo cleanup with a visible warning**

In `handle_cancel`, after:

```rust
let ws_dir = expanded_workspace_dir(&workspace);
```

add:

```rust
let worktree_statuses = repo_worktree_statuses(&workspace, &ws_dir);
```

In the uncommitted-change loop, replace the `Path::new(&worktree_path).exists()` check with a lookup:

```rust
let worktree = worktree_statuses
    .iter()
    .find(|status| status.repo_name == repo_entry.name);
if worktree.is_some_and(|status| status.exists)
    && git.has_uncommitted_changes(&worktree_path)?
    && !tui::confirm(
        &format!(
            "repo '{}' has uncommitted changes. Continue?",
            repo_entry.name
        ),
        false,
    )?
{
    anyhow::bail!("canceled by user");
}
```

In the cleanup loop, after `worktree_path` is computed and before `pre_remove`, add:

```rust
let worktree = worktree_statuses
    .iter()
    .find(|status| status.repo_name == repo_entry.name);
if worktree.is_some_and(|status| !status.exists) {
    println!(
        "  missing worktree: {} ({})",
        repo_entry.name, worktree_path
    );
    continue;
}
```

Keep the existing final workspace directory removal:

```rust
if Path::new(&ws_dir).exists() {
    if let Err(e) = std::fs::remove_dir_all(&ws_dir) {
        warn_or_bail(args.force, e.into(), "failed to remove workspace directory")?;
    }
}
```

- [ ] **Step 7: Verify execution helper tests pass**

Run: `cargo test ensure_required_worktrees_exist`

Expected: PASS for both helper tests.

- [ ] **Step 8: Verify workspace tests still pass**

Run: `cargo test --lib workspace`

Expected: PASS for the workspace module unit tests.

- [ ] **Step 9: Commit**

```bash
git add src/cli/workspace.rs
git commit -m "feat: guard commands against missing worktrees"
```

---

### Task 7: Update User Documentation and zootree Skill Metadata

**Files:**
- Modify: `README.md`
- Modify: `README.zh-CN.md`
- Modify: `skills/zootree-dev/SKILL.md`

- [ ] **Step 1: Document duplicate repo add naming in English README**

In `README.md`, near the repo add examples, add:

```markdown
If the derived or explicit repo name already exists, `repo add` keeps the
existing repo config and registers the new repo with the next available suffix
such as `myrepo-2`, `myrepo-3`, and so on.
```

- [ ] **Step 2: Document duplicate repo add naming in Chinese README**

In `README.zh-CN.md`, near the repo add examples, add:

```markdown
如果推导出的仓库名或 `--name` 指定的仓库名已经存在，`repo add` 不会覆盖原配置，
而是使用下一个可用后缀注册新仓库，例如 `myrepo-2`、`myrepo-3`。
```

- [ ] **Step 3: Update zootree-dev architecture tree**

In `skills/zootree-dev/SKILL.md`, update the `core/` section so it includes:

```text
│   ├── name_gen.rs  # 工作空间名称生成器
│   ├── repo_names.rs # repo 名称冲突处理
│   ├── worktree_status.rs # workspace repo worktree 路径存在性检查
│   └── completers.rs # 动态补全候选生成器 (workspace/repo/template)
```

Keep the rest of the architecture tree unchanged.

- [ ] **Step 4: Run docs-oriented checks**

Run:

```bash
rg -n "myrepo-2|repo_names|worktree_status" README.md README.zh-CN.md skills/zootree-dev/SKILL.md
```

Expected: output includes the new README lines and the new skill architecture entries.

- [ ] **Step 5: Commit**

```bash
git add README.md README.zh-CN.md skills/zootree-dev/SKILL.md
git commit -m "docs: document repo add collision handling"
```

---

### Task 8: Final Verification

**Files:**
- Verify all changed files.

- [ ] **Step 1: Run focused tests**

Run:

```bash
cargo test unique_repo_name
cargo test repo_add_
cargo test worktree_status
cargo test render_list_
cargo test --test info_test
cargo test ensure_required_worktrees_exist
```

Expected: all commands exit successfully.

- [ ] **Step 2: Run the full test suite**

Run:

```bash
cargo test
```

Expected: all tests pass.

- [ ] **Step 3: Run cargo check**

Run:

```bash
cargo check
```

Expected: command exits successfully without compile errors.

- [ ] **Step 4: Inspect git history and status**

Run:

```bash
git status --short
git log --oneline -8
```

Expected: `git status --short` is empty. Recent commits include:

```text
docs: document repo add collision handling
feat: guard commands against missing worktrees
feat: mark missing worktrees in info
feat: mark missing worktrees in list
feat: add repo worktree status helper
feat: avoid repo add name collisions
refactor: share repo name collision helper
```

## Self-Review

- Spec coverage: Tasks 3-6 cover display and execution worktree checks; Tasks 1-2 cover repo name collision behavior; Task 7 covers user docs and required zootree skill metadata.
- Placeholder scan: The plan contains concrete file paths, code snippets, commands, expected results, and commit commands.
- Type consistency: `RepoWorktreeStatus`, `repo_worktree_statuses`, `missing_worktrees`, `format_missing_worktrees_error`, and `unique_repo_name` are defined before any task uses them.
