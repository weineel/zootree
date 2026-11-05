# 07: [zootree] Operation 核心,以及 done 和 cancel 端点

**What to build:** 通过 Daemon 发起 done 或 cancel,请求立即返回 Operation 标识。可以查询 Operation 的状态、输出和最终结果。同一 Workspace 上已有 Operation 在跑时,第二个修改请求被拒绝并返回专用的“正忙”错误码,附带正在执行的 Operation 标识;不同 Workspace 可并行。这是第一条完整的端到端修改路径。

**Blocked by:** 01 — [zootree] 为 done 和 cancel 抽出服务层,并引入请求上下文; 06 — [zootree] 只读 API:Workspace、repo、agent,以及信封、错误码、版本协商

**Status:** ready-for-agent

- [ ] done 和 cancel 请求立即返回 Operation 标识,操作在后台完成
- [ ] 查询接口返回状态、累计输出和成功或失败结果,失败时带完整错误输出
- [ ] 同一 Workspace 并发的第二个修改请求返回“正忙”错误码与正在执行的 Operation 标识;不同 Workspace 互不影响
- [ ] 与直接运行命令行(不经过 Daemon)并发时,文件锁保证只有一方成功
- [ ] Operation 记录只存内存,保留最近 100 条或 24 小时
- [ ] 测试通过 HTTP 接口断言以上行为
