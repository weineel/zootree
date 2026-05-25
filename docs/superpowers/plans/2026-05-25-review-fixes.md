# Review Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the reviewed behavior gaps around rebase completion, template-based creation, file hook paths, and README command references.

**Architecture:** Keep changes inside existing modules. `GitOps` owns command sequencing for merge strategies, `workspace.rs` owns create-time repo entry resolution, `HookEngine` owns hook command construction, and README files mirror the implemented CLI surface.

**Tech Stack:** Rust 2021, clap, anyhow, shellexpand, existing `MockRunner` command tests, Cargo test and clippy.

---

## File Structure

- Modify `src/core/git.rs`: change only the `Some("rebase")` branch in `GitOps::merge`.
- Modify `tests/git_test.rs`: add a focused command-order test for the rebase merge strategy.
- Modify `src/cli/workspace.rs`: extract repo-entry construction helper and route `--repos`, `--template`, and interactive selection through clear branches.
- Modify `tests/config_test.rs`: add unit tests for the new helper because it can use `ConfigManager::with_base_dir` and `MockRunner` without driving the TUI.
- Modify `src/core/hook.rs`: expand tilde for `HookValue::File`.
- Modify `tests/hook_test.rs`: add tilde expansion coverage and keep simple/inline behavior unchanged.
- Modify `README.md` and `README.zh-CN.md`: remove unimplemented command references and document `template save`.

---

### Task 1: Fix Rebase Merge Strategy

**Files:**
- Modify: `src/core/git.rs`
- Test: `tests/git_test.rs`

- [ ] **Step 1: Add the failing rebase command-order test**

Add this test after `test_merge_command` in `tests/git_test.rs`:

```rust
#[test]
fn test_merge_rebase_command_order() {
    let runner = MockRunner::new();
    runner.push_response(success_output()); // checkout workspace branch
    runner.push_response(success_output()); // rebase target branch
    runner.push_response(success_output()); // checkout target branch
    runner.push_response(success_output()); // fast-forward target
    let git = GitOps::new(&runner);

    git.merge(
        "/home/user/projects/frontend",
        "zootree/calm-river",
        "develop",
        Some("rebase"),
        "unused for rebase",
    )
    .unwrap();

    let calls = runner.take_calls();
    assert_eq!(calls.len(), 4);
    assert_eq!(
        calls[0].args,
        vec![
            "-C",
            "/home/user/projects/frontend",
            "checkout",
            "zootree/calm-river"
        ]
    );
    assert_eq!(
        calls[1].args,
        vec!["-C", "/home/user/projects/frontend", "rebase", "develop"]
    );
    assert_eq!(
        calls[2].args,
        vec!["-C", "/home/user/projects/frontend", "checkout", "develop"]
    );
    assert_eq!(
        calls[3].args,
        vec![
            "-C",
            "/home/user/projects/frontend",
            "merge",
            "--ff-only",
            "zootree/calm-river"
        ]
    );
}
```

- [ ] **Step 2: Run the failing test**

Run:

```sh
cargo test --test git_test test_merge_rebase_command_order
```

Expected: FAIL because current code runs `checkout develop` then `rebase zootree/calm-river`.

- [ ] **Step 3: Implement the new rebase sequence**

In `src/core/git.rs`, change only this branch inside `GitOps::merge`:

```rust
Some("rebase") => {
    self.git(repo_path, vec!["checkout", branch])?;
    self.git(repo_path, vec!["rebase", target])?;
    self.git(repo_path, vec!["checkout", target])?;
    self.git(repo_path, vec!["merge", "--ff-only", branch])?;
}
```

Keep the existing initial checkout for non-rebase strategies by moving it into the `Some("merge")` and squash branches:

