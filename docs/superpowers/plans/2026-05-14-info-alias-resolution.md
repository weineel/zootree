# `zootree info` Agent 块 alias 解析 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 `zootree info` 的 `Agent:` 块在 `global.agent_cli` 命中 `agent_cli_alias` 时，先单层解析再展示，并附加 `(via alias: <name>)` 标注与单行 `Alias:` 段。

**Architecture:** 在 `src/core/layout.rs` 引入 `AliasInfo`/`AgentDisplay` 结构体并把 `build_agent_cli_display` 改为 `(tpl, alias_map, ws) -> Option<Result<AgentDisplay>>`，把 alias 解析下沉到 layout 层。`cli/info.rs` 与 `tui_app/info.rs` 两个调用点根据 `AgentDisplay.alias` 决定是否追加标注与 `Alias:` 段。`InfoState` 同步存一份 `agent_cli_alias`，reload 时刷新。

**Tech Stack:** Rust 2021, ratatui 0.29, crossterm 0.28, textwrap 0.16, shlex 1, anyhow 1。

参考 spec：`docs/superpowers/specs/2026-05-13-info-alias-resolution-design.md`。

---

### Task 1: `build_agent_cli_display` API 重构（破坏性签名变更）

**Files:**
- Modify: `src/core/layout.rs`
- Modify: `src/cli/info.rs:99-114`
- Modify: `src/tui_app/info.rs:24` (InfoState struct), `src/tui_app/info.rs:42-62` (reload), `src/tui_app/info.rs:186` (build_meta_lines call), `src/tui_app/info.rs:320-358`
- Modify: `tests/agent_cli_test.rs:34-91`（适配新签名）

本步只做 API 重构 + 调用点适配；调用点暂不渲染 alias 标注，仅"丢弃" `AgentDisplay.alias` 字段。这样 cargo build/test 在本步即可通过且 user-visible 行为不变。

- [ ] **Step 1: 替换 `src/core/layout.rs` 中 `build_agent_cli_display` 与新增类型**

把现有的 `build_agent_cli_display` 整体替换为下面这段：

```rust
/// Resolved alias info: which alias key was hit, and the alias's raw template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AliasInfo {
    pub name: String,
    pub template: String,
}

/// Display-ready agent command, plus optional alias provenance.
#[derive(Debug, Clone)]
pub struct AgentDisplay {
    /// Shell-quoted command line with `$prompt` substituted.
    pub command: String,
    /// `Some(..)` when `agent_cli_tpl` was a key in the alias map.
    pub alias: Option<AliasInfo>,
}

/// Resolved display of agent_cli with `$prompt` substituted, suitable for showing in `zootree info`.
///
/// Resolves `agent_cli_tpl` against `alias_map` (single level, same semantics as
/// [`resolve_agent_cli`]) before parsing — so when `agent_cli_tpl` is an alias
/// key, the displayed command reflects the alias's underlying template.
///
/// - Returns `None` when `agent_cli_tpl` is `None` (caller should fall back to displaying `build_prompt`).
/// - Returns `Some(Err(..))` when the (possibly alias-resolved) template fails to parse via shlex,
///   is empty, or fails to re-join after substitution.
/// - Returns `Some(Ok(display))` where `display.command` is a single shell-quoted command string;
///   `display.alias` is `Some(..)` if the input matched an alias key.
pub fn build_agent_cli_display(
    agent_cli_tpl: Option<&str>,
    alias_map: &BTreeMap<String, String>,
    workspace: &WorkspaceConfig,
) -> Option<anyhow::Result<AgentDisplay>> {
    let tpl = agent_cli_tpl?;
    let alias = alias_map.get(tpl).map(|template| AliasInfo {
        name: tpl.to_string(),
        template: template.clone(),
    });
    let resolved: &str = alias
        .as_ref()
        .map(|a| a.template.as_str())
        .unwrap_or(tpl);
    let prompt = build_prompt(workspace);

    let result = (|| -> anyhow::Result<String> {
        let tokens = shlex::split(resolved)
            .ok_or_else(|| anyhow::anyhow!("failed to parse agent_cli: {}", resolved))?;
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

    Some(result.map(|command| AgentDisplay { command, alias }))
}
```

- [ ] **Step 2: 适配 `src/cli/info.rs` 的调用点**

把 `src/cli/info.rs` 第 99-114 行的整个 `match` 替换为：

