# Zootree — 多库协同开发工作区管理工具

## 概述

Zootree 是一个 Rust CLI 工具，用于管理多仓库协同开发任务。核心概念是 workspace：一个开发任务关联多个 git 仓库，在每个仓库中创建同名分支的 worktree，通过 zellij 组织为统一的终端工作环境。

## 核心概念

- **Workspace**：一个开发任务的工作环境，关联多个 repo 的 worktree
- **Repo**：已注册的 git 仓库，可配置 hook、LazyGit、copy_files 等
- **Layout**：zellij 布局模板（KDL 格式），支持变量替换和 per-repo 重复

## 目录结构

```
~/.config/zootree/
├── config.toml              # 全局配置
├── repos/
│   ├── frontend.toml        # repo 配置
│   ├── backend.toml
│   └── shared-lib.toml
├── layouts/
│   ├── default.kdl          # 默认布局模板
│   └── minimal.kdl          # 用户自定义布局
├── templates/
│   ├── recently.toml        # 自动保存上次创建的配置
│   └── fullstack.toml       # 用户自定义模板
├── workspaces/
│   ├── pending/             # 只有配置文件
│   ├── in_progress/         # 已创建工作目录和 worktree
│   └── archived/
│       ├── done/
│       └── canceled/
└── logs/
    ├── zootree.log          # 当前日志
    └── zootree.log.1        # 滚动日志
```

## 配置文件

### 全局配置 — config.toml

```toml
default_layout = "default"
workspace_root = "~/zootree-workspaces"
branch_prefix = "zootree"    # 分支名 = {branch_prefix}/{workspace_name}

# 所有 repo 都会复制的文件
copy_files = [".env"]

[hooks]
post_create = ""
pre_remove = ""
post_start = ""
pre_done = ""
pre_cancel = ""

[log]
# dir = "~/.config/zootree/logs"
# max_files = 5
# max_size = "10MB"
```

### Repo 配置 — repos/frontend.toml

```toml
path = "~/projects/frontend"
default_target_branch = "develop"

# 创建 worktree 后从主仓库复制的文件（支持 glob）
copy_files = [
    ".env.local",
    ".vscode/settings.json",
]

[hooks]
post_create = "npm install"
pre_remove = "npm run clean"

[lazygit]
config = "~/projects/frontend/.lazygit.yml"
```

### Workspace 配置 — workspaces/in_progress/calm-river.toml

```toml
title = "用户认证功能"
name = "calm-river"
description = "前后端联调 OAuth2 登录"
branch = "zootree/calm-river"
workspace_dir = "~/zootree-workspaces/calm-river"
created_at = "2026-04-28T10:30:00+08:00"

# layout = "minimal"
# session_mode = "standalone"
# session_name = "my-work"

[[repos]]
name = "frontend"
target_branch = "develop"

[[repos]]
name = "backend"
target_branch = "develop"

[[events]]
action = "created"
timestamp = "2026-04-28T10:30:00+08:00"

[[events]]
action = "started"
timestamp = "2026-04-28T10:31:00+08:00"
```

### 模板 — templates/fullstack.toml

```toml
repos = ["frontend", "backend", "shared-lib"]
# layout = "default"
# session_mode = "standalone"
```

## 命令体系

```
zootree repo add <name> --path <path>     # 注册 repo
zootree repo list                          # 列出已注册 repo
zootree repo edit <name>                   # 用 $EDITOR 打开 repo 配置文件
zootree repo remove <name>                 # 移除 repo 注册

zootree create [--template <name>]         # 创建 workspace（交互式）
zootree create --title "xxx" --repos frontend:develop,backend:develop
                                           # 命令行直传
zootree create --title "xxx" --name auth --repos frontend:develop --branch feat/auth
                                           # 完全指定

zootree list [--status pending|in_progress|done|canceled]
                                           # 列出 workspace
zootree start <name> [--no-zellij]         # 创建 worktree + 工作目录，可选启动 zellij
zootree open <name>                        # 启动/附加 zellij session

zootree done <name> [--no-merge] [--no-clean] [--push] [--delete-remote]
                                           # 完成 workspace
zootree cancel <name> [--no-clean]         # 取消 workspace

zootree prune [--all]                      # 清理 archived workspace（配置 + 残留目录）

zootree template list                      # 列出模板
zootree template save <name> --from <workspace>  # 从 workspace 配置提取为模板

zootree logs                               # 查看日志
```

所有需要 `<name>` 的地方，不传则进入 dialoguer 交互选择器。

### --repos 语法

```bash
# repo:target_branch 格式
--repos frontend:develop,backend:develop

# 不指定分支则取 repo 配置的 default_target_branch，都没有则交互询问
--repos frontend,backend:develop

# 混合使用
--repos frontend:develop,backend,shared-lib:main
```

### 交互式创建流程

