# Configurable terminal multiplexer design

## 背景

`zootree` 当前默认通过 zellij 打开工作空间。配置、布局模板、启动和清理路径都直接使用 zellij 概念：

- 全局、workspace、template、repo 配置里有 `zellij` 分组。
- `src/core/zellij.rs` 封装 zellij session 操作。
- `src/cli/workspace.rs` 的 `start` / `open` / `done` / `cancel` 直接依赖 `ZellijOps`。
- 默认布局是 zellij KDL，路径是 `~/.config/zootree/layouts/<name>.kdl`。

新的目标是支持可配置的终端复用器。默认仍使用 zellij，但可以配置为 cmux。cmux 的模型是 workspace / pane / surface，不直接使用 zellij KDL，因此需要抽象生命周期接口，并为 cmux 增加 JSON layout 渲染。

项目仍处于开发期，不保留旧 `[zellij]` 配置兼容。所有相关配置迁移到新的 `[multiplexer]` 配置项。

## 目标

- 新增统一 multiplexer 配置，默认 `kind = "zellij"`，可配置为 `cmux`。
- 删除旧 `[zellij]` 配置入口、`session_mode` 和 `session_name` 语义。
- 将 zellij 启动、打开、关闭逻辑适配到通用 multiplexer 边界。
- 新增 cmux 实现，首版尽量还原当前 zellij 默认布局：overview、每个 repo 的 lazygit/shell 区域、agent 注入。
- 支持 cmux 自定义 JSON layout 模板，路径为 `~/.config/zootree/layouts/<name>.cmux.json`。
- 将 CLI 跳过参数从 `--no-zellij` 改为 `--no-multiplexer`。
- 为未来 tmux 或其他实现留下清晰扩展点。

## 非目标

- 不兼容读取旧 `[zellij]` 配置。
- 不支持 zellij shared session。所有实现都只支持 workspace 独占生命周期。
- 不把 zellij KDL 自动转换为 cmux JSON。
- 不实现 tmux。
- 不新增复杂插件注册系统；当前只需要 zellij 和 cmux 两个内置实现。
- 不改变 git worktree、hook、copy files、agent alias 的既有语义。

## 配置模型

新的全局配置示例：

```toml
workspace_root = "~/zootree-workspaces"
branch_prefix = "zootree"

[multiplexer]
kind = "zellij"

[multiplexer.zellij]
layout = "default"

[multiplexer.cmux]
layout = "default"
```

