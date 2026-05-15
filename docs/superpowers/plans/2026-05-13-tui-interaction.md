# TUI 交互优化 实施计划（替换 dialoguer）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 删除 `dialoguer`，用基于 `ratatui` + `crossterm` + `tui-textarea` 的 inline prompt 替换 `src/tui.rs` 的 `Input` / `Select` / `MultiSelect` / `Confirm`，修复 CJK 删除 bug、加入多行编辑（Alt+Enter / Shift+Enter 换行，Enter 提交），保持 9 处 callsite 零改动。

**Architecture:** 在 `src/tui_app/` 下新增 `prompt.rs`，定义四个 `*PromptState`（纯逻辑，不碰终端，可单测），加上一个 `InlineApp` trait + `run_inline` 运行时（`Viewport::Inline` + bracketed paste + scrollback 总结）。`src/tui.rs` 保留全部公开签名，内部改为构造 state 并调 `run_inline`，把 `PromptOutcome` 映射为 `Result`。`PromptError::Cancelled` 在 `main.rs` 入口被识别，退出码 1、stderr `aborted`。

**Tech Stack:** Rust 2021、`ratatui = "0.29"`、`crossterm = "0.28"`、新增 `tui-textarea = "0.7"` / `unicode-width = "0.2"` / `fuzzy-matcher = "0.3"`，移除 `dialoguer = "0.11"`。

---

## File Structure

| 文件 | 改动类型 | 责任 |
|------|---------|------|
| `Cargo.toml` | 修改 | 移除 `dialoguer`；新增 `tui-textarea`、`unicode-width`、`fuzzy-matcher` |
| `src/tui_app/mod.rs` | 修改 | 新增 `InlineApp` trait、`run_inline`、`PromptOutcome`、`CancelledByUser` 错误；扩展 panic hook（pop enhancement flags + disable bracketed paste） |
| `src/tui_app/prompt.rs` | 新建 | 四个 prompt state（`TextPromptState` / `SelectPromptState` / `MultiSelectPromptState` / `ConfirmPromptState`）、各自的 `InlineApp` 实现、模块内 `#[cfg(test)] mod tests` |
| `src/tui.rs` | 修改 | 5 个公开函数内部从 `dialoguer::*` 改为构造 state + `run_inline`，签名不变 |
| `src/main.rs` | 修改 | `run` 错误处理新增 `CancelledByUser` 分支：stderr `aborted`、exit 1、不打 backtrace |
| `tests/tui_app_test.rs` | 修改 | 不动现有；如需为 `InlineApp` 加 NoopInlineApp 测试可在此追加 |

`prompt.rs` 单文件起步；如果实现完成后超过 ~500 行，再拆 `prompt/{text,select,multi,confirm}.rs`，本计划不预先拆分。

---

## Task 1: 调整依赖

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: 修改 `[dependencies]`，移除 dialoguer，新增三个 crate**

把现有 `dialoguer = "0.11"` 那一行删掉。在 `crossterm = "0.28"` 之后追加三行：

```toml
tui-textarea = "0.7"
unicode-width = "0.2"
fuzzy-matcher = "0.3"
```

- [ ] **Step 2: 验证 Cargo.lock 可以解析**

Run: `cargo check 2>&1 | head -80`
Expected: 出现 `error[E0432]` 之类提示 —— `tui.rs` 里 `use dialoguer::...` 失效。这是预期的，下一步开始就会逐步修复。**不要在本任务里回滚改动；接下来的任务会用 Cargo.toml 反映出的新依赖。**

- [ ] **Step 3: 暂存当前修改，但本任务不 commit**

`Cargo.toml` 改动会和后面的代码改动一起在 Task 2 commit。原因：单独 commit 一份编译失败的 Cargo.toml 没意义，会污染 git history、且会让 bisect 落到红色 commit 上。

```bash
git status
```
Expected: 只有 `Cargo.toml` 被修改。

---

## Task 2: 模块脚手架（`InlineApp` trait + `run_inline` 占位 + 错误类型）

**Files:**
- Modify: `src/tui_app/mod.rs`
- Create: `src/tui_app/prompt.rs`

本任务只搭骨架并恢复编译。`run_inline` 暂时只支持空操作（NoopInlineApp 能跑），不接管终端 IO 之外的任何事情。`prompt.rs` 留空模块声明，便于后续任务往里填。

- [ ] **Step 1: 在 `src/tui_app/mod.rs` 顶部新增 `pub mod prompt;`**

```rust
//! TUI application framework built on ratatui + crossterm.
//!
//! Each interactive view implements the `App` trait; `run_app` drives the
//! terminal setup, event loop, and cleanup.

use std::time::Duration;

pub mod info;
pub mod prompt;
```

- [ ] **Step 2: 在同一文件追加 `Event::Paste` 变体（兼容 bracketed paste）**

定位现有 `pub enum Event { Key(...), Tick, Resize(...) }`，改成：

```rust
/// Events dispatched to the active `App` (and `InlineApp`) on every iteration
/// of the main loop.
pub enum Event {
    Key(crossterm::event::KeyEvent),
    Tick,
    Resize(u16, u16),
    Paste(String),
}
```

注意：现有 `App` trait 的 `on_event` 也会收到 `Paste`，但当前 `InfoApp` 不感兴趣，行为是 ignore。这是设计上想要的 —— 共享 `Event` 类型避免重复定义。

- [ ] **Step 3: 在文件末尾追加 `PromptOutcome` / `CancelledByUser` / `InlineApp` 定义**

```rust
/// Outcome of an inline prompt, returned by `run_inline`.
pub enum PromptOutcome<T> {
    Submitted(T),
    Skipped,
    Aborted,
    Interrupted,
}

/// Sentinel error inserted into the `anyhow` chain when an inline prompt is
/// cancelled by the user (Esc on a required prompt, or Ctrl+C anywhere).
/// `main` downcasts to this and exits cleanly with code 1.
#[derive(Debug)]
pub struct CancelledByUser;

impl std::fmt::Display for CancelledByUser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "aborted")
    }
}

impl std::error::Error for CancelledByUser {}

/// Inline (non-fullscreen) TUI app. Shares the `Event` type with `App`, but
/// uses `Viewport::Inline` and exposes a typed `Output` to `run_inline`.
pub trait InlineApp {
    type Output;
    fn on_event(&mut self, event: Event) -> anyhow::Result<()>;
    fn render(&mut self, frame: &mut ratatui::Frame);
    fn desired_height(&self) -> u16;
    fn poll(&mut self) -> Option<PromptOutcome<Self::Output>>;
    /// One-line summary written to scrollback after the prompt exits with
    /// `Submitted`. `None` => write nothing.
    fn summary(&self) -> Option<String> {
        None
    }
}
```

- [ ] **Step 4: 写一个最小的 `run_inline` —— 占位实现**

把以下函数追加到 `src/tui_app/mod.rs` 末尾。**这个版本不开 bracketed paste、不 push enhancement flags、不写 scrollback** —— 这些会在 Task 7 接管。本任务只做：进 raw mode、用 inline viewport 跑事件循环、退出还原。

```rust
use crossterm::event::{
    DisableBracketedPaste as _DisableBracketedPasteUnusedYet, EnableBracketedPaste as _EnableBracketedPasteUnusedYet,
};
// (注：上面两行是为了让本任务保留导入但不报 unused。Task 7 会真正使用。
//  如果你的 rustc 仍然报 unused，加 #[allow(unused_imports)]。)

pub fn run_inline<A: InlineApp>(mut app: A) -> anyhow::Result<PromptOutcome<A::Output>> {
    install_panic_hook();
    enable_raw_mode()?;
    let mut stdout = io::stdout();

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = ratatui::Terminal::with_options(
        backend,
        ratatui::TerminalOptions {
            viewport: ratatui::Viewport::Inline(app.desired_height()),
        },
    )?;

    let result = inline_loop(&mut terminal, &mut app);

    let _ = disable_raw_mode();

    result
}

fn inline_loop<B: ratatui::backend::Backend, A: InlineApp>(
    terminal: &mut ratatui::Terminal<B>,
    app: &mut A,
) -> anyhow::Result<PromptOutcome<A::Output>> {
    loop {
        let height = app.desired_height();
        terminal.resize(ratatui::layout::Rect::new(0, 0, terminal.size()?.width, height))?;
        terminal.draw(|f| app.render(f))?;

        if let Some(outcome) = app.poll() {
            // Clear the inline viewport so the prompt's UI doesn't linger.
            terminal.clear()?;
            return Ok(outcome);
        }

        if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                CtEvent::Key(k) => app.on_event(Event::Key(k))?,
                CtEvent::Resize(w, h) => app.on_event(Event::Resize(w, h))?,
                CtEvent::Paste(s) => app.on_event(Event::Paste(s))?,
                _ => {}
            }
        }
    }
}
```

如果 rustc 抱怨 unused import，给两行加 `#[allow(unused_imports)]`：

```rust
#[allow(unused_imports)]
use crossterm::event::{DisableBracketedPaste as _DisableBracketedPasteUnusedYet, EnableBracketedPaste as _EnableBracketedPasteUnusedYet};
```

