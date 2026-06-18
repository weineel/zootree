# 文本输入框迁移到 ratatui-textarea 实施计划（v2）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 zootree 的 inline TUI 输入框获得 Emacs 风格快捷键 + soft-wrap + undo/redo。前置依赖：升级 ratatui 0.29→0.30 + crossterm 0.28→0.29，因为 ratatui-textarea 0.8+ 已切到 ratatui 0.30 拆包架构。

**Architecture:** 单 PR 合并 3 件事：
1. **ratatui 0.30 升级**：仅触动 `src/tui_app/mod.rs` 的 4 处 `?` 调用——通过给 `main_loop<B>` / `inline_loop<B>` 加 `B::Error: Send + Sync + 'static` 约束放过 `?` 转 anyhow。`info.rs` 不动（probe 验证）。
2. **crossterm 0.29 同步**：避免 ratatui-crossterm（依赖 crossterm 0.29）与项目本身（crossterm 0.28）双份依赖共存。
3. **ratatui-textarea 迁库 + handle_key 重写**：原 spec 的核心内容。`TextPromptState::handle_key` 由"按键白名单"改为"语义键拦截 + 委托 `textarea.input(key)`"；启用 `WrapMode::WordOrGlyph` 的 soft-wrap。

**Tech Stack:** Rust 2021、ratatui 0.30、crossterm 0.29、ratatui-textarea 0.9。

设计文档：`docs/superpowers/specs/2026-06-16-text-input-migrate-to-ratatui-textarea-design.md`。

**Probe 数据（已实测）：**
- 单纯升 ratatui 0.30：12 个 cargo check 错误。
  - 4 个在 `prompt.rs:23/153/531/532`——是因为旧 tui-textarea 0.7 还在，造成 ratatui 0.29/0.30 双份共存，类型不匹配。换库后自动消失。
  - 8 个在 `mod.rs:79/229/231/234`（每处 `Send` + `Sync` 各一）——`Backend::Error` 不再默认 `Send + Sync`，`?` 不能直接转 anyhow。
- crossterm：ratatui 0.30 经 ratatui-crossterm 0.1 拉入 crossterm 0.29；与项目锁的 0.28 会双份共存，必须同步升级。

---

## File Structure

| 文件 | 改动 | 责任 |
|---|---|---|
| `Cargo.toml` | 修改 3 个版本字段 | 依赖版本 |
| `Cargo.lock` | 由 cargo 自动重写 | 锁文件 |
| `src/tui_app/mod.rs` | 第 69 行 / 第 218 行 `where` 子句补 `B::Error: Send + Sync + 'static` | inline TUI 框架，必须兼容 ratatui 0.30 |
| `src/tui_app/prompt.rs` | use 改名；`TextPromptState::new` 启用 soft-wrap；`TextPromptState::handle_key` 重写；新增 4 条回归测试 | inline prompt 状态/逻辑 |

**不动**：`src/tui_app/info.rs`、`src/tui.rs`、`tests/**`、`src/core/**`、`src/main.rs`。

---

### Task 1: 三项依赖一并升级 + ratatui 0.30 适配

把 `Cargo.toml` 的 3 个版本一次性升级，编译失败后用 `mod.rs` 的最小改动修复——这是把 ratatui 0.30 升级合并进迁移 PR 的必要条件。本任务结束后 `cargo check` 应通过；测试可能失败（prompt.rs 的 use 还是旧路径），那由 Task 2 处理。

**为什么三项一起升**：单独升 ratatui 0.30 会产生 12 错误，其中 4 个是因为旧 tui-textarea 还在；只有同时换库、错误才收敛到 8 个独立错误。逐步升反而比一次到位更乱。

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/tui_app/mod.rs`（第 69 行、第 218 行附近）

- [ ] **Step 1: Cargo.toml 三项替换**

在 `Cargo.toml` 的 `[dependencies]` 段：

把：
```toml
ratatui = "0.29"
crossterm = "0.28"
tui-textarea = "0.7"
```

改为：
```toml
ratatui = "0.30"
crossterm = "0.29"
ratatui-textarea = "0.9"
```

注意：`tui-textarea` 这一行**完全删除**，换成 `ratatui-textarea`（key 不同，所以是删一行加一行，不是改值）。其他依赖项（unicode-width、anyhow、shellexpand 等）保持不变。

- [ ] **Step 2: 跑 cargo check，确认错误清单**

Run: `cargo check 2>&1 | tee /tmp/zootree-probe-check.log`

Expected: 编译失败。错误数应为 **大约 12 个**：
- 4 个在 `src/tui_app/prompt.rs` 的第 7 行附近（`unresolved import tui_textarea::...`），因为 use 路径还没改——Task 2 处理。
- 8 个在 `src/tui_app/mod.rs` 第 79、229、231、234 行附近，4 处 `?` 调用，每处 `Send` + `Sync` 各一个错误。错误信息形如：
  ```
  error[E0277]: `<B as ratatui_core::backend::Backend>::Error` cannot be sent between threads safely
  ```

如果错误数差异显著（>16 或 <8），STOP 报告——可能 ratatui 0.30 还有其他 surface 变化未被 probe 覆盖。

- [ ] **Step 3: 给 mod.rs 的两个泛型函数加 `B::Error: Send + Sync + 'static` bound**

