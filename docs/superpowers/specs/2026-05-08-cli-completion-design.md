# Shell 补全（静态 + 动态）设计

- 日期：2026-05-08
- 作者：weineel + Claude
- 状态：已通过 brainstorming，待 writing-plans

## 背景与目标

zootree 当前没有任何 shell 补全支持。用户必须手动记忆 10 个顶级子命令、众多 flag，以及 workspace / repo / template 等动态值。本设计为 zootree 引入完整的 shell 补全：

- **静态补全**：子命令、flag 名（如 `zootree wo<TAB>` → `workspace`、`--ver<TAB>` → `--verbose`）
- **动态值补全**：workspace 名、repo 名、template 名
- **多 shell 支持**：bash、zsh、fish、powershell、elvish 五种全部覆盖
- **按命令合法性过滤动态值**：例如 `zootree start <TAB>` 只列 pending workspace
- **zsh / fish 候选附带描述**：例如 workspace 候选后显示 title + status

非目标（YAGNI）：
- 启动时自动检测并安装补全（用户自行执行一次性 source 命令即可）
- 候选缓存机制（实时读取已足够快）
- `--repos` 列表中冒号分隔的 branch 子段补全（第一版仅补 repo 名段）
- shell 集成测试（依赖外部 shell，CI 上脆弱）

## 实现方案选择

采用 **`clap_complete` 4.5 的 dynamic-completion 引擎**（feature `unstable-dynamic`）。

理由：
- 5 种 shell 的 glue 脚本由 clap 维护，无需手写
- 动态值闭包用 Rust 写一次，全 shell 通用
- 描述、状态过滤天然支持
- 已被 cargo-binstall 等成熟项目采用，实质风险可控

放弃的方案：
- **静态生成 + 自定义 `__complete` 子命令**（kubectl/gh 模式）：需要为 5 个 shell 各写 glue 脚本，维护成本高
- **纯静态补全**：与「全部都要」需求不符

## 架构与文件改动

### 依赖

`Cargo.toml` 新增：

```toml
clap_complete = { version = "4.5", features = ["unstable-dynamic"] }
```

### 新增文件

| 文件 | 职责 |
|------|------|
| `src/cli/completions.rs` | `Completions` 子命令的 `Args` 与 `handle_*` 函数（`zootree completions <shell>` 静态生成）|
| `src/core/completers.rs` | 动态候选闭包工厂：`workspace_completer(filter)`、`repo_completer()`、`template_completer()` |
| `tests/completions_test.rs` | 静态生成 smoke test + 动态候选过滤 / 兜底 / 前缀单元测试 |

### 修改文件

| 文件 | 改动 |
|------|------|
| `src/cli/mod.rs` | `Commands` 新增 `Completions(completions::CompletionsArgs)` 变体；`pub mod completions` |
| `src/main.rs` | 在 `Cli::parse()` **之前**调用 `CompleteEnv::with_factory(\|\| Cli::command()).complete()` |
| `src/cli/workspace.rs` | `StartArgs` / `OpenArgs` / `DoneArgs` / `CancelArgs` / `CreateArgs` 的 name/template/repos 字段挂 `ArgValueCompleter` |
| `src/cli/repo.rs` | `RepoCommands::Edit` / `RepoCommands::Remove` 的 name 字段挂 completer |
| `src/cli/template.rs` | `TemplateCommands::Save` 的 `--from` 字段挂 workspace completer |
| `src/core/mod.rs` | `pub mod completers;` |
| `Cargo.toml` | 新增依赖 |
| `README.md` / `README.zh-CN.md` | 新增「Shell Completions / Shell 补全」章节 |

### 调用拓扑

- **静态生成路径**：`zootree completions zsh` → `cli::completions::handle_completions` → `clap_complete::generate(Cli::command(), ...)` → 输出脚本到 stdout
- **动态补全路径**：`COMPLETE=<shell> zootree -- ...`（由 shell glue 脚本在 TAB 时调起）→ `main.rs` 中 `CompleteEnv::complete()` 拦截 → 命中字段上挂载的 `ArgValueCompleter` → 闭包通过 `ConfigManager` 读取候选 → 返回 `Vec<CompletionCandidate>`

## 静态补全：`completions` 子命令

### CLI 接口

```
zootree completions <SHELL>
```

