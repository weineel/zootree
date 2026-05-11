---
name: zootree-dynamic-completions
description: 修复 zootree start 命令无法补全 pending workspace 的问题
type: project
---

## 问题

用户配置 `eval "$(zootree completions zsh)"` 后，`zootree start <TAB>` 只会触发目录补全，而非 pending 状态的 workspace 列表。

### 根因

zootree 有两套补全机制：

1. **动态补全**（`CompleteEnv`）：通过 `COMPLETE` 环境变量拦截，调用 `ArgValueCompleter` 回调，能获取 pending workspace
2. **静态补全**（`clap_complete::generate`）：生成静态脚本，只含固定子命令/flag，不含动态候选

当前 `zootree completions` 命令走的是静态生成，所以 shell 拿到的是没有动态候选的脚本，按 Tab 时 zsh 找不到任何候选项，退化到默认的 `_files`（目录补全）。

main.rs 已经正确配置了动态拦截：
```rust
CompleteEnv::with_factory(Cli::command).complete();
```

但 `completions` 子命令没有利用这个机制。

## 修复方案

修改 `src/cli/completions.rs`，让 `handle_completions` 使用 `CompleteEnv` 的动态协议生成注册脚本，而非静态脚本。

### 实现步骤

1. **修改 `src/cli/completions.rs`**
   - 导入 `clap_complete::env::{EnvCompleter, Shells}`
   - 重写 `handle_completions`：
     - 用当前可执行文件路径作为 bin/completer（`std::env::current_exe()`，失败时回退到 `"zootree"`）
     - 用 `Shells::builtins().completer(<shell 名称>)` 根据用户传入的 shell 拿到 `EnvCompleter`
     - 调用 `completer.write_registration("COMPLETE", "zootree", bin, completer_path, &mut io::stdout())` 输出动态注册脚本
   - 保留原 `generate_to` 函数（`tests/completions_test.rs` 依赖它做快照测试）
   - shell 名称从现有的 `Shell` enum 派生，用 `{}` 格式化即可得到（如 `Shell::Zsh` → `"zsh"`）

2. **更新测试**
   - `tests/completions_test.rs` 里测 `handle_completions` 实际输出的测试（如果有）需要改成检查动态注册脚本特征（包含 `COMPLETE` / `_CLAP_COMPLETE_INDEX` 等），而不是静态脚本的子命令列表
   - 用 `generate_to` 做快照的测试保持不变（那是对静态脚本生成器的单元测试）

3. **手动验证**
   - `cargo install --path . --force` 安装新版本
   - `eval "$(zootree completions zsh)"`，`zootree start <TAB>` 应列出 pending workspace 并显示 title
   - `zootree open <TAB>` / `zootree done <TAB>` 应列出 in-progress workspace
   - `zootree cancel <TAB>` 应列出 pending + in-progress workspace

### 关键 API 参考

```rust
use clap_complete::env::{EnvCompleter, Shells};

// 通过 shell 名称查找 EnvCompleter 实现
let shell_name = format!("{}", args.shell); // e.g. "zsh"
let completer = Shells::builtins()
    .completer(&shell_name)
    .ok_or_else(|| anyhow::anyhow!("unsupported shell: {}", shell_name))?;

// 生成该 shell 的动态注册脚本
completer.write_registration(
    "COMPLETE",   // 环境变量名（和 main.rs 的 CompleteEnv 默认值一致）
    "zootree",    // command name
    &bin,         // binary name（通常就是 "zootree"）
    &completer_path, // 用于补全的可执行路径
    &mut io::stdout(),
)?;
```

### 用户体验

修复前后对比：

| 场景 | 修复前 | 修复后 |
|------|--------|--------|
| `eval "$(zootree completions zsh)"` | 静态脚本 | 动态注册脚本 |
| `zootree start <TAB>` | 目录补全 | pending workspace 列表 |
| 新增子命令/flag | 需要重装补全 | 自动生效（零维护） |

## Why

commit 0180ab3 文档写了"升级零维护"，但实现还是静态脚本，两者不一致。这次修复让实现和文档对齐。

## How to apply

实现后，用户现有的 rc 配置 `eval "$(zootree completions zsh)"` 不需要任何改动，重启 shell 或 source rc 文件后生效。