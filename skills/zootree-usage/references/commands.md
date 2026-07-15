# 安装与命令参考

## 目录

- 安装
- 仓库管理
- 工作空间操作
- 模板与维护命令
- 完整工作流
- 故障排查

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

前置依赖：Git、cmux（推荐）或 Zellij、LazyGit（可选）。

## 仓库管理

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

## 工作空间操作

### 创建

工作空间创建后状态为 `pending`。Agent 执行时使用完整参数，避免进入 wizard。

```bash
# 创建 pending workspace
zootree create \
  --title "feat: 新功能开发" \
  --description "实现新功能开发任务" \
  --name new-feature \
  --branch zootree/new-feature \
  --repos frontend:feature/abc,backend:feature/abc

# 创建并启动全局配置的默认 agent
zootree create \
  --title "feat: 新功能开发" \
  --description "实现新功能开发任务" \
  --name new-feature \
  --branch zootree/new-feature \
  --repos frontend:feature/abc \
  --run-agent

# 用模板创建
zootree create \
  --title "feat: 新功能开发" \
  --description "实现新功能开发任务" \
  --name my-ws \
  --branch zootree/my-ws \
  --template my-template
```

### 启动

创建工作树、执行 Hook，并启动已配置的终端复用器。

```bash
# 指定名称
zootree start my-workspace

# 显式覆盖为已配置的 agent alias
zootree start my-workspace --run-agent claude-safe
```

### 查看与打开

```bash
zootree list
zootree list --status in_progress  # pending|in_progress|done|canceled
zootree info my-workspace
zootree open my-workspace
```

### 完成

合并分支、清理工作树并归档。

```bash
zootree done my-ws
zootree done my-ws --push
zootree done my-ws --no-merge
zootree done my-ws --no-clean
zootree done my-ws --strategy squash  # squash/rebase/merge
```

### 取消

清理工作树、删除分支并归档。

```bash
zootree cancel my-ws
zootree cancel my-ws --no-clean
zootree cancel my-ws --force
```

## 模板与维护命令

```bash
zootree template list
zootree template save <name> --from <workspace>
zootree prune  # 清理孤立的 worktree
zootree logs   # 查看日志文件
```

每次创建工作空间时会自动保存为 `recently` 模板，方便下次使用。

## 完整工作流

```bash
# 1. 初始化
mkdir -p ~/.config/zootree/layouts

# 2. 注册仓库
zootree repo add ~/projects/frontend --name frontend --default-target-branch develop
zootree repo add ~/projects/backend --name backend --default-target-branch develop

# 3. 创建工作空间并启动默认 agent
zootree create \
  --title "feat(auth): 添加用户登录" \
  --description "实现用户登录功能，保持现有认证行为不变并添加聚焦测试。" \
  --name user-login \
  --branch zootree/user-login \
  --repos frontend:develop,backend:develop \
  --run-agent

# 4. 最小验证
zootree info user-login

# 5. 在已配置的终端复用器中开发...

# 6. 完成并合并
zootree done user-login --push
```

## 故障排查

- **worktree 创建失败**：检查分支名是否冲突，用 `zootree prune` 清理孤立 worktree。
- **终端复用器未启动**：确认已配置的 cmux（推荐）或 Zellij 在 `PATH` 中，或使用 `--no-multiplexer` 跳过。
- **Hook 执行失败**：检查脚本语法，设置详细日志 `--verbose` 查看错误。
- **日志位置**：`~/.config/zootree/logs/zootree.log`。
