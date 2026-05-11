# zootree 动态补全修复 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 `zootree completions <shell>` 输出 clap_complete 的动态注册脚本（而非静态 AOT 脚本），使 `zootree start <TAB>` 等命令能补全出真实 workspace/repo/template 候选。

**Architecture:** `src/main.rs` 已经正确调用 `CompleteEnv::with_factory(Cli::command).complete()` 作为动态补全拦截器（通过 `COMPLETE` 环境变量触发）。本次改动只需把 `src/cli/completions.rs` 的 `handle_completions` 从 `clap_complete::generate`（AOT 静态生成器，无法触发 `ArgValueCompleter`）切换到 `clap_complete::env::Shells::builtins().completer(name).write_registration(...)`（动态注册脚本生成器），即可让 shell rc 文件里 `eval "$(zootree completions <shell>)"` 装的脚本走回动态协议。用户现有的 rc 配置不用动。

**Tech Stack:** Rust 2021, clap 4 (derive), clap_complete 4.5 (features = ["unstable-dynamic"]), anyhow。测试框架 `cargo test` + 临时 `tempfile::TempDir`。

**Spec:** `docs/superpowers/specs/2026-05-09-zootree-dynamic-completions.md`

---

## 文件结构

| 文件 | 动作 | 职责 |
|------|------|------|
| `src/cli/completions.rs` | 重写 | `CompletionsArgs` 不变；`handle_completions` 改走 `EnvCompleter::write_registration`；新增可测试入口 `write_registration(shell, completer_path, buf)`；删除 `generate_to`（不再有用户） |
| `tests/completions_test.rs` | 替换尾部 5 个测试 | 第 298 行起的 5 个 `generates_<shell>_script` 测试目前测的是 AOT 静态脚本特征，改为测动态注册脚本的特征字符串（包含 `COMPLETE`、dispatch 函数名等） |

动态补全的真正 dispatcher (`CompleteEnv::with_factory(Cli::command).complete()`) 在 `src/main.rs:50` 已就位，这次不用动。`ArgValueCompleter` 回调（`src/cli/workspace.rs:537` 等）也不用改。

---

## Task 1: 重写 completions.rs 并更新测试

**Files:**
- Modify: `src/cli/completions.rs` (整个文件重写)
- Modify: `tests/completions_test.rs:298-342`（删掉原 5 个静态脚本测试和 `use` 行，替换为 5 个动态脚本测试）

- [ ] **Step 1: 写失败测试 —— 替换 `tests/completions_test.rs:298-342` 的旧测试块**

先 Read 文件确认 289-342 行的精确内容，然后把从 `use clap_complete::Shell;` 到文件末尾的部分替换为下面这段。保留 297 行及以前的 `complete_*_with` 等既有测试不动。

