# 03: [zootree] 为 create、add-repo、prune 以及 repo 注册和移除抽出服务层

**What to build:** 其余生命周期操作也通过同一套请求上下文和事件接口提供服务,命令行行为不变。create 在缺参数时服务层直接报错,交互式向导留在命令行适配层。

**Blocked by:** 01 — [zootree] 为 done 和 cancel 抽出服务层,并引入请求上下文

**Status:** done

- [ ] create、add-repo、prune、repo 注册、repo 移除各有不依赖命令行参数类型的服务函数
- [ ] 交互式向导和选择器只存在于命令行适配层
- [ ] 命令行输出和现有测试不变
- [ ] 新增服务层测试覆盖缺参数报错与事件发出