打开 `src/tui_app/mod.rs`，定位两处函数签名（第 69 行和第 218 行附近）：

**第 69 行附近**，把：

```rust
fn main_loop<B: ratatui::backend::Backend, A: App>(
    terminal: &mut Terminal<B>,
    app: &mut A,
) -> anyhow::Result<()> {
```

改为：

```rust
fn main_loop<B: ratatui::backend::Backend, A: App>(
    terminal: &mut Terminal<B>,
    app: &mut A,
) -> anyhow::Result<()>
where
    B::Error: Send + Sync + 'static,
{
```

**第 218 行附近**，把：

```rust
fn inline_loop<B: ratatui::backend::Backend, A: InlineApp>(
    terminal: &mut ratatui::Terminal<B>,
    app: &mut A,
) -> anyhow::Result<...> {
```

改为相同模式（保留原返回类型，只在函数签名右括号后面、`{` 前面插入 `where` 子句）：

```rust
fn inline_loop<B: ratatui::backend::Backend, A: InlineApp>(
    terminal: &mut ratatui::Terminal<B>,
    app: &mut A,
) -> anyhow::Result<...>
where
    B::Error: Send + Sync + 'static,
{
```

> 注意：原 `inline_loop` 的返回类型不是单纯的 `anyhow::Result<()>`，照抄原代码即可；不要修改返回类型本身。

如果 mod.rs 里还有其他 `fn ...<B: ratatui::backend::Backend, ...>(...) -> anyhow::Result<...>` 的泛型函数也需要同样加 where 子句，逐一加。用 `grep -nE "<B: ratatui::backend::Backend" src/tui_app/mod.rs` 确认列表。

- [ ] **Step 4: 再跑 cargo check**

Run: `cargo check 2>&1 | tee /tmp/zootree-probe-check2.log`

Expected: 错误数从 ~12 降到 ~4，剩下的都在 `src/tui_app/prompt.rs:7`（`use tui_textarea::...` 仍然指向已删除的 crate）。`mod.rs` 不再有错误。

如果 `mod.rs` 仍然报 `Send`/`Sync` 错误，说明还有其他 `?` 调用点没覆盖到——再 `grep -n "?$" src/tui_app/mod.rs` 找剩余调用点，把对应函数的 where 子句也加上 bound。

- [ ] **Step 5: 不提交，进入 Task 2**

---

### Task 2: prompt.rs use 改名

把 `prompt.rs` 第 7 行的 `tui_textarea::` 改为 `ratatui_textarea::`，并新增 `WrapMode` 导入（Task 3 用到）。

**Files:**
- Modify: `src/tui_app/prompt.rs`（第 7 行）

- [ ] **Step 1: 改 use 语句**

在 `src/tui_app/prompt.rs`，把第 7 行：

```rust
use tui_textarea::{CursorMove, TextArea};
```

改为：

```rust
use ratatui_textarea::{CursorMove, TextArea, WrapMode};
```

`WrapMode` 暂未使用，预期 rustc 出 `unused_imports` warning——Task 3 立刻就会用到。

- [ ] **Step 2: cargo check 通过**

Run: `cargo check 2>&1`
Expected: 编译成功；可能有 1 个 `unused_imports: WrapMode` warning（预期）。

如果出现 `TextArea` / `CursorMove` 相关的方法不存在 / signature 不匹配错误，STOP 报告——基础 API 两个 fork 应该一致。

- [ ] **Step 3: 跑测试，建立"换库后行为相同"基线**