```rust
    match crate::core::layout::build_agent_cli_display(
        global.agent_cli.as_deref(),
        &global.agent_cli_alias,
        ws,
    ) {
        Some(Ok(display)) => {
            let _ = writeln!(out, "Agent:");
            let _ = writeln!(out, "  {}", display.command);
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
```

(注意：Task 3 会再扩展这段加入 alias 标注与 Alias 段；本步先保持现有可见行为。)

- [ ] **Step 3: 给 `InfoState` 加上 `agent_cli_alias` 字段并在 reload 时刷新**

修改 `src/tui_app/info.rs`：

把 `InfoState` 结构体（第 20-25 行）改为：

```rust
pub(crate) struct InfoState {
    pub status: WorkspaceStatus,
    pub workspace: WorkspaceConfig,
    pub loaded_at: DateTime<Local>,
    pub agent_cli: Option<String>,
    pub agent_cli_alias: std::collections::BTreeMap<String, String>,
}
```

把 `reload` 方法（第 42-62 行）替换为：

```rust
    pub(crate) fn reload(&mut self) {
        match self.config_mgr.load_workspace(&self.name) {
            Ok((status, workspace)) => {
                let global = self.config_mgr.load_global_config().ok();
                let agent_cli = global.as_ref().and_then(|g| g.agent_cli.clone());
                let agent_cli_alias = global
                    .map(|g| g.agent_cli_alias)
                    .unwrap_or_default();
                self.state = Some(InfoState {
                    status,
                    workspace,
                    loaded_at: Local::now(),
                    agent_cli,
                    agent_cli_alias,
                });
                self.last_error = None;
            }
            Err(e) => {
                self.last_error = Some(format!("{:#}", e));
            }
        }
    }
```

- [ ] **Step 4: 适配 `src/tui_app/info.rs` 的渲染调用点**

把 `resolve_agent_or_prompt_display`（第 320-329 行）替换为：

```rust
fn resolve_agent_or_prompt_display(
    ws: &WorkspaceConfig,
    agent_cli: Option<&str>,
    alias_map: &std::collections::BTreeMap<String, String>,
) -> (&'static str, String) {
    match crate::core::layout::build_agent_cli_display(agent_cli, alias_map, ws) {
        Some(Ok(display)) => ("Agent:", display.command),
        Some(Err(e)) => ("Agent:", format!("(failed to parse agent_cli: {:#})", e)),
        None => ("Prompt:", crate::core::layout::build_prompt(ws)),
    }
}
```

把 `build_meta_lines` 函数签名（第 331-335 行）改为：

```rust
fn build_meta_lines(
    ws: &WorkspaceConfig,
    agent_cli: Option<&str>,
    alias_map: &std::collections::BTreeMap<String, String>,
    area_width: u16,
) -> Vec<Line<'static>> {
```

并把函数体内 `let (label, content) = resolve_agent_or_prompt_display(ws, agent_cli);`（第 352 行）改为：

```rust
    let (label, content) = resolve_agent_or_prompt_display(ws, agent_cli, alias_map);
```

把 `render_body` 中 `let meta_lines = build_meta_lines(ws, state.agent_cli.as_deref(), area.width);`（第 186 行）改为：

```rust
        let meta_lines = build_meta_lines(
            ws,
            state.agent_cli.as_deref(),
            &state.agent_cli_alias,
            area.width,
        );
```

- [ ] **Step 5: 适配 `tests/agent_cli_test.rs` 中现有 `build_agent_cli_display` 测试**

把第 34-91 行的 5 个测试替换为下面这段（签名增加了 `&BTreeMap`，行为期望不变）：

