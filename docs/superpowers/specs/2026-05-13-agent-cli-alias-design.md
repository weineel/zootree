# `agent_cli_alias` 与 `--run-agent` 接收值设计

- 日期：2026-05-13
- 作者：weineel + Claude
- 状态：已通过 brainstorming，待 writing-plans
- 前置：[2026-05-12-agent-cli-design.md](2026-05-12-agent-cli-design.md)

## 背景与目标

当前 `zootree start --run-agent` 是个 bool flag，启用后只能用 `GlobalConfig.agent_cli` 这一个全局命令模板。用户在不同场景下想切换 agent（如 Claude / Gemini / Codex，或带不同权限/参数的 Claude）只能反复改 `~/.config/zootree/config.toml`，体验差。

本次新增：

1. **配置**：`GlobalConfig` 增加 `agent_cli_alias: BTreeMap<String, String>`，让用户预先注册多个有名字的 agent_cli 模板。
2. **CLI**：`--run-agent` 从 `bool` 升级为 `Option<Option<String>>`，可选地接收一个 value。Value 优先按 alias 名查找，未命中则当作字面量 agent_cli 命令。
3. **解析统一**：`agent_cli` 字段本身也走同一套 alias 解析（一层），用户可以把它配置成某个 alias 的 key。
4. **补全**：`--run-agent` 支持动态补全 alias 名，匹配 `agent_cli` 字段的那条用 `(default)` 标记。

### 非目标（YAGNI）

- 不支持递归 alias（`a → b → c` 这种链式），仅一层查找。
- 不在 alias 解析失败时给警告或建议（"did you mean..."）。
- 不增加 `--agent` 之类的额外 flag（保持 `--run-agent` 单一入口）。
- 不在 alias map 中存放除模板字符串以外的元数据（不引入 `[agent_cli_alias.foo]` 嵌套）。
- 不为 `zootree open` 加 `--run-agent`（仍属上一份 spec 的非目标，本次不变）。
- alias 模板的语法不变（仍只支持 `$prompt` 占位符 + `shlex::split`）。

## 核心决策

| 决策点 | 选定方案 | 备注 |
|--------|----------|------|
| 配置数据结构 | 保留 `agent_cli: Option<String>`，新增独立 top-level `agent_cli_alias: BTreeMap<String, String>` | 向后兼容；BTreeMap 保证序列化与补全顺序稳定 |
| `agent_cli` 字段语义 | 字符串值会先在 alias 表中查找，命中则等价于该 alias 的模板 | 与 `--run-agent <value>` 解析路径完全一致 |
| Alias 解析层级 | 一层，命中则返回 alias 模板，未命中返回原字符串 | 避免环依赖，简化心智模型 |
| `--run-agent` 语法 | `Option<Option<String>>` + `num_args = 0..=1` + `default_missing_value = ""` | 同时接受 `--run-agent`、`--run-agent foo`、`--run-agent=foo` |
| 未命中 alias 行为 | 直接当字面量 agent_cli 命令传给 zellij，不警告 | 与用户初衷一致；错误由 shell 在执行时暴露 |
| 补全候选 | alias 名 + help 中标记 `(default)`（当 `agent_cli` 字符串值等于该 alias 名时） | 不引入虚拟 `default` 候选项 |
| `(default)` 判定 | `global.agent_cli == Some(alias_name)` 即标记 | 当 `agent_cli` 是字面量字符串而非 alias 名时，列表中没有任何项被标记 |

## 数据模型

`src/config/global.rs`：

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GlobalConfig {
    // ... 既有字段 ...
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_cli: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub agent_cli_alias: BTreeMap<String, String>,
}
```

`Default::default()` 中：`agent_cli_alias: BTreeMap::new()`。

**示例 config.toml**：

```toml
agent_cli = "claude"   # 引用 alias "claude"

[agent_cli_alias]
claude = "claude --dangerously-skip-permissions -- $prompt"
claude-safe = "claude -- $prompt"
gemini = "gemini chat -- $prompt"
codex = "codex --skip-confirm -- $prompt"
```

也允许 `agent_cli` 是字面量：

```toml
agent_cli = "claude --dangerously-skip-permissions -- $prompt"

