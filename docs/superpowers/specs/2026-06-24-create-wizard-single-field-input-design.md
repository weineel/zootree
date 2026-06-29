# `zootree create` 字段级输入页设计

- 日期：2026-06-24
- 作者：weineel + Codex
- 状态：已通过 brainstorming，待 implementation plan

## 背景

当前 `zootree create` 已经改成 fullscreen wizard：左侧显示当前 step，右侧或窄屏顶部显示 draft preview。这个结构让用户可以随时看到整体 draft，因此主输入区不需要再同时摆出多个字段。

现有实现为了补齐可编辑能力，在 `Workspace info` 页里仍然同时显示 `title / description / name / branch` 多个字段，并通过局部 edit mode 处理 `Branches` 和 `After create` 的文字输入。这能工作，但模型不够清楚：有些页面是 step，有些页面又在 step 内切 field；`j/k` 导航和文字输入也需要额外冲突处理。

本次目标是把 create wizard 的主输入区收敛为 **一步只处理一个字段或一个控制面**，并用成熟的 textarea 编辑组件承载文本输入。

## 目标

- 每个 wizard page 只处理一个字段或一个控制面。
- 所有文本字段复用一个基于 `ratatui_textarea::TextArea` 的输入状态，而不是继续手写 `String + Backspace`。
- 保留 fullscreen app、draft preview、宽窄屏响应式布局和非交互 CLI 语义。
- 文本输入支持常用编辑能力：
  - 多行内容。
  - paste。
  - `Ctrl+A` / `Ctrl+E`。
  - `Ctrl+W`、`Ctrl+K`、`Ctrl+U` 等现有 inline prompt 已验证的 Emacs 风格编辑键。
  - textarea 内部滚动。
- `Enter` 提交当前字段并进入下一字段页。
- `Alt+Enter` / `Shift+Enter` 在 textarea 中插入换行。

## 非目标

- 不重写 `handle_start`。
- 不改变 workspace、repo、template config schema。
- 不把 create wizard 抽成通用 form framework。
- 不改变完整参数路径：`--title + --repos` 或 `--title + --template` 仍不进 wizard。
- 不迁移到其他 TUI 框架。
- 不做终端像素级 snapshot 测试。

## 页面模型

新增字段级页面枚举，替代当前以大阶段为主的 `CreateStep` 状态机：

```rust
pub enum CreateWizardPage {
    Title,
    Description,
    WorkspaceName,
    WorkspaceBranch,
    Repos,
    TargetBranch { repo_name: String },
    AfterCreate,
    RunAgent,
    Review,
}
```

页面顺序：

1. `Title`
2. `Description`
3. `WorkspaceName`
4. `WorkspaceBranch`
5. `Repos`
6. `TargetBranch { repo_name }` for each selected repo
7. `AfterCreate`
8. `RunAgent` only when `AfterCreateMode::StartAndRunAgent`
9. `Review`

`RunAgent` 是条件字段页：

- `Create only` -> `Review`
- `Create and start` -> `Review`
- `Create, start and run agent` -> `RunAgent` -> `Review`

顶部进度仍显示大阶段，避免 target branch repo 多时进度过长：

- `Workspace`
- `Repos`
- `Branches`
- `After create`
- `Review`

当前页面标题显示具体字段，例如：

- `Workspace: Title`
- `Workspace: Description`
- `Branches: frontend`
- `After create: Run agent`

## 输入组件

新增一个 create wizard 内部输入状态，例如：

```rust
struct WizardTextField {
    textarea: ratatui_textarea::TextArea<'static>,
    kind: WizardTextKind,
}

enum WizardTextKind {
    Multiline,
    SingleLine,
}
```

它只服务 create wizard，不作为公共 prompt framework。

职责：

- 从 draft 当前字段加载文本。
- 渲染当前字段 textarea。
- 接收 `Event::Key` 和 `Event::Paste`。
- 拦截 zootree 级语义键：
  - `Enter`：提交当前字段，不传给 textarea。
  - `Alt+Enter` / `Shift+Enter`：插入换行。
  - `Esc`：返回上一页。
  - `Ctrl+C`：取消。
- 其余编辑键交给 `textarea.input(key)`，复用成熟编辑行为。
- `Event::Paste(text)` 使用 `textarea.insert_str(text)`。

和现有 inline prompt 的关系：