Run: `cargo test 2>&1 | tee /tmp/zootree-postswap-test.log`
Expected: 全部测试通过——`TextArea::insert_char` / `delete_char` / `move_cursor(CursorMove::...)` / `insert_newline` / `insert_str` / `lines()` / `set_cursor_line_style` 这些 API 在 ratatui-textarea 0.9 与 tui-textarea 0.7 一致，所以现有测试应不变。

如果有失败：
- `info.rs` 测试失败：说明 `TestBackend` API 在 0.30 有意外变化。在 `src/tui_app/info.rs:498` 附近的 `render_to_string` 函数里 `Terminal::new(backend).unwrap()`：若 `TestBackend::new(w, h)` 现在直接返回 `TestBackend`（不再是 `Result`），把 `.unwrap()` 那行的 backend 调用改成不带 `.unwrap()`；若 `Terminal::new` 返回类型变化，用相应的 stable trait 方法。逐一定位、最小修复，并在 commit message 中记录。
- `prompt.rs` 测试失败：先记录失败列表，进入 Task 3，最后在 Task 5 复盘是否仍存在。

- [ ] **Step 4: 不提交，进入 Task 3**

---

### Task 3: TextPromptState::new 启用 soft-wrap

**Files:**
- Modify: `src/tui_app/prompt.rs` — `TextPromptState::new`（约第 20–30 行）

- [ ] **Step 1: 在构造函数里启用 soft-wrap**

定位 `impl TextPromptState { pub fn new(prompt: &str) -> Self { ... } }`。在 `textarea.set_cursor_line_style(Style::default());` 之后、`Self { ... }` 之前，插入两行注释 + 一行调用：

```rust
        // 长行按可视宽度自动折行显示；逻辑上仍是一行。
        // WordOrGlyph: 优先按词断行，长单词回退到 grapheme cluster。
        textarea.set_wrap_mode(WrapMode::WordOrGlyph);
```

完整函数应为：

```rust
    pub fn new(prompt: &str) -> Self {
        let mut textarea = TextArea::default();
        textarea.set_cursor_line_style(Style::default());
        // 长行按可视宽度自动折行显示；逻辑上仍是一行。
        // WordOrGlyph: 优先按词断行，长单词回退到 grapheme cluster。
        textarea.set_wrap_mode(WrapMode::WordOrGlyph);
        Self {
            prompt: prompt.to_string(),
            required: true,
            textarea,
            outcome: None,
        }
    }
```

**不要**把 `set_wrap_mode` 加到 `SelectPromptState::new` 的 filter textarea——那是单行场景。

- [ ] **Step 2: cargo test 应仍绿**

Run: `cargo test`
Expected: ALL PASS。`line_count()` 仍按逻辑行计数（`textarea.lines().len()`），所以 `text_desired_height_*` 三条测试不受影响。

如果 `cargo build` 报 `no method named set_wrap_mode` 或 `cannot find type WrapMode`，STOP 报告——这与计划研究的 ratatui-textarea 0.9 API 矛盾。

- [ ] **Step 3: 不提交，进入 Task 4**

---

### Task 4: 重写 TextPromptState::handle_key（TDD）

先加 4 条 emacs 键回归测试 → 看到失败 → 重写 handle_key → 看到通过 → 跑完整测试集。

**Files:**
- Modify: `src/tui_app/prompt.rs` — `TextPromptState::handle_key`（约第 68–124 行）及末尾 `mod tests`

- [ ] **Step 1: 在 mod tests 末尾追加 4 条新测试**

在 `src/tui_app/prompt.rs` 末尾的 `#[cfg(test)] mod tests { ... }` 内、紧跟 `confirm_release_event_is_ignored` 测试之后、`mod tests` 闭合 `}` 之前，追加：

