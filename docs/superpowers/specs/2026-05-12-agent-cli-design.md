# `agent_cli` 配置与 `zootree start --run-agent` 设计

- 日期：2026-05-12
- 作者：weineel + Claude
- 状态：已通过 brainstorming，待 writing-plans

## 背景与目标

`zootree start` 启动 zellij 后，用户目前需要手动在某个 pane 里敲 `claude --dangerously-skip-permissions -- "<workspace 描述>"` 之类的命令才能让 coding agent 接入 workspace 上下文。这个操作每次都要从 workspace 信息里复制粘贴 title / description，枯燥且易错。

本次新增：

1. **配置**：`GlobalConfig` 增加可选 `agent_cli` 字段，让用户配置一次启动 agent 的命令模板。
2. **CLI**：`zootree start` 新增 `--run-agent` flag，启用后在指定 pane 自动执行 `agent_cli`，prompt 由 workspace 的 `title` 和 `description` 组装。
3. **Layout**：在 `LayoutVar` 和 `LayoutRenderer` 中引入 `$overview_agent_cli` / `$repo_agent_cli` 占位符，默认 layout 把它们放在"最后一个 pane"的位置，自定义 layout 用户可自由选择写不写。

### 非目标（YAGNI）

- 不支持 per-repo 的 agent_cli（全局一个即可）
- 不支持同时跑多个不同 agent
- 不支持交互式选择 prompt 内容（永远用 title + description）
- 不支持 `$prompt` 之外的其它 agent_cli 占位符（如 `$title` / `$description` 单独替换）
- 本次 `zootree open` 不支持 `--run-agent`（单独迭代再谈；`build_agent_cli_kdl` 已为复用预留）
- 不支持 agent 退出后自动重启或通知

## 核心决策

| 决策点 | 选定方案 | 备注 |
|--------|----------|------|
| `agent_cli` 解析方式 | `shlex::split` 拆成 argv，`$prompt` 是其中一个 token | 无 shell 注入风险；prompt 含特殊字符天然安全 |
| prompt 占位符命名 | `$prompt` | 与项目里 `$workspace_name` 等现有 layout 变量风格一致 |
| prompt 内容 | `title` / `title + "\n" + description` | 复用 `handle_done` 的 commit message 风格（`src/cli/workspace.rs:705-709`）|
| layout 中 agent pane 标记方式 | 不标记；用 `$overview_agent_cli` / `$repo_agent_cli` 两个独立占位符 | 用户想在哪 pane 跑就在哪写占位符；零魔法标记 |
| 未开 `--run-agent` 时的行为 | 占位符被替换成空串，layout 合法性不变 | 不影响现有用户 |
| 1 repo vs ≥2 repo 行为差异 | 1 repo 在 repo tab 最后 pane；≥2 repo 在 overview tab 最后 pane | 对单 repo 场景减少 tab 切换；多 repo 场景避免不知道选哪个 repo |

## 架构与文件改动

### 依赖

`Cargo.toml` 新增：

```toml
shlex = "1"
```

（精确版本号在 writing-plans 阶段锁死。）

### 修改文件

| 文件 | 改动 |
|------|------|
| `src/config/global.rs` | `GlobalConfig` 增加 `pub agent_cli: Option<String>` 字段 |
| `src/core/layout.rs` | `LayoutVar` 增加 `overview_agent_cli: String` / `repo_agent_cli: String` 两个字段；`replace_vars` 增加对应两行替换；新增 `build_agent_cli_kdl(agent_cli_tpl, prompt) -> Result<String>` helper；`default_layout()` 在 overview 最后 pane 和每个 repo tab 的 70% pane 末尾插入对应占位符 |
| `src/cli/workspace.rs` | `StartArgs` 增加 `pub run_agent: bool`；`handle_start` 把 `args` 透传给 `launch_zellij`；`launch_zellij` 按规则表构造 `overview_kdl` / `repo_kdl_for_first` 并分发到 `LayoutVar` |
| `tests/layout_test.rs` | 补 agent_cli 空串 / 有值两条渲染断言 |
| `tests/agent_cli_test.rs` | **新增**，纯函数覆盖 `build_agent_cli_kdl` 和 `build_prompt` |
| `tests/start_agent_test.rs` | **新增**，集成测试不真跑 zellij，只读 `recently.kdl` 验证内容 |

## 详细设计

