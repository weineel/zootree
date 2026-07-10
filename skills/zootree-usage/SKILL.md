---
name: zootree-usage
description: >
  Use when users mention zootree, workspace 工作空间, worktree, 多仓库管理,
  zellij/cmux, lazygit, run-agent, config.toml, Hook 脚本, 布局模板,
  或需要创建、启动、打开、完成、取消 zootree 工作空间（任务）。
---

# zootree 使用指南

zootree 是基于 Git Worktree + 终端复用器（推荐 cmux，Zellij 作为兼容默认值）+ LazyGit 的多仓库协作工作空间管理工具。

## 核心概念

zootree 管理「工作空间」—— 一个工作空间包含多个仓库在同一个分支名上工作。
状态流转: `pending` → `in_progress` → `done` / `canceled`

## Agent 执行规则

替用户执行 zootree 命令时，默认使用非交互参数；不要把 TUI 留给 agent 现场填写。

| 场景 | 必须预填 |
|------|----------|
| `create` | `--title`，以及 `--repos` 或 `--template`；启动 agent 时还要把任务 prompt 放进 `--description` |
| `start/open/done/cancel` | workspace name |
| `repo edit/remove` | repo name |

- `zootree create` 只有在同时给出 `--title` 和 repo 来源时才不会进入 wizard；repo 来源是 `--repos repo:branch,...` 或 `--template name`。缺 `--title` 时即使传了 `--run-agent` 也会进入 TUI，不能直接启动执行。
- 用带 `--run-agent codexd` 的 `zootree create` 命令分派开发任务时，必须同时传 `--title`、`--description` 和 repo 来源；`--run-agent` 会隐含 start，但不会补齐 create 所需的非交互字段。
- 默认给 `--name`，避免随机 workspace 名导致后续 `start/done/info` 不好引用。
- 任务标题必须使用 `cz-conventional-changelog` 规范：`<type>(<scope>): <subject>`。
- `--title` 只放一行短标题；给 agent 的详细任务 prompt 放到 `--description`。agent prompt 会由 title 和 description 组合而成，所以不要把长 prompt 塞进 `--title`。
- 实际执行命令前必须把 `<repo-root>`、`<repo-name>`、`<current-branch>`、`<workspace-name>` 这类占位符替换成真实值；不要把占位符原样交给 shell。
- 若当前 git repo 尚未注册，先用 `zootree repo add <repo-root> --name <repo-name> --default-target-branch <current-branch>` 非交互注册，再 `create --repos <repo-name>:<current-branch>`。
- `zootree start` 的 `--run-agent` 要放在 workspace name 后面：`zootree start <ws> --run-agent <alias>`。`zootree start --run-agent <ws>` 会把 `<ws>` 当 agent alias，仍可能进入 workspace 选择。

非交互创建模板：

```bash
zootree create \
  --title "feat(xxx): implement file preview" \
  --description "$(cat <<'EOF'
Implement the file preview feature.

Scope:
- Add preview routing for supported file types.
- Keep existing fallback behavior unchanged.

Expected verification:
- cargo test
EOF
)" \
  --name sample-name \
  --branch zootree/sample-name \
  --repos f-hiro:feature/xxx \
  --run-agent codexd
```

如果只需要创建 pending workspace，不启动 agent：

```bash
zootree create \
  --title "feat(xxx): implement file preview" \
  --description "Implement the file preview feature." \
  --name sample-name \
  --branch zootree/sample-name \
  --repos f-hiro:feature/xxx
```

## 安装

CI/CD 配置会发布 GitHub Release shell installer、Homebrew formula 和 crates.io package。普通用户优先使用预编译安装方式，开发当前 checkout 时才用 `cargo install --path .`。

推荐安装 zootree CLI：

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/weineel/zootree/releases/latest/download/zootree-installer.sh | sh
```

macOS / Homebrew：

```bash
brew install weineel/tap/zootree
```

Rust / crates.io：

```bash
cargo install zootree --locked
```

本地源码 checkout：

```bash
cargo install --path .
```

前置依赖: Git、cmux（推荐）或 Zellij、LazyGit（可选）

## 命令参考

### 仓库管理

```bash
# 指定路径添加
zootree repo add ~/projects/myrepo