- 复用同一套技术边界：`ratatui_textarea` 负责编辑能力，wizard 自己负责 submit/cancel 语义。
- 可以从 `src/tui_app/prompt.rs::TextPromptState` 复制或提取小范围 helper，但不需要把 inline prompt 改成通用抽象。

## 字段提交规则

字段页进入时调用 `enter_page(page)`：

- 从 `CreateDraft` 或选中 repo 读取当前值。
- 初始化 `WizardTextField`。
- 清空当前字段错误。

`Enter` 调用 `commit_current_page()`：

- 从 textarea 读取文本。
- 按字段做 trim / validation。
- 成功后写回 `CreateDraft`，进入下一字段页。
- 失败时停留当前页，只显示当前字段错误。

`Esc`：

- 回到上一字段页。
- 丢弃当前未提交编辑。
- 第一页 `Esc` 取消 create。

字段规则：

- `Title`
  - 单行字段。
  - trim 后必须非空。
  - 包含换行时报当前字段错误。
- `Description`
  - 多行字段。
  - 允许空。
- `WorkspaceName`
  - 单行字段。
  - trim 后必须非空。
  - 提交时调用 `CreateDraft::set_name`。
  - 如果 branch 未被手动编辑，同步派生 branch。
  - 总是同步 `workspace_dir`。
- `WorkspaceBranch`
  - 单行字段。
  - trim 后必须非空。
  - 提交时调用 `CreateDraft::set_branch`，设置 `branch_was_edited = true`。
- `Repos`
  - 保留列表页。
  - 顶部提供 filter 输入区，filter 使用同一个 textarea-backed 输入状态。
  - `Space` 切换 repo。
  - 至少选择一个 repo。
  - 该页仍然是 repo selection 控制面；filter 只缩小列表，不把 Repos 页变成普通文本字段。
- `TargetBranch { repo_name }`
  - 单行字段。
  - trim 后必须非空。
  - 每个 selected repo 一个独立字段页。
- `AfterCreate`
  - 选择页，不是 textarea。
  - 三个模式：create only、create and start、create/start/run agent。
- `RunAgent`
  - 单行字段。
  - 允许空。
  - 空表示使用 `global.agent_cli`。
  - 如果空且 `global.agent_cli` 缺失，当前字段页显示错误，不进入 Review。
- `Review`
  - 只读完整 draft。
  - 提交前做最终全量校验，防御动态页面变化遗漏。

## 动态页面构建

`CreateWizardApp` 不再只用 `CreateStep::next()` / `prev()`。

建议持有：

```rust
pages: Vec<CreateWizardPage>,
page_index: usize,
text_field: Option<WizardTextField>,
draft: CreateDraft,
repo_cursor: usize,
after_create_cursor: usize,
errors: Vec<CreateDraftError>,
```

`pages` 由 `draft` 动态构建：

- 基础 workspace 字段页始终存在。
- `TargetBranch` 页来自当前 selected repos。
- `RunAgent` 页只在 after-create 选择 run-agent 时存在。

当 repo selection 变化：

- 重建 target branch pages。
- 如果当前页对应 repo 被取消选择，跳到下一个仍存在的 target branch 页；没有则跳到 `AfterCreate`。
- clamp `repo_cursor`。

当 after-create mode 变化：

- 如果切到 run-agent，加入 `RunAgent` 页。
- 如果从 run-agent 切走，移除 `RunAgent` 页。
- 如果当前就在 `RunAgent` 页并切走，跳到 `Review` 或 `AfterCreate`，以最少惊讶为准。

## 渲染

沿用当前响应式布局：

- `width >= 100`：双栏。
  - 左侧：当前字段页。
  - 右侧：draft preview。
  - 底部：help bar。
- `50 <= width < 100`：单列。
  - 紧凑 draft summary。
  - 当前字段页。
  - help bar。
- `width < 50`：显示 resize message，不回退 inline prompt。

当前字段页渲染规则：

- 文本字段：显示一个 textarea block。
- Repos：显示 filter 输入区和 repo 列表。
- AfterCreate：显示模式列表。
- Review：显示完整 draft。

Draft preview 继续展示整体上下文：

- title
- description 摘要
- name
- branch
- workspace_dir
- repos 和 target branches
- after-create 行为
- run agent 值或默认来源

如果当前文本字段有未提交编辑，preview 可以显示已提交值；当前页 block 本身显示正在编辑的值。先不做“dirty preview”叠加，避免复杂化。