- [ ] **Step 5: 创建 `src/tui_app/prompt.rs` 占位**

```rust
//! Inline prompts replacing the previous `dialoguer`-based `src/tui.rs`
//! implementation. Each prompt has a pure-logic state struct that handles
//! events and exposes outcome / current text / selection; rendering and
//! terminal IO are delegated to `run_inline`.

// State structs and their `InlineApp` impls are added in subsequent tasks.
```

- [ ] **Step 6: 让 `src/tui.rs` 编译通过 —— 最小化改动**

dialoguer 已被移除，但本任务还没实现真 prompt。临时让 5 个公开函数返回 `unimplemented!("prompt impl pending")`，以便项目可以 `cargo check` 通过。其它 callsite 不改。

替换 `src/tui.rs` 的全部内容为：

```rust
//! Public prompt API. Implementations live in `crate::tui_app::prompt`.
//!
//! Until those are wired up (see plan: Tasks 7-10), the bodies are
//! placeholders that panic if called. No callsite has changed yet.

use anyhow::Result;

pub fn input_required(_prompt: &str) -> Result<String> {
    unimplemented!("input_required: see tui-interaction plan task 7");
}

pub fn input_optional(_prompt: &str) -> Result<Option<String>> {
    unimplemented!("input_optional: see tui-interaction plan task 7");
}

pub fn select_one(_prompt: &str, _items: &[String]) -> Result<usize> {
    unimplemented!("select_one: see tui-interaction plan task 8");
}

pub fn select_multi(_prompt: &str, _items: &[String]) -> Result<Vec<usize>> {
    unimplemented!("select_multi: see tui-interaction plan task 9");
}

pub fn confirm(_prompt: &str, _default: bool) -> Result<bool> {
    unimplemented!("confirm: see tui-interaction plan task 10");
}
```

- [ ] **Step 7: `cargo check`**

Run: `cargo check`
Expected: 通过（warnings 允许）。`cargo build --tests` 也应通过 —— 现有测试没有调 `tui::*`。

- [ ] **Step 8: Commit**

```bash
git add Cargo.toml Cargo.lock src/tui.rs src/tui_app/mod.rs src/tui_app/prompt.rs
git commit -m "$(cat <<'EOF'
feat(tui_app): scaffold InlineApp trait + prompt module skeleton

- 移除 dialoguer，新增 tui-textarea / unicode-width / fuzzy-matcher
- 新增 PromptOutcome、CancelledByUser、InlineApp trait
- run_inline 占位实现（无 bracketed paste / scrollback，由 Task 7 接管）
- src/tui.rs 5 个函数暂时 unimplemented!，待后续任务填充
EOF
)"
```

---

## Task 3: `TextPromptState` 纯逻辑 + 测试

**Files:**
- Modify: `src/tui_app/prompt.rs`

实现并单测一个不接终端的 `TextPromptState`：接 `KeyEvent` / 粘贴字符串，输出 `outcome` 和当前文本。`tui-textarea` 直接用作内部存储以拿到 CJK / unicode-width 的正确行为。