```rust
match strategy {
    Some("rebase") => {
        self.git(repo_path, vec!["checkout", branch])?;
        self.git(repo_path, vec!["rebase", target])?;
        self.git(repo_path, vec!["checkout", target])?;
        self.git(repo_path, vec!["merge", "--ff-only", branch])?;
    }
    Some("merge") => {
        self.git(repo_path, vec!["checkout", target])?;
        self.git(repo_path, vec!["merge", branch])?;
    }
    _ => {
        self.git(repo_path, vec!["checkout", target])?;
        self.git(repo_path, vec!["merge", "--squash", branch])?;
        let has_staged = {
            let mut args = vec!["-C".to_string(), repo_path.to_string()];
            args.extend(
                ["diff", "--staged", "--quiet"]
                    .iter()
                    .map(|s| s.to_string()),
            );
            let spec = CommandSpec {
                program: "git".into(),
                args,
                cwd: None,
                env: HashMap::new(),
            };
            let output = self.runner.run(&spec)?;
            !output.status.success()
        };
        if has_staged {
            self.git(repo_path, vec!["commit", "-m", message])?;
        } else {
            tracing::warn!("nothing to merge from '{}' into '{}'", branch, target);
        }
    }
}
```

- [ ] **Step 4: Verify git tests pass**

Run:

```sh
cargo test --test git_test
```

Expected: PASS for all git tests.

- [ ] **Step 5: Commit Task 1**

```sh
git add src/core/git.rs tests/git_test.rs
git commit -m "fix: make rebase merge strategy safe"
```

---

### Task 2: Make `create --template` Use Template Repos

**Files:**
- Modify: `src/cli/workspace.rs`
- Test: `tests/config_test.rs`

- [ ] **Step 1: Add failing helper tests**

At the top of `tests/config_test.rs`, extend imports:

```rust
use std::os::unix::process::ExitStatusExt;
use std::process::{ExitStatus, Output};
use tempfile::TempDir;
use zootree::cli::workspace::{build_repo_entries, parse_repos_arg};
use zootree::config::ConfigManager;
use zootree::runner::MockRunner;
```

Replace the existing `use zootree::cli::workspace::parse_repos_arg;` with the combined import above.

Add this helper and tests near the existing `parse_repos_arg` tests:

```rust
fn success_branch_output(branch: &str) -> Output {
    Output {
        status: ExitStatus::from_raw(0),
        stdout: format!("{}\n", branch).into_bytes(),
        stderr: Vec::new(),
    }
}

#[test]
fn build_repo_entries_prefers_explicit_branch() {
    let tmp = TempDir::new().unwrap();
    let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
    mgr.ensure_dirs().unwrap();
    mgr.save_repo_config(
        "frontend",
        &RepoConfig {
            path: "/repo/frontend".into(),
            default_target_branch: Some("develop".into()),
            copy_files: Vec::new(),
            hooks: Default::default(),
            lazygit: None,
            zellij: None,
        },
    )
    .unwrap();
    let runner = MockRunner::new();

    let entries = build_repo_entries(
        &mgr,
        &runner,
        vec![("frontend".to_string(), Some("release".to_string()))],
    )
    .unwrap();

    assert_eq!(entries[0].name, "frontend");
    assert_eq!(entries[0].target_branch.as_deref(), Some("release"));
    assert!(runner.take_calls().is_empty());
}

#[test]
fn build_repo_entries_uses_repo_default_branch() {
    let tmp = TempDir::new().unwrap();
    let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
    mgr.ensure_dirs().unwrap();
    mgr.save_repo_config(
        "frontend",
        &RepoConfig {
            path: "/repo/frontend".into(),
            default_target_branch: Some("develop".into()),
            copy_files: Vec::new(),
            hooks: Default::default(),
            lazygit: None,
            zellij: None,
        },
    )
    .unwrap();
    let runner = MockRunner::new();

    let entries = build_repo_entries(&mgr, &runner, vec![("frontend".to_string(), None)]).unwrap();

    assert_eq!(entries[0].target_branch.as_deref(), Some("develop"));
    assert!(runner.take_calls().is_empty());
}

#[test]
fn build_repo_entries_falls_back_to_current_branch() {
    let tmp = TempDir::new().unwrap();
    let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
    mgr.ensure_dirs().unwrap();
    mgr.save_repo_config(
        "frontend",
        &RepoConfig {
            path: "/repo/frontend".into(),
            default_target_branch: None,
            copy_files: Vec::new(),
            hooks: Default::default(),
            lazygit: None,
            zellij: None,
        },
    )
    .unwrap();
    let runner = MockRunner::new();
    runner.push_response(success_branch_output("mainline"));

    let entries = build_repo_entries(&mgr, &runner, vec![("frontend".to_string(), None)]).unwrap();

    assert_eq!(entries[0].target_branch.as_deref(), Some("mainline"));
    let calls = runner.take_calls();
    assert_eq!(
        calls[0].args,
        vec![
            "-C",
            "/repo/frontend",
            "rev-parse",
            "--abbrev-ref",
            "HEAD"
        ]
    );
}
```

