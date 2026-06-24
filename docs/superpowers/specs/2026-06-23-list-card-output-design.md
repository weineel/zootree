# `zootree list` 紧凑卡片输出设计

- 日期：2026-06-23
- 状态：已通过 brainstorming，待 writing-plans

## 背景与目标

`zootree list` 当前把每个 workspace 压成一行：

```text
  pure-vine (in_progress) - title [zootree:main] /Users/lijufeng/zootree-workspaces/pure-vine
```

当 title、repo target branch 或 workspace dir 较长时，窄屏下横向信息堆叠严重，阅读成本高。用户同时明确希望保留旧的一行输出，方便继续接 `fzf`、脚本或其它管道工具。

目标是把默认的人眼浏览输出改成紧凑卡片式，同时通过 `--oneline` 保留旧格式。

## 非目标

- 不改 workspace 的读取、过滤、状态流转或排序逻辑。
- 不新增交互式 TUI、分页、滚动、颜色或宽度自适应。
- 不把 `zootree list` 扩展成 `zootree info` 的替代品；不展示 created_at、description、recent events。
- 不改变 `--status` 过滤行为。

## 输出行为

默认 `zootree list` 使用紧凑卡片式输出。每个 workspace 固定展示核心字段：

```text
pure-vine  [in_progress]  zootree/pure-vine
  title: zootree list 每项都堆在一行显示再窄屏时可视化效果太差
  repos: zootree:main
  dir:   /Users/lijufeng/zootree-workspaces/pure-vine

true-owl  [in_progress]  zootree/true-owl
  title: feat(PBI-9623): 每轮回答结束后, 添加推荐问题
  repos: hiro-sse:feature/recommendation-question, f-hiro:feature/PBI-9623
  dir:   /Users/lijufeng/zootree-workspaces/true-owl
```

字段规则：

- 首行：`{name}  [{status}]  {branch}`
- 第二行：`title: {workspace.title}`
- 第三行：`repos: {repo:target_branch, ...}`
- 只有 `in_progress` workspace 显示 `dir: {workspace_dir}`
- 非 `in_progress` workspace 不显示 `dir`，保持现有语义
- workspace 之间空一行
- 如果 repos 为空，显示 `repos: (none)`

`zootree list --oneline` 保留当前一行格式：

```text
  pure-vine (in_progress) - title [zootree:main] /Users/lijufeng/zootree-workspaces/pure-vine
```

兼容性要求：

- `--oneline` 不改变字段顺序、缩进、分隔符和 `in_progress` 才显示 workspace dir 的规则。
- `--status` 可以和默认卡片模式或 `--oneline` 同时使用。
- formatter 中的状态文本继续使用现有 `pending`、`in_progress`、`done`、`canceled`。
- Clap 的输入枚举继续使用现有 `kebab-case` 解析规则；这不影响输出文本。

## CLI 设计

`ListArgs` 增加一个布尔参数：

```rust
#[arg(long, help = "Use the legacy one-line output format")]
pub oneline: bool,
```

行为矩阵：

```text
zootree list
  -> 默认紧凑卡片

zootree list --status in-progress
  -> 过滤后仍用紧凑卡片

zootree list --oneline
  -> 旧单行格式

zootree list --status in-progress --oneline
  -> 过滤后旧单行格式
```

## 架构设计

`handle_list` 保持为薄协调层：

1. 解析 status filter。
2. 调用 `ConfigManager::list_workspaces(...)`。
3. 空列表时继续输出 `no workspaces found` 并返回。
4. 对每个 workspace 调用 `load_workspace(&ws.name)` 获取真实 status。
5. 组装内部展示模型。
6. 根据 `args.oneline` 调用对应 formatter。
7. `print!("{output}")`。

建议增加内部展示模型：

```rust
struct ListWorkspaceItem {
    status: WorkspaceStatus,
    workspace: WorkspaceConfig,
}
```

新增两个私有 formatter：

```rust
fn render_list_cards(items: &[ListWorkspaceItem]) -> String
fn render_list_oneline(items: &[ListWorkspaceItem]) -> String
```

`render_list_oneline` 复制现有输出格式，作为兼容性锁点。`render_list_cards` 只负责默认卡片输出。这样配置读取、状态过滤和文本布局互相隔离，后续若需要 `--json`、颜色或 width-aware 输出，也能在 formatter 层扩展。

## 测试设计

formatter 单测优先放在 `src/cli/workspace.rs` 的模块内测试中，因为 formatter 是私有实现细节，不需要扩大 public API。

覆盖项：

1. `render_list_oneline_matches_legacy_format`：锁住旧格式，包括 `in_progress` 才带 dir。
2. `render_list_cards_includes_branch_title_repos_and_dir_for_in_progress`。
3. `render_list_cards_omits_dir_for_pending`。
4. `render_list_cards_separates_items_with_blank_line`。
5. `render_list_cards_shows_none_when_repos_empty`。

如果现有 clap 路由测试较少，本次不额外扩张 CLI parse 测试；`ListArgs` 的布尔 flag 由 clap derive 承担。

## 风险与处理

| 风险 | 处理 |
| --- | --- |
| 默认输出改变影响脚本 | 用 `--oneline` 提供旧格式；文档中明确脚本应使用 `--oneline` |
| 一些用户习惯复制旧格式 | `--oneline` 完整保留旧行为，迁移成本低 |
| 默认卡片输出过重 | 采用紧凑卡片，不展示 description、事件、created_at |
| `branch` 与 repo target branch 概念混淆 | 首行 branch 是 workspace branch；repos 行继续显示每个 repo 的 target branch |
| 未来想做宽度自适应 | formatter 边界已拆出，后续可单独扩展 |

## 验收标准

- `zootree list` 默认输出为紧凑卡片式。
- `zootree list --oneline` 输出与改动前的旧格式一致。
- `zootree list --status in-progress` 和 `zootree list --status in-progress --oneline` 都能按状态过滤。
- 新增 formatter 单测通过。
- 至少运行 `cargo test` 或覆盖相关模块的等价测试命令。