- [ ] **Step 1: 写失败的测试 —— 在 `src/tui_app/prompt.rs` 末尾追加 `#[cfg(test)] mod tests`**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui_app::PromptOutcome;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }
    fn key_mod(code: KeyCode, m: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, m)
    }

    #[test]
    fn text_cjk_backspace_removes_one_char() {
        let mut s = TextPromptState::new("Title").required();
        s.handle_key(key(KeyCode::Char('你')));
        s.handle_key(key(KeyCode::Char('好')));
        assert_eq!(s.text(), "你好");
        s.handle_key(key(KeyCode::Backspace));
        assert_eq!(s.text(), "你");
    }

    #[test]
    fn text_alt_enter_inserts_newline() {
        let mut s = TextPromptState::new("Desc").optional();
        s.handle_key(key(KeyCode::Char('a')));
        s.handle_key(key_mod(KeyCode::Enter, KeyModifiers::ALT));
        s.handle_key(key(KeyCode::Char('b')));
        assert_eq!(s.text(), "a\nb");
        assert!(s.outcome().is_none());
    }

    #[test]
    fn text_shift_enter_inserts_newline() {
        let mut s = TextPromptState::new("Desc").optional();
        s.handle_key(key(KeyCode::Char('a')));
        s.handle_key(key_mod(KeyCode::Enter, KeyModifiers::SHIFT));
        s.handle_key(key(KeyCode::Char('b')));
        assert_eq!(s.text(), "a\nb");
    }

    #[test]
    fn text_enter_submits() {
        let mut s = TextPromptState::new("Title").required();
        s.handle_key(key(KeyCode::Char('x')));
        s.handle_key(key(KeyCode::Enter));
        assert!(matches!(s.outcome(), Some(PromptOutcome::Submitted(t)) if t == "x"));
    }

    #[test]
    fn text_optional_esc_skipped() {
        let mut s = TextPromptState::new("Desc").optional();
        s.handle_key(key(KeyCode::Esc));
        assert!(matches!(s.outcome(), Some(PromptOutcome::Skipped)));
    }

    #[test]
    fn text_required_esc_aborted() {
        let mut s = TextPromptState::new("Title").required();
        s.handle_key(key(KeyCode::Esc));
        assert!(matches!(s.outcome(), Some(PromptOutcome::Aborted)));
    }

    #[test]
    fn text_ctrl_c_interrupted() {
        let mut s = TextPromptState::new("Title").required();
        s.handle_key(key_mod(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(matches!(s.outcome(), Some(PromptOutcome::Interrupted)));
    }

    #[test]
    fn text_paste_inserts_multiline_without_submitting() {
        let mut s = TextPromptState::new("Desc").optional();
        s.handle_paste("line1\nline2");
        assert_eq!(s.text(), "line1\nline2");
        assert!(s.outcome().is_none());
    }

    #[test]
    fn text_after_outcome_keys_are_ignored() {
        let mut s = TextPromptState::new("Title").required();
        s.handle_key(key(KeyCode::Esc));
        s.handle_key(key(KeyCode::Char('a')));
        assert_eq!(s.text(), "");
        assert!(matches!(s.outcome(), Some(PromptOutcome::Aborted)));
    }
}
```

- [ ] **Step 2: 运行测试，确认全失败**

Run: `cargo test --test tui_app_test 2>/dev/null; cargo test text_ -- --nocapture 2>&1 | head -40`
Expected: 编译错 —— `TextPromptState` 未定义。这是预期的失败信号。

- [ ] **Step 3: 实现 `TextPromptState` —— 在 `prompt.rs` 顶部追加**

把 `prompt.rs` 改为：

```rust
//! Inline prompts replacing the previous `dialoguer`-based `src/tui.rs`
//! implementation.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tui_textarea::{CursorMove, TextArea};

use crate::tui_app::PromptOutcome;

/// Multi-line text prompt. Backed by `tui_textarea::TextArea` for correct
/// CJK / unicode-width behavior.
pub struct TextPromptState {
    prompt: String,
    required: bool,
    textarea: TextArea<'static>,
    outcome: Option<PromptOutcome<String>>,
}

impl TextPromptState {
    pub fn new(prompt: &str) -> Self {
        Self {
            prompt: prompt.to_string(),
            required: true,
            textarea: TextArea::default(),
            outcome: None,
        }
    }

    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    pub fn optional(mut self) -> Self {
        self.required = false;
        self
    }

    pub fn text(&self) -> String {
        self.textarea.lines().join("\n")
    }

    pub fn outcome(&self) -> Option<&PromptOutcome<String>> {
        self.outcome.as_ref()
    }

    pub fn prompt(&self) -> &str {
        &self.prompt
    }

    pub fn is_required(&self) -> bool {
        self.required
    }

    pub fn line_count(&self) -> usize {
        self.textarea.lines().len()
    }

    /// Width-1 textarea preview for rendering. Callers should use
    /// `textarea_widget()`; this leaks the inner widget by ref.
    pub fn textarea(&self) -> &TextArea<'static> {
        &self.textarea
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if self.outcome.is_some() {
            return;
        }
        let m = key.modifiers;
        let ctrl = m.contains(KeyModifiers::CONTROL);
        let alt = m.contains(KeyModifiers::ALT);
        let shift = m.contains(KeyModifiers::SHIFT);

        match key.code {
            KeyCode::Char('c') if ctrl => {
                self.outcome = Some(PromptOutcome::Interrupted);
            }
            KeyCode::Esc => {
                self.outcome = Some(if self.required {
                    PromptOutcome::Aborted
                } else {
                    PromptOutcome::Skipped
                });
            }
            KeyCode::Enter if alt || shift => {
                self.textarea.insert_newline();
            }
            KeyCode::Enter => {
                let text = self.text();
                self.outcome = Some(PromptOutcome::Submitted(text));
            }
            KeyCode::Backspace => {
                self.textarea.delete_char();
            }
            KeyCode::Char(c) => {
                self.textarea.insert_char(c);
            }
            KeyCode::Left => self.textarea.move_cursor(CursorMove::Back),
            KeyCode::Right => self.textarea.move_cursor(CursorMove::Forward),
            KeyCode::Up => self.textarea.move_cursor(CursorMove::Up),
            KeyCode::Down => self.textarea.move_cursor(CursorMove::Down),
            KeyCode::Home => self.textarea.move_cursor(CursorMove::Head),
            KeyCode::End => self.textarea.move_cursor(CursorMove::End),
            _ => {}
        }
    }

    pub fn handle_paste(&mut self, s: &str) {
        if self.outcome.is_some() {
            return;
        }
        for c in s.chars() {
            if c == '\n' {
                self.textarea.insert_newline();
            } else {
                self.textarea.insert_char(c);
            }
        }
    }
}
```

- [ ] **Step 4: 运行测试，确认全通过**

Run: `cargo test text_ -- --nocapture`
Expected: 9 个测试全 PASS。

- [ ] **Step 5: Commit**

```bash
git add src/tui_app/prompt.rs
git commit -m "feat(tui_app): TextPromptState 纯逻辑 + 单测"
```

---

## Task 4: `SelectPromptState` 纯逻辑 + 测试

**Files:**
- Modify: `src/tui_app/prompt.rs`

加入 `SelectPromptState`：维护一个 `Vec<String>` items、过滤词（用一个迷你单行 `TextArea`）、当前光标 index、过滤后命中索引列表。Enter 提交时返回原数组的索引。

- [ ] **Step 1: 写失败的测试 —— 追加到现有 `mod tests`**

```rust
    #[test]
    fn select_filter_cjk_filters_items() {
        let items = vec!["前端".to_string(), "后端".to_string(), "中间件".to_string()];
        let mut s = SelectPromptState::new("Choose", items);
        s.handle_key(key(KeyCode::Char('前')));
        let visible = s.visible_indices();
        assert_eq!(visible, vec![0]);
    }

    #[test]
    fn select_arrow_navigation_wraps() {
        let items = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let mut s = SelectPromptState::new("Pick", items);
        assert_eq!(s.cursor_visible_index(), Some(0));
        s.handle_key(key(KeyCode::Down));
        assert_eq!(s.cursor_visible_index(), Some(1));
        s.handle_key(key(KeyCode::Down));
        s.handle_key(key(KeyCode::Down));
        assert_eq!(s.cursor_visible_index(), Some(0)); // wrap
        s.handle_key(key(KeyCode::Up));
        assert_eq!(s.cursor_visible_index(), Some(2)); // wrap up
    }

    #[test]
    fn select_enter_submits_original_index() {
        let items = vec!["alpha".into(), "beta".into(), "gamma".into()];
        let mut s = SelectPromptState::new("Pick", items);
        s.handle_key(key(KeyCode::Down));
        s.handle_key(key(KeyCode::Enter));
        assert!(matches!(s.outcome(), Some(PromptOutcome::Submitted(1))));
    }

    #[test]
    fn select_enter_after_filter_returns_original_index() {
        let items = vec!["alpha".into(), "beta".into(), "gamma".into()];
        let mut s = SelectPromptState::new("Pick", items);
        s.handle_key(key(KeyCode::Char('g')));
        s.handle_key(key(KeyCode::Enter));
        assert!(matches!(s.outcome(), Some(PromptOutcome::Submitted(2))));
    }

    #[test]
    fn select_enter_with_empty_filter_match_is_noop() {
        let items = vec!["alpha".into(), "beta".into()];
        let mut s = SelectPromptState::new("Pick", items);
        s.handle_key(key(KeyCode::Char('z')));
        s.handle_key(key(KeyCode::Char('z')));
        s.handle_key(key(KeyCode::Char('z')));
        assert!(s.visible_indices().is_empty());
        s.handle_key(key(KeyCode::Enter));
        assert!(s.outcome().is_none());
    }

    #[test]
    fn select_esc_aborted() {
        let items = vec!["a".into()];
        let mut s = SelectPromptState::new("Pick", items);
        s.handle_key(key(KeyCode::Esc));
        assert!(matches!(s.outcome(), Some(PromptOutcome::Aborted)));
    }

    #[test]
    fn select_ctrl_c_interrupted() {
        let items = vec!["a".into()];
        let mut s = SelectPromptState::new("Pick", items);
        s.handle_key(key_mod(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(matches!(s.outcome(), Some(PromptOutcome::Interrupted)));
    }
```

- [ ] **Step 2: 运行测试，确认全失败**

Run: `cargo test select_ -- --nocapture`
Expected: 编译错 —— `SelectPromptState` 未定义。

- [ ] **Step 3: 实现 `SelectPromptState`**

把以下追加到 `prompt.rs` 中（紧跟 `TextPromptState` 之后）：

```rust
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;

pub struct SelectPromptState {
    prompt: String,
    items: Vec<String>,
    filter: TextArea<'static>,
    visible: Vec<usize>,   // indices into `items`, after filtering
    cursor: usize,         // index into `visible`
    matcher: SkimMatcherV2,
    outcome: Option<PromptOutcome<usize>>,
}

impl SelectPromptState {
    pub fn new(prompt: &str, items: Vec<String>) -> Self {
        let visible = (0..items.len()).collect();
        let mut filter = TextArea::default();
        // single-line mode: insert_newline 不会被外部触发，但还是显式禁用 Enter
        filter.set_cursor_line_style(ratatui::style::Style::default());
        Self {
            prompt: prompt.to_string(),
            items,
            filter,
            visible,
            cursor: 0,
            matcher: SkimMatcherV2::default(),
            outcome: None,
        }
    }

    pub fn prompt(&self) -> &str {
        &self.prompt
    }
    pub fn items(&self) -> &[String] {
        &self.items
    }
    pub fn visible_indices(&self) -> Vec<usize> {
        self.visible.clone()
    }
    pub fn cursor_visible_index(&self) -> Option<usize> {
        if self.visible.is_empty() {
            None
        } else {
            Some(self.cursor.min(self.visible.len() - 1))
        }
    }
    pub fn filter_text(&self) -> String {
        self.filter.lines().join("")
    }
    pub fn outcome(&self) -> Option<&PromptOutcome<usize>> {
        self.outcome.as_ref()
    }
    pub fn match_indices_for(&self, item_idx: usize) -> Vec<usize> {
        let f = self.filter_text();
        if f.is_empty() {
            return Vec::new();
        }
        self.matcher
            .fuzzy_indices(&self.items[item_idx], &f)
            .map(|(_score, idxs)| idxs)
            .unwrap_or_default()
    }

    fn recompute_visible(&mut self) {
        let f = self.filter_text();
        if f.is_empty() {
            self.visible = (0..self.items.len()).collect();
        } else {
            let mut scored: Vec<(i64, usize)> = self
                .items
                .iter()
                .enumerate()
                .filter_map(|(i, item)| self.matcher.fuzzy_match(item, &f).map(|s| (s, i)))
                .collect();
            scored.sort_by(|a, b| b.0.cmp(&a.0));
            self.visible = scored.into_iter().map(|(_, i)| i).collect();
        }
        if self.cursor >= self.visible.len() {
            self.cursor = self.visible.len().saturating_sub(1);
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if self.outcome.is_some() {
            return;
        }
        let m = key.modifiers;
        let ctrl = m.contains(KeyModifiers::CONTROL);

        match key.code {
            KeyCode::Char('c') if ctrl => {
                self.outcome = Some(PromptOutcome::Interrupted);
            }
            KeyCode::Esc => {
                self.outcome = Some(PromptOutcome::Aborted);
            }
            KeyCode::Enter => {
                if let Some(vi) = self.cursor_visible_index() {
                    let original = self.visible[vi];
                    self.outcome = Some(PromptOutcome::Submitted(original));
                }
            }
            KeyCode::Down | KeyCode::Char('n') if matches!(key.code, KeyCode::Down) || ctrl => {
                if !self.visible.is_empty() {
                    self.cursor = (self.cursor + 1) % self.visible.len();
                }
            }
            KeyCode::Up | KeyCode::Char('p') if matches!(key.code, KeyCode::Up) || ctrl => {
                if !self.visible.is_empty() {
                    self.cursor = if self.cursor == 0 {
                        self.visible.len() - 1
                    } else {
                        self.cursor - 1
                    };
                }
            }
            KeyCode::Backspace => {
                self.filter.delete_char();
                self.recompute_visible();
            }
            KeyCode::Char(c) => {
                self.filter.insert_char(c);
                self.recompute_visible();
            }
            _ => {}
        }
    }
}
```

注意：`KeyCode::Down | KeyCode::Char('n')` 这种带条件的 match arm 在 Rust 里写法是分两条 arm 或者用 guard。改成下面的更安全形式：

```rust
            KeyCode::Down => {
                if !self.visible.is_empty() {
                    self.cursor = (self.cursor + 1) % self.visible.len();
                }
            }
            KeyCode::Char('n') if ctrl => {
                if !self.visible.is_empty() {
                    self.cursor = (self.cursor + 1) % self.visible.len();
                }
            }
            KeyCode::Up => {
                if !self.visible.is_empty() {
                    self.cursor = if self.cursor == 0 {
                        self.visible.len() - 1
                    } else {
                        self.cursor - 1
                    };
                }
            }
            KeyCode::Char('p') if ctrl => {
                if !self.visible.is_empty() {
                    self.cursor = if self.cursor == 0 {
                        self.visible.len() - 1
                    } else {
                        self.cursor - 1
                    };
                }
            }
```

把上面这块替换掉前一段里 `KeyCode::Down | KeyCode::Char('n') ...` 那两个 arm。

- [ ] **Step 4: 运行测试，确认全通过**

Run: `cargo test select_ -- --nocapture`
Expected: 7 个测试全 PASS。

- [ ] **Step 5: Commit**

```bash
git add src/tui_app/prompt.rs
git commit -m "feat(tui_app): SelectPromptState 纯逻辑 + 单测"
```

---

## Task 5: `MultiSelectPromptState` 纯逻辑 + 测试

**Files:**
- Modify: `src/tui_app/prompt.rs`

与 SelectPromptState 同布局，多一个勾选状态向量；Enter 返回**勾选顺序**的索引数组（与 dialoguer 现状一致）。

- [ ] **Step 1: 写失败的测试**

```rust
    #[test]
    fn multi_space_toggles_current_item() {
        let mut s = MultiSelectPromptState::new("Pick", vec!["a".into(), "b".into()]);
        s.handle_key(key(KeyCode::Char(' ')));
        assert!(s.is_checked(0));
        s.handle_key(key(KeyCode::Char(' ')));
        assert!(!s.is_checked(0));
    }

    #[test]
    fn multi_a_toggles_select_all() {
        let mut s = MultiSelectPromptState::new("Pick", vec!["a".into(), "b".into(), "c".into()]);
        s.handle_key(key(KeyCode::Char('a')));
        assert!(s.is_checked(0) && s.is_checked(1) && s.is_checked(2));
        s.handle_key(key(KeyCode::Char('a')));
        assert!(!s.is_checked(0) && !s.is_checked(1) && !s.is_checked(2));
    }

    #[test]
    fn multi_returns_indices_in_selection_order() {
        let mut s = MultiSelectPromptState::new("Pick", vec!["a".into(), "b".into(), "c".into()]);
        s.handle_key(key(KeyCode::Down));
        s.handle_key(key(KeyCode::Down));
        s.handle_key(key(KeyCode::Char(' '))); // selects "c" first
        s.handle_key(key(KeyCode::Up));
        s.handle_key(key(KeyCode::Up));
        s.handle_key(key(KeyCode::Char(' '))); // selects "a" second
        s.handle_key(key(KeyCode::Enter));
        assert!(matches!(s.outcome(), Some(PromptOutcome::Submitted(v)) if v == &vec![2, 0]));
    }

    #[test]
    fn multi_enter_with_no_selection_submits_empty() {
        let mut s = MultiSelectPromptState::new("Pick", vec!["a".into(), "b".into()]);
        s.handle_key(key(KeyCode::Enter));
        assert!(matches!(s.outcome(), Some(PromptOutcome::Submitted(v)) if v.is_empty()));
    }

    #[test]
    fn multi_filter_then_toggle_then_clear_filter_keeps_selection() {
        let mut s = MultiSelectPromptState::new(
            "Pick",
            vec!["alpha".into(), "beta".into(), "gamma".into()],
        );
        // type 'g' -> only gamma visible
        s.handle_key(key(KeyCode::Char('g')));
        s.handle_key(key(KeyCode::Char(' ')));
        assert!(s.is_checked(2));
        // backspace -> all visible again
        s.handle_key(key(KeyCode::Backspace));
        assert_eq!(s.visible_indices(), vec![0, 1, 2]);
        assert!(s.is_checked(2));
    }

    #[test]
    fn multi_esc_aborted() {
        let mut s = MultiSelectPromptState::new("Pick", vec!["a".into()]);
        s.handle_key(key(KeyCode::Esc));
        assert!(matches!(s.outcome(), Some(PromptOutcome::Aborted)));
    }
```

- [ ] **Step 2: 运行测试，确认全失败**

Run: `cargo test multi_ -- --nocapture`
Expected: 编译错 —— `MultiSelectPromptState` 未定义。

- [ ] **Step 3: 实现 `MultiSelectPromptState`**

追加到 `prompt.rs`：

```rust
pub struct MultiSelectPromptState {
    inner: SelectPromptState,
    selection_order: Vec<usize>, // original indices in checking order
    checked: Vec<bool>,          // length == items.len()
    outcome: Option<PromptOutcome<Vec<usize>>>,
}

impl MultiSelectPromptState {
    pub fn new(prompt: &str, items: Vec<String>) -> Self {
        let n = items.len();
        Self {
            inner: SelectPromptState::new(prompt, items),
            selection_order: Vec::new(),
            checked: vec![false; n],
            outcome: None,
        }
    }

    pub fn prompt(&self) -> &str {
        self.inner.prompt()
    }
    pub fn items(&self) -> &[String] {
        self.inner.items()
    }
    pub fn visible_indices(&self) -> Vec<usize> {
        self.inner.visible_indices()
    }
    pub fn cursor_visible_index(&self) -> Option<usize> {
        self.inner.cursor_visible_index()
    }
    pub fn filter_text(&self) -> String {
        self.inner.filter_text()
    }
    pub fn is_checked(&self, original_idx: usize) -> bool {
        self.checked.get(original_idx).copied().unwrap_or(false)
    }
    pub fn match_indices_for(&self, item_idx: usize) -> Vec<usize> {
        self.inner.match_indices_for(item_idx)
    }
    pub fn outcome(&self) -> Option<&PromptOutcome<Vec<usize>>> {
        self.outcome.as_ref()
    }

    fn toggle(&mut self, original_idx: usize) {
        if self.checked[original_idx] {
            self.checked[original_idx] = false;
            self.selection_order.retain(|&i| i != original_idx);
        } else {
            self.checked[original_idx] = true;
            self.selection_order.push(original_idx);
        }
    }

    fn select_all_toggle(&mut self) {
        let all_set = self.checked.iter().all(|&c| c);
        if all_set {
            self.checked.fill(false);
            self.selection_order.clear();
        } else {
            for (i, c) in self.checked.iter_mut().enumerate() {
                if !*c {
                    *c = true;
                    self.selection_order.push(i);
                }
            }
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if self.outcome.is_some() {
            return;
        }
        let m = key.modifiers;
        let ctrl = m.contains(KeyModifiers::CONTROL);

        match key.code {
            KeyCode::Char('c') if ctrl => {
                self.outcome = Some(PromptOutcome::Interrupted);
            }
            KeyCode::Esc => {
                self.outcome = Some(PromptOutcome::Aborted);
            }
            KeyCode::Char(' ') => {
                if let Some(vi) = self.inner.cursor_visible_index() {
                    let original = self.inner.visible_indices()[vi];
                    self.toggle(original);
                }
            }
            KeyCode::Char('a') if !ctrl && self.inner.filter_text().is_empty() => {
                // 'a' is only the select-all toggle when the filter is empty.
                // Otherwise it inserts into the filter (handled by inner).
                self.select_all_toggle();
            }
            KeyCode::Enter => {
                self.outcome = Some(PromptOutcome::Submitted(self.selection_order.clone()));
            }
            _ => {
                // Delegate movement / filter / typing to the inner SelectPromptState.
                // We bypass its Enter/Esc/Ctrl+C handling because we already covered
                // those above; but the inner will re-receive them as no-ops since
                // it sets its own outcome — which we won't propagate. To prevent
                // double-handling, only forward keys we want it to act on.
                self.inner.handle_key(key);
                // Inner may have set its own outcome for keys we didn't intercept
                // (it shouldn't, given the match above, but defensive):
                self.inner.clear_outcome_if_set();
            }
        }
    }
}
```

`SelectPromptState` 需要一个辅助方法 `clear_outcome_if_set` —— 因为我们把它当作内嵌的过滤+导航组件复用，不希望它自行结束。把这个 helper 加到 `SelectPromptState` 末尾：

```rust
impl SelectPromptState {
    /// Used by `MultiSelectPromptState` when it embeds a select state purely
    /// for filter + navigation; we never want the inner state's outcome to
    /// leak to the caller.
    pub(crate) fn clear_outcome_if_set(&mut self) {
        self.outcome = None;
    }
}
```

- [ ] **Step 4: 运行测试，确认全通过**

Run: `cargo test multi_ -- --nocapture`
Expected: 6 个测试全 PASS。

- [ ] **Step 5: Commit**

```bash
git add src/tui_app/prompt.rs
git commit -m "feat(tui_app): MultiSelectPromptState 纯逻辑 + 单测"
```

---

## Task 6: `ConfirmPromptState` 纯逻辑 + 测试

**Files:**
- Modify: `src/tui_app/prompt.rs`

- [ ] **Step 1: 写失败的测试**

```rust
    #[test]
    fn confirm_default_true_on_enter_returns_true() {
        let mut s = ConfirmPromptState::new("Delete?", true);
        s.handle_key(key(KeyCode::Enter));
        assert!(matches!(s.outcome(), Some(PromptOutcome::Submitted(true))));
    }

    #[test]
    fn confirm_default_false_on_enter_returns_false() {
        let mut s = ConfirmPromptState::new("Delete?", false);
        s.handle_key(key(KeyCode::Enter));
        assert!(matches!(s.outcome(), Some(PromptOutcome::Submitted(false))));
    }

    #[test]
    fn confirm_y_returns_true() {
        let mut s = ConfirmPromptState::new("Delete?", false);
        s.handle_key(key(KeyCode::Char('y')));
        assert!(matches!(s.outcome(), Some(PromptOutcome::Submitted(true))));
    }

    #[test]
    fn confirm_capital_n_returns_false() {
        let mut s = ConfirmPromptState::new("Delete?", true);
        s.handle_key(key(KeyCode::Char('N')));
        assert!(matches!(s.outcome(), Some(PromptOutcome::Submitted(false))));
    }

    #[test]
    fn confirm_esc_aborted() {
        let mut s = ConfirmPromptState::new("Delete?", false);
        s.handle_key(key(KeyCode::Esc));
        assert!(matches!(s.outcome(), Some(PromptOutcome::Aborted)));
    }

    #[test]
    fn confirm_ctrl_c_interrupted() {
        let mut s = ConfirmPromptState::new("Delete?", true);
        s.handle_key(key_mod(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(matches!(s.outcome(), Some(PromptOutcome::Interrupted)));
    }
```

- [ ] **Step 2: 运行测试，确认全失败**

Run: `cargo test confirm_ -- --nocapture`
Expected: 编译错。

- [ ] **Step 3: 实现 `ConfirmPromptState`**

追加到 `prompt.rs`：

```rust
pub struct ConfirmPromptState {
    prompt: String,
    default: bool,
    outcome: Option<PromptOutcome<bool>>,
}

impl ConfirmPromptState {
    pub fn new(prompt: &str, default: bool) -> Self {
        Self {
            prompt: prompt.to_string(),
            default,
            outcome: None,
        }
    }

    pub fn prompt(&self) -> &str {
        &self.prompt
    }
    pub fn default_value(&self) -> bool {
        self.default
    }
    pub fn outcome(&self) -> Option<&PromptOutcome<bool>> {
        self.outcome.as_ref()
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if self.outcome.is_some() {
            return;
        }
        let m = key.modifiers;
        let ctrl = m.contains(KeyModifiers::CONTROL);

        match key.code {
            KeyCode::Char('c') if ctrl => {
                self.outcome = Some(PromptOutcome::Interrupted);
            }
            KeyCode::Esc => {
                self.outcome = Some(PromptOutcome::Aborted);
            }
            KeyCode::Enter => {
                self.outcome = Some(PromptOutcome::Submitted(self.default));
            }
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                self.outcome = Some(PromptOutcome::Submitted(true));
            }
            KeyCode::Char('n') | KeyCode::Char('N') => {
                self.outcome = Some(PromptOutcome::Submitted(false));
            }
            _ => {}
        }
    }
}
```

- [ ] **Step 4: 运行测试，确认全通过**

Run: `cargo test confirm_ -- --nocapture`
Expected: 6 个测试全 PASS。

- [ ] **Step 5: Commit**

```bash
git add src/tui_app/prompt.rs
git commit -m "feat(tui_app): ConfirmPromptState 纯逻辑 + 单测"
```

---

## Task 7: 完整版 `run_inline`（bracketed paste、enhancement flags、scrollback 总结、panic hook 扩展）

**Files:**
- Modify: `src/tui_app/mod.rs`

把 Task 2 的占位 `run_inline` 升级为 spec §2 描述的完整版本。

- [ ] **Step 1: 修改 `install_panic_hook` 让它同时还原所有终端状态**

在 `src/tui_app/mod.rs` 找到 `install_panic_hook`，替换为：

```rust
fn install_panic_hook() {
    use std::sync::Once;
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let original = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            // Restore terminal in the broadest order: pop kitty flags (if pushed),
            // disable bracketed paste (if enabled), leave alternate screen (if entered),
            // disable raw mode. Each is best-effort; ignore errors.
            let _ = crossterm::execute!(
                io::stdout(),
                crossterm::event::PopKeyboardEnhancementFlags,
                crossterm::event::DisableBracketedPaste,
                LeaveAlternateScreen,
            );
            let _ = disable_raw_mode();
            original(info);
        }));
    });
}
```

注意：`PopKeyboardEnhancementFlags` 在没有 push 过的终端上是 no-op，安全。

- [ ] **Step 2: 替换 `run_inline` 为完整版**

把 Task 2 写的 `run_inline` 整体替换为：

```rust
pub fn run_inline<A: InlineApp>(mut app: A) -> anyhow::Result<PromptOutcome<A::Output>> {
    install_panic_hook();

    enable_raw_mode()?;
    let mut stdout = io::stdout();

    // Best-effort: enable kitty keyboard protocol so Shift+Enter is reported
    // distinctly from Enter. Terminals that don't support it will silently
    // ignore; we don't track whether the push succeeded because the matching
    // Pop is itself best-effort.
    let _ = crossterm::execute!(
        stdout,
        crossterm::event::PushKeyboardEnhancementFlags(
            crossterm::event::KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                | crossterm::event::KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS,
        ),
    );

    // Bracketed paste so multi-line paste arrives as a single Event::Paste(s)
    // instead of being interpreted character-by-character (which would let an
    // embedded '\n' submit the prompt prematurely).
    let _ = crossterm::execute!(stdout, crossterm::event::EnableBracketedPaste);

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = ratatui::Terminal::with_options(
        backend,
        ratatui::TerminalOptions {
            viewport: ratatui::Viewport::Inline(app.desired_height()),
        },
    )?;

    let result = inline_loop(&mut terminal, &mut app);

    // Cleanup. Each step best-effort.
    let _ = crossterm::execute!(
        terminal.backend_mut(),
        crossterm::event::DisableBracketedPaste,
        crossterm::event::PopKeyboardEnhancementFlags,
    );
    let _ = disable_raw_mode();

    // If the prompt submitted, write a one-line summary into scrollback so the
    // command's history shows what was answered. Skip on Aborted/Interrupted/Skipped.
    if let Ok(PromptOutcome::Submitted(_)) = &result {
        if let Some(line) = app.summary() {
            // Use insert_before to push the summary above the inline viewport,
            // then clear the viewport so nothing of the prompt's UI remains.
            let _ = terminal.insert_before(1, |buf| {
                let area = buf.area;
                let span = ratatui::text::Span::raw(line.clone());
                buf.set_string(area.x, area.y, &line, ratatui::style::Style::default());
                let _ = span; // keep span construction explicit; not actually used
            });
        }
    }
    let _ = terminal.clear();

    result
}

