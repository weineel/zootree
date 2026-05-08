# --force / --skip-hooks 行为重构 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 拆分 `--force` 和 hook 跳过的职责，新增 `--skip-hooks`，让 `--force` 只负责把清理流程中的报错降级为 warn。

**Architecture:** 新增 `warn_or_bail` 辅助函数统一处理"force 时 warn、非 force 时 bail + 提示 --force"；`git.rs` merge 函数在 squash 策略下检测 staged 变更，无变更时 warn 而非报错；`handle_done`/`handle_cancel` 中 hook 执行和清理步骤统一改用新语义。

**Tech Stack:** Rust, clap (CLI args), anyhow (错误处理), tracing (warn 日志), MockRunner (测试)

---

## File Map

- Modify: `src/cli/workspace.rs` — 新增 `skip_hooks` flag、`warn_or_bail`，更新 `handle_done`/`handle_cancel`
- Modify: `src/core/git.rs` — merge squash 策略检测 staged 变更
- Modify: `tests/git_test.rs` — 更新 `test_merge_squash`，新增 nothing-to-merge 测试

---

### Task 1: `warn_or_bail` 辅助函数

**Files:**
- Modify: `src/cli/workspace.rs`

- [ ] **Step 1: 在 `src/cli/workspace.rs` 末尾添加测试模块，写两个失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warn_or_bail_with_force_returns_ok() {
        let err = anyhow::anyhow!("hook failed");
        let result = warn_or_bail(true, err, "pre_done hook");
        assert!(result.is_ok());
    }

    #[test]
    fn warn_or_bail_without_force_returns_err_with_hint() {
        let err = anyhow::anyhow!("hook failed");
        let result = warn_or_bail(false, err, "pre_done hook");
        assert!(result.is_err());
        let msg = format!("{:#}", result.unwrap_err());
        assert!(msg.contains("use --force to proceed anyway"), "got: {}", msg);
    }
}
```

- [ ] **Step 2: 运行测试，确认编译失败（函数未定义）**

```bash
cargo test warn_or_bail 2>&1 | head -20
```

Expected: `error[E0425]: cannot find function warn_or_bail`

- [ ] **Step 3: 在 `src/cli/workspace.rs` 中 `handle_done` 前添加函数**

```rust
fn warn_or_bail(force: bool, err: anyhow::Error, context: &str) -> Result<()> {
    if force {
        tracing::warn!("{}: {:#}", context, err);
        Ok(())
    } else {
        Err(err.context(format!("{} (use --force to proceed anyway)", context)))
    }
}
```

- [ ] **Step 4: 运行测试，确认通过**

```bash
cargo test warn_or_bail
```

Expected: `test tests::warn_or_bail_with_force_returns_ok ... ok` 和 `warn_or_bail_without_force_returns_err_with_hint ... ok`

- [ ] **Step 5: Commit**

```bash
git add src/cli/workspace.rs
git commit -m "feat: add warn_or_bail helper for force-mode error handling"
```

---

### Task 2: git.rs merge squash 无内容时 warn

**Files:**
- Modify: `src/core/git.rs`
- Modify: `tests/git_test.rs`

- [ ] **Step 1: 更新 `tests/git_test.rs` 中的 `test_merge_squash`，加入 `diff --staged --quiet` 响应**

将原测试改为（新增第三个 push_response，commit 改为第四个）：

```rust
#[test]
fn test_merge_squash() {
    let runner = MockRunner::new();
    runner.push_response(success_output()); // checkout
    runner.push_response(success_output()); // merge --squash
    runner.push_response(Output {           // diff --staged --quiet: exit 1 = has staged changes
        status: ExitStatus::from_raw(256),
        stdout: Vec::new(),
        stderr: Vec::new(),
    });
    runner.push_response(success_output()); // commit
    let git = GitOps::new(&runner);

    git.merge(
        "/home/user/projects/frontend",
        "zootree/calm-river",
        "develop",
        Some("squash"),
        "fix: resolve login issue",
    ).unwrap();

    let calls = runner.take_calls();
    assert_eq!(calls.len(), 4);
    assert_eq!(calls[1].args, vec!["-C", "/home/user/projects/frontend", "merge", "--squash", "zootree/calm-river"]);
    assert_eq!(calls[2].args, vec!["-C", "/home/user/projects/frontend", "diff", "--staged", "--quiet"]);
    assert_eq!(calls[3].args, vec!["-C", "/home/user/projects/frontend", "commit", "-m", "fix: resolve login issue"]);
}
```

- [ ] **Step 2: 在 `tests/git_test.rs` 新增 nothing-to-merge 测试**

```rust
#[test]
fn test_merge_squash_nothing_to_merge() {
    let runner = MockRunner::new();
    runner.push_response(success_output()); // checkout
    runner.push_response(success_output()); // merge --squash
    runner.push_response(success_output()); // diff --staged --quiet: exit 0 = nothing staged
    let git = GitOps::new(&runner);

    git.merge(
        "/home/user/projects/frontend",
        "zootree/calm-river",
        "develop",
        None, // default = squash
        "fix: resolve login issue",
    ).unwrap();

    let calls = runner.take_calls();
    // commit should NOT be called
    assert_eq!(calls.len(), 3);
    assert_eq!(calls[2].args, vec!["-C", "/home/user/projects/frontend", "diff", "--staged", "--quiet"]);
}
```

- [ ] **Step 3: 运行测试，确认失败（逻辑未改）**

```bash
cargo test test_merge_squash
```

Expected: 两个测试失败（调用次数不对）

- [ ] **Step 4: 修改 `src/core/git.rs` 的 merge squash 分支**

将原来的：
```rust
_ => {
    // 默认使用 squash 方式
    self.git(repo_path, vec!["merge", "--squash", branch])?;
    self.git(repo_path, vec!["commit", "-m", message])?;
}
```

改为：

```rust
_ => {
    // 默认使用 squash 方式
    self.git(repo_path, vec!["merge", "--squash", branch])?;
    // exit 1 表示有 staged 变更，exit 0 表示无变更（已是最新）
    let has_staged = {
        let mut args = vec!["-C".to_string(), repo_path.to_string()];
        args.extend(["diff", "--staged", "--quiet"].iter().map(|s| s.to_string()));
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
```

- [ ] **Step 5: 运行测试，确认通过**

```bash
cargo test test_merge_squash
```

Expected: `test_merge_squash ... ok`, `test_merge_squash_nothing_to_merge ... ok`

- [ ] **Step 6: 运行全量测试确认无回归**

```bash
cargo test
```

Expected: all tests pass

- [ ] **Step 7: Commit**

```bash
git add src/core/git.rs tests/git_test.rs
git commit -m "feat: warn instead of error when squash merge has nothing to commit"
```

---

### Task 3: 新增 `--skip-hooks` flag，更新 `--force` help text

**Files:**
- Modify: `src/cli/workspace.rs` — `DoneArgs`（约 470 行）和 `CancelArgs`（约 481 行）

- [ ] **Step 1: 在 `DoneArgs` 中更新 `--force` 并新增 `--skip-hooks`**

找到（约 475 行）：
```rust
#[arg(long, help = "Skip hooks and uncommitted-changes check")]
pub force: bool,
```

改为：
```rust
#[arg(long, help = "Continue even if steps fail (errors become warnings)")]
pub force: bool,
#[arg(long, help = "Skip all hooks (pre_done/pre_remove)")]
pub skip_hooks: bool,
```

- [ ] **Step 2: 在 `CancelArgs` 中更新 `--force` 并新增 `--skip-hooks`**

找到（约 487 行）：
```rust
#[arg(long, help = "Skip hooks and confirmation prompts")]
pub force: bool,
```

改为：
```rust
#[arg(long, help = "Continue even if steps fail (errors become warnings)")]
pub force: bool,
#[arg(long, help = "Skip all hooks (pre_cancel/pre_remove)")]
pub skip_hooks: bool,
```

- [ ] **Step 3: 编译确认无错误**

```bash
cargo build 2>&1 | head -30
```

Expected: 编译成功（可能有 unused field warning，后续任务会用到）

- [ ] **Step 4: Commit**

```bash
git add src/cli/workspace.rs
git commit -m "feat: add --skip-hooks flag to done/cancel, update --force help text"
```

---

### Task 4: 更新 `handle_done`

**Files:**
- Modify: `src/cli/workspace.rs`

三处改动：pre_done hook、pre_remove hook、remove_dir_all。

- [ ] **Step 1: 更新 pre_done hook（约 532 行）**

将：
```rust
if !args.force {
    hook_engine.execute_if_set(&global.hooks.pre_done, &HookContext {
        workspace: workspace.name.clone(),
        repo: None,
        branch: workspace.branch.clone(),
        target_branch: None,
        worktree_path: None,
        workspace_dir: ws_dir.clone(),
    })?;
}
```

改为：
```rust
if !args.skip_hooks {
    if let Err(e) = hook_engine.execute_if_set(&global.hooks.pre_done, &HookContext {
        workspace: workspace.name.clone(),
        repo: None,
        branch: workspace.branch.clone(),
        target_branch: None,
        worktree_path: None,
        workspace_dir: ws_dir.clone(),
    }) {
        warn_or_bail(args.force, e, "pre_done hook failed")?;
    }
}
```

- [ ] **Step 2: 更新 pre_remove hook（约 603 行）**

将：
```rust
if let Some(h) = hook {
    if !args.force {
        hook_engine.execute(h, &HookContext {
            workspace: workspace.name.clone(),
            repo: Some(repo_entry.name.clone()),
            branch: workspace.branch.clone(),
            target_branch: Some(target_branch.clone()),
            worktree_path: Some(worktree_path.clone()),
            workspace_dir: ws_dir.clone(),
        })?;
    }
}
```

改为：
```rust
if let Some(h) = hook {
    if !args.skip_hooks {
        if let Err(e) = hook_engine.execute(h, &HookContext {
            workspace: workspace.name.clone(),
            repo: Some(repo_entry.name.clone()),
            branch: workspace.branch.clone(),
            target_branch: Some(target_branch.clone()),
            worktree_path: Some(worktree_path.clone()),
            workspace_dir: ws_dir.clone(),
        }) {
            warn_or_bail(args.force, e, "pre_remove hook failed")?;
        }
    }
}
```

- [ ] **Step 3: 更新 remove_dir_all（约 628 行）**

将：
```rust
if Path::new(&ws_dir).exists() {
    std::fs::remove_dir_all(&ws_dir)?;
}
```

改为：
```rust
if Path::new(&ws_dir).exists() {
    if let Err(e) = std::fs::remove_dir_all(&ws_dir) {
        warn_or_bail(args.force, e.into(), "failed to remove workspace directory")?;
    }
}
```

- [ ] **Step 4: 编译确认无错误**

```bash
cargo build 2>&1 | head -30
```

Expected: 编译成功

- [ ] **Step 5: 运行全量测试确认无回归**

```bash
cargo test
```

Expected: all tests pass

- [ ] **Step 6: Commit**

```bash
git add src/cli/workspace.rs
git commit -m "feat: update handle_done to use skip_hooks/warn_or_bail semantics"
```

---

### Task 5: 更新 `handle_cancel`

**Files:**
- Modify: `src/cli/workspace.rs`

三处改动：pre_cancel hook、pre_remove hook、remove_dir_all。

- [ ] **Step 1: 更新 pre_cancel hook（约 703 行）**

将：
```rust
if !args.force {
    hook_engine.execute_if_set(&global.hooks.pre_cancel, &HookContext {
        workspace: workspace.name.clone(),
        repo: None,
        branch: workspace.branch.clone(),
        target_branch: None,
        worktree_path: None,
        workspace_dir: ws_dir.clone(),
    })?;
}
```

改为：
```rust
if !args.skip_hooks {
    if let Err(e) = hook_engine.execute_if_set(&global.hooks.pre_cancel, &HookContext {
        workspace: workspace.name.clone(),
        repo: None,
        branch: workspace.branch.clone(),
        target_branch: None,
        worktree_path: None,
        workspace_dir: ws_dir.clone(),
    }) {
        warn_or_bail(args.force, e, "pre_cancel hook failed")?;
    }
}
```

- [ ] **Step 2: 更新 pre_remove hook（约 721 行）**

将：
```rust
if let Some(h) = hook {
    if !args.force {
        let _ = hook_engine.execute(h, &HookContext {
            workspace: workspace.name.clone(),
            repo: Some(repo_entry.name.clone()),
            branch: workspace.branch.clone(),
            target_branch: repo_entry.target_branch.clone(),
            worktree_path: Some(worktree_path.clone()),
            workspace_dir: ws_dir.clone(),
        });
    }
}
```

改为：
```rust
if let Some(h) = hook {
    if !args.skip_hooks {
        if let Err(e) = hook_engine.execute(h, &HookContext {
            workspace: workspace.name.clone(),
            repo: Some(repo_entry.name.clone()),
            branch: workspace.branch.clone(),
            target_branch: repo_entry.target_branch.clone(),
            worktree_path: Some(worktree_path.clone()),
            workspace_dir: ws_dir.clone(),
        }) {
            warn_or_bail(args.force, e, "pre_remove hook failed")?;
        }
    }
}
```

- [ ] **Step 3: 更新 remove_dir_all（约 746 行）**

将：
```rust
if Path::new(&ws_dir).exists() {
    std::fs::remove_dir_all(&ws_dir)?;
}
```

改为：
```rust
if Path::new(&ws_dir).exists() {
    if let Err(e) = std::fs::remove_dir_all(&ws_dir) {
        warn_or_bail(args.force, e.into(), "failed to remove workspace directory")?;
    }
}
```

- [ ] **Step 4: 编译确认无错误**

```bash
cargo build 2>&1 | head -30
```

Expected: 编译成功，无 unused field warning for `skip_hooks`

- [ ] **Step 5: 运行全量测试确认无回归**

```bash
cargo test
```

Expected: all tests pass

- [ ] **Step 6: Commit**

```bash
git add src/cli/workspace.rs
git commit -m "feat: update handle_cancel to use skip_hooks/warn_or_bail semantics"
```
