# `zootree info` 与 TUI 地基设计

- 日期：2026-05-11
- 作者：weineel + Claude
- 状态：已通过 brainstorming，待 writing-plans

## 背景与目标

`zootree start` 启动 zellij 后，overview tab 的第一个 pane 当前跑的是 `zootree list --status in_progress`——展示**所有** in_progress workspace 的扁平列表。对刚启动了某个 workspace、只想集中注意力在**当前** workspace 的用户来说，这个信息面板既冗余又缺关键细节（看不到 title、description、repos 的 target_branch 对照、事件流等）。

本次同时承担两件事：

1. **功能**：新增 `zootree info` 子命令，以完整详细视图展示单个 workspace 的全部可见信息，并把 overview 第一个 pane 的命令替换为它。
2. **地基**：将 `info` 实现为第一个 ratatui 应用，顺带搭建 `src/tui_app/` 下的**薄薄一层** TUI 公共骨架，为后续 TUI 重构铺路。现有 `src/tui.rs`（dialoguer 封装）暂不动，两套共存。

### 非目标（YAGNI）

- 不在本次重写 `src/tui.rs` 的 input/select/confirm 调用点（那是另一次独立重构）
- 不引入文件监听（notify crate）—— 定时轮询已足够
- 不提供 `info --json` 或其它机器可读输出（按需再加）
- 不提供翻滚事件流 / 分页视图（现有事件数量不会爆炸；滚动在下一个 TUI 重构迭代再谈）
- 不做 workspace 间的键盘切换（info 单 workspace 聚焦，切换交给 list）

## 核心决策

| 决策点 | 选定方案 | 备注 |
|--------|----------|------|
| 信息粒度 | 完整详细视图 | title / name / status / branch / dir / created_at / description / repos 对照 / 最近事件 |
| 命令形态 | 顶级 `zootree info [name] [--watch] [--interval <sec>]` | 省略 `name` 时交互式选择；默认打印一次即退出 |
| 刷新机制 | 定时轮询（后台线程 + channel） | 零新增依赖，`--interval` 默认 5s |
| TUI 框架 | ratatui + crossterm | 未来 TUI 重构的统一地基 |
| 本次铺的基 | 薄薄一层 `tui_app` | 抽 `run_app` / `Event` / `App` trait 三件套，约 100 行 |
| `--watch` 退出键 | 不屏蔽 `q`，保留 `Ctrl+C` / `Esc` / `q` 任一退出 | 用户不介意 pane 关闭 |
| 支持的 workspace 状态 | 全部（pending / in_progress / done / canceled） | 只读命令，无理由限制 |

## 架构与文件改动

### 依赖

`Cargo.toml` 新增：

```toml
ratatui = "0.29"
crossterm = "0.28"
```

（版本以撰写计划时的最新稳定版为准，writing-plans 阶段会锁死具体版本号。）

### 新增文件

| 文件 | 职责 |
|------|------|
| `src/tui_app/mod.rs` | TUI 骨架：`run_app` 入口、`Event` 枚举、`App` trait、panic hook 恢复终端 |
| `src/tui_app/info.rs` | `InfoApp`：实现 `App`，持有 `InfoState`（当前 workspace 快照）与渲染逻辑 |
| `src/cli/info.rs` | `InfoArgs` + `handle_info`：解析参数、解析 name、调用 `InfoApp::run` 或一次性打印 |
| `tests/info_test.rs` | 纯函数单测：格式化、状态解析、name 解析回退逻辑 |
| `tests/tui_app_test.rs` | `App` trait 驱动测试：用 Mock backend 验证事件循环与 quit 逻辑 |

### 修改文件

| 文件 | 改动 |
|------|------|
| `src/lib.rs` | 新增 `pub mod tui_app;` |
| `src/cli/mod.rs` | `pub mod info;` + `Commands::Info(info::InfoArgs)` 变体 |
| `src/main.rs` | 新增 `Commands::Info(args) => zootree::cli::info::handle_info(&args)?` 路由分支 |
| `src/core/layout.rs` | `default_layout()` 中的 overview 第一个 pane 由 `"list" "--status" "in_progress"` 改为 `"info" "$workspace_name" "--watch"`；`LayoutVar` 已含 `workspace_name` 字段，替换逻辑已覆盖 |
| `tests/layout_test.rs` | 补一条断言：`default_layout()` 渲染后包含 `"info" "<workspace_name>" "--watch"`，不再包含 `"list" "--status" "in_progress"` |

## 详细设计

### `tui_app` 骨架（`src/tui_app/mod.rs`）

三个要素：`Event`、`App` trait、`run_app`。加上一段在初始化时安装的 panic hook，保证进程 panic 时终端能恢复正常状态（不至于留下 raw mode 的烂终端）。

```rust
pub enum Event {
    Key(crossterm::event::KeyEvent),
    Tick,
    Resize(u16, u16),
}

pub trait App {
    fn on_event(&mut self, event: Event) -> anyhow::Result<()>;
    fn render(&mut self, frame: &mut ratatui::Frame);
    fn should_quit(&self) -> bool;
    fn tick_interval(&self) -> Option<std::time::Duration> { None }
}

pub fn run_app<A: App>(app: A) -> anyhow::Result<()>;
```