fn inline_loop<B: ratatui::backend::Backend, A: InlineApp>(
    terminal: &mut ratatui::Terminal<B>,
    app: &mut A,
) -> anyhow::Result<PromptOutcome<A::Output>> {
    let mut last_height = app.desired_height();
    loop {
        let height = app.desired_height();
        if height != last_height {
            terminal.resize(ratatui::layout::Rect::new(
                0,
                0,
                terminal.size()?.width,
                height,
            ))?;
            last_height = height;
        }
        terminal.draw(|f| app.render(f))?;

        if let Some(outcome) = app.poll() {
            return Ok(outcome);
        }

        if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                CtEvent::Key(k) => app.on_event(Event::Key(k))?,
                CtEvent::Resize(w, h) => app.on_event(Event::Resize(w, h))?,
                CtEvent::Paste(s) => app.on_event(Event::Paste(s))?,
                _ => {}
            }
        }
    }
}
```

如果 `Step 1` 中的 `Cargo.toml` 没拉到 `crossterm` 的 `bracketed-paste` feature，需要确认：crossterm 0.28 默认开启 `bracketed-paste`，无需额外 feature 标志。`cargo doc -p crossterm --open` 可验证。

- [ ] **Step 3: `cargo check`**

Run: `cargo check`
Expected: 通过。可能有 warnings 关于 `_DisableBracketedPasteUnusedYet` 之类残留导入 —— 删掉它们：

把 Task 2 加的：
```rust
use crossterm::event::{
    DisableBracketedPaste as _DisableBracketedPasteUnusedYet,
    EnableBracketedPaste as _EnableBracketedPasteUnusedYet,
};
```
整段删除（实际 API 调用都用全限定名 `crossterm::event::*`）。

- [ ] **Step 4: 运行所有现有单测**

Run: `cargo test`
Expected: 全部 PASS（现有测试 + 之前几个 Task 的纯逻辑测试）。

- [ ] **Step 5: Commit**

```bash
git add src/tui_app/mod.rs
git commit -m "feat(tui_app): run_inline 完整版（bracketed paste、kitty flags、scrollback 总结）"
```

---

## Task 8: `TextPromptState` 接 `InlineApp` + 渲染 + `tui::input_*` 接线

**Files:**
- Modify: `src/tui_app/prompt.rs`
- Modify: `src/tui.rs`

- [ ] **Step 1: 在 `prompt.rs` 末尾追加 `TextPromptState` 的 `InlineApp` 实现**

```rust
use crate::tui_app::{Event, InlineApp};
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

