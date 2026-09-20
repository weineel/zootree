# 命令行选择 Terminal environment 后端

状态：设计已确认并实现。核心选择与边界均按本文件落实。

## 已确认的选择

`~/.config/zootree/config.toml` 的 `[multiplexer]` 提供创建或重新开启 Workspace 时的默认设置。`create`、`start` 和 `reopen` 接受 `--multiplexer <zellij|cmux|herdr>`，仅覆盖后端类型，不修改全局配置文件。

| 命令 | 配置基础 | 显式参数 | 保存行为 |
| --- | --- | --- | --- |
| `create` | 当前全局完整 multiplexer 配置，忽略模板中的 multiplexer 配置 | 覆盖 kind | 保存到新 Workspace |
| `start` | pending Workspace 已保存的完整 multiplexer 配置 | 覆盖 kind | 保存到启动后的 Workspace |
| `reopen` | 当前全局完整 multiplexer 配置 | 覆盖 kind | 保存到重新开启的 Workspace |

各后端的详细设置来自上述配置基础，包括 Zellij/cmux layout 和 Herdr session。`start` 不重新读取全局设置覆盖 Workspace 快照。

`open` 不增加覆盖参数，沿用 Workspace 的选择和已有运行时恢复规则。后续 `done/cancel` 关闭该 Workspace 所属的 Terminal environment。

`--multiplexer` 可与 `start/reopen` 的 `--no-multiplexer` 同时使用：保存选择，但跳过本次终端激活。后续 `open` 使用已保存的选择。

例如：

```sh
zootree create --multiplexer cmux
zootree start example --multiplexer cmux --no-multiplexer
zootree open example
zootree reopen example --multiplexer herdr
```

以上仅展示相关参数，其余必需信息仍按现有命令规则提供。

## 已确认的边界

- 创建向导继续使用命令行提供的选择；本次不增加独立的终端选择页面。选择模板不能覆盖显式参数或恢复模板中的旧终端偏好。
- 选择在激活终端前随 Workspace 配置持久化。若 Workspace 已进入 `in_progress` 后终端激活失败，保留选择，使 `open` 可按该选择重试；生命周期转换前的失败沿用既有回滚边界。
- `reopen --dry-run` 不保存选择。覆盖 worktree 前需要关闭旧终端时，仍使用归档 Workspace 的旧配置和状态；新选择仅用于重新开启后的环境。
- 旧模板文件继续可读；暂时保留其 multiplexer 字段以兼容现有文件，新建 Workspace 时不消费该字段。模板格式清理不属于本次范围。
- 参数采用固定枚举校验和补全；无效值在发生 Workspace 副作用前报错。

## 实现与验证范围

实现贯通 CLI 参数、create draft/向导、Workspace 保存及 start/reopen 调用链；显式 `create --multiplexer` 的选择必须保留到自动启动路径。不得依靠仅修改全局配置的临时副本绕过 Workspace 快照。

聚焦验证：全局默认、模板覆盖被忽略、三条命令的显式选择及详细设置保留、创建后自动启动、与 `--no-multiplexer` 组合、激活失败后的 `open` 重试、reopen dry-run 与旧环境关闭定位、非法枚举和补全。

已同步两个 README、相关技能说明，并更新 `docs/adr/0003-reopen-archived-workspaces.md` 对全局快照的描述，使其包含命令行 kind 覆盖。此选择沿用现有生命周期边界，不单独新增 ADR。
