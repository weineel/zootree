# zellij 配置分组重构 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将散落在各 config struct 中的 zellij 相关字段收敛到统一的 `[zellij]` 分组下

**Architecture:** 新增 `ZellijConfig` 结构体（所有字段 Optional），GlobalConfig/TemplateConfig/WorkspaceConfig/RepoConfig 用 `#[serde(default)]` 嵌入，更新所有字段访问路径和文档

**Tech Stack:** Rust, serde, toml

---

### Task 1: 新增 ZellijConfig 并更新 GlobalConfig

**Files:**
- Modify: `src/config/global.rs`

- [ ] **Step 1: 添加 ZellijConfig 结构体**

在 `LogConfig` 的 `impl Default` 块之前插入：

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

- [ ] **Step 2: 更新 GlobalConfig struct**

将 `GlobalConfig` 中的 `zellij_layout` 字段替换为 `zellij`：

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GlobalConfig {
    #[serde(default)]
    pub zellij: ZellijConfig,
    #[serde(default = "default_workspace_root")]
    pub workspace_root: String,
    #[serde(default = "default_branch_prefix")]
    pub branch_prefix: String,
    #[serde(default)]
    pub copy_files: Vec<String>,
    #[serde(default)]
    pub hooks: HooksConfig,
    #[serde(default)]
    pub log: LogConfig,
}
```

- [ ] **Step 3: 更新 GlobalConfig 的 Default impl**

```rust
impl Default for GlobalConfig {
    fn default() -> Self {
        Self {
            zellij: ZellijConfig {
                layout: Some("default".into()),
                ..Default::default()
            },
            workspace_root: default_workspace_root(),
            branch_prefix: default_branch_prefix(),
            copy_files: Vec::new(),
            hooks: HooksConfig::default(),
            log: LogConfig::default(),
        }
    }
}
```

- [ ] **Step 4: 删除不再需要的函数和代码**

删除：
- `default_zellij_layout()` 函数
- `#[serde(default = "default_zellij_layout")]` 已在上一步移除

- [ ] **Step 5: 编译检查**

```bash
cargo check 2>&1
```

Expected: 有编译错误（其他文件还在引用旧字段），确认都是字段访问路径变更导致的，不是结构体定义问题。

- [ ] **Step 6: Commit**

```bash
git add src/config/global.rs
git commit -m "feat: add ZellijConfig and update GlobalConfig to use [zellij] grouping"
```

---

### Task 2: 更新 TemplateConfig

**Files:**
- Modify: `src/config/template.rs`

- [ ] **Step 1: 替换字段**

```rust
use serde::{Deserialize, Serialize};
use super::global::ZellijConfig;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TemplateConfig {
    #[serde(default)]
    pub repos: Vec<String>,
    #[serde(default)]
    pub zellij: ZellijConfig,
}
```

移除 `layout: Option<String>` 和 `session_mode: Option<String>`。

- [ ] **Step 2: Commit**

```bash
git add src/config/template.rs
git commit -m "feat: update TemplateConfig to use ZellijConfig grouping"
```

---

### Task 3: 更新 WorkspaceConfig

**Files:**
- Modify: `src/config/workspace.rs`

- [ ] **Step 1: 替换字段**

移除 `layout: Option<String>`、`session_mode: String`、`session_name: Option<String>`，替换为 `zellij: ZellijConfig`：

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkspaceConfig {
    pub title: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub branch: String,
    pub workspace_dir: String,
    pub created_at: String,
    #[serde(default)]
    pub zellij: ZellijConfig,
    #[serde(default)]
    pub repos: Vec<RepoEntry>,
    #[serde(default)]
    pub events: Vec<Event>,
}
```

- [ ] **Step 2: 删除 default_session_mode 函数**

移除文件末尾的：
```rust
fn default_session_mode() -> String {
    "standalone".into()
}
```

- [ ] **Step 3: Commit**

```bash
git add src/config/workspace.rs
git commit -m "feat: update WorkspaceConfig to use ZellijConfig grouping"
```

---

### Task 4: 更新 RepoConfig

**Files:**
- Modify: `src/config/repo.rs`

- [ ] **Step 1: 替换 layout 字段**

```rust
use serde::{Deserialize, Serialize};
use super::global::{HooksConfig, ZellijConfig};

