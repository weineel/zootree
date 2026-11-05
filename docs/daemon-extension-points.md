# Daemon 的 Web 扩展点（仅预留）

本期不实现 Web 页面、回环 TCP、浏览器鉴权和静态资源托管。下面四处只是在代码里留好位置，现有 unix socket 行为不变，也没有任何未使用的运行时代码。

## 1. 传输（`src/daemon/transport.rs`）

`Listener` 枚举是"已绑定的监听器"。目前只有 `Unix`（0600 socket）。路由和处理函数不感知传输，`server::run` 在枚举上分派并调用同一个 `axum::serve`。
意图：将来加回环 TCP 时新增一个变体（以及它自己的 `Authenticator`），API 不改。
边界：现在不开任何端口；发现文件、单实例锁、陈旧端点接管仍只针对 unix socket。

## 2. 鉴权（`src/daemon/auth.rs`）

`Authenticator` trait 是独立、可替换的一层，`AppState.auth` 持有实现，`require_auth` 中间件对所有请求（含 `/status`、未知路径）先于路由与版本协商执行。当前实现 `BearerToken`：`Authorization: Bearer <发现文件里的令牌>`，精确匹配。
意图：浏览器场景（Cookie/会话、Origin 校验等）作为另一个实现接入。
边界：处理函数永远不接触凭证；不得为 Web 放宽 unix socket 的鉴权。

## 3. 能力串（`src/daemon/mod.rs` 的 `CAPABILITIES`）

预留名称 `web`，默认不声明，测试断言当前集合恰为 `status`、`read_api`、`operations`、`events`。
意图：Web 传输真正可用时才加入，供客户端判断。
边界：声明 `web` 之前不得出现依赖它的行为；新增能力不需要 bump 协议版本。

## 4. 静态资源托管（`src/daemon/server.rs` 的 `router`）

挂载点是 `router` 函数：将来在这里为 Web 传输挂载静态资源服务（作为 fallback，自行决定鉴权豁免范围）。
边界：不托管任何文件，不新增依赖；API 路径空间（`/status`、`/workspaces` 等）保持不变，静态资源不得遮蔽它们。

## 外部变化检测

`daemon::watcher::ChangeDetector` 是唯一接缝：默认的 `FsDetector` 轮询（有订阅者时每 2 秒，比较修改时间、大小、inode），以后可换成文件系统监听而不改事件 API。通过 `DaemonOptions::detector` 替换。
