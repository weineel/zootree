# 05: [zootree] Daemon 骨架:生命周期、发现文件、令牌、状态接口

**What to build:** 新增 `zootree daemon start|stop|status`。Daemon 单实例运行,在本机 unix socket 上提供 `/status`,返回版本、协议版本、最低兼容版本和能力列表。发现文件记录进程号、端点、令牌、启动时间、协议版本。无连接且无进行中的 Operation 约 10 分钟后自动退出。

**Blocked by:** None (can start immediately)

**Status:** done

- [ ] `daemon start` 启动后台进程并确认就绪;`daemon status` 报告运行状态;`daemon stop` 正常退出
- [ ] 第二次启动被拒绝并说明已有实例在运行
- [ ] socket 与发现文件只有当前用户可读写
- [ ] 存活判断为三态:仅连接被拒绝或端点不存在才算确认已死;超时和权限错误视为无法判断且不删除端点
- [ ] 残留的陈旧发现文件和端点能被下一次启动正确接管;不会删除不是自己创建的东西
- [ ] 空闲退出可用较短的超时在测试中验证
- [ ] 不同大版本的 Daemon 使用不同的端点名
- [ ] 同一 Daemon 接缝的测试:进程内启动在临时目录,只通过 HTTP 请求断言
