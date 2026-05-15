# TUI 交互优化设计（替换 dialoguer）

- 日期：2026-05-13
- 作者：weineel + Claude
- 状态：已通过 brainstorming，待 writing-plans

## 背景与目标

`zootree create` / `start` / `done` / `cancel` / `open` / `prune` / `info` / `repo edit` / `repo remove` 这些命令在不传参数时进入交互式 TUI，目前由 `src/tui.rs`（`dialoguer` 0.11 的薄壳）承载。两个突出问题：

1. **CJK 输入 bug**：`dialoguer::Input` 用基于字节的编辑逻辑，中文输入后 Backspace 会把多字节字符切坏，画面与底层字符串错位。
2. **不支持多行**：`description` 等字段实际上常常需要多行内容，但 `Input` 只能单行，Enter 直接提交。

本次目标：

1. 删掉 `dialoguer`，自建一套基于 `ratatui` + `crossterm` + `tui-textarea` 的 inline prompt（`Viewport::Inline`），覆盖 `Input` / `Select` / `MultiSelect` / `Confirm` 四种交互。
2. **CJK 正常**（删除按字符、宽度计算正确），**全部文本字段支持多行**（Alt+Enter / Shift+Enter 换行，Enter 提交）。
3. **保持 `src/tui.rs` 5 个公开函数签名不变**，9 处 callsite 零改动。
4. Select / MultiSelect **始终带过滤框**（fuzzy 高亮）。
5. **Esc 语义**：选填字段跳过取空值，必填字段中止整条命令；Ctrl+C 一律硬中止；中止时 stderr 输出 `aborted`、退出码 1。

### 非目标（YAGNI）

- 不为 `create` 单独做"全屏多字段表单页"（仍是顺序的 inline prompt）。
- 不做输入历史 / Tab 补全。
- 不做 dialoguer 当前没有的额外编辑能力（vim 模式、宏、selection 持久化等）。
- 不做 snapshot 渲染测试（项目目前没基础设施）。
- Windows 多行体验只保证"粘贴"路径可用，不投入额外适配。

## 核心决策

| 决策点 | 选定方案 | 备注 |
|--------|----------|------|
| 总体方案 | 在 `src/tui_app/` 下新增 `prompt.rs`，`src/tui.rs` 内部改为调用它 | callsite 零改动；与现有 `tui_app::run_app`（alternate screen）互不冲突 |
| 渲染模式 | `Viewport::Inline`，动态高度 | 不进 alternate screen，体感像 dialoguer，退出后历史区干净 |
| 多行编辑器 | 引入 `tui-textarea` crate | 原生支持 unicode-width / CJK 删除 / Alt+Enter / 选区 |
| 多行触发键 | Alt+Enter（主推）/ Shift+Enter（终端支持时） | Shift+Enter 依赖 Kitty keyboard protocol，不强求 |
| 提交键 | Enter | 单行多行都一样 |
| Select 过滤 | **始终显示过滤框**，模糊匹配 + 命中字符高亮 | 列表短时也保留，换取交互一致 |
| 取消语义 | Esc：选填→Skipped、必填→Aborted；Ctrl+C：永远 Interrupted | Skipped 走默认值，Aborted/Interrupted → main 退出码 1 |
| 退出码 | 中止 = 1 | 与项目其它错误一致（不用 130） |
| Scrollback 总结 | `terminal.insert_before` 写一行 `✔ {prompt}: {value}` | 命令结束后历史区有迹可循；多行内容只保留首行 + ` …` |
| 粘贴 | 启用 crossterm bracketed paste，整段插入 | 否则粘贴到 description 中第一个换行就被解释为提交 |

## 架构与文件改动

### 依赖（`Cargo.toml`）

- 移除：`dialoguer = "0.11"`
- 新增：`tui-textarea = "0.7"`
- 新增：`unicode-width = "0.2"`（Select / Confirm 自渲染时计算列宽）
- 可选新增：`fuzzy-matcher = "0.3"`（Select 过滤匹配；如果想避免新依赖也可改用纯子串匹配，决定权交给 implementation 阶段，初版用 `fuzzy-matcher`）

### 模块结构

```
src/
  tui.rs                   # 保持公开签名，内部改调 tui_app::prompt::*
  tui_app/
    mod.rs                 # 新增 run_inline 辅助 + InlineApp trait + bracketed paste 启停
    info.rs                # 不动
    prompt.rs              # 新增：TextPromptState / SelectPromptState /
                           #       MultiSelectPromptState / ConfirmPromptState
                           # 以及对应的 run() 入口
```

`prompt.rs` 单文件起步；如果实现完成后超过 ~500 行再拆 `prompt/{text,select,multi,confirm}.rs` 子模块。

### 运行模式：`run_inline` + `InlineApp`