- `<SHELL>` 位置参数，必填，可选值由 `clap_complete::Shell` 枚举提供：`bash | zsh | fish | powershell | elvish`
- 输出统一到 stdout，由用户重定向。这是 kubectl/gh/rustup 的通用约定

### Args 与 handler

```rust
// src/cli/completions.rs
use clap::{Args, CommandFactory};
use clap_complete::Shell;
use crate::cli::Cli;
use anyhow::Result;

#[derive(Args)]
pub struct CompletionsArgs {
    #[arg(value_enum, help = "Target shell")]
    pub shell: Shell,
}

pub fn handle_completions(args: &CompletionsArgs) -> Result<()> {
    let mut cmd = Cli::command();
    let bin = cmd.get_name().to_string();
    clap_complete::generate(args.shell, &mut cmd, bin, &mut std::io::stdout());
    Ok(())
}
```

### 用户安装命令

| Shell | 命令 |
|-------|------|
| bash | `zootree completions bash > ~/.local/share/bash-completion/completions/zootree` |
| zsh  | `zootree completions zsh > "${fpath[1]}/_zootree"` |
| fish | `zootree completions fish > ~/.config/fish/completions/zootree.fish` |
| powershell | `zootree completions powershell \| Out-String \| Invoke-Expression`（写入 `$PROFILE`）|
| elvish | `zootree completions elvish > ~/.config/elvish/lib/zootree.elv`（在 `rc.elv` 中 `use zootree`）|

### 静态与动态的关系

`clap_complete` dynamic-completion 的 glue 脚本由 `clap_complete::generate` 生成。脚本内部在 TAB 时调用 `COMPLETE=<shell> zootree -- <args>`，由 `CompleteEnv` 拦截并返回候选。

用户**只需跑一次安装命令**，之后所有补全（包括动态值）都自动生效，无须再手动 source。

### 错误处理与退出码

- `Shell` 是 `ValueEnum`，clap 自动校验非法 shell 名并以非零退出
- 生成不会失败（IO 写 stdout），不输出额外日志（避免污染重定向到的脚本文件）

## 动态补全：字段映射与候选数据来源

### 完整字段映射表

| 命令 / 参数 | Completer | 候选过滤 |
|------------|-----------|---------|
| `start <name>` | workspace | `status == pending` |
| `open <name>` | workspace | `status == in_progress` |
| `done <name>` | workspace | `status == in_progress` |
| `cancel <name>` | workspace | `status in {pending, in_progress}` |
| `repo edit <name>` | repo | 全部已注册 repo |
| `repo remove <name>` | repo | 全部已注册 repo |
| `template save --from <ws>` | workspace | 全部 workspace（任意状态都可保存为模板）|
| `create --template <name>` | template | 全部 template |
| `create --repos <list>` | repo（列表型，见下）| 全部已注册 repo |
| `done --strategy <s>` | clap ValueEnum | `squash / rebase / merge`（顺手优化，详见下）|
| `list --status <s...>` | clap ValueEnum | 当前是 `Vec<String>`，本次顺手改为 `Vec<WorkspaceStatus>` 以启用补全 |

未列出的命令（`list`、`logs`、`prune`、`repo add`、`repo list`、`template list`、`done` 的其他 flag、`cancel --keep` 等）无需动态补全。

### `--repos` 列表型补全

参数格式 `repo1:branch1,repo2:branch2,repo3`。补全器收到当前光标处的整段字符串：

1. 用 `,` 切分得到当前正在编辑的最后一段 `current`
2. 若 `current` 不含 `:` → 当作 repo 名补全；候选 = 已写好的前缀 `repo1:branch1,` + 候选 repo 名
3. 若 `current` 含 `:` → **第一版不补全**（branch 段留给用户手输；YAGNI）

### 候选数据来源（`src/core/completers.rs`）

