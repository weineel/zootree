---
name: zootree-dev
description: >
  帮助开发者理解和修改 zootree 的 Rust 源代码，遵循项目架构和编码约定。
  当用户提到开发 zootree、添加命令/子命令、修改 zootree 配置或核心逻辑、
  编写 zootree 测试、或需要理解 zootree 代码架构时，使用此 skill。
---

# zootree 开发指南

## 项目架构

```
src/
├── main.rs          # 入口点: CLI 解析 + config recovery 提前路由 + tracing 初始化 + 常规命令路由
├── lib.rs           # 模块声明
├── cli/             # CLI 命令定义和处理
│   ├── mod.rs       # Cli struct + Commands enum (clap derive)
│   ├── config.rs    # config path/show/edit recovery + agents 人类/JSON 输出
│   ├── repo.rs      # repo add/list/edit/remove
│   ├── workspace.rs # create/start/list/open/reopen/done/cancel
│   ├── template.rs  # template list/save
│   ├── prune.rs     # prune 清理
│   ├── completions.rs # 生成 shell 补全脚本 (completions 子命令)
│   └── info.rs      # info [name] [--watch]
├── config/          # 配置管理
│   ├── mod.rs       # ConfigManager: 配置读写中枢
│   ├── name.rs      # config-backed repo/workspace/template 名称 slug 校验
│   ├── global.rs    # GlobalConfig + HooksConfig + HookValue
│   ├── repo.rs      # RepoConfig + LazyGitConfig
│   ├── workspace.rs # WorkspaceConfig + Event + WorkspaceStatus
│   └── template.rs  # TemplateConfig
├── core/            # 核心功能
│   ├── mod.rs
│   ├── agent_cli.rs # agent 默认值、alias 解析与共享 catalog
│   ├── git.rs       # GitOps: worktree/merge/push 等 git 操作
│   ├── hook.rs      # HookEngine + HookContext
│   ├── layout.rs    # LayoutRenderer: KDL 模板变量替换
│   ├── multiplexer/
│   │   ├── mod.rs      # crate-private adapter command 模块
│   │   ├── zellij.rs   # crate-private Zellij 命令翻译
│   │   ├── cmux.rs     # crate-private cmux group 命令翻译与 rollback
│   │   └── herdr.rs    # crate-private Herdr JSON CLI 命令翻译与创建事务
│   ├── terminal_environment/
│   │   ├── mod.rs      # 稳定 activate/close 门面、opaque stored state 解释与 adapter 路由
│   │   ├── cmux.rs     # cmux group reconciliation、layout/agent placement 与规范状态
│   │   ├── zellij.rs   # Zellij session reconciliation、KDL/agent placement 与规范状态
│   │   └── herdr.rs    # Herdr workspace reconciliation、内置 topology、agent 与规范状态
│   ├── cmux_layout.rs  # cmux JSON layout renderer
│   ├── copy_files.rs # 文件复制逻辑
│   ├── editor.rs    # $VISUAL/$EDITOR/vi 解析与 CommandRunner 交互式启动
│   ├── name_gen.rs  # 工作空间名称生成器
│   ├── repo_names.rs # repo 名称冲突处理
│   ├── repo_status.rs # 注册 repo 配置路径存在性检查
│   ├── reopen.rs   # archived workspace 的全量恢复计划、Git worktree 执行与回滚
│   ├── worktree_status.rs # workspace repo worktree 路径存在性检查
│   └── completers.rs # 动态补全候选生成器 (workspace/repo/template/agent alias)
├── tui_app/         # TUI 应用框架（ratatui + crossterm）
│   ├── mod.rs       # Event / App trait / run_app 事件循环
│   ├── create_wizard/
│   │   ├── mod.rs        # 稳定门面、公共 re-export 与 wizard 运行入口
│   │   ├── state.rs      # wizard 状态、页面模型与步骤导航数据
│   │   ├── navigation.rs # 键盘事件、校验、提交与取消状态转换
│   │   ├── render.rs     # wizard 页面与 review/draft 面板渲染
│   │   ├── repo_page.rs  # 仓库列表交互与显示标签
│   │   └── text_field.rs # 基于 tui-textarea 的文本字段状态
│   └── info.rs      # InfoApp + 格式化辅助函数
├── runner.rs        # CommandRunner trait + RealRunner + MockRunner
└── tui.rs           # dialoguer 封装的交互式 UI 工具函数
```

## 核心设计模式

### CommandRunner 依赖注入

所有外部命令调用通过 `CommandRunner` trait 进行，支持测试时用 `MockRunner` 替换：

