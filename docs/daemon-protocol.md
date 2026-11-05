# Daemon 协议参考

本文是 zootree Daemon（ADR-0006）线上协议的可查参考，面向 Consumer（命令行、Orca、将来的 Web 页）作者。事实来源是 `src/daemon/`；与代码不一致时以代码为准。用户视角的用法见 README 的 “Daemon” 一节，Web 扩展点见 `docs/daemon-extension-points.md`。

## 1. 传输、端点与文件

HTTP/1.1 + JSON，事件流用 SSE，监听权限为 0600 的 unix socket。所有文件都在配置目录下的 `daemon/` 里（默认 `~/.config/zootree/daemon/`），目录权限 0700（已存在且权限更宽时会被收紧）。

| 文件 | 权限 | 内容 |
|---|---|---|
| `<端点名>.sock` | 0600 | unix socket；路径不得超过 100 字节，否则 Daemon 拒绝启动 |
| `<端点名>.json` | 0600 | 发现文件（原子写：先写临时文件再 rename） |
| `<端点名>.lock` | - | 单实例 flock；持有者退出（无论如何退出）内核即释放 |
| `<端点名>.log` | - | `zootree daemon start` 拉起的进程的 stdout/stderr（追加） |

端点名按大版本区分，不同大版本的 Daemon 共存：`0.x` 版本取 `v0.<minor>`（0.1.4 为 `v0.1`），1.0 之后取 `v<major>`。

发现文件字段：

```json
{
  "pid": 12345,
  "endpoint": "/Users/me/.config/zootree/daemon/v0.1.sock",
  "token": "<64 位十六进制>",
  "started_at": "2026-11-05T08:00:00+00:00",
  "protocol_version": 1
}
```

存活判断是三态，客户端必须遵守：连接成功才算在监听；只有 “连接被拒绝” 或 “端点不存在” 才算确认已死；超时、权限错误等一律是 “无法判断”，按可能在运行处理，绝不据此删除端点。Daemon 只清理自己创建的东西：退出时只有发现文件里的令牌仍是自己的才删除 socket 与发现文件；启动时只接管确认已死的陈旧 socket，对非 socket 文件或无法判断的端点拒绝启动。

## 2. 鉴权与令牌

每次 Daemon 启动生成新的随机令牌（32 字节随机数的十六进制，64 字符），写入 0600 的发现文件。每个请求（包括 `/status`、未知路径、事件流）都必须带 `Authorization: Bearer <令牌>`，精确匹配；否则 401 `unauthorized`。鉴权先于路由和版本协商。Daemon 重启后令牌失效，客户端要重新读发现文件。

## 3. 版本、最低兼容版本与能力串

`GET /status` 返回：

```json
{
  "version": "0.1.4",
  "protocol_version": 1,
  "min_compatible_protocol_version": 1,
  "capabilities": ["status", "read_api", "operations", "events"],
  "pid": 12345,
  "started_at": "2026-11-05T08:00:00+00:00"
}
```

- `protocol_version`：Daemon 的协议版本，当前为 1。只在删除方法、改变字段语义或改变鉴权方式时提升。
- `min_compatible_protocol_version`：Daemon 仍然服务的最老客户端协议版本，当前为 1。
- 客户端必须在每个请求里带请求头 `X-Zootree-Protocol-Version: <整数>`（`/status` 除外）。不带返回 426 `protocol_version_required`（客户端需升级，消息里写明该头）；不是非负整数返回 400 `bad_request`；小于最低兼容版本返回 426 `client_too_old`（客户端需升级）；大于 Daemon 的版本返回 426 `client_too_new`（需要升级并重启 Daemon）。`/status` 豁免此检查，使客户端总能读到版本信息来解释不兼容。
- 客户端用 `/status` 判断是否可用：要求 `min_compatible_protocol_version ≤ 自身协议版本 ≤ protocol_version`。命令行客户端在区间不重叠时给出 stderr 提示并本地执行；缺少必需能力时静默本地执行。
- 客户端提交修改请求后的重试规则：请求体完整写出之前失败（连不上、写失败）可以改为本地执行；请求体写完之后任何失败（读响应超时、连接被切断、响应无法解析）都不得重发或回退本地，因为 Daemon 可能已接受该 Operation，应报错并提示用 `GET /operations`（命令行 `zootree operations`）核对。
- 响应对未知字段宽容：客户端必须忽略不认识的字段；新增字段、新增错误码、新增能力都不需要提升协议版本。
- 版本信息只在 `/status` 与发现文件里，普通响应没有信封级的版本字段。

