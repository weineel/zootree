# TUI Text Prompt 多行输入向上卷屏修复

- 日期：2026-05-15
- 作者：weineel + Claude
- 状态：已通过 brainstorming，待 writing-plans
- 关联：基于 `docs/superpowers/specs/2026-05-13-tui-interaction-design.md` 已落地的实现

## 背景

`2926aee` 之后的 inline prompt 实现在用户体验上暴露一个具体问题：

> 在 Alacritty 终端中，文本 prompt（`zootree create` 的 title / description 等）按 `Shift+Enter` 或 `Alt+Enter`，画面会向上滚一行；多按几次或在多 prompt 序列中（先 title 再 description），上一题的 `✔ title: ...` 总结、当前 prompt 的 header 被推出可视区。用户的直观感受是"输入被清空，没有换行"。

## 根因

`src/tui_app/mod.rs` 的 `inline_loop` 在每帧前检查 `desired_height` 变化：

```rust
let height = app.desired_height().max(1);
if height != last_height {
    terminal.resize(Rect::new(0, 0, terminal.size()?.width, height))?;
    last_height = height;
}
```

`Viewport::Inline` 的 `terminal.resize` 在变高且光标下方剩余行不足时，会发 `ScrollUp(diff)` 把已有终端内容上移，给 viewport 让出空间。`TextPromptState::desired_height` 当前为：

```rust
fn desired_height(&self) -> u16 {
    let editor = (self.line_count() as u16).clamp(1, TEXT_MAX_VISIBLE_LINES);
    1 + editor + 2 + 1
}
```

下界是 1，所以用户每按一次 Alt+Enter，`line_count` 从 N 增至 N+1，`desired_height` 同步 +1，触发一次 `ScrollUp(1)`。在 vertically constrained 的终端窗口里这是普遍发生的。

按键路径本身没有 bug：Alacritty 0.13+ 支持 kitty `DISAMBIGUATE_ESCAPE_CODES`，`Shift+Enter` / `Alt+Enter` 都被正确识别为带修饰符的 KeyEvent，落到 `KeyCode::Enter if alt || shift` 分支并 `insert_newline()`；现有单测 `text_alt_enter_inserts_newline` / `text_shift_enter_inserts_newline` 已经覆盖。可视区"看不到"是 viewport 已被 ScrollUp 推走，不是 textarea 没长高。

次要隐患：四个 `*PromptState::handle_key` 当前不区分 `KeyEventKind`。在 kitty 协议下，部分终端会同时下发 Press 与 Release 事件，可能造成单次 Alt+Enter 写入两个 `\n`、单次字符触发两次输入等。

## 目标 / 非目标

### 目标

1. 在常见输入规模（≤ 5 行）下，多行输入不再触发 `ScrollUp`，用户视野稳定。
2. 解决潜在 KeyEventKind 双发隐患，避免 Press/Release 同时被处理。
3. 不破坏 `src/tui.rs` 公开签名、不改 callsite、不改依赖。
4. 单测覆盖新增行为。

### 非目标（YAGNI）

- 不为 `TextPromptState` 引入预留 MAX 行（A1）：会让 prompt 一出现就大幅 ScrollUp。
- 不让前一 prompt 的 `✔ ...` 总结永远可见：那是把"已完成 prompt 历史"嵌入当前 inline viewport 的结构性改造，不在本次范围。
- 不调整 `SelectPromptState` / `MultiSelectPromptState` / `ConfirmPromptState` 的 `desired_height`：它们高度基本静态，无相同问题。
- 不改 ratatui 版本或 viewport 模式（继续 `Viewport::Inline`）。
- 不引入新 crate 或新依赖。

## 设计

### 改动 1：抬高 `TextPromptState` 的可见编辑区下限

文件：`src/tui_app/prompt.rs`

新增常量（紧邻现有 `TEXT_MAX_VISIBLE_LINES`）：

```rust
const TEXT_MIN_VISIBLE_LINES: u16 = 5;
const TEXT_MAX_VISIBLE_LINES: u16 = 10;
```

修改 `impl InlineApp for TextPromptState` 中的 `desired_height`：

```rust
fn desired_height(&self) -> u16 {
    let editor = (self.line_count() as u16)
        .clamp(TEXT_MIN_VISIBLE_LINES, TEXT_MAX_VISIBLE_LINES);
    // 1 header + editor + 2 borders + 1 help
    1 + editor + 2 + 1
}
```

效果：

| line_count | 现状 desired_height | 新 desired_height |
|------------|--------------------|-------------------|
| 1          | 5                  | 9                 |
| 5          | 9                  | 9                 |
| 6          | 10                 | 10                |
| 10+        | 14                 | 14                |

