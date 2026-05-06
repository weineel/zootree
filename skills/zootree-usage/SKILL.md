---
name: zootree-usage
description: >
  帮助用户安装、配置和使用 zootree 多仓库协作工作空间管理工具。
  当用户提到 zootree、workspace 工作空间、worktree、多仓库管理、zellij 布局、
  lazygit 集成、或需要创建/启动/完成/取消工作空间时，使用此 skill。
  也适用于配置 zootree 的 config.toml、Hook 脚本、KDL 布局模板等场景。
---

# zootree 使用指南

zootree 是基于 Git Worktree + Zellij + LazyGit 的多仓库协作工作空间管理工具。

## 核心概念

zootree 管理「工作空间」—— 一个工作空间包含多个仓库在同一个分支名上工作。
状态流转: `pending` → `in_progress` → `done` / `canceled`

## 安装

```bash
cargo install --path .
```

前置依赖: Git、Zellij、LazyGit（可选）

## 命令参考

### 仓库管理

```bash
# 交互式添加
zootree repo add

# 指定名称和默认目标分支
zootree repo add ~/projects/myrepo --name myrepo --default-target-branch develop

# 列出所有仓库
zootree repo list

# 编辑仓库配置（会用 $EDITOR 打开）
zootree repo edit [name]

# 移除仓库
zootree repo remove [name]
```

### 工作空间操作

**创建** - 工作空间创建后状态为 `pending`

```bash
# 交互式创建
zootree create

# 命令行创建（repo:branch 格式，逗号分隔）
zootree create --title "新功能开发" --repos frontend:feature/abc,backend:feature/abc

# 指定分支名、名称或使用模板
zootree create --branch my-feature --name my-ws --template my-template
```

**启动** - 创建工作树、执行 hook、启动 Zellij

```bash
# 交互式选择
zootree start

# 指定名称
zootree start my-workspace

# 不启动 Zellij
zootree start --no-zellij
```

**查看** - 列出工作空间

```bash
zootree list
zootree list --status in_progress  # pending|in_progress|done|canceled
```

**打开** - 重新打开已启动的工作空间（重新启动 Zellij session）

```bash
zootree open            # 交互式
zootree open my-workspace
```

**完成** - 合并分支、清理工作树、归档

```bash
zootree done                  # 交互式
zootree done my-ws
zootree done --push           # 合并后推送
zootree done --no-merge       # 跳过合并
zootree done --no-clean       # 跳过清理
zootree done --delete-remote  # 删除远程分支
zootree done --strategy squash  # 合并策略: squash/rebase/merge
zootree done --dry-run        # 预览
```

**取消** - 清理工作树、删除分支、归档

```bash
zootree cancel [name]
zootree cancel --no-clean     # 不清理 worktree
zootree cancel --force        # 强制取消
```

### 模板管理

```bash
zootree template list              # 列出所有模板
zootree template show <name>       # 查看模板内容
zootree template delete <name>     # 删除模板
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
default_layout = "default"
workspace_root = "~/zootree-workspaces"
branch_prefix = "zootree"
copy_files = [".env"]

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

可用变量：`$repo_name`、`$worktree_path`、`$branch`、`$workspace_name`、`$workspace_dir`、`$lazygit_config`

- `// @repeat-per-repo` 标记下的 tab 块会为每个仓库重复展开
- 如果 lazygit_config 为空，`-ucf "$lazygit_config"` 参数对会自动移除

## 完整工作流示例

```bash
# 1. 初始化
mkdir -p ~/.config/zootree/layouts

# 2. 注册仓库
zootree repo add ~/projects/frontend --default-target-branch develop
zootree repo add ~/projects/backend --default-target-branch develop

# 3. 创建工作空间
zootree create --title "用户登录功能" --repos frontend:feature/login,backend:feature/login

# 4. 开始工作
zootree start

# 5. 在 Zellij 中开发...

# 6. 完成并合并
zootree done --push
```

## 故障排查

- **worktree 创建失败**: 检查分支名是否冲突，用 `zootree prune` 清理孤立 worktree
- **Zellij 未启动**: 确认 `zellij` 在 PATH 中，或使用 `--no-zellij` 跳过
- **Hook 执行失败**: 检查脚本语法，设置详细日志 `--verbose` 查看错误
- **日志位置**: `~/.config/zootree/logs/zootree.log`
