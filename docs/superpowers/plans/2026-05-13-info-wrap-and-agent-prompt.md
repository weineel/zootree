# `zootree info` 长文本换行 + Agent/Prompt 显示 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 `zootree info` 中按 pane 宽度换行长 title/description，并新增 `Agent:` / `Prompt:` 区块展示替换 `$prompt` 后的 agent_cli 命令（未配置时 fallback 到原始 prompt）。

**Architecture:** 在 `core/layout.rs` 新增 `build_agent_cli_display()` 作为唯一的展示串生成入口；TUI (`tui_app/info.rs`) 与 one-shot (`cli/info.rs`) 共享该函数。TUI 用 `textwrap` 预换行成 `Vec<Line>`，one-shot 依赖终端 autowrap。

**Tech Stack:** Rust，`textwrap = "0.16"`（新增），`shlex`（已有），`ratatui` / `crossterm`（已有）

**Spec:** `docs/superpowers/specs/2026-05-13-info-wrap-and-agent-prompt-design.md`

---

## File Structure

| 文件 | 类型 | 职责 |
|------|------|------|
| `Cargo.toml` | 修改 | 新增 `textwrap = "0.16"` 依赖 |
| `src/core/layout.rs` | 修改 | 新增 `build_agent_cli_display()` 函数 |
| `tests/agent_cli_test.rs` | 修改 | 新增 `build_agent_cli_display` 单元测试 |
| `src/cli/info.rs` | 修改 | `render_once` 签名增加 `global: &GlobalConfig`；新增 Agent/Prompt 区块；`handle_info` 加载并传入 global |
| `tests/info_test.rs` | 修改 | 更新调用 `render_once` 的测试以传入 global；新增 Agent/Prompt 区块测试 |
| `src/tui_app/info.rs` | 修改 | `InfoState` 增加 `agent_cli` 字段；`reload()` 同时加载 global；`render_body` 改为 textwrap-based 渲染并加入 Agent/Prompt 区块 |

---

## Task 1: 添加 textwrap 依赖

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: 加依赖**

打开 `Cargo.toml`，找到 `[dependencies]` 段，在合适位置（按字母序）插入：

```toml
textwrap = "0.16"
```

- [ ] **Step 2: 验证编译**

Run: `cargo check`
Expected: PASS（不报错；textwrap 被解析下载）

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "deps: add textwrap 0.16 for info wrap"
```

---

## Task 2: `build_agent_cli_display` 函数 + 单元测试

**Files:**
- Modify: `src/core/layout.rs` (append after `build_agent_cli_kdl`)
- Modify: `tests/agent_cli_test.rs` (append at end)

- [ ] **Step 1: 写第一个失败测试 — 未配置时返回 None**

打开 `tests/agent_cli_test.rs`，在文件末尾追加：

```rust
use zootree::core::layout::build_agent_cli_display;

#[test]
fn build_agent_cli_display_returns_none_when_unset() {
    let ws = ws_with("Hello", "");
    assert!(build_agent_cli_display(None, &ws).is_none());
}
```

注：`ws_with` 是该文件已有的辅助函数；如果不存在，看文件顶部使用什么辅助函数构造 workspace 并复用。

- [ ] **Step 2: 运行测试，确认失败**

Run: `cargo test --test agent_cli_test build_agent_cli_display_returns_none_when_unset`
Expected: FAIL（编译错误：`build_agent_cli_display` 不存在）

- [ ] **Step 3: 实现最小函数**

打开 `src/core/layout.rs`，在 `build_agent_cli_kdl` 函数定义之后追加：

```rust
/// Resolved display of agent_cli with `$prompt` substituted, suitable for showing in `zootree info`.
///
/// - Returns `None` when `agent_cli_tpl` is `None` (caller should fall back to displaying `build_prompt`).
/// - Returns `Some(Err(..))` when the template fails to parse via shlex or is empty.
/// - Returns `Some(Ok(line))` where `line` is a single shell-quoted command string;
///   multi-line prompts are POSIX-quoted by `shlex::try_join` so the output is copy-pasteable.
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

- [ ] **Step 4: 运行测试，确认通过**