const TEXT_MAX_VISIBLE_LINES: u16 = 10;

impl InlineApp for TextPromptState {
    type Output = String;

    fn on_event(&mut self, event: Event) -> anyhow::Result<()> {
        match event {
            Event::Key(k) => self.handle_key(k),
            Event::Paste(s) => self.handle_paste(&s),
            Event::Resize(_, _) | Event::Tick => {}
        }
        Ok(())
    }

    fn render(&mut self, frame: &mut ratatui::Frame) {
        let area = frame.area();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // prompt header
                Constraint::Min(1),    // editor (variable height)
                Constraint::Length(1), // help
            ])
            .split(area);

        let header_spans = if self.is_required() {
            vec![Span::styled(format!("> {}", self.prompt()), Style::default().fg(Color::Cyan))]
        } else {
            vec![
                Span::styled(format!("> {}", self.prompt()), Style::default().fg(Color::Cyan)),
                Span::raw(" "),
                Span::styled("(optional)", Style::default().fg(Color::DarkGray)),
            ]
        };
        frame.render_widget(Paragraph::new(Line::from(header_spans)), chunks[0]);

        // Render the textarea with a thin border. tui-textarea handles cursor itself.
        let mut ta = self.textarea.clone();
        ta.set_block(Block::default().borders(Borders::ALL));
        frame.render_widget(&ta, chunks[1]);

        let help = if self.is_required() {
            "enter submit · alt+enter newline · esc cancel · ctrl+c abort"
        } else {
            "enter submit · alt+enter newline · esc skip · ctrl+c abort"
        };
        frame.render_widget(
            Paragraph::new(Span::styled(help, Style::default().fg(Color::DarkGray))),
            chunks[2],
        );
    }

    fn desired_height(&self) -> u16 {
        let editor = (self.line_count() as u16).clamp(1, TEXT_MAX_VISIBLE_LINES);
        // 1 header + editor + 2 borders + 1 help
        1 + editor + 2 + 1
    }

    fn poll(&mut self) -> Option<PromptOutcome<Self::Output>> {
        self.outcome.take()
    }

    fn summary(&self) -> Option<String> {
        match &self.outcome {
            Some(PromptOutcome::Submitted(text)) => {
                let first = text.lines().next().unwrap_or("");
                let suffix = if text.lines().count() > 1 { " …" } else { "" };
                Some(format!("✔ {}: {}{}", self.prompt(), first, suffix))
            }
            _ => None,
        }
    }
}
```

注意：`poll` 用 `take` 是因为 trait 签名是 `&mut self`；但我们在 `summary` 里又要读 `outcome`。冲突。修法：把 `poll` 改成只 `peek`，让 `run_inline` 在拿到结果后**不要再调用** `poll`，直接从 `summary` 拿。

更稳妥的做法是：把 `summary` 接受 `outcome` 参数。但当前 `run_inline` 里 `summary` 是 `&self`，且我们已经返回了 outcome。两难。

**采纳的折中方案：** `poll` 只复制不消费；`outcome` 字段本身就保留到 drop。代价是 `outcome` 字段改成 `Option<PromptOutcome<T>>` 中 `T: Clone`（`String` / `usize` / `bool` / `Vec<usize>` 全 Clone，符合）。

把上面的 `poll` 改为：

```rust
    fn poll(&mut self) -> Option<PromptOutcome<Self::Output>> {
        match &self.outcome {
            Some(PromptOutcome::Submitted(t)) => Some(PromptOutcome::Submitted(t.clone())),
            Some(PromptOutcome::Skipped) => Some(PromptOutcome::Skipped),
            Some(PromptOutcome::Aborted) => Some(PromptOutcome::Aborted),
            Some(PromptOutcome::Interrupted) => Some(PromptOutcome::Interrupted),
            None => None,
        }
    }
