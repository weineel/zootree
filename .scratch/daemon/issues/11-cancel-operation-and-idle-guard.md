# 11: [zootree] 显式取消 Operation,以及运行中不空闲退出

**What to build:** 客户端断开连接默认不会终止 Operation。需要停止时,显式取消会尽力中断当前子进程,并走已有的回滚逻辑(例如 start 的回滚)。有进行中的 Operation 时,Daemon 不会因空闲而退出。

**Blocked by:** 04 — [zootree] 外部命令逐行转发输出,服务层发出进度和日志事件; 07 — [zootree] Operation 核心,以及 done 和 cancel 端点

**Status:** done

- [ ] 客户端断开不影响正在执行的 Operation
- [ ] 显式取消能中断当前子进程,Operation 最终状态为已取消,并说明停在哪一步
- [ ] start 被取消后清理本次创建的 worktree,与现有回滚一致
- [ ] 取消一个已结束的 Operation 得到明确的响应而不是错误
- [ ] 有进行中的 Operation 时空闲计时不会触发退出
- [ ] 测试通过 HTTP 接口断言