# 指定名称和默认目标分支
zootree repo add ~/projects/myrepo --name myrepo --default-target-branch develop

# 列出所有仓库
zootree repo list

# 编辑仓库配置（会用 $EDITOR 打开）
zootree repo edit myrepo

# 移除仓库
zootree repo remove myrepo
```

### 工作空间操作

**创建** - 工作空间创建后状态为 `pending`

```bash
# Agent 执行时使用完整参数，避免进入 wizard
zootree create --title "feat(other): 新功能开发" --description "实现新功能开发任务" --name new-feature --repos frontend:feature/abc,backend:feature/abc

# 创建并启动开发 agent；description 是传给 agent 的任务 prompt
zootree create --title "feat(other): 新功能开发" --description "实现新功能开发任务" --name new-feature --repos frontend:feature/abc --run-agent codexd

# 指定分支名、名称或使用模板
zootree create --title "feat(other): 新功能开发" --description "实现新功能开发任务" --branch my-feature --name my-ws --template my-template
```

**启动** - 创建工作树、执行 hook、启动已配置的终端复用器

```bash
# 指定名称
zootree start my-workspace

# 指定名称并启动 agent；--run-agent 放在 workspace name 后面
zootree start my-workspace --run-agent claude-safe
```

**查看** - 列出工作空间

```bash
zootree list
zootree list --status in_progress  # pending|in_progress|done|canceled
```

**打开** - 重新打开已启动的工作空间（使用已配置的终端复用器）

```bash
zootree open my-workspace
```

**完成** - 合并分支、清理工作树、归档

```bash
zootree done my-ws
zootree done my-ws --push           # 合并后推送
zootree done my-ws --no-merge       # 跳过合并
zootree done my-ws --no-clean       # 跳过清理
zootree done my-ws --strategy squash  # 合并策略: squash/rebase/merge
```

**取消** - 清理工作树、删除分支、归档

```bash
zootree cancel my-ws
zootree cancel my-ws --no-clean     # 不清理 worktree
zootree cancel my-ws --force        # 强制取消
```

### 模板管理

```bash
zootree template list              # 列出所有模板
zootree template save <name> --from <workspace>
                                   # 将 workspace 保存为模板
```

每次创建工作空间时会自动保存为 `recently` 模板，方便下次使用。

### 维护工具

```bash
zootree prune    # 清理孤立的 worktree
zootree logs     # 查看日志文件
```

## 配置文件详解

### 全局配置 (~/.config/zootree/config.toml)

```toml
workspace_root = "~/zootree-workspaces"
branch_prefix = "zootree"
copy_files = [".env"]

[multiplexer]
kind = "cmux"

[multiplexer.zellij]
layout = "default"

[multiplexer.cmux]
layout = "default"

[hooks]
post_create = "echo created"
post_start = "echo started"
pre_done = "echo cleaning up"
pre_cancel = "echo canceled"
pre_remove = "echo removing"

[log]
max_files = 5
max_size = "10MB"
```

未配置 `[multiplexer].kind` 时 zootree 保持兼容默认值 `zellij`；新配置推荐显式设置为 `cmux`。

cmux 模式会为一个 zootree workspace 创建一个 cmux workspace group。group name 使用 workspace title；group anchor 左侧运行 `zootree info`，右侧只有一个 terminal：多 repo 且使用 `--run-agent` 时运行 agent，不加 `--run-agent` 时是普通 shell；group 内每个 repo 一个 workspace，repo workspace 左侧运行 lazygit、右侧运行 shell。单 repo 的 `--run-agent` 运行在 repo workspace 右下 terminal。当前 cmux group 模式只支持 `layout = "default"`。

### agent_cli 与别名

`agent_cli` 字段既可以是字面量命令模板（含 `$prompt` 占位符），也可以是
`agent_cli_alias` 表中已注册的别名 key。`zootree start <ws> --run-agent` 默认使用
`agent_cli` 字段；也可显式传入别名名或字面量命令。

```toml
agent_cli = "claude"   # 引用下面的 alias