## 按键语义

全局：

- `Enter`：提交当前字段 / 当前控制面并进入下一字段页。
- `Esc`：返回上一字段页；第一页取消。
- `Ctrl+C`：任意页取消。
- `Alt+Enter` / `Shift+Enter`：文本字段插入换行。

文本字段：

- 语义键之外的输入交给 `ratatui_textarea::TextArea::input`。
- `Paste` 插入文本。
- 单行字段在提交时拒绝换行。

Repos 页：

- `Up` / `Down`、`j` / `k`、`Ctrl+n` / `Ctrl+p`：移动 repo cursor。
- `Space`：切换 repo selection。
- 如果 filter 激活，普通字符和 paste 进入 filter 输入；移动键仍保留列表导航。

AfterCreate 页：

- `Up` / `Down`、`j` / `k`、`Ctrl+n` / `Ctrl+p`：移动模式。
- `Enter`：提交选择。

`Tab` / `Shift+Tab` 不再用于多个字段间移动，因为字段已经拆成页面。它们可以保留给页面内部辅助焦点，例如 Repos filter 激活/退出；如果当前页面没有内部焦点，则 no-op。

## CLI 参数语义

保留上一轮已确认语义：

- 完整非交互 `--title + --repos`：不进入 wizard。
- 完整非交互 `--title + --template`：不进入 wizard，只使用 template repos。
- 半交互 `--template` 缺 title：进入 wizard，Repos 页显示全部 repo，并将 template repos 预选，用户可以调整。
- `--repos` 与 `--template` 同时传入时，`--repos` 优先。
- 只有 repo source 缺失时，才允许当前 git repo 自动注册 / 默认选中。

## 测试策略

重点测试纯状态、输入行为和转换逻辑，不做像素级 snapshot。

新增或调整测试：

- 字段页顺序：
  - Title -> Description -> WorkspaceName -> WorkspaceBranch -> Repos -> target branch pages -> AfterCreate -> conditional RunAgent -> Review。
- 文本输入：
  - `Enter` 提交字段。
  - `Alt+Enter` / `Shift+Enter` 插入换行。
  - `Paste` 插入文本。
  - `Ctrl+A` / `Ctrl+E` 通过 textarea 生效。
  - `Ctrl+W` / `Ctrl+K` / `Ctrl+U` 覆盖关键编辑行为。
- 单行字段：
  - 提交时 trim。
  - 包含换行时报字段错误。
- Description：
  - 支持多行。
  - 允许空。
- WorkspaceName：
  - 提交后更新 `workspace_dir`。
  - branch 未手动编辑时同步默认 branch。
- WorkspaceBranch：
  - 提交后 `branch_was_edited = true`。
  - 后续 name 修改不覆盖 branch。
- Repos：
  - 至少选择一个 repo。
  - selection 变化后 target branch pages 动态更新。
  - template 半交互显示全 repo + template repos selected。
- TargetBranch：
  - 每个 selected repo 一个字段页。
  - 空 target branch 阻塞当前页。
- AfterCreate / RunAgent：
  - run-agent 模式插入 RunAgent 页。
  - 非 run-agent 模式跳过 RunAgent 页。
  - RunAgent 空值使用 global default。
  - 无 default 时当前页报错。
- Render smoke：
  - 宽屏、窄屏、极窄仍覆盖。
  - 文本字段页只显示一个 textarea 输入区。
- Non-interactive create：
  - 现有 CLI path 测试保持通过。

## 迁移计划概览

1. 引入 `CreateWizardPage` 和 page builder，保留旧 tests 作为基线。
2. 引入 `WizardTextField`，先覆盖 Title 字段红绿测试。
3. 迁移 Workspace text pages：Title、Description、Name、Branch。
4. 迁移 Repos 页面，保留多选列表，并接入 filter 输入。
5. 迁移 TargetBranch 动态字段页。
6. 迁移 AfterCreate / RunAgent 条件页。
7. 更新渲染和 help bar。
8. 删除旧 `CreateStep` 主状态机和临时 edit mode。
9. 跑 focused tests、全量 tests、隔离 CLI smoke。

## 自检

- 无 TBD / TODO。
- 页面模型与“每次只处理一个字段”一致。
- 输入组件明确复用 `ratatui_textarea`，不继续手写编辑器。
- CLI 非交互语义保持原样。
- scope 限定在 create wizard，不扩展为通用 form engine。