```rust
#[test]
fn build_agent_cli_display_returns_none_when_unset() {
    let ws = make_workspace("Hello", "");
    assert!(build_agent_cli_display(None, &BTreeMap::new(), &ws).is_none());
}

#[test]
fn build_agent_cli_display_substitutes_single_line_prompt() {
    let ws = make_workspace("Add login", "");
    let out = build_agent_cli_display(Some("claude --skip -- $prompt"), &BTreeMap::new(), &ws)
        .expect("Some")
        .expect("Ok");
    assert!(out.command.contains("claude"), "got: {}", out.command);
    assert!(out.command.contains("--skip"), "got: {}", out.command);
    assert!(
        out.command.contains("'Add login'"),
        "expected single-quoted prompt, got: {}",
        out.command
    );
    assert!(out.alias.is_none(), "expected no alias, got: {:?}", out.alias);
}

#[test]
fn build_agent_cli_display_handles_multiline_prompt() {
    let ws = make_workspace("Add login", "Implement OAuth2");
    let out = build_agent_cli_display(Some("claude --skip -- $prompt"), &BTreeMap::new(), &ws)
        .expect("Some")
        .expect("Ok");
    assert!(
        out.command.contains("Add login\nImplement OAuth2"),
        "got: {:?}",
        out.command
    );
}

#[test]
fn build_agent_cli_display_returns_err_on_invalid_template() {
    let ws = make_workspace("Hello", "");
    let unclosed = "claude 'unclosed";
    let result = build_agent_cli_display(Some(unclosed), &BTreeMap::new(), &ws).expect("Some");
    assert!(
        result.is_err(),
        "expected Err for unclosed quote, got: {:?}",
        result.map(|d| d.command)
    );
}

#[test]
fn build_agent_cli_display_returns_err_on_empty_template() {
    let ws = make_workspace("Hello", "");
    let result = build_agent_cli_display(Some("   "), &BTreeMap::new(), &ws).expect("Some");
    assert!(
        result.is_err(),
        "expected Err for empty template, got: {:?}",
        result.map(|d| d.command)
    );
}
```

并在文件顶部 `use zootree::core::layout::build_agent_cli_display;`（第 32 行）这一行旁边一起改用新导出（`build_agent_cli_display` 仍然导出，但 `AgentDisplay` 也要在后续用到 `.command` / `.alias` 时被识别）：

把第 32 行：

```rust
use zootree::core::layout::build_agent_cli_display;
```

替换为：

```rust
use zootree::core::layout::{build_agent_cli_display, AgentDisplay, AliasInfo};
```

(`AgentDisplay`/`AliasInfo` 在 Task 2 测试里直接用，提前 import 就好。)

- [ ] **Step 6: 编译并跑全量测试**

Run: `cargo build`
Expected: 编译通过，无 warning。

Run: `cargo test`
Expected: 全部测试通过，包括 `tests/agent_cli_test.rs`、`tests/info_test.rs`、`src/tui_app/info.rs` 的 inline tests。

- [ ] **Step 7: 提交**

```bash
git add src/core/layout.rs src/cli/info.rs src/tui_app/info.rs tests/agent_cli_test.rs
git commit -m "refactor(layout): build_agent_cli_display takes alias_map and returns AgentDisplay"
```

---

### Task 2: alias 解析的单元测试

**Files:**
- Modify: `tests/agent_cli_test.rs`（追加测试）

- [ ] **Step 1: 写命中 alias 的失败测试**

在 `tests/agent_cli_test.rs` 文件末尾追加：

```rust
#[test]
fn build_agent_cli_display_resolves_alias_and_reports_provenance() {
    let ws = make_workspace("Add login", "");
    let mut alias_map = BTreeMap::new();
    alias_map.insert("safe".to_string(), "claude --skip -- $prompt".to_string());

    let out = build_agent_cli_display(Some("safe"), &alias_map, &ws)
        .expect("Some")
        .expect("Ok");

    assert_eq!(
        out.alias,
        Some(AliasInfo {
            name: "safe".to_string(),
            template: "claude --skip -- $prompt".to_string(),
        }),
    );
    assert!(out.command.contains("claude"), "got: {}", out.command);
    assert!(out.command.contains("--skip"), "got: {}", out.command);
    assert!(
        out.command.contains("'Add login'"),
        "expected prompt expansion, got: {}",
        out.command
    );
}

#[test]
fn build_agent_cli_display_no_alias_when_tpl_is_literal() {
    let ws = make_workspace("Hello", "");
    let mut alias_map = BTreeMap::new();
    alias_map.insert("safe".to_string(), "claude -- $prompt".to_string());

    let out = build_agent_cli_display(Some("gemini chat -- $prompt"), &alias_map, &ws)
        .expect("Some")
        .expect("Ok");

    assert!(out.alias.is_none(), "got: {:?}", out.alias);
    assert!(out.command.contains("gemini"), "got: {}", out.command);
}

#[test]
fn build_agent_cli_display_alias_with_invalid_template_errors() {
    let ws = make_workspace("Hello", "");
    let mut alias_map = BTreeMap::new();
    alias_map.insert("broken".to_string(), "claude 'unclosed".to_string());

    let result = build_agent_cli_display(Some("broken"), &alias_map, &ws).expect("Some");
    assert!(
        result.is_err(),
        "expected Err for alias pointing at invalid template, got: {:?}",
        result.map(|d| d.command)
    );
}

#[test]
fn build_agent_cli_display_alias_with_empty_template_errors() {
    let ws = make_workspace("Hello", "");
    let mut alias_map = BTreeMap::new();
    alias_map.insert("empty".to_string(), "   ".to_string());

    let result = build_agent_cli_display(Some("empty"), &alias_map, &ws).expect("Some");
    assert!(
        result.is_err(),
        "expected Err for alias pointing at empty template, got: {:?}",
        result.map(|d| d.command)
    );
}
```

