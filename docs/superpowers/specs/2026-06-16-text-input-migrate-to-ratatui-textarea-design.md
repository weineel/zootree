# 文本输入框迁移到 ratatui-textarea 设计

- 日期：2026-06-16
- 作者：weineel + Claude
- 状态：已通过 brainstorming + probe 实测；范围已扩大到包含 ratatui 0.29→0.30 升级
- 修订历史：
  - 2026-06-16 初版：仅替换 textarea 库
  - 2026-06-16 修订 v2：实施时发现 `ratatui-textarea 0.8+` 已切到 ratatui 0.30 拆包架构，
    无法在 ratatui 0.29 项目中孤立使用。Probe 确认 ratatui 0.30 在本项目造成 8 处独立
    破坏（mod.rs 的 `Backend::Error` 不再默认 `Send + Sync`）。范围扩大为"升级
    ratatui 到 0.30 + 升级 crossterm 到 0.29 + 迁库到 ratatui-textarea 0.9"，合并为
    单个 PR。

## 背景与目标

当前 inline prompt 基于 `tui-textarea` 0.7（rhysd/tui-textarea，已停止更新）。`src/tui_app/prompt.rs` 里 `TextPromptState::handle_key` 手写了一张很窄的按键白名单：
方向键 / Home / End / Backspace / Enter / 普通字符。

由此暴露三个痛点：

1. **缺 Emacs 风格快捷键**——Ctrl+A/E/W/U/K、Alt+B/F/D、Ctrl+Z/R 等编辑/移动键全部缺失，与 shell、Claude Code、Codex 等输入框习惯不一致。
2. **长行不 soft-wrap**——一行超过可视宽度时光标跑出视区，看不到全文。
3. **多行粘贴处理不理想**——粘贴依赖 `insert_str`，但因为没有 soft-wrap，长行粘进来观感差；后续若要支持视觉行 Up/Down 也无从下手。

本次目标：通过**替换底层 textarea 库**，一次解决以上三个痛点，且对项目其他部分零影响。

### 选型结论