能力串决定客户端能用什么；新增行为用新的能力串门控：

| 能力串 | 含义 |
|---|---|
| `status` | `/status`、`/shutdown` |
| `read_api` | 只读接口 `GET /workspaces`、`/workspaces/{name}`、`/repos`、`/agents` |
| `operations` | 所有修改请求、`GET /operations/{id}`、`POST /operations/{id}/cancel` |
| `events` | `GET /events` |

命令行客户端模式要求 `read_api`、`operations`、`events` 三者齐全。`web` 为预留名，在 Web 传输真正存在前不得声明（见 `docs/daemon-extension-points.md`）。

## 4. 错误信封与错误码

所有错误响应为 `{"error": {"code": "<码>", "message": "<文本>"}}`。客户端只按 `code` 分支，不按 `message`。错误码只增不改名、不复用；移除要提升协议版本。

| 码 | HTTP | 出现位置与含义 |
|---|---|---|
| `bad_request` | 400 | 请求格式错误：路径里的名字非法、未知的 `status` 过滤值、请求体不是合法 JSON 或含未知字段、缺必填字段、`Last-Event-ID` 或协议版本头不是非负整数、`DELETE /workspaces/{name}` 的目标不是 done/canceled |
| `unauthorized` | 401 | 令牌缺失或错误 |
| `not_found` | 404 | 路径不存在，或 Workspace / repo / Operation 不存在（Operation 记录已被淘汰也是 404） |
| `busy` | 409 | 目标上已有运行中的 Operation；错误对象多一个 `operation_id` 指向它。不排队 |
| `internal_error` | 500 | Daemon 内部失败 |
| `client_too_old` | 426 | 客户端协议版本低于最低兼容版本 |
| `client_too_new` | 426 | 客户端协议版本高于 Daemon 的 |
| `protocol_version_required` | 426 | 请求缺少 `X-Zootree-Protocol-Version` 头（`/status` 除外的所有端点，含 `/events` 与 `/shutdown`）；鉴权先于此检查 |
| `confirmation_required` | - | 仅出现在失败 Operation 的 `error` 里：`cancel` 遇到未提交改动，错误对象带 `repos`（有未提交改动的 repo 名数组）。带 `confirmed_uncommitted: true` 或 `force: true` 重试；Daemon 从不提示 |
| `operation_failed` | - | 仅出现在失败 Operation 的 `error` 里：操作已执行但失败，`message` 是完整错误链 |

失败 Operation 的 `error.code` 还可能是 `busy`（命令行进程持有该 Workspace 的文件锁，此时没有 `operation_id`）和 `internal_error`（worker panic，Operation 被记为失败，Workspace 不会卡在忙碌）。

## 5. 端点一览

所有请求体都是可选 JSON：空体等于全部默认值，未知字段返回 400。所有修改请求都返回 202 与一个 Operation 视图（见第 6 节）。校验顺序：鉴权 → 版本 → 请求体与路径参数（400/404）→ 忙碌判定（409）。

| 方法与路径 | 能力 | 说明 |
|---|---|---|
| `GET /status` | status | 见第 3 节 |
| `POST /shutdown` | status | 优雅停止，返回 `{"stopping": true}` |
| `GET /workspaces[?status=a&status=b,c]` | read_api | Workspace 摘要数组，与 `list --json` 相同。`status` 取 `pending`、`in_progress`、`done`、`canceled`，可重复或逗号分隔；缺省为 pending + in_progress；未知取值 400，未知查询参数忽略 |
| `GET /workspaces/{name}` | read_api | 与 `info <name> --json` 相同 |
| `GET /repos` | read_api | 与 `repo list --json` 相同 |
| `GET /agents` | read_api | agent 目录 `{default, aliases[]}`，与 `config agents --json` 相同 |
| `GET /operations/{id}` | operations | Operation 视图 |
| `POST /operations/{id}/cancel` | operations | 见第 7 节，无请求体 |
| `GET /events` | events | SSE，见第 8 节 |
| `POST /workspaces` | operations | create |
| `POST /workspaces/{name}/start` | operations | start |
| `POST /workspaces/{name}/reopen` | operations | reopen |
| `POST /workspaces/{name}/add-repo` | operations | add-repo |
| `POST /workspaces/{name}/done` | operations | done |
| `POST /workspaces/{name}/cancel` | operations | cancel（取消 Workspace，不是取消 Operation） |
| `DELETE /workspaces/{name}` | operations | 删除一个已归档（done/canceled）的 Workspace，等价于 prune 一项 |
| `POST /repos` | operations | 登记 repo |
| `DELETE /repos/{name}` | operations | 移除 repo 登记 |

