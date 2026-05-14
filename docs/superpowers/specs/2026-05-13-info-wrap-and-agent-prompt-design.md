# `zootree info` 长文本换行 + Agent/Prompt 显示设计

- 日期：2026-05-13
- 作者：weineel + Claude
- 状态：已通过 brainstorming，待 writing-plans

## 背景与目标

`zootree info` 当前在 TUI 模式（`--watch`）下用 `Paragraph::new(lines)` 渲染 title / description，长文本会被横向截断。同时，info 没有展示「实际会传给 agent 的内容」——用户在 overview 面板里需要清楚知道：当前这个 workspace 一旦走 `--run-agent`，agent_cli 会以什么命令、什么 prompt 启动。

本次同时解决两件事：

1. **换行**：TUI 模式下，title、description、新增的 Agent/Prompt 区块都按 pane 宽度自动换行。
2. **新增展示**：在 meta 区块里加一段 `Agent:` 或 `Prompt:`，显示要么是带 `$prompt` 替换后的完整 agent_cli 命令，要么是原始 prompt 文本。

### 非目标（YAGNI）

- 不在 one-shot（无 `--watch`）模式下做显式换行——依赖终端自动 wrap，保持 pipe/脚本输出可预测
- 不引入交互式滚动 / 折叠（meta 区块加长就加长，靠 ratatui 的 Min 约束让 events 区块吃剩余空间）
- 不缓存 GlobalConfig——每次 reload 重读，反映用户对 config.toml 的实时修改
- 不引入 `terminal_size` 之类依赖，textwrap 已能基于 ratatui 的 area.width 完成全部换行需求

## 核心决策

| 决策点 | 选定方案 | 备注 |
|--------|----------|------|
| 新区块语义 | 显示完整 agent_cli 命令（`$prompt` 已替换并 shell 引号化） | 用户在 overview pane 直接看到命令全貌 |
| agent_cli 未配置时 | fallback：显示原始 `build_prompt(workspace)` 结果（title+description） | 让用户感知"如果配置了 agent_cli，会传什么内容" |
| 区块标签 | 配置了 → `Agent:`；未配置 → `Prompt:` | 标签即语义，不堆叠 |
| 换行库 | `textwrap = "0.16"`（无传递依赖） | 比手写 word-wrap 稳，比 ratatui 内置 Wrap 更易计算高度 |
| 应用范围 | TUI 显式换行 + one-shot 仅新增区块（不显式 wrap） | one-shot 靠终端 autowrap |
| GlobalConfig 加载位置 | TUI：`InfoApp::reload()` 内一并加载；one-shot：`handle_info` 加载后传入 `render_once` | 避免无谓的 config 读取 |

## 架构与文件改动

### 依赖

`Cargo.toml` 新增：

```toml
textwrap = "0.16"
```

无需启用任何 feature——默认行为（按 unicode 词边界换行、保留续行缩进）即可。

### 修改文件

| 文件 | 改动 |
|------|------|
| `Cargo.toml` | 新增 `textwrap = "0.16"` |
| `src/core/layout.rs` | 新增 `build_agent_cli_display()` 函数 |
| `src/tui_app/info.rs` | `InfoState` 增加 `agent_cli: Option<String>`；`reload()` 同时加载 `GlobalConfig`；`render_body` 改写为基于 textwrap 的 `Vec<Line>` 渲染，新增 Agent/Prompt 区块 |
| `src/cli/info.rs` | `render_once` 签名增加 `global: &GlobalConfig`；新增 Agent/Prompt 区块；`handle_info` 加载 global 后传入 |
| `tests/info_test.rs` | 集成测试覆盖 Agent/Prompt 两种情况 |

### 不修改

- `src/cli/workspace.rs` 的启动逻辑——info 只是只读展示
- `src/core/layout.rs::build_prompt` / `build_agent_cli_kdl`——已有的 KDL 生成路径不动
- `src/tui_app/mod.rs` 骨架——这次不动 App trait 或事件循环

## 详细设计

### `build_agent_cli_display`（`src/core/layout.rs`）