```rust
    #[test]
    fn text_ctrl_a_moves_to_line_start() {
        // 验证 Emacs 键 Ctrl+A：通过库的 input() 路径生效。
        let mut s = TextPromptState::new("Title").required();
        s.handle_key(key(KeyCode::Char('a')));
        s.handle_key(key(KeyCode::Char('b')));
        s.handle_key(key(KeyCode::Char('c')));
        // 光标当前在 "abc" 末尾；Ctrl+A 应跳到行首
        s.handle_key(key_mod(KeyCode::Char('a'), KeyModifiers::CONTROL));
        // 在行首插入 'X' 验证位置
        s.handle_key(key(KeyCode::Char('X')));
        assert_eq!(s.text(), "Xabc");
    }

    #[test]
    fn text_ctrl_e_moves_to_line_end() {
        // 验证 Emacs 键 Ctrl+E：通过库的 input() 路径生效。
        let mut s = TextPromptState::new("Title").required();
        s.handle_key(key(KeyCode::Char('a')));
        s.handle_key(key(KeyCode::Char('b')));
        s.handle_key(key(KeyCode::Char('c')));
        s.handle_key(key_mod(KeyCode::Char('a'), KeyModifiers::CONTROL)); // 跳到行首
        s.handle_key(key_mod(KeyCode::Char('e'), KeyModifiers::CONTROL)); // 再跳到行尾
        s.handle_key(key(KeyCode::Char('Z')));
        assert_eq!(s.text(), "abcZ");
    }

    #[test]
    fn text_ctrl_w_deletes_previous_word() {
        // 验证 Emacs 键 Ctrl+W：删除前一个词。
        let mut s = TextPromptState::new("Title").required();
        for c in "hello world".chars() {
            s.handle_key(key(KeyCode::Char(c)));
        }
        assert_eq!(s.text(), "hello world");
        s.handle_key(key_mod(KeyCode::Char('w'), KeyModifiers::CONTROL));
        // "world" 被删除；"hello " 后可能保留或不保留尾随空格，
        // 取决于库的词边界定义。两种都接受，但至少 "world" 必须没了。
        let after = s.text();
        assert!(
            after == "hello " || after == "hello",
            "expected 'hello' or 'hello ', got {:?}",
            after
        );
    }

    #[test]
    fn text_ctrl_k_cuts_to_line_end() {
        // 验证 Emacs 键 Ctrl+K：从光标位置到行尾的内容被删除。
        let mut s = TextPromptState::new("Title").required();
        for c in "foobar".chars() {
            s.handle_key(key(KeyCode::Char(c)));
        }
        s.handle_key(key_mod(KeyCode::Char('a'), KeyModifiers::CONTROL)); // 跳到行首
        s.handle_key(key(KeyCode::Right)); // 移到 'o' 之前的第 1 位
        s.handle_key(key(KeyCode::Right)); // 移到 'o' 之前的第 2 位
        s.handle_key(key(KeyCode::Right)); // 现在光标在 'b' 之前
        s.handle_key(key_mod(KeyCode::Char('k'), KeyModifiers::CONTROL));
        assert_eq!(s.text(), "foo");
    }
```

- [ ] **Step 2: 运行新增测试，确认 FAIL**

Run: `cargo test --lib prompt::tests::text_ctrl`
Expected: 至少 1 条失败（很可能 4 条都失败）。当前 `handle_key` 的 `Char(c) if ctrl => insert_char(c)` 分支会把 Ctrl+a 当成插入 'a'，所以 `text_ctrl_a_moves_to_line_start` 会得到 "abcaX" 而非 "Xabc"；其他类似。

- [ ] **Step 3: 重写 handle_key + 清理 unused import**

把 `src/tui_app/prompt.rs` 中 `impl TextPromptState` 内 `pub fn handle_key(&mut self, key: KeyEvent)` 的**整段函数体**替换为：

```rust
    pub fn handle_key(&mut self, key: KeyEvent) {
        if self.outcome.is_some() {
            return;
        }
        if key.kind != KeyEventKind::Press {
            return;
        }
        let m = key.modifiers;
        let ctrl = m.contains(KeyModifiers::CONTROL);
        let alt = m.contains(KeyModifiers::ALT);
        let shift = m.contains(KeyModifiers::SHIFT);

        // —— zootree 自定义语义键：必须在 textarea.input() 之前拦截 ——
        // - Ctrl+C：硬中断
        // - Esc：取消（必填）或跳过（选填）
        // - Alt|Shift+Enter：显式插入换行
        // - 裸 Enter：提交（库默认会把 Enter 当 newline，必须截下）
        match key.code {
            KeyCode::Char('c') if ctrl => {
                self.outcome = Some(PromptOutcome::Interrupted);
                return;
            }
            KeyCode::Esc => {
                self.outcome = Some(if self.required {
                    PromptOutcome::Aborted
                } else {
                    PromptOutcome::Skipped
                });
                return;
            }
            KeyCode::Enter if alt || shift => {
                self.textarea.insert_newline();
                return;
            }
            KeyCode::Enter => {
                let text = self.text();
                self.outcome = Some(PromptOutcome::Submitted(text));
                return;
            }
            _ => {}
        }

        // 其余所有键交给库：Backspace、方向键、Home/End、
        // Ctrl+A/E/W/U/K、Alt+B/F/D、Ctrl+Z/R(undo/redo)、文本选择、yank……
        // 返回值是"是否改动了 buffer"，对我们没有意义。
        let _ = self.textarea.input(key);
    }
```