// LazyGitConfig 保持不变 ...

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RepoConfig {
    pub path: String,
    pub default_target_branch: Option<String>,
    #[serde(default)]
    pub copy_files: Vec<String>,
    #[serde(default)]
    pub hooks: HooksConfig,
    pub lazygit: Option<LazyGitConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zellij: Option<ZellijConfig>,
}
```

移除 `layout: Option<String>`，添加 `zellij: Option<ZellijConfig>`。

- [ ] **Step 2: 更新 src/cli/repo.rs:62 中的 RepoConfig 构造**

原代码：
```rust
layout: None,
```

改为：
```rust
zellij: None,
```

- [ ] **Step 3: Commit**

```bash
git add src/config/repo.rs
git commit -m "feat: update RepoConfig to use ZellijConfig grouping"
```

---

### Task 5: 更新 workspace.rs 中的字段访问

**Files:**
- Modify: `src/cli/workspace.rs`

- [ ] **Step 1: 更新 WorkspaceConfig 构造 (行 127-129)**

原代码：
```rust
layout: None,
session_mode: "standalone".into(),
session_name: None,
```

改为：
```rust
zellij: ZellijConfig {
    session_mode: Some("standalone".into()),
    ..Default::default()
},
```

注意需要导入 `ZellijConfig`：在文件顶部的 use 语句中添加 `use crate::config::global::ZellijConfig;`

- [ ] **Step 2: 更新 TemplateConfig 构造 (行 142-143)**

原代码：
```rust
layout: workspace.layout.clone(),
session_mode: Some(workspace.session_mode.clone()),
```

改为：
```rust
zellij: workspace.zellij.clone(),
```

- [ ] **Step 3: 更新 layout 解析 (行 367-368)**

原代码：
```rust
let layout_name = workspace.layout.as_deref()
    .unwrap_or(&global.zellij_layout);
```

改为：
```rust
let layout_name = workspace.zellij.layout.as_deref()
    .or(global.zellij.layout.as_deref())
    .unwrap_or("default");
```

- [ ] **Step 4: 更新 session_name 构造 (行 406-410)**

原代码：
```rust
let session_name = match workspace.session_mode.as_str() {
    "shared" => workspace.session_name.clone()
        .ok_or_else(|| anyhow::anyhow!("shared mode requires session_name"))?,
    _ => format!("zootree-{}", workspace.name),
};
```

改为：
```rust
let session_name = match workspace.zellij.session_mode.as_deref() {
    Some("shared") => workspace.zellij.session_name.clone()
        .ok_or_else(|| anyhow::anyhow!("shared mode requires session_name"))?,
    _ => format!("zootree-{}", workspace.name),
};
```

- [ ] **Step 5: 更新 done 中的 session_name (行 627-630)**

原代码：
```rust
let session_name = match workspace.session_mode.as_str() {
    "shared" => workspace.session_name.clone(),
    _ => Some(format!("zootree-{}", workspace.name)),
};
```

改为：
```rust
let session_name = match workspace.zellij.session_mode.as_deref() {
    Some("shared") => workspace.zellij.session_name.clone(),
    _ => Some(format!("zootree-{}", workspace.name)),
};
```

- [ ] **Step 6: 更新 cancel 中的 session_name (行 740-743)**

原代码：
```rust
let session_name = match workspace.session_mode.as_str() {
    "shared" => workspace.session_name.clone(),
    _ => Some(format!("zootree-{}", workspace.name)),
};
```

改为：
```rust
let session_name = match workspace.zellij.session_mode.as_deref() {
    Some("shared") => workspace.zellij.session_name.clone(),
    _ => Some(format!("zootree-{}", workspace.name)),
};
```

- [ ] **Step 7: 检查是否有遗漏**

```bash
grep -n "\.layout\|\.session_mode\|\.session_name\|zellij_layout" src/cli/workspace.rs | grep -v zellij\.layout | grep -v zellij\.session
```

Expected: 无输出（所有访问都已更新）

- [ ] **Step 8: Commit**

```bash
git add src/cli/workspace.rs
git commit -m "feat: update workspace CLI to use workspace.zellij.* access paths"
```

---

### Task 6: 更新 template.rs 中的字段访问

**Files:**
- Modify: `src/cli/template.rs`

- [ ] **Step 1: 更新 TemplateConfig 构造 (行 46-47)**

原代码：
```rust
layout: workspace.layout.clone(),
session_mode: Some(workspace.session_mode.clone()),
```

改为：
```rust
zellij: workspace.zellij.clone(),
```

- [ ] **Step 2: Commit**

```bash
git add src/cli/template.rs
git commit -m "feat: update template CLI to use workspace.zellij access"
```

---

### Task 7: 编译验证 + 修复

**Files:**
- 可能有遗漏的文件

- [ ] **Step 1: 编译检查**

```bash
cargo build 2>&1
```

确认编译通过。如果还有编译错误，定位并修复遗漏的字段访问。

- [ ] **Step 2: 运行测试**

```bash
cargo test 2>&1
```

确认所有测试通过。

---

### Task 8: 更新 README.md

**Files:**
- Modify: `README.md`

- [ ] **Step 1: 更新 Global config 示例 (行 133-148)**

将：
```toml
default_layout = "default"
workspace_root = "~/zootree-workspaces"
branch_prefix = "zootree"
copy_files = [".env"]
```

改为：
```toml
workspace_root = "~/zootree-workspaces"
branch_prefix = "zootree"
copy_files = [".env"]

