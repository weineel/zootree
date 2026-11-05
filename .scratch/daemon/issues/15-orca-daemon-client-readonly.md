# 15: [orca] Daemon 客户端替换命令行执行器:只读看板、事件驱动、版本与启动失败体验

**What to build:** Orca 的 Zootree Board 通过 Daemon 读取数据:发现并探测 Daemon,必要时按需启动它,用快照加事件流渲染看板,去掉轮询和命令行执行器。版本门槛改为检查协议版本和能力(对应 zootree 0.2.0)。Daemon 启动失败时显示原因,并保留二进制路径输入框。

**Blocked by:** 06 — [zootree] 只读 API:Workspace、repo、agent,以及信封、错误码、版本协商; 09 — [zootree] 事件流:SSE、序号、断线续传、重新同步

**Status:** ready-for-agent

- [ ] 看板数据来自 Daemon 的快照与事件流,状态变化无需轮询即可刷新
- [ ] Daemon 不在运行时,Orca 能按需启动它;启动失败时看板显示原因和路径输入框
- [ ] 版本或能力不满足时显示升级提示并禁用操作
- [ ] 短暂的连接失败不会把整个看板换成错误页
- [ ] 命令行执行器被删除;看板业务规则的现有测试尽量保持,传输层测试改用假 Daemon 客户端
- [ ] Orca 的 lint、类型检查通过