```rust
use clap_complete::Shell;
use zootree::cli::completions::write_registration;

fn dynamic_script(shell: Shell) -> String {
    let mut buf = Vec::new();
    write_registration(shell, "zootree", &mut buf).unwrap();
    String::from_utf8(buf).unwrap()
}

#[test]
fn dynamic_zsh_registration_dispatches_to_complete_env() {
    let s = dynamic_script(Shell::Zsh);
    assert!(s.contains("#compdef zootree"), "zsh script missing compdef: {s}");
    assert!(s.contains("_clap_dynamic_completer"), "zsh script missing dispatcher: {s}");
    assert!(s.contains("COMPLETE"), "zsh script missing COMPLETE env var: {s}");
    // AOT 脚本会把每个子命令名 inline，动态脚本不应该；抽查一个
    assert!(
        !s.contains("completions:Generate"),
        "zsh script looks like AOT output (contains subcommand list): {s}"
    );
}

#[test]
fn dynamic_bash_registration_dispatches_to_complete_env() {
    let s = dynamic_script(Shell::Bash);
    assert!(s.contains("_clap_complete_"), "bash script missing dispatcher: {s}");
    assert!(s.contains("COMPLETE"), "bash script missing COMPLETE env var: {s}");
    assert!(s.contains("zootree"), "bash script missing bin name: {s}");
}

#[test]
fn dynamic_fish_registration_dispatches_to_complete_env() {
    let s = dynamic_script(Shell::Fish);
    assert!(s.contains("COMPLETE=fish"), "fish script missing dynamic env invocation: {s}");
    assert!(s.contains("--command zootree"), "fish script missing bin name: {s}");
}

#[test]
fn dynamic_powershell_registration_dispatches_to_complete_env() {
    let s = dynamic_script(Shell::PowerShell);
    assert!(
        s.contains("Register-ArgumentCompleter"),
        "powershell script missing registration: {s}"
    );
    assert!(s.contains("COMPLETE"), "powershell script missing COMPLETE env var: {s}");
}

#[test]
fn dynamic_elvish_registration_dispatches_to_complete_env() {
    let s = dynamic_script(Shell::Elvish);
    assert!(
        s.contains("edit:completion:arg-completer"),
        "elvish script missing arg-completer binding: {s}"
    );
    assert!(s.contains("COMPLETE"), "elvish script missing COMPLETE env var: {s}");
}
```

- [ ] **Step 2: 运行失败测试确认失败原因是"`write_registration` 未定义"**

Run: `cargo test --test completions_test -- dynamic_`
Expected: FAIL，错误类似：
```
error[E0432]: unresolved import `zootree::cli::completions::write_registration`
```

（此时旧的 `generates_zsh_script` 等测试也已被删除，不应再出现它们的输出。）

- [ ] **Step 3: 重写 `src/cli/completions.rs`**

完整新内容（替换原文件）：

```rust
use anyhow::{anyhow, Result};
use clap::Args;
use clap_complete::env::Shells;
use clap_complete::Shell;
use std::io::{self, Write};

#[derive(Args)]
pub struct CompletionsArgs {
    #[arg(value_enum, help = "Target shell")]
    pub shell: Shell,
}

pub fn handle_completions(args: &CompletionsArgs) -> Result<()> {
    let completer_path = std::env::current_exe()
        .ok()
        .and_then(|p| p.into_os_string().into_string().ok())
        .unwrap_or_else(|| "zootree".to_string());
    write_registration(args.shell, &completer_path, &mut io::stdout())
}

/// 写入给定 shell 的 clap_complete 动态补全注册脚本。
/// 抽出来以便测试可直接传 buffer，不依赖 stdout / current_exe。
pub fn write_registration(shell: Shell, completer_path: &str, buf: &mut dyn Write) -> Result<()> {
    let shell_name = shell.to_string();
    let completer = Shells::builtins()
        .completer(&shell_name)
        .ok_or_else(|| anyhow!("unsupported shell: {}", shell_name))?;
    completer.write_registration("COMPLETE", "zootree", "zootree", completer_path, buf)?;
    Ok(())
}
```

关键点：
- 删除了原 `generate_to` 函数（没有别处在用，跑 `grep -rn "generate_to" src tests` 确认）
- `Shell` 实现了 `Display`，`shell.to_string()` 返回 `"zsh"` / `"bash"` 等小写名称，正是 `Shells::builtins().completer(name)` 期望的
- `Shells::builtins()` 内置支持 bash/elvish/fish/powershell/zsh，覆盖 clap_complete 的 `Shell` enum 所有变体
- `"COMPLETE"` 必须和 `src/main.rs:50` 的 `CompleteEnv::with_factory(Cli::command).complete()` 默认环境变量一致（`CompleteEnv` 的 `var` 默认就是 `"COMPLETE"`，未被 `.var(...)` 覆盖过，两边天然一致）

- [ ] **Step 4: 运行新测试通过**

Run: `cargo test --test completions_test`
Expected: 所有测试（包括原有 `complete_*_with` 的 workspace/repo/template 测试 + 新增 5 个 `dynamic_*` 测试）全部 PASS。