```rust
use clap_complete::CompletionCandidate;
use std::ffi::OsStr;
use crate::config::ConfigManager;
use crate::config::workspace::WorkspaceStatus;

pub enum WorkspaceFilter {
    Pending,
    InProgress,
    Active,        // pending or in_progress
    Any,
}

impl WorkspaceFilter {
    fn matches(&self, status: &WorkspaceStatus) -> bool {
        match (self, status) {
            (Self::Any, _) => true,
            (Self::Pending, WorkspaceStatus::Pending) => true,
            (Self::InProgress, WorkspaceStatus::InProgress) => true,
            (Self::Active, WorkspaceStatus::Pending | WorkspaceStatus::InProgress) => true,
            _ => false,
        }
    }
}

pub fn workspace_completer(
    filter: WorkspaceFilter,
) -> impl Fn(&OsStr) -> Vec<CompletionCandidate> + Send + Sync + 'static {
    move |current: &OsStr| {
        let prefix = current.to_string_lossy();
        let Ok(mgr) = ConfigManager::new() else { return vec![]; };
        let Ok(names) = mgr.list_workspaces() else { return vec![]; };
        names
            .into_iter()
            .filter_map(|name| mgr.load_workspace(&name).ok().map(|(_, w)| (name, w)))
            .filter(|(_, w)| filter.matches(&w.status))
            .filter(|(name, _)| name.starts_with(prefix.as_ref()))
            .map(|(name, w)| {
                CompletionCandidate::new(&name)
                    .help(Some(format!("{} ({:?})", w.title, w.status).into()))
            })
            .collect()
    }
}

// repo_completer / template_completer 同结构：
// - repo  描述 = repo.path
// - template 描述 = tmpl.repos.join(", ")
```

### 在 Args 上挂载

```rust
// 例：src/cli/workspace.rs
use clap_complete::ArgValueCompleter;
use crate::core::completers::{workspace_completer, WorkspaceFilter};

pub struct StartArgs {
    #[arg(
        help = "Workspace name to start (interactive if omitted)",
        add = ArgValueCompleter::new(workspace_completer(WorkspaceFilter::Pending))
    )]
    pub name: Option<String>,
    // ...
}
```

### 失败兜底（重要）

所有 completer 闭包**永不 panic、永不写 stderr**：
- 配置目录不存在 → 返回 `vec![]`
- 单个 TOML 解析失败 → 跳过该条，继续返回其他
- IO 错误 → `vec![]`

补全脚本必须与用户主流程隔离，绝不让一个补全错误污染用户终端。

### `done --strategy` 与 `list --status` 顺手优化

两处都将 `String` 类型改成 `ValueEnum`，原因是这能直接启用 clap 内建的值补全（无需自定义 completer）：

```rust
#[derive(Clone, ValueEnum)]
pub enum MergeStrategy { Squash, Rebase, Merge }
```

`done --strategy` 字段类型从 `Option<String>` 改为 `Option<MergeStrategy>`；`handle_done` 内部用 `match` 转回现有字符串逻辑或直接消费枚举。

`list --status` 字段类型从 `Vec<String>` 改为 `Vec<WorkspaceStatus>`（`WorkspaceStatus` 已 derive 必要的 trait，需补 `ValueEnum`）。`handle_list` 现有过滤逻辑改为按枚举比对。

这两处都是「在工作区域内顺带改进」，符合 brainstorming 设计原则；具体迁移路径在 writing-plans 阶段细化。

## 动态补全入口接线、性能、时效性

### `main.rs` 的拦截顺序

`CompleteEnv` 必须在 `Cli::parse()` **之前**、`init_tracing` **之前**调用。命中环境变量时它会自己解析 ARGV、输出候选、`exit(0)`，不会回到主流程。

```rust
// src/main.rs
use clap_complete::CompleteEnv;
use clap::CommandFactory;

fn main() {
    // 1. 动态补全拦截器：命中环境变量则处理后退出
    CompleteEnv::with_factory(|| Cli::command()).complete();

    // 2. 正常路径
    let cli = Cli::parse();
    let _guard = match init_tracing(cli.verbose, cli.quiet) {
        Ok(g) => g,
        Err(e) => { eprintln!("Error: failed to initialize tracing: {}", e); std::process::exit(1); }
    };
    if let Err(e) = run(cli.command) {
        tracing::error!("{:#}", e);
        std::process::exit(1);
    }
}
```

**为什么放在 tracing 之前**：补全调用每次 TAB 都发生。如果先初始化 tracing，会在 `~/.config/zootree/logs/` 不停产生空日志和 `WorkerGuard` flush，干扰用户日志。补全路径应该是**零副作用**的（除了向 stdout 写候选）。

### 性能预算

每次 TAB 触发一次 zootree 子进程：

| 阶段 | 估算 |
|------|------|
| 进程启动 + 链接 | ~5–15ms |
| `CompleteEnv` 解析 ARGV | <1ms |
| `ConfigManager::new()` + 列目录 | ~2–5ms |
| 逐个加载 workspace TOML | ~0.5ms × N |