[agent_cli_alias]
gemini = "gemini chat -- $prompt"
codex = "codex --skip-confirm -- $prompt"
```

## CLI 行为

`StartArgs.run_agent` 类型变更：

```rust
#[arg(
    long,
    num_args = 0..=1,
    default_missing_value = "",
    value_name = "ALIAS_OR_CMD",
    help = "Launch agent_cli in the designated pane (alias name or literal command)",
    add = ArgValueCompleter::new(|c: &OsStr| complete_agent_cli_alias(c)),
)]
pub run_agent: Option<Option<String>>,
```

clap 解析三态：

| 命令 | `run_agent` 值 | 含义 |
|------|----------------|------|
| `start ws` | `None` | 不启动 agent |
| `start ws --run-agent` | `Some(Some(""))` | 用 `agent_cli` 默认 |
| `start ws --run-agent claude-safe` | `Some(Some("claude-safe"))` | 走 alias / 字面量解析 |
| `start ws --run-agent="codex --skip -- $prompt"` | `Some(Some("codex --skip -- $prompt"))` | 字面量 |

注：`default_missing_value = ""` 让 bare `--run-agent` 落到空字符串分支，解析逻辑统一。

## 解析流程

新增 `src/core/layout.rs::resolve_agent_cli`：

```rust
pub fn resolve_agent_cli<'a>(
    value: &'a str,
    alias_map: &'a BTreeMap<String, String>,
) -> &'a str {
    alias_map.get(value).map(String::as_str).unwrap_or(value)
}
```

`launch_zellij`（`src/cli/workspace.rs`）改写处理逻辑：

```rust
let agent_cli_tpl: Option<String> = match args.run_agent.as_ref() {
    None => None,
    Some(value) => {
        let raw = match value.as_deref() {
            Some(s) if !s.is_empty() => s,
            _ => global.agent_cli.as_deref().ok_or_else(|| {
                anyhow::anyhow!(
                    "--run-agent requires agent_cli in global config (~/.config/zootree/config.toml)"
                )
            })?,
        };
        let resolved = resolve_agent_cli(raw, &global.agent_cli_alias);
        Some(resolved.to_string())
    }
};
```

需注意：
- `agent_cli` 自身的 alias 解析也通过同一个 `resolve_agent_cli` 路径（因为 `raw = global.agent_cli` 时同样经过 resolve）。
- `launch_zellij` 内部下游逻辑不变：拿到模板字符串后仍走 `build_agent_cli_kdl(tpl, &prompt)` → `LayoutVar` → `LayoutRenderer`。
- `launch_zellij` 形参 `run_agent: bool` 改为 `run_agent: Option<Option<String>>`。
- `template_content.contains("$overview_agent_cli")` 那段未占位提示的判断，改为「`run_agent.is_some()`」即触发。

## 错误处理 / 边界

| 场景 | 行为 |
|------|------|
| `--run-agent` (bare) + `agent_cli = None` | 报错（同今天） |
| `--run-agent foo`，alias 中无 `foo`，`agent_cli = None` | 不报错，把 `foo` 作字面量 |
| `--run-agent foo`，alias 中有 `foo` | 用 alias 模板，**不依赖** `agent_cli` |
| 模板中无 `$overview_agent_cli` / `$repo_agent_cli` | 沿用现有 warning 行为 |
| `agent_cli = "x"`，alias 中无 `x` | `agent_cli` 当字面量命令 `x` |
| `agent_cli_alias` 字段缺失 | `serde(default)` 给空 map |
| Alias 模板自身解析失败（`shlex::split`） | 由 `build_agent_cli_kdl` 报错（既有行为） |

## 动态补全

新增 `src/core/completers.rs::complete_agent_cli_alias_with` / `complete_agent_cli_alias`：

```rust
pub fn complete_agent_cli_alias_with(
    mgr: &ConfigManager,
    current: &OsStr,
) -> Vec<CompletionCandidate> {
    let prefix = current.to_string_lossy();
    let Ok(global) = mgr.load_global_config() else {
        return vec![];
    };
    let default_tpl = global.agent_cli.as_deref();

    global
        .agent_cli_alias
        .iter()
        .filter(|(name, _)| name.starts_with(prefix.as_ref()))
        .map(|(name, tpl)| {
            let is_default = default_tpl == Some(name.as_str());
            let help = if is_default {
                format!("(default) {}", tpl)
            } else {
                tpl.clone()
            };
            CompletionCandidate::new(name).help(Some(help.into()))
        })
        .collect()
}

