# zootree

多仓库协作开发工作空间管理工具。基于 Git Worktree + 终端复用器（推荐 cmux，Zellij 作为兼容默认值）+ LazyGit 实现。

[English](README.md)

## 功能特性

- **多仓库管理** - 同时在多个仓库的同一分支上工作
- **工作空间** - 创建、管理和清理工作空间
- **终端复用器集成** - 自动启动布局好的 cmux 或 Zellij 终端环境
- **Hook 机制** - 自定义钩子支持 (simple/file/inline)
- **文件复制** - 自动复制配置文件到 worktree
- **模板系统** - 保存和复用工作空间配置

## 安装

CI/CD release 流程由 `cargo-dist` 生成，会发布 GitHub Release 产物、更新
Homebrew tap，并发布 crate 到 crates.io。

### 推荐：通过 shell installer 安装预编译二进制

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/weineel/zootree/releases/latest/download/zootree-installer.sh | sh
```

### 通过 Homebrew 安装预编译二进制

```sh
brew install weineel/tap/zootree
```

### 通过 crates.io 安装

```bash
cargo install zootree --locked
```

### 从本地 checkout 安装

```bash
cargo install --path .
```

### 安装 skill

```bash
npx skills install weineel/zootree
```

## Shell 补全

zootree 支持 5 种 shell 的补全：bash、zsh、fish、PowerShell、elvish，
覆盖子命令、flag 名以及动态值（workspace 名、repo 名、template 名）。

### 安装

在 shell 的 rc 文件里加一行。脚本会在每次 shell 启动时重新生成，
始终与已安装的 `zootree` 版本同步 —— 升级后无需手动刷新。

| Shell | 加到哪 | 加什么 |
|-------|-------|--------|
| bash  | `~/.bashrc` | `eval "$(zootree completions bash)"` |
| zsh   | `~/.zshrc`（`compinit` 之后）| `eval "$(zootree completions zsh)"` |
| fish  | `~/.config/fish/config.fish` | `zootree completions fish \| source` |
| PowerShell | `$PROFILE` | `zootree completions powershell \| Out-String \| Invoke-Expression` |
| elvish | `~/.config/elvish/rc.elv` | `eval (zootree completions elvish \| slurp)` |

重启 shell 或 source rc 文件即可生效。

### 补全范围

- 所有子命令和 flag
- `zootree start <TAB>` — pending 状态的 workspace
- `zootree open <TAB>` / `zootree done <TAB>` — in-progress 状态的 workspace
- `zootree cancel <TAB>` — pending 或 in-progress 状态的 workspace
- `zootree repo edit <TAB>` / `zootree repo remove|delete <TAB>` — 已注册的 repo
- `zootree template save --from <TAB>` — 任意 workspace
- `zootree create --template <TAB>` — 已保存的 template
- `zootree create --repos <TAB>` — 已注册的 repo（逗号分隔列表）
- `zootree list --status <TAB>` — workspace 状态值（pending、in-progress、done、canceled）
- `zootree done --strategy <TAB>` — 合并策略值（squash、rebase、merge）

zsh 与 fish 还会在候选项旁显示简短描述（workspace 标题 + 状态、
repo 路径、或 template 涵盖的 repo 列表）。

### 故障排查

补全无响应时，可直接验证动态拦截器：

```bash
COMPLETE=zsh zootree -- zootree start ''
```

应当每行输出一个候选项。若为空，确认是否有匹配状态的 workspace（`zootree list`）。

## 快速开始

### 1. 初始化配置目录

```bash
mkdir -p ~/.config/zootree
```

### 2. 添加仓库

```bash
# 交互式添加
zootree repo add

# 命令行添加 (自动从路径提取名称)
zootree repo add ~/projects/myrepo

# 指定仓库名称
zootree repo add ~/projects/myrepo --name myrepo --default-target-branch develop
```

如果推导出的仓库名或 `--name` 指定的仓库名已经存在，`repo add` 不会覆盖原配置，
而是使用下一个可用后缀注册新仓库，例如 `myrepo-2`、`myrepo-3`。

### 3. 创建工作空间

```bash
# 交互式创建
zootree create