[agent_cli_alias]
claude = "claude --dangerously-skip-permissions -- $prompt"
claude-safe = "claude -- $prompt"
gemini = "gemini chat -- $prompt"
codex = "codex --skip-confirm -- $prompt"
```

用法：

```bash
zootree start ws --run-agent                  # 用 agent_cli 默认
zootree start ws --run-agent claude-safe      # 切到指定 alias
zootree start ws --run-agent="codex -- $prompt"  # 直接传字面量
```

- 别名解析单层：`agent_cli_alias` 中找不到的字符串按字面量执行，不报错。
- `--run-agent <TAB>` 会列出所有 alias 名，与 `agent_cli` 匹配的那条标 `(default)`。

### 仓库配置 (~/.config/zootree/repos/<name>.toml)

```toml
path = "~/projects/myrepo"
default_target_branch = "develop"
copy_files = [".env.local"]

[hooks]
post_create = "npm install"

[lazygit]
config = "~/.config/lazygit/custom.yml"
```

- `copy_files`: 全局和仓库级别的 `copy_files` 会合并，启动时复制到 worktree
- `hooks`: 仓库级别 hook 优先于全局 hook

### Hook 格式

三种写法，功能等价：

```toml
# 简单命令
post_create = "echo hello"

# 执行脚本文件
pre_remove = { file = "~/.config/zootree/hooks/cleanup.sh" }

# 内联 shell 脚本
pre_done = { inline = "echo 'checking...' && cargo test" }
```

Hook 可用的环境变量：`ZOOTREE_WORKSPACE`、`ZOOTREE_REPO`、`ZOOTREE_BRANCH`、`ZOOTREE_TARGET_BRANCH`、`ZOOTREE_WORKTREE_PATH`、`ZOOTREE_WORKSPACE_DIR`

### 布局模板 (~/.config/zootree/layouts/<name>.kdl)

```kdl
// 自动生成，修改无效，仅作参考和调试用途
layout {
    default_tab_template {
        pane size=1 borderless=true {
            plugin location="tab-bar"
        }
        children
        pane size=1 borderless=true {
            plugin location="status-bar"
        }
    }

    tab name="overview" {
        pane split_direction="vertical" {
            pane command="zootree" {
                args "info" "$workspace_name" "--watch"
            }
            pane cwd="$workspace_dir" $overview_agent_cli
        }
    }

    // @repeat-per-repo
    tab name="$repo_name" {
        pane split_direction="vertical" {
            pane size="60%" command="lazygit" {
                args "-p" "$worktree_path" "-ucf" "$lazygit_config"
            }
            pane {
                pane size="30%" cwd="$worktree_path"
                pane size="70%" cwd="$worktree_path" $repo_agent_cli
            }
        }
    }
}
```

可用变量：`$repo_name`、`$worktree_path`、`$branch`、`$workspace_name`、`$workspace_dir`、`$lazygit_config`、`$overview_agent_cli`、`$repo_agent_cli`

- `// @repeat-per-repo` 标记下的 tab 块会为每个仓库重复展开
- 如果 lazygit_config 为空，`-ucf "$lazygit_config"` 参数对会自动移除
- `$overview_agent_cli` 和 `$repo_agent_cli` 是 `--run-agent` 的占位符；zellij 单 repo 使用 repo tab 右下 pane，多 repo 使用 overview tab 最后 pane，未使用的位置会回退为普通 shell

## 完整工作流示例

```bash
# 1. 初始化
mkdir -p ~/.config/zootree/layouts

# 2. 注册仓库
zootree repo add ~/projects/frontend --default-target-branch develop
zootree repo add ~/projects/backend --default-target-branch develop

# 3. 创建工作空间
zootree create --title "feat(xxx): 用户登录功能" --description "实现用户登录功能" --name user-login --repos frontend:feature/login,backend:feature/login --run-agent codexd

# 4. 查看工作空间
zootree info user-login

# 5. 在已配置的终端复用器中开发...

# 6. 完成并合并
zootree done user-login --push
```

## 故障排查

- **worktree 创建失败**: 检查分支名是否冲突，用 `zootree prune` 清理孤立 worktree
- **终端复用器未启动**: 确认已配置的 `cmux`（推荐）或 `zellij` 在 PATH 中，或使用 `--no-multiplexer` 跳过
- **Hook 执行失败**: 检查脚本语法，设置详细日志 `--verbose` 查看错误
- **日志位置**: `~/.config/zootree/logs/zootree.log`