pub fn complete_agent_cli_alias(current: &OsStr) -> Vec<CompletionCandidate> {
    let Ok(mgr) = ConfigManager::new() else {
        return vec![];
    };
    complete_agent_cli_alias_with(&mgr, current)
}
```

约定：
- 失败返回 `vec![]`（沿用 `complete_workspace` / `complete_repo` 模式）。
- `BTreeMap` 迭代天然按 key 字母排序，无需额外 sort。
- `(default)` 仅在 `agent_cli` 字符串值恰好等于某 alias 名时出现一次；字面量 / `None` 时不出现。

## 测试计划

### `tests/start_agent_test.rs`（扩展）

1. `run_agent_with_alias_resolves_template` — alias = `{"safe": "claude -- $prompt"}`，`Some(Some("safe"))` 渲染含 `claude` 命令。
2. `run_agent_with_unknown_alias_falls_back_to_literal` — alias = `{"safe": "..."}`，`Some(Some("gemini chat -- $prompt"))` 渲染含 `gemini`。
3. `run_agent_bare_uses_global_agent_cli` — `agent_cli = "claude -- $prompt"`、空 alias、`Some(Some(""))` 渲染含 `claude`。
4. `run_agent_alias_template_takes_precedence_over_agent_cli` — `agent_cli = "foo -- $prompt"`、alias = `{"bar": "bar -- $prompt"}`、`Some(Some("bar"))` 渲染 `bar` 而非 `foo`。
5. 现有 `run_agent_without_agent_cli_errors` 保留，确认 `Some(Some(""))` + `agent_cli = None` 仍报错。

### `tests/agent_cli_test.rs`（新增 resolve 测试）

6. `resolve_returns_alias_value_when_key_matches`
7. `resolve_returns_input_when_key_missing`
8. `resolve_returns_input_with_empty_alias_map`
9. `resolve_does_not_chain_aliases` — `{"a": "b", "b": "real"}`，resolve("a") == "b"。

### `tests/config_test.rs`（扩展）

10. `agent_cli_alias_loads_from_toml` — 反序列化含 `[agent_cli_alias]` 表。
11. `agent_cli_alias_default_empty` — 缺字段时为空 BTreeMap。
12. `agent_cli_alias_empty_not_serialized` — 空 map 不写回 toml。

### `tests/completions_test.rs`（扩展）

13. `completes_alias_names_no_prefix` — 三个 alias 按字母序返回。
14. `completes_alias_names_with_prefix` — 前缀过滤。
15. `completes_marks_default_when_agent_cli_matches_alias_key` — `agent_cli = "claude"` + alias 含 `claude` → 该项 help 以 `(default)` 开头。
16. `completes_no_default_marker_when_agent_cli_is_literal` — `agent_cli = "claude -- $prompt"` + alias 不含此 key → 没有任何 default 标记。
17. `completes_empty_when_no_alias_configured` — 空 map → `vec![]`。

### 测试模式

- 沿用 `MockRunner` + `ConfigManager::with_base_dir(temp)`。
- 补全测试用 `complete_agent_cli_alias_with(mgr, current)` 注入临时 `ConfigManager`。
- `start_agent_test.rs` 中已有的 helper `render_agent_cli` 需要扩展接受 `alias_map: &BTreeMap<String, String>` + `run_agent: Option<Option<String>>` 形参。

## 文档与 Skill

- **README.md / README.zh-CN.md**：在 `agent_cli` 章节补充 alias 用法、`--run-agent <ALIAS>` / `--run-agent="literal"` 示例 + 补全行为说明。
- **`skills/zootree-usage`**：检查 `agent_cli` / `--run-agent` 描述，加 alias 概念、补全行为。
- **`.claude/skills/zootree-dev`**：本次仅修改 `GlobalConfig` 字段、新增一个 completer 函数、扩展 CLI 参数 — 现有「常见开发任务」章节已覆盖这些模式。检查后若内容仍准确则不必修改。
- **KDL layout 模板**：不变（`default.kdl` 继续用 `$overview_agent_cli` / `$repo_agent_cli` 占位符）。

## 兼容性

- 现有 `agent_cli = "..."` 单字符串配置无需修改即可继续工作（alias 表为空时 resolve 是恒等映射）。
- 现有 `--run-agent` 用法（bare flag）行为不变。
- 新字段 `agent_cli_alias` 用 `#[serde(default)]`，旧 config 反序列化无障碍。
- `launch_zellij` 形参类型变更只影响内部调用者，无外部 API 变化。

## 风险与权衡

- **未命中 alias 不警告**：用户拼错 alias 名时启动后才会发现命令不存在。这是用户明确选择的行为（与"灵活把任意字面量当 agent_cli"诉求一致）。
- **`--run-agent` greedy 吃后续 token**：`zootree start --run-agent ws` 会把 `ws` 当成 alias 值，positional `name` 留空进 interactive picker。建议用户把 `--run-agent` 放命令最后；README 示例统一用这种顺序。
- **`(default)` 标记歧义**：当 `agent_cli` 是字面量且巧合等于某 alias 名（如 `agent_cli = "claude"` 且有 `agent_cli_alias.claude`）时，会被识别为 default — 这其实就是用户配置的语义（一层 alias），不算 bug。
