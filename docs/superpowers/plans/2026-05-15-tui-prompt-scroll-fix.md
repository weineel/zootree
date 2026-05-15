# TUI Text Prompt 多行输入向上卷屏修复实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 抬高 `TextPromptState` 编辑区下界（5 行）以消除常见输入下 inline viewport 的 `ScrollUp`，并在 4 个 prompt state 的 `handle_key` 入口过滤非 Press KeyEvent。

**Architecture:** 仅修改 `src/tui_app/prompt.rs` 一个文件。按 TDD 把改动分为两组 commit：(1) `desired_height` 下界从 1 提到 5 + 三条单测；(2) 四个 `*PromptState::handle_key` 顶部增加 `KeyEventKind::Press` 过滤 + 四条单测。最后在 Alacritty 内手测验收。

**Tech Stack:** Rust / ratatui / crossterm / tui-textarea。配套 spec：`docs/superpowers/specs/2026-05-15-tui-prompt-scroll-fix-design.md`。

---

## 文件清单

- 修改：`src/tui_app/prompt.rs`
  - 顶部 `use` 列表（line 6）：补 `KeyEventKind`
  - line 468：在 `TEXT_MAX_VISIBLE_LINES` 旁新增 `TEXT_MIN_VISIBLE_LINES` 常量
  - `impl InlineApp for TextPromptState::desired_height`（line 525-529）：用新常量做下界
  - 4 处 `*PromptState::handle_key`（lines 66, 235, 368, 434）：开头添加 Release/Repeat 过滤
  - `mod tests`（line 772+）：新增 7 条测试

无新增/删除文件，无依赖变化。

---

### Task 1: TextPromptState `desired_height` 提高下界

**Files:**
- Modify: `src/tui_app/prompt.rs:468`（常量）
- Modify: `src/tui_app/prompt.rs:525-529`（`desired_height`）
- Test: `src/tui_app/prompt.rs`（`mod tests` 内追加）

- [ ] **Step 1.1: 在测试模块顶部 use 列表添加 InlineApp**

定位 `mod tests`（约 line 772）的 use 列表：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui_app::PromptOutcome;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    ...
```

补上一行，使 `desired_height` 这个 trait 方法在测试里可以直接 `s.desired_height()` 调用：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui_app::{InlineApp, PromptOutcome};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    ...
```

（即把原本的 `use crate::tui_app::PromptOutcome;` 替换为合并后的 `use crate::tui_app::{InlineApp, PromptOutcome};`）

- [ ] **Step 1.2: 在 `mod tests` 末尾、紧邻最后一个 `confirm_*` 测试之后追加三条新测试**

```rust
    #[test]
    fn text_desired_height_floor_is_min_visible() {
        let s = TextPromptState::new("Title").required();
        // 1 header + 5 editor (floor) + 2 borders + 1 help = 9
        assert_eq!(s.desired_height(), 9);
    }

    #[test]
    fn text_desired_height_unchanged_for_few_newlines() {
        let mut s = TextPromptState::new("Desc").optional();
        // Three Alt+Enter → line_count = 4, still under floor 5
        s.handle_key(key_mod(KeyCode::Enter, KeyModifiers::ALT));
        s.handle_key(key_mod(KeyCode::Enter, KeyModifiers::ALT));
        s.handle_key(key_mod(KeyCode::Enter, KeyModifiers::ALT));
        assert_eq!(s.line_count(), 4);
        assert_eq!(s.desired_height(), 9);
    }

    #[test]
    fn text_desired_height_grows_above_floor() {
        let mut s = TextPromptState::new("Desc").optional();
        for _ in 0..6 {
            s.handle_key(key_mod(KeyCode::Enter, KeyModifiers::ALT));
        }
        // line_count = 7 → 1 + 7 + 2 + 1 = 11
        assert_eq!(s.line_count(), 7);
        assert_eq!(s.desired_height(), 11);
    }
```

