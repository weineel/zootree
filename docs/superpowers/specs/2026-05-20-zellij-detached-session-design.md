# Zellij Detached Session Support

## 背景与动机

`zootree start` / `create --start` / `create --run-agent` / `open` 当前最终调用 `ZellijOps::start_session`，使用 `zellij --new-session-with-layout LAYOUT --session NAME`。该路径是**交互式**的：zellij 接管当前终端。

当 zootree 自身运行在已有 zellij session 内时，这导致：
- zellij 默认拒绝嵌套，命令失败；当前代码隐式 fallback 到 `attach_session`，行为不可预测。
- 用户场景"在 session 中启动一个新 session"无法工作。

实测确认 zellij 0.44 的 `attach --create-background` 子命令支持后台创建 detached session：
- 命令在 layout 中声明的 pane（包括 `--run-agent` 注入的 `command=...`）会真实启动；
- session 持久化、可被 `list-sessions` 看到；
- 可后续通过 `zellij attach <name>` 或 `zootree open <name>` 接管。

唯一前提：被调用的 zellij 进程必须**剥离** `ZELLIJ` / `ZELLIJ_SESSION_NAME` / `ZELLIJ_PANE_ID` 环境变量；否则在嵌套场景下创建会失败（实测验证）。

## 目标

在 `launch_zellij` 内统一引入"嵌套感知"：

| 当前是否在 zellij | session 是否已存在 | 行为 |
|---|---|---|
| 否 | 否 | 前台创建（保持现状） |
| 否 | 是 | 前台 attach（保持现状） |
| 是 | 否 | 后台创建 + 打印 attach 提示，正常退出 |
| 是 | 是 | 仅打印提示，正常退出 |

所有走 `launch_zellij` 的入口（`start` / `create --start` / `create --run-agent` / `open`）自动获得新行为。

## 非目标

- 不为 `add_tab`（`workspace add-repo`）增加智能路径 —— 它本就非交互、不受影响。
- 不引入 `--background` flag 强制后台。
- 不新增 `zootree attach <name>` 子命令 —— `open` 已覆盖。
- 不处理 zellij dead session 的 resurrection UX。
- 不真实端到端跑 zellij —— 测试沿用 `MockRunner`。

## 架构

三层分明，对应三处改动：

| 层 | 位置 | 职责 |
|---|---|---|
| 决策（纯） | `src/core/zellij.rs` 文件顶层 | `LaunchPlan` enum、`plan_launch(in_zellij, session_exists)`、`is_inside_zellij()` |
| 原语（IO 边界） | `src/core/zellij.rs::ZellijOps` | 新增 `start_session_background(name, layout)`，1:1 包装 zellij CLI |
| 编排 | `src/cli/workspace.rs::launch_zellij` | 调 `plan_launch`，match 分派到 ZellijOps 或仅打印 |

`CommandSpec` 增加 `env_remove: Vec<String>` 字段以支撑环境变量剥离。

## 决策矩阵

```rust
#[derive(Debug, PartialEq, Eq)]
pub enum LaunchPlan {
    ForegroundCreate,
    ForegroundAttach,
    BackgroundCreate,
    AlreadyRunningHint,
}

pub fn plan_launch(in_zellij: bool, session_exists: bool) -> LaunchPlan {
    match (in_zellij, session_exists) {
        (false, false) => LaunchPlan::ForegroundCreate,
        (false, true)  => LaunchPlan::ForegroundAttach,
        (true,  false) => LaunchPlan::BackgroundCreate,
        (true,  true)  => LaunchPlan::AlreadyRunningHint,
    }
}

pub fn is_inside_zellij() -> bool {
    std::env::var_os("ZELLIJ").is_some()
        || std::env::var_os("ZELLIJ_SESSION_NAME").is_some()
}
```

`is_inside_zellij` 与 `plan_launch` 拆开是为可测：`plan_launch` 收 `bool`，与进程环境解耦。

## 提示文案

`BackgroundCreate` 分支：

```
zellij session '<session_name>' is running in background.
Run `zootree open <workspace_name>` (outside zellij) to attach.
```

`AlreadyRunningHint` 分支：

```
zellij session '<session_name>' already exists.
Run `zootree open <workspace_name>` (outside zellij) to attach.
```

注意提示中的 `<workspace_name>` 是 workspace 名（`zootree open` 接的参数），与 `<session_name>`（可能是 `zootree-<name>` 或 shared 模式下的自定义名）区分开。

退出码：四个分支均 `Ok(())`，包括 `AlreadyRunningHint` —— "我已经启动了"是有效状态，不当作错误。

## 原语层改动

### `CommandSpec` 扩展

```rust
pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<String>,
    pub env: HashMap<String, String>,
    pub env_remove: Vec<String>,   // 新增
}
```

`RealRunner::run` / `run_interactive` 在应用 `env` 之前先：

```rust
for k in &spec.env_remove {
    cmd.env_remove(k);
}
```

`MockRunner::run` / `run_interactive` 当前手动 clone `CommandSpec` 字段（`runner.rs:81-86` 和 `92-97`）写入 `self.calls`；这两处 clone 也需补上 `env_remove: spec.env_remove.clone()`，确保断言可见。

所有现有 `CommandSpec { ... }` 字面量需补 `env_remove: vec![]`。不引入 builder/`Default` —— 项目当前风格为字面量构造，统一加字段更直白。

### `ZellijOps::start_session_background`