```rust
/// Resolved display of agent_cli with $prompt substituted.
///
/// - Returns None when agent_cli is not configured.
/// - Returns Some(Err) when agent_cli template fails to parse via shlex.
/// - Returns Some(Ok(line)) where `line` is a single shell-quoted command string
///   suitable for display (multi-line prompts are POSIX-quoted by shlex::try_join).
pub fn build_agent_cli_display(
    agent_cli_tpl: Option<&str>,
    workspace: &WorkspaceConfig,
) -> Option<anyhow::Result<String>> {
    let tpl = agent_cli_tpl?;
    let prompt = build_prompt(workspace);

    let result = (|| -> anyhow::Result<String> {
        let tokens = shlex::split(tpl)
            .ok_or_else(|| anyhow::anyhow!("failed to parse agent_cli: {}", tpl))?;
        if tokens.is_empty() {
            anyhow::bail!("agent_cli is empty");
        }
        let substituted: Vec<String> = tokens
            .into_iter()
            .map(|t| t.replace("$prompt", &prompt))
            .collect();
        let joined = shlex::try_join(substituted.iter().map(|s| s.as_str()))
            .map_err(|e| anyhow::anyhow!("failed to join agent_cli: {}", e))?;
        Ok(joined)
    })();

    Some(result)
}
```

**为什么用 `shlex::try_join`**：
- 多行 prompt 中的换行符会被 POSIX 风格的单引号正确包裹（`'line1\nline2'` 形式）
- 含空格、特殊字符的参数自动加引号
- 输出可直接复制到 shell 执行，无歧义

**注意**：单行 prompt 也会被引号化，可能略显冗余（`claude --dangerously-skip-permissions -- 'Title'`），但保证一致性比省略引号更重要。

### `InfoState` 与 `InfoApp::reload`（`src/tui_app/info.rs`）

```rust
pub(crate) struct InfoState {
    pub status: WorkspaceStatus,
    pub workspace: WorkspaceConfig,
    pub loaded_at: DateTime<Local>,
    pub agent_cli: Option<String>, // 新增：来自 GlobalConfig
}
```

`reload()` 改动：

```rust
pub(crate) fn reload(&mut self) {
    let workspace_result = self.config_mgr.load_workspace(&self.name);
    let global_result = self.config_mgr.load_global();

    match workspace_result {
        Ok((status, workspace)) => {
            let agent_cli = global_result.ok().and_then(|g| g.agent_cli);
            self.state = Some(InfoState {
                status,
                workspace,
                loaded_at: Local::now(),
                agent_cli,
            });
            self.last_error = None;
        }
        Err(e) => {
            self.last_error = Some(format!("{:#}", e));
        }
    }
}
```

GlobalConfig 加载失败不致命——`agent_cli` 退化成 None，info 仍然能渲染（fallback 到 Prompt 区块）。

### `render_body` 重构（`src/tui_app/info.rs`）

核心思路：把 meta 区块的全部内容（title / branch / dir / created / description / agent-or-prompt）预先 wrap 成 `Vec<Line>`，根据实际行数确定 `meta_height`，然后用一个 `Paragraph::new(lines)` 渲染（不需要 ratatui 的 `Wrap` 包裹）。

```rust
fn build_meta_lines(
    ws: &WorkspaceConfig,
    agent_cli: Option<&str>,
    width: u16,
) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();

    // Title 可能很长 → wrap
    let title_label = "Title:";
    push_wrapped_kv(&mut lines, title_label, &ws.title, width);

    // 单行字段
    lines.push(meta_line("Branch:", &ws.branch));
    lines.push(meta_line("Dir:", &ws.workspace_dir));
    lines.push(meta_line("Created:", &format_rfc3339_to_minute(&ws.created_at)));

    // Description 块
    if !ws.description.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from("  Description:"));
        for wrapped in textwrap::wrap(&ws.description, content_width(width)) {
            lines.push(Line::from(format!("    {}", wrapped)));
        }
    }

    // Agent / Prompt 块
    lines.push(Line::from(""));
    let (label, content) = resolve_agent_or_prompt_display(ws, agent_cli);
    lines.push(Line::from(format!("  {}", label)));
    for wrapped in textwrap::wrap(&content, content_width(width)) {
        lines.push(Line::from(format!("    {}", wrapped)));
    }

    lines
}
```

辅助函数：