**目标**：`N ≤ 50` 时总耗时 < 50ms（人类感知不到）。`N ≥ 200` 时可能到 150ms，仍可接受。

### 不做的优化（YAGNI）

- 候选缓存到 `~/.cache/zootree/completion-cache.toml`：会引入「数据陈旧」问题（刚 `done` 的 workspace 仍在补全里出现），价值不抵复杂度
- 并行加载 TOML：单文件 IO 已足够快
- 字段精简（另写 `WorkspaceSummary`）：等 `N > 200` 再说

### 数据时效性

**实时**。每次 TAB 都重新读 `~/.config/zootree/{workspaces,repos,templates}/`。这保证用户在某 zellij pane 里跑 `done` 后，**当前 shell 的补全立刻反映新状态**。

### 单元测试钩子

`completers.rs` 同时暴露**接受 `ConfigManager` 的可测变体**和**默认调 `ConfigManager::new()` 的零参变体**：

```rust
// 测试用
pub fn workspace_completer_for(
    mgr: ConfigManager,
    filter: WorkspaceFilter,
) -> impl Fn(&OsStr) -> Vec<CompletionCandidate> + Send + Sync + 'static;

// 实际挂在 Args 上
pub fn workspace_completer(
    filter: WorkspaceFilter,
) -> impl Fn(&OsStr) -> Vec<CompletionCandidate> + Send + Sync + 'static;
```

测试用 `ConfigManager::with_base_dir(tempdir)` 隔离配置目录。

## 测试策略

### 静态生成 smoke test

5 种 shell 各一个：调 `clap_complete::generate` 写到 `Vec<u8>` 缓冲区，断言脚本非空 + 含该 shell 的关键标记（zsh `#compdef`、bash `complete -F`、fish `complete -c zootree`、PowerShell `Register-ArgumentCompleter`、Elvish `edit:completion:arg-completer`）。

### 动态候选单元测试

| 测试 | 内容 |
|------|------|
| `workspace_completer_filters_by_status` | 4 个 filter 各一组断言，确认状态过滤正确 |
| `workspace_completer_filters_by_prefix` | 输入前缀只返回匹配的候选 |
| `workspace_completer_includes_description` | 候选 `help` 含 title 和 status |
| `repo_completer_basic` | 列出全部 + 前缀过滤 |
| `repo_completer_includes_path` | 候选 `help` 含 repo path |
| `template_completer_basic` | 列出全部 + 前缀过滤 |
| `template_completer_includes_repos` | 候选 `help` 含 `repos.join(", ")` |
| `completer_returns_empty_when_config_dir_missing` | 不存在的配置目录返回空，不 panic |
| `completer_skips_corrupted_toml` | 单文件损坏不影响其他候选 |

### 不覆盖

- bash/zsh/fish 实际 shell 行为（依赖外部 shell，CI 上脆弱）
- 真实 TAB 触发链路
- 性能基准（YAGNI）
- 端到端 `CompleteEnv` 拦截（仅在 README 故障排查里给出验证命令）

## README 文档章节

`README.md` 和 `README.zh-CN.md` 各新增「Shell Completions / Shell 补全」章节，位于「Installation」之后、「Usage」之前。结构：

1. **简介**：支持哪些 shell、补全哪些内容
2. **Install** 表格：5 种 shell 的一次性安装命令
3. **What gets completed**：列出主要命令的动态补全行为
4. **Troubleshooting**：给出 `COMPLETE=zsh zootree -- zootree start ''` 这类直接验证命令

## 提交边界

整个特性作为**单个 PR**：核心实现 + 测试 + 文档一起进。不拆分原因：`CompleteEnv` 接线、Args 字段挂载、completer 工厂三者耦合，分开提交反而难审。

提交信息按现有风格：`feat: 添加 shell 补全（静态 + 动态）`。

## 留待 writing-plans 阶段细化

设计阶段的所有决策点已覆盖。以下是实现层细节，留到 writing-plans 中拆 step 时确定：

- `MergeStrategy` / `WorkspaceStatus` 改 `ValueEnum` 的迁移与本次 PR 是否拆分
- `--repos` 列表的前缀拼接细节（Rust 端如何把已写好的逗号前缀附回到候选）
- `clap_complete::Shell` 的 `ValueEnum` 与现有 `Cli` `CommandFactory` 接线时的细节