Run: `cargo test --test agent_cli_test build_agent_cli_display_returns_none_when_unset`
Expected: PASS

- [ ] **Step 5: 写第二个失败测试 — 单行 prompt 替换**

在 `tests/agent_cli_test.rs` 同一段下方追加：

```rust
#[test]
fn build_agent_cli_display_substitutes_single_line_prompt() {
    let ws = ws_with("Add login", "");
    let out = build_agent_cli_display(Some("claude --skip -- $prompt"), &ws)
        .expect("Some")
        .expect("Ok");
    assert!(out.contains("claude"), "got: {}", out);
    assert!(out.contains("--skip"), "got: {}", out);
    assert!(out.contains("'Add login'"), "expected single-quoted prompt, got: {}", out);
}
```

- [ ] **Step 6: 运行测试，确认通过**（已实现，应直接 PASS）

Run: `cargo test --test agent_cli_test build_agent_cli_display_substitutes_single_line_prompt`
Expected: PASS

- [ ] **Step 7: 写第三个失败测试 — 多行 prompt 保留换行**

```rust
#[test]
fn build_agent_cli_display_handles_multiline_prompt() {
    let ws = ws_with("Add login", "Implement OAuth2");
    let out = build_agent_cli_display(Some("claude --skip -- $prompt"), &ws)
        .expect("Some")
        .expect("Ok");
    // build_prompt joins title + description with '\n'.
    // shlex::try_join uses POSIX single-quoting which preserves the literal newline byte.
    assert!(out.contains("Add login\nImplement OAuth2"), "got: {:?}", out);
}
```

Run: `cargo test --test agent_cli_test build_agent_cli_display_handles_multiline_prompt`
Expected: PASS

- [ ] **Step 8: 写第四个失败测试 — 模板解析失败时返回 Err**

```rust
#[test]
fn build_agent_cli_display_returns_err_on_invalid_template() {
    let ws = ws_with("Hello", "");
    let unclosed = "claude 'unclosed";
    let result = build_agent_cli_display(Some(unclosed), &ws).expect("Some");
    assert!(result.is_err(), "expected Err for unclosed quote, got: {:?}", result);
}
```

Run: `cargo test --test agent_cli_test build_agent_cli_display_returns_err_on_invalid_template`
Expected: PASS

- [ ] **Step 9: 写第五个失败测试 — 空模板**

```rust
#[test]
fn build_agent_cli_display_returns_err_on_empty_template() {
    let ws = ws_with("Hello", "");
    let result = build_agent_cli_display(Some("   "), &ws).expect("Some");
    assert!(result.is_err(), "expected Err for empty template, got: {:?}", result);
}
```

Run: `cargo test --test agent_cli_test build_agent_cli_display_returns_err_on_empty_template`
Expected: PASS

- [ ] **Step 10: 跑完整 agent_cli_test 验证不破坏老测试**

Run: `cargo test --test agent_cli_test`
Expected: PASS（所有老测试 + 5 个新测试全绿）

- [ ] **Step 11: Commit**

```bash
git add src/core/layout.rs tests/agent_cli_test.rs
git commit -m "feat(layout): add build_agent_cli_display for info"
```

---

## Task 3: 更新 `render_once` 签名 + Agent/Prompt 区块

**Files:**
- Modify: `src/cli/info.rs`
- Modify: `tests/info_test.rs`

- [ ] **Step 1: 更新现有 6 个 `render_once` 测试调用以传入 global**

打开 `tests/info_test.rs`。在文件顶部 import 块加入：

```rust
use zootree::config::global::GlobalConfig;
```

然后在每个调用 `render_once(&status, &ws)` 的测试中改为 `render_once(&status, &ws, &GlobalConfig::default())`。具体涉及：

- `render_once_includes_core_fields`
- `render_once_omits_description_when_empty`
- `render_once_includes_description_when_present`
- `render_once_lists_repos_with_target_branch`
- `render_once_shows_last_five_events`
- `render_once_covers_all_statuses`（注意循环里也调用）

测试此时编译会失败——这是预期的，下一步会修 `render_once`。

- [ ] **Step 2: 修改 `render_once` 签名 + 加 Agent/Prompt 区块**

打开 `src/cli/info.rs`。在文件顶部 import 块新增：