# 命令行创建
zootree create --title "新功能开发" --repos frontend:feature/abc,backend:feature/abc
```

### 4. 启动工作空间

```bash
zootree start
# 或指定名称
zootree start my-workspace
```

### 5. 完成工作空间

```bash
# 合并分支并清理
zootree done

# 仅合并不清理
zootree done --no-clean

# 合并并推送
zootree done --push
```

## 命令参考

### 仓库管理

```bash
zootree repo add <path>              # 添加仓库
zootree repo list                    # 列出仓库
zootree repo remove|delete <name>    # 移除仓库
```

### 工作空间

```bash
zootree create [options]             # 创建工作空间
  --title <title>                    # 标题
  --name <name>                      # 工作空间名称
  --repos <repos>                    # 仓库列表 (repo:branch 格式)
  --branch <branch>                  # 分支名
  --template <name>                  # 使用模板

zootree start [name]                 # 启动工作空间
  --no-multiplexer                   # 不启动已配置的终端复用器
  --run-agent [alias|command]        # 启动 coding agent（详见「配置 → Agent CLI」）

zootree list                         # 以紧凑卡片形式列出工作空间
  --status pending|in-progress|done|canceled
  --oneline                          # 使用旧版单行输出，便于 fzf/脚本处理

zootree open [name]                  # 打开已有工作空间

zootree done [name]                  # 完成工作空间
  --no-merge                         # 不合并
  --no-clean                         # 不清理
  --push                             # 推送
  --force                            # 强制执行

zootree cancel [name]                # 取消 active 工作空间（pending 或 in-progress）
  --no-clean                         # 不清理
  --force                            # 强制执行
```

### 模板

```bash
zootree template list                # 列出模板
zootree template save <name> --from <workspace>
                                    # 将 workspace 保存为模板
```

### 工具

```bash
zootree prune                        # 清理孤立 worktree
zootree logs                         # 查看日志
```

## 配置文件

zootree 从 `~/.config/zootree/` 读取配置。速查表：

| 文件 / 字段 | 作用 |
|---|---|
| `config.toml` | 全局默认值：workspace 根目录、分支前缀、文件复制、hooks、agent CLI |
| `repos/<name>.toml` | 单仓库覆盖：path、目标分支、复制文件、hooks、lazygit 配置 |
| `layouts/<name>.kdl` | 自定义 Zellij KDL 布局，供 `[multiplexer.zellij].layout` 引用 |
| `[multiplexer.cmux].layout` | cmux layout 选择器；group-aware cmux 当前只支持 `default` |
| `[hooks]` 小节 | workspace/repo 生命周期事件触发的 shell 命令 |
| `agent_cli` / `agent_cli_alias` | `zootree start --run-agent` 启动的 coding agent 命令模板 |

### 全局配置 (~/.config/zootree/config.toml)

```toml
workspace_root = "~/zootree-workspaces"
branch_prefix = "zootree"
copy_files = [".env"]
agent_cli = "claude --dangerously-skip-permissions -- $prompt"

[multiplexer]
kind = "cmux"

[multiplexer.zellij]
layout = "default"

[multiplexer.cmux]
layout = "default"

[hooks]
post_create = "echo created"
post_start = "echo started"
pre_done = "echo done"
pre_cancel = "echo canceled"
pre_remove = "echo removed"

[log]
max_files = 5
```

cmux 是新配置推荐的终端复用器；如果省略 `[multiplexer].kind`，zootree 保持兼容默认值 `zellij`。

### 仓库配置 (~/.config/zootree/repos/<name>.toml)

```toml
path = "~/projects/myrepo"
default_target_branch = "develop"
copy_files = [".env.local", ".vscode/settings.json"]

[hooks]
post_create = "npm install"

[lazygit]
config = "~/projects/myrepo/.lazygit.yml"
```

### Hook 格式

```toml
# 简单命令
post_create = "echo hello"

# 文件脚本
pre_remove = { file = "~/.config/zootree/hooks/cleanup.sh" }