```

`PromptOutcome` 需要派生 `Clone`：把 `src/tui_app/mod.rs` 里的 `pub enum PromptOutcome<T>` 上加 `#[derive(Clone)]`，且约束 `T: Clone`：

```rust
#[derive(Clone)]
pub enum PromptOutcome<T> {
    Submitted(T),
    Skipped,
    Aborted,
    Interrupted,
}
```

- [ ] **Step 2: 把 `tui::input_required` / `input_optional` 接到新实现**

替换 `src/tui.rs`：

```rust
//! Public prompt API. Implementations live in `crate::tui_app::prompt` and
//! are run via `crate::tui_app::run_inline`.

use anyhow::Result;

use crate::tui_app::prompt::{
    ConfirmPromptState, MultiSelectPromptState, SelectPromptState, TextPromptState,
};
use crate::tui_app::{run_inline, CancelledByUser, PromptOutcome};

pub fn input_required(prompt: &str) -> Result<String> {
    let state = TextPromptState::new(prompt).required();
    match run_inline(state)? {
        PromptOutcome::Submitted(s) => Ok(s),
        PromptOutcome::Aborted | PromptOutcome::Interrupted => Err(CancelledByUser.into()),
        PromptOutcome::Skipped => unreachable!("required text prompt cannot be skipped"),
    }
}

pub fn input_optional(prompt: &str) -> Result<Option<String>> {
    let state = TextPromptState::new(prompt).optional();
    match run_inline(state)? {
        PromptOutcome::Submitted(s) if s.is_empty() => Ok(None),
        PromptOutcome::Submitted(s) => Ok(Some(s)),
        PromptOutcome::Skipped => Ok(None),
        PromptOutcome::Aborted | PromptOutcome::Interrupted => Err(CancelledByUser.into()),
    }
}

pub fn select_one(prompt: &str, items: &[String]) -> Result<usize> {
    let state = SelectPromptState::new(prompt, items.to_vec());
    match run_inline(state)? {
        PromptOutcome::Submitted(idx) => Ok(idx),
        PromptOutcome::Aborted | PromptOutcome::Interrupted => Err(CancelledByUser.into()),
        PromptOutcome::Skipped => unreachable!("select_one is required"),
    }
}

pub fn select_multi(prompt: &str, items: &[String]) -> Result<Vec<usize>> {
    let state = MultiSelectPromptState::new(prompt, items.to_vec());
    match run_inline(state)? {
        PromptOutcome::Submitted(v) => Ok(v),
        PromptOutcome::Aborted | PromptOutcome::Interrupted => Err(CancelledByUser.into()),
        PromptOutcome::Skipped => unreachable!("select_multi is required"),
    }
}

pub fn confirm(prompt: &str, default: bool) -> Result<bool> {
    let state = ConfirmPromptState::new(prompt, default);
    match run_inline(state)? {
        PromptOutcome::Submitted(v) => Ok(v),
        PromptOutcome::Aborted | PromptOutcome::Interrupted => Err(CancelledByUser.into()),
        PromptOutcome::Skipped => unreachable!("confirm is required"),
    }
}
```

注意：本任务结束时 `select_one` / `select_multi` / `confirm` 的 `InlineApp` 实现还没写，编译会失败。Task 9-11 会逐个补齐。**为了让本任务能 commit 并保持「编译通过」**，本任务暂时只暴露 `input_required` / `input_optional` 的真实现，其余三个继续 unimplemented:

```rust
pub fn select_one(_prompt: &str, _items: &[String]) -> Result<usize> {
    unimplemented!("select_one: see plan task 9");
}
pub fn select_multi(_prompt: &str, _items: &[String]) -> Result<Vec<usize>> {
    unimplemented!("select_multi: see plan task 10");
}
pub fn confirm(_prompt: &str, _default: bool) -> Result<bool> {
    unimplemented!("confirm: see plan task 11");
}
```

并删掉 `tui.rs` 顶部的 `SelectPromptState` / `MultiSelectPromptState` / `ConfirmPromptState` 导入。**实际 commit 的 `tui.rs` 用这个混合版本**（`input_*` 真实，`select_*` / `confirm` 还是 placeholder）。

- [ ] **Step 3: `cargo check && cargo test`**

Run: `cargo check && cargo test`
Expected: 通过。新加的 InlineApp 实现编译过；原有测试 + 纯逻辑测试全 PASS。

- [ ] **Step 4: 人工 smoke 测试 input**

Run（需要真终端，CI 中跳过）：
```bash
cargo run -- create
```
Expected：进 `Title` prompt，输入 `项目 标题`，按 Backspace 中文按字符删；按 Enter 提交。然后进 `Description (optional)`，按 Alt+Enter 换行，再 Enter 提交。如果中途按 Ctrl+C，命令应该立刻退出（暂时会因为 select_multi 还没实现报 unimplemented，正常 —— 我们只验证 input 部分）。

- [ ] **Step 5: Commit**

