# zootree

多仓库协作开发工作空间管理工具。基于 Git Worktree + Zellij + LazyGit 实现。

[English](README.md)

## 功能特性

- **多仓库管理** - 同时在多个仓库的同一分支上工作
- **工作空间** - 创建、管理和清理工作空间
- **Zellij 集成** - 自动启动布局好的终端环境
- **Hook 机制** - 自定义钩子支持 (simple/file/inline)
- **文件复制** - 自动复制配置文件到 worktree
- **模板系统** - 保存和复用工作空间配置

## 安装

```bash
cargo install --path .
```

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
zootree repo remove <name>           # 移除仓库
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
  --no-zellij                        # 不启动 Zellij

zootree list                         # 列出工作空间
  --status pending|in_progress|done|canceled

zootree open [name]                  # 打开已有工作空间

zootree done [name]                  # 完成工作空间
  --no-merge                         # 不合并
  --no-clean                         # 不清理
  --push                             # 推送
  --delete-remote                    # 删除远程分支
  --force                            # 强制执行

zootree cancel [name]                # 取消工作空间
  --no-clean                         # 不清理
  --force                            # 强制执行
```

### 模板

```bash
zootree template list                # 列出模板
zootree template show <name>         # 显示模板
zootree template delete <name>       # 删除模板
```

### 工具

```bash
zootree prune                        # 清理孤立 worktree
zootree logs                         # 查看日志
```

## 配置文件

### 全局配置 (~/.config/zootree/config.toml)

```toml
workspace_root = "~/zootree-workspaces"
branch_prefix = "zootree"
copy_files = [".env"]

[zellij]
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
pre_done = { inline = "echo 'cleaning up' && rm -rf $WORKTREE_PATH" }
```

Hook 可用环境变量：
- `WORKSPACE` - 工作空间名称
- `REPO` - 仓库名称
- `BRANCH` - 分支名
- `TARGET_BRANCH` - 目标分支
- `WORKTREE_PATH` - worktree 路径
- `WORKSPACE_DIR` - 工作空间目录

### 布局模板 (~/.config/zootree/layouts/<name>.kdl)

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

## 选项

```bash
-v, --verbose    # 详细输出
-q, --quiet      # 静默输出
-h, --help       # 帮助
--version        # 版本
```

## 依赖

- Git
- Zellij
- LazyGit (可选)