```
$ zootree create
? Title: 用户认证功能
? Description (optional): 前后端联调 OAuth2
? Select repos:
  [x] frontend
  [x] backend
  [ ] shared-lib
? Target branch for frontend: develop
? Target branch for backend: develop
→ name: calm-river (auto generated)
→ branch: zootree/calm-river (auto)
```

### Workspace Name 自动生成

不指定 `--name` 时，自动生成 `adjective-noun` 格式的随机名称（如 `calm-river`、`brave-fox`）。内置一组常用形容词和名词词表，碰撞时追加数字后缀（`calm-river-2`）。

## Workspace 状态流转

```
pending → in_progress → done
                      → canceled
```

- `zootree create` → pending（只有配置文件）
- `zootree start` → in_progress（创建 worktree + 工作目录，可选启动 zellij）
- `zootree done` → archived/done（合并 + 清理）
- `zootree cancel` → archived/canceled（清理）

## Workspace 工作目录

```
~/zootree-workspaces/calm-river/
├── frontend/          # git worktree
├── backend/           # git worktree
└── shared-lib/        # git worktree
```

## Git Worktree 操作

```bash
# 创建
git -C <repo_path> worktree add -b <branch> <workspace_dir>/<repo_name> <target_branch>

# 删除
git -C <repo_path> worktree remove <workspace_dir>/<repo_name>
```

## Done 流程

```
1. 执行 pre_done hook（全局）
2. 对每个 repo：
   a. 检查 worktree 是否有未提交更改，有则提示用户处理
   b. git checkout <target_branch>（在主仓库）
   c. git merge <branch>（默认），或 --rebase / --squash 可选
   d. 合并冲突时中断，提示用户手动解决后重新执行
   e. --push：git push origin <target_branch>
   f. --delete-remote：git push origin --delete <branch>
   g. 执行 pre_remove hook（repo 级）
   h. git worktree remove <path>
   i. git branch -d <branch>
3. 删除 workspace 工作目录
4. 配置文件移到 archived/done/
```

## Cancel 流程

```
1. 执行 pre_cancel hook（全局）
2. 对每个 repo：
   a. 检查未提交更改，有则提示确认（--force 跳过）
   b. 执行 pre_remove hook（repo 级）
   c. git worktree remove <path>（--force 如果有未提交更改）
   d. git branch -D <branch>
3. 删除 workspace 工作目录
4. 配置文件移到 archived/canceled/
```

## Hook 机制

Hook 分全局和 repo 级，repo 级覆盖全局。

### 配置格式

三种方式：

```toml
# 1. 简单命令（字符串）
[hooks]
post_create = "npm install"

# 2. 脚本文件
[hooks.pre_remove]
file = "~/.config/zootree/hooks/cleanup.sh"

# 3. 内联多行脚本
[hooks.post_start]
inline = """
cd $ZOOTREE_WORKTREE_PATH
npm install
npm run db:migrate
echo "ready"
"""
```

解析优先级：如果值是字符串，当简单命令执行；如果是 table，检查 `file` 或 `inline` 字段。

### 执行时注入环境变量

```
ZOOTREE_WORKSPACE=calm-river
ZOOTREE_REPO=frontend
ZOOTREE_BRANCH=zootree/calm-river
ZOOTREE_TARGET_BRANCH=develop
ZOOTREE_WORKTREE_PATH=~/zootree-workspaces/calm-river/frontend
ZOOTREE_WORKSPACE_DIR=~/zootree-workspaces/calm-river
```

生命周期：

```
create → 各 repo: git worktree add → copy_files → post_create hook
start  → post_start hook
done   → pre_done hook → merge → 各 repo: pre_remove hook → git worktree remove
cancel → pre_cancel hook → 各 repo: pre_remove hook → git worktree remove
```

Hook 脚本执行失败时中断操作并报错，`--force` 跳过。

## Copy Files

创建 worktree 后、post_create hook 之前，从主仓库复制未被 git 追踪的文件到 worktree。

- 全局 `config.toml` 的 `copy_files` 和 repo 级的 `copy_files` 合并（不覆盖）
- 支持 glob 模式

## 布局模板

### 优先级

workspace 指定 > repo 指定 > 全局 `default_layout` > 内置 fallback

### 变量

- `$repo_name` — repo 注册名
- `$worktree_path` — worktree 绝对路径
- `$branch` — 分支名
- `$workspace_name` — workspace 名称
- `$workspace_dir` — workspace 工作目录路径
- `$lazygit_config` — repo 的 LazyGit 配置路径（无配置则为空）

### 默认布局（类似 grove）

```kdl
layout {
    tab name="overview" {
        pane command="zootree" {
            args "list" "--status" "in_progress"
        }
    }

    // @repeat-per-repo
    tab name="$repo_name" {
        pane split_direction="vertical" {
            pane size="60%" command="lazygit" {
                args "-p" "$worktree_path" "-ucf" "$lazygit_config"
            }
            pane size="12%" cwd="$worktree_path"
            pane size="28%" cwd="$worktree_path"
        }
    }
}
```