```rust
// runner.rs
pub trait CommandRunner {
    fn run(&self, spec: &CommandSpec) -> Result<Output>;
}

pub struct RealRunner;      // 真实执行命令
pub struct MockRunner {     // 测试用
    pub calls: RefCell<Vec<CommandSpec>>,
    pub responses: RefCell<Vec<Output>>,
}
```

所有 `core/` 模块的函数接受 `&R: CommandRunner` 泛型参数。

### Terminal environment 生命周期门面

`src/core/terminal_environment/mod.rs` 定义 workspace caller 最终使用的同步生命周期门面。它直接持有 `ConfigManager`、`GlobalConfig` 和 `CommandRunner`，对外只暴露 adapter-neutral 的 `activate` / `close` 以及 `AgentIntent`、`Activation`、`CloseReport`：

```rust
pub struct TerminalEnvironment<'a, R: CommandRunner> {
    config_manager: &'a ConfigManager,
    global_config: &'a GlobalConfig,
    runner: &'a R,
    in_zellij: bool,
    herdr_caller: HerdrCallerContext,
}
```

两个 caller-context 字段都在 facade 构造时捕获，仅影响对应 adapter 的呈现行为；它们不参与 Terminal environment 的目标选择或持久化 identity。

`WorkspaceConfig.multiplexer_state` 的 TOML 字段名保持不变，但 Rust 类型是内部字段私有的 `StoredTerminalEnvironmentState`。配置层只负责 serde round-trip，只有 Terminal environment module 内部的私有类型解释 legacy state 或 `version + adapter + payload` envelope；未知版本同样必须可读并无损保存。配置层不公开 legacy state 结构。

cmux、Zellij 与 Herdr 均通过 `TerminalEnvironment::activate` / `close` 路由。adapter 优先使用可信 stored ref，失败后按确定性 display name 唯一恢复，无匹配时创建，歧义时拒绝猜测。成功 activate 返回 `version = 1` 的 opaque state；复用已有 terminal environment 时不会注入 agent，并通过 `Activation.warnings` 报告被忽略的请求。workspace caller 只保存返回 state 和记录 warning，不解释 adapter outcome/runtime refs。

`start` 与 `open` 共用同一个 activate caller seam：成功后保存 opaque state 并呈现 warning；`start` 在 worktree 与 `in_progress` 已完成后若激活失败，返回可由 `open` 重试的 partial-success 错误且不回滚。`done` / `cancel` 先完成 event 和最终状态归档，再调用 best-effort close；close warning 不改变最终 workspace 状态。`CloseReport.closed` 表示 adapter 是否确认目标已关闭，warning 只补充恢复过程信息；`reopen --overwrite` 必须检查 `closed`，成功 fallback 即使带 warning 也可继续。`--no-multiplexer` 只跳过当次 `start` / `reopen` 的 activate。

`core::reopen` 把 archived workspace 恢复封装为两段式边界：`build_reopen_plan` 完成全部只读 Git、路径和用户决策检查，`execute_reopen_plan` 才创建或覆盖 worktree、复用 `copy_files` / `post_create`、写 `reopened` event 并迁移状态，最后复用既有 `post_start`。reopen 的目标 Workspace 必须通过 `ReopenPlan::apply_current_terminal_config` 使用当前 global `[multiplexer]` 快照并清空 archived `multiplexer_state`；若 overwrite 需要先关闭旧环境，close caller 应重新加载 archived Workspace，仅把旧 config/state 用于定位旧 runtime。状态迁移前失败必须保留原 archived config 且 best-effort 回滚本次新建 worktree；迁移后的 hook/terminal 失败是可用 `open` 重试的 partial success。

`core::multiplexer` 是 crate-private 命令翻译实现，不提供通用 trait，也不暴露 launch、identity 或 outcome 类型。其私有模块单元测试直接验证精确 argv、环境变量清理、输出解析和 rollback；integration tests 只通过 `TerminalEnvironment` 验证生命周期 contract。

`terminal_environment::zellij` 负责 session 恢复、KDL layout 准备、agent placement 和规范状态；`src/core/multiplexer/zellij.rs` 只翻译 Zellij 命令。外部 Zellij 时前台创建/attach，内部 Zellij 时后台创建或提示已存在；close 先确认 session，目标不存在视为成功，list/delete 失败进入 `CloseReport.warnings`。