- [ ] **Step 2: 跑测试**

Run: `cargo test --test agent_cli_test`
Expected: 4 个新增测试全部通过；既有测试维持通过。

- [ ] **Step 3: 提交**

```bash
git add tests/agent_cli_test.rs
git commit -m "test(agent_cli): cover alias resolution in build_agent_cli_display"
```

---

### Task 3: `cli/info.rs::render_once` 渲染 alias 标注与 Alias 段

**Files:**
- Modify: `src/cli/info.rs:99-114`
- Modify: `tests/info_test.rs`（追加测试）

- [ ] **Step 1: 给 `tests/info_test.rs` 加失败测试 — alias 命中**

在 `tests/info_test.rs` 文件末尾追加：

```rust
#[test]
fn render_once_includes_alias_annotation_and_alias_section() {
    use std::collections::BTreeMap;
    let ws = base_ws();
    let mut alias_map = BTreeMap::new();
    alias_map.insert("safe".to_string(), "claude --skip -- $prompt".to_string());
    let global = GlobalConfig {
        agent_cli: Some("safe".into()),
        agent_cli_alias: alias_map,
        ..Default::default()
    };
    let out = render_once(&WorkspaceStatus::InProgress, &ws, &global);

    assert!(out.contains("Agent:"), "missing Agent: section:\n{}", out);
    assert!(
        out.contains("(via alias: safe)"),
        "missing alias annotation:\n{}",
        out
    );
    assert!(
        out.contains("Alias:\n  safe = claude --skip -- $prompt"),
        "missing single-line Alias section:\n{}",
        out
    );
}

#[test]
fn render_once_omits_alias_section_for_literal_template() {
    let ws = base_ws();
    let global = GlobalConfig {
        agent_cli: Some("claude --skip -- $prompt".into()),
        ..Default::default()
    };
    let out = render_once(&WorkspaceStatus::InProgress, &ws, &global);

    assert!(out.contains("Agent:"), "missing Agent: section:\n{}", out);
    assert!(
        !out.contains("via alias:"),
        "should not include alias annotation:\n{}",
        out
    );
    assert!(
        !out.contains("Alias:"),
        "should not include Alias section:\n{}",
        out
    );
}

#[test]
fn render_once_omits_alias_section_on_parse_error() {
    use std::collections::BTreeMap;
    let ws = base_ws();
    let mut alias_map = BTreeMap::new();
    alias_map.insert("broken".to_string(), "claude 'unclosed".to_string());
    let global = GlobalConfig {
        agent_cli: Some("broken".into()),
        agent_cli_alias: alias_map,
        ..Default::default()
    };
    let out = render_once(&WorkspaceStatus::InProgress, &ws, &global);

    assert!(out.contains("Agent:"), "missing Agent: section:\n{}", out);
    assert!(out.contains("failed to parse"), "missing error:\n{}", out);
    assert!(
        !out.contains("via alias:"),
        "should not show alias annotation on parse error:\n{}",
        out
    );
    assert!(
        !out.contains("Alias:"),
        "should not show Alias section on parse error:\n{}",
        out
    );
}
```

- [ ] **Step 2: 跑测试，确认新增的 3 个失败**

Run: `cargo test --test info_test render_once_includes_alias_annotation_and_alias_section render_once_omits_alias_section_for_literal_template render_once_omits_alias_section_on_parse_error`
Expected: 3 个新测试全部 FAIL（Agent 行还没有 `(via alias:..)`，也没有 `Alias:` 段）。其余原测试仍然通过。

- [ ] **Step 3: 在 `cli/info.rs` 中实现**

把 `src/cli/info.rs` 当前的 `Some(Ok(display))` 分支扩展为带 alias 标注与 Alias 段。整个 match 替换为：

```rust
    match crate::core::layout::build_agent_cli_display(
        global.agent_cli.as_deref(),
        &global.agent_cli_alias,
        ws,
    ) {
        Some(Ok(display)) => {
            let _ = writeln!(out, "Agent:");
            if let Some(alias) = &display.alias {
                let _ = writeln!(
                    out,
                    "  {}  (via alias: {})",
                    display.command, alias.name
                );
                let _ = writeln!(out);
                let _ = writeln!(out, "Alias:");
                let _ = writeln!(out, "  {} = {}", alias.name, alias.template);
            } else {
                let _ = writeln!(out, "  {}", display.command);
            }
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
```