- [ ] **Step 1.3: 跑测试，确认 1.1 OK + 三条新测试中第一条立刻失败（断言 5 而拿到 5+5）**

实际：当前实现 `desired_height` 在 line_count=1 返回 `1 + 1 + 2 + 1 = 5`，期望 9 → 失败。

```bash
cargo test -p zootree --lib tui_app::prompt::tests::text_desired_height -- --nocapture
```

期望输出包含：

```
test tui_app::prompt::tests::text_desired_height_floor_is_min_visible ... FAILED
left: 5, right: 9
```

- [ ] **Step 1.4: 在 `src/tui_app/prompt.rs:468` 旁追加下界常量**

把：

```rust
const TEXT_MAX_VISIBLE_LINES: u16 = 10;
```

改为：

```rust
const TEXT_MIN_VISIBLE_LINES: u16 = 5;
const TEXT_MAX_VISIBLE_LINES: u16 = 10;
```

- [ ] **Step 1.5: 修改 `impl InlineApp for TextPromptState::desired_height`（line 525-529）**

原代码：

```rust
    fn desired_height(&self) -> u16 {
        let editor = (self.line_count() as u16).clamp(1, TEXT_MAX_VISIBLE_LINES);
        // 1 header + editor + 2 borders + 1 help
        1 + editor + 2 + 1
    }
```

改为：

```rust
    fn desired_height(&self) -> u16 {
        let editor = (self.line_count() as u16)
            .clamp(TEXT_MIN_VISIBLE_LINES, TEXT_MAX_VISIBLE_LINES);
        // 1 header + editor + 2 borders + 1 help
        1 + editor + 2 + 1
    }
```

- [ ] **Step 1.6: 跑测试，三条 `text_desired_height_*` 全过；其余测试不回归**

```bash
cargo test -p zootree --lib tui_app::prompt
```

期望：所有 prompt 单测通过，新增 3 条全绿。

- [ ] **Step 1.7: Commit**

