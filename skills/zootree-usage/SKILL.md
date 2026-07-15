---
name: zootree-usage
description: Use when users need to create, start, open, inspect, finish, or cancel zootree workspaces; manage zootree repos or templates; configure run-agent, zellij, cmux, hooks, or layouts; or troubleshoot zootree commands.
---

# zootree 使用指南

zootree 用 Git Worktree 和 cmux/Zellij 管理一个或多个仓库组成的隔离工作空间。状态流转为 `pending` → `in_progress` → `done` / `canceled`。

## Agent 非交互执行契约

替用户执行命令时预填所有必要参数，不把 TUI 或 selector 留给 agent 现场填写。

| 场景 | 必须预填 |
|---|---|
| `create` | `--title`、`--description`、`--name`、`--branch`，以及 `--repos` 或 `--template` |
| `start/open/done/cancel` | workspace name |
| `repo edit/remove` | repo name |

- `create` 只有同时提供 title 和 repo 来源时才绕过 wizard；`--run-agent` 不会补齐这些参数。
- 默认使用裸 `--run-agent`，让 zootree 读取全局 `agent_cli`。只有用户明确选择时才传 `--run-agent <alias-or-command>`。
- `--run-agent` 会隐含 start。裸 flag 要求 `~/.config/zootree/config.toml` 已配置 `agent_cli`。
- 标题使用 `<type>(<scope>): <subject>`；scope 使用源码模块、命令或行为边界，不明确时省略，不使用 `other`。
- title 只放一行摘要；给 agent 的完整任务简报放进 description。zootree 会把二者组合为 agent prompt。
- 执行前把示例变量替换为真实且 shell-safe 的值。
- 当前 Git repo 未注册时，先用 repo root、repo name 和当前分支执行非交互 `zootree repo add`。
- `start` 的正确顺序是 `zootree start <workspace> --run-agent [alias]`，workspace name 必须在 flag 前。

## 创建并启动默认 agent

```bash
zootree create \
  --title "feat(preview): add file preview" \
  --description "Implement file preview with focused tests." \
  --name "file-preview" \
  --branch "zootree/file-preview" \
  --repos "frontend:main" \
  --run-agent
```

成功后用 `zootree info file-preview` 做最小验证。

## 按需读取

- 安装、repo/workspace/template 命令、完整工作流和故障排查：读取 `references/commands.md`。
- `config.toml`、`agent_cli`/alias、repo 配置和 Hook：读取 `references/configuration.md`。
- cmux/Zellij 行为、KDL 布局和布局变量：读取 `references/layouts.md`。