新 trait（与现有 `App` 平行，**不复用**）：

```rust
pub trait InlineApp {
    type Output;
    fn on_event(&mut self, e: Event) -> anyhow::Result<()>;
    fn render(&mut self, frame: &mut Frame);
    fn desired_height(&self) -> u16;
    fn poll(&mut self) -> Option<PromptOutcome<Self::Output>>;
}

pub enum PromptOutcome<T> {
    Submitted(T),
    Skipped,         // Esc on optional prompt
    Aborted,         // Esc on required prompt
    Interrupted,     // Ctrl+C
}
```

`run_inline<A: InlineApp>(app, initial_height) -> anyhow::Result<PromptOutcome<A::Output>>`：

1. `enable_raw_mode()`
2. 尝试 `PushKeyboardEnhancementFlags(DISAMBIGUATE_ESCAPE_CODES | REPORT_ALTERNATE_KEYS)`，失败静默吞错（终端不支持 Kitty protocol）。
3. `execute!(stdout, EnableBracketedPaste)`。
4. `Terminal::with_options(TerminalOptions { viewport: Viewport::Inline(initial_height) })`。
5. 主循环：每次循环 `terminal.draw`、`event::read`，按 `desired_height()` 变化时 `terminal.resize`。`Event::Paste(s)` 事件单独分发到 `on_event`。
6. `poll()` 返回 `Some(_)` 时跳出循环。
7. 退出前：`terminal.insert_before(1, |buf| 写总结行)` → `terminal.clear()` → `disable_raw_mode()` → `PopKeyboardEnhancementFlags` → `DisableBracketedPaste`。
8. panic hook 复用 `install_panic_hook`，但需扩展：同时 pop enhancement flags 和 bracketed paste，否则 panic 后终端会卡在奇怪状态。

为什么不复用现有 `App` trait：现有 `run_app` 走 alternate screen，`render` 没有动态高度概念，事件循环假设全屏。共用一个 trait 会让 `App` 同时背负两套语义。两 trait 并列、共享 `Event` 类型即可。

### `src/tui.rs` 包装层

签名一律不变：

```rust
pub fn input_required(prompt: &str) -> Result<String>
pub fn input_optional(prompt: &str) -> Result<Option<String>>
pub fn select_one(prompt: &str, items: &[String]) -> Result<usize>
pub fn select_multi(prompt: &str, items: &[String]) -> Result<Vec<usize>>
pub fn confirm(prompt: &str, default: bool) -> Result<bool>
```

实现方式：构造对应的 `*PromptState`，调 `run_inline`，把 `PromptOutcome` 映射回 `Result`：

```rust
pub fn input_required(prompt: &str) -> Result<String> {
    match run_inline(TextPromptState::new(prompt).required(), 3)? {
        PromptOutcome::Submitted(s) => Ok(s),
        PromptOutcome::Aborted | PromptOutcome::Interrupted => Err(PromptError::Cancelled.into()),
        PromptOutcome::Skipped => unreachable!(),
    }
}

pub fn input_optional(prompt: &str) -> Result<Option<String>> {
    match run_inline(TextPromptState::new(prompt).optional(), 3)? {
        PromptOutcome::Submitted(s) if s.is_empty() => Ok(None),
        PromptOutcome::Submitted(s) => Ok(Some(s)),
        PromptOutcome::Skipped => Ok(None),
        PromptOutcome::Aborted | PromptOutcome::Interrupted => Err(PromptError::Cancelled.into()),
    }
}
// select_one / select_multi / confirm 同样：成功 → Ok；
// Aborted / Interrupted → Err(PromptError::Cancelled)。
```

### `PromptError::Cancelled` 在 main 里的处理

`src/main.rs` 入口（或现有 cli error 入口）增加一段：命令链返回的 `anyhow::Error` 如果 `downcast_ref::<PromptError>()` 等于 `Cancelled`，stderr 输出 `aborted` 并以退出码 `1` 退出，不打印 backtrace。其他错误维持现状。

callsite 完全不动：用户按 Esc/Ctrl+C，`?` 一路冒泡，main 优雅退出。

## 四种 Prompt 行为

### `TextPrompt`（替代 `Input`）

- 内部 `tui_textarea::TextArea`。**不调用** `TextArea::input(KeyEvent)`（默认 keymap 包含 Ctrl+H/Ctrl+M/Ctrl+D 等，会与提交 / 中断冲突），改为显式分发：
  ```text
  Char(c)               -> textarea.insert_char(c)
  Backspace             -> textarea.delete_char()
  Enter (alt|shift)     -> textarea.insert_newline()
  Enter                 -> Submitted(text())
  Esc                   -> required ? Aborted : Skipped
  Ctrl+C                -> Interrupted
  ←/→/Home/End/↑/↓      -> 光标移动（沿用 textarea 内置 cursor api）
  Paste(s)              -> textarea.insert_str(s)
  ```