路径里的 Workspace / repo 名只能含 ASCII 字母、数字、`-`、`_`，否则 400；Workspace 不存在 404。

### 修改请求的请求体与结果

所有修改请求体都接受可选的 `env`（见第 9 节）。`run_agent`：缺省、`null` 或 `false` 不运行 agent，`true` 用默认 agent，字符串为命令或别名。

| 端点 | 请求体字段 | Operation `result` |
|---|---|---|
| `POST /workspaces` | `title`（必填）；`repos[]`（名字或 `{name, target_branch}`）与 `template` 二选一必填；`name`（缺省时 Daemon 生成）、`description`、`branch`、`multiplexer`、`start`、`run_agent` | `{workspace, status, title, branch, repos[], started, attach, agent_cli}` |
| `POST /workspaces/{name}/start` | `multiplexer`、`no_multiplexer`、`run_agent` | `{workspace, dry_run, attach, agent_cli}` |
| `POST /workspaces/{name}/reopen` | `multiplexer`、`from[]`（`current` 或 `REPO:BRANCH`）、`overwrite[]`、`run_agent`、`skip_hooks`、`no_multiplexer`、`dry_run` | 同 start |
| `POST /workspaces/{name}/add-repo` | `repo`（必填）、`target_branch` | `{repo, workspace, target_branch, workspace_branch, worktree_path, terminal}`，`terminal` 为 `updated` 或 `absent` |
| `POST /workspaces/{name}/done` | `no_merge`、`no_clean`、`push`、`strategy`（`squash`/`rebase`/`merge`）、`force`、`skip_hooks`、`dry_run` | `{workspace, dry_run}` |
| `POST /workspaces/{name}/cancel` | `no_clean`、`force`、`skip_hooks`、`confirmed_uncommitted` | `{workspace, dry_run}` |
| `DELETE /workspaces/{name}` | 仅 `env` | `{pruned: [name]}` |
| `POST /repos` | `path`（必填）、`name`（缺省取路径的目录名）、`default_target_branch` | `{name, path}` |
| `DELETE /repos/{name}` | 仅 `env` | `{name}` |

补充规则：

- `attach` 是 `{program, args[]}` 或 `null`。Daemon 绝不执行前台 attach；start / reopen（以及带 `start` 的 create）把需要的 attach 交给 Consumer，由它在自己的终端执行。`agent_cli` 是 Workspace 上保存的 agent 命令。
- `create` 缺 `title`、缺 `repos`/`template`、`repos` 为空数组且无 `template`、`name` 非法，都在登记 Operation 前返回 400，不会进入向导。`start: true` 时在同一个 Operation 里 create 后 start；start 失败则 Operation 失败，Workspace 保持 pending。
- `reopen` 没有人可询问：缺少需要回答的选项（如 `from`）时 Operation 以 `operation_failed` 失败，错误信息指明缺哪个选项，且发生在任何修改之前。
- `DELETE /workspaces/{name}` 只能删 done 或 canceled 的 Workspace，其他状态 400。
- 忙碌键：Workspace 类 Operation 用 Workspace 名；repo 登记与移除用 `repo:<名>`（Operation 的 `workspace` 字段就是这个键）。同一键上同时只允许一个运行中的 Operation，不同键并行。`create` 未给 `name` 时先生成名字再登记，使忙碌键确定。

## 6. Operation 视图

```json
{
  "id": "op-0123456789abcdef",
  "kind": "done",
  "workspace": "fair-fox",
  "status": "running",
  "output": ["..."],
  "result": null,
  "error": null,
  "cancel_requested": false,
  "canceled": null,
  "started_at": "2026-11-05T08:00:00+00:00",
  "finished_at": null
}
```

- `kind`：`done`、`cancel`、`start`、`reopen`、`add-repo`、`create`、`prune`（`DELETE /workspaces/{name}`）、`repo-add`、`repo-remove`。
- `status`：`running`、`succeeded`、`failed`、`canceled`。
- `output`：服务层事件累计的文本行（进度、子进程输出等，步骤开始/结束以 `==> 名`、`<== 名 ok|failed` 记录，警告带 `warning: ` 前缀）。`done`/`cancel` 的收尾摘要是最后一行。
- `result`：仅 `succeeded` 时非空，结构见第 5 节。
- `error`：仅 `failed` 时非空，`{code, message}`，`confirmation_required` 另带 `repos`。
- `canceled`：仅 `canceled` 时非空，见第 7 节。
- `cancel_requested`：收到过取消请求即为 true，不论最终结果。
- Operation 记录只存内存：保留最近 100 条已结束记录、至多 24 小时；运行中的从不淘汰。Daemon 重启后全部丢失。
- 客户端断开连接从不影响 Operation。