`src/core/multiplexer/cmux.rs` 中的 cmux helper 只负责 group-aware 命令翻译和 rollback：第一个 repo workspace 先创建，再通过 `workspace-group create --from <first-repo>` 创建 group；cmux 会自动生成一个默认 header/anchor，所以 zootree 随后创建自己的 anchor workspace、`set-anchor` 到它、并关闭 cmux 自动生成的默认 anchor。后续 repo workspaces 加入同一 group。group 恢复决策、agent 终端位置和运行时引用解释集中在 `terminal_environment::cmux`。

Herdr mode 把一个 zootree workspace 映射为显式 named session 中的一个 Herdr workspace；规范 state 保存 `session`、`workspace_id` 和 `label`。`terminal_environment::herdr` 负责按 stored ID/精确 label 恢复、内置 overview/repo tab topology、单 agent placement 与 caller session 判断；`multiplexer::herdr` 只通过 `CommandRunner` 构造 `herdr --session <session> ...`、解码 JSON 并在结构创建失败时关闭整个新 workspace。zootree 不管理 Herdr server/session，也不修复已有 topology。

### ConfigManager 模式

`ConfigManager` 是配置读写的中枢，不依赖外部命令（不需要 runner）。
- 初始化: `ConfigManager::new()` → `~/.config/zootree/`；home 为相对路径时先按当前目录绝对化
- 测试: `ConfigManager::with_base_dir(temp_path)` 指向临时目录
- `global_config_path()` 是全局 `config.toml` 路径的唯一定位入口
- config recovery 的原样读取、按需创建和强制解析分别使用 `read_global_config_source()`、`ensure_global_config_file()`、`parse_global_config_file()`，CLI 不直接读写配置文件
- `load_global_config()` 仅用于常规运行时加载；文件缺失时返回 `GlobalConfig::default()`
- 所有 save/load 使用 `toml` crate 进行序列化
- workspace 列表读取使用稳定排序：按传入 status 顺序遍历，每个 status 内按 workspace name 排序
- 需要同时使用 workspace status 和配置时，优先用 `list_workspaces_with_status`，避免先 `list_workspaces` 再逐个 `load_workspace`

### 命令路由

`main.rs` 先识别 `config path/show/edit` 并在全局配置解析与文件日志初始化前调用 `handle_bootstrap_command`，保证配置缺失或损坏时仍可恢复。其余命令完成常规初始化后匹配 `Commands` 枚举并调用对应的 `handle_*` 函数：

```rust
match cli.command {
    Commands::Config(args) => zootree::cli::config::handle_config_command(&args.command, &global)?,
    Commands::Repo(args) => zootree::cli::repo::handle_repo_command(&args.command)?,
    Commands::Create(args) => zootree::cli::workspace::handle_create(&args)?,
    Commands::Info(args) => zootree::cli::info::handle_info(&args)?,
    Commands::Completions(args) => zootree::cli::completions::handle_completions(&args)?,
    // ...
}
```

## 添加新命令

### 添加顶级命令

1. 在 `src/cli/mod.rs` 的 `Commands` enum 中添加变体
2. 在 `src/cli/` 下创建处理模块（或加到现有模块）
3. 在 `src/main.rs` 的 match 分支中添加路由
4. 在 `src/cli/<module>.rs` 中实现 `handle_*` 函数和 `Args` struct

示例 —— 添加 `zootree status` 命令：

```rust
// src/cli/mod.rs - Commands enum
Status(workspace::StatusArgs),

// src/cli/workspace.rs - Args + handler
#[derive(Args)]
pub struct StatusArgs { pub name: Option<String> }

pub fn handle_status(args: &StatusArgs) -> Result<()> { ... }

// src/main.rs - 路由
Commands::Status(args) => zootree::cli::workspace::handle_status(&args)?,
```

### Args struct 约定

- 使用 `clap::Args` derive
- 可选参数用 `Option<String>` + `#[arg(long)]`
- 位置参数直接用 `String` 类型（不加 `#[arg]`）
- 子命令用 `#[command(subcommand)]` + 独立 enum

## 测试规范

### 测试文件位置

`tests/` 目录下每个功能一个文件，命名 `*_test.rs`

### 测试模式

所有涉及 git、zellij、cmux、herdr 或 shell 的操作使用 `MockRunner`：

```rust
use zootree::runner::MockRunner;

#[test]
fn test_something() {
    let runner = MockRunner::new();
    runner.push_response(success_output()); // 预先填充响应
    let component = Component::new(&runner);

    component.do_something().unwrap();

    let calls = runner.take_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].program, "expected_program");
    assert_eq!(calls[0].args, vec!["expected", "args"]);
}
```