然后**清理 unused import**。原 use 行：

```rust
use ratatui_textarea::{CursorMove, TextArea, WrapMode};
```

改为：

```rust
use ratatui_textarea::{TextArea, WrapMode};
```

`CursorMove` 在重写后的 `handle_key` 里不再使用。检查整个文件其他地方是否还在用 `CursorMove`：`grep -n CursorMove src/tui_app/prompt.rs`，应没有命中。

- [ ] **Step 4: 跑新增测试，应通过**

Run: `cargo test --lib prompt::tests::text_ctrl`
Expected: PASS（4 条全过）。

如果 `text_ctrl_k_cuts_to_line_end` 失败：跑 `cargo test --lib prompt::tests::text_ctrl_k_cuts_to_line_end -- --nocapture` 抓打印；报告 actual 字符串和分析。Fallback：把 `KeyCode::Right => textarea.move_cursor(CursorMove::Forward)` 显式拦截重新加上（同时把 `CursorMove` 加回 use），但这种回退说明 input() 对 Right 的处理与预期不一致，不应静默接受——记入 DONE_WITH_CONCERNS。

如果 `text_ctrl_w_deletes_previous_word` 的 actual 字符串既不是 "hello" 也不是 "hello "（例如保留了 "hello w"），说明库的词边界定义跟主流 emacs 习惯不同——报告 actual 值，让我们决定是否修订测试断言。

- [ ] **Step 5: 跑完整测试，确认旧用例仍绿**

Run: `cargo test`
Expected: ALL PASS——包括：
- `text_cjk_backspace_removes_one_char`
- `text_alt_enter_inserts_newline` / `text_shift_enter_inserts_newline`
- `text_enter_submits`
- `text_optional_esc_skipped` / `text_required_esc_aborted` / `text_ctrl_c_interrupted`
- `text_paste_inserts_multiline_without_submitting` / `text_paste_crlf_inserts_single_newline`
- `text_after_outcome_keys_are_ignored` / `text_release_event_is_ignored`
- `text_char_c_without_ctrl_inserts_literal`
- `text_desired_height_*`（3 条）
- 所有 select / multi / confirm 测试
- `info.rs` 的 `render_*` 测试（如果 Task 2 步 3 已通过，这里也应通过）

**Failure handling**:

- `text_paste_crlf_inserts_single_newline` 失败：说明 ratatui-textarea 的 `insert_str` 不归一化 CRLF。Fix：在同文件的 `handle_paste` 中：

  ```rust
  pub fn handle_paste(&mut self, s: &str) {
      if self.outcome.is_some() { return; }
      // ratatui-textarea 的 insert_str 不一定规范化 CRLF；显式归一化保持与
      // tui-textarea 0.7 时代相同的行为。
      let normalized = s.replace("\r\n", "\n");
      self.textarea.insert_str(&normalized);
  }
  ```
  
  纳入本任务的工作；不需要单独 commit。

- `text_cjk_backspace_removes_one_char` 失败：极不可能；若发生，把 `KeyCode::Backspace => self.textarea.delete_char()` 加回到拦截 match 块的开头（重新放在 Enter 处理之前），保持其他逻辑不变。

- `info.rs` 测试在这一步失败（前面没失败但这里失败）：不可能发生（前面已通过）；若发生 STOP 报告。

- 其他失败：逐项定位差异，最小修复，commit message 中记录。

- [ ] **Step 6: 不提交，留待 Task 5 一次提交**

---

### Task 5: 验证 + 单 commit

**Files:** 验证 `Cargo.toml`、`Cargo.lock`、`src/tui_app/mod.rs`、`src/tui_app/prompt.rs`。

- [ ] **Step 1: 全量测试**

Run: `cargo test`
Expected: 全绿。

- [ ] **Step 2: clippy**

Run: `cargo clippy --all-targets -- -D warnings`
Expected: 无 warning。

常见问题处理：
- `unused_variables: shift` ——把 `let shift = m.contains(KeyModifiers::SHIFT);` 改为 `let _shift = m.contains(KeyModifiers::SHIFT);`，**只在 rustc/clippy 实际报警时改**。
- `unused_imports: CursorMove`（应该已在 Task 4 步 3 清理）——再次确认 use 行只剩 `{TextArea, WrapMode}`。

