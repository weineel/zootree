# 13: [zootree] Web 扩展点(仅预留)

**What to build:** 不实现任何 Web 功能,只在设计上留好位置:监听器可以有多种传输(目前只有 unix socket,留出回环 TCP 的位置);鉴权是独立的一层;能力列表预留 web 相关项;静态资源托管有挂载点。

**Blocked by:** 06 — [zootree] 只读 API:Workspace、repo、agent,以及信封、错误码、版本协商

**Status:** done

- [ ] 监听器抽象允许在不改 API 的前提下增加传输;现有 unix socket 行为不变
- [ ] 鉴权作为可替换的独立一层,现有令牌校验行为不变
- [ ] 能力列表中预留 web 项,默认不声明
- [ ] 在文档中说明每个扩展点的意图和边界
- [ ] 没有引入任何未使用的运行时代码