```bash
git add src/tui_app/mod.rs src/tui_app/prompt.rs src/tui.rs
git commit -m "feat(tui): TextPromptState 渲染 + input_required/input_optional 接线"
```

---

## Task 9: `SelectPromptState` 渲染 + `tui::select_one` 接线

**Files:**
- Modify: `src/tui_app/prompt.rs`
- Modify: `src/tui.rs`

- [ ] **Step 1: 在 `prompt.rs` 追加 `SelectPromptState` 的 `InlineApp` 实现**

```rust
const SELECT_MAX_VISIBLE_ROWS: u16 = 8;

impl InlineApp for SelectPromptState {
    type Output = usize;

    fn on_event(&mut self, event: Event) -> anyhow::Result<()> {
        if let Event::Key(k) = event {
            self.handle_key(k);
        }
        Ok(())
    }

    fn render(&mut self, frame: &mut ratatui::Frame) {
        let area = frame.area();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // prompt + filter
                Constraint::Min(1),    // list
                Constraint::Length(1), // help
            ])
            .split(area);

        let header_spans = vec![
            Span::styled(format!("> {}: ", self.prompt()), Style::default().fg(Color::Cyan)),
            Span::raw(self.filter_text()),
            Span::styled("█", Style::default().fg(Color::DarkGray)), // fake cursor
        ];
        frame.render_widget(Paragraph::new(Line::from(header_spans)), chunks[0]);

        let visible = self.visible_indices();
        let cursor = self.cursor_visible_index();
        let lines: Vec<Line> = visible
            .iter()
            .enumerate()
            .take(SELECT_MAX_VISIBLE_ROWS as usize)
            .map(|(vi, &orig)| {
                let item = &self.items()[orig];
                let hits = self.match_indices_for(orig);
                let mut spans: Vec<Span> = Vec::new();
                let style = if Some(vi) == cursor {
                    Style::default().add_modifier(Modifier::REVERSED)
                } else {
                    Style::default()
                };
                let prefix = if Some(vi) == cursor { "> " } else { "  " };
                spans.push(Span::styled(prefix, style));
                for (i, ch) in item.chars().enumerate() {
                    let mut s = style;
                    if hits.contains(&i) {
                        s = s.fg(Color::Yellow);
                    }
                    spans.push(Span::styled(ch.to_string(), s));
                }
                Line::from(spans)
            })
            .collect();
        frame.render_widget(Paragraph::new(lines), chunks[1]);

        frame.render_widget(
            Paragraph::new(Span::styled(
                "↑↓ navigate · enter select · esc cancel · ctrl+c abort",
                Style::default().fg(Color::DarkGray),
            )),
            chunks[2],
        );
    }

    fn desired_height(&self) -> u16 {
        let rows = (self.visible_indices().len() as u16).clamp(1, SELECT_MAX_VISIBLE_ROWS);
        1 + rows + 1
    }

    fn poll(&mut self) -> Option<PromptOutcome<Self::Output>> {
        self.outcome.clone()
    }

    fn summary(&self) -> Option<String> {
        match &self.outcome {
            Some(PromptOutcome::Submitted(idx)) => {
                Some(format!("✔ {}: {}", self.prompt(), self.items()[*idx]))
            }
            _ => None,
        }
    }
}
```

注意：`fuzzy-matcher` 返回的是字节索引，但我们要按字符索引高亮。简化处理：先按字节匹配，再用 `item.char_indices()` 把字节位置映射回字符位置。或者：把 `match_indices_for` 改为直接返回字符索引集合。本计划采用后者 —— 修改 `match_indices_for`：

```rust
    pub fn match_indices_for(&self, item_idx: usize) -> Vec<usize> {
        let f = self.filter_text();
        if f.is_empty() {
            return Vec::new();
        }
        let item = &self.items[item_idx];
        let byte_hits = self
            .matcher
            .fuzzy_indices(item, &f)
            .map(|(_score, idxs)| idxs)
            .unwrap_or_default();
        // Convert byte positions to character positions.
        let byte_to_char: std::collections::HashMap<usize, usize> = item
            .char_indices()
            .enumerate()
            .map(|(ci, (bi, _))| (bi, ci))
            .collect();
        byte_hits
            .into_iter()
            .filter_map(|b| byte_to_char.get(&b).copied())
            .collect()
    }
```

把 `prompt.rs` 中已有的 `match_indices_for` 替换成上面这版。**注：测试 `select_filter_cjk_filters_items` 已覆盖了「能过滤」这件事，但没测高亮位置；高亮是渲染纯视觉，不再加单测，YAGNI。**

- [ ] **Step 2: 接 `tui::select_one`**

把 Task 8 中 `tui.rs` 的 `select_one` placeholder 替换为真实现（即 Task 8 Step 2 已经写过的版本）：

```rust
pub fn select_one(prompt: &str, items: &[String]) -> Result<usize> {
    let state = SelectPromptState::new(prompt, items.to_vec());
    match run_inline(state)? {
        PromptOutcome::Submitted(idx) => Ok(idx),
        PromptOutcome::Aborted | PromptOutcome::Interrupted => Err(CancelledByUser.into()),
        PromptOutcome::Skipped => unreachable!("select_one is required"),
    }
}
```

把 `use` 那一行加上 `SelectPromptState`。

- [ ] **Step 3: `cargo check && cargo test`**

Run: `cargo check && cargo test`
Expected: 通过。

- [ ] **Step 4: 人工 smoke 测试 select**

```bash
cargo run -- start
```
Expected：列出 pending workspaces，输入子串过滤，↑↓ 移动，Enter 选中。Esc 应该让命令以 `aborted` 退出（**目前 main.rs 还没接 `CancelledByUser`，会显示 anyhow 默认错误信息**；这是预期的，Task 12 修复）。

- [ ] **Step 5: Commit**

```bash
git add src/tui_app/prompt.rs src/tui.rs
git commit -m "feat(tui): SelectPromptState 渲染 + select_one 接线"
```

---

## Task 10: `MultiSelectPromptState` 渲染 + `tui::select_multi` 接线

**Files:**
- Modify: `src/tui_app/prompt.rs`
- Modify: `src/tui.rs`

- [ ] **Step 1: 在 `prompt.rs` 追加 `MultiSelectPromptState` 的 `InlineApp` 实现**

```rust
impl InlineApp for MultiSelectPromptState {
    type Output = Vec<usize>;

    fn on_event(&mut self, event: Event) -> anyhow::Result<()> {
        if let Event::Key(k) = event {
            self.handle_key(k);
        }
        Ok(())
    }

    fn render(&mut self, frame: &mut ratatui::Frame) {
        let area = frame.area();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(1),
                Constraint::Length(1),
            ])
            .split(area);

        let header_spans = vec![
            Span::styled(format!("> {}: ", self.prompt()), Style::default().fg(Color::Cyan)),
            Span::raw(self.filter_text()),
            Span::styled("█", Style::default().fg(Color::DarkGray)),
        ];
        frame.render_widget(Paragraph::new(Line::from(header_spans)), chunks[0]);

        let visible = self.visible_indices();
        let cursor = self.cursor_visible_index();
        let lines: Vec<Line> = visible
            .iter()
            .enumerate()
            .take(SELECT_MAX_VISIBLE_ROWS as usize)
            .map(|(vi, &orig)| {
                let item = &self.items()[orig];
                let hits = self.match_indices_for(orig);
                let style = if Some(vi) == cursor {
                    Style::default().add_modifier(Modifier::REVERSED)
                } else {
                    Style::default()
                };
                let cursor_prefix = if Some(vi) == cursor { ">" } else { " " };
                let check = if self.is_checked(orig) { "[x]" } else { "[ ]" };
                let mut spans = vec![
                    Span::styled(format!("{} {} ", cursor_prefix, check), style),
                ];
                for (i, ch) in item.chars().enumerate() {
                    let mut s = style;
                    if hits.contains(&i) {
                        s = s.fg(Color::Yellow);
                    }
                    spans.push(Span::styled(ch.to_string(), s));
                }
                Line::from(spans)
            })
            .collect();
        frame.render_widget(Paragraph::new(lines), chunks[1]);

        frame.render_widget(
            Paragraph::new(Span::styled(
                "↑↓ navigate · space toggle · a all · enter submit · esc cancel · ctrl+c abort",
                Style::default().fg(Color::DarkGray),
            )),
            chunks[2],
        );
    }

    fn desired_height(&self) -> u16 {
        let rows = (self.visible_indices().len() as u16).clamp(1, SELECT_MAX_VISIBLE_ROWS);
        1 + rows + 1
    }

    fn poll(&mut self) -> Option<PromptOutcome<Self::Output>> {
        self.outcome.clone()
    }

    fn summary(&self) -> Option<String> {
        match &self.outcome {
            Some(PromptOutcome::Submitted(v)) => {
                Some(format!("✔ {}: {} selected", self.prompt(), v.len()))
            }
            _ => None,
        }
    }
}
```

- [ ] **Step 2: 接 `tui::select_multi`**