- [ ] **Step 4: 跑测试，确认新增 3 个全部通过**

Run: `cargo test --test info_test`
Expected: 全部通过（含原有的 `render_once_*` 测试）。

- [ ] **Step 5: 提交**

```bash
git add src/cli/info.rs tests/info_test.rs
git commit -m "feat(info): show alias annotation and Alias section in zootree info"
```

---

### Task 4: TUI 渲染 alias 标注与 Alias 段

**Files:**
- Modify: `src/tui_app/info.rs:320-358`（`resolve_agent_or_prompt_display`、`build_meta_lines`）
- Modify: `src/tui_app/info.rs:680-710`（既有 inline 测试，追加）

- [ ] **Step 1: 给 `src/tui_app/info.rs` 内嵌测试增加失败测试 — alias 命中**

在 `#[cfg(test)] mod tests` 内（最末，紧接 `render_shows_prompt_section_when_not_configured` 之后）追加：

```rust
    #[test]
    fn render_shows_alias_annotation_and_alias_section() {
        use crate::config::global::GlobalConfig;
        use std::collections::BTreeMap;

        let tmp = tempfile::tempdir().unwrap();
        let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
        mgr.ensure_dirs().unwrap();
        let ws = sample_workspace("demo");
        mgr.save_workspace(&WorkspaceStatus::InProgress, &ws)
            .unwrap();

        let mut alias_map = BTreeMap::new();
        alias_map.insert("safe".to_string(), "claude --skip -- $prompt".to_string());
        let global = GlobalConfig {
            agent_cli: Some("safe".into()),
            agent_cli_alias: alias_map,
            ..Default::default()
        };
        mgr.save_global_config(&global).unwrap();

        let mgr2 = ConfigManager::with_base_dir(tmp.path().to_path_buf());
        let mut app = InfoApp::new("demo".into(), mgr2, false, Duration::from_secs(5));

        let out = render_to_string(&mut app, 120, 30);
        assert!(out.contains("Agent:"), "missing Agent:\n{}", out);
        assert!(
            out.contains("(via alias: safe)"),
            "missing alias annotation:\n{}",
            out
        );
        assert!(out.contains("Alias:"), "missing Alias label:\n{}", out);
        assert!(
            out.contains("safe = claude --skip -- $prompt"),
            "missing alias body:\n{}",
            out
        );
    }

    #[test]
    fn render_omits_alias_section_for_literal_template() {
        use crate::config::global::GlobalConfig;

        let tmp = tempfile::tempdir().unwrap();
        let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
        mgr.ensure_dirs().unwrap();
        let ws = sample_workspace("demo");
        mgr.save_workspace(&WorkspaceStatus::InProgress, &ws)
            .unwrap();

        let global = GlobalConfig {
            agent_cli: Some("claude --skip -- $prompt".into()),
            ..Default::default()
        };
        mgr.save_global_config(&global).unwrap();

        let mgr2 = ConfigManager::with_base_dir(tmp.path().to_path_buf());
        let mut app = InfoApp::new("demo".into(), mgr2, false, Duration::from_secs(5));

        let out = render_to_string(&mut app, 120, 30);
        assert!(out.contains("Agent:"), "missing Agent:\n{}", out);
        assert!(
            !out.contains("via alias:"),
            "should not include alias annotation:\n{}",
            out
        );
        assert!(
            !out.contains("Alias:"),
            "should not include Alias section:\n{}",
            out
        );
    }
```

- [ ] **Step 2: 跑测试，确认 alias 测试 FAIL**

Run: `cargo test --lib tui_app::info::tests::render_shows_alias_annotation_and_alias_section tui_app::info::tests::render_omits_alias_section_for_literal_template`
Expected: 第一个 FAIL（找不到 `(via alias: safe)` 与 `Alias:` 段），第二个 PASS。

- [ ] **Step 3: 改造 `resolve_agent_or_prompt_display` 返回 alias 信息**

把 `src/tui_app/info.rs` 中现有的 `resolve_agent_or_prompt_display` 替换为：