- [ ] **Step 5: 跑完整 lint / fmt / test 套件**

Run:
```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```
Expected: 全部通过，无 warning。

如果 `cargo fmt --check` 失败，跑 `cargo fmt` 后再次 `cargo fmt --check`。如果 clippy 报 warning，就地修复。

- [ ] **Step 6: commit**

```bash
git add src/cli/completions.rs tests/completions_test.rs
git commit -m "fix: completions 子命令改走动态补全注册脚本

原先 handle_completions 调用 clap_complete::generate 生成静态 AOT 脚本，
shell 拿不到 ArgValueCompleter 的动态候选（如 pending workspace），
用户按 Tab 时退化到文件路径补全。

改为通过 clap_complete::env::Shells::builtins().write_registration 输出
动态注册脚本，让脚本最终 dispatch 回 CompleteEnv::complete() 拦截器，
走 ArgValueCompleter 回调，真正补全 pending workspace 等动态候选。

main.rs 的 CompleteEnv 拦截器保持不变，用户 rc 文件里
eval \"\$(zootree completions <shell>)\" 一行无需改动。"
```

---

## Task 2: 端到端手动验证

**Files:** 无（只是验证 + 可能更新 skill 文档）

- [ ] **Step 1: 安装新二进制**

Run: `cargo install --path . --force`
Expected: 成功安装到 `~/.cargo/bin/zootree`。

- [ ] **Step 2: 启动新 shell，验证 start 补全**

在新的 zsh 窗口（或 `exec zsh` 重载）：

```bash
zootree list  # 先确认至少有一个 pending workspace
zootree start <TAB>
```

Expected: 列出 pending 状态的 workspace 名，并带 title 和状态描述（例如 `fix-login  -- Fix login (pending)`）。不应再出现当前目录下的文件/目录。

如果没有 pending workspace，先 `zootree create --title test-comp` 造一个再测。

- [ ] **Step 3: 验证其他命令的动态补全也生效**

```bash
zootree open <TAB>     # 应列 in-progress workspace
zootree done <TAB>     # 应列 in-progress workspace
zootree cancel <TAB>   # 应列 pending + in-progress workspace
zootree repo edit <TAB>  # 应列 repo 名
```

Expected: 都能列出对应候选；为空时（如没有 in-progress）Tab 无候选而不是退化到文件。

- [ ] **Step 4: 验证直接走动态协议（排除 shell 插件干扰）**

```bash
COMPLETE=zsh zootree -- zootree start ''
```
Expected: 每行一个 pending workspace 名（形如 `fix-login:Fix login (pending)`）。

如果本步能输出但 Step 2 不行，说明是 shell 补全系统的缓存/fpath 问题，不是 zootree 本身的问题 —— 尝试 `rm -f ~/.zcompdump*; exec zsh` 后再测。

- [ ] **Step 5: 如无代码改动则跳过 commit；如发现文档需要同步更新：**

检查 `README.md` / `README.zh-CN.md` 里"Shell 补全"章节的描述是否和新实现一致（文案不用改，因为仍然是 `eval "$(zootree completions <shell>)"`；但"零维护"一词现在才真正成立，如有描述矛盾可以顺手修正）。

如果 `.claude/skills/zootree-dev/SKILL.md` 里"给新命令添加动态补全"章节提到了 `generate_to` 或静态脚本，同步更新。

```bash
# 仅当有文档修改时：
git add -p   # 按需挑选
git commit -m "docs: 同步动态补全实现细节"
```

---

## 完成验收标准

- `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test` 全绿
- 新 shell 窗口里 `zootree start <TAB>` 输出 pending workspace 而非文件列表
- `COMPLETE=zsh zootree -- zootree start ''` 输出 pending workspace 候选
- commit history 中有一个清晰的 `fix:` commit，diff 只包含 `src/cli/completions.rs` 和 `tests/completions_test.rs`（± 可选的文档同步 commit）