- 渲染：1 行 prompt 文案（`> Title`，optional 时右侧追加灰色 `(optional)`）+ N 行编辑区（带边框，初始 1 行，按内容长高，**最多 10 行**后内部滚动）+ 1 行帮助 `enter submit · alt+enter newline · esc {skip|cancel}`。
- `desired_height = 1 + min(content_lines, 10) + 1`。
- 必填 vs 选填：`required()` / `optional()` builder 切换，仅影响 Esc 语义和帮助行的文案（`cancel` vs `skip`）。
- Scrollback 总结：`✔ Title: foo`（多行内容只保留首行 + ` …`）。

### `SelectPrompt`（替代 `Select`，**始终带过滤框**）

- 顶部 `> {prompt}` + 下方一行过滤输入框（用同一个 `TextArea` 单行模式承载，复用 CJK 处理）。
- 列表区高度固定 `min(items.len(), 8)` 行，超过滚动；过滤后展示**模糊匹配命中**的项（`fuzzy-matcher`），命中字符 `Color::Yellow` 高亮。
- 键位：
  ```text
  ↑/Ctrl+P              -> 上移光标（回绕）
  ↓/Ctrl+N              -> 下移光标（回绕）
  Enter                 -> 列表非空时 Submitted(原始索引)
  Esc                   -> 必填 Aborted（当前所有 select_one 都是必填，所以实际就是 Aborted）
  Ctrl+C                -> Interrupted
  其它 char/backspace   -> 进入过滤输入
  ```
- `desired_height = 1 + 1 + 8 + 1 = 11`（顶 + 过滤 + 列表 + 帮助）。
- Scrollback 总结：`✔ {prompt}: {选中项文本}`。

### `MultiSelectPrompt`（替代 `MultiSelect`）

- 与 `SelectPrompt` 同布局，前面多 `[x]` / `[ ]` 标记。
- 键位增量：
  ```text
  Space                 -> 切换当前项的勾选
  a                     -> 全选 / 全不选切换
  Enter                 -> Submitted(选中索引数组，按选中顺序)
  ```
- 与 `dialoguer::MultiSelect` 当前行为一致：返回的索引顺序是用户**勾选的先后顺序**，不是原数组顺序（保留这一行为以避免影响 callsite）。
- Scrollback 总结：`✔ {prompt}: 3 selected`（数量；列出全部名字会太长）。

### `ConfirmPrompt`（替代 `Confirm`）

- 单行 `{prompt} [Y/n]` 或 `[y/N]`（取决于 default）。
- 键位：
  ```text
  y/Y                   -> Submitted(true)
  n/N                   -> Submitted(false)
  Enter                 -> Submitted(default)
  Esc                   -> Aborted
  Ctrl+C                -> Interrupted
  ```
- `desired_height = 1`。
- Scrollback 总结：`✔ {prompt}: yes` / `no`。

### 共用细节

- `NO_COLOR` 环境变量下退化为无色（不加颜色样式）。
- 选中行 `Style::reversed()`，命中字符 `Color::Yellow`。
- 非 TTY 环境（`!io::stdout().is_terminal()`）：所有 prompt 立刻返回 `Err(...)`，错误信息 `interactive prompt requires a TTY`。当前 9 处 callsite 都已经在确实需要交互的分支里，不会被误触。
- 空集合：`select_one` / `select_multi` 入口前的 `if items.is_empty() { bail!() }` 由调用方（`workspace.rs` / `prune.rs`）保留，prompt 不处理空集。

## Esc 语义对每个 callsite 的影响

| 文件:行 | 调用 | 当前行为 | 新行为 |
|---|---|---|---|
| workspace.rs:63 | `input_required("Title")` | 必须输入 | Esc → 命令终止 |
| workspace.rs:68 | `input_optional("Description ...")` | 可空 | Esc → 描述为空，继续 |
| workspace.rs:102 | `select_multi("Select repos")` | 必须，空选会 bail | Esc → 命令终止（早于原 bail） |
| workspace.rs:119 | `input_optional("Target branch ...")` | 可空 | **Esc → 用 current 分支默认值，继续** |
| workspace.rs:214 / 395 / 658 / 852 | `select_one("Select workspace ...")` | 必须 | Esc → 命令终止 |
| workspace.rs:870 | `confirm(...)` | 必须 y/n | Esc → 命令终止（按 n 仍走"不删"分支，保持原语义） |
| info.rs:47 | `select_one("Select workspace")` | 必须 | Esc → 命令终止 |
| prune.rs:37 | `select_multi("Select workspaces to prune")` | 必须 | Esc → 命令终止 |
| repo.rs:100 / 117 | `select_one("Select repo to ...")` | 必须 | Esc → 命令终止 |