```bash
git add src/tui_app/prompt.rs
git commit -m "$(cat <<'EOF'
fix(tui_app): TextPromptState desired_height 下界从 1 提到 5

inline viewport 在 ScrollUp 让出空间时会把已有终端内容上推，导致 Alt+Enter
增高 viewport 时上一题的总结、当前 prompt 的 header 被滚出可视区。把编辑区
下界设为 5，让常见输入规模（≤5 行）保持 9 行高度恒定，不触发 resize。

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

### Task 2: 4 个 `handle_key` 入口过滤非 Press 事件

**Files:**
- Modify: `src/tui_app/prompt.rs:6`（顶部 use 列表）
- Modify: `src/tui_app/prompt.rs:66-68`（`TextPromptState::handle_key`）
- Modify: `src/tui_app/prompt.rs:235-237`（`SelectPromptState::handle_key`）
- Modify: `src/tui_app/prompt.rs:368-370`（`MultiSelectPromptState::handle_key`）
- Modify: `src/tui_app/prompt.rs:434-436`（`ConfirmPromptState::handle_key`）
- Test: `src/tui_app/prompt.rs`（`mod tests` 内追加）

- [ ] **Step 2.1: 在 `mod tests` 末尾追加四条新测试**

紧接 Task 1 追加的三条测试之后，再追加：

```rust
    fn key_release(code: KeyCode, m: KeyModifiers) -> KeyEvent {
        use crossterm::event::KeyEventKind;
        KeyEvent::new_with_kind(code, m, KeyEventKind::Release)
    }

    #[test]
    fn text_release_event_is_ignored() {
        let mut s = TextPromptState::new("Title").required();
        s.handle_key(key(KeyCode::Char('a')));
        s.handle_key(key_release(KeyCode::Enter, KeyModifiers::ALT));
        assert_eq!(s.text(), "a");
        assert_eq!(s.line_count(), 1);
        assert!(s.outcome().is_none());
    }

    #[test]
    fn select_release_event_is_ignored() {
        let items = vec!["a".into(), "b".into(), "c".into()];
        let mut s = SelectPromptState::new("Pick", items);
        // cursor starts at 0; a Release Down event must NOT advance it.
        s.handle_key(key_release(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(s.cursor_visible_index(), Some(0));
        assert!(s.filter_text().is_empty());
    }

    #[test]
    fn multi_release_event_is_ignored() {
        let mut s = MultiSelectPromptState::new("Pick", vec!["a".into(), "b".into()]);
        s.handle_key(key_release(KeyCode::Char(' '), KeyModifiers::NONE));
        assert!(!s.is_checked(0));
        assert!(s.outcome().is_none());
    }

    #[test]
    fn confirm_release_event_is_ignored() {
        let mut s = ConfirmPromptState::new("Delete?", false);
        s.handle_key(key_release(KeyCode::Char('y'), KeyModifiers::NONE));
        assert!(s.outcome().is_none());
    }
```

- [ ] **Step 2.2: 跑测试，确认四条 release 测试都失败（Release 当前会被当 Press 处理）**

```bash
cargo test -p zootree --lib tui_app::prompt::tests -- --nocapture release_event_is_ignored
```

期望：

```
test tui_app::prompt::tests::text_release_event_is_ignored ... FAILED
test tui_app::prompt::tests::select_release_event_is_ignored ... FAILED
test tui_app::prompt::tests::multi_release_event_is_ignored ... FAILED
test tui_app::prompt::tests::confirm_release_event_is_ignored ... FAILED
```

具体失败原因：例如 `text_release_event_is_ignored` 会因为 Alt+Enter Release 也走进 `insert_newline` 分支，导致 `line_count == 2`、`text() == "a\n"`。

- [ ] **Step 2.3: 修改 `src/tui_app/prompt.rs:6` 顶部 use**

原：

```rust
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
```

改为：

```rust
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
```

- [ ] **Step 2.4: 在 `TextPromptState::handle_key`（line 66）顶部插入 Press 过滤**

原代码（line 66-69）：

```rust
    pub fn handle_key(&mut self, key: KeyEvent) {
        if self.outcome.is_some() {
            return;
        }
        let m = key.modifiers;
```

改为：

```rust
    pub fn handle_key(&mut self, key: KeyEvent) {
        if self.outcome.is_some() {
            return;
        }
        if key.kind != KeyEventKind::Press {
            return;
        }
        let m = key.modifiers;
```

- [ ] **Step 2.5: 同样修改 `SelectPromptState::handle_key`（约 line 235）**

原：

```rust
    pub fn handle_key(&mut self, key: KeyEvent) {
        if self.outcome.is_some() {
            return;
        }
        let m = key.modifiers;
```

改为：

```rust
    pub fn handle_key(&mut self, key: KeyEvent) {
        if self.outcome.is_some() {
            return;
        }
        if key.kind != KeyEventKind::Press {
            return;
        }
        let m = key.modifiers;
```

- [ ] **Step 2.6: 同样修改 `MultiSelectPromptState::handle_key`（约 line 368）**

原：

```rust
    pub fn handle_key(&mut self, key: KeyEvent) {
        if self.outcome.is_some() {
            return;
        }
        let m = key.modifiers;
```

改为：

```rust
    pub fn handle_key(&mut self, key: KeyEvent) {
        if self.outcome.is_some() {
            return;
        }
        if key.kind != KeyEventKind::Press {
            return;
        }
        let m = key.modifiers;
```

- [ ] **Step 2.7: 同样修改 `ConfirmPromptState::handle_key`（约 line 434）**

原：

```rust
    pub fn handle_key(&mut self, key: KeyEvent) {
        if self.outcome.is_some() {
            return;
        }
        let m = key.modifiers;
```

改为：

```rust
    pub fn handle_key(&mut self, key: KeyEvent) {
        if self.outcome.is_some() {
            return;
        }
        if key.kind != KeyEventKind::Press {
            return;
        }
        let m = key.modifiers;
```

- [ ] **Step 2.8: 跑测试，新增四条 release 测试转绿，所有原有 prompt 测试不回归**

```bash
cargo test -p zootree --lib tui_app::prompt
```

期望：所有 prompt 单测通过。`KeyEvent::new` 默认产出 Press，所有原有测试不受影响。

另外跑一次完整 lib 测试确认无回归：

```bash
cargo test -p zootree --lib
```

期望：全绿。

- [ ] **Step 2.9: Commit**

```bash
git add src/tui_app/prompt.rs
git commit -m "$(cat <<'EOF'
fix(tui_app): handle_key 仅处理 KeyEventKind::Press 事件

kitty 协议（DISAMBIGUATE_ESCAPE_CODES + REPORT_ALTERNATE_KEYS）下，部分终端
会下发 Press / Release 双事件。原 handle_key 不区分 kind 会让一次按键被处理
两次（如 Alt+Enter 写两个 \n）。在 4 个 *PromptState::handle_key 顶部统一
过滤非 Press 事件。

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

### Task 3: Alacritty 手测验收

**Files:** 无改动。

- [ ] **Step 3.1: 编译运行 `zootree create`**

```bash
cargo run -- create
```

- [ ] **Step 3.2: 验证 prompt 初始高度**

观察 title prompt 出现时占 9 行（1 header + 5 编辑行 + 2 边框 + 1 help）。输入若干字符（含中文），按 Enter 提交。

- [ ] **Step 3.3: 验证 5 行内 Alt+Enter 不滚屏**

进入 description prompt 后，连按 Alt+Enter 三次。期望：

- 画面没有任何向上滚动；
- title 的 `✔` 总结仍可见在 description prompt 上方；
- description 编辑框内能看到 4 行（1 当前行 + 3 空行），光标在第 4 行行首。

按 Shift+Enter 同样验证一次（Alacritty 0.13+ 应识别为带 SHIFT）。

- [ ] **Step 3.4: 验证超过 5 行才开始扩高**

继续按 Alt+Enter 直到 line_count 第 6 次时（编辑器内容达到 6 行），观察画面这一刻才开始上滚 1 行（因为 desired_height 从 9 → 10）。再多按几次每次再上滚 1 行直至总高 14 触顶。Esc 取消。

- [ ] **Step 3.5: 验证其它 prompt 无回归**

```bash
cargo run -- info
```

任意选一个 workspace；用 ↑↓ + 模糊过滤导航；按 Esc 退出。期望：select / 过滤行为与改动前一致，无视觉异常。

```bash
cargo run -- open
```

触发到 confirm prompt（如果该路径有），按 y / n / Esc 试一遍。期望行为与改动前一致。

> 如果发现行为有偏差，回到 Task 1/2 修复后再回到本步重测。

---

## Self-Review 清单

- **Spec coverage**：spec 的"改动 1"由 Task 1 实现；"改动 2"由 Task 2 实现；"改动 3 单测"分别在 Task 1.2 / Task 2.1 完成（5 条 height + release 类，spec 列了 6 条；本计划合并为 7 条：3 条 height + 1 条 helper + 4 条 release，覆盖等价）。Spec 的"测试/验收"步骤由 Task 3 的手测脚本覆盖。
- **Placeholder scan**：每步都给了完整代码或精确命令；无 TBD/TODO。
- **Type 一致性**：`TEXT_MIN_VISIBLE_LINES` 在 Task 1.4 定义为 `u16`，Task 1.5 中 `clamp(TEXT_MIN_VISIBLE_LINES, TEXT_MAX_VISIBLE_LINES)` 与已有 `TEXT_MAX_VISIBLE_LINES: u16 = 10` 类型一致；`KeyEventKind::Press` 的常量名与 Task 2.1 的 `KeyEventKind::Release` 同一 enum；`key_release` 助手函数的签名 `(KeyCode, KeyModifiers) -> KeyEvent` 与 4 条 release 测试调用方式一致。
