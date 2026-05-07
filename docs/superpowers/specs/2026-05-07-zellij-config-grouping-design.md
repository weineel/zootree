# zellij 配置分组重构

## 动机

当前 zellij 相关配置项 (`layout`, `session_mode`, `session_name`, `zellij_layout`) 散落在 GlobalConfig、TemplateConfig、WorkspaceConfig、RepoConfig 的顶层字段中。将它们收敛到 `[zellij]` 分组，结构更清晰，与现有的 `[hooks]`、`[log]`、`[lazygit]` 分组风格一致。

## 不兼容变更

**破坏性变更，不做向后兼容。** 用户需手工迁移配置。

## 新增 `ZellijConfig`

统一结构体，所有字段 `Option`，放在 `src/config/global.rs`：

```rust
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct ZellijConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layout: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_name: Option<String>,
}
```

## 各配置结构体变更

### GlobalConfig

- 移除 `zellij_layout: String`
- 新增 `zellij: ZellijConfig`，`#[serde(default)]`
- 默认值在 `impl Default` 中设置 `layout: Some("default".into())`

### TemplateConfig

- 移除 `layout: Option<String>`、`session_mode: Option<String>`
- 新增 `zellij: ZellijConfig`，`#[serde(default)]`

### WorkspaceConfig

- 移除 `layout: Option<String>`、`session_mode: String`、`session_name: Option<String>`
- 新增 `zellij: ZellijConfig`，`#[serde(default)]`
- 默认值在 `impl Default` 没有，而是在构造时设置 `session_mode: Some("standalone".into())`

### RepoConfig

- 移除 `layout: Option<String>`
- 新增 `zellij: Option<ZellijConfig>`

## 使用侧改动

`src/cli/workspace.rs` 和 `src/cli/template.rs` 中所有字段访问路径更新：

- `workspace.layout` → `workspace.zellij.layout`
- `workspace.session_mode` → `workspace.zellij.session_mode`
- `workspace.session_name` → `workspace.zellij.session_name`
- `global.zellij_layout` → `global.zellij.layout`

## 文档同步

- `README.md` / `README.zh-CN.md` — 更新配置示例
- `skills/zootree-dev/SKILL.md` — 更新代码约定和示例
- `skills/zootree-usage/SKILL.md` — 更新配置示例

## 不涉及

- CLI 参数不变
- `core/zellij.rs` 不变
- `core/layout.rs` 不变
