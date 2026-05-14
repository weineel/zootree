# `zootree info` Agent 块 alias 解析设计

## 背景

`src/config/global.rs` 增加了 `agent_cli_alias: BTreeMap<String, String>` 后，`zootree start --run-agent` 路径（`src/cli/workspace.rs`）已通过 `resolve_agent_cli` 把 alias key 解析成真正的模板再喂给 `build_agent_cli_kdl`。但 `zootree info` 的一次性输出（`src/cli/info.rs::render_once`）与 `--watch` TUI（`src/tui_app/info.rs`）都直接把 `global.agent_cli` 传给 `build_agent_cli_display`，没过 `resolve_agent_cli`。

后果：当 `global.agent_cli = "claude-safe"`（一个 alias 名），info 会把它当字面量渲染成 `command="claude-safe"`，`$prompt` 不会注入，与 `--run-agent` 实际执行的命令不一致。

## 目标

- `zootree info` 显示的 `Agent:` 块反映 `--run-agent`（不带参数时）真正会执行的命令
- 命中 alias 时给出来源标注，让用户一眼看出这条命令是从哪个 alias 展开来的
- 一次性输出与 `--watch` 两端行为一致

## 范围

- 单层解析：`agent_cli` 命中 alias 时展开一次，不做多级/环检测（与 `--run-agent` 当前行为一致）
- 只在命中时显示 `Alias:` 段；不列出所有可用别名
- 不改变 `--run-agent` 路径的行为，不改变 `resolve_agent_cli` 语义

## API 变更

### `src/core/layout.rs`

新增结构体：

```rust
pub struct AliasInfo {
    pub name: String,     // alias key，如 "claude-safe"
    pub template: String, // 原文模板，未做 $prompt 替换
}

pub struct AgentDisplay {
    pub command: String,          // shell-quoted，已替换 $prompt
    pub alias: Option<AliasInfo>, // Some(..) 当输入 tpl 是 alias key
}
```

修改 `build_agent_cli_display` 签名：

```rust
pub fn build_agent_cli_display(
    agent_cli_tpl: Option<&str>,
    alias_map: &BTreeMap<String, String>,
    workspace: &WorkspaceConfig,
) -> Option<anyhow::Result<AgentDisplay>>
```

内部流程：
1. `let tpl = agent_cli_tpl?;`
2. `let alias = alias_map.get(tpl).map(|t| AliasInfo { name: tpl.to_string(), template: t.clone() });`
3. `let resolved: &str = alias.as_ref().map(|a| a.template.as_str()).unwrap_or(tpl);`
4. 对 `resolved` 执行原 shlex-split → `$prompt` 替换 → `shlex::try_join` 流程，得到 `command`
5. 返回 `Some(Ok(AgentDisplay { command, alias }))`

返回值语义（与既有文档保持一致，失败包含 shlex 解析失败、空模板、try_join 失败三种情形）以原文模板（alias 命中时是解析后的模板）为解析对象。

`resolve_agent_cli` 保持原样不动（仍被 `workspace.rs` 使用）。

## 呈现规则

### 一次性（`cli/info.rs::render_once`）

命中 alias：

```
Agent:
  claude --dangerously-skip-permissions -- 'feat: ...'  (via alias: claude-safe)

Alias:
  claude-safe = claude --dangerously-skip-permissions -- $prompt
```

字面量模板：

```
Agent:
  some-cmd --flag 'feat: ...'
```

`agent_cli = None`：保持现状，走 `Prompt:` 分支。

解析失败：`Agent:` + `  (failed to parse agent_cli: <err>)`，不附带 alias 标注或 Alias 段。

规则细则：
- 生效标注紧跟在 command 后，两个空格分隔：`  (via alias: <name>)`
- `Alias:` 段在命中时插入，前导一个空行与 `Agent:` 段分隔；未命中或 `None` / 解析失败分支都不出现
- `Alias:` 段单行：`  <name> = <template>`，`template` 原文输出，不替换 `$prompt`

### TUI（`tui_app/info.rs`）

- `InfoState` 增加 `agent_cli_alias: BTreeMap<String, String>` 字段，与 `agent_cli` 同时在 reload 时从 `global` 刷新
- `resolve_agent_or_prompt_display` 改为调用新签名 `build_agent_cli_display(tpl, &alias_map, ws)`，并返回 `(header, command_with_suffix, alias: Option<AliasInfo>)` 三元组给上层渲染
- Agent 行仍走 `push_wrapped_kv`：值传入 `format!("{}  (via alias: {})", cmd, name)`（命中时）或 `cmd`；标注不单独成行，跟随 command 一起按现有 `kv_content_width` 做换行
- 命中 alias 时，紧接 Agent 段后追加一个空行，然后一行：`push_wrapped_kv("Alias:", format!("{} = {}", name, template))`；也走现有 wrap
- 解析失败分支不变（现有错误提示照旧，且不追加 Alias 段）

## 调用点改造

- `src/cli/info.rs:99` 附近的 `build_agent_cli_display(global.agent_cli.as_deref(), ws)` 改为传 `&global.agent_cli_alias`，并根据返回的 `AgentDisplay.alias` 决定是否附加标注与 Alias 段
- `src/tui_app/info.rs`：
  - `InfoState` 增字段 `agent_cli_alias`
  - reload 逻辑同步填充该字段
  - `resolve_agent_or_prompt_display` 使用新 API，上层渲染负责追加标注与 Alias 段

## 测试

### `tests/agent_cli_test.rs`（追加）

- alias 命中：`global.agent_cli = "safe"`, `agent_cli_alias = {"safe" -> "claude -- $prompt"}` → `command` 含已展开 prompt，`alias = Some(AliasInfo { name: "safe", template: "claude -- $prompt" })`
- 字面量：`agent_cli = "claude -- $prompt"`，空 alias_map → `command` 正确展开，`alias = None`
- alias_map 非空但 tpl 未命中：`alias = None`，`command` 按字面量展开
- 错误路径：空 tpl → `Some(Err(..))`；alias 命中但 template 解析失败 → `Some(Err(..))`，错误信息关联到解析后的模板
- `None` 入参：返回 `None`，与 alias_map 内容无关

### `tests/info_render_test.rs`（新建或追加）

断言 `render_once` 的字符串输出：

- alias 命中 → 含 `(via alias: <name>)` 与 `Alias:` 段、段间有空行
- 字面量 → 既无标注也无 `Alias:` 段
- 解析失败 → 含 `(failed to parse agent_cli: ..)`，无标注、无 `Alias:` 段
- `agent_cli = None` → 走 `Prompt:` 分支，无 `Agent:`/`Alias:`

### `src/tui_app/info.rs`（TestBackend 快照，追加）

- alias 命中 → 帧缓冲中找到 `(via alias: <name>)` 与 `Alias:` 标签 + 单行内容
- 字面量 → 不含上述文本
- 命令超长 → Agent 标注随 command 一起被 wrap 到下一行，缩进对齐（用现有 `kv_content_width` 规则）

## 非目标

- 不实现多级 alias 解析与环检测
- 不列出所有已配置别名
- 不改变 `--run-agent` 路径行为
- 不改动 `resolve_agent_cli` 语义或签名