替换为 [`ratatui-textarea`](https://crates.io/crates/ratatui-textarea) 0.9（由 ratatui 官方组织 + orhun 维护，141k 总下载、128k/90d 下载、53 个反向依赖、2026-06-12 仍在更新）。

**前置：必须先升级 ratatui 0.29→0.30 + crossterm 0.28→0.29。** `ratatui-textarea 0.8+` 和 `tui-textarea-2 0.7.1+`（截至 2026-06-16 唯二活跃的 fork）都已切到 ratatui 0.30 的拆包架构（`ratatui-core` + `ratatui-widgets`），不再支持 monolithic 0.29。Probe 测得这次升级在本项目的破坏面是 8 处独立错误 + crossterm 必须同步到 0.29 以避免双份。

候选对比（截至 2026-06-16）：

| 候选 | 维护状态 | 三个痛点覆盖 | 备注 |
|---|---|---|---|
| **ratatui-textarea** (选) | ratatui org 维护，128k/90d 下载，53 reverse-deps | ✅ Emacs 键 + soft-wrap + undo | 事实标准 |
| tui-textarea-2 (srothgan) | 个人 fork，44k/90d 下载 | ✅ 同上，多了 `set_max_rows` auto-size 糖 | 维护单点风险更高 |
| tui-textarea 0.7（现状） | 上游停滞 | ❌ soft-wrap 缺失，emacs 键不全 | 不解决核心问题 |
| edtui | 活跃 | ✅ 但默认 vim 模式 | 对 prompt 场景过度设计 |
| rat-text | 活跃 | ✅ | 绑定整套 rat- 生态，依赖过重 |
| 自研（参考 codex-rs） | — | ✅ | ~1000 行编辑器代码，长期维护成本高 |

两个 fork（`tui-textarea-2` vs `ratatui-textarea`）功能几乎一致——都从 rhysd/tui-textarea 派生，都实现了 emacs 键 + soft-wrap + undo/redo。选 `ratatui-textarea` 的核心理由是**生态归属**：它在 ratatui 官方 org 下、下载量是另一个的 ~3x、反向依赖数 53、维护团队就是 ratatui 核心维护者，长期生命力更有保障。

### 非目标（YAGNI）

- **不**新增历史记录（↑/↓ 回放上次输入）。
- **不**新增斜杠命令 / `@file` 自动补全弹层。
- **不**改动 `tui_app/mod.rs` 的事件循环或 `Viewport::Inline` 框架。
- **不**改动 `tui_app/info.rs`。
- **不**改 `SelectPromptState` / `MultiSelectPromptState` / `ConfirmPromptState` 的交互行为——它们里面用 `TextArea` 仅作单行 fuzzy filter 缓冲，迁库后代码层面零变化。
- **不**做"按可视行自适应高度"——`line_count()` 维持逻辑行口径。

## 核心决策

### 决策 1：用 ratatui-textarea 替换 tui-textarea

详见上节选型结论。

### 决策 2：TextPromptState::handle_key 从"白名单"改成"语义键拦截 + 委托 input()"

旧：每个按键都要在 `prompt.rs` 里显式声明动作。
新：先拦截 **zootree 自己定义的语义键**（Ctrl+C 中断、Esc 取消/跳过、Alt/Shift+Enter 换行、裸 Enter 提交），其余全部交给 `textarea.input(key)`，由库统一处理 Backspace / 方向键 / Home/End / Ctrl+A/E/W/U/K / Alt+B/F/D / undo/redo 等。

这是迁移收益的核心——之所以选 ratatui-textarea，就是为了把这张表删掉。

### 决策 3：仅在 `TextPromptState` 打开 soft-wrap

通过 `textarea.set_wrap_mode(WrapMode::WordOrGlyph)` 启用。仅作用于多行文本输入。

`SelectPromptState` 内部 filter 是单行输入——**不**开 soft-wrap。

### 决策 4：高度计算仍按逻辑行

`TextPromptState::line_count()` 返回 `textarea.lines().len()`（逻辑行数），不切换到 measured visual rows。`TEXT_MIN_VISIBLE_LINES` / `TEXT_MAX_VISIBLE_LINES` 维持不变。这保证 inline viewport 高度行为跟当前一致；soft-wrap 仅影响显示，不影响布局算法。

### 决策 5：粘贴路径不变

`tui_app/mod.rs` 已用 `EnableBracketedPaste` + `Event::Paste(String)`；到达 `TextPromptState::handle_paste` 就是一段 `&str`，继续 `textarea.insert_str(s)`。新库的 `insert_str` 跨 `\n` 拆行行为一致。

## 实施范围

### 改动的文件

| 文件 | 改动 |
|---|---|
| `Cargo.toml` | `ratatui = "0.29"` → `"0.30"`；`crossterm = "0.28"` → `"0.29"`；`tui-textarea = "0.7"` → `ratatui-textarea = "0.9"` |
| `src/tui_app/mod.rs` | 给 `main_loop<B>` / `inline_loop<B>` 加 `where B::Error: Send + Sync + 'static`（让 `?` 转 anyhow 生效）。Probe 已定位 4 处 `?` 调用点：第 79、229、231、234 行 |
| `src/tui_app/prompt.rs` | use 改名；`TextPromptState::new` 加 `set_wrap_mode`；`TextPromptState::handle_key` 重写；新增 4 条 emacs 键回归测试 |
| `Cargo.lock` | cargo 自动更新 |

### 不改的文件

- `src/tui_app/info.rs`：probe 显示 0.30 不破坏此文件。`TestBackend` 的 `error type` 从 `io::Error` 变 `Infallible`，但 `Terminal::new(backend).unwrap()` 仍能调用（在 `Infallible` 上 `unwrap()` 是 stable trait 方法）。运行时通过 `cargo test` 验证。
- `src/tui.rs`：5 个公开函数签名不变。
- `tests/*.rs`：黑盒，行为不变。

## 实施大纲（writing-plans 阶段细化）

1. **依赖切换**：编辑 `Cargo.toml`，`cargo update -p tui-textarea --precise <new>` 或直接换 crate；`cargo check` 通过。
2. **import 改名**：`prompt.rs` 里所有 `tui_textarea::` 改成 `ratatui_textarea::`；新增 `WrapMode` 导入。
3. **`TextPromptState::new` 开 soft-wrap**：插入 `textarea.set_wrap_mode(WrapMode::WordOrGlyph)`。
4. **`TextPromptState::handle_key` 重写**：保留 Ctrl+C / Esc / Alt|Shift+Enter / Enter 四条拦截分支；其余全部 `self.textarea.input(key);`。
5. **跑测试，修断言**：`cargo test`；逐一审视 `prompt.rs` 内 `tests` 模块，调整任何依赖"旧默认按键无效"的断言。
6. **加 2 条回归测试**：
   - `Ctrl+A` 应将光标移到行首（验证库的 emacs 键生效）。
   - `Ctrl+W` 应删除前一个词（同上，另一类操作）。
7. **手动验证**：本地跑 `zootree create` 或任何走 `TextPromptState` 的命令，确认：
   - 长行 soft-wrap 正常显示
   - Ctrl+A/E/W/U/K、Alt+B/F 工作
   - Enter 提交、Alt+Enter 换行、Esc 取消、Ctrl+C 中断行为不变
   - CJK 输入与删除仍正常

## 风险与对策

| 风险 | 概率 | 对策 |
|---|---|---|
| `ratatui-textarea` 的 `input()` 默认把 Enter 当 newline，若忘了拦截则 Enter 提交失效 | 已识别 | `handle_key` 显式拦截 Enter；加 "Enter submits" 测试 |
| Soft-wrap 让用户感觉高度计算"不直觉" | 低 | `line_count()` 仍按逻辑行；用户感受到的是"显示折行"，跟主流编辑器一致 |
| 测试快照断言失败 | 中 | 实施步骤 5 留出修复时间，影响面 2–3 处 |
| 库的某个 emacs 键覆盖了用户期望的输入字符 | 低 | 默认表只占 Ctrl/Alt 组合键，普通字符 `Char(_)` 不冲突；若出现可换 `input_without_shortcuts` 退路 |

## 工作量估算

- 代码改动：~30 行（10 删、20 增/调）
- 测试改动：2–3 处断言更新 + 2 条新增
- 验证：1 次 `cargo test` + 5 分钟手动验证
- 总计：单次 PR，半小时内可完成
