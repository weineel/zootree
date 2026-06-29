# `zootree create` 全屏向导设计

- 日期：2026-06-23
- 作者：weineel + Codex
- 状态：已通过 brainstorming，待 writing-plans

## 背景

`zootree create` 当前在参数不足时使用一串 inline prompt 收集信息：

1. title
2. description
3. repos
4. 每个 repo 的 target branch

这套流程已经解决了 CJK、多行输入、repo 默认选中等基础问题，但它仍然是线性的。用户在后面步骤无法方便回看或修改前面的选择，也看不到完整 workspace draft。`create --start` / `--run-agent` 已经存在，但交互式 create 默认仍只创建 pending workspace，用户常用的“创建后立即启动/启动 agent”需要提前传参数。

本次目标是把 **交互式 `zootree create`** 重做为全屏分步向导，让用户可以在一个 draft 中前进、后退、查看摘要并确认后提交。

## 目标

- 只替换 `zootree create` 在缺少必要交互信息时的路径。
- 保留完整参数化路径：带 `--title`、`--repos`、`--template` 等足够参数时继续按现有非交互逻辑执行。
- 新增一个全屏 `CreateWizardApp`，使用现有 `tui_app::run_app` alternate screen runner。
- 使用分步向导体验，而不是 commander/dashboard：
  1. Workspace info
  2. Repos
  3. Target branches
  4. After create
  5. Review
- 宽屏使用“当前 step + live draft summary”双栏布局。
- 窄屏使用响应式单列布局，不回退旧 inline prompt。
- 把 `create only` / `create and start` / `create, start and run agent` 纳入向导最后的可选项。
- template 作为 repo 选择页里的 `Apply template` 辅助动作，而不是主流程分支。

## 非目标

- 不重写 `handle_start`。
- 不改变 `start` 子命令行为。
- 不改变 repo、workspace、template 的配置文件格式。
- 不把 `src/tui.rs` 的通用 inline prompt API 改成全屏 prompt。
- 不为其他命令引入通用 wizard framework。
- 不做终端像素级 snapshot 测试。

## 范围边界

`CreateArgs` 的参数语义保持现状：

- `--title`、`--description`、`--name`、`--branch`、`--repos`、`--template` 继续可跳过对应交互。
- `--start`、`--run-agent` 继续可从命令行直接指定。
- 当用户进入全屏向导时，命令行参数作为初始 draft 值；用户可以在向导中确认或修改。
- 纯非交互路径不进入 `CreateWizardApp`，避免脚本调用受到 TUI 影响。

## 架构

新增模块：

```text
src/tui_app/create_wizard.rs
```

主要类型：

```rust
pub struct CreateWizardApp { ... }
pub struct CreateDraft { ... }
pub enum CreateStep { Info, Repos, Branches, AfterCreate, Review }
pub enum AfterCreateMode { CreateOnly, Start, StartAndRunAgent }
pub struct CreateWizardOutput { ... }
```

`CreateWizardApp` 实现现有 `tui_app::App`：

```rust
impl App for CreateWizardApp {
    fn on_event(&mut self, event: Event) -> anyhow::Result<()>;
    fn render(&mut self, frame: &mut ratatui::Frame);
    fn should_quit(&self) -> bool;
}
```

`src/cli/workspace.rs` 负责：

1. 加载 config/global。
2. 解析命令行参数。
3. 当需要交互时构造 `CreateDraft` 初始值并调用 `run_app(CreateWizardApp)`.
4. 将 `CreateWizardOutput` 转换为现有 `WorkspaceConfig`。
5. 保存 workspace 和 `recently` template。
6. 根据 `AfterCreateMode` 决定是否复用 `handle_start`。

为了避免 `handle_create` 继续膨胀，建议提取纯函数：

- `build_create_draft(...) -> Result<CreateDraft>`
- `workspace_from_create_output(...) -> WorkspaceConfig`
- `save_created_workspace(...) -> Result<()>`
- `start_after_create_if_needed(...) -> Result<()>`

这些函数服务于 create 流程，不暴露成通用 framework。

## Wizard 流程

### Step 1: Workspace info

字段：

- `title`：必填。
- `description`：可空，多行。
- `name`：可选编辑。默认使用 `NameGenerator::generate_avoiding`。

派生字段：

- `branch` 默认是 `<global.branch_prefix>/<name>`。
- `workspace_dir` 默认是 `<global.workspace_root>/<name>`。

当用户修改 name 时，如果 branch 仍是自动派生值，同步更新 branch；如果用户已经手动改过 branch，则不再覆盖。

### Step 2: Repos

主区域是 repo 多选列表：

- 默认预选当前 git repo。
- 如果当前 git repo 未注册，沿用现有 `ensure_current_repo_registered` 逻辑自动注册。
- 支持 filter。
- `Space` 切换勾选。

辅助动作：

- `Apply template`：打开 template 选择列表，将 template 的 repo 集合应用到当前选择。
- 应用 template 后仍允许手动增删 repo。
- `recently` 和用户自定义 template 一视同仁。

校验：

- 至少选择一个 repo。