- prompt 出现即占 9 行（1 header + 5 编辑行 + 2 边框 + 1 help）；
- 在 5 行以内的换行不改变 `desired_height`，`inline_loop` 中 `height != last_height` 不命中，不调 `terminal.resize`，不发生 ScrollUp；
- 超过 5 行才逐行扩到 14 行——此时是用户主动写长文本，预期合理。

> 注：5 行下限只影响 viewport 高度。textarea 自身依然显示用户实际输入的行数，多余空间在带边框的编辑框内显示为空白，与 dialoguer 的"输入框带固定高度"视觉一致。

### 改动 2：四个 prompt state 过滤非 Press KeyEvent

文件：`src/tui_app/prompt.rs`

在 `TextPromptState::handle_key`、`SelectPromptState::handle_key`、`MultiSelectPromptState::handle_key`、`ConfirmPromptState::handle_key` 各自顶部、紧跟现有 `if self.outcome.is_some()` 早返之后，新增：

```rust
use crossterm::event::KeyEventKind;
// ...
if key.kind != KeyEventKind::Press {
    return;
}
```

`KeyEventKind` 已在 `crossterm::event` 导出，与现有 `KeyCode` / `KeyEvent` / `KeyModifiers` 同模块；引入到 `prompt.rs` 顶部 `use` 列表即可。

> `handle_paste` 不需要改——`Event::Paste` 一次只对应一段字符串，没有 Press/Release 区分。

### 改动 3：单测

文件：`src/tui_app/prompt.rs`（同文件 `mod tests`）

新增以下测试：

1. **`text_desired_height_floor_is_min_visible`**
   - 空 `TextPromptState`，断言 `<TextPromptState as InlineApp>::desired_height(&s) == 9`。
2. **`text_desired_height_unchanged_for_few_newlines`**
   - 调 `s.handle_key(key_mod(KeyCode::Enter, KeyModifiers::ALT))` 三次（line_count 4），断言 `desired_height() == 9`，确认 5 行以内不长高。
3. **`text_desired_height_grows_above_floor`**
   - 通过 6 次 Alt+Enter 让 line_count 达到 7，断言 `desired_height() == 11`（1 + 7 + 2 + 1），证明超阈值开始扩。
4. **`text_release_event_is_ignored`**
   - 构造 `KeyEvent { code: Enter, modifiers: ALT, kind: KeyEventKind::Release, state: KeyEventState::NONE }`，先按 Press 写入 `'a'`，再发 Release Alt+Enter，断言 `s.text() == "a"`、line_count 1。
5. **`select_release_event_is_ignored`** / **`multi_release_event_is_ignored`** / **`confirm_release_event_is_ignored`**
   - 各构造一个会改变状态的 Release 事件（如 Release Down / Release Space / Release y），断言对应状态字段未变。

测试用一个小辅助：

```rust
fn key_release(code: KeyCode, m: KeyModifiers) -> KeyEvent {
    KeyEvent::new_with_kind(code, m, KeyEventKind::Release)
}
```

## 风险与权衡

- **prompt 初次出现占 9 行**：相比之前的 5 行多出 4 行。在 24 行的中等终端里仍占 ~37%，可接受；同 spec 中"非目标"已显式排除"始终 14 行"和"预留 MAX"两种更激进的方案。
- **5 行以上仍会 ScrollUp**：刻意保留。需求是消除"按一次 Alt+Enter 就上滚"的高频发生，而不是消除所有 ScrollUp。
- **多 prompt 序列中前一题的 `✔ ...` 总结仍可能因为 description 启动出现 9 行 viewport 被推出去**：这是 inline 模型的固有取舍，不在本次目标。Alacritty 自带 scrollback，用户可上滚查看。
- **KeyEventKind::Press 过滤**：依赖 crossterm 在所有目标终端的 Press 事件正常下发。crossterm 的默认行为就是 Press（无 kitty 时），开了 kitty 也会下发 Press；过滤只是丢弃额外的 Repeat / Release。

## 影响面

- 文件：仅 `src/tui_app/prompt.rs`。
- 公开 API：无改动。
- 依赖 / Cargo：无改动。
- callsite：无改动。
- 行为变化：仅 `TextPromptState` 的初始 viewport 高度由 5 → 9，其余 prompt 不变。

## 测试 / 验收

1. `cargo test -p zootree --lib tui_app::prompt` 全绿，包含新增 6 条测试。
2. `cargo run -- create` 在 Alacritty 中：
   - title prompt 出现，输入若干字符按 Enter 提交。
   - description prompt 出现，按 Alt+Enter 三次，画面无可见上滚，输入与已写内容均可见。
   - 继续按 Alt+Enter 直至超过 5 行，确认从第 6 行开始 viewport 才逐行长高。
3. 其它 prompt（`zootree info` 的 select、`zootree open` 的 confirm）行为无回归。