```rust
/// 给定 area 宽度，返回 description/agent 内容区可用的列宽（扣除 4 列缩进 + 安全边距）。
fn content_width(area_width: u16) -> usize {
    (area_width as usize).saturating_sub(4).max(20)
}

/// 决定 Agent: 还是 Prompt: 区块要展示什么内容。
fn resolve_agent_or_prompt_display(
    ws: &WorkspaceConfig,
    agent_cli: Option<&str>,
) -> (&'static str, String) {
    match crate::core::layout::build_agent_cli_display(agent_cli, ws) {
        Some(Ok(cmd)) => ("Agent:", cmd),
        Some(Err(e)) => ("Agent:", format!("(failed to parse agent_cli: {:#})", e)),
        None => ("Prompt:", crate::core::layout::build_prompt(ws)),
    }
}

/// 把 "Label: long-text" 这种 key-value 行 wrap 成多行，续行只缩进不重复 label。
fn push_wrapped_kv(
    lines: &mut Vec<Line<'static>>,
    label: &str,
    value: &str,
    area_width: u16,
) {
    let wrapped = textwrap::wrap(value, content_width(area_width));
    if wrapped.is_empty() {
        lines.push(meta_line(label, ""));
        return;
    }
    lines.push(meta_line(label, &wrapped[0]));
    for cont in &wrapped[1..] {
        // 续行：跳过 "  Label:    " 的对齐空间，只输出值
        lines.push(Line::from(format!("            {}", cont))); // 12 列缩进对齐到值列
    }
}
```

`render_body` 主体：

```rust
fn render_body(&self, frame: &mut Frame, area: Rect) {
    let Some(state) = &self.state else { /* 同原代码 */ };

    let ws = &state.workspace;
    let meta_lines = build_meta_lines(ws, state.agent_cli.as_deref(), area.width);
    let meta_height = meta_lines.len() as u16;

    // 给 meta 区块设上限：不能吃掉整个 body，至少给 repos+events 留 6 行
    let max_meta = area.height.saturating_sub(6);
    let actual_meta = meta_height.min(max_meta);

    let chunks = Layout::vertical([
        Constraint::Length(actual_meta),
        Constraint::Length(2 + ws.repos.len().max(1) as u16),
        Constraint::Min(1),
    ])
    .split(area);

    frame.render_widget(Paragraph::new(meta_lines), chunks[0]);
    // repos & events 渲染同原代码
}
```

**为什么给 meta 加上限**：极端情况下 description 或 agent 命令可能撑爆整个 body，repos / events 完全看不到。给 6 行兜底（events 至少 1 行 + repos 表头 + 一两条数据），meta 超出部分会被 ratatui 自动截断。

**为什么不让 meta 滚动**：YAGNI——这次不引入滚动状态机。如果 meta 撑爆，用户能从 Description 推断 prompt 内容，不至于丢关键信息。后续如有需求可在独立迭代里加 PageUp/PageDown。

### `render_once` 改动（`src/cli/info.rs`）

```rust
pub fn render_once(
    status: &WorkspaceStatus,
    ws: &WorkspaceConfig,
    global: &GlobalConfig,
) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "Workspace: {} ({})", ws.title, ws.name);
    let _ = writeln!(out, "Status:    {}", status_label(status));
    let _ = writeln!(out, "Branch:    {}", ws.branch);
    let _ = writeln!(out, "Dir:       {}", ws.workspace_dir);
    let _ = writeln!(out, "Created:   {}", format_rfc3339_to_minute(&ws.created_at));

    if !ws.description.is_empty() {
        let _ = writeln!(out);
        let _ = writeln!(out, "Description:");
        for l in ws.description.lines() {
            let _ = writeln!(out, "  {}", l);
        }
    }

    // 新增 Agent/Prompt 区块
    let _ = writeln!(out);
    match crate::core::layout::build_agent_cli_display(global.agent_cli.as_deref(), ws) {
        Some(Ok(cmd)) => {
            let _ = writeln!(out, "Agent:");
            let _ = writeln!(out, "  {}", cmd);
        }
        Some(Err(e)) => {
            let _ = writeln!(out, "Agent:");
            let _ = writeln!(out, "  (failed to parse agent_cli: {:#})", e);
        }
        None => {
            let _ = writeln!(out, "Prompt:");
            for l in crate::core::layout::build_prompt(ws).lines() {
                let _ = writeln!(out, "  {}", l);
            }
        }
    }

    // repos / events 渲染同原代码
    out
}
```