### GlobalConfig 字段

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GlobalConfig {
    // ...已有字段...
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_cli: Option<String>,
}
```

`Default` 返回 `None`；老配置文件无此字段可正常加载。

配置示例：

```toml
agent_cli = "claude --dangerously-skip-permissions -- $prompt"
```

### CLI 参数

```rust
#[derive(Args)]
pub struct StartArgs {
    // ...已有字段...
    #[arg(long, help = "Launch configured agent_cli in the designated pane")]
    pub run_agent: bool,
}
```

### prompt 构造

```rust
fn build_prompt(workspace: &WorkspaceConfig) -> String {
    if workspace.description.is_empty() {
        workspace.title.clone()
    } else {
        format!("{}\n{}", workspace.title, workspace.description)
    }
}
```

### `build_agent_cli_kdl`

新增在 `src/core/layout.rs`：

```rust
pub fn build_agent_cli_kdl(agent_cli_tpl: &str, prompt: &str) -> anyhow::Result<String>;
```

流程：

1. `shlex::split(agent_cli_tpl)` → `Option<Vec<String>>`
   - `None`（解析失败，比如未闭合引号）→ `bail!("failed to parse agent_cli: {}", agent_cli_tpl)`
   - `Some(vec)` 但 `vec.is_empty()` → `bail!("agent_cli is empty")`
2. 遍历 tokens，对每个 token 做 `token.replace("$prompt", prompt)`（允许 `$prompt` 既可作为独立 token，也可嵌在 token 里，如 `--prompt=$prompt`）
3. 第 0 个 token 是 command，其余是 args
4. 对 command 和每个 arg 做 kdl 字符串转义：

   ```rust
   fn kdl_escape(s: &str) -> String {
       s.replace('\\', r"\\")
        .replace('"', r#"\""#)
        .replace('\n', r"\n")
        .replace('\r', r"\r")
        .replace('\t', r"\t")
   }
   ```

5. 输出：

   ```
   command="<escaped command>" {
       args "<escaped arg1>" "<escaped arg2>" ...
   }
   ```

   若 `args` 为空，省略整个 `{ args ... }` 块，只返回 `command="..."`。

### `LayoutVar` 与 `replace_vars`

```rust
pub struct LayoutVar {
    pub repo_name: String,
    pub worktree_path: String,
    pub branch: String,
    pub workspace_name: String,
    pub workspace_dir: String,
    pub lazygit_config: String,
    pub overview_agent_cli: String,  // 空串或 kdl 片段
    pub repo_agent_cli: String,      // 空串或 kdl 片段
}
```

`replace_vars` 在现有替换链末尾追加：

```rust
result = result.replace("$overview_agent_cli", &vars.overview_agent_cli);
result = result.replace("$repo_agent_cli", &vars.repo_agent_cli);
```

顺序保证：先替所有标准变量（`$workspace_name` 等），最后替 agent_cli 占位符，避免 agent_cli 内容（可能含 `$workspace_name` 之类字面量）被二次替换。

分 overview / repo 两个字段的原因：`LayoutRenderer::render` 在 overview 段（`before_marker` / `after_tab`）用 `repos.first()` 的 vars 做替换，repo 段在 `repos.iter()` 循环里每个 repo 用自己的 vars。同一个字段两处用会冲突，所以显式分开。

### `launch_zellij` 规则表实现

```rust
let (overview_kdl, repo_kdl_for_first) = if args.run_agent {
    let agent_cli_tpl = global.agent_cli.as_deref()
        .ok_or_else(|| anyhow::anyhow!(
            "--run-agent requires agent_cli in global config"
        ))?;
    let prompt = build_prompt(workspace);
    let kdl = build_agent_cli_kdl(agent_cli_tpl, &prompt)?;
    if workspace.repos.len() == 1 {
        (String::new(), kdl)
    } else {
        (kdl, String::new())
    }
} else {
    (String::new(), String::new())
};
```

构造 `Vec<LayoutVar>` 时：

- 所有 `LayoutVar` 实例的 `overview_agent_cli` 都设成 `overview_kdl`（`render` 只用 `repos.first()` 的值，但设成一致值更稳）
- 第 0 个 repo 的 `repo_agent_cli = repo_kdl_for_first`，其余 repo 的 `repo_agent_cli = String::new()`

规则表：

| `--run-agent` | repo 数量 | overview pane | 每个 repo tab 最后 pane |
|---|---|---|---|
| false | 任意 | 空（正常 shell） | 空（正常 shell） |
| true | 1 | 空 | 跑 agent |
| true | ≥2 | 跑 agent | 空 |

### 默认 layout 改动

`default_layout()` 中 overview tab 最后一个 pane：

```kdl
tab name="overview" {
    pane size=1 borderless=true { plugin location="tab-bar" }
    pane split_direction="vertical" {
        pane command="zootree" { args "info" "$workspace_name" "--watch" }
        pane cwd="$workspace_dir" $overview_agent_cli
    }
    pane size=1 borderless=true { plugin location="status-bar" }
}
```

repo tab 内层 70% pane：

```kdl
pane {
    pane size="30%" cwd="$worktree_path"
    pane size="70%" cwd="$worktree_path" $repo_agent_cli
}
```

不启用 agent 时，占位符被替换成空串，尾部留一个空格不影响 kdl 合法性；启用时，`$xxx_agent_cli` 变成 `command="..." { args "..." }`，`command=` 是 pane 的属性、后面 `{ args ... }` 是子节点，合法。

### 边界与错误处理

- `--run-agent` 但 `agent_cli` 未配置 → `bail!`（`launch_zellij` 里）
- `agent_cli` 是空串或 shlex 拆分出空 tokens → `bail!`（`build_agent_cli_kdl` 里）
- `shlex::split` 返回 `None`（未闭合引号等）→ `bail!`
- `--run-agent` 但 layout 里没出现 `$overview_agent_cli` 也没有 `$repo_agent_cli` → `tracing::warn!("--run-agent set but layout contains no $agent_cli placeholder")`，不 bail，继续启动
- `--run-agent` 未设置但 layout 含占位符 → 替换成空串，静默

## 测试计划

### 纯函数单测（`tests/agent_cli_test.rs`）

- `build_agent_cli_kdl("claude -- $prompt", "hello")` → `command="claude" { args "--" "hello" }`
- prompt 含 `"` → escape 为 `\"`
- prompt 含 `\` → escape 为 `\\`
- prompt 含 `\n` → escape 为 `\n`（字面反斜杠 n）
- `build_agent_cli_kdl("gemini chat", "ignored")` → `command="gemini" { args "chat" }`（无 `$prompt` 占位符、prompt 被忽略、允许）
- `build_agent_cli_kdl("", _)` → error
- `build_agent_cli_kdl("claude \"unclosed", _)` → error（shlex 解析失败）
- `build_agent_cli_kdl("claude", _)` → `command="claude"`（无 args 块）
- `build_prompt` 两分支：description 空 / 非空

### layout render 测试（扩 `tests/layout_test.rs`）

- `agent_cli` 字段为空串时，`default_layout()` 渲染结果合法 kdl、语义不变（即和当前版本结果一致）
- `agent_cli` 字段有值时，渲染结果在正确位置含 `command="claude"`（overview 或 repo pane）

### 集成测试（`tests/start_agent_test.rs`）

不真跑 zellij，只读 `~/.config/zootree/layouts/recently.kdl` 验证内容。测试用例：

- `--run-agent` + `agent_cli` 未配置 → 非零退出 + 错误消息指向 global config
- `--run-agent` + 1 repo → `recently.kdl` 里 repo tab 的 70% pane 含 agent command，overview 最后 pane 不含
- `--run-agent` + ≥2 repo → overview 最后 pane 含 agent command，各 repo tab 不含
- 不带 `--run-agent` → 两处都不含 agent command
- `agent_cli` 含 `$prompt` 嵌入式（`--prompt=$prompt`）→ 正确替换

## 风险与权衡

| 风险 | 缓解 |
|------|------|
| kdl 转义漏洞：prompt 含 `"` / `\` 破坏 kdl 语法 | `build_agent_cli_kdl` 内部逐字符转义（`\` → `\\`、`"` → `\"`、`\n` → `\n` 等），并写单测覆盖 |
| shlex 解析失败（用户 agent_cli 有未闭合引号） | 启动时立即 `bail!`，错误消息指向 `global.agent_cli` |
| 用户自定义 layout 不含 `$agent_cli` 占位符 | 渲染完成后检测，不含则 `tracing::warn!` 但继续，不 bail（用户可能"先开 flag 再改 layout"） |
| prompt 内容含 `$workspace_name` 等字面量被 `replace_vars` 二次替换 | `replace_vars` 执行顺序：先替所有标准变量，最后替 `$overview_agent_cli` / `$repo_agent_cli`，保证 agent_cli 内容不再过 replace 流程 |
| `LayoutRenderer::render` 的 overview 段只用 `repos.first()` 的 vars | 构造 `Vec<LayoutVar>` 时所有实例的 `overview_agent_cli` 设同一值，保证 overview 段替换稳定 |
| 老用户升级后 default layout 自动被覆盖引入新占位符 | `write_default_layout` 每次 start 覆盖，文件头"自动生成修改无效"注释已提示；自定义 layout 用户需在 CHANGELOG 中得到手动加 `$agent_cli` 提示 |
| `description` 过长导致 agent 命令行超限 | shell 平台通常 argv 长度上限在 128KB 以上；workspace description 一般远小于此；本次不做长度限制，未来有反馈再加 |

## 待 writing-plans 阶段细化

- `shlex` crate 精确版本号
- 错误文案具体措辞（"agent_cli not configured" vs "agent_cli is required with --run-agent"）
- `default_layout()` 里 `$xxx_agent_cli` 占位符前的空格 / 换行策略（保证替换成空串后 kdl 仍整洁，比如替换前 `pane cwd="x" $overview_agent_cli` 替换后是否要去掉尾部空格）
- `tracing::warn!` 的具体消息内容
