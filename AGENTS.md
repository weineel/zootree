# Repository Guidelines

## Project Structure & Module Organization

zootree 是 Rust 2021 CLI 工具。`src/main.rs` 负责 CLI 解析、tracing 初始化和命令路由，`src/lib.rs` 声明库模块。`src/cli/` 放 clap 命令与 handler，`src/config/` 放 TOML 配置模型与 `ConfigManager`，`src/core/` 放 git、hook、layout、multiplexer、补全和状态检查等核心逻辑。`src/tui_app/` 是 ratatui/crossterm TUI，`src/runner.rs` 提供 `CommandRunner` 测试注入。集成测试位于 `tests/*_test.rs`，项目技能文档位于 `skills/`。

## Build, Test, and Development Commands

- `cargo fmt --check`: 检查 Rust 格式；提交前可用 `cargo fmt --all` 自动修复。
- `cargo clippy -- -D warnings`: 运行静态检查，CI 将 warning 视为失败。
- `cargo test`: 运行全部测试。
- `cargo test completions_test`: 运行单个测试目标。
- `cargo run -- list`: 本地运行 CLI；可替换为 `repo add`、`create`、`info --watch` 等命令。
- `cargo install --path .`: 从当前 checkout 安装本地二进制。
- `cargo release patch --execute`: 维护者本地发版入口，按 `release.toml` 生成 release commit 和 tag。

## Coding Style & Naming Conventions

使用 `cargo fmt` 默认格式和 snake_case 文件/函数命名。错误处理统一返回 `anyhow::Result<T>`，用 `anyhow::bail!()` 构造失败；日志使用 `tracing`，不要在核心逻辑中直接 `println!`。外部命令通过 `CommandRunner` 执行，避免直接调用 `std::process::Command`。用户输入路径在使用前用 `shellexpand::tilde()` 展开。新增结构性模块、命令或依赖时，同步更新 `skills/zootree-dev/SKILL.md`。

## Testing Guidelines

测试文件命名为 `tests/<feature>_test.rs`。涉及 git、zellij、cmux 或 shell 的逻辑使用 `MockRunner` 断言命令调用；配置测试使用 `ConfigManager::with_base_dir(temp_dir)`，不要写入真实 `~/.config/zootree/`。对 CLI 输出、补全、TUI 状态变化和序列化兼容性添加聚焦测试。

## Commit & Pull Request Guidelines

提交信息优先使用 Conventional Commit，例如 `feat: add create wizard`、`fix(config): validate names`、`docs: update release flow`、`chore: release 0.0.9`。PR 描述应包含变更目的、主要行为变化、验证命令和关联 issue；CLI/TUI 输出变化请附示例输出或截图。文档行为变化需同步 `README.md` 与 `README.zh-CN.md`。

## Agent-Specific Instructions

总是使用中文回复。创建 zootree workspace/task 时，任务标题使用 `cz-conventional-changelog`：`<type>(<scope>): <subject>`。`scope` 使用源码模块、命令或行为边界，如 `config`、`layout`、`tui`、`workspace`、`repo`；不要使用内部项目编号、客户编号或 `other`，不明确时省略 scope。拆分任务时，无依赖关系拆成独立 workspace；有共享调用链或强依赖关系放入同一 workspace。需要启动 agent 时使用完整非交互 `zootree create ... --run-agent codexd`，并提供 `--title`、`--description`、`--name`、`--branch` 以及 `--repos` 或 `--template`。
