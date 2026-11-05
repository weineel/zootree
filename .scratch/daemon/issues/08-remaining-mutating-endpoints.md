# 08: [zootree] 其余修改端点:start、reopen、add-repo、create、归档删除、repo 注册和移除

**What to build:** 所有生命周期请求都能通过 Daemon 完成,并被统一建模为 Operation。请求携带的环境快照被应用到子进程;需要前台 attach 的信息由 Daemon 返回,而不是在 Daemon 里执行。

**Blocked by:** 02 — [zootree] 为 start、open、reopen 抽出服务层,并让终端环境上下文可注入; 03 — [zootree] 为 create、add-repo、prune 以及 repo 注册和移除抽出服务层; 07 — [zootree] Operation 核心,以及 done 和 cancel 端点

**Status:** done

- [ ] start、reopen、add-repo、create、归档删除、repo 注册和移除都有端点并返回 Operation 标识
- [ ] 请求中的环境快照(PATH、SSH 认证、复用器上下文)被应用;Daemon 自己的环境不会泄漏进子进程
- [ ] start 返回 agent 命令等 Consumer 需要的信息;需要 attach 时返回描述而不执行
- [ ] start 失败时回滚行为与命令行一致
- [ ] 测试通过 HTTP 接口断言,外部命令使用测试替身