替换 `tui.rs` 中的 `select_multi` placeholder 为真实现，加 `MultiSelectPromptState` 到 `use`。

- [ ] **Step 3: `cargo check && cargo test`**

Run: `cargo check && cargo test`
Expected: 通过。

- [ ] **Step 4: 人工 smoke 测试**

```bash
cargo run -- create
```
（提供 title / description 后到 `Select repos`）：用 Space 切换勾选，按 `a` 全选，Enter 提交。

- [ ] **Step 5: Commit**

```bash
git add src/tui_app/prompt.rs src/tui.rs
git commit -m "feat(tui): MultiSelectPromptState 渲染 + select_multi 接线"
```

---

## Task 11: `ConfirmPromptState` 渲染 + `tui::confirm` 接线

**Files:**
- Modify: `src/tui_app/prompt.rs`
- Modify: `src/tui.rs`

- [ ] **Step 1: 在 `prompt.rs` 追加 `ConfirmPromptState` 的 `InlineApp` 实现**

```rust
impl InlineApp for ConfirmPromptState {
    type Output = bool;

    fn on_event(&mut self, event: Event) -> anyhow::Result<()> {
        if let Event::Key(k) = event {
            self.handle_key(k);
        }
        Ok(())
    }

    fn render(&mut self, frame: &mut ratatui::Frame) {
        let area = frame.area();
        let hint = if self.default_value() { "[Y/n]" } else { "[y/N]" };
        let line = Line::from(vec![
            Span::styled(format!("> {} ", self.prompt()), Style::default().fg(Color::Cyan)),
            Span::styled(hint, Style::default().fg(Color::DarkGray)),
        ]);
        frame.render_widget(Paragraph::new(line), area);
    }

    fn desired_height(&self) -> u16 {
        1
    }

    fn poll(&mut self) -> Option<PromptOutcome<Self::Output>> {
        self.outcome.clone()
    }

    fn summary(&self) -> Option<String> {
        match &self.outcome {
            Some(PromptOutcome::Submitted(v)) => {
                Some(format!("✔ {}: {}", self.prompt(), if *v { "yes" } else { "no" }))
            }
            _ => None,
        }
    }
}
```

- [ ] **Step 2: 接 `tui::confirm`**

替换 `tui.rs` 中的 `confirm` placeholder 为真实现，加 `ConfirmPromptState` 到 `use`。

- [ ] **Step 3: `cargo check && cargo test`**

Run: `cargo check && cargo test`
Expected: 通过。

- [ ] **Step 4: 人工 smoke 测试**

```bash
cargo run -- cancel <some-workspace-with-uncommitted-changes>
```
Expected：出现 `Discard worktree changes? [y/N]`，按 `y` / `n` / Enter 各自正常工作。

- [ ] **Step 5: Commit**

```bash
git add src/tui_app/prompt.rs src/tui.rs
git commit -m "feat(tui): ConfirmPromptState 渲染 + confirm 接线"
```

---

## Task 12: `main.rs` 处理 `CancelledByUser` —— 输出 `aborted` + 退出码 1

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: 修改 `main` 函数错误处理分支**

定位现有：
```rust
if let Err(e) = run(cli.command) {
    tracing::error!("{:#}", e);
    std::process::exit(1);
}
```

替换为：
```rust
if let Err(e) = run(cli.command) {
    if e.downcast_ref::<zootree::tui_app::CancelledByUser>().is_some() {
        eprintln!("aborted");
        std::process::exit(1);
    }
    tracing::error!("{:#}", e);
    std::process::exit(1);
}
```

- [ ] **Step 2: `cargo check`**

Run: `cargo check`
Expected: 通过。

- [ ] **Step 3: 人工 smoke 测试**

```bash
cargo run -- start
```
进 `Select workspace` 后按 Esc。
Expected：stderr 输出 `aborted`，退出码 1（用 `echo $?` 验证）。Ctrl+C 行为相同。

- [ ] **Step 4: Commit**

```bash
git add src/main.rs
git commit -m "feat(main): 识别 CancelledByUser，输出 aborted 并以 1 退出"
```

---

## Task 13: 全量人工 smoke + 最终回归 + 清理

**Files:**
- 无代码修改（除非 smoke 发现 bug）

- [ ] **Step 1: 跑全部测试**

Run: `cargo test`
Expected: 全 PASS。

- [ ] **Step 2: 跑 fmt + check（项目 pre-commit hook 等价）**

Run: `cargo fmt --all -- --check && cargo check --all-targets`
Expected: 通过。失败则 `cargo fmt --all` 后再次 commit。

- [ ] **Step 3: 终端 smoke 矩阵**

在真实终端中依次跑（对每条命令记录是否符合预期）：

| 命令 | 关键操作 | 预期 |
|---|---|---|
| `zootree create` | 在 Title 输入「项目 标题」+ Backspace 验 CJK | 中文按字符删，画面与文本同步 |
| `zootree create` | Description Alt+Enter 换行后提交 | 落盘 yaml 中 description 含 `\n` |
| `zootree create` | Description 中粘贴含换行的多行文本 | 整段插入，不会因 `\n` 提前提交 |
| `zootree create` | repo selector 输入 filter 过滤 | 列表收敛，命中字符黄色高亮 |
| `zootree start` | 多个 pending workspace 中过滤选择 | 正常选中并启动 |
| `zootree done` | 同上 | 正常 |
| `zootree cancel <ws>` | confirm `[y/N]` | y / n / Enter 各自正确 |
| 任意上述命令 | 中途 Ctrl+C | stderr `aborted`，退出码 1 |
| 任意上述命令 | 中途 Esc（必填字段） | stderr `aborted`，退出码 1 |
| `zootree create` | Description 处 Esc | 描述空，命令继续 |
| `zootree create` | Target branch 处 Esc | 用 current 分支默认值，命令继续 |

- [ ] **Step 4: 验证 dialoguer 已彻底移除**

Run: `grep -rn dialoguer src/ tests/ Cargo.toml Cargo.lock | head -20`
Expected: 0 行匹配在 `src/` `tests/` `Cargo.toml`；`Cargo.lock` 中也应已移除。如果 `Cargo.lock` 残留，`cargo update -p dialoguer` 会报错（已不在依赖图），手动删除对应条目或重新 `cargo build` 即可刷新。

- [ ] **Step 5: 验证 dialoguer 间接依赖也清理**

Run: `cargo tree -i dialoguer 2>&1 | head -5; cargo tree -i console 2>&1 | head -5`
Expected: `cargo tree -i dialoguer` 报「package not found」；`console` 也不应在依赖图（除非别的 crate 也用）。

- [ ] **Step 6: 如果 smoke 通过，无需 commit；如有调整 commit 之**

```bash
git status
```
若没有改动则跳过 commit。否则：
```bash
git add -p
git commit -m "fix(tui): smoke 测试发现的回归"
```

---

## 自检（plan-vs-spec coverage）

| Spec 要求 | 对应 Task |
|---|---|
| 删除 dialoguer，新增 tui-textarea / unicode-width / fuzzy-matcher | Task 1, 13 |
| `Viewport::Inline`、动态高度 | Task 2, 7 |
| Kitty enhancement flags（Shift+Enter） | Task 7 |
| Bracketed paste | Task 7 |
| `terminal.insert_before` 写 scrollback 总结 | Task 7 |
| panic hook 扩展（pop flags + disable paste） | Task 7 |
| `TextPromptState`：CJK 删除、Alt+Enter / Shift+Enter、Enter 提交、Esc 语义、Ctrl+C | Task 3 |
| `SelectPromptState`：始终带过滤、模糊匹配、↑↓ 回绕、Enter 提交 | Task 4, 9 |
| `MultiSelectPromptState`：Space 切换、`a` 全选、勾选顺序索引 | Task 5, 10 |
| `ConfirmPromptState`：[Y/n] / [y/N]、Enter = default、y/n、Esc | Task 6, 11 |
| `tui.rs` 5 个公开签名不变，9 处 callsite 不动 | Task 8-11 |
| `CancelledByUser` 错误冒泡 → main 输出 `aborted` 并退出 1 | Task 2, 12 |
| 纯逻辑层单测（CJK、Alt+Enter、Shift+Enter、过滤、Space、a 键、confirm 键位等） | Task 3-6 |
| 人工 smoke 矩阵（CJK 删除、多行换行、粘贴、过滤、Esc / Ctrl+C 取消） | Task 13 |
| 不引入 vim 模式 / 历史 / Tab 补全（YAGNI） | 全任务无 |
| 不为 `create` 单独做全屏表单（YAGNI） | 全任务无 |
| Windows：粘贴是唯一可靠多行路径，不投入额外适配 | spec 已说明，计划无单独 Windows 任务 |

所有 spec 要求都有任务覆盖。无遗漏。

---

**计划完成。两个执行选项：**

1. **Subagent-Driven（推荐）** — 每个 Task 派一个新 subagent 执行，每个 Task 完成后我审，迭代快。
2. **Inline Execution** — 在当前会话里按 Task 顺序执行，带 checkpoint。

选哪种？