`run_app` 职责：

1. 安装 panic hook（恢复终端后再调用原 hook）
2. 进入 raw mode + alternate screen
3. 起一个后台线程：持续 `event::poll` + `event::read`，把键盘/Resize 塞进 channel
4. 如果 `app.tick_interval()` 非 `None`，再起一个线程每 N 秒发 `Event::Tick`
5. 主循环：`render` → `recv` → `on_event` → `should_quit` ? 退出 : 继续
6. 无论正常退出还是错误返回，`Drop`-safe 地退出 raw mode + leave alternate screen

Panic hook 关键点：

```rust
let original = std::panic::take_hook();
std::panic::set_hook(Box::new(move |info| {
    let _ = crossterm::terminal::disable_raw_mode();
    let _ = crossterm::execute!(std::io::stdout(), LeaveAlternateScreen);
    original(info);
}));
```

### `InfoApp`（`src/tui_app/info.rs`）

```rust
pub struct InfoApp {
    name: String,
    config_mgr: ConfigManager,
    state: InfoState,              // 最新一次读取的快照
    watch: bool,
    interval: Duration,
    quit: bool,
    last_error: Option<String>,    // 读取失败时显示，不让 app 崩
}

struct InfoState {
    status: WorkspaceStatus,
    workspace: WorkspaceConfig,
    loaded_at: DateTime<Local>,
}
```

- 构造：`InfoApp::new(name, config_mgr, watch, interval)`，内部立即调用 `reload()` 加载一次
- `reload()`：`config_mgr.load_workspace(&self.name)` → 写入 `state` 或 `last_error`
- `on_event`：
  - `Event::Key(q | Esc | Ctrl+C)` → `quit = true`
  - `Event::Key(r)` → `reload()`（即便不 watch 也可手动刷新）
  - `Event::Tick` → `reload()`
  - `Event::Resize` → 无操作（ratatui 下一帧自动处理）
- `tick_interval()` → `watch` ? `Some(interval)` : `None`
- `render`：见下方布局

### 渲染布局

单屏内垂直分三块（`Layout::vertical` + `Constraint::Length/Min`）：

```
┌─ zootree info — calm-river  [in_progress] ──────────────────────┐   ← title bar (1行)
│                                                                  │
│  Title:     添加导出功能                                          │   ← meta block
│  Branch:    zootree/calm-river                                   │   （高度按内容撑开）
│  Dir:       ~/ws/calm-river                                      │
│  Created:   2026-05-10 14:22                                     │
│                                                                  │
│  Description:                                                    │
│    为报表模块加 CSV 导出，含权限校验                               │
│                                                                  │
├── Repos ─────────────────────────────────────────────────────────┤
│  NAME         TARGET    WORKTREE                                 │   ← repos table
│  frontend     main      ~/ws/calm-river/frontend                 │
│  backend      develop   ~/ws/calm-river/backend                  │
├── Recent events ─────────────────────────────────────────────────┤
│  2026-05-10 14:22  created                                       │   ← events list
│  2026-05-10 14:35  started                                       │
└──────────────────────────────────────────────────────────────────┘
 [q] quit   [r] reload   watching (5s)   updated 14:35:12          ← status line (1行)
```

具体 widget 选型：

- **title bar**：`Paragraph`，内容 `zootree info — {name}  [{status}]`，状态用颜色区分（pending=灰、in_progress=绿、done=蓝、canceled=红）
- **meta block**：`Paragraph` 里用多行 `Line`，label 列右对齐 9 字符后跟值；description 多行展开
- **repos**：`Table` widget，列宽按内容自适应（`Constraint::Length` / `Constraint::Min`）
- **events**：`List` widget，取最后 5 条，时间格式化为 `YYYY-MM-DD HH:MM`
- **status line**：`Paragraph`，左侧热键提示，右侧显示 watch 状态 + 上次刷新时间（`state.loaded_at` 格式化为 `HH:MM:SS`）

边界处理：

- `description` 为空：整个 Description 段不渲染
- `repos` 为空：表格正文显示 `(none)`
- `events` 少于 5：按实际条数显示
- 终端过窄：ratatui 自动按列截断，不做额外处理
- `load_workspace` 失败（比如 workspace 被删了）：title bar 下方显示 `last_error`，其余区块冻结在上次成功的 state

### `InfoArgs` 与 `handle_info`

```rust
#[derive(Args)]
pub struct InfoArgs {
    #[arg(
        help = "Workspace name (interactive if omitted)",
        add = ArgValueCompleter::new(|c: &OsStr| complete_workspace(c, WorkspaceFilter::Any))
    )]
    pub name: Option<String>,

    #[arg(long, help = "Watch mode: auto-refresh periodically")]
    pub watch: bool,

    #[arg(long, default_value = "5", help = "Refresh interval in seconds (with --watch)")]
    pub interval: u64,
}
```

