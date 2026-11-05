# 06: [zootree] 只读 API:Workspace、repo、agent,以及信封、错误码、版本协商

**What to build:** Daemon 提供 Workspace 列表和详情、Registered repository 列表、agent 目录的读取接口,响应体沿用已发布的 `--json` 结构。每个请求校验令牌,错误使用稳定的字符串错误码,并在客户端协议版本过旧或过新时返回明确的升级提示。

**Blocked by:** 05 — [zootree] Daemon 骨架:生命周期、发现文件、令牌、状态接口

**Status:** done

- [ ] 四类读取接口返回的结构与已发布的 `--json` 一致
- [ ] 缺少或错误的令牌被拒绝
- [ ] 响应对未知字段保持宽容;错误码稳定且有文档
- [ ] 客户端声明的协议版本低于最低兼容版本或高于服务端时,得到明确的“请升级”错误
- [ ] 测试通过 HTTP 接口断言以上行为
