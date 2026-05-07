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
├── main.rs          # 入口点: CLI 解析 + tracing 初始化 + 命令路由
├── lib.rs           # 模块声明
├── cli/             # CLI 命令定义和处理
│   ├── mod.rs       # Cli struct + Commands enum (clap derive)
│   ├── repo.rs      # repo add/list/edit/remove
│   ├── workspace.rs # create/start/list/open/done/cancel
│   ├── template.rs  # template list/show/delete
│   └── prune.rs     # prune 清理
├── config/          # 配置管理
│   ├── mod.rs       # ConfigManager: 配置读写中枢
│   ├── global.rs    # GlobalConfig + HooksConfig + HookValue
│   ├── repo.rs      # RepoConfig + LazyGitConfig
│   ├── workspace.rs # WorkspaceConfig + Event + WorkspaceStatus
│   └── template.rs  # TemplateConfig
├── core/            # 核心功能
│   ├── mod.rs
│   ├── git.rs       # GitOps: worktree/merge/push 等 git 操作
│   ├── hook.rs      # HookEngine + HookContext
│   ├── layout.rs    # LayoutRenderer: KDL 模板变量替换
│   ├── zellij.rs    # ZellijOps: session 管理
│   ├── copy_files.rs # 文件复制逻辑
│   └── name_gen.rs  # 工作空间名称生成器
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

### ConfigManager 模式

`ConfigManager` 是配置读写的中枢，不依赖外部命令（不需要 runner）。
- 初始化: `ConfigManager::new()` → `~/.config/zootree/`
- 测试: `ConfigManager::with_base_dir(temp_path)` 指向临时目录
- 所有 save/load 使用 `toml` crate 进行序列化

### 命令路由

`main.rs` 中匹配 `Commands` 枚举，每个变体调用对应的 `handle_*` 函数：

```rust
match cli.command {
    Commands::Repo(args) => zootree::cli::repo::handle_repo_command(&args.command)?,
    Commands::Create(args) => zootree::cli::workspace::handle_create(&args)?,
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

所有涉及 git/zellij/shell 的操作使用 `MockRunner`：

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

### 配置测试

使用 `ConfigManager::with_base_dir(temp_dir)` 指向临时目录，避免污染真实配置。

## 关键依赖

| Crate | 用途 |
|-------|------|
| `clap` (4, derive) | CLI 参数解析 |
| `dialoguer` (0.11) | 交互式 TUI (Input, Select, MultiSelect, Confirm) |
| `toml` (0.8) | 配置文件序列化 |
| `serde` (1, derive) | 序列化框架 |
| `kdl` (6) | KDL 布局文件解析 |
| `tracing` + `tracing-subscriber` + `tracing-appender` | 日志系统 |
| `shellexpand` (3) | 路径中的 `~` 展开 |
| `anyhow` (1) | 错误处理 |
| `rand` (0.8) | 名称随机生成 |
| `chrono` (0.4, serde) | 时间戳 |

## 代码约定

- **错误处理**: 统一使用 `anyhow::Result<T>`，用 `anyhow::bail!()` 返回错误
- **可测试性**: 外部命令调用通过 `CommandRunner` trait，不直接调用 `std::process::Command`
- **日志**: 使用 `tracing::info!()` / `tracing::debug!()` 而非 `println!`
- **序列化**: 所有配置 struct 都 derive `Serialize + Deserialize + Debug + Clone + PartialEq`
- **rename_all**: workspace status 使用 `#[serde(rename_all = "snake_case")]`
- **untagged enum**: `HookValue` 使用 `#[serde(untagged)]` 支持三种格式
- **zellij 分组**: 所有 zellij 相关配置统一在 `ZellijConfig` 中（`src/config/global.rs`），字段 Optional，用 `#[serde(default)]` 嵌入各配置 struct
- **shellexpand**: 所有用户输入的路径在使用前都要 `shellexpand::tilde()` 展开 `~`

## 常见开发任务

### 给 RepoConfig 添加新字段

1. 在 `src/config/repo.rs` 的 struct 中添加字段（带 `#[serde(default)]` 如果不必须）
2. 在 `src/cli/repo.rs` 的 `RepoCommands::Add` 中添加对应的 CLI 参数
3. 在使用该配置的地方（如 `workspace.rs` handle_start）处理新字段

### 添加新的 Hook 事件

1. 在 `src/config/global.rs` 的 `HooksConfig` 中添加 `pub <hook_name>: Option<HookValue>`
2. 在对应功能点调用 `hook_engine.execute_if_set(&config.hooks.<hook_name>, &ctx)`
3. 构造 `HookContext` 时填充相关字段

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
