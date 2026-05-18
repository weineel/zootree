# `zootree create --start` / `--run-agent` 设计

## 背景

`zootree create` 当前只创建 pending 状态的 workspace 元数据，用户接着必须再跑一次 `zootree start <name>` 才能真正建出 worktree、启动 zellij、可选启动 agent_cli。在常用场景下（创建即开始工作）这两步可以合并，减少一次输入。

## 目标

- `zootree create` 新增 `--start`：创建完成后直接走 start 流程。
- `zootree create` 新增 `--run-agent`：与 `start` 子命令的同名参数语义完全一致，且**出现即视为同时传了 `--start`**。
- 不引入 `--no-zellij` 透传（保持 create 表面参数最小）。

## 非目标

- 不重构 `handle_start`，不抽出新的内部函数。
- 不改 start 子命令的任何行为。
- 不改 workspace 状态机（仍然 pending → in_progress 由 start 推进）。

## CLI 参数

在 `src/cli/workspace.rs` 的 `CreateArgs` 中新增两个字段，clap 配置与 `StartArgs` 上对应字段保持一致：

```rust
#[arg(long, help = "Start the workspace immediately after creation")]
pub start: bool,

#[arg(
    long,
    num_args = 0..=1,
    default_missing_value = "",
    value_name = "ALIAS_OR_CMD",
    help = "Launch agent_cli in the designated pane after start (implies --start)",
    add = ArgValueCompleter::new(|c: &std::ffi::OsStr| complete_agent_cli_alias(c)),
)]
pub run_agent: Option<Option<String>>,
```

- `--start`：bool，不带值。
- `--run-agent`：可裸用（取 `global.agent_cli`）或带别名/命令字面量，复用 `complete_agent_cli_alias` 补全。

## 行为

在 `handle_create` 末尾，所有现有打印语句之后追加：

```rust
let should_start = args.start || args.run_agent.is_some();
if should_start {
    let start_args = StartArgs {
        name: Some(name.clone()),
        no_zellij: false,
        run_agent: args.run_agent.clone(),
    };
    handle_start(&start_args)?;
}
```

行为矩阵：

| `--start` | `--run-agent` | 结果 |
|---|---|---|
| 否 | 否 | 仅创建（回归现状） |
| 是 | 否 | 创建后启动，不跑 agent_cli |
| 否 | 是 | 等价于 `--start --run-agent ...`，启动并跑 agent_cli |
| 是 | 是 | 同上 |

错误处理：
- create 失败：`?` 早返回，不触发 start（与现状一致）。
- start 失败：错误冒泡到调用方。此时 workspace 已落到 pending，用户可以手动 `zootree start <name>` 重试，符合现有 start 子命令独立调用时的失败语义。
- 在 zellij session 内调用 `--start`：复用 `handle_start` 内已有的 `ZELLIJ` 环境变量检查，会 bail 报错。

## 测试

本次改动是参数透传 + 直接函数调用，无可隔离的纯函数值得单测。**不新增单元测试**。

实现完成后手动验证清单：

1. `zootree create --title t --repos foo` → 仅创建，状态 pending（回归）。
2. `zootree create --title t --repos foo --start` → 创建后进入 start 流程，无 agent。
3. `zootree create --title t --repos foo --run-agent` → 等价于 `--start --run-agent`，启动 agent_cli（取 `global.agent_cli`）。
4. `zootree create --title t --repos foo --run-agent claude` → 透传别名/命令字面量。
5. 在 zellij session 内跑带 `--start` 的 create：应当 bail（回归 ZELLIJ 检查）。

## 影响范围

- 修改文件：`src/cli/workspace.rs`（仅 `CreateArgs` 和 `handle_create` 末尾）。
- 不影响：`StartArgs`、`handle_start`、其他子命令、配置格式、状态机、补全脚本（`--run-agent` 补全函数已存在）。
