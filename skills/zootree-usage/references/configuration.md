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
dir = "~/.config/zootree/logs"
max_files = 5
```

运行时默认值是 `workspace_root = "~/zootree-workspaces"`、`branch_prefix = "zootree"` 和 `multiplexer.kind = "zellij"`。新配置推荐显式设置 `kind = "cmux"`。

配置文件辅助命令：

```bash
zootree config path   # 输出 config.toml 的绝对路径，不创建文件
zootree config show   # 原样输出文件；缺失时返回错误和 edit 指引
zootree config edit   # 按需创建空文件，编辑后校验 TOML
```

`path/show/edit` 不依赖配置解析，因此配置损坏时仍可用于定位、查看和修复。`edit` 依次使用 `$VISUAL`、`$EDITOR`、`vi`，支持带参数的编辑器命令；校验失败时保留用户修改。

`log.dir` 指定日志目录并支持 `~` 展开；未配置时使用 zootree 配置目录下的 `logs/`。日志按天（DAILY）轮转，`max_files` 是保留的日志日文件数，默认为 5。

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
- `zootree config agents` 列出默认值与全部 alias；`--json` 输出供自动化消费的同一份 catalog。
- `--run-agent <TAB>` 会列出所有 alias 名，与 `agent_cli` 匹配的那条排在首位并标记为 `(default)`。

```bash
zootree config agents                              # 人类可读候选
zootree config agents --json                       # 结构化候选
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

所有 Hook 都会收到 `ZOOTREE_HOOK`、`ZOOTREE_OPERATION`、`ZOOTREE_HOOK_SCOPE`、`ZOOTREE_HOOK_CONFIG_SCOPE`、`ZOOTREE_WORKSPACE`、`ZOOTREE_WORKSPACE_TITLE`、`ZOOTREE_WORKSPACE_DESCRIPTION`、`ZOOTREE_WORKSPACE_STATUS`、`ZOOTREE_WORKSPACE_DIR`、`ZOOTREE_BRANCH` 与 `ZOOTREE_VERSION`。repo 级 Hook 还会收到 `ZOOTREE_REPO`、`ZOOTREE_REPO_SOURCE_DIR`、`ZOOTREE_WORKTREE_PATH`，以及可用时的 `ZOOTREE_TARGET_BRANCH`。

repo 级 Hook 的 cwd 是对应 worktree，workspace 级 Hook 的 cwd 是 Workspace 根目录。执行前会移除父进程继承的全部官方 Hook 变量，再注入本次调用的真实上下文。repo 配置优先于全局 fallback；`post_start` 在 `start` 与 `reopen` 中执行，`add-repo` 不执行它。