### 渲染流程

1. 读取模板 KDL 文件
2. 找到 `// @repeat-per-repo` 注释后的 tab 块，为每个 repo 复制一份
3. 对所有内容做变量替换
4. 条件渲染：`$lazygit_config` 为空时，省略 `-ucf` 参数（渲染引擎识别包含空变量的参数行并移除）
5. 输出最终 KDL 到临时文件
6. `zellij --layout <临时文件>` 启动

## Zellij Session 模式

### standalone（默认）

一个 workspace 一个 session，session 名自动生成为 `zootree-<workspace_name>`。

```bash
zellij --session "zootree-calm-river" --layout <rendered.kdl>
zellij attach "zootree-calm-river"
```

### shared

多个 workspace 共享一个 session，通过 `zellij action` 动态添加 tab。

```bash
zellij --session "my-work" action new-tab --layout <per-repo.kdl> --name "auth/frontend"
```

Tab 命名：`<workspace_name>/<repo_name>`。

配置：

```toml
session_mode = "shared"
session_name = "my-work"     # shared 模式必填
```

生命周期：
- standalone：done/cancel 时 kill session
- shared：done/cancel 时只移除该 workspace 的 tab，session 里还有其他 tab 则保留

## 日志

### 运行时日志

- 终端输出：默认 INFO，`--verbose` 开启 DEBUG，`--quiet` 只输出错误
- 文件日志：固定 DEBUG 级别，记录所有 git 命令、hook 执行、输出
- 日志滚动：保留最近几个文件，可配置 max_files 和 max_size
- `zootree logs` 快速查看

### Workspace 事件日志

记录在 workspace 配置文件的 `[[events]]` 数组中，持久化生命周期事件。

## 技术选型

- 语言：Rust
- CLI 解析：clap 4（derive 模式）
- 交互式 TUI：dialoguer
- 配置：TOML（serde）
- 布局模板：KDL + 简单变量替换
- Git 操作：直接调用 git 命令（std::process::Command）
- Zellij 操作：直接调用 zellij 命令
- 日志：tracing + tracing-subscriber + tracing-appender
- 路径：dirs
- Glob：glob

### 项目结构

```
zootree/
├── Cargo.toml
├── src/
│   ├── main.rs              # CLI 入口
│   ├── cli/
│   │   ├── mod.rs
│   │   ├── repo.rs          # repo add/list/edit/remove
│   │   ├── workspace.rs     # create/list/start/open/done/cancel
│   │   ├── template.rs      # template list/save
│   │   └── prune.rs
│   ├── config/
│   │   ├── mod.rs
│   │   ├── global.rs        # config.toml
│   │   ├── repo.rs          # repos/*.toml
│   │   ├── workspace.rs     # workspaces/**/*.toml
│   │   └── template.rs      # templates/*.toml
│   ├── git.rs               # git worktree 操作封装
│   ├── zellij.rs            # session 管理
│   ├── hook.rs              # hook 执行引擎
│   ├── layout.rs            # KDL 模板解析与变量替换
│   └── tui.rs               # dialoguer 交互封装
```

### 依赖

```toml
[dependencies]
clap = { version = "4", features = ["derive"] }
dialoguer = "0.11"
toml = "0.8"
serde = { version = "1", features = ["derive"] }
kdl = "6"
dirs = "6"
glob = "0.3"
tracing = "0.1"
tracing-subscriber = "0.3"
tracing-appender = "0.2"
```

## 外部依赖

- git（必须）
- zellij（可选，不使用 zellij 功能时不需要）
- lazygit（可选，布局模板中使用时需要）

## 测试策略

核心逻辑单元测试，外部调用（git、zellij）mock。

### 测试范围

- **配置解析**：TOML 读写、全局/repo/workspace 配置的序列化反序列化、hook 三种格式解析、`--repos` 参数解析（`frontend:develop,backend` 格式）
- **布局渲染**：KDL 模板变量替换、`@repeat-per-repo` 块复制、空变量条件省略、布局优先级
- **状态流转**：pending → in_progress → done/canceled、配置文件在子目录间移动的逻辑
- **Git 命令拼装**：worktree add/remove 命令参数生成、merge/push/branch 删除命令生成（不实际执行，验证拼装结果）
- **Hook 引擎**：环境变量注入、三种 hook 格式的解析与命令构建、失败中断逻辑
- **Copy Files**：glob 匹配、全局与 repo 级合并
- **Workspace Name 生成**：随机名称格式、碰撞时追加后缀

### Mock 策略

抽象 `trait CommandRunner` 接口，生产代码用 `std::process::Command` 实现，测试用 mock 实现记录调用参数并返回预设结果。
