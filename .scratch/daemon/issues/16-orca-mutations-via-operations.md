# 16: [orca] 通过 Operation 执行修改,显示进度,处理“正忙”,更新 Orca 看板 spec

**What to build:** 迁移、done、cancel、reopen、追加仓库、注册和移除 repo 都经由 Daemon 发起 Operation。卡片显示实时进度和日志。收到“正忙”错误时给出提示。Orca 的看板 spec 同步更新,去掉“不引入常驻进程”和“定时轮询”的描述。

**Blocked by:** 08 — [zootree] 其余修改端点:start、reopen、add-repo、create、归档删除、repo 注册和移除; 15 — [orca] Daemon 客户端替换命令行执行器:只读看板、事件驱动、版本与启动失败体验

**Status:** done

- [ ] 看板上的所有修改操作都通过 Operation 完成
- [ ] 进行中的卡片显示实时进度与日志,完成后显示结果,失败时显示完整错误输出
- [ ] 对正忙的 Workspace 发起操作时给出明确提示,不重复提交
- [ ] Orca 关闭再打开后,仍能看到仍在运行或已完成的 Operation 结果
- [ ] 看板 spec 的相关描述已更新
- [ ] Orca 的 lint、类型检查和相关测试通过
