# Design: --force / --skip-hooks 行为重构

Date: 2026-05-08

## 背景

`handle_done` 和 `handle_cancel` 中 `--force` 的语义混乱：它既跳过 hooks，又影响其他检查。
目标是拆分职责，并让清理流程在 `--force` 时尽可能走完，而非中途报错退出。

## 需求

1. `--force` 不再跳过 hooks
2. 新增 `--skip-hooks`，显式跳过所有 hooks（pre_done/pre_cancel/pre_remove）
3. `--force` 语义变为：步骤失败时 warn 而不是 bail，确保后续清理流程继续
4. merge 本身**始终阻塞**，`--force` 不影响 merge 失败
5. merge "nothing to commit"（squash 下无差异）视为 warn，不报错
6. 所有非 `--force` 时阻塞的错误，提示信息加上 `use --force to proceed anyway`

## 涉及文件

- `src/cli/workspace.rs` — `DoneArgs`、`CancelArgs`、`handle_done`、`handle_cancel`
- `src/core/git.rs` — `merge` 函数

## 详细设计

### 新增 flag

`DoneArgs` 和 `CancelArgs` 各加：

```rust
#[arg(long, help = "Skip all hooks (pre_done/pre_cancel/pre_remove)")]
pub skip_hooks: bool,
```

`--force` 的 help text 改为：

```rust
#[arg(long, help = "Continue even if steps fail (errors become warnings)")]
pub force: bool,
```

### warn_or_bail 辅助函数

在 `workspace.rs` 中添加私有函数：

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

### handle_done 改动

| 步骤 | 现行 | 新行为 |
|------|------|--------|
| pre_done hook | `!force` 时跳过，否则不执行 | 始终执行（除非 `skip_hooks`），失败 → `warn_or_bail(force, ...)` |
| 未提交检查 | `!force` 时 bail（已含 `--force` 提示） | 不变 |
| merge | 始终 `?` 传播 | 不变，始终阻塞 |
| merge "nothing to commit" | 报错 | git.rs 内检测，warn 并跳过 commit |
| pre_remove hook | `!force` 时执行且 `?` 传播，`force` 时跳过 | 始终执行（除非 `skip_hooks`），失败 → `warn_or_bail(force, ...)` |
| remove_dir_all | `?` 传播 | 失败 → `warn_or_bail(force, ...)` |

### handle_cancel 改动

| 步骤 | 现行 | 新行为 |
|------|------|--------|
| 未提交确认 | `!force` 时 confirm | 不变 |
| pre_cancel hook | `!force` 时跳过，否则不执行 | 始终执行（除非 `skip_hooks`），失败 → `warn_or_bail(force, ...)` |
| pre_remove hook | `!force` 时 `let _` 忽略，`force` 时跳过 | 始终执行（除非 `skip_hooks`），失败 → `warn_or_bail(force, ...)` |
| remove_dir_all | `?` 传播 | 失败 → `warn_or_bail(force, ...)` |

### git.rs merge 函数改动

squash 策略（默认策略）下，`git merge --squash` 成功后检测是否有 staged 变更：

- `git diff --staged --quiet` 返回 exit 0 → 无变更，warn "nothing to merge from '{}' into '{}'" 并跳过 commit
- `git diff --staged --quiet` 返回 exit 1 → 有变更，执行 `git commit -m message`

rebase 和 merge 策略本身已处理 "Already up to date." 场景（exit 0），无需修改。

## 不在范围内

- `--force` 对 merge 失败的影响（始终阻塞，不在本次改动范围）
- state save/move_workspace 的错误处理（状态损坏属于严重错误，保持阻塞）