`handle_info` 改动：

```rust
let global = config_mgr.load_global().unwrap_or_default();
// ...
print!("{}", render_once(&status, &workspace, &global));
```

`load_global()` 失败时退化到 `GlobalConfig::default()`（agent_cli 为 None），保证 info 在配置文件出问题时仍可用。

## 测试计划

### 单元测试

新增到 `src/core/layout.rs#tests` 或 `tests/layout_test.rs`：

1. `build_agent_cli_display_returns_none_when_unset`：`agent_cli_tpl = None` → `None`
2. `build_agent_cli_display_substitutes_prompt`：模板 `claude --dangerously-skip-permissions -- $prompt` + workspace title="Hello" → 包含 `'Hello'` 的字符串
3. `build_agent_cli_display_handles_multiline_prompt`：description 含 `\n` → 输出中的 prompt 用 POSIX 单引号包裹保留换行
4. `build_agent_cli_display_returns_err_on_invalid_template`：模板 `"unclosed quote` → `Some(Err)`

新增到 `src/tui_app/info.rs#tests`：

5. `render_includes_agent_section_when_configured`：在 ConfigManager 中保存带 `agent_cli` 的 GlobalConfig，render_to_string 后断言包含 `Agent:` 与命令片段
6. `render_includes_prompt_section_when_not_configured`：GlobalConfig 默认（agent_cli=None），断言包含 `Prompt:` 与原始 title 文本
7. `render_wraps_long_title`：title="A".repeat(200)，width=40 → 输出包含多行 title 片段
8. `render_wraps_long_description`：同上，对 description
9. `tick_reloads_agent_cli_changes`：保存 GlobalConfig（agent_cli=None）→ render → 改写 GlobalConfig（agent_cli=Some(...)）→ Tick → 再 render，断言新内容出现

新增到 `tests/info_test.rs`（如该文件存在；否则集成进 `src/cli/info.rs#tests`）：

10. `render_once_includes_agent_section_when_configured`
11. `render_once_includes_prompt_section_when_not_configured`

### 边界场景

- 空 title / 空 description：渲染不 panic，Prompt 区块依然显示（即使内容为空字符串）
- 极窄宽度（width=20）：`content_width` 兜底返回 20，textwrap 不会无限循环
- meta 撑爆 body：`actual_meta = max_meta`，repos/events 至少有 6 行可见

## 风险与权衡

| 风险 | 缓解 |
|------|------|
| textwrap 对 CJK（中文）按字符宽度处理可能不理想 | textwrap 默认用 unicode-width，CJK 算 2 列，行为和 helix/zellij 一致；如有特殊需求可在 writing-plans 阶段评估 `unicode-linebreak` feature |
| shlex::try_join 对单引号内容会用 `'\''` 嵌套引号，多行 prompt 显示略繁琐 | 这是 POSIX 标准行为，复制即可执行；为可读性损失换准确性是合理交换 |
| GlobalConfig 加载失败时静默 fallback 可能掩盖配置错误 | one-shot 用 `unwrap_or_default()`，TUI 用 `.ok().and_then(...)`——错误不阻塞 info 显示，但用户用 `zootree start` 时 GlobalConfig 加载失败仍会 bail，问题不会被永久掩盖 |
| meta 区块撑爆 body 时 repos/events 不可见 | 给 meta 加 `max_meta = body_height - 6` 上限，至少留 6 行给下方区块；后续如需可加滚动 |

## 待 writing-plans 阶段细化

- `textwrap = "0.16"` 锁定到 minor 版本即可，patch 自由升级
- `content_width` 的安全下限（当前 20 列）是否需要从配置可调（默认应足够）
- meta `max_meta` 的兜底行数（当前 6）是否需要根据 repos 数量动态计算
- 是否给 `render_once` 的 Agent/Prompt 区块前面那条空行加一个统一的分隔线（`---` 或类似）以便机器解析——若需，下一迭代再做