- [ ] **Step 2: Run failing helper tests**

Run:

```sh
cargo test --test config_test build_repo_entries
```

Expected: FAIL because `build_repo_entries` does not exist.

- [ ] **Step 3: Extract and expose the helpers**

In `src/cli/workspace.rs`, add these functions after `parse_repos_arg`:

```rust
pub fn build_repo_entries<R: crate::runner::CommandRunner>(
    config_mgr: &ConfigManager,
    runner: &R,
    repos: Vec<(String, Option<String>)>,
) -> Result<Vec<RepoEntry>> {
    let git = GitOps::new(runner);
    let mut entries = Vec::new();

    for (name, branch) in repos {
        let repo_config = config_mgr.load_repo_config(&name)?;
        let repo_path = shellexpand::tilde(&repo_config.path).into_owned();
        let target_branch = branch
            .or(repo_config.default_target_branch.clone())
            .unwrap_or_else(|| {
                git.current_branch(&repo_path)
                    .unwrap_or_else(|_| "main".into())
            });
        entries.push(RepoEntry {
            name,
            target_branch: Some(target_branch),
        });
    }

    Ok(entries)
}

fn template_repos_to_entries_input(
    tmpl_name: &str,
    repos: Vec<String>,
) -> Result<Vec<(String, Option<String>)>> {
    if repos.is_empty() {
        anyhow::bail!("template '{}' has no repos", tmpl_name);
    }
    Ok(repos.into_iter().map(|name| (name, None)).collect())
}
```

- [ ] **Step 4: Route explicit `--repos` through the helper**

In `handle_create`, replace the full existing `let repo_entries` expression, from its `if let Some(repos_str)` branch through the final semicolon, with this complete block:

```rust
let repo_entries = if let Some(repos_str) = &args.repos {
    build_repo_entries(&config_mgr, &runner, parse_repos_arg(repos_str))?
} else if let Some(tmpl_name) = &args.template {
    let tmpl = config_mgr.load_template(tmpl_name)?;
    let repos = template_repos_to_entries_input(tmpl_name, tmpl.repos)?;
    build_repo_entries(&config_mgr, &runner, repos)?
} else {
    let all_repos = config_mgr.list_repos()?;
    if all_repos.is_empty() {
        anyhow::bail!("no repos registered. Use 'zootree repo add' first.");
    }

    let selected = tui::select_multi("Select repos", &all_repos)?;
    if selected.is_empty() {
        anyhow::bail!("at least one repo must be selected");
    }

    let mut entries = Vec::new();
    for idx in selected {
        let name = &all_repos[idx];
        let repo_config = config_mgr.load_repo_config(name)?;

        let repo_path = shellexpand::tilde(&repo_config.path).into_owned();
        let current = git
            .current_branch(&repo_path)
            .unwrap_or_else(|_| "main".into());
        let target_branch = if let Some(default) = &repo_config.default_target_branch {
            default.clone()
        } else {
            let input = tui::input_optional(&format!(
                "Target branch for {} (default: {})",
                name, current
            ))?;
            input.unwrap_or(current)
        };

        entries.push(RepoEntry {
            name: name.clone(),
            target_branch: Some(target_branch),
        });
    }
    entries
};
```

The interactive branch intentionally keeps its prompt behavior. It does not call `build_repo_entries` because that helper falls back to `main` when current branch lookup fails, while the interactive path currently asks the user for target branch when no repo default exists.

- [ ] **Step 5: Run helper tests**

Run:

```sh
cargo test --test config_test build_repo_entries
```

Expected: PASS.

- [ ] **Step 6: Add empty-template regression coverage at helper boundary**

Add this unit test inside `#[cfg(test)] mod tests` in `src/cli/workspace.rs`:

```rust
#[test]
fn template_repos_to_entries_input_errors_on_empty_template() {
    let result = template_repos_to_entries_input("empty", Vec::new());
    assert!(result.is_err());
    let msg = format!("{:#}", result.unwrap_err());
    assert!(msg.contains("template 'empty' has no repos"), "got: {}", msg);
}
```