- [ ] **Step 3: 手动验证（由用户而非 subagent 进行）**

> Subagent 应在 Step 2 通过后**停下并在报告中提示**：本步是手动验证（交互式 TUI 在 subagent 环境下无法模拟），由用户在合并前完成。Step 4（commit）可以由 subagent 直接做。

用户手动验证项（subagent 不必执行；写在 commit message 里通知用户去验）：
1. `cargo run -- create` 进入交互输入。
2. 窄终端验证 soft-wrap：把终端宽度调到 60 列，输入一长串，应自动折行。
3. Emacs 键：Ctrl+A/E、Ctrl+W、Alt+B/F、Ctrl+U、Ctrl+Z/R。
4. zootree 语义键：Enter 提交、Alt+Enter 换行、Esc 取消/跳过、Ctrl+C 中断。
5. CJK：输入"你好世界"，Backspace 一次删除"界"。

- [ ] **Step 4: 提交**

Run:

```bash
git add Cargo.toml Cargo.lock src/tui_app/mod.rs src/tui_app/prompt.rs
git commit -m "feat(tui): 升级 ratatui 0.30 + 迁移输入框到 ratatui-textarea

依赖升级：
- ratatui 0.29 -> 0.30（拆包架构 ratatui-core + ratatui-widgets）
- crossterm 0.28 -> 0.29（与 ratatui-crossterm 共享）
- tui-textarea 0.7 -> ratatui-textarea 0.9（ratatui 官方生态 fork）

src/tui_app/mod.rs：main_loop / inline_loop 的泛型 where 子句补
\`B::Error: Send + Sync + 'static\`——ratatui 0.30 的 Backend::Error
不再默认 Send + Sync，\`?\` 转 anyhow 需要显式 bound。

src/tui_app/prompt.rs：
- TextPromptState::new 启用 WrapMode::WordOrGlyph，长行按可视宽度
  自动折行（逻辑上仍是一行）。
- TextPromptState::handle_key 由按键白名单改为'语义键拦截 +
  委托 input()'：Ctrl+C / Esc / Alt|Shift+Enter / 裸 Enter 仍由
  zootree 显式处理；Backspace / 方向键 / Home/End / Ctrl+A/E/W/U/K /
  Alt+B/F/D / Ctrl+Z/R(undo/redo) 由库统一处理。
- 新增 4 条回归测试验证 Emacs 键生效。

SelectPromptState / MultiSelectPromptState / ConfirmPromptState 代码不变。

详见 docs/superpowers/specs/2026-06-16-text-input-migrate-to-ratatui-textarea-design.md"
```

Expected: 提交成功，工作区干净。

- [ ] **Step 5: 提示用户做交互式验证**

报告中写明："commit 已落地。请运行 `cargo run -- create` 走一遍交互输入，验证 soft-wrap + Emacs 键 + 语义键 + CJK 行为符合预期。"

---

## Self-Review

- **Spec coverage**：
  - 决策 1（换库）→ Task 1（依赖换） + Task 2（use 改名）
  - 决策 2（handle_key 委托）→ Task 4
  - 决策 3（仅 TextPromptState 开 soft-wrap）→ Task 3；SelectPromptState 未触及
  - 决策 4（line_count 仍按逻辑行）→ 未改 `line_count()`；Task 3 步 2 与 Task 4 步 5 验证 `text_desired_height_*`
  - 决策 5（粘贴路径不变）→ Task 4 步 5 验证 + CRLF 退路
  - ratatui 0.30 升级 → Task 1 步 3（mod.rs Send + Sync 修复）
  - crossterm 0.29 同步 → Task 1 步 1
- **Placeholder scan**：无 TBD/TODO；每个失败处理路径都给了具体替换代码。
- **Type consistency**：`textarea.input(key)`、`textarea.insert_newline()`、`textarea.insert_str(&s)`、`textarea.lines()`、`textarea.set_cursor_line_style(...)`、`set_wrap_mode(WrapMode::WordOrGlyph)`、`PromptOutcome::{Interrupted, Aborted, Skipped, Submitted}`、`KeyEventKind::Press`、`KeyModifiers::{CONTROL, ALT, SHIFT}`、`B::Error: Send + Sync + 'static` 全文一致。
