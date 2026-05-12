# `zootree info` 与 TUI 地基 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 新增 `zootree info` 子命令，以 ratatui TUI 展示单个 workspace 的完整详细信息，并把默认 zellij overview tab 的第一个 pane 改为使用它；同时搭建最小化的 `src/tui_app/` 公共骨架作为后续 TUI 重构的地基。

**Architecture:** 在 `src/tui_app/mod.rs` 提供薄薄一层抽象（`Event` 枚举 + `App` trait + `run_app` 单线程事件循环），`src/tui_app/info.rs` 实现第一个应用 `InfoApp`（持有 `ConfigManager`、workspace 快照、定时重载）。新子命令入口在 `src/cli/info.rs`：非 `--watch` 走多行 `println!`；`--watch` 进入 TUI。默认 layout 同步更新，`$workspace_name` 已由现有 `LayoutRenderer` 支持。

**Tech Stack:** Rust 2021 + ratatui 0.29 + crossterm 0.28 + chrono + anyhow + clap。TUI 行为测试用 `ratatui::backend::TestBackend`，配置测试用 `ConfigManager::with_base_dir(tempdir)`。

**Key Facts (Don't re-derive):**
- `ConfigManager::load_workspace(name)` 已会自动扫 pending / in_progress / archived/done / archived/canceled 四个目录，`reload` 天然跟踪状态迁移
- `WorkspaceFilter::Any` 已存在（`src/core/completers.rs`），可直接用于 `info` 的 name 补全
- `LayoutVar` 已含 `workspace_name` 字段，`replace_vars` 已替换 `$workspace_name`
- 默认 layout 每次 `zootree start` 都会被 `write_default_layout` 无条件覆盖（文件头部 `// 自动生成，修改无效`），老用户升级自动生效

---

## 文件结构

```
src/
├── Cargo.toml                    # [modify] 加 ratatui、crossterm
├── lib.rs                        # [modify] 加 pub mod tui_app;
├── tui_app/                      # [new dir]
│   ├── mod.rs                    # [new] Event / App trait / run_app
│   └── info.rs                   # [new] InfoApp + 格式化辅助函数
├── cli/
│   ├── mod.rs                    # [modify] 加 pub mod info; + Commands::Info
│   └── info.rs                   # [new] InfoArgs + handle_info + render_once
├── main.rs                       # [modify] 加 Commands::Info 路由
└── core/
    └── layout.rs                 # [modify] default_layout 第一个 pane args
tests/
├── info_test.rs                  # [new] 集成测试（render_once 等 pub 函数）
└── layout_test.rs                # [modify] 新增 overview args 断言
```

单元测试（涉及 `pub(crate)` 成员或内部状态）就地放入 `src/tui_app/info.rs` 的 `#[cfg(test)] mod tests`，符合项目现有混合风格（参考 `src/cli/workspace.rs::warn_or_bail` 的内联测试）。

---

## Task 1: 添加依赖 + 声明 tui_app 模块

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/lib.rs`
- Create: `src/tui_app/mod.rs`

- [ ] **Step 1: 在 Cargo.toml 的 `[dependencies]` 末尾追加两行**

```toml
ratatui = "0.29"
crossterm = "0.28"
```

- [ ] **Step 2: 修改 `src/lib.rs`，在现有 `pub mod tui;` 行后加一行**

最终 `src/lib.rs` 内容：

```rust
pub mod cli;
pub mod config;
pub mod core;
pub mod runner;
pub mod tui;
pub mod tui_app;
```

- [ ] **Step 3: 创建 `src/tui_app/mod.rs` 占位**

```rust
//! TUI application framework built on ratatui + crossterm.
//!
//! Scaffolding for future interactive views. Populated by subsequent tasks.
```

- [ ] **Step 4: 运行 `cargo build`**

Run: `cargo build`
Expected: 构建成功（会下载 ratatui、crossterm 依赖），无 warning 相关 failure。

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock src/lib.rs src/tui_app/mod.rs
git commit -m "chore: add ratatui + crossterm deps and tui_app module scaffold"
```

---

## Task 2: 定义 Event 枚举和 App trait

**Files:**
- Modify: `src/tui_app/mod.rs`
- Create: `tests/tui_app_test.rs`

- [ ] **Step 1: 先写失败测试 `tests/tui_app_test.rs`**

```rust
use std::time::Duration;
use zootree::tui_app::{App, Event};

struct NoopApp {
    quit: bool,
    last_seen: Option<&'static str>,
}

impl NoopApp {
    fn new() -> Self {
        Self { quit: false, last_seen: None }
    }
}

impl App for NoopApp {
    fn on_event(&mut self, event: Event) -> anyhow::Result<()> {
        match event {
            Event::Key(_) => self.last_seen = Some("key"),
            Event::Tick => self.last_seen = Some("tick"),
            Event::Resize(_, _) => self.last_seen = Some("resize"),
        }
        self.quit = true;
        Ok(())
    }
    fn render(&mut self, _frame: &mut ratatui::Frame) {}
    fn should_quit(&self) -> bool {
        self.quit
    }
}

#[test]
fn default_tick_interval_is_none() {
    let app = NoopApp::new();
    assert_eq!(app.tick_interval(), None);
}

#[test]
fn key_event_can_be_dispatched() {
    let mut app = NoopApp::new();
    let key = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('a'),
        crossterm::event::KeyModifiers::NONE,
    );
    app.on_event(Event::Key(key)).unwrap();
    assert_eq!(app.last_seen, Some("key"));
    assert!(app.should_quit());
}

#[test]
fn tick_event_can_be_dispatched() {
    let mut app = NoopApp::new();
    app.on_event(Event::Tick).unwrap();
    assert_eq!(app.last_seen, Some("tick"));
}

#[test]
fn resize_event_carries_dimensions() {
    let mut app = NoopApp::new();
    app.on_event(Event::Resize(80, 24)).unwrap();
    assert_eq!(app.last_seen, Some("resize"));
}

#[test]
fn custom_tick_interval_overrides_default() {
    struct WatchApp;
    impl App for WatchApp {
        fn on_event(&mut self, _: Event) -> anyhow::Result<()> { Ok(()) }
        fn render(&mut self, _: &mut ratatui::Frame) {}
        fn should_quit(&self) -> bool { false }
        fn tick_interval(&self) -> Option<Duration> { Some(Duration::from_secs(2)) }
    }
    assert_eq!(WatchApp.tick_interval(), Some(Duration::from_secs(2)));
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --test tui_app_test`
Expected: FAIL with "unresolved import `zootree::tui_app::App`" 等。

- [ ] **Step 3: 在 `src/tui_app/mod.rs` 实现 Event + App trait**

完整替换文件内容：

```rust
//! TUI application framework built on ratatui + crossterm.
//!
//! Each interactive view implements the `App` trait; `run_app` drives the
//! terminal setup, event loop, and cleanup.

use std::time::Duration;

pub mod info;

/// Events dispatched to the active `App` on every iteration of the main loop.
pub enum Event {
    Key(crossterm::event::KeyEvent),
    Tick,
    Resize(u16, u16),
}

/// A TUI view. Implementors own their state, decide when to quit, and render
/// one frame at a time.
pub trait App {
    fn on_event(&mut self, event: Event) -> anyhow::Result<()>;
    fn render(&mut self, frame: &mut ratatui::Frame);
    fn should_quit(&self) -> bool;

    /// Override to request periodic `Event::Tick` delivery. `None` disables
    /// ticks (the loop still polls for key/resize events).
    fn tick_interval(&self) -> Option<Duration> {
        None
    }
}
```

（`pub mod info;` 预先引入——后续任务填充 `info.rs`；目前模块为空即可。）

- [ ] **Step 4: 创建 `src/tui_app/info.rs` 空占位**

```rust
//! `InfoApp`: detailed single-workspace view.
//!
//! Populated by subsequent tasks.
```

- [ ] **Step 5: 跑测试确认通过**

Run: `cargo test --test tui_app_test`
Expected: PASS（5 个用例）。

- [ ] **Step 6: Commit**

```bash
git add src/tui_app/mod.rs src/tui_app/info.rs tests/tui_app_test.rs
git commit -m "feat(tui_app): add Event and App trait"
```

---

## Task 3: 实现 run_app 单线程事件循环

**Files:**
- Modify: `src/tui_app/mod.rs`

这一步没有直接单元测试（`run_app` 依赖真实 stdin/stdout 和全局终端状态）。冒烟验证延后到 Task 8 的手动 `zootree info ... --watch`。

**设计备注（供后续维护者）**：spec 原写"后台线程 + channel"，实际用**单线程 `event::poll(timeout)` 方案**——功能等价、零新增同步原语、零 `mpsc::channel`，更容易推理与回归。

- [ ] **Step 1: 在 `src/tui_app/mod.rs` 末尾追加 `run_app` 实现**

```rust
use std::io;
use std::time::Instant;

use crossterm::event::{self, Event as CtEvent};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

/// Run a TUI application: enter alternate screen + raw mode, drive the main
/// loop, then restore the terminal on exit (and on panic).
pub fn run_app<A: App>(mut app: A) -> anyhow::Result<()> {
    install_panic_hook();

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let loop_result = main_loop(&mut terminal, &mut app);

    // Always restore the terminal, even if the loop errored.
    let _ = disable_raw_mode();
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
    let _ = terminal.show_cursor();

    loop_result
}

fn main_loop<B: ratatui::backend::Backend, A: App>(
    terminal: &mut Terminal<B>,
    app: &mut A,
) -> anyhow::Result<()> {
    // Poll budget: tick interval if set, otherwise a reasonable idle timeout so
    // we still respond to keys quickly when ticks are off.
    let tick_rate = app.tick_interval().unwrap_or(Duration::from_millis(250));
    let mut last_tick = Instant::now();

    loop {
        terminal.draw(|f| app.render(f))?;

        if app.should_quit() {
            return Ok(());
        }

        let timeout = tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or(Duration::ZERO);

        if event::poll(timeout)? {
            match event::read()? {
                CtEvent::Key(k) => app.on_event(Event::Key(k))?,
                CtEvent::Resize(w, h) => app.on_event(Event::Resize(w, h))?,
                _ => {}
            }
        }

        if last_tick.elapsed() >= tick_rate {
            if app.tick_interval().is_some() {
                app.on_event(Event::Tick)?;
            }
            last_tick = Instant::now();
        }
    }
}

fn install_panic_hook() {
    use std::sync::Once;
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let original = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let _ = disable_raw_mode();
            let _ = execute!(io::stdout(), LeaveAlternateScreen);
            original(info);
        }));
    });
}
```

- [ ] **Step 2: `cargo build` 确保编译**

Run: `cargo build`
Expected: 构建成功。

- [ ] **Step 3: 再跑一遍 Task 2 的测试确保没破坏 trait**

Run: `cargo test --test tui_app_test`
Expected: PASS。

- [ ] **Step 4: Commit**

```bash
git add src/tui_app/mod.rs
git commit -m "feat(tui_app): add run_app single-threaded event loop"
```

---

## Task 4: InfoApp 数据结构 + reload + 格式化辅助函数

**Files:**
- Modify: `src/tui_app/info.rs`

内联测试（`mod tests`）用 `tempfile` + `ConfigManager::with_base_dir` 隔离。`tempfile` 已是 `dev-dependencies`。

- [ ] **Step 1: 在 `src/tui_app/info.rs` 先写辅助函数与测试**

完整替换文件内容为：

```rust
//! `InfoApp`: detailed single-workspace view.

use std::time::Duration;

use chrono::{DateTime, Local};

use crate::config::workspace::{WorkspaceConfig, WorkspaceStatus};
use crate::config::ConfigManager;

pub struct InfoApp {
    pub(crate) name: String,
    pub(crate) config_mgr: ConfigManager,
    pub(crate) state: Option<InfoState>,
    pub(crate) watch: bool,
    pub(crate) interval: Duration,
    pub(crate) quit: bool,
    pub(crate) last_error: Option<String>,
}

pub(crate) struct InfoState {
    pub status: WorkspaceStatus,
    pub workspace: WorkspaceConfig,
    pub loaded_at: DateTime<Local>,
}

impl InfoApp {
    pub fn new(name: String, config_mgr: ConfigManager, watch: bool, interval: Duration) -> Self {
        let mut app = Self {
            name,
            config_mgr,
            state: None,
            watch,
            interval,
            quit: false,
            last_error: None,
        };
        app.reload();
        app
    }

    pub(crate) fn reload(&mut self) {
        match self.config_mgr.load_workspace(&self.name) {
            Ok((status, workspace)) => {
                self.state = Some(InfoState {
                    status,
                    workspace,
                    loaded_at: Local::now(),
                });
                self.last_error = None;
            }
            Err(e) => {
                self.last_error = Some(format!("{:#}", e));
            }
        }
    }
}

/// Parse an RFC3339 timestamp and re-format it in the local zone as
/// `YYYY-MM-DD HH:MM`. On parse failure, returns the original string.
pub fn format_rfc3339_to_minute(s: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Local).format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|_| s.to_string())
}

pub(crate) fn format_time_of_day(dt: &DateTime<Local>) -> String {
    dt.format("%H:%M:%S").to_string()
}

/// Return up to the last `n` elements of the slice, preserving order.
pub fn last_n<T>(items: &[T], n: usize) -> &[T] {
    if items.len() <= n {
        items
    } else {
        &items[items.len() - n..]
    }
}

pub(crate) fn status_label(s: &WorkspaceStatus) -> &'static str {
    match s {
        WorkspaceStatus::Pending => "pending",
        WorkspaceStatus::InProgress => "in_progress",
        WorkspaceStatus::Done => "done",
        WorkspaceStatus::Canceled => "canceled",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::global::ZellijConfig;

    fn sample_workspace(name: &str) -> WorkspaceConfig {
        WorkspaceConfig {
            title: "Demo title".into(),
            name: name.into(),
            description: "line one\nline two".into(),
            branch: format!("zootree/{}", name),
            workspace_dir: format!("/tmp/{}", name),
            created_at: "2026-05-10T14:22:00+08:00".into(),
            zellij: ZellijConfig::default(),
            repos: vec![],
            events: vec![],
        }
    }

    #[test]
    fn format_rfc3339_to_minute_parses_valid() {
        let s = "2026-05-10T14:22:00+08:00";
        // Exact output is timezone-dependent, so just check shape.
        let out = format_rfc3339_to_minute(s);
        assert_eq!(out.len(), 16);
        assert_eq!(&out[4..5], "-");
        assert_eq!(&out[10..11], " ");
        assert_eq!(&out[13..14], ":");
    }

    #[test]
    fn format_rfc3339_to_minute_falls_back_on_invalid() {
        assert_eq!(format_rfc3339_to_minute("not-a-date"), "not-a-date");
    }

    #[test]
    fn last_n_returns_all_when_shorter() {
        let v = vec![1, 2, 3];
        assert_eq!(last_n(&v, 5), &[1, 2, 3]);
    }

    #[test]
    fn last_n_returns_tail_when_longer() {
        let v = vec![1, 2, 3, 4, 5];
        assert_eq!(last_n(&v, 3), &[3, 4, 5]);
    }

    #[test]
    fn last_n_handles_zero() {
        let v = vec![1, 2, 3];
        assert_eq!(last_n(&v, 0), &[] as &[i32]);
    }

    #[test]
    fn status_label_covers_all_variants() {
        assert_eq!(status_label(&WorkspaceStatus::Pending), "pending");
        assert_eq!(status_label(&WorkspaceStatus::InProgress), "in_progress");
        assert_eq!(status_label(&WorkspaceStatus::Done), "done");
        assert_eq!(status_label(&WorkspaceStatus::Canceled), "canceled");
    }

    #[test]
    fn reload_populates_state_for_existing_workspace() {
        let tmp = tempfile::tempdir().unwrap();
        let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
        mgr.ensure_dirs().unwrap();
        let ws = sample_workspace("demo");
        mgr.save_workspace(&WorkspaceStatus::InProgress, &ws).unwrap();

        let mgr_for_app = ConfigManager::with_base_dir(tmp.path().to_path_buf());
        let app = InfoApp::new("demo".into(), mgr_for_app, false, Duration::from_secs(5));

        assert!(app.last_error.is_none());
        let state = app.state.as_ref().expect("state populated");
        assert!(matches!(state.status, WorkspaceStatus::InProgress));
        assert_eq!(state.workspace.name, "demo");
    }

    #[test]
    fn reload_records_error_for_missing_workspace() {
        let tmp = tempfile::tempdir().unwrap();
        let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
        mgr.ensure_dirs().unwrap();

        let app = InfoApp::new("ghost".into(), mgr, false, Duration::from_secs(5));
        assert!(app.state.is_none());
        assert!(app.last_error.is_some());
        assert!(app.last_error.as_deref().unwrap().contains("ghost"));
    }
}
```

- [ ] **Step 2: 跑失败测试**

Run: `cargo test --lib tui_app::info::tests`
Expected: FAIL（类型/函数还没接入 App trait；或者全绿——因为 App trait 实现还没写，lib tests 可通过）。

如已全绿：进入 Step 3，先把本轮单元测试视为实现了；App trait 实现在 Task 5/6。

- [ ] **Step 3: 确认 `cargo build` 通过**

Run: `cargo build`
Expected: 成功。注意：InfoApp 还没实现 `App` trait，但单独编译 `info.rs` 不需要。

- [ ] **Step 4: 跑 lib 单元测试**

Run: `cargo test --lib tui_app::info`
Expected: 7 个测试 PASS。

- [ ] **Step 5: Commit**

```bash
git add src/tui_app/info.rs
git commit -m "feat(tui_app): add InfoApp state, reload, and format helpers"
```

---

## Task 5: InfoApp 渲染（实现 App trait 的 render + 相关辅助）

**Files:**
- Modify: `src/tui_app/info.rs`

使用 `ratatui::backend::TestBackend` 做行为测试：渲染后检查 buffer 文本。

- [ ] **Step 1: 先写失败测试（追加到 `src/tui_app/info.rs` 的 `mod tests`）**

在 `mod tests` 内追加：

```rust
    fn buffer_to_string(buf: &ratatui::buffer::Buffer) -> String {
        let mut out = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    fn render_to_string(app: &mut InfoApp, width: u16, height: u16) -> String {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| {
            <InfoApp as crate::tui_app::App>::render(app, f)
        }).unwrap();
        buffer_to_string(terminal.backend().buffer())
    }

    #[test]
    fn render_shows_name_status_and_title() {
        let tmp = tempfile::tempdir().unwrap();
        let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
        mgr.ensure_dirs().unwrap();
        let ws = sample_workspace("demo");
        mgr.save_workspace(&WorkspaceStatus::InProgress, &ws).unwrap();

        let mgr2 = ConfigManager::with_base_dir(tmp.path().to_path_buf());
        let mut app = InfoApp::new("demo".into(), mgr2, false, Duration::from_secs(5));

        let out = render_to_string(&mut app, 80, 20);
        assert!(out.contains("demo"), "missing name:\n{}", out);
        assert!(out.contains("in_progress"), "missing status:\n{}", out);
        assert!(out.contains("Demo title"), "missing title:\n{}", out);
        assert!(out.contains("zootree/demo"), "missing branch:\n{}", out);
    }

    #[test]
    fn render_shows_last_error_when_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
        mgr.ensure_dirs().unwrap();
        let mut app = InfoApp::new("ghost".into(), mgr, false, Duration::from_secs(5));

        let out = render_to_string(&mut app, 80, 10);
        assert!(out.contains("ghost"), "error should mention name:\n{}", out);
    }

    #[test]
    fn render_shows_repos_row() {
        use crate::config::workspace::RepoEntry;
        let tmp = tempfile::tempdir().unwrap();
        let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
        mgr.ensure_dirs().unwrap();
        let mut ws = sample_workspace("demo");
        ws.repos = vec![RepoEntry {
            name: "frontend".into(),
            target_branch: Some("main".into()),
        }];
        mgr.save_workspace(&WorkspaceStatus::InProgress, &ws).unwrap();

        let mgr2 = ConfigManager::with_base_dir(tmp.path().to_path_buf());
        let mut app = InfoApp::new("demo".into(), mgr2, false, Duration::from_secs(5));
        let out = render_to_string(&mut app, 100, 20);
        assert!(out.contains("frontend"), "missing repo name:\n{}", out);
        assert!(out.contains("main"), "missing target branch:\n{}", out);
    }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --lib tui_app::info::tests::render_`
Expected: FAIL（`App` trait 未实现 `render`、`should_quit`、`on_event`）。

- [ ] **Step 3: 实现 App trait（追加到 `src/tui_app/info.rs` 末尾）**

```rust
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Row, Table};
use ratatui::Frame;

impl crate::tui_app::App for InfoApp {
    fn on_event(&mut self, _event: crate::tui_app::Event) -> anyhow::Result<()> {
        // Filled in by Task 6.
        Ok(())
    }

    fn render(&mut self, frame: &mut Frame) {
        let area = frame.area();

        let chunks = Layout::vertical([
            Constraint::Length(1), // title bar
            Constraint::Min(3),    // body
            Constraint::Length(1), // status line
        ])
        .split(area);

        self.render_title(frame, chunks[0]);
        self.render_body(frame, chunks[1]);
        self.render_status_line(frame, chunks[2]);
    }

    fn should_quit(&self) -> bool {
        self.quit
    }

    fn tick_interval(&self) -> Option<Duration> {
        if self.watch {
            Some(self.interval)
        } else {
            None
        }
    }
}

impl InfoApp {
    fn render_title(&self, frame: &mut Frame, area: Rect) {
        let (title_text, color) = match &self.state {
            Some(s) => (
                format!(
                    "zootree info — {}  [{}]",
                    self.name,
                    status_label(&s.status)
                ),
                status_color(&s.status),
            ),
            None => (format!("zootree info — {}  [?]", self.name), Color::DarkGray),
        };
        let para = Paragraph::new(Span::styled(title_text, Style::default().fg(color)));
        frame.render_widget(para, area);
    }

    fn render_body(&self, frame: &mut Frame, area: Rect) {
        let Some(state) = &self.state else {
            let msg = self
                .last_error
                .clone()
                .unwrap_or_else(|| "loading...".into());
            let para = Paragraph::new(msg).block(Block::default().borders(Borders::ALL));
            frame.render_widget(para, area);
            return;
        };

        let ws = &state.workspace;

        // Compute meta block height: 4 fixed lines (Title/Branch/Dir/Created),
        // plus description block if non-empty (blank line + "Description:" + N lines).
        let desc_height = if ws.description.is_empty() {
            0
        } else {
            2 + ws.description.lines().count() as u16
        };
        let meta_height = 4 + desc_height;

        // Repos block: top border + header + rows (or 1 "(none)" row).
        let repos_rows = ws.repos.len().max(1) as u16;
        let repos_height = 2 + repos_rows;

        let chunks = Layout::vertical([
            Constraint::Length(meta_height),
            Constraint::Length(repos_height),
            Constraint::Min(1),
        ])
        .split(area);

        // Meta
        let mut lines: Vec<Line> = Vec::new();
        lines.push(meta_line("Title:", &ws.title));
        lines.push(meta_line("Branch:", &ws.branch));
        lines.push(meta_line("Dir:", &ws.workspace_dir));
        lines.push(meta_line(
            "Created:",
            &format_rfc3339_to_minute(&ws.created_at),
        ));
        if !ws.description.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from("  Description:"));
            for l in ws.description.lines() {
                lines.push(Line::from(format!("    {}", l)));
            }
        }
        frame.render_widget(Paragraph::new(lines), chunks[0]);

        // Repos
        let rows: Vec<Row> = if ws.repos.is_empty() {
            vec![Row::new(vec![
                "(none)".to_string(),
                "".to_string(),
                "".to_string(),
            ])]
        } else {
            ws.repos
                .iter()
                .map(|r| {
                    let target = r.target_branch.as_deref().unwrap_or("*");
                    let worktree = format!("{}/{}", ws.workspace_dir, r.name);
                    Row::new(vec![r.name.clone(), target.to_string(), worktree])
                })
                .collect()
        };
        let table = Table::new(
            rows,
            [
                Constraint::Length(15),
                Constraint::Length(15),
                Constraint::Min(20),
            ],
        )
        .header(Row::new(vec!["NAME", "TARGET", "WORKTREE"]).style(Style::default().fg(Color::DarkGray)))
        .block(Block::default().borders(Borders::TOP).title(" Repos "));
        frame.render_widget(table, chunks[1]);

        // Events
        let recent = last_n(&ws.events, 5);
        let items: Vec<ListItem> = recent
            .iter()
            .map(|e| {
                let ts = format_rfc3339_to_minute(&e.timestamp);
                let mut text = format!("{}  {}", ts, e.action);
                if let Some(d) = &e.detail {
                    text.push_str(&format!("  ({})", d));
                }
                ListItem::new(text)
            })
            .collect();
        let list = List::new(items)
            .block(Block::default().borders(Borders::TOP).title(" Recent events "));
        frame.render_widget(list, chunks[2]);
    }

    fn render_status_line(&self, frame: &mut Frame, area: Rect) {
        let left = "[q] quit   [r] reload".to_string();
        let right = if let Some(state) = &self.state {
            let mode = if self.watch {
                format!("watching ({}s)", self.interval.as_secs())
            } else {
                "once".to_string()
            };
            format!("{}   updated {}", mode, format_time_of_day(&state.loaded_at))
        } else {
            "loading".to_string()
        };

        let width = area.width as usize;
        let combined = if left.len() + right.len() + 2 <= width {
            let pad = width - left.len() - right.len();
            format!("{}{}{}", left, " ".repeat(pad), right)
        } else {
            format!("{}  {}", left, right)
        };

        frame.render_widget(
            Paragraph::new(combined).style(Style::default().fg(Color::DarkGray)),
            area,
        );
    }
}

fn meta_line<'a>(label: &'a str, value: &'a str) -> Line<'a> {
    Line::from(vec![
        Span::styled(
            format!("  {:<10}", label),
            Style::default().fg(Color::DarkGray),
        ),
        Span::raw(value.to_string()),
    ])
}

fn status_color(s: &WorkspaceStatus) -> Color {
    match s {
        WorkspaceStatus::Pending => Color::DarkGray,
        WorkspaceStatus::InProgress => Color::Green,
        WorkspaceStatus::Done => Color::Blue,
        WorkspaceStatus::Canceled => Color::Red,
    }
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test --lib tui_app::info`
Expected: 之前的 7 个 + 新 3 个 = 10 个 PASS。

- [ ] **Step 5: `cargo build` 检查 warning/错误**

Run: `cargo build`
Expected: 成功，无未使用警告。

- [ ] **Step 6: Commit**

```bash
git add src/tui_app/info.rs
git commit -m "feat(tui_app): implement InfoApp rendering"
```

---

## Task 6: InfoApp 事件处理

**Files:**
- Modify: `src/tui_app/info.rs`

- [ ] **Step 1: 先写失败测试（追加到 `src/tui_app/info.rs` 的 `mod tests`）**

```rust
    fn make_in_progress_app(tmp: &tempfile::TempDir, name: &str) -> InfoApp {
        let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
        mgr.ensure_dirs().unwrap();
        let ws = sample_workspace(name);
        mgr.save_workspace(&WorkspaceStatus::InProgress, &ws).unwrap();
        let mgr2 = ConfigManager::with_base_dir(tmp.path().to_path_buf());
        InfoApp::new(name.into(), mgr2, true, Duration::from_secs(5))
    }

    #[test]
    fn key_q_sets_quit() {
        let tmp = tempfile::tempdir().unwrap();
        let mut app = make_in_progress_app(&tmp, "demo");
        let ev = crate::tui_app::Event::Key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('q'),
            crossterm::event::KeyModifiers::NONE,
        ));
        <InfoApp as crate::tui_app::App>::on_event(&mut app, ev).unwrap();
        assert!(<InfoApp as crate::tui_app::App>::should_quit(&app));
    }

    #[test]
    fn key_esc_sets_quit() {
        let tmp = tempfile::tempdir().unwrap();
        let mut app = make_in_progress_app(&tmp, "demo");
        let ev = crate::tui_app::Event::Key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Esc,
            crossterm::event::KeyModifiers::NONE,
        ));
        <InfoApp as crate::tui_app::App>::on_event(&mut app, ev).unwrap();
        assert!(<InfoApp as crate::tui_app::App>::should_quit(&app));
    }

    #[test]
    fn key_ctrl_c_sets_quit() {
        let tmp = tempfile::tempdir().unwrap();
        let mut app = make_in_progress_app(&tmp, "demo");
        let ev = crate::tui_app::Event::Key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('c'),
            crossterm::event::KeyModifiers::CONTROL,
        ));
        <InfoApp as crate::tui_app::App>::on_event(&mut app, ev).unwrap();
        assert!(<InfoApp as crate::tui_app::App>::should_quit(&app));
    }

    #[test]
    fn key_r_triggers_reload() {
        let tmp = tempfile::tempdir().unwrap();
        let mut app = make_in_progress_app(&tmp, "demo");
        let first_loaded = app.state.as_ref().unwrap().loaded_at;
        std::thread::sleep(std::time::Duration::from_millis(10));
        let ev = crate::tui_app::Event::Key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('r'),
            crossterm::event::KeyModifiers::NONE,
        ));
        <InfoApp as crate::tui_app::App>::on_event(&mut app, ev).unwrap();
        let second_loaded = app.state.as_ref().unwrap().loaded_at;
        assert!(second_loaded > first_loaded);
    }

    #[test]
    fn tick_triggers_reload() {
        let tmp = tempfile::tempdir().unwrap();
        let mut app = make_in_progress_app(&tmp, "demo");
        let first_loaded = app.state.as_ref().unwrap().loaded_at;
        std::thread::sleep(std::time::Duration::from_millis(10));
        <InfoApp as crate::tui_app::App>::on_event(&mut app, crate::tui_app::Event::Tick).unwrap();
        let second_loaded = app.state.as_ref().unwrap().loaded_at;
        assert!(second_loaded > first_loaded);
    }

    #[test]
    fn resize_does_not_crash() {
        let tmp = tempfile::tempdir().unwrap();
        let mut app = make_in_progress_app(&tmp, "demo");
        <InfoApp as crate::tui_app::App>::on_event(&mut app, crate::tui_app::Event::Resize(120, 40)).unwrap();
        assert!(!<InfoApp as crate::tui_app::App>::should_quit(&app));
    }

    #[test]
    fn tick_interval_reflects_watch_flag() {
        let tmp = tempfile::tempdir().unwrap();
        let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
        mgr.ensure_dirs().unwrap();
        let ws = sample_workspace("demo");
        mgr.save_workspace(&WorkspaceStatus::InProgress, &ws).unwrap();

        let mgr_watch = ConfigManager::with_base_dir(tmp.path().to_path_buf());
        let watching = InfoApp::new("demo".into(), mgr_watch, true, Duration::from_secs(7));
        assert_eq!(
            <InfoApp as crate::tui_app::App>::tick_interval(&watching),
            Some(Duration::from_secs(7))
        );

        let mgr_once = ConfigManager::with_base_dir(tmp.path().to_path_buf());
        let once = InfoApp::new("demo".into(), mgr_once, false, Duration::from_secs(5));
        assert_eq!(<InfoApp as crate::tui_app::App>::tick_interval(&once), None);
    }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --lib tui_app::info::tests::key_`
Expected: FAIL（`on_event` 目前是 no-op）。

- [ ] **Step 3: 替换 `on_event` 的 Task 5 空实现**

在 `impl crate::tui_app::App for InfoApp` 内，替换 `on_event` 方法：

```rust
    fn on_event(&mut self, event: crate::tui_app::Event) -> anyhow::Result<()> {
        use crate::tui_app::Event as E;
        use crossterm::event::{KeyCode, KeyModifiers};

        match event {
            E::Key(k) => {
                let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
                match k.code {
                    KeyCode::Char('q') | KeyCode::Esc => self.quit = true,
                    KeyCode::Char('c') if ctrl => self.quit = true,
                    KeyCode::Char('r') => self.reload(),
                    _ => {}
                }
            }
            E::Tick => self.reload(),
            E::Resize(_, _) => {}
        }
        Ok(())
    }
```

- [ ] **Step 4: 跑测试确认全部通过**

Run: `cargo test --lib tui_app::info`
Expected: 之前 10 个 + 新 7 个 = 17 个 PASS。

- [ ] **Step 5: Commit**

```bash
git add src/tui_app/info.rs
git commit -m "feat(tui_app): wire InfoApp event handling (quit/reload)"
```

---

## Task 7: `zootree info` CLI 非 watch 分支

**Files:**
- Create: `src/cli/info.rs`
- Modify: `src/cli/mod.rs`
- Modify: `src/main.rs`
- Create: `tests/info_test.rs`

这一步把 CLI 挂进去，并实现非 watch 的一次性 println 输出；watch 分支先 `anyhow::bail!`，Task 8 填充。

- [ ] **Step 1: 创建 `src/cli/info.rs`**

```rust
use std::ffi::OsStr;
use std::fmt::Write as _;

use anyhow::Result;
use clap::Args;
use clap_complete::ArgValueCompleter;

use crate::config::workspace::{WorkspaceConfig, WorkspaceStatus};
use crate::config::ConfigManager;
use crate::core::completers::{complete_workspace, WorkspaceFilter};
use crate::tui;
use crate::tui_app::info::{
    format_rfc3339_to_minute, last_n, status_label,
};

#[derive(Args, Debug)]
pub struct InfoArgs {
    #[arg(
        help = "Workspace name (interactive if omitted)",
        add = ArgValueCompleter::new(|c: &OsStr| complete_workspace(c, WorkspaceFilter::Any))
    )]
    pub name: Option<String>,

    #[arg(long, help = "Watch mode: render as a TUI and auto-refresh")]
    pub watch: bool,

    #[arg(
        long,
        default_value = "5",
        help = "Refresh interval in seconds (used with --watch)"
    )]
    pub interval: u64,
}

pub fn handle_info(args: &InfoArgs) -> Result<()> {
    let config_mgr = ConfigManager::new()?;

    let name = match &args.name {
        Some(n) => n.clone(),
        None => {
            let all = config_mgr.list_workspaces(None)?;
            if all.is_empty() {
                anyhow::bail!("no workspaces found");
            }
            let items: Vec<String> = all
                .iter()
                .map(|w| format!("{} - {}", w.name, w.title))
                .collect();
            let idx = tui::select_one("Select workspace", &items)?;
            all[idx].name.clone()
        }
    };

    let (status, workspace) = config_mgr.load_workspace(&name)?;

    if args.watch {
        // Filled in by Task 8.
        anyhow::bail!("--watch not implemented yet");
    }

    print!("{}", render_once(&status, &workspace));
    Ok(())
}

/// Build the multi-line textual report shown by `zootree info <name>`
/// without `--watch`. Pure function — easy to test.
pub fn render_once(status: &WorkspaceStatus, ws: &WorkspaceConfig) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "Workspace: {} ({})", ws.title, ws.name);
    let _ = writeln!(out, "Status:    {}", status_label(status));
    let _ = writeln!(out, "Branch:    {}", ws.branch);
    let _ = writeln!(out, "Dir:       {}", ws.workspace_dir);
    let _ = writeln!(
        out,
        "Created:   {}",
        format_rfc3339_to_minute(&ws.created_at)
    );
    if !ws.description.is_empty() {
        let _ = writeln!(out);
        let _ = writeln!(out, "Description:");
        for l in ws.description.lines() {
            let _ = writeln!(out, "  {}", l);
        }
    }
    let _ = writeln!(out);
    let _ = writeln!(out, "Repos:");
    if ws.repos.is_empty() {
        let _ = writeln!(out, "  (none)");
    } else {
        for r in &ws.repos {
            let target = r.target_branch.as_deref().unwrap_or("*");
            let _ = writeln!(out, "  - {:<15} -> {}", r.name, target);
        }
    }
    if !ws.events.is_empty() {
        let _ = writeln!(out);
        let _ = writeln!(out, "Recent events:");
        for e in last_n(&ws.events, 5) {
            let ts = format_rfc3339_to_minute(&e.timestamp);
            if let Some(d) = &e.detail {
                let _ = writeln!(out, "  {}  {}  ({})", ts, e.action, d);
            } else {
                let _ = writeln!(out, "  {}  {}", ts, e.action);
            }
        }
    }
    out
}
```

- [ ] **Step 2: 修改 `src/cli/mod.rs`**

在 `pub mod` 列表加 `pub mod info;`（按字母序位于 `completions` 之后、`prune` 之前），在 `Commands` enum 加一项。最终关键片段：

```rust
pub mod completions;
pub mod info;
pub mod prune;
pub mod repo;
pub mod template;
pub mod workspace;
```

`Commands` enum 中在 `Open` 之后插入：

```rust
    #[command(about = "Show detailed info about a workspace")]
    Info(info::InfoArgs),
```

（放在 `Open` 之后、`Done` 之前，保持"读类"命令聚集。）

- [ ] **Step 3: 修改 `src/main.rs`**

在 `fn run` 的 match 中，`Commands::Open(args) => {...}` 之后加：

```rust
        Commands::Info(args) => {
            zootree::cli::info::handle_info(&args)?;
        }
```

- [ ] **Step 4: 创建 `tests/info_test.rs`**

```rust
use zootree::cli::info::render_once;
use zootree::config::global::ZellijConfig;
use zootree::config::workspace::{Event, RepoEntry, WorkspaceConfig, WorkspaceStatus};

fn base_ws() -> WorkspaceConfig {
    WorkspaceConfig {
        title: "Demo title".into(),
        name: "demo".into(),
        description: String::new(),
        branch: "zootree/demo".into(),
        workspace_dir: "/tmp/demo".into(),
        created_at: "2026-05-10T14:22:00+08:00".into(),
        zellij: ZellijConfig::default(),
        repos: vec![],
        events: vec![],
    }
}

#[test]
fn render_once_includes_core_fields() {
    let out = render_once(&WorkspaceStatus::InProgress, &base_ws());
    assert!(out.contains("Workspace: Demo title (demo)"), "{}", out);
    assert!(out.contains("Status:    in_progress"), "{}", out);
    assert!(out.contains("Branch:    zootree/demo"), "{}", out);
    assert!(out.contains("Dir:       /tmp/demo"), "{}", out);
    assert!(out.contains("Repos:\n  (none)"), "{}", out);
}

#[test]
fn render_once_omits_description_when_empty() {
    let out = render_once(&WorkspaceStatus::Pending, &base_ws());
    assert!(!out.contains("Description:"), "{}", out);
}

#[test]
fn render_once_includes_description_when_present() {
    let mut ws = base_ws();
    ws.description = "line one\nline two".into();
    let out = render_once(&WorkspaceStatus::Pending, &ws);
    assert!(out.contains("Description:\n  line one\n  line two"), "{}", out);
}

#[test]
fn render_once_lists_repos_with_target_branch() {
    let mut ws = base_ws();
    ws.repos = vec![
        RepoEntry { name: "frontend".into(), target_branch: Some("main".into()) },
        RepoEntry { name: "backend".into(), target_branch: None },
    ];
    let out = render_once(&WorkspaceStatus::InProgress, &ws);
    assert!(out.contains("- frontend"), "{}", out);
    assert!(out.contains("-> main"), "{}", out);
    assert!(out.contains("- backend"), "{}", out);
    assert!(out.contains("-> *"), "{}", out);
}

#[test]
fn render_once_shows_last_five_events() {
    let mut ws = base_ws();
    for i in 0..7 {
        ws.events.push(Event {
            action: format!("step-{}", i),
            timestamp: "2026-05-10T14:22:00+08:00".into(),
            detail: None,
        });
    }
    let out = render_once(&WorkspaceStatus::InProgress, &ws);
    assert!(out.contains("Recent events:"), "{}", out);
    assert!(!out.contains("step-0"), "oldest trimmed: {}", out);
    assert!(!out.contains("step-1"), "oldest trimmed: {}", out);
    assert!(out.contains("step-2"), "{}", out);
    assert!(out.contains("step-6"), "{}", out);
}

#[test]
fn render_once_covers_all_statuses() {
    use WorkspaceStatus::*;
    for s in [Pending, InProgress, Done, Canceled] {
        let out = render_once(&s, &base_ws());
        let label = match s {
            Pending => "pending",
            InProgress => "in_progress",
            Done => "done",
            Canceled => "canceled",
        };
        assert!(out.contains(&format!("Status:    {}", label)), "{}: {}", label, out);
    }
}
```

- [ ] **Step 5: 跑测试确认通过**

Run: `cargo test --test info_test`
Expected: 6 个测试 PASS。

- [ ] **Step 6: 跑全套测试确保不回归**

Run: `cargo test`
Expected: 全部 PASS（包含之前所有测试 + 新加的）。

- [ ] **Step 7: 手动 smoke —— 非 watch 能打印**

Run: `cargo run -- info --help`
Expected: 显示 `zootree info` 帮助，含 `[NAME]`、`--watch`、`--interval` 三项。

- [ ] **Step 8: Commit**

```bash
git add src/cli/info.rs src/cli/mod.rs src/main.rs tests/info_test.rs
git commit -m "feat(cli): add zootree info subcommand (one-shot mode)"
```

---

## Task 8: `--watch` 分支接入 TUI

**Files:**
- Modify: `src/cli/info.rs`

- [ ] **Step 1: 替换 `handle_info` 内 `anyhow::bail!("--watch not implemented yet")` 行所在的 if 块**

找到：

```rust
    if args.watch {
        // Filled in by Task 8.
        anyhow::bail!("--watch not implemented yet");
    }

    print!("{}", render_once(&status, &workspace));
    Ok(())
```

替换为：

```rust
    if args.watch {
        // `workspace` / `status` above were just a reachability check; the TUI
        // reloads on its own via the consumed config_mgr.
        let _ = (status, workspace);
        let app = crate::tui_app::info::InfoApp::new(
            name,
            config_mgr,
            true,
            std::time::Duration::from_secs(args.interval),
        );
        crate::tui_app::run_app(app)?;
        return Ok(());
    }

    print!("{}", render_once(&status, &workspace));
    Ok(())
```

- [ ] **Step 2: 确认全套测试仍过**

Run: `cargo test`
Expected: 全 PASS。

- [ ] **Step 3: 手动 smoke —— 验证 `--watch` 在本地可运行（需要一个已存在的 workspace）**

前置：本机已有 `~/.config/zootree/workspaces/in_progress/` 下任一 workspace（例如 `demo`）。

Run: `cargo run -- info demo --watch --interval 2`

Expected:
- 终端进入 alternate screen，显示 title bar "zootree info — demo [...]" + meta + repos + events + status line
- 每 2 秒 status line 右侧 `updated HH:MM:SS` 时间更新
- 按 `q` 或 `Esc` 或 `Ctrl+C` 退出，终端恢复正常

如果没有可用 workspace：跳过本步骤，Task 9 完成后下一次 `zootree start` 会自然走这条路径。

- [ ] **Step 4: Commit**

```bash
git add src/cli/info.rs
git commit -m "feat(cli): wire zootree info --watch to TUI"
```

---

## Task 9: 默认 layout overview pane 改为 `zootree info`

**Files:**
- Modify: `src/core/layout.rs`
- Modify: `tests/layout_test.rs`

- [ ] **Step 1: 先在 `tests/layout_test.rs` 追加失败断言**

在文件末尾追加：

```rust
#[test]
fn default_layout_overview_uses_info_watch() {
    let template = LayoutRenderer::default_layout();
    assert!(
        template.contains(r#""info" "$workspace_name" "--watch""#),
        "default layout should use `zootree info <name> --watch` in overview\n---\n{}",
        template
    );
    assert!(
        !template.contains(r#""list" "--status" "in_progress""#),
        "default layout should no longer spawn list in overview\n---\n{}",
        template
    );
}

#[test]
fn default_layout_info_args_expanded_on_render() {
    let template = LayoutRenderer::default_layout();
    let vars = vec![LayoutVar {
        repo_name: "frontend".into(),
        worktree_path: "/ws/calm-river/frontend".into(),
        branch: "zootree/calm-river".into(),
        workspace_name: "calm-river".into(),
        workspace_dir: "/ws/calm-river".into(),
        lazygit_config: "".into(),
    }];
    let rendered = LayoutRenderer::render(template, &vars);
    assert!(
        rendered.contains(r#""info" "calm-river" "--watch""#),
        "expected $workspace_name to expand\n---\n{}",
        rendered
    );
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --test layout_test default_layout_overview_uses_info_watch`
Expected: FAIL（原 layout 仍是 `"list" "--status" "in_progress"`）。

- [ ] **Step 3: 修改 `src/core/layout.rs` 的 `default_layout` 函数**

找到 `default_layout` 返回的 KDL 字符串中的这段：

```kdl
        pane split_direction="vertical" {
            pane command="zootree" {
                args "list" "--status" "in_progress"
            }
            pane cwd="$workspace_dir"
        }
```

替换为：

```kdl
        pane split_direction="vertical" {
            pane command="zootree" {
                args "info" "$workspace_name" "--watch"
            }
            pane cwd="$workspace_dir"
        }
```

（只改 `args` 行，其余保持原样。）

- [ ] **Step 4: 跑测试确认 layout_test 全过**

Run: `cargo test --test layout_test`
Expected: 原 4 个测试 + 新 2 个 = 6 个 PASS。

- [ ] **Step 5: 跑全套测试**

Run: `cargo test`
Expected: 全 PASS。

- [ ] **Step 6: Commit**

```bash
git add src/core/layout.rs tests/layout_test.rs
git commit -m "feat(layout): overview pane now shows current workspace via zootree info"
```

---

## Task 10: 同步 zootree-dev skill 文档

**Files:**
- Modify: `.claude/skills/zootree-dev/SKILL.md`

本步只更新 skill 文档以反映新增模块/命令/依赖，无测试。

- [ ] **Step 1: 更新"项目架构"树**

找到架构树段，加入两行并在 `cli/` 子树加 `info.rs`。关键改动：

- `cli/` 子树新增：`├── info.rs      # info [name] [--watch]`
- `src/` 顶层新增（放在 `core/` 之后）：
  ```
  ├── tui_app/         # TUI 应用框架（ratatui + crossterm）
  │   ├── mod.rs       # Event / App trait / run_app 事件循环
  │   └── info.rs      # InfoApp + 格式化辅助函数
  ```

- [ ] **Step 2: 更新"关键依赖"表**

追加两行：

| Crate | 用途 |
|-------|------|
| `ratatui` (0.29) | TUI 框架，`src/tui_app/` 的渲染内核 |
| `crossterm` (0.28) | 终端后端：raw mode、事件读取、alternate screen |

- [ ] **Step 3: 更新"命令路由"示例**

在 match 示例里补一行：

```rust
Commands::Info(args) => zootree::cli::info::handle_info(&args)?,
```

- [ ] **Step 4: 在"常见开发任务"章节新增小节 `添加新的 TUI 视图`**

```markdown
### 添加新的 TUI 视图

1. 在 `src/tui_app/` 下新建模块 `<name>.rs` 并在 `src/tui_app/mod.rs` 加 `pub mod <name>;`
2. 实现 `App` trait：`on_event` / `render` / `should_quit`，需要定时刷新则覆写 `tick_interval`
3. 入口调用 `tui_app::run_app(app)`；渲染测试用 `ratatui::backend::TestBackend` + `Terminal::draw`
4. 事件处理测试直接调 `<App>::on_event` 并断言状态变化，不必进真实终端
```

- [ ] **Step 5: 人工通读一遍 SKILL.md，确保无遗漏并无前后矛盾**

- [ ] **Step 6: Commit**

```bash
git add .claude/skills/zootree-dev/SKILL.md
git commit -m "docs(skill): update zootree-dev for info + tui_app"
```

---

## Self-Review（作者已执行）

**Spec 覆盖：**

| Spec 条目 | 覆盖 Task |
|-----------|-----------|
| 新增 `Cargo.toml` 依赖 ratatui + crossterm | Task 1 |
| `src/lib.rs` 加 `pub mod tui_app;` | Task 1 |
| `src/tui_app/mod.rs`：Event + App trait + run_app | Task 2 + Task 3 |
| `src/tui_app/info.rs`：InfoApp + InfoState + reload + 格式化 | Task 4 + Task 5 + Task 6 |
| `src/cli/info.rs`：InfoArgs + handle_info + render_once | Task 7 + Task 8 |
| `src/cli/mod.rs` + `src/main.rs` 接入 | Task 7 |
| `src/core/layout.rs` 改 overview pane args | Task 9 |
| `tests/layout_test.rs` 新 args 断言 | Task 9 |
| 纯函数单测（格式化、last_n） | Task 4 |
| TUI 行为测试（render buffer / on_event） | Task 5 + Task 6 |
| 集成测试（render_once 全覆盖） | Task 7 |
| 支持全部四种 workspace 状态 + `WorkspaceFilter::Any` | Task 7（`render_once_covers_all_statuses`、`complete_workspace(.., Any)`）|
| `q` / `Esc` / `Ctrl+C` 退出；`r` 刷新 | Task 6 |
| panic hook 恢复终端 | Task 3 |
| 默认 layout 每次覆盖，老用户无感升级 | 已写入 spec；Task 9 改了源即自动生效，无需额外迁移步骤 |
| 更新 zootree-dev skill | Task 10 |

**Spec 偏差（已说明）：**

- `run_app` 实现用**单线程 event::poll 方案**而非 spec 里的后台线程 + channel——功能等价、更简单、零新增同步原语。Task 3 的设计备注里解释了。
- spec 中 "watch 状态下遇 `done`/`cancel` 文件搬移" 的 fallback 逻辑**无需额外代码**：`ConfigManager::load_workspace` 已自动扫全部状态目录，reload 天然跟踪迁移，title bar 颜色/标签也自动更新。

**Placeholder 扫描：** 全部 task 含具体代码与断言，未见 TODO/TBD 类占位。

**类型一致性检查：** `InfoApp`、`InfoState`、`Event`、`App`、`run_app`、`main_loop`、`format_rfc3339_to_minute`、`last_n`、`status_label`、`status_color`、`meta_line` 跨任务引用一致。`render_once(&WorkspaceStatus, &WorkspaceConfig) -> String` 在 Task 7 定义，Task 7 的测试消费——一致。`InfoApp::new(name, config_mgr, watch, interval)` 在 Task 4 定义，Task 5/6/8 调用——一致。

---

## Execution Handoff

**Plan complete and saved to `docs/superpowers/plans/2026-05-11-workspace-info.md`. Two execution options:**

**1. Subagent-Driven (recommended)** - I dispatch a fresh subagent per task, review between tasks, fast iteration

**2. Inline Execution** - Execute tasks in this session using executing-plans, batch execution with checkpoints

**Which approach?**