```rust
use crate::config::global::GlobalConfig;
```

修改 `render_once` 函数签名与内容：

```rust
/// Build the multi-line textual report shown by `zootree info <name>`
/// without `--watch`. Pure function — easy to test.
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

    // Agent / Prompt block
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

- [ ] **Step 3: 修改 `handle_info` 加载 global 并传入**

在 `handle_info` 函数中，把：

```rust
print!("{}", render_once(&status, &workspace));
```

改为：

```rust
let global = config_mgr.load_global_config().unwrap_or_default();
print!("{}", render_once(&status, &workspace, &global));
```

注意 `load_global_config` 已经在文件不存在时返回 default，但 `unwrap_or_default()` 进一步兜底解析失败的情况。

- [ ] **Step 4: 跑修改的老测试，确认全部通过**

Run: `cargo test --test info_test`
Expected: PASS（6 个老测试加了 GlobalConfig 后全绿）

- [ ] **Step 5: 写新测试 — agent_cli 配置时输出 Agent: 区块**

在 `tests/info_test.rs` 末尾追加：

```rust
#[test]
fn render_once_includes_agent_section_when_configured() {
    let ws = base_ws();
    let mut global = GlobalConfig::default();
    global.agent_cli = Some("claude --skip -- $prompt".into());
    let out = render_once(&WorkspaceStatus::InProgress, &ws, &global);
    assert!(out.contains("Agent:"), "missing Agent: section:\n{}", out);
    assert!(out.contains("claude"), "missing claude in command:\n{}", out);
    assert!(!out.contains("Prompt:"), "should not include Prompt: when configured:\n{}", out);
}
```

- [ ] **Step 6: 写新测试 — agent_cli 未配置时输出 Prompt: 区块**

```rust
#[test]
fn render_once_includes_prompt_section_when_not_configured() {
    let ws = base_ws();
    let global = GlobalConfig::default(); // agent_cli = None
    let out = render_once(&WorkspaceStatus::InProgress, &ws, &global);
    assert!(out.contains("Prompt:"), "missing Prompt: section:\n{}", out);
    assert!(!out.contains("Agent:"), "should not include Agent: when unconfigured:\n{}", out);
}
```

- [ ] **Step 7: 写新测试 — agent_cli 模板解析失败时显示错误**

```rust
#[test]
fn render_once_shows_agent_section_with_error_on_invalid_template() {
    let ws = base_ws();
    let mut global = GlobalConfig::default();
    global.agent_cli = Some("claude 'unclosed".into());
    let out = render_once(&WorkspaceStatus::InProgress, &ws, &global);
    assert!(out.contains("Agent:"), "missing Agent: section:\n{}", out);
    assert!(out.contains("failed to parse"), "missing error message:\n{}", out);
}
```

- [ ] **Step 8: 跑全部 info_test，确认通过**

Run: `cargo test --test info_test`
Expected: PASS

- [ ] **Step 9: Commit**

```bash
git add src/cli/info.rs tests/info_test.rs
git commit -m "feat(info): render Agent/Prompt section in one-shot output"
```

---

## Task 4: TUI `InfoState` 加 agent_cli 字段 + reload 加载 global

**Files:**
- Modify: `src/tui_app/info.rs`

- [ ] **Step 1: 修改 `InfoState` 结构定义**

打开 `src/tui_app/info.rs`。找到 `pub(crate) struct InfoState` 定义，改为：

```rust
pub(crate) struct InfoState {
    pub status: WorkspaceStatus,
    pub workspace: WorkspaceConfig,
    pub loaded_at: DateTime<Local>,
    pub agent_cli: Option<String>,
}
```

- [ ] **Step 2: 更新 `reload()` 同时加载 global**

找到 `impl InfoApp { ... pub(crate) fn reload(&mut self) { ... } ... }`，把 reload 实现改为：

```rust
pub(crate) fn reload(&mut self) {
    match self.config_mgr.load_workspace(&self.name) {
        Ok((status, workspace)) => {
            let agent_cli = self
                .config_mgr
                .load_global_config()
                .ok()
                .and_then(|g| g.agent_cli);
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

- [ ] **Step 3: 跑 cargo check 确认编译通过**

Run: `cargo check --tests`
Expected: PASS（结构字段访问处此前没用 agent_cli，应不破坏现有用例）

- [ ] **Step 4: 跑现有 info.rs 单测确认未回归**

Run: `cargo test --lib tui_app::info`
Expected: PASS（老测试不应失败；构造 `InfoState` 的辅助函数若硬编码字段，需补 `agent_cli: None`——下方步骤处理）

注：如果 `cargo test` 报错说 `InfoState` 字段不全（例如某个测试直接构造 `InfoState`），打开报错位置在初始化处加 `agent_cli: None,`。如果通过则继续。

- [ ] **Step 5: Commit**

```bash
git add src/tui_app/info.rs
git commit -m "feat(tui-info): load agent_cli into InfoState on reload"
```

---

## Task 5: TUI `render_body` 重构 — textwrap + Agent/Prompt 区块

**Files:**
- Modify: `src/tui_app/info.rs`

- [ ] **Step 1: 写失败测试 — 长 title 在窄宽度下换行**

在 `src/tui_app/info.rs` 末尾的 `#[cfg(test)] mod tests { ... }` 内追加：

```rust
#[test]
fn render_wraps_long_title() {
    let tmp = tempfile::tempdir().unwrap();
    let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
    mgr.ensure_dirs().unwrap();
    let mut ws = sample_workspace("demo");
    ws.title = "A".repeat(200);
    mgr.save_workspace(&WorkspaceStatus::InProgress, &ws).unwrap();

    let mgr2 = ConfigManager::with_base_dir(tmp.path().to_path_buf());
    let mut app = InfoApp::new("demo".into(), mgr2, false, Duration::from_secs(5));

    let out = render_to_string(&mut app, 40, 30);
    // 长 title 至少应该被换成 2 行：第一行带 "Title:" 标签，后续行靠续行缩进。
    let title_a_lines: Vec<&str> = out.lines().filter(|l| l.contains("AAAA")).collect();
    assert!(
        title_a_lines.len() >= 2,
        "expected title to wrap to >=2 lines at width=40, got {}:\n{}",
        title_a_lines.len(),
        out
    );
}
```

- [ ] **Step 2: 写失败测试 — agent_cli 配置时显示 Agent: 区块**

继续追加：

```rust
#[test]
fn render_shows_agent_section_when_configured() {
    use crate::config::global::GlobalConfig;

    let tmp = tempfile::tempdir().unwrap();
    let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
    mgr.ensure_dirs().unwrap();
    let ws = sample_workspace("demo");
    mgr.save_workspace(&WorkspaceStatus::InProgress, &ws).unwrap();

    let mut global = GlobalConfig::default();
    global.agent_cli = Some("claude --skip -- $prompt".into());
    mgr.save_global_config(&global).unwrap();

    let mgr2 = ConfigManager::with_base_dir(tmp.path().to_path_buf());
    let mut app = InfoApp::new("demo".into(), mgr2, false, Duration::from_secs(5));

    let out = render_to_string(&mut app, 100, 30);
    assert!(out.contains("Agent:"), "missing Agent: section:\n{}", out);
    assert!(out.contains("claude"), "missing claude command:\n{}", out);
    assert!(!out.contains("Prompt:"), "should not include Prompt:\n{}", out);
}
```

- [ ] **Step 3: 写失败测试 — agent_cli 未配置时显示 Prompt: 区块**

```rust
#[test]
fn render_shows_prompt_section_when_not_configured() {
    let tmp = tempfile::tempdir().unwrap();
    let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
    mgr.ensure_dirs().unwrap();
    let ws = sample_workspace("demo");
    mgr.save_workspace(&WorkspaceStatus::InProgress, &ws).unwrap();
    // 不保存 GlobalConfig => load_global_config 返回 default => agent_cli = None

    let mgr2 = ConfigManager::with_base_dir(tmp.path().to_path_buf());
    let mut app = InfoApp::new("demo".into(), mgr2, false, Duration::from_secs(5));

    let out = render_to_string(&mut app, 100, 30);
    assert!(out.contains("Prompt:"), "missing Prompt: section:\n{}", out);
    assert!(!out.contains("Agent:"), "should not include Agent: when unconfigured:\n{}", out);
}
```

- [ ] **Step 4: 运行三个新测试，确认失败**

Run: `cargo test --lib tui_app::info::tests::render_wraps_long_title tui_app::info::tests::render_shows_agent_section_when_configured tui_app::info::tests::render_shows_prompt_section_when_not_configured`
Expected: FAIL（功能尚未实现 / Agent: 与 Prompt: 都不会出现）

- [ ] **Step 5: 跳过 — textwrap 用全限定路径调用**

后续辅助函数中使用 `textwrap::wrap(...)` 全限定路径，无需在文件顶部添加 `use textwrap;`。继续 Step 6。

- [ ] **Step 6: 修改 `meta_line` 返回 `Line<'static>` + 添加渲染辅助函数**

定位到现有：

```rust
fn meta_line<'a>(label: &'a str, value: &'a str) -> Line<'a> {
    Line::from(vec![
        Span::styled(
            format!("  {:<10}", label),
            Style::default().fg(Color::DarkGray),
        ),
        Span::raw(value.to_string()),
    ])
}
```

替换为（去掉 `'a` 生命周期，返回 `Line<'static>` 以便加入 `Vec<Line<'static>>`）：

```rust
fn meta_line(label: &str, value: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("  {:<10}", label),
            Style::default().fg(Color::DarkGray),
        ),
        Span::raw(value.to_string()),
    ])
}
```

然后在 `meta_line` 之后追加：

```rust
/// 给定 area 宽度，返回 description / agent / prompt 内容区可用的列宽（扣除 4 列缩进 + 安全下限）。
fn content_width(area_width: u16) -> usize {
    (area_width as usize).saturating_sub(4).max(20)
}

/// 把 "Label: long-text" 这种 key-value 行 wrap 成多行，续行只缩进不重复 label。
/// 续行缩进 12 列，与 `meta_line` 中 "  {label:<10}" 的对齐位置一致。
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
        lines.push(Line::from(format!("            {}", cont)));
    }
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

/// 根据宽度构造 meta 区块的全部行（包含 Title / Branch / Dir / Created / Description / Agent or Prompt）。
fn build_meta_lines(
    ws: &WorkspaceConfig,
    agent_cli: Option<&str>,
    area_width: u16,
) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();

    push_wrapped_kv(&mut lines, "Title:", &ws.title, area_width);
    lines.push(meta_line("Branch:", &ws.branch));
    lines.push(meta_line("Dir:", &ws.workspace_dir));
    lines.push(meta_line(
        "Created:",
        &format_rfc3339_to_minute(&ws.created_at),
    ));

    if !ws.description.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from("  Description:"));
        for wrapped in textwrap::wrap(&ws.description, content_width(area_width)) {
            lines.push(Line::from(format!("    {}", wrapped)));
        }
    }

    lines.push(Line::from(""));
    let (label, content) = resolve_agent_or_prompt_display(ws, agent_cli);
    lines.push(Line::from(format!("  {}", label)));
    for wrapped in textwrap::wrap(&content, content_width(area_width)) {
        lines.push(Line::from(format!("    {}", wrapped)));
    }

    lines
}
```

- [ ] **Step 7: 重写 `render_body`**

定位到 `impl InfoApp { ... fn render_body(...) { ... } ... }` 中的 `render_body`，整体替换为：

```rust
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
    let meta_lines = build_meta_lines(ws, state.agent_cli.as_deref(), area.width);
    let meta_height_full = meta_lines.len() as u16;

    // 留至少 6 行给 repos + events，避免 meta 撑爆 body
    let max_meta = area.height.saturating_sub(6);
    let actual_meta = meta_height_full.min(max_meta);

    let repos_rows = ws.repos.len().max(1) as u16;
    let repos_height = 2 + repos_rows;

    let chunks = Layout::vertical([
        Constraint::Length(actual_meta),
        Constraint::Length(repos_height),
        Constraint::Min(1),
    ])
    .split(area);

    frame.render_widget(Paragraph::new(meta_lines), chunks[0]);

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
    .header(
        Row::new(vec!["NAME", "TARGET", "WORKTREE"])
            .style(Style::default().fg(Color::DarkGray)),
    )
    .block(Block::default().borders(Borders::TOP).title(" Repos "));
    frame.render_widget(table, chunks[1]);

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
    let list = List::new(items).block(
        Block::default()
            .borders(Borders::TOP)
            .title(" Recent events "),
    );
    frame.render_widget(list, chunks[2]);
}
```

- [ ] **Step 8: 跑三个新测试，确认通过**

Run: `cargo test --lib tui_app::info::tests::render_wraps_long_title tui_app::info::tests::render_shows_agent_section_when_configured tui_app::info::tests::render_shows_prompt_section_when_not_configured`
Expected: PASS

- [ ] **Step 9: 跑完整 info 模块测试，确认未回归**

Run: `cargo test --lib tui_app::info`
Expected: PASS

特别检查老测试 `render_shows_name_status_and_title`、`render_shows_repos_row` 是否仍通过——如失败，多半是 description 现在是单行 wrap 而非按 `\n` 切分；调整断言或测试里的 description 内容（保持 wrap 行为下断言通过）。

- [ ] **Step 10: 跑全量测试套件**

Run: `cargo test`
Expected: PASS（所有 unit + integration 测试全绿）

- [ ] **Step 11: Commit**

```bash
git add src/tui_app/info.rs
git commit -m "feat(tui-info): wrap long title/desc + render Agent/Prompt block"
```

---

## Task 6: 手工 smoke test + 最终验证

**Files:** none（仅运行）

- [ ] **Step 1: cargo fmt + clippy**

Run: `cargo fmt --all && cargo clippy --all-targets -- -D warnings`
Expected: 无 warning / 无 format diff

- [ ] **Step 2: One-shot 手测（agent_cli 已配置）**

确认本机 `~/.config/zootree/config.toml` 是否已配置 `agent_cli`：

```bash
grep agent_cli ~/.config/zootree/config.toml || echo "(unset)"
```

如已配置，直接：

```bash
cargo run -- info
```

（交互选一个已有 workspace。）期望输出包含 `Agent:` 段，下方一行是 shell-quoted 命令。

如未配置，先临时改一份测试用 config 或在 ConfigManager 上跑（可选）；fallback 路径在 step 3 验证。

- [ ] **Step 3: One-shot 手测（agent_cli 未配置场景）**

临时备份并清空 agent_cli：

```bash
cp ~/.config/zootree/config.toml /tmp/zootree-config.toml.bak
sed -i.bak '/^agent_cli/d' ~/.config/zootree/config.toml
cargo run -- info <some-workspace-name>
```

期望输出包含 `Prompt:` 段（不是 Agent:），下方多行展开 title/description。

恢复：

```bash
mv /tmp/zootree-config.toml.bak ~/.config/zootree/config.toml
```

- [ ] **Step 4: TUI watch 手测（窄终端）**

把终端窗口横向缩到 ~50 列宽，跑：

```bash
cargo run -- info <some-workspace-name> --watch
```

肉眼检查：
- 长 title 自动换行，续行靠 12 列缩进对齐到值列
- description（如有）按宽度换行
- 底部有 `Agent:` 或 `Prompt:` 区块且其内容也会换行
- `q` 能退出

- [ ] **Step 5: TUI watch 手测（agent_cli 配置时）**

确认 `agent_cli` 已设置后再跑 `cargo run -- info <name> --watch`，验证显示 `Agent:` + 命令。Ctrl+C 或 `q` 退出。

- [ ] **Step 6: 提交（如 fmt/clippy 修改了文件）**

```bash
git status
# 如果有改动：
git add -A
git commit -m "chore: cargo fmt/clippy fixes"
```

如无改动，跳过。

---

## 完成

至此本次 feature 全部实现并通过：
- TUI 模式下长 title/description 按 pane 宽度换行
- 新增 `Agent:` 区块（agent_cli 配置时）/ `Prompt:` 区块（fallback）
- one-shot 与 watch 行为一致
- 单元 + 集成测试覆盖核心场景

**下一步**：使用 `superpowers:finishing-a-development-branch` 决定如何 ship（合 main / 开 PR / 推 remote）。