## 7. 取消 Operation

`POST /operations/{id}/cancel`（无请求体）：

- 运行中：202，`{"id", "outcome": "cancel_requested", "status": "running", "operation": <视图>}`。
- 已结束（含已取消）：200，`{"id", "outcome": "already_finished", "status": <终态>, "operation": <视图>}`，幂等。
- 未知 id：404 `not_found`。

取消是尽力而为：正在运行的子进程命令被杀掉（`RealRunner` 给子进程独立进程组并整组 SIGKILL），服务函数经自己的回滚展开（如 start 删除已创建的 worktree），取消令牌只消费一次，之后的回滚命令不会再被打断。命令被打断且工作返回错误时 Operation 以 `canceled` 结束，`canceled` 字段为：

```json
{
  "stopped_at": "merge",
  "interrupted_command": "git merge ...",
  "cleanup": "none",
  "notes": ["..."],
  "detail": "<完整错误链>"
}
```

`stopped_at` 是最后报告的步骤名或 `null`；`cleanup` 为 `rolled_back`（start 的回滚已完成）、`rollback_failed` 或 `none`（该操作没有回滚，已完成的步骤保持生效）；`merge` 被杀时 `notes` 会提示目标仓库可能留有半完成的合并。取消到达时没有命令可打断，或工作先完成了，Operation 保持真实结果（`succeeded` / `failed`），`cancel_requested` 仍为 true。

适用范围：所有类型的 Operation（`done`、`cancel`、`start`、`reopen`、`add-repo`、`create`、`prune`、`repo-add`、`repo-remove`）共用同一条提交路径，子进程命令都接入取消令牌。但只有 `start`（以及带 start 的 `create`）有回滚；其余类型被打断时 `cleanup` 为 `none`，已完成的步骤保持生效。没有子进程命令可打断的工作（例如纯配置读写）不会被中断，按真实结果结束。

## 8. 事件流（`GET /events`）

SSE（`Content-Type: text/event-stream`），每 15 秒一条保活注释。一条全局流，不按 Workspace 过滤。每个事件的 `id:` 是单调递增的序号（从 1 起），data 里另有 `seq` 与 `at`（RFC 3339 时间）。

### 事件类型

| 事件名 | data 字段 | 说明 |
|---|---|---|
| `workspace.changed` | `workspace`（完整摘要，同 `GET /workspaces` 的一项） | 创建、修改、变更状态；摘要是完整的，重复应用无害 |
| `workspace.removed` | `name` | Workspace 不再存在 |
| `operation.started` | `operation_id`, `kind`, `workspace`, `started_at` | |
| `operation.progress` | `operation_id`, `workspace`, `phase`, `message`, `ok`（仅 `step_finished`） | `phase`：`step`、`step_started`、`step_finished`、`warning` |
| `operation.log` | `operation_id`, `workspace`, `message` | 子进程的一行输出 |
| `operation.finished` | `operation_id`, `kind`, `workspace`, `status`, `result`, `error`, `finished_at` | `status` 为 `succeeded`、`failed`、`canceled`；`canceled` 时 `result` 携带第 7 节的取消说明 |
| `repo.changed` | `name`, `action` | `action`：`added`、`removed`、`updated` |

Daemon 自己的 Operation 结束时，先发 `workspace.changed`（或 `workspace.removed` / `repo.changed`），再发 `operation.finished`，不依赖扫描。外部改动（手工编辑配置、命令行直接执行）由变化检测发现：有订阅者时每 2 秒轮询 Workspace 状态目录与 `repos/` 里 `*.toml` 的修改时间、大小、inode，无订阅者不扫描；该 Workspace 有运行中的 Operation、或发现的内容与 Daemon 自己刚发布的相同时不重复报告；改到一半解析失败的文件下一轮再看。外部改动只报告摘要里有的字段，只改 description 等摘要外字段且恰好紧随 Daemon 自己的事件时可能不再报告。

### 控制帧（不占序号，无 `id:`，除 `ready`）