`handle_info` 流程：

1. 解析 `name`
   - 给了就用
   - 没给：`list_workspaces(None)` 拉全部状态，用 `tui::select_one` 选（watch 与否都走这条路径）
2. `load_workspace` 一次，确认存在（失败直接 `bail!`）
3. 分支：
   - **非 watch**：直接 `println!` 以 `key: value` 多行格式输出全部信息（无框线，便于 pipe/脚本化）；repos 用对齐表格；events 按时间顺序列出
   - **watch**：`InfoApp::new(...)` → `tui_app::run_app(app)`，进入上面 ASCII 图所示的 TUI 界面

**动态补全**：复用 `src/core/completers.rs` 已有的 `WorkspaceFilter::Any`（覆盖 pending / in_progress / done / canceled 全部四种状态），无需扩展 completers。

### layout 改动

`src/core/layout.rs` 的 `default_layout()`：

```kdl
tab name="overview" {
    pane size=1 borderless=true { plugin location="tab-bar" }
    pane split_direction="vertical" {
        pane command="zootree" {
            args "info" "$workspace_name" "--watch"
        }
        pane cwd="$workspace_dir"
    }
    pane size=1 borderless=true { plugin location="status-bar" }
}
```

只改第一个子 pane 的 `args`。`$workspace_name` 变量已由 `LayoutRenderer::replace_vars` 处理。

**升级行为**：`launch_zellij` 在 `layout_name == "default"` 分支下调用 `write_default_layout`，**每次启动都会覆盖** `~/.config/zootree/layouts/default.kdl`（文件头部注释 `// 自动生成，修改无效` 即表明这一点）。所以使用默认 layout 的老用户升级 zootree 后，下次 `zootree start` 会自动拿到新的 overview panel，无需任何手动操作。

**例外**：用户如果显式配置了 `zellij.layout = "xxx"`（非 `"default"`），会直接读 `xxx.kdl`（若存在），不会被覆盖。这类自定义 layout 需要用户手动同步 overview 第一个 pane 的 args。这种情况在 CHANGELOG 中提示即可。

## 测试计划

### 纯函数单测（`tests/info_test.rs`）

- `format_created_at` 把 RFC3339 转 `YYYY-MM-DD HH:MM`
- `format_event_time` 同上
- `truncate_events` 只保留最后 N 条（N=0 / N<total / N>=total 三 case）
- `resolve_workspace_name`：给了 name 直接返回；没给 + 空 list 时报错；没给 + 非空按索引选择（用 mock selector）

### TUI 行为测试（`tests/tui_app_test.rs`）

用 ratatui 的 `TestBackend` 做快照比较：

- `InfoApp` 在给定 `InfoState` 下 render 出的 buffer 包含预期文字
- 接收 `Event::Key(q)` 后 `should_quit() == true`
- 接收 `Event::Tick` 会触发 `reload`（用 `MockConfigManager` 或临时目录）

### 集成 smoke（`tests/info_test.rs` 末尾）

- 真跑 `zootree info <name>` 非 watch 模式 → stdout 包含 title / status / repo 名
- `zootree info nonexistent` → 非零退出 + 错误消息

### layout 回归

- `tests/layout_test.rs` 补一条断言：`default_layout()` 渲染后包含 `"info" "<workspace_name>" "--watch"`，不再包含 `"list" "--status" "in_progress"`

## 风险与权衡

| 风险 | 缓解 |
|------|------|
| ratatui + crossterm 在不同终端的兼容性 | 两个库在 helix / gitui / atuin 等成熟项目大规模使用，风险可控；不加 `--watch` 时走一次性 println 输出，天然绕过终端特殊能力依赖 |
| 后台 tick 线程在退出时不优雅 | tick 线程 `send` 失败（主循环已 drop receiver）即 break；主循环 quit 时 drop tx，receiver 自然解除阻塞 |
| watch 模式下 workspace 被 `done` / `cancel`：文件从 in_progress 搬到其它状态目录 | `config_mgr.load_workspace(name)` 本身就扫描所有状态目录，reload 后 `status` 字段自然跟着更新，title bar 的颜色也随之变化；若最终被 `prune` 彻底删除，`load` 失败，写入 `last_error` 并保持上次 snapshot |
| 老用户自定义 layout（非 default）不会自动更新 | default layout 每次 start 都会被自动覆盖，无需处理；自定义 layout 在 CHANGELOG 中提示用户手动同步 |
| panic 时残留 raw mode | 安装 panic hook 恢复终端 |

## 待 writing-plans 阶段细化

- ratatui / crossterm 的精确版本号
- `tui_app::Event` 是否要加 `Event::Error(anyhow::Error)` 以统一上报读取失败（或者保持 App 内部 `last_error` 字段即可）
- `handle_info` 非 watch 模式多行文本输出的精确格式（对齐宽度、空字段处理）
- `WorkspaceFilter::Any` 对 completer 描述字段的影响（状态字段要不要一起显示）