```rust
fn resolve_agent_or_prompt_display(
    ws: &WorkspaceConfig,
    agent_cli: Option<&str>,
    alias_map: &std::collections::BTreeMap<String, String>,
) -> (
    &'static str,
    String,
    Option<crate::core::layout::AliasInfo>,
) {
    match crate::core::layout::build_agent_cli_display(agent_cli, alias_map, ws) {
        Some(Ok(display)) => ("Agent:", display.command, display.alias),
        Some(Err(e)) => (
            "Agent:",
            format!("(failed to parse agent_cli: {:#})", e),
            None,
        ),
        None => ("Prompt:", crate::core::layout::build_prompt(ws), None),
    }
}
```

- [ ] **Step 4: 在 `build_meta_lines` 中渲染 alias 标注与 Alias 段**

把 `build_meta_lines` 的实现（当前第 331-358 行）替换为：

```rust
fn build_meta_lines(
    ws: &WorkspaceConfig,
    agent_cli: Option<&str>,
    alias_map: &std::collections::BTreeMap<String, String>,
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
    let (label, content, alias) = resolve_agent_or_prompt_display(ws, agent_cli, alias_map);
    lines.push(Line::from(format!("  {}", label)));
    let body = match &alias {
        Some(a) => format!("{}  (via alias: {})", content, a.name),
        None => content,
    };
    for wrapped in textwrap::wrap(&body, content_width(area_width)) {
        lines.push(Line::from(format!("    {}", wrapped)));
    }
    if let Some(a) = alias {
        lines.push(Line::from(""));
        lines.push(Line::from("  Alias:"));
        let alias_body = format!("{} = {}", a.name, a.template);
        for wrapped in textwrap::wrap(&alias_body, content_width(area_width)) {
            lines.push(Line::from(format!("    {}", wrapped)));
        }
    }
    lines
}
```

- [ ] **Step 5: 全量测试**

Run: `cargo test`
Expected: 所有测试通过，包括两个新增的 inline 测试与原有 `render_shows_agent_section_when_configured`、`render_shows_prompt_section_when_not_configured`。

- [ ] **Step 6: fmt + clippy**

Run: `cargo fmt -- --check && cargo clippy --all-targets -- -D warnings`
Expected: 干净，无 warning。

- [ ] **Step 7: 提交**

```bash
git add src/tui_app/info.rs
git commit -m "feat(info-tui): show alias annotation and Alias section in watch mode"
```

---

## Self-Review

**Spec coverage:**
- ✅ "Agent 行末追加 `(via alias: <name>)`，仅命中 alias" — Task 3 Step 3、Task 4 Step 4
- ✅ "命中时显示单行 `Alias:` 段" — Task 3 Step 3、Task 4 Step 4
- ✅ "字面量/解析失败/None 不显示 alias 段" — Task 3 Step 3 全分支显式处理；Task 4 Step 3 `None` 分支返回 alias=None；Task 4 Step 4 `if let Some(a)` 仅命中分支输出
- ✅ "`InfoState` 增 `agent_cli_alias`，reload 同步" — Task 1 Step 3
- ✅ "TUI 走现有 wrap" — Task 4 Step 4 通过 `content_width` + `textwrap::wrap`
- ✅ API 形态（`AliasInfo`/`AgentDisplay` + 新签名） — Task 1 Step 1
- ✅ `resolve_agent_cli` 不动 — 计划全程未触
- ✅ 单层解析，不做多级 — Task 1 Step 1 内仅一次 `alias_map.get(tpl)`

**测试覆盖：**
- API 单元：Task 1 Step 5（既有改造）+ Task 2（新增 4 个 alias 测试）
- 一次性输出：Task 3 Step 1（3 个新测试覆盖 alias 命中 / 字面量 / 解析失败）
- TUI：Task 4 Step 1（2 个 TestBackend 测试覆盖 alias 命中 / 字面量）

**类型一致性：**
- `AliasInfo { name: String, template: String }`、`AgentDisplay { command: String, alias: Option<AliasInfo> }` 在所有任务里同名同字段
- `build_agent_cli_display(Option<&str>, &BTreeMap<String, String>, &WorkspaceConfig)` 全文一致
- TUI: `agent_cli_alias: BTreeMap<String, String>` 字段名与 GlobalConfig 同名
- `(via alias: <name>)` 与 `<name> = <template>` 字面量在测试与实现里一致

**占位符扫描：** 无 TBD/TODO/"similar to"/"add error handling"。

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-05-14-info-alias-resolution.md`. Two execution options:

1. **Subagent-Driven (recommended)** - I dispatch a fresh subagent per task, review between tasks, fast iteration
2. **Inline Execution** - Execute tasks in this session using executing-plans, batch execution with checkpoints

Which approach?