Terminal environment 的 activate/close contract 放在 `tests/terminal_environment_test.rs`；Zellij/cmux/Herdr 的低层命令翻译测试放在对应 crate-private module 的 `#[cfg(test)]` 中，不为 integration tests 公开 adapter seam。Zellij KDL 与 cmux JSON renderer 分别继续由 `tests/layout_test.rs` 和 `tests/cmux_layout_test.rs` 覆盖。

### 配置测试

使用 `ConfigManager::with_base_dir(temp_dir)` 指向临时目录，避免污染真实配置。

## 关键依赖

| Crate | 用途 |
|-------|------|
| `clap` (4, derive) | CLI 参数解析 |
| `clap_complete` (4, unstable-dynamic) | Shell 补全脚本生成 + 动态补全引擎 |
| `dialoguer` (0.11) | 交互式 TUI (Input, Select, MultiSelect, Confirm) |
| `toml` (0.8) | 配置文件序列化 |
| `serde` (1, derive) | 序列化框架 |
| `kdl` (6) | KDL 布局文件解析 |
| `tracing` + `tracing-subscriber` + `tracing-appender` | 日志系统 |
| `shellexpand` (3) | 路径中的 `~` 展开 |
| `anyhow` (1) | 错误处理 |
| `rand` (0.8) | 名称随机生成 |
| `chrono` (0.4, serde) | 时间戳 |
| `shlex` (1) | 把 `agent_cli` 字符串模板拆成 argv |
| `cargo-husky` (1, dev, `default-features = false`, `user-hooks`) | 安装 `.cargo-husky/hooks/` 下的 git hook 到 `.git/hooks/`，在 `cargo check --tests` 首次构建时生效 |
| `ratatui` (0.30) | TUI 框架，`src/tui_app/` 的渲染内核 |
| `crossterm` (0.29) | 终端后端：raw mode、事件读取、alternate screen |

## 代码约定

- **错误处理**: 统一使用 `anyhow::Result<T>`，用 `anyhow::bail!()` 返回错误
- **可测试性**: 外部命令调用通过 `CommandRunner` trait，不直接调用 `std::process::Command`
- **编辑器启动**: config/repo 编辑统一使用 `core::editor`，解析顺序为 `$VISUAL`、`$EDITOR`、`vi`；编辑器字符串用 `shlex` 拆分并通过 `run_interactive` 执行
- **日志**: 使用 `tracing::info!()` / `tracing::debug!()` 而非 `println!`
- **序列化**: 所有配置 struct 都 derive `Serialize + Deserialize + Debug + Clone + PartialEq`
- **rename_all**: workspace status 使用 `#[serde(rename_all = "snake_case")]`
- **workspace status 展示**: 用户可见 status 字符串统一使用 `WorkspaceStatus::as_str()`，不要从 `Debug` 派生后手动 lowercase
- **untagged enum**: `HookValue` 使用 `#[serde(untagged)]` 支持三种格式
- **multiplexer 分组**: 所有终端复用器配置统一在 `MultiplexerConfig` 中（`src/config/global.rs`），字段用 `#[serde(default)]` 嵌入各配置 struct；默认 `kind = "zellij"`；Zellij 支持 `layouts/<name>.kdl`，cmux group-aware 模式当前只支持 `layout = "default"`，Herdr 首版仅配置显式 named `session`
- **multiplexer kind 展示**: 用户可见的 adapter 名称统一使用 `MultiplexerKind::as_str()`，不要在调用模块重复维护枚举到字符串的映射
- **cmux group state**: cmux mode maps one zootree workspace to one cmux workspace group. `workspace-group create --from <first-repo>` creates a default header/anchor; zootree then creates its own anchor workspace with the `zootree info` layout, uses `workspace-group set-anchor`, and closes the generated default anchor. Legacy `cmux_group` / `cmux_repo_workspaces` / `cmux_workspace` / `cmux_anchor_workspace` remain readable；成功 activate 后统一写入 `multiplexer_state` 的 `version = 1`、`adapter = "cmux"` 和 private payload，不再写 legacy shape。
- **terminal environment stored state**: workspace TOML 继续使用 `[multiplexer_state]`；配置层把它当作 opaque carrier。当前 envelope 使用 `version = 1`、`adapter` 与私有 `payload`，unknown version 也必须 round-trip；只有成功 `activate` 可以写出规范 envelope，不做后台批量迁移。
- **Zellij terminal environment state**: Zellij 成功 activate 后在 private payload 中保存 session name；stored session 失效时按 `zootree-<workspace-name>` 恢复。default/custom KDL 和 agent CLI 解析都留在 Zellij adapter，且 AgentIntent 只在创建新 session 时生效。
- **Herdr terminal environment state**: Herdr 0.8.0+ 成功 activate 后在 private payload 保存 named session、workspace ID 和当前 label；stored state session 优先于后来变更的 global/workspace config。按 ID、stored label、当前派生 label 精确唯一恢复；仅关闭 owned workspace，不管理共享 server/session。
- **shellexpand**: 所有用户输入的路径在使用前都要 `shellexpand::tilde()` 展开 `~`
- **config-backed names**: 用来派生配置文件路径的 repo/workspace/template 名称必须通过 `config::name::validate_config_name` 校验；只允许非空 ASCII 字母、数字、`-` 和 `_`