```rust
pub fn start_session_background(&self, session_name: &str, layout_path: &Path) -> Result<()> {
    info!("starting zellij session in background: {}", session_name);
    let spec = CommandSpec {
        program: "zellij".into(),
        args: vec![
            "-l".into(),
            layout_path.to_string_lossy().into(),
            "attach".into(),
            "--create-background".into(),
            session_name.into(),
        ],
        cwd: None,
        env: HashMap::new(),
        env_remove: vec![
            "ZELLIJ".into(),
            "ZELLIJ_SESSION_NAME".into(),
            "ZELLIJ_PANE_ID".into(),
        ],
    };
    let output = self.runner.run(&spec)?;
    if !output.status.success() {
        bail!(
            "zellij background session create failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}
```

要点：
- 用 `runner.run`（非交互），不接管终端。
- 不复用现有 `self.zellij(...)` helper —— 后者无 `env_remove` 入口；这里直接构造 `CommandSpec` 让意图显式。
- `-l <layout>` 是 zellij 顶层 flag，必须出现在 `attach` 子命令之前。
- 三个待剥离的环境变量名为实测验证过的最小集。

## 编排层改动

`launch_zellij` 增加 `in_zellij: bool` 参数（依赖注入，避免测试时操作进程级 env）。外层入口（`handle_open` / `handle_start` / `handle_create`）在调用前一次性 `crate::core::zellij::is_inside_zellij()`。

尾段改写为：

```rust
let in_zellij = ...; // 来自参数
let exists = zellij.session_exists(&session_name)?;
let plan = plan_launch(in_zellij, exists);

match plan {
    LaunchPlan::ForegroundCreate => {
        zellij.start_session(&session_name, &layout_file)?;
    }
    LaunchPlan::ForegroundAttach => {
        zellij.attach_session(&session_name)?;
    }
    LaunchPlan::BackgroundCreate => {
        zellij.start_session_background(&session_name, &layout_file)?;
        println!(
            "zellij session '{}' is running in background.",
            session_name
        );
        println!(
            "Run `zootree open {}` (outside zellij) to attach.",
            workspace.name
        );
    }
    LaunchPlan::AlreadyRunningHint => {
        println!("zellij session '{}' already exists.", session_name);
        println!(
            "Run `zootree open {}` (outside zellij) to attach.",
            workspace.name
        );
    }
}

Ok(())
```

### 移除现有隐式 fallback

当前代码：

```rust
match zellij.start_session(&session_name, &layout_file) {
    Ok(()) => {}
    Err(e) => {
        tracing::warn!("start_session failed ({}), trying attach", e);
        zellij.attach_session(&session_name)?;
    }
}
```

**删除**。`session_exists` 已显式判定状态，盲目 fallback 会掩盖真实错误（例如 layout 文件损坏被误判为"已存在"）。

## 边界情况

- **`shared` session 模式**：session 名来自配置；提示文案中 `zootree open <workspace_name>` 仍然有效，无需特判。
- **session 存在但 dead**（zellij resurrection）：`session_exists` 通过 grep `list-sessions` 输出，dead session 也会出现 → 落入 `AlreadyRunningHint` → 用户调 `zootree open` 时由 zellij 自然处理（询问是否复活）。
- **`add_tab` 路径**：非交互调用，不受影响。
- **`--run-agent` 语义**：detached session 照常解析 layout 并 spawn `command=...` panes。Agent 真实启动，输出缓冲在 pane，attach 后可见历史。

## 测试策略

### 纯函数（追加到 `tests/zellij_test.rs`）

`plan_launch` 4 入口穷举：

```rust
assert_eq!(plan_launch(false, false), LaunchPlan::ForegroundCreate);
assert_eq!(plan_launch(false, true),  LaunchPlan::ForegroundAttach);
assert_eq!(plan_launch(true,  false), LaunchPlan::BackgroundCreate);
assert_eq!(plan_launch(true,  true),  LaunchPlan::AlreadyRunningHint);
```

`is_inside_zellij()` 不单测（stdlib wrapper）。

### 原语层（追加到 `tests/zellij_test.rs`）

用 `MockRunner` 断言 `start_session_background`：

- 命令为 `zellij`、参数顺序正确、`env_remove` 包含三个 ZELLIJ 变量。
- 失败路径：通过 `MockRunner::push_response(Output)` 注入非零退出 + stderr 后，错误信息应包含 stderr。

### 编排层（追加到 `tests/start_agent_test.rs` 或新文件）

通过 MockRunner + 直接传 `in_zellij` 参数（不操作 env）覆盖：

- `BackgroundCreate`：list-sessions（不含目标）+ attach --create-background；stdout 含 "running in background"。
- `AlreadyRunningHint`：list-sessions（含目标），无后续命令；stdout 含 "already exists"。
- `ForegroundCreate` / `ForegroundAttach`：保留现有覆盖（如有）。

## 实施顺序提示（供后续 plan 参考）

1. 扩展 `CommandSpec` 字段 + `RealRunner` / `MockRunner` 同步 → 既有测试全过。
2. 在 `core/zellij.rs` 加 `LaunchPlan` / `plan_launch` / `is_inside_zellij` + 单测。
3. 在 `ZellijOps` 加 `start_session_background` + MockRunner 断言。
4. 改 `launch_zellij` 签名加 `in_zellij`，外层入口传入；删除旧 fallback；加编排测试。
5. 全量 `cargo test`，手测：在 zellij 内 `zootree start <ws>` 应后台创建 + 打印提示；外部 `zootree open <ws>` 应能 attach 到该 session 并看到 agent 输出。