配置结构建议：

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MultiplexerKind {
    Zellij,
    Cmux,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MultiplexerConfig {
    #[serde(default)]
    pub kind: MultiplexerKind,
    #[serde(default)]
    pub zellij: ZellijMultiplexerConfig,
    #[serde(default)]
    pub cmux: CmuxMultiplexerConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ZellijMultiplexerConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layout: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CmuxMultiplexerConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layout: Option<String>,
}
```

`GlobalConfig`、`WorkspaceConfig`、`TemplateConfig`、`RepoConfig` 中原来的 `zellij` 字段迁移为 `multiplexer` 字段。未配置时默认：

- `kind = zellij`
- `multiplexer.zellij.layout = "default"`
- `multiplexer.cmux.layout = "default"`

旧字段处理：

- `[zellij]` 不再被读取。
- `session_mode` 不再是合法字段。
- `session_name` 不再是合法字段。
- 文档、示例、测试 fixture 全部迁移到 `[multiplexer]`。

## 模块边界

新增模块：

```text
src/core/multiplexer/
├── mod.rs
├── zellij.rs
└── cmux.rs
```

`src/core/zellij.rs` 的逻辑迁移到 `src/core/multiplexer/zellij.rs`，或先作为内部适配层保留，再由 `ZellijMultiplexer` 包装。最终 CLI 不应直接依赖 `ZellijOps`。

核心接口：

```rust
pub struct MultiplexerLaunch {
    pub workspace_name: String,
    pub display_name: String,
    pub workspace_dir: PathBuf,
    pub layout_name: String,
    pub rendered_layout: String,
    pub layout_file: PathBuf,
}

pub struct MultiplexerIdentity {
    pub workspace_name: String,
    pub display_name: String,
    pub cmux_workspace: Option<String>,
}

pub enum LaunchOutcome {
    Launched,
    Attached,
    AlreadyRunning,
    BackgroundCreated,
}

pub trait TerminalMultiplexer {
    fn kind(&self) -> MultiplexerKind;
    fn launch(&self, launch: &MultiplexerLaunch) -> Result<LaunchOutcome>;
    fn open(
        &self,
        launch: &MultiplexerLaunch,
        identity: &MultiplexerIdentity,
    ) -> Result<LaunchOutcome>;
    fn close(&self, identity: &MultiplexerIdentity) -> Result<()>;
}
```

`workspace.rs` 负责准备 workspace 数据、渲染布局、选择实现；multiplexer 实现负责调用外部 CLI。
`open` 同时接收 `MultiplexerLaunch` 和 `MultiplexerIdentity`，因为 cmux 找不到既有 workspace 时需要用同一份 layout 重新创建。

CLI 层变化：

- `launch_zellij` 改为 `launch_multiplexer`。
- `dispatch_launch` 从 zellij 专用函数下沉到 `ZellijMultiplexer`。
- `handle_open`、`handle_done`、`handle_cancel` 通过选中的 multiplexer 操作，不直接构造 zellij/cmux 命令。
- `StartArgs.no_zellij` 改为 `no_multiplexer`。

## Zellij 实现

zellij 继续保留当前 standalone session 行为：

- session 名固定为 `zootree-<workspace_name>`。
- 不再读取 `session_mode`。
- 不再读取 `session_name`。
- 外部终端中，如果 session 不存在，前台创建；如果存在，前台 attach。
- zellij 内部中，如果 session 不存在，后台创建；如果存在，输出提示。
- close 时调用 `zellij delete-session --force zootree-<workspace_name>`。

现有 `plan_launch(in_zellij, session_exists)` 决策矩阵仍适用，只是 session name 永远来自 workspace 名。

zellij layout 仍使用 KDL：

- 路径：`~/.config/zootree/layouts/<name>.kdl`
- 默认：`default.kdl`
- `default` 缺失时生成参考文件。
- 非 default 缺失时报错，不静默回退到 default。

## Cmux 实现

cmux 使用 workspace 模型：

- cmux workspace 名固定为 `zootree-<workspace_name>`。
- `launch` 使用 `cmux new-workspace --name <display_name> --cwd <workspace_dir> --layout <json> --focus true`。
- `open` 优先使用持久化 cmux workspace id/ref；缺失时按固定名称查找。
- `close` 优先关闭持久化 id/ref；缺失时按固定名称查找。查不到只 warning；查到多个则 warning 并跳过，避免误关用户工作区。

`WorkspaceConfig` 增加 multiplexer runtime state：

```rust
pub struct MultiplexerState {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<MultiplexerKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cmux_workspace: Option<String>,
}
```

启动 cmux 后，zootree 尝试从 cmux 输出或 `cmux tree --json` / `cmux list-workspaces` 中解析 workspace id/ref，并写回 `workspace.multiplexer_state.cmux_workspace`。如果解析失败，启动仍成功，但后续只能用固定名称兜底。

错误处理：

- `cmux` 不在 PATH 中时报错，提示安装 cmux 或切回 zellij。
- `new-workspace` 失败时报错并透出 stderr。
- socket 不可连接时，需要 socket 的查找/open/close 操作返回明确错误或 warning。
- 关闭时不做不确定匹配；宁可跳过，也不误关。

## Cmux JSON Layout

cmux layout 路径：

```text
~/.config/zootree/layouts/<name>.cmux.json
```

`layout = "default"` 时，如果文件不存在，生成默认参考文件。默认 layout 尽量贴近当前 zellij 默认布局：

- overview 区域运行 `zootree info <workspace_name> --watch`。
- 每个 repo 有 lazygit surface。
- 每个 repo 有 shell surface，cwd 为 worktree。
- `--run-agent` 保持当前语义：
  - 单 repo：agent 注入第一个 repo 区域。
  - 多 repo：agent 注入 overview 区域。

变量沿用 `$...` 语法：

- `$workspace_name`
- `$workspace_dir`
- `$repo_name`
- `$worktree_path`
- `$branch`
- `$lazygit_config`
- `$overview_agent_command`
- `$repo_agent_command`

cmux 的 agent 变量是完整 command string，不是 KDL fragment。命令构造复用现有 agent CLI 解析逻辑：先用 `shlex` 解析模板，替换 `$prompt`，再安全 join 成 command 字符串。

自定义 JSON 模板支持 repo 重复块：

```json
{
  "direction": "horizontal",
  "children": [
    {
      "pane": {
        "surfaces": [
          {
            "type": "terminal",
            "command": "zootree info $workspace_name --watch"
          },
          {
            "type": "terminal",
            "command": "$overview_agent_command",
            "cwd": "$workspace_dir"
          }
        ]
      }
    },
    {
      "zootree_repeat_per_repo": {
        "direction": "vertical",
        "children": [
          {
            "pane": {
              "surfaces": [
                {
                  "type": "terminal",
                  "command": "lazygit -p $worktree_path"
                }
              ]
            }
          },
          {
            "pane": {
              "surfaces": [
                {
                  "type": "terminal",
                  "cwd": "$worktree_path"
                },
                {
                  "type": "terminal",
                  "command": "$repo_agent_command",
                  "cwd": "$worktree_path"
                }
              ]
            }
          }
        ]
      }
    }
  ]
}
```

渲染规则：

- workspace 级变量替换一次。
- `zootree_repeat_per_repo` 节点按 repo 展开。
- 展开后移除 `zootree_repeat_per_repo` 包装字段。
- 如果某个 surface 的 `command` 替换后为空字符串，删除该 surface。
- 如果删除空 command surface 后 pane/surface 容器为空，删除空容器。
- 渲染后的 JSON 必须能被 `serde_json` 解析，再传给 `cmux new-workspace --layout`。

需要新增 `serde_json` 依赖，除非当前依赖树已有可直接使用的 JSON crate。

## 启动数据流

`zootree start`：

1. 创建 worktree 和 workspace 目录。
2. 保存 workspace config。
3. 如果没有 `--no-multiplexer`，读取 workspace/global/template/repo 的 multiplexer 配置。
4. 根据 kind 渲染对应 layout。
5. 调用 `TerminalMultiplexer::launch`。
6. 如果是 cmux 且拿到 workspace id/ref，更新 workspace config。

`zootree open`：

1. 加载 in-progress workspace。
2. 检查 worktree 存在性。
3. 优先使用 `workspace.multiplexer_state.kind`；缺失时使用当前配置 kind。
4. zellij 走 session attach/create 逻辑。
5. cmux 优先 select 持久化 workspace；缺失时按固定名称查找；仍缺失时重新 launch。

`zootree done` / `zootree cancel`：

1. 保持现有 merge、hook、cleanup 行为。
2. 归档后调用当前 workspace 的 multiplexer close。
3. `--no-clean` 不影响 multiplexer close，保持当前 zellij 关闭行为。

## 测试计划

配置测试：

- 空配置默认 `multiplexer.kind = zellij`。
- 解析 `[multiplexer] kind = "zellij"` 和 `[multiplexer.zellij] layout = "default"`。
- 解析 `[multiplexer] kind = "cmux"` 和 `[multiplexer.cmux] layout = "default"`。
- fixture 全部移除旧 `zellij` 字段。
- `--no-multiplexer` 能被 clap 解析。
- `--no-zellij` 不再是有效参数。

zellij 测试：

- 迁移现有 `tests/zellij_test.rs` 到新模块路径。
- 保留 `plan_launch` 四分支测试。
- 保留后台创建时 env_remove 断言。
- 保留 `delete-session --force` 断言。

cmux 测试：

- `launch` 调用 `cmux new-workspace --name zootree-<name> --cwd <dir> --layout <json> --focus true`。
- `close` 优先使用持久化 id/ref。
- `close` 缺失 id 时按固定名称查找。
- 查不到或查到多个时不调用 `close-workspace`。

layout 测试：

- 默认 cmux layout 是合法 JSON。
- repo repeat 能按 repo 数量展开。
- `$workspace_*` 和 `$repo_*` 变量正确替换。
- 单 repo 时 agent command 出现在 repo 区域。
- 多 repo 时 agent command 出现在 overview 区域。
- 空 agent command surface 被删除。
- 非 default cmux layout 文件缺失时报错。

CLI/集成测试：

- `start --no-multiplexer` 不调用 zellij/cmux。
- `open` 使用 workspace state 的 kind。
- `done` / `cancel` 通过 multiplexer close，且不直接依赖 zellij。
- 使用 `MockRunner` 和 temp config，避免真实 zellij/cmux。

文档和 skill：

- README / README.zh-CN 改为 "terminal multiplexer"。
- 依赖列表从 Zellij-only 改为 Git + zellij/cmux 二选一 + LazyGit 可选。
- `skills/zootree-dev/SKILL.md` 更新项目架构、配置约定、核心设计模式。
- `skills/zootree-usage/SKILL.md` 更新使用和配置示例。

## 验收标准

- 默认配置下，zootree 继续使用 zellij，现有 standalone 启动/打开/关闭行为保持。
- 配置 `kind = "cmux"` 后，`start` / `open` / `done` / `cancel` 使用 cmux 实现。
- `--no-multiplexer` 跳过 zellij/cmux 启动。
- 旧 `--no-zellij` 不再出现在 CLI help 中。
- 旧 `[zellij]` 不再出现在文档、默认配置、测试 fixture 中。
- zellij shared session 逻辑被移除。
- cmux 默认 layout 可以为每个 repo 创建对应区域，并支持 agent 注入。
- cmux 自定义 layout 文件使用 `layouts/<name>.cmux.json`，变量和 repeat 块按设计渲染。
- cmux close 不会在无法唯一识别 workspace 时误关其他 workspace。
- `cargo test` 通过。
