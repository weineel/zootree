# 12: [zootree] 命令行客户端模式:Daemon 运行时命令经由它执行

**What to build:** Daemon 在运行时,`list`、`info`、`start`、`done`、`cancel` 等命令经由它执行,输出与以前一致。按 Ctrl-C 默认只是不再等待,不终止操作。编辑器、终端 attach、`config` 恢复命令仍在本地执行。没有 Daemon 时行为不变,且普通命令不会启动 Daemon。

**Blocked by:** 08 — [zootree] 其余修改端点:start、reopen、add-repo、create、归档删除、repo 注册和移除; 09 — [zootree] 事件流:SSE、序号、断线续传、重新同步; 11 — [zootree] 显式取消 Operation,以及运行中不空闲退出

**Status:** done

- [ ] Daemon 运行时,命令行输出与不经过 Daemon 时一致
- [ ] 命令通过事件流等待 Operation 并实时打印进度
- [ ] Ctrl-C 默认断开等待;显式取消子命令才终止操作
- [ ] 命令行把自己的 PATH、SSH 认证和复用器上下文作为环境快照随请求发送
- [ ] 前台 attach、编辑器和 `config path|show|edit` 不经过 Daemon,配置文件损坏时仍可用
- [ ] 没有 Daemon 时所有命令照旧;普通命令不会启动 Daemon
- [ ] 协议版本不兼容时给出明确提示并回退为本地执行