所有改动都退化为 `aborted` + 退出码 1，不会执行半截命令。

## 测试策略

把 prompt 拆成两层：

- **纯逻辑层**：`*PromptState`，只接收 `KeyEvent`（含 modifiers），输出 `PromptOutcome` 和当前可渲染快照。**不碰终端**。
- **运行层**：`run_inline`，负责 crossterm IO + ratatui 渲染。

单元测试覆盖纯逻辑层（放 `prompt.rs` 内 `#[cfg(test)] mod tests`）：

| 测试 | 验证 |
|---|---|
| `text_cjk_backspace` | `"你好"` 后 Backspace → 剩 `"你"`（按字符，不是按字节） |
| `text_alt_enter_inserts_newline` | Alt+Enter 后内容含 `'\n'` |
| `text_shift_enter_inserts_newline` | Shift+Enter 同上 |
| `text_enter_submits` | Enter → `Submitted` |
| `text_optional_esc_skipped` | optional 模式 Esc → `Skipped` |
| `text_required_esc_aborted` | required 模式 Esc → `Aborted` |
| `text_ctrl_c_interrupted` | 任何模式 Ctrl+C → `Interrupted` |
| `text_paste_inserts_multiline` | `Event::Paste("a\nb")` 后内容是 `"a\nb"`，不会因换行触发提交 |
| `select_filter_cjk` | 过滤词 `"前"` 过滤包含 `"前端"` 的项 |
| `select_arrow_navigation` | ↑↓ 移动光标，越界回绕 |
| `select_empty_filter_disables_enter` | 过滤后无命中时 Enter 无效 |
| `multi_space_toggles` | Space 切换标记，再次切回 |
| `multi_select_all` | `a` 全选 / 全不选 |
| `multi_returns_indices_in_selection_order` | 返回顺序与用户勾选顺序一致 |
| `confirm_default_on_enter` | default=true → Enter 返回 true |
| `confirm_keys` | y/Y/n/N 各自返回正确值 |

人工 smoke 测试在 PR 描述里列出：

- macOS Terminal.app + iTerm2 跑 `zootree create`，输入 `"项目 标题"` 后 Backspace 删，确认中文按字符删。
- 在 description 里 Alt+Enter 插入换行，submit，落盘 yaml 中 description 含 `\n`。
- 把含换行的文本粘贴到 description，确认整段插入而不是中途提交。
- `zootree start`（多个 pending workspace）在过滤框输入子串确认列表收敛。
- Ctrl+C 在任意 prompt 中按下，命令退出码 = 1，stderr 是 `aborted`。

## 边界 / 风险

1. **Shift+Enter 兼容性**：依赖 Kitty keyboard protocol（Kitty / WezTerm / Alacritty 新版 / Ghostty / 新版 iTerm2 支持；Terminal.app / tmux 默认 / 旧 iTerm2 不支持）。文档主推 Alt+Enter，help 行只写 `alt+enter newline`，Shift+Enter 是锦上添花。Push flags 失败静默吞错。
2. **`tui-textarea` 默认 keymap 冲突**：不调 `TextArea::input(KeyEvent)`，改自己分发到 `insert_char` / `delete_char` / `insert_newline` / `insert_str`，避免 Ctrl+H、Ctrl+M、Ctrl+D 等默认绑定干扰。
3. **viewport 高度变化**：多行 `TextPrompt` 长高时会把 viewport 下边界往下推（ratatui 标准行为，类似 fzf），上方已写入 scrollback 的内容不会回到 viewport，符合预期。
4. **Scrollback 总结失败的降级**：如果 `terminal.insert_before` 出错，退出后改为 `eprintln!` 写一行；不让总结行的失败影响主退出路径。
5. **粘贴必须开 bracketed paste**：否则粘贴到 description 中第一个 `\n` 会被解释为提交，是用户最容易踩的坑。这是**必做**项。
6. **删除 `dialoguer` 间接依赖**：`dialoguer` 拉了 `console`、`zeroize` 等。删除前 grep / `cargo tree` 确认这些 crate 没被项目其它代码直接 `use`。当前 grep 没看到引用，安全。
7. **Windows**：crossterm + ratatui 在 Windows console 工作，但 Kitty enhancement flags 完全不支持，Alt+Enter 也常被系统拦截。Windows 的多行只保证粘贴路径可用，文档写明。
8. **panic 路径**：现有 `install_panic_hook` 只 `disable_raw_mode + LeaveAlternateScreen`。需扩展为同时 `PopKeyboardEnhancementFlags` + `DisableBracketedPaste`，否则 panic 后终端会卡在奇怪状态。

## 开放问题

无（所有关键决策已锁定）。