| 帧 | data | 含义 |
|---|---|---|
| `snapshot` | `{seq, workspaces[], operations[]}` | 权威状态：全部四种状态的 Workspace 摘要，以及运行中的 Operation 视图。`seq` 是订阅位置 |
| `resync` | `{reason, seq}` | 请求的位置无法由缓冲满足，丢弃本地状态，采用随后的 `snapshot`（或重新拉 `/workspaces`）。`reason`：`unknown_position`（`Last-Event-ID` 大于当前序号，通常是 Daemon 重启过）、`gap_too_large`（位置已被淘汰）、`lagged`（订阅者读得太慢，事件被淘汰） |
| `replay` | `{from, to, count}` | 续传标记：接下来 `count` 帧是缓冲里 `from` 之后的历史事件，不是实时的 |
| `ready` | `{seq, replay}`，`id` 为当前序号 | 此前都是历史/快照，此后是实时事件 |

### 订阅与续传

订阅顺序为 “先监听、再建基线与快照、最后就绪”：先在锁内固定位置，再让外部变化检测同步记下基线（已有基线则跳过），再读快照，再发 `ready`，之后推送位置之后的事件。快照与固定位置之间发生的变化会作为事件再出现一次；基线在快照之前建立，所以快照之后的外部改动要么已在快照里，要么被下一轮扫描报告，不存在既不在快照里也没有事件的窗口。

- 全新订阅（无 `Last-Event-ID`）：`snapshot` → `ready{replay:false}` → 实时事件。
- 带 `Last-Event-ID: n` 重连且 `n` 仍在缓冲内：`replay` → 序号大于 `n` 的缓冲事件 → `ready{replay:true}` → 实时事件，没有 `snapshot`。`n` 等于当前序号时 `count` 为 0。
- `n` 大于当前序号，或小于已淘汰的最高序号：`resync{reason}` → `snapshot` → `ready{replay:false}`。
- 慢订阅者：发 `resync{reason:"lagged"}` 后服务端关闭这条流；客户端重连即得到新的快照。
- `Last-Event-ID` 不是非负整数：400 `bad_request`。
- 缓冲保留最近 1000 条且不超过 10 分钟的事件。序号只在 Daemon 一次运行内有意义。
- 打开的事件流算一个连接，阻止空闲退出；Daemon 关闭时所有流被结束。

## 9. 环境快照（`env`）

修改请求的 `env` 是调用方环境的白名单快照，服务层启动的子进程用它代替 Daemon 自身的环境。白名单只有七个变量，其余名字静默丢弃：

`PATH`、`SSH_AUTH_SOCK`、`SSH_AGENT_PID`、`ZELLIJ`、`ZELLIJ_SESSION_NAME`、`HERDR_ENV`、`HERDR_SOCKET_PATH`

- 省略 `env`：使用 Daemon 自己环境里的这七个变量（不会得到空 PATH）。
- 给了 `env`（哪怕是 `{}`）：白名单变量按请求覆盖。快照里有的变量用快照的值；快照里没有的 `SSH_AUTH_SOCK`、`SSH_AGENT_PID`、`ZELLIJ`、`ZELLIJ_SESSION_NAME`、`HERDR_ENV`、`HERDR_SOCKET_PATH` 在子进程里被移除（绝不退回 Daemon 自己的值）；缺失的 `PATH` 例外，保持继承 Daemon 的 `PATH`（ADR-0005），否则子进程找不到 `git`。命令行客户端总是发送自己环境的白名单快照。
- 白名单之外的变量（`HOME` 等）不受请求影响，始终沿用 Daemon 进程的环境。
- 终端环境判断（是否已在 zellij / herdr 会话内）也由快照里的变量决定。

## 10. 生命周期与空闲退出

- 启动：`zootree daemon start` 拉起隐藏的前台入口 `zootree daemon run` 为脱离终端的独立进程组，最多等 10 秒就绪；已在运行或状态无法判断时拒绝启动。普通命令（`list`、`start` 等）绝不会启动 Daemon；长期运行的 Consumer（如 Orca）可以自己调用 `daemon start`。
- 单实例：同一端点名只允许一个 Daemon（flock）；第二个启动失败。
- 空闲退出：没有任何打开的请求（含事件流）也没有运行中的 Operation 的时间达到 10 分钟（默认，`DaemonOptions::idle_timeout`，目前没有命令行开关）即退出。有请求或 Operation 时计时重置。
- 停止：`zootree daemon stop` 发送 `POST /shutdown` 并等待最多 10 秒；SIGTERM / SIGINT 同样触发优雅退出。退出时结束所有事件流，并清理自己发布的 socket 与发现文件。
