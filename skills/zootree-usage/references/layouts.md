# cmux、Zellij 与布局

## 目录

- Multiplexer 选择
- cmux workspace group
- Zellij KDL 布局
- 布局变量

## Multiplexer 选择

未配置 `[multiplexer].kind` 时 zootree 使用兼容默认值 `zellij`；新配置推荐显式设置为 `cmux`。当前 cmux workspace group 模式只支持 `layout = "default"`。

## cmux workspace group

cmux 模式会为一个 zootree workspace 创建一个 cmux workspace group，group name 使用 workspace title。

- group anchor 左侧运行 `zootree info`。
- group anchor 右侧只有一个 terminal：仅多 repo 且使用 `--run-agent` 时运行 agent，其余情况回退为普通 shell。
- group 内每个 repo 有一个 workspace，使用 50/50 分栏：agent 在左侧，右侧上下各是一个普通 shell。
- 单 repo 时，`--run-agent` 在 repo workspace 左侧 terminal 运行 agent；不加时左侧回退为普通 shell。cmux 默认 repo 布局不启动 lazygit。

## Zellij KDL 布局

Zellij 布局文件位于 `~/.config/zootree/layouts/<name>.kdl`：

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

`// @repeat-per-repo` 标记下的 tab 块会为每个仓库重复展开。如果 `lazygit_config` 为空，`-ucf "$lazygit_config"` 参数对会自动移除。

## 布局变量

KDL 布局可用变量：`$repo_name`、`$worktree_path`、`$branch`、`$workspace_name`、`$workspace_dir`、`$lazygit_config`、`$overview_agent_cli`、`$repo_agent_cli`。

`$overview_agent_cli` 和 `$repo_agent_cli` 是 `--run-agent` 的占位符：Zellij 单 repo 使用 repo tab 右下 pane，多 repo 使用 overview tab 最后的 pane，未使用的位置回退为普通 shell。