- [ ] **Step 7: Run workspace unit tests**

Run:

```sh
cargo test cli::workspace::tests
```

Expected: PASS.

- [ ] **Step 8: Commit Task 2**

```sh
git add src/cli/workspace.rs tests/config_test.rs
git commit -m "fix: create workspaces from templates"
```

---

### Task 3: Expand Tilde for File Hooks

**Files:**
- Modify: `src/core/hook.rs`
- Test: `tests/hook_test.rs`

- [ ] **Step 1: Add failing tilde expansion test**

Add this test after `test_file_hook_command` in `tests/hook_test.rs`:

```rust
#[test]
fn test_file_hook_expands_tilde_path() {
    let runner = MockRunner::new();
    runner.push_response(success_output());
    let engine = HookEngine::new(&runner);

    let ctx = HookContext {
        workspace: "calm-river".into(),
        repo: None,
        branch: "zootree/calm-river".into(),
        target_branch: None,
        worktree_path: None,
        workspace_dir: "/home/user/ws/calm-river".into(),
    };

    let hook = HookValue::File {
        file: "~/.config/zootree/hooks/cleanup.sh".into(),
    };
    engine.execute(&hook, &ctx).unwrap();

    let calls = runner.take_calls();
    assert_eq!(calls[0].program, "sh");
    assert_ne!(calls[0].args[0], "~/.config/zootree/hooks/cleanup.sh");
    assert!(
        calls[0].args[0].ends_with("/.config/zootree/hooks/cleanup.sh"),
        "expanded path should keep hook suffix, got: {:?}",
        calls[0].args
    );
}
```

- [ ] **Step 2: Run failing hook test**

Run:

```sh
cargo test --test hook_test test_file_hook_expands_tilde_path
```

Expected: FAIL because the argument still starts with `~`.

- [ ] **Step 3: Implement file hook path expansion**

In `src/core/hook.rs`, change the `HookValue::File` match arm:

```rust
HookValue::File { file } => (
    "sh".to_string(),
    vec![shellexpand::tilde(file).into_owned()],
),
```

Leave `Simple` and `Inline` unchanged.

- [ ] **Step 4: Run hook tests**

Run:

```sh
cargo test --test hook_test
```

Expected: PASS.

- [ ] **Step 5: Commit Task 3**

```sh
git add src/core/hook.rs tests/hook_test.rs
git commit -m "fix: expand file hook paths"
```

---

### Task 4: Align README Command References

**Files:**
- Modify: `README.md`
- Modify: `README.zh-CN.md`

- [ ] **Step 1: Verify stale references exist**

Run:

```sh
rg "delete-remote|template show|template delete" README.md README.zh-CN.md
```

Expected: matches in both README files.

- [ ] **Step 2: Update English README**

In `README.md`, remove this line from the `done` command reference:

```md
  --delete-remote                    # Delete remote branches
```

Replace the template command block with:

```md
zootree template list                # List templates
zootree template save <name> --from <workspace>
                                    # Save a workspace as a template
```

- [ ] **Step 3: Update Chinese README**

In `README.zh-CN.md`, remove this line from the `done` command reference:

```md
  --delete-remote                    # 删除远程分支
```

Replace the template command block with:

```md
zootree template list                # 列出模板
zootree template save <name> --from <workspace>
                                    # 将 workspace 保存为模板
```

- [ ] **Step 4: Verify stale references are gone**

Run:

```sh
rg "delete-remote|template show|template delete" README.md README.zh-CN.md
```

Expected: exit code 1 with no matches.

- [ ] **Step 5: Commit Task 4**

```sh
git add README.md README.zh-CN.md
git commit -m "docs: align command reference with cli"
```

---

### Task 5: Final Verification

**Files:**
- Verify only unless a previous task left failures.

- [ ] **Step 1: Run all tests**

```sh
cargo test
```

Expected: PASS.

- [ ] **Step 2: Run clippy**

```sh
cargo clippy --all-targets -- -D warnings
```

Expected: PASS.

- [ ] **Step 3: Verify README cleanup**

```sh
rg "delete-remote|template show|template delete" README.md README.zh-CN.md
```

Expected: no output and exit code 1.

- [ ] **Step 4: Check final diff**

```sh
git status --short
git log --oneline -5
```

Expected: clean working tree after commits, with task commits present.
