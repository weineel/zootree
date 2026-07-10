# AGENTS.md instructions

<INSTRUCTIONS>
总是使用中文回复。

本项目中创建 zootree workspace/task 时，任务标题模板使用 `cz-conventional-changelog` 规范：

- 格式：`<type>(<scope>): <subject>`
- `type` 使用常见 Conventional Commit 类型，例如 `feat`、`fix`、`refactor`、`docs`、`test`、`chore`
- 本项目是开源项目；`scope` 使用源码模块、命令或行为边界，例如 `config`、`layout`、`tui`、`workspace`、`log`、`repo`、`docs`、`test`
- 不要使用内部项目编号、客户编号、组织内部语境或 `other` 作为 `scope`，scope 非必须；无法明确归属时优先省略 scope
- 示例：`fix(config): validate config-backed names`
- 示例：`fix(layout): escape zellij kdl variables`
- 示例：`refactor(tui): split create wizard app`

拆分 review backlog 或开发任务时：

- 无依赖关系的任务拆成独立 zootree workspace
- 有共享调用链或强依赖关系的任务放到同一个 zootree workspace
- 需要直接启动 agent 执行开发任务时，使用完整非交互 `zootree create ... --run-agent codexd` 命令，并提供 `--title`、`--description`、`--name`、`--branch` 和 `--repos` 或 `--template`
- workspace 名称使用简短 slug，任务标题使用上面的 Conventional Commit 风格
</INSTRUCTIONS>