# 内联脚本
pre_done = { inline = "echo 'cleaning up' && rm -rf $ZOOTREE_WORKTREE_PATH" }
```

Hook 可用环境变量：
- `ZOOTREE_WORKSPACE` - 工作空间名称
- `ZOOTREE_REPO` - 仓库名称
- `ZOOTREE_BRANCH` - 分支名
- `ZOOTREE_TARGET_BRANCH` - 目标分支
- `ZOOTREE_WORKTREE_PATH` - worktree 路径
- `ZOOTREE_WORKSPACE_DIR` - 工作空间目录

### Zellij 布局模板 (~/.config/zootree/layouts/<name>.kdl)

```kdl
layout {
  tab {
    pane { name "frontend" }
    pane { name "backend" }
  }
}
```

可用变量：
- `@REPO_NAME@` - 仓库名
- `@WORKTREE_PATH@` - worktree 路径
- `@BRANCH@` - 分支名
- `@WORKSPACE_NAME@` - 工作空间名
- `@WORKSPACE_DIR@` - 工作空间目录

### cmux group 布局

当 `[multiplexer] kind = "cmux"` 时，zootree 会为每个 zootree workspace 创建一个 cmux workspace group。

- group name 使用 zootree workspace title。
- group anchor 左侧运行 `zootree info <workspace> --watch`。
- group anchor 右侧只有一个 terminal：多 repo 且使用 `--run-agent` 时运行 agent；不加 `--run-agent` 时是普通 shell。
- group 内每个 repo 一个 workspace。
- 每个 repo workspace 左侧运行 `lazygit -p <worktree_path>`，右侧是 shell。
- 单 repo 且使用 `--run-agent` 时，agent 运行在该 repo workspace 的右下 terminal。

Group-aware cmux 当前只支持 `layout = "default"`。非 default cmux layout 会返回明确错误，直到后续支持 group-aware 多模板配置。

### Agent CLI

`agent_cli` 是在终端复用器 pane 中启动 coding agent 的命令模板。模板会用 shell 风格拆分 token，并把 `$prompt` 替换为 workspace 的 `title`（若 `description` 非空则用换行连接）。`$prompt` 也可以嵌在 token 内部，例如 `--prompt=$prompt`。

对于 zellij，渲染后的命令在以下位置执行：

- **1 个 repo** -> repo tab 右下 pane
- **>=2 个 repo** -> overview tab 最后一个 pane

对于 cmux，渲染后的命令在以下位置执行：

- **1 个 repo** -> repo workspace 右下 terminal
- **>=2 个 repo** -> group anchor 右侧 terminal

不加 `--run-agent` 时，这些占位 pane 会回退为普通 shell。

#### 别名

`agent_cli` 既可以是字面量命令模板，也可以是 `agent_cli_alias` 中已注册的别名：

```toml
agent_cli = "claude"   # 引用 alias "claude"

[agent_cli_alias]
claude = "claude --dangerously-skip-permissions -- $prompt"
claude-safe = "claude -- $prompt"
gemini = "gemini chat -- $prompt"
codex = "codex --skip-confirm -- $prompt"
```

别名解析为一层：`agent_cli_alias` 中找不到 key 时，原字符串作字面量命令使用，**不会**报错或警告。

#### 使用 `--run-agent`

```bash
zootree start ws                                        # 不启动 agent
zootree start ws --run-agent                            # 用 agent_cli 默认值（这里是 "claude"）
zootree start ws --run-agent claude-safe                # 切换到 alias claude-safe
zootree start ws --run-agent="codex --skip -- $prompt"  # 直接传字面量
```

- `--run-agent` 建议放在 workspace 名之后。`zootree start --run-agent ws` 会把 `ws` 当作 alias 值吃掉，positional 名留空进入交互式选择器。
- shell 补全（`--run-agent <TAB>`）会列出所有 alias 名，与当前 `agent_cli` 值匹配的那条在描述里以 `(default)` 标记。

## 选项

```bash
-v, --verbose    # 详细输出
-q, --quiet      # 静默输出
-h, --help       # 帮助
-V, --version    # 版本
```

## 依赖

- Git
- cmux（推荐）或 Zellij
- LazyGit (可选)

## 发布

发布由本地 `cargo-release` 命令触发。本地命令会更新 `Cargo.toml`
版本号、创建 release commit、创建 `vX.Y.Z` tag，并把 commit 和 tag 推送到
远端。tag 推上去之后，会触发由 `cargo-dist` 生成的 GitHub Actions release
pipeline。

发布 patch 版本时执行：

```bash
cargo release patch --execute
```

如果要发布 minor 或 major 版本，使用同样流程：

```bash
cargo release minor --execute
cargo release major --execute
```
