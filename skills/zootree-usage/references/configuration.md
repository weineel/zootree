# 配置、Agent 与 Hook

## 目录

- 全局配置
- agent_cli 与别名
- 仓库配置
- Hook 格式

## 全局配置

全局配置文件位于 `~/.config/zootree/config.toml`：

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

运行时默认值是 `workspace_root = "~/zootree-workspaces"`、`branch_prefix = "zootree"` 和 `multiplexer.kind = "zellij"`。新配置推荐显式设置 `kind = "cmux"`。

## agent_cli 与别名

`agent_cli` 可以是 `agent_cli_alias` 表中的 key，也可以是包含 `$prompt` 占位符的字面量命令模板。

```toml
agent_cli = "codex"

[agent_cli_alias]
claude = "claude --dangerously-skip-permissions -- $prompt"
claude-safe = "claude -- $prompt"
gemini = "gemini chat -- $prompt"
codex = "codex --skip-confirm -- $prompt"
```

解析规则：

- 裸 `--run-agent` 读取已配置的 `agent_cli`；如果该值匹配 alias key，再解析为 alias 命令，否则按字面量命令执行。
- 显式 `--run-agent <value>` 使用 `<value>`；匹配 alias key 时选择该 alias，找不到时按字面量命令执行。
- 别名只解析一层；`agent_cli_alias` 中找不到的字符串不报错。
- `--run-agent <TAB>` 会列出所有 alias 名，与 `agent_cli` 匹配的那条标记为 `(default)`。

```bash
zootree start ws --run-agent                       # 解析全局 agent_cli
zootree start ws --run-agent claude-safe           # 显式选择 alias
zootree start ws --run-agent='codex -- $prompt'    # 显式字面量命令
```

## 仓库配置

仓库配置文件位于 `~/.config/zootree/repos/<name>.toml`：

```toml
path = "~/projects/myrepo"
default_target_branch = "develop"
copy_files = [".env.local"]

[hooks]
post_create = "npm install"

[lazygit]
config = "~/.config/lazygit/custom.yml"
```

- 全局和仓库级别的 `copy_files` 会合并，启动时复制到 worktree。
- 仓库级别 Hook 优先于全局 Hook。

## Hook 格式

Hook 支持三种等价写法：

```toml
# 简单命令
post_create = "echo hello"

# 执行脚本文件
pre_remove = { file = "~/.config/zootree/hooks/cleanup.sh" }

# 内联 shell 脚本
pre_done = { inline = "echo 'checking...' && cargo test" }
```

Hook 可用的环境变量：`ZOOTREE_WORKSPACE`、`ZOOTREE_REPO`、`ZOOTREE_BRANCH`、`ZOOTREE_TARGET_BRANCH`、`ZOOTREE_WORKTREE_PATH`、`ZOOTREE_WORKSPACE_DIR`。
