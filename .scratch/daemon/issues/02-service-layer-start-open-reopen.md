# 02: [zootree] 为 start、open、reopen 抽出服务层,并让终端环境上下文可注入

**What to build:** start、open、reopen 也变成使用同一请求上下文和事件接口的服务函数。终端环境对“是否在 zellij 或 herdr 内”的判断从读进程环境改为可注入的调用者上下文。前台 attach 仍然是客户端本地动作,服务层只返回需要 attach 的信息。

**Blocked by:** 01 — [zootree] 为 done 和 cancel 抽出服务层,并引入请求上下文

**Status:** ready-for-agent

- [ ] start、open、reopen 的服务函数不依赖命令行参数类型,不直接打印或进入交互
- [ ] 终端环境的调用者上下文可以由请求注入;命令行路径传入自己进程的上下文,行为不变
- [ ] 服务层返回“需要 attach 什么”,真正的 attach 仍由命令行执行
- [ ] start 失败时的回滚行为保持不变并有测试覆盖
- [ ] 命令行输出和现有测试不变,全量测试通过(原有 3 个 herdr 用例的失败除外)