### Step 3: Target branches

逐 repo 展示 target branch：

- 当前 repo 默认当前分支。
- 其他 repo 优先 `repo_config.default_target_branch`。
- 没有 config default 时使用该 repo 当前分支。

用户可以编辑每个 target branch。空值不允许提交。

### Step 4: After create

三种模式：

- `Create only`：仅保存 pending workspace。
- `Create and start`：保存后复用 `handle_start`。
- `Create, start and run agent`：保存后复用 `handle_start` 并传入 `run_agent`。

run agent 值：

- 可选择已有 agent alias。
- 可输入字面命令。
- 可留空表示使用 `global.agent_cli`。

如果选择默认 agent 但 `global.agent_cli` 缺失，向导内显示校验错误，不到提交后才失败。

### Step 5: Review

显示完整 draft：

- title
- description 首行或多行摘要
- name
- branch
- workspace dir
- repos 和 target branches
- after-create 行为
- run agent 值或默认来源

确认后提交。用户可以返回任意前序 step 修改。

## 布局

### 宽屏

当终端宽度足够时使用双栏：

- 左侧：当前 step 内容。
- 右侧：live draft summary。
- 底部：help bar。

建议阈值：`width >= 100` 使用双栏。

### 窄屏

低于阈值时使用单列：

- 当前 step 占据主区域。
- 顶部或底部显示紧凑 summary，例如 `name · repos count · after-create mode`。
- `s` 切到完整 summary/review 视图。
- 不回退旧 inline prompt。

极窄终端仍需要最低可用宽度。建议 `width < 50` 时显示“terminal too narrow, resize to at least 50 columns”，但不切换到旧 prompt。

## 按键

全局：

- `Tab` / `Shift+Tab`：切换当前 step 内字段。
- `Enter`：下一步或确认当前动作。
- `Esc`：返回上一步；第一步时取消 create。
- `Ctrl+C`：任意位置硬退出。
- `s`：窄屏时切换 summary。

列表：

- `Up` / `Down`：移动。
- `j` / `k`：移动别名。
- `Ctrl+n` / `Ctrl+p`：移动别名。
- `Space`：切换勾选。

底部 help bar 始终显示当前 step 可用动作。

## 错误处理

字段级错误留在向导内：

- title 为空。
- workspace name 与已有 workspace 冲突。
- repo 未选择。
- target branch 为空。
- run agent 默认值缺失。

系统级错误继续 `anyhow` 冒泡：

- 配置读写失败。
- git 命令失败。
- workspace 保存失败。
- `handle_start` 失败。

提交后如果 create 保存成功但 start 失败，保留 pending workspace，让用户可以手动 `zootree start <name>` 重试。这与当前 `create --start` 语义一致。

取消语义：

- 第一页 `Esc` 或任意位置 `Ctrl+C` 不创建 workspace。
- 退出 alternate screen 后输出 `aborted`，使用现有 `CancelledByUser` 风格的干净错误。

## 测试策略

优先测试纯状态和转换逻辑，不做脆弱的终端截图测试。

覆盖点：

1. `CreateDraft` 默认值：
   - 当前 repo 预选。
   - 未注册当前 repo 自动注册。
   - name、branch、workspace dir 默认派生。
2. template 应用：
   - 应用 template 更新 repo selection。
   - 应用后仍允许手动增删 repo。
3. target branch 决策：
   - 当前 repo 用当前分支。
   - 其他 repo 用 config default。
   - 无 config default 时用当前分支。
4. after-create 决策：
   - create only 不调用 start。
   - start 调用 `handle_start` 等价路径。
   - start and run agent 正确携带 `run_agent`。
   - 默认 agent 缺失时 wizard 校验失败。
5. submit 转换：
   - `CreateDraft` 生成的 `WorkspaceConfig` 与现有 create 语义一致。
   - `recently` template 正确保存 repo 列表。
6. 响应式布局：
   - 宽度足够时双栏。
   - 低于阈值时单列。
   - 极窄时显示 resize 提示。

## 手动验收

- `zootree create` 打开全屏 wizard。
- 可前进、后退，并保留已编辑字段。
- 当前 repo 默认选中。
- template 可以应用到 repo 选择。
- target branch 默认值符合当前规则。
- review 后 `Create only` 只生成 pending workspace。
- review 后 `Create and start` 进入现有 start 流程。
- review 后 `Create, start and run agent` 进入现有 run-agent 流程。
- 小宽度终端仍可完成流程，summary 可查看。
- `zootree create --title t --repos foo` 等完整参数化路径不进入 wizard。

## 影响范围

预计修改：

- `src/cli/workspace.rs`
- `src/tui_app/mod.rs`（仅在需要扩展 event 或 helper 时改）
- `src/tui_app/create_wizard.rs`（新增）
- `src/tui_app/mod.rs` 增加 `pub mod create_wizard;`
- `tests/*` 增加 create draft / output 转换相关测试

不应修改：

- workspace config schema
- repo config schema
- start/open/done/cancel/prune 的交互路径
- `src/tui.rs` 的公共 prompt API
