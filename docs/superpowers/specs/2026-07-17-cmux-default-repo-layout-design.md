# cmux 默认 Repo 布局调整设计

## 目标

调整 cmux 的默认 repo workspace 布局：左侧运行 agent，右侧上下各保留一个普通 shell，并将根布局改为左右各占 50%。默认布局不再启动 lazygit。

## 布局合同

默认 repo workspace 使用水平根分割，`split` 为 `0.5`：

- 左侧是单个名为 `agent` 的 terminal surface，工作目录为 repo worktree，使用 `$agent_command`，并保持默认焦点。
- 右侧使用垂直分割，上下各是一个普通 shell，工作目录均为 repo worktree。
- 没有 agent command 时，沿用现有 fallback，把左侧 agent surface 转换成普通 shell，并移除空 command。
- 默认布局中不存在 lazygit surface，也不执行 `$lazygit_command`。

## 实现边界

只修改 `default_cmux_repo_layout()` 的默认 JSON 模板。保留 `render_cmux_repo_layout()` 对 `$lazygit_command` 和 lazygit 配置的渲染能力，避免改变自定义模板合同。

以下行为不变：

- cmux anchor workspace 布局。
- zellij 布局和启动逻辑。
- cmux workspace group、生命周期和持久化状态。
- agent command 解析和空 agent 到 shell 的 fallback。

## 测试与文档

更新 `tests/cmux_layout_test.rs` 的结构断言，验证根分割比例、三个 terminal surface 的位置、名称、工作目录、命令、焦点，以及没有 agent 时的 fallback。现有 lazygit 路径和配置转义测试改用显式自定义模板，以继续保护 renderer 能力。

同步更新 `README.md`、`README.zh-CN.md` 和 `skills/zootree-usage/references/layouts.md` 中对 cmux 默认 repo workspace 布局的说明。此次变更不新增模块、命令、依赖或编码约定，因此无需修改 `skills/zootree-dev/SKILL.md`。