## 常见开发任务

### 给 RepoConfig 添加新字段

1. 在 `src/config/repo.rs` 的 struct 中添加字段（带 `#[serde(default)]` 如果不必须）
2. 在 `src/cli/repo.rs` 的 `RepoCommands::Add` 中添加对应的 CLI 参数
3. 在使用该配置的地方（如 `workspace.rs` handle_start）处理新字段

### 添加新的 Hook 事件

1. 在 `src/config/global.rs` 的 `HooksConfig` 中添加 `pub <hook_name>: Option<HookValue>`
2. 在对应功能点调用 `hook_engine.execute_if_set(&config.hooks.<hook_name>, &ctx)`
3. 构造 `HookContext` 时填充相关字段

### 给新命令添加动态补全

1. 确认候选数据来源（workspace/repo/template）；如需新增类别，在 `src/core/completers.rs` 中新增 `complete_<thing>_with(mgr, current)` 和 `complete_<thing>(current)`，遵循「失败返回 vec![]」原则；workspace 补全要用 `list_workspaces_with_status` 直接拿状态并生成 help 文案
2. 在对应 `Args` 字段加 `add = ArgValueCompleter::new(|c: &OsStr| complete_<thing>(c))`
3. 在 `tests/completions_test.rs` 添加：基本列表、前缀过滤、描述包含正确字段三个测试
4. 静态值（如固定枚举）改为 `clap::ValueEnum`，clap 自动补全

### 添加新的 TUI 视图

1. 在 `src/tui_app/` 下新建模块 `<name>.rs` 并在 `src/tui_app/mod.rs` 加 `pub mod <name>;`
2. 实现 `App` trait：`on_event` / `render` / `should_quit`，需要定时刷新则覆写 `tick_interval`
3. 入口调用 `tui_app::run_app(app)`；渲染测试用 `ratatui::backend::TestBackend` + `Terminal::draw`
4. 事件处理测试直接调 `<App>::on_event` 并断言状态变化，不必进真实终端

## Skill 自我迭代

**核心规则：每次对 zootree 代码做出结构性变更后，必须同步更新本 skill 文件。**

### 什么时候需要更新 skill

| 变更类型 | 需要更新的 skill 章节 |
|----------|----------------------|
| 新增/删除/重命名源文件或模块 | 项目架构 |
| 新增顶级命令或子命令 | 添加新命令 + 项目架构 |
| 新增/移除 crate 依赖 | 关键依赖 |
| 改变核心设计模式（如新增 trait、改变 ConfigManager 接口） | 核心设计模式 |
| 新增编码约定或改变现有约定 | 代码约定 |
| 新增常见开发任务模式 | 常见开发任务 |
| 改变测试模式或测试文件组织方式 | 测试规范 |

### 如何更新

1. **完成代码变更后**，回顾本次改动是否属于上表中的变更类型
2. **直接编辑本文件** (`skills/zootree-dev/SKILL.md`)，保持内容与代码同步
3. 更新时遵循以下原则：
   - 项目架构树只反映实际文件结构，用 `find src -type f` 验证
   - 代码示例必须来自真实代码，不要编造
   - 删除已不存在的内容，不要保留过时信息
   - 新增内容保持与现有风格一致（中文描述、代码示例、表格格式）

### 更新检查清单

完成代码修改后，执行以下检查：

```bash
# 验证架构树是否与实际文件一致
find src -type f -name "*.rs" | sort

# 验证模块声明
grep -r "^pub mod\|^mod" src/lib.rs src/cli/mod.rs src/core/mod.rs src/config/mod.rs

# 验证依赖列表
grep "^\[dependencies" Cargo.toml -A 100 | grep -v "^\[" | grep -v "^$" | grep -v "^#"

# 验证 Commands enum
grep -A 30 "enum Commands" src/cli/mod.rs
```

如果任何输出与本 skill 中的描述不一致，立即更新 skill。