[zellij]
layout = "default"
```

注意 `[zellij]` 放在 `[hooks]` 之前。

- [ ] **Step 2: Commit**

```bash
git add README.md
git commit -m "docs: update README config example with [zellij] grouping"
```

---

### Task 9: 更新 README.zh-CN.md

**Files:**
- Modify: `README.zh-CN.md`

- [ ] **Step 1: 更新配置示例，同 Task 8**

将：
```toml
default_layout = "default"
```

改为：
```toml
[zellij]
layout = "default"
```

- [ ] **Step 2: Commit**

```bash
git add README.zh-CN.md
git commit -m "docs: update README.zh-CN config example with [zellij] grouping"
```

---

### Task 10: 更新 zootree-dev SKILL.md

**Files:**
- Modify: `skills/zootree-dev/SKILL.md`

- [ ] **Step 1: 更新代码约定 — 添加 ZellijConfig 约定**

在 `skills/zootree-dev/SKILL.md` 的 "代码约定" 部分，`- **untagged enum**` 行之后添加：

```markdown
- **zellij 分组**: 所有 zellij 相关配置统一在 `ZellijConfig` 中（`src/config/global.rs`），字段 Option，用 `#[serde(default)]` 嵌入各配置 struct
```

- [ ] **Step 2: Commit**

```bash
git add skills/zootree-dev/SKILL.md
git commit -m "docs: update zootree-dev skill with ZellijConfig convention"
```

---

### Task 11: 更新 zootree-usage SKILL.md

**Files:**
- Modify: `skills/zootree-usage/SKILL.md`

- [ ] **Step 1: 更新全局配置示例 (行 130-148)**

将：
```toml
default_layout = "default"
workspace_root = "~/zootree-workspaces"
```

改为：
```toml
workspace_root = "~/zootree-workspaces"
...
[zellij]
layout = "default"
```

- [ ] **Step 2: Commit**

```bash
git add skills/zootree-usage/SKILL.md
git commit -m "docs: update zootree-usage skill config examples with [zellij]"
```

---

### Task 12: 最终验证

- [ ] **Step 1: 完整构建**

```bash
cargo build 2>&1
```

Expected: 编译成功

- [ ] **Step 2: 运行全部测试**

```bash
cargo test 2>&1
```

Expected: 全部测试通过

- [ ] **Step 3: 检查 git log**

```bash
git log --oneline -15
```

确认提交历史清晰，每个 commit 聚焦单一变更。

- [ ] **Step 4: 最终检查未提交变更**

```bash
git status
```

Expected: clean

---

### 变更汇总

| 文件 | 变更类型 |
|------|----------|
| `src/config/global.rs` | 新增 `ZellijConfig`，`GlobalConfig` 字段调整 |
| `src/config/template.rs` | `layout`/`session_mode` → `zellij: ZellijConfig` |
| `src/config/workspace.rs` | `layout`/`session_mode`/`session_name` → `zellij: ZellijConfig` |
| `src/config/repo.rs` | `layout` → `zellij: Option<ZellijConfig>` |
| `src/cli/workspace.rs` | 所有字段访问路径更新 |
| `src/cli/template.rs` | 字段访问路径更新 |
| `README.md` | 配置示例 |
| `README.zh-CN.md` | 配置示例 |
| `skills/zootree-dev/SKILL.md` | 代码约定 |
| `skills/zootree-usage/SKILL.md` | 配置示例 |
