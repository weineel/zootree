# Zootree Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a Rust CLI tool for managing multi-repo collaborative development workspaces using git worktree and zellij.

**Architecture:** CLI (clap derive) → config layer (TOML/KDL serde) → core operations (git worktree, hook engine, layout renderer) → external commands (git, zellij, lazygit). All external command execution goes through a `CommandRunner` trait for testability.

**Tech Stack:** Rust 1.94, clap 4, dialoguer, serde + toml, kdl 6, tracing, glob

---

## File Structure

```
zootree/
├── Cargo.toml
├── src/
│   ├── main.rs                  # CLI entry, tracing init
│   ├── cli/
│   │   ├── mod.rs               # Top-level Cli enum (clap derive)
│   │   ├── repo.rs              # RepoCmd: add/list/edit/remove
│   │   ├── workspace.rs         # WorkspaceCmd: create/list/start/open/done/cancel
│   │   ├── template.rs          # TemplateCmd: list/save
│   │   └── prune.rs             # PruneCmd
│   ├── config/
│   │   ├── mod.rs               # ConfigManager: load/save, path helpers
│   │   ├── global.rs            # GlobalConfig struct
│   │   ├── repo.rs              # RepoConfig struct
│   │   ├── workspace.rs         # WorkspaceConfig, WorkspaceStatus, RepoEntry, Event
│   │   └── template.rs          # TemplateConfig struct
│   ├── core/
│   │   ├── mod.rs
│   │   ├── git.rs               # GitOps: worktree add/remove, merge, push, branch
│   │   ├── hook.rs              # HookEngine: parse config, build command, execute
│   │   ├── layout.rs            # LayoutRenderer: KDL parse, repeat-per-repo, var replace
│   │   ├── zellij.rs            # ZellijOps: session create/attach/kill, tab add/remove
│   │   ├── copy_files.rs        # FileCopier: glob resolve, copy from repo to worktree
│   │   └── name_gen.rs          # NameGenerator: adjective-noun random names
│   ├── runner.rs                # CommandRunner trait + RealRunner impl
│   └── tui.rs                   # Interactive prompts: select repos, input title, etc.
├── tests/
│   ├── config_test.rs           # Config parsing tests
│   ├── git_test.rs              # Git command assembly tests
│   ├── hook_test.rs             # Hook parsing and command build tests
│   ├── layout_test.rs           # KDL template rendering tests
│   ├── copy_files_test.rs       # Glob and merge tests
│   └── name_gen_test.rs         # Name generation tests
```

---

### Task 1: Project Scaffold + CommandRunner Trait

**Files:**
- Create: `Cargo.toml`
- Create: `src/main.rs`
- Create: `src/runner.rs`

- [ ] **Step 1: Initialize Cargo project**

```bash
cd /Users/lijufeng/project/weineel/zootree
cargo init --name zootree
```

- [ ] **Step 2: Set up Cargo.toml with dependencies**

Replace `Cargo.toml`:

```toml
[package]
name = "zootree"
version = "0.1.0"
edition = "2021"

[dependencies]
clap = { version = "4", features = ["derive"] }
dialoguer = "0.11"
toml = "0.8"
serde = { version = "1", features = ["derive"] }
kdl = "6"
dirs = "6"
glob = "0.3"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
tracing-appender = "0.2"
chrono = { version = "0.4", features = ["serde"] }
rand = "0.8"
anyhow = "1"
```

- [ ] **Step 3: Write CommandRunner trait**

Create `src/runner.rs`:

```rust
use std::collections::HashMap;
use std::path::Path;
use std::process::{Command, Output};
use anyhow::Result;

pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<String>,
    pub env: HashMap<String, String>,
}

pub trait CommandRunner {
    fn run(&self, spec: &CommandSpec) -> Result<Output>;
}

pub struct RealRunner;

impl CommandRunner for RealRunner {
    fn run(&self, spec: &CommandSpec) -> Result<Output> {
        let mut cmd = Command::new(&spec.program);
        cmd.args(&spec.args);
        if let Some(cwd) = &spec.cwd {
            cmd.current_dir(cwd);
        }
        for (k, v) in &spec.env {
            cmd.env(k, v);
        }
        let output = cmd.output()?;
        Ok(output)
    }
}

#[cfg(test)]
pub struct MockRunner {
    pub calls: std::cell::RefCell<Vec<CommandSpec>>,
    pub responses: std::cell::RefCell<Vec<Output>>,
}

#[cfg(test)]
impl MockRunner {
    pub fn new() -> Self {
        Self {
            calls: std::cell::RefCell::new(Vec::new()),
            responses: std::cell::RefCell::new(Vec::new()),
        }
    }

    pub fn push_response(&self, output: Output) {
        self.responses.borrow_mut().push(output);
    }

    pub fn take_calls(&self) -> Vec<CommandSpec> {
        self.calls.borrow_mut().drain(..).collect()
    }
}

#[cfg(test)]
impl CommandRunner for MockRunner {
    fn run(&self, spec: &CommandSpec) -> Result<Output> {
        self.calls.borrow_mut().push(CommandSpec {
            program: spec.program.clone(),
            args: spec.args.clone(),
            cwd: spec.cwd.clone(),
            env: spec.env.clone(),
        });
        let output = self.responses.borrow_mut().remove(0);
        Ok(output)
    }
}
```

- [ ] **Step 4: Write minimal main.rs**

```rust
mod runner;

fn main() {
    println!("zootree");
}
```

- [ ] **Step 5: Verify it compiles**

Run: `cargo build`
Expected: compiles without errors

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock src/
git commit -m "feat: project scaffold with CommandRunner trait"
```

---

### Task 2: Config Layer — GlobalConfig + ConfigManager

**Files:**
- Create: `src/config/mod.rs`
- Create: `src/config/global.rs`
- Create: `tests/config_test.rs`

- [ ] **Step 1: Write failing test for GlobalConfig parsing**

Create `tests/config_test.rs`:

```rust
use zootree::config::global::GlobalConfig;

#[test]
fn test_parse_global_config_full() {
    let toml_str = r#"
default_layout = "default"
workspace_root = "~/zootree-workspaces"
branch_prefix = "zootree"
copy_files = [".env"]

[hooks]
post_create = "echo hello"

[log]
max_files = 5
"#;
    let config: GlobalConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.default_layout, "default");
    assert_eq!(config.workspace_root, "~/zootree-workspaces");
    assert_eq!(config.branch_prefix, "zootree");
    assert_eq!(config.copy_files, vec![".env"]);
    assert_eq!(config.hooks.post_create, Some(zootree::config::global::HookValue::Simple("echo hello".into())));
    assert_eq!(config.log.max_files, Some(5));
}

#[test]
fn test_parse_global_config_defaults() {
    let toml_str = "";
    let config: GlobalConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.default_layout, "default");
    assert_eq!(config.branch_prefix, "zootree");
    assert!(config.copy_files.is_empty());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test config_test`
Expected: FAIL — module not found

- [ ] **Step 3: Implement GlobalConfig**

Create `src/config/global.rs`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum HookValue {
    Simple(String),
    File { file: String },
    Inline { inline: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct HooksConfig {
    pub post_create: Option<HookValue>,
    pub pre_remove: Option<HookValue>,
    pub post_start: Option<HookValue>,
    pub pre_done: Option<HookValue>,
    pub pre_cancel: Option<HookValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LogConfig {
    pub dir: Option<String>,
    pub max_files: Option<u32>,
    pub max_size: Option<String>,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            dir: None,
            max_files: Some(5),
            max_size: Some("10MB".into()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GlobalConfig {
    #[serde(default = "default_layout")]
    pub default_layout: String,
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

fn default_layout() -> String { "default".into() }
fn default_workspace_root() -> String { "~/zootree-workspaces".into() }
fn default_branch_prefix() -> String { "zootree".into() }

impl Default for GlobalConfig {
    fn default() -> Self {
        Self {
            default_layout: default_layout(),
            workspace_root: default_workspace_root(),
            branch_prefix: default_branch_prefix(),
            copy_files: Vec::new(),
            hooks: HooksConfig::default(),
            log: LogConfig::default(),
        }
    }
}
```

- [ ] **Step 4: Create config/mod.rs with ConfigManager**

Create `src/config/mod.rs`:

```rust
pub mod global;

use std::path::PathBuf;
use anyhow::Result;

pub struct ConfigManager {
    pub base_dir: PathBuf,
}

impl ConfigManager {
    pub fn new() -> Result<Self> {
        let base_dir = dirs::config_dir()
            .ok_or_else(|| anyhow::anyhow!("cannot find config directory"))?
            .join("zootree");
        Ok(Self { base_dir })
    }

    pub fn with_base_dir(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    pub fn ensure_dirs(&self) -> Result<()> {
        let dirs = [
            "repos", "layouts", "templates",
            "workspaces/pending", "workspaces/in_progress",
            "workspaces/archived/done", "workspaces/archived/canceled",
            "logs",
        ];
        for d in dirs {
            std::fs::create_dir_all(self.base_dir.join(d))?;
        }
        Ok(())
    }

    pub fn load_global_config(&self) -> Result<global::GlobalConfig> {
        let path = self.base_dir.join("config.toml");
        if path.exists() {
            let content = std::fs::read_to_string(&path)?;
            Ok(toml::from_str(&content)?)
        } else {
            Ok(global::GlobalConfig::default())
        }
    }

    pub fn save_global_config(&self, config: &global::GlobalConfig) -> Result<()> {
        let path = self.base_dir.join("config.toml");
        let content = toml::to_string_pretty(config)?;
        std::fs::write(path, content)?;
        Ok(())
    }
}
```

- [ ] **Step 5: Export config module from lib.rs**

Create `src/lib.rs`:

```rust
pub mod config;
pub mod runner;
```

Update `src/main.rs`:

```rust
use anyhow::Result;

fn main() -> Result<()> {
    println!("zootree");
    Ok(())
}
```

- [ ] **Step 6: Run tests**

Run: `cargo test --test config_test`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add src/ tests/ Cargo.toml
git commit -m "feat: config layer with GlobalConfig and ConfigManager"
```

---

### Task 3: Config Layer — RepoConfig

**Files:**
- Create: `src/config/repo.rs`
- Modify: `src/config/mod.rs`
- Modify: `tests/config_test.rs`

- [ ] **Step 1: Write failing test for RepoConfig**

Append to `tests/config_test.rs`:

```rust
use zootree::config::repo::RepoConfig;
use zootree::config::global::HookValue;

#[test]
fn test_parse_repo_config() {
    let toml_str = r#"
path = "~/projects/frontend"
default_target_branch = "develop"
copy_files = [".env.local", ".vscode/settings.json"]

[hooks]
post_create = "npm install"

[hooks.pre_remove]
file = "~/.config/zootree/hooks/cleanup.sh"

[lazygit]
config = "~/projects/frontend/.lazygit.yml"
"#;
    let config: RepoConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.path, "~/projects/frontend");
    assert_eq!(config.default_target_branch, Some("develop".into()));
    assert_eq!(config.copy_files, vec![".env.local", ".vscode/settings.json"]);
    assert_eq!(config.hooks.post_create, Some(HookValue::Simple("npm install".into())));
    assert_eq!(config.hooks.pre_remove, Some(HookValue::File { file: "~/.config/zootree/hooks/cleanup.sh".into() }));
    assert_eq!(config.lazygit.as_ref().unwrap().config, "~/projects/frontend/.lazygit.yml");
}

#[test]
fn test_parse_repo_config_minimal() {
    let toml_str = r#"
path = "~/projects/backend"
"#;
    let config: RepoConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.path, "~/projects/backend");
    assert!(config.default_target_branch.is_none());
    assert!(config.copy_files.is_empty());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test config_test test_parse_repo`
Expected: FAIL — module not found

- [ ] **Step 3: Implement RepoConfig**

Create `src/config/repo.rs`:

```rust
use serde::{Deserialize, Serialize};
use super::global::HooksConfig;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LazyGitConfig {
    pub config: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RepoConfig {
    pub path: String,
    pub default_target_branch: Option<String>,
    #[serde(default)]
    pub copy_files: Vec<String>,
    #[serde(default)]
    pub hooks: HooksConfig,
    pub lazygit: Option<LazyGitConfig>,
    pub layout: Option<String>,
}
```

- [ ] **Step 4: Add repo methods to ConfigManager**

Append to `src/config/mod.rs`:

```rust
pub mod repo;

// Add to ConfigManager impl:
impl ConfigManager {
    pub fn repos_dir(&self) -> PathBuf {
        self.base_dir.join("repos")
    }

    pub fn load_repo_config(&self, name: &str) -> Result<repo::RepoConfig> {
        let path = self.repos_dir().join(format!("{}.toml", name));
        let content = std::fs::read_to_string(&path)?;
        Ok(toml::from_str(&content)?)
    }

    pub fn save_repo_config(&self, name: &str, config: &repo::RepoConfig) -> Result<()> {
        let path = self.repos_dir().join(format!("{}.toml", name));
        let content = toml::to_string_pretty(config)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    pub fn list_repos(&self) -> Result<Vec<String>> {
        let dir = self.repos_dir();
        let mut names = Vec::new();
        if dir.exists() {
            for entry in std::fs::read_dir(dir)? {
                let entry = entry?;
                if let Some(name) = entry.path().file_stem() {
                    if entry.path().extension().map_or(false, |e| e == "toml") {
                        names.push(name.to_string_lossy().into_owned());
                    }
                }
            }
        }
        names.sort();
        Ok(names)
    }

    pub fn remove_repo_config(&self, name: &str) -> Result<()> {
        let path = self.repos_dir().join(format!("{}.toml", name));
        std::fs::remove_file(path)?;
        Ok(())
    }
}
```

- [ ] **Step 5: Export repo module**

Update `src/config/mod.rs` to include `pub mod repo;`

- [ ] **Step 6: Run tests**

Run: `cargo test --test config_test`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add src/ tests/
git commit -m "feat: RepoConfig with hooks, lazygit, copy_files"
```

---

### Task 4: Config Layer — WorkspaceConfig + TemplateConfig

**Files:**
- Create: `src/config/workspace.rs`
- Create: `src/config/template.rs`
- Modify: `src/config/mod.rs`
- Modify: `tests/config_test.rs`

- [ ] **Step 1: Write failing test for WorkspaceConfig**

Append to `tests/config_test.rs`:

```rust
use zootree::config::workspace::{WorkspaceConfig, WorkspaceStatus, RepoEntry, Event};

#[test]
fn test_parse_workspace_config() {
    let toml_str = r#"
title = "用户认证功能"
name = "calm-river"
description = "前后端联调 OAuth2 登录"
branch = "zootree/calm-river"
workspace_dir = "~/zootree-workspaces/calm-river"
created_at = "2026-04-28T10:30:00+08:00"

[[repos]]
name = "frontend"
target_branch = "develop"

[[repos]]
name = "backend"
target_branch = "develop"

[[events]]
action = "created"
timestamp = "2026-04-28T10:30:00+08:00"
"#;
    let config: WorkspaceConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.title, "用户认证功能");
    assert_eq!(config.name, "calm-river");
    assert_eq!(config.repos.len(), 2);
    assert_eq!(config.repos[0].name, "frontend");
    assert_eq!(config.repos[0].target_branch, "develop");
    assert_eq!(config.events.len(), 1);
}

#[test]
fn test_parse_template_config() {
    let toml_str = r#"
repos = ["frontend", "backend", "shared-lib"]
layout = "default"
session_mode = "standalone"
"#;
    let config: zootree::config::template::TemplateConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.repos, vec!["frontend", "backend", "shared-lib"]);
    assert_eq!(config.layout, Some("default".into()));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test config_test test_parse_workspace`
Expected: FAIL

- [ ] **Step 3: Implement WorkspaceConfig**

Create `src/config/workspace.rs`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceStatus {
    Pending,
    InProgress,
    Done,
    Canceled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RepoEntry {
    pub name: String,
    pub target_branch: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Event {
    pub action: String,
    pub timestamp: String,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkspaceConfig {
    pub title: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub branch: String,
    pub workspace_dir: String,
    pub created_at: String,
    pub layout: Option<String>,
    #[serde(default = "default_session_mode")]
    pub session_mode: String,
    pub session_name: Option<String>,
    #[serde(default)]
    pub repos: Vec<RepoEntry>,
    #[serde(default)]
    pub events: Vec<Event>,
}

fn default_session_mode() -> String { "standalone".into() }
```

- [ ] **Step 4: Implement TemplateConfig**

Create `src/config/template.rs`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TemplateConfig {
    #[serde(default)]
    pub repos: Vec<String>,
    pub layout: Option<String>,
    pub session_mode: Option<String>,
}
```

- [ ] **Step 5: Add workspace/template methods to ConfigManager**

Add to `src/config/mod.rs`:

```rust
pub mod workspace;
pub mod template;

// Add to ConfigManager impl:
impl ConfigManager {
    fn workspace_status_dir(&self, status: &workspace::WorkspaceStatus) -> PathBuf {
        match status {
            workspace::WorkspaceStatus::Pending => self.base_dir.join("workspaces/pending"),
            workspace::WorkspaceStatus::InProgress => self.base_dir.join("workspaces/in_progress"),
            workspace::WorkspaceStatus::Done => self.base_dir.join("workspaces/archived/done"),
            workspace::WorkspaceStatus::Canceled => self.base_dir.join("workspaces/archived/canceled"),
        }
    }

    pub fn save_workspace(&self, status: &workspace::WorkspaceStatus, config: &workspace::WorkspaceConfig) -> Result<()> {
        let dir = self.workspace_status_dir(status);
        let path = dir.join(format!("{}.toml", config.name));
        let content = toml::to_string_pretty(config)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    pub fn load_workspace(&self, name: &str) -> Result<(workspace::WorkspaceStatus, workspace::WorkspaceConfig)> {
        use workspace::WorkspaceStatus::*;
        for status in [Pending, InProgress, Done, Canceled] {
            let path = self.workspace_status_dir(&status).join(format!("{}.toml", name));
            if path.exists() {
                let content = std::fs::read_to_string(&path)?;
                let config: workspace::WorkspaceConfig = toml::from_str(&content)?;
                return Ok((status, config));
            }
        }
        anyhow::bail!("workspace '{}' not found", name)
    }

    pub fn move_workspace(&self, name: &str, from: &workspace::WorkspaceStatus, to: &workspace::WorkspaceStatus) -> Result<()> {
        let from_path = self.workspace_status_dir(from).join(format!("{}.toml", name));
        let to_path = self.workspace_status_dir(to).join(format!("{}.toml", name));
        std::fs::rename(from_path, to_path)?;
        Ok(())
    }

    pub fn list_workspaces(&self, status: Option<&workspace::WorkspaceStatus>) -> Result<Vec<workspace::WorkspaceConfig>> {
        use workspace::WorkspaceStatus::*;
        let statuses = match status {
            Some(s) => vec![s.clone()],
            None => vec![Pending, InProgress, Done, Canceled],
        };
        let mut workspaces = Vec::new();
        for s in statuses {
            let dir = self.workspace_status_dir(&s);
            if dir.exists() {
                for entry in std::fs::read_dir(&dir)? {
                    let entry = entry?;
                    if entry.path().extension().map_or(false, |e| e == "toml") {
                        let content = std::fs::read_to_string(entry.path())?;
                        if let Ok(config) = toml::from_str(&content) {
                            workspaces.push(config);
                        }
                    }
                }
            }
        }
        Ok(workspaces)
    }

    pub fn delete_workspace_config(&self, name: &str, status: &workspace::WorkspaceStatus) -> Result<()> {
        let path = self.workspace_status_dir(status).join(format!("{}.toml", name));
        std::fs::remove_file(path)?;
        Ok(())
    }

    // Template methods
    fn templates_dir(&self) -> PathBuf {
        self.base_dir.join("templates")
    }

    pub fn load_template(&self, name: &str) -> Result<template::TemplateConfig> {
        let path = self.templates_dir().join(format!("{}.toml", name));
        let content = std::fs::read_to_string(&path)?;
        Ok(toml::from_str(&content)?)
    }

    pub fn save_template(&self, name: &str, config: &template::TemplateConfig) -> Result<()> {
        let path = self.templates_dir().join(format!("{}.toml", name));
        let content = toml::to_string_pretty(config)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    pub fn list_templates(&self) -> Result<Vec<String>> {
        let dir = self.templates_dir();
        let mut names = Vec::new();
        if dir.exists() {
            for entry in std::fs::read_dir(dir)? {
                let entry = entry?;
                if let Some(name) = entry.path().file_stem() {
                    if entry.path().extension().map_or(false, |e| e == "toml") {
                        names.push(name.to_string_lossy().into_owned());
                    }
                }
            }
        }
        names.sort();
        Ok(names)
    }
}
```

- [ ] **Step 6: Run tests**

Run: `cargo test --test config_test`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add src/ tests/
git commit -m "feat: WorkspaceConfig, TemplateConfig with status management"
```

---

### Task 5: Name Generator

**Files:**
- Create: `src/core/mod.rs`
- Create: `src/core/name_gen.rs`
- Create: `tests/name_gen_test.rs`

- [ ] **Step 1: Write failing test**

Create `tests/name_gen_test.rs`:

```rust
use zootree::core::name_gen::NameGenerator;

#[test]
fn test_generate_name_format() {
    let gen = NameGenerator::new();
    let name = gen.generate();
    let parts: Vec<&str> = name.split('-').collect();
    assert_eq!(parts.len(), 2, "name should be adjective-noun: {}", name);
    assert!(parts[0].chars().all(|c| c.is_ascii_lowercase()));
    assert!(parts[1].chars().all(|c| c.is_ascii_lowercase()));
}

#[test]
fn test_generate_unique_names() {
    let gen = NameGenerator::new();
    let names: Vec<String> = (0..20).map(|_| gen.generate()).collect();
    let unique: std::collections::HashSet<&String> = names.iter().collect();
    assert!(unique.len() > 1, "should generate different names");
}

#[test]
fn test_generate_with_existing_avoids_collision() {
    let gen = NameGenerator::new();
    let first = gen.generate();
    let existing = vec![first.clone()];
    let second = gen.generate_avoiding(&existing);
    assert_ne!(first, second);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test name_gen_test`
Expected: FAIL

- [ ] **Step 3: Implement NameGenerator**

Create `src/core/mod.rs`:

```rust
pub mod name_gen;
```

Create `src/core/name_gen.rs`:

```rust
use rand::seq::SliceRandom;

const ADJECTIVES: &[&str] = &[
    "bold", "brave", "calm", "cool", "dark", "deep", "fair", "fast",
    "free", "glad", "gold", "good", "keen", "kind", "late", "lean",
    "live", "long", "loud", "mild", "neat", "nice", "open", "pale",
    "pure", "rare", "rich", "safe", "slim", "soft", "sure", "tall",
    "thin", "true", "warm", "wide", "wild", "wise", "young", "keen",
];

const NOUNS: &[&str] = &[
    "arch", "bark", "beam", "bird", "bolt", "cave", "clay", "cove",
    "dawn", "deer", "dove", "dune", "dust", "fern", "fire", "fish",
    "ford", "fox", "gate", "glen", "glow", "hawk", "hill", "jade",
    "lake", "leaf", "lion", "lynx", "mist", "moon", "moss", "oak",
    "owl", "palm", "peak", "pine", "pond", "rain", "reed", "reef",
    "ridge", "river", "rock", "rose", "sage", "sand", "seal", "snow",
    "star", "stone", "swan", "tide", "tree", "vale", "vine", "wave",
    "wind", "wolf", "wood", "wren",
];

pub struct NameGenerator;

impl NameGenerator {
    pub fn new() -> Self {
        Self
    }

    pub fn generate(&self) -> String {
        let mut rng = rand::thread_rng();
        let adj = ADJECTIVES.choose(&mut rng).unwrap();
        let noun = NOUNS.choose(&mut rng).unwrap();
        format!("{}-{}", adj, noun)
    }

    pub fn generate_avoiding(&self, existing: &[String]) -> String {
        for _ in 0..100 {
            let name = self.generate();
            if !existing.contains(&name) {
                return name;
            }
        }
        let name = self.generate();
        format!("{}-{}", name, rand::random::<u16>() % 1000)
    }
}
```

- [ ] **Step 4: Export core module from lib.rs**

Update `src/lib.rs`:

```rust
pub mod config;
pub mod core;
pub mod runner;
```

- [ ] **Step 5: Run tests**

Run: `cargo test --test name_gen_test`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add src/ tests/
git commit -m "feat: workspace name generator (adjective-noun)"
```

---

### Task 6: Git Operations

**Files:**
- Create: `src/core/git.rs`
- Create: `tests/git_test.rs`

- [ ] **Step 1: Write failing test for worktree add command assembly**

Create `tests/git_test.rs`:

```rust
use zootree::core::git::GitOps;
use zootree::runner::MockRunner;
use std::os::unix::process::ExitStatusExt;
use std::process::{ExitStatus, Output};

fn success_output() -> Output {
    Output {
        status: ExitStatus::from_raw(0),
        stdout: Vec::new(),
        stderr: Vec::new(),
    }
}

#[test]
fn test_worktree_add_command() {
    let runner = MockRunner::new();
    runner.push_response(success_output());
    let git = GitOps::new(&runner);

    git.worktree_add(
        "/home/user/projects/frontend",
        "zootree/calm-river",
        "/home/user/zootree-workspaces/calm-river/frontend",
        "develop",
    ).unwrap();

    let calls = runner.take_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].program, "git");
    assert_eq!(calls[0].args, vec![
        "-C", "/home/user/projects/frontend",
        "worktree", "add",
        "-b", "zootree/calm-river",
        "/home/user/zootree-workspaces/calm-river/frontend",
        "develop",
    ]);
}

#[test]
fn test_worktree_remove_command() {
    let runner = MockRunner::new();
    runner.push_response(success_output());
    let git = GitOps::new(&runner);

    git.worktree_remove(
        "/home/user/projects/frontend",
        "/home/user/zootree-workspaces/calm-river/frontend",
        false,
    ).unwrap();

    let calls = runner.take_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].program, "git");
    assert_eq!(calls[0].args, vec![
        "-C", "/home/user/projects/frontend",
        "worktree", "remove",
        "/home/user/zootree-workspaces/calm-river/frontend",
    ]);
}

#[test]
fn test_worktree_remove_force() {
    let runner = MockRunner::new();
    runner.push_response(success_output());
    let git = GitOps::new(&runner);

    git.worktree_remove(
        "/home/user/projects/frontend",
        "/home/user/zootree-workspaces/calm-river/frontend",
        true,
    ).unwrap();

    let calls = runner.take_calls();
    assert_eq!(calls[0].args, vec![
        "-C", "/home/user/projects/frontend",
        "worktree", "remove", "--force",
        "/home/user/zootree-workspaces/calm-river/frontend",
    ]);
}

#[test]
fn test_merge_command() {
    let runner = MockRunner::new();
    runner.push_response(success_output()); // checkout
    runner.push_response(success_output()); // merge
    let git = GitOps::new(&runner);

    git.merge(
        "/home/user/projects/frontend",
        "zootree/calm-river",
        "develop",
        None,
    ).unwrap();

    let calls = runner.take_calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].args, vec!["-C", "/home/user/projects/frontend", "checkout", "develop"]);
    assert_eq!(calls[1].args, vec!["-C", "/home/user/projects/frontend", "merge", "zootree/calm-river"]);
}

#[test]
fn test_merge_squash() {
    let runner = MockRunner::new();
    runner.push_response(success_output()); // checkout
    runner.push_response(success_output()); // merge --squash
    runner.push_response(success_output()); // commit
    let git = GitOps::new(&runner);

    git.merge(
        "/home/user/projects/frontend",
        "zootree/calm-river",
        "develop",
        Some("squash"),
    ).unwrap();

    let calls = runner.take_calls();
    assert_eq!(calls.len(), 3);
    assert_eq!(calls[1].args, vec!["-C", "/home/user/projects/frontend", "merge", "--squash", "zootree/calm-river"]);
}

#[test]
fn test_push_command() {
    let runner = MockRunner::new();
    runner.push_response(success_output());
    let git = GitOps::new(&runner);

    git.push("/home/user/projects/frontend", "develop").unwrap();

    let calls = runner.take_calls();
    assert_eq!(calls[0].args, vec!["-C", "/home/user/projects/frontend", "push", "origin", "develop"]);
}

#[test]
fn test_delete_remote_branch() {
    let runner = MockRunner::new();
    runner.push_response(success_output());
    let git = GitOps::new(&runner);

    git.delete_remote_branch("/home/user/projects/frontend", "zootree/calm-river").unwrap();

    let calls = runner.take_calls();
    assert_eq!(calls[0].args, vec!["-C", "/home/user/projects/frontend", "push", "origin", "--delete", "zootree/calm-river"]);
}

#[test]
fn test_delete_local_branch() {
    let runner = MockRunner::new();
    runner.push_response(success_output());
    let git = GitOps::new(&runner);

    git.delete_local_branch("/home/user/projects/frontend", "zootree/calm-river", false).unwrap();

    let calls = runner.take_calls();
    assert_eq!(calls[0].args, vec!["-C", "/home/user/projects/frontend", "branch", "-d", "zootree/calm-river"]);
}

#[test]
fn test_has_uncommitted_changes() {
    let runner = MockRunner::new();
    runner.push_response(Output {
        status: ExitStatus::from_raw(0),
        stdout: b" M src/main.rs\n".to_vec(),
        stderr: Vec::new(),
    });
    let git = GitOps::new(&runner);

    let result = git.has_uncommitted_changes("/home/user/worktree/frontend").unwrap();
    assert!(result);

    let calls = runner.take_calls();
    assert_eq!(calls[0].args, vec!["-C", "/home/user/worktree/frontend", "status", "--porcelain"]);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test git_test`
Expected: FAIL

- [ ] **Step 3: Implement GitOps**

Create `src/core/git.rs`:

```rust
use crate::runner::{CommandRunner, CommandSpec};
use anyhow::{Result, bail};
use std::collections::HashMap;

pub struct GitOps<'a, R: CommandRunner> {
    runner: &'a R,
}

impl<'a, R: CommandRunner> GitOps<'a, R> {
    pub fn new(runner: &'a R) -> Self {
        Self { runner }
    }

    fn git(&self, repo_path: &str, args: Vec<&str>) -> Result<std::process::Output> {
        let mut full_args = vec!["-C".to_string(), repo_path.to_string()];
        full_args.extend(args.into_iter().map(String::from));
        let spec = CommandSpec {
            program: "git".into(),
            args: full_args,
            cwd: None,
            env: HashMap::new(),
        };
        let output = self.runner.run(&spec)?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("git command failed: {}", stderr);
        }
        Ok(output)
    }

    pub fn worktree_add(&self, repo_path: &str, branch: &str, worktree_path: &str, base: &str) -> Result<()> {
        self.git(repo_path, vec!["worktree", "add", "-b", branch, worktree_path, base])?;
        Ok(())
    }

    pub fn worktree_remove(&self, repo_path: &str, worktree_path: &str, force: bool) -> Result<()> {
        let mut args = vec!["worktree", "remove"];
        if force {
            args.push("--force");
        }
        args.push(worktree_path);
        self.git(repo_path, args)?;
        Ok(())
    }

    pub fn merge(&self, repo_path: &str, branch: &str, target: &str, strategy: Option<&str>) -> Result<()> {
        self.git(repo_path, vec!["checkout", target])?;
        match strategy {
            Some("squash") => {
                self.git(repo_path, vec!["merge", "--squash", branch])?;
                self.git(repo_path, vec!["commit", "-m", &format!("squash merge {}", branch)])?;
            }
            Some("rebase") => {
                self.git(repo_path, vec!["rebase", branch])?;
            }
            _ => {
                self.git(repo_path, vec!["merge", branch])?;
            }
        }
        Ok(())
    }

    pub fn push(&self, repo_path: &str, branch: &str) -> Result<()> {
        self.git(repo_path, vec!["push", "origin", branch])?;
        Ok(())
    }

    pub fn delete_remote_branch(&self, repo_path: &str, branch: &str) -> Result<()> {
        self.git(repo_path, vec!["push", "origin", "--delete", branch])?;
        Ok(())
    }

    pub fn delete_local_branch(&self, repo_path: &str, branch: &str, force: bool) -> Result<()> {
        let flag = if force { "-D" } else { "-d" };
        self.git(repo_path, vec!["branch", flag, branch])?;
        Ok(())
    }

    pub fn has_uncommitted_changes(&self, worktree_path: &str) -> Result<bool> {
        let spec = CommandSpec {
            program: "git".into(),
            args: vec!["-C".into(), worktree_path.into(), "status".into(), "--porcelain".into()],
            cwd: None,
            env: HashMap::new(),
        };
        let output = self.runner.run(&spec)?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(!stdout.trim().is_empty())
    }

    pub fn get_default_branch(&self, repo_path: &str) -> Result<String> {
        let spec = CommandSpec {
            program: "git".into(),
            args: vec![
                "-C".into(), repo_path.into(),
                "symbolic-ref".into(), "refs/remotes/origin/HEAD".into(),
                "--short".into(),
            ],
            cwd: None,
            env: HashMap::new(),
        };
        let output = self.runner.run(&spec)?;
        let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(branch.strip_prefix("origin/").unwrap_or(&branch).to_string())
    }

    pub fn list_branches(&self, repo_path: &str) -> Result<Vec<String>> {
        let spec = CommandSpec {
            program: "git".into(),
            args: vec![
                "-C".into(), repo_path.into(),
                "branch".into(), "--format=%(refname:short)".into(),
            ],
            cwd: None,
            env: HashMap::new(),
        };
        let output = self.runner.run(&spec)?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(stdout.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect())
    }
}
```

- [ ] **Step 4: Export git module**

Update `src/core/mod.rs`:

```rust
pub mod git;
pub mod name_gen;
```

- [ ] **Step 5: Run tests**

Run: `cargo test --test git_test`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add src/ tests/
git commit -m "feat: GitOps with worktree, merge, push, branch operations"
```

---

### Task 7: Hook Engine

**Files:**
- Create: `src/core/hook.rs`
- Create: `tests/hook_test.rs`

- [ ] **Step 1: Write failing test**

Create `tests/hook_test.rs`:

```rust
use zootree::core::hook::{HookEngine, HookContext};
use zootree::config::global::HookValue;
use zootree::runner::MockRunner;
use std::os::unix::process::ExitStatusExt;
use std::process::{ExitStatus, Output};

fn success_output() -> Output {
    Output {
        status: ExitStatus::from_raw(0),
        stdout: Vec::new(),
        stderr: Vec::new(),
    }
}

#[test]
fn test_simple_hook_command() {
    let runner = MockRunner::new();
    runner.push_response(success_output());
    let engine = HookEngine::new(&runner);

    let ctx = HookContext {
        workspace: "calm-river".into(),
        repo: Some("frontend".into()),
        branch: "zootree/calm-river".into(),
        target_branch: Some("develop".into()),
        worktree_path: Some("/home/user/ws/calm-river/frontend".into()),
        workspace_dir: "/home/user/ws/calm-river".into(),
    };

    let hook = HookValue::Simple("npm install".into());
    engine.execute(&hook, &ctx).unwrap();

    let calls = runner.take_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].program, "sh");
    assert_eq!(calls[0].args, vec!["-c", "npm install"]);
    assert_eq!(calls[0].env.get("ZOOTREE_WORKSPACE").unwrap(), "calm-river");
    assert_eq!(calls[0].env.get("ZOOTREE_REPO").unwrap(), "frontend");
    assert_eq!(calls[0].env.get("ZOOTREE_BRANCH").unwrap(), "zootree/calm-river");
    assert_eq!(calls[0].env.get("ZOOTREE_TARGET_BRANCH").unwrap(), "develop");
    assert_eq!(calls[0].env.get("ZOOTREE_WORKTREE_PATH").unwrap(), "/home/user/ws/calm-river/frontend");
    assert_eq!(calls[0].env.get("ZOOTREE_WORKSPACE_DIR").unwrap(), "/home/user/ws/calm-river");
}

#[test]
fn test_file_hook_command() {
    let runner = MockRunner::new();
    runner.push_response(success_output());
    let engine = HookEngine::new(&runner);

    let ctx = HookContext {
        workspace: "calm-river".into(),
        repo: None,
        branch: "zootree/calm-river".into(),
        target_branch: None,
        worktree_path: None,
        workspace_dir: "/home/user/ws/calm-river".into(),
    };

    let hook = HookValue::File { file: "/home/user/.config/zootree/hooks/cleanup.sh".into() };
    engine.execute(&hook, &ctx).unwrap();

    let calls = runner.take_calls();
    assert_eq!(calls[0].program, "sh");
    assert_eq!(calls[0].args, vec!["/home/user/.config/zootree/hooks/cleanup.sh"]);
}

#[test]
fn test_inline_hook_command() {
    let runner = MockRunner::new();
    runner.push_response(success_output());
    let engine = HookEngine::new(&runner);

    let ctx = HookContext {
        workspace: "calm-river".into(),
        repo: None,
        branch: "zootree/calm-river".into(),
        target_branch: None,
        worktree_path: None,
        workspace_dir: "/home/user/ws/calm-river".into(),
    };

    let script = "cd $ZOOTREE_WORKTREE_PATH\nnpm install\nnpm run db:migrate";
    let hook = HookValue::Inline { inline: script.into() };
    engine.execute(&hook, &ctx).unwrap();

    let calls = runner.take_calls();
    assert_eq!(calls[0].program, "sh");
    assert_eq!(calls[0].args, vec!["-c", script]);
}

#[test]
fn test_hook_failure_returns_error() {
    let runner = MockRunner::new();
    runner.push_response(Output {
        status: ExitStatus::from_raw(256), // exit code 1
        stdout: Vec::new(),
        stderr: b"command not found".to_vec(),
    });
    let engine = HookEngine::new(&runner);

    let ctx = HookContext {
        workspace: "test".into(),
        repo: None,
        branch: "zootree/test".into(),
        target_branch: None,
        worktree_path: None,
        workspace_dir: "/tmp/test".into(),
    };

    let hook = HookValue::Simple("bad-command".into());
    let result = engine.execute(&hook, &ctx);
    assert!(result.is_err());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test hook_test`
Expected: FAIL

- [ ] **Step 3: Implement HookEngine**

Create `src/core/hook.rs`:

```rust
use crate::config::global::HookValue;
use crate::runner::{CommandRunner, CommandSpec};
use anyhow::{Result, bail};
use std::collections::HashMap;

pub struct HookContext {
    pub workspace: String,
    pub repo: Option<String>,
    pub branch: String,
    pub target_branch: Option<String>,
    pub worktree_path: Option<String>,
    pub workspace_dir: String,
}

impl HookContext {
    fn env_vars(&self) -> HashMap<String, String> {
        let mut env = HashMap::new();
        env.insert("ZOOTREE_WORKSPACE".into(), self.workspace.clone());
        if let Some(repo) = &self.repo {
            env.insert("ZOOTREE_REPO".into(), repo.clone());
        }
        env.insert("ZOOTREE_BRANCH".into(), self.branch.clone());
        if let Some(tb) = &self.target_branch {
            env.insert("ZOOTREE_TARGET_BRANCH".into(), tb.clone());
        }
        if let Some(wp) = &self.worktree_path {
            env.insert("ZOOTREE_WORKTREE_PATH".into(), wp.clone());
        }
        env.insert("ZOOTREE_WORKSPACE_DIR".into(), self.workspace_dir.clone());
        env
    }
}

pub struct HookEngine<'a, R: CommandRunner> {
    runner: &'a R,
}

impl<'a, R: CommandRunner> HookEngine<'a, R> {
    pub fn new(runner: &'a R) -> Self {
        Self { runner }
    }

    pub fn execute(&self, hook: &HookValue, ctx: &HookContext) -> Result<()> {
        let (program, args) = match hook {
            HookValue::Simple(cmd) => ("sh".to_string(), vec!["-c".to_string(), cmd.clone()]),
            HookValue::File { file } => ("sh".to_string(), vec![file.clone()]),
            HookValue::Inline { inline } => ("sh".to_string(), vec!["-c".to_string(), inline.clone()]),
        };

        let cwd = ctx.worktree_path.clone().or_else(|| Some(ctx.workspace_dir.clone()));

        let spec = CommandSpec {
            program,
            args,
            cwd,
            env: ctx.env_vars(),
        };

        let output = self.runner.run(&spec)?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("hook failed: {}", stderr);
        }
        Ok(())
    }

    pub fn execute_if_set(&self, hook: &Option<HookValue>, ctx: &HookContext) -> Result<()> {
        if let Some(h) = hook {
            self.execute(h, ctx)?;
        }
        Ok(())
    }
}
```

- [ ] **Step 4: Export hook module**

Update `src/core/mod.rs`:

```rust
pub mod git;
pub mod hook;
pub mod name_gen;
```

- [ ] **Step 5: Run tests**

Run: `cargo test --test hook_test`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add src/ tests/
git commit -m "feat: HookEngine with simple/file/inline hook support"
```

---

### Task 8: Copy Files

**Files:**
- Create: `src/core/copy_files.rs`
- Create: `tests/copy_files_test.rs`

- [ ] **Step 1: Write failing test**

Create `tests/copy_files_test.rs`:

```rust
use zootree::core::copy_files::merge_copy_files;

#[test]
fn test_merge_copy_files_combines() {
    let global = vec![".env".to_string()];
    let repo = vec![".env.local".to_string(), ".vscode/settings.json".to_string()];
    let merged = merge_copy_files(&global, &repo);
    assert_eq!(merged, vec![".env", ".env.local", ".vscode/settings.json"]);
}

#[test]
fn test_merge_copy_files_dedup() {
    let global = vec![".env".to_string()];
    let repo = vec![".env".to_string(), ".env.local".to_string()];
    let merged = merge_copy_files(&global, &repo);
    assert_eq!(merged, vec![".env", ".env.local"]);
}

#[test]
fn test_merge_copy_files_empty() {
    let global: Vec<String> = vec![];
    let repo: Vec<String> = vec![];
    let merged = merge_copy_files(&global, &repo);
    assert!(merged.is_empty());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test copy_files_test`
Expected: FAIL

- [ ] **Step 3: Implement copy_files module**

Create `src/core/copy_files.rs`:

```rust
use anyhow::Result;
use glob::glob;
use std::path::Path;
use tracing::{info, warn};

pub fn merge_copy_files(global: &[String], repo: &[String]) -> Vec<String> {
    let mut merged = global.to_vec();
    for f in repo {
        if !merged.contains(f) {
            merged.push(f.clone());
        }
    }
    merged
}

pub fn copy_files_to_worktree(
    repo_path: &Path,
    worktree_path: &Path,
    patterns: &[String],
) -> Result<()> {
    for pattern in patterns {
        let full_pattern = repo_path.join(pattern);
        let pattern_str = full_pattern.to_string_lossy();

        let matches: Vec<_> = glob(&pattern_str)?.filter_map(|r| r.ok()).collect();

        if matches.is_empty() {
            let plain_path = repo_path.join(pattern);
            if plain_path.exists() {
                let relative = plain_path.strip_prefix(repo_path)?;
                let dest = worktree_path.join(relative);
                if let Some(parent) = dest.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::copy(&plain_path, &dest)?;
                info!("copied {} -> {}", plain_path.display(), dest.display());
            } else {
                warn!("copy_files pattern '{}' matched nothing", pattern);
            }
            continue;
        }

        for source in matches {
            let relative = source.strip_prefix(repo_path)?;
            let dest = worktree_path.join(relative);
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(&source, &dest)?;
            info!("copied {} -> {}", source.display(), dest.display());
        }
    }
    Ok(())
}
```

- [ ] **Step 4: Export module**

Update `src/core/mod.rs`:

```rust
pub mod copy_files;
pub mod git;
pub mod hook;
pub mod name_gen;
```

- [ ] **Step 5: Run tests**

Run: `cargo test --test copy_files_test`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add src/ tests/
git commit -m "feat: copy_files with glob merge and file copy"
```

---

### Task 9: Layout Renderer

**Files:**
- Create: `src/core/layout.rs`
- Create: `tests/layout_test.rs`

- [ ] **Step 1: Write failing test for variable replacement**

Create `tests/layout_test.rs`:

```rust
use zootree::core::layout::{LayoutRenderer, LayoutVar};

#[test]
fn test_variable_replacement() {
    let template = r#"tab name="$repo_name" {
    pane cwd="$worktree_path"
}"#;
    let vars = LayoutVar {
        repo_name: "frontend".into(),
        worktree_path: "/home/user/ws/calm-river/frontend".into(),
        branch: "zootree/calm-river".into(),
        workspace_name: "calm-river".into(),
        workspace_dir: "/home/user/ws/calm-river".into(),
        lazygit_config: "".into(),
    };
    let result = LayoutRenderer::replace_vars(template, &vars);
    assert!(result.contains(r#"name="frontend""#));
    assert!(result.contains(r#"cwd="/home/user/ws/calm-river/frontend""#));
}

#[test]
fn test_empty_lazygit_config_removes_ucf_arg() {
    let template = r#"pane command="lazygit" {
    args "-p" "$worktree_path" "-ucf" "$lazygit_config"
}"#;
    let vars = LayoutVar {
        repo_name: "frontend".into(),
        worktree_path: "/home/user/ws/frontend".into(),
        branch: "zootree/test".into(),
        workspace_name: "test".into(),
        workspace_dir: "/home/user/ws".into(),
        lazygit_config: "".into(),
    };
    let result = LayoutRenderer::replace_vars(template, &vars);
    assert!(!result.contains("-ucf"));
    assert!(!result.contains("$lazygit_config"));
}

#[test]
fn test_nonempty_lazygit_config_keeps_ucf_arg() {
    let template = r#"pane command="lazygit" {
    args "-p" "$worktree_path" "-ucf" "$lazygit_config"
}"#;
    let vars = LayoutVar {
        repo_name: "frontend".into(),
        worktree_path: "/home/user/ws/frontend".into(),
        branch: "zootree/test".into(),
        workspace_name: "test".into(),
        workspace_dir: "/home/user/ws".into(),
        lazygit_config: "/home/user/.lazygit.yml".into(),
    };
    let result = LayoutRenderer::replace_vars(template, &vars);
    assert!(result.contains(r#""-ucf" "/home/user/.lazygit.yml""#));
}

#[test]
fn test_repeat_per_repo() {
    let template = r#"layout {
    tab name="overview" {
        pane command="zootree"
    }

    // @repeat-per-repo
    tab name="$repo_name" {
        pane cwd="$worktree_path"
    }
}"#;
    let repos = vec![
        LayoutVar {
            repo_name: "frontend".into(),
            worktree_path: "/ws/frontend".into(),
            branch: "zootree/test".into(),
            workspace_name: "test".into(),
            workspace_dir: "/ws".into(),
            lazygit_config: "".into(),
        },
        LayoutVar {
            repo_name: "backend".into(),
            worktree_path: "/ws/backend".into(),
            branch: "zootree/test".into(),
            workspace_name: "test".into(),
            workspace_dir: "/ws".into(),
            lazygit_config: "".into(),
        },
    ];
    let result = LayoutRenderer::render(template, &repos);
    assert!(result.contains(r#"name="overview""#));
    assert!(result.contains(r#"name="frontend""#));
    assert!(result.contains(r#"name="backend""#));
    assert!(result.contains(r#"cwd="/ws/frontend""#));
    assert!(result.contains(r#"cwd="/ws/backend""#));
    assert!(!result.contains("@repeat-per-repo"));
    assert!(!result.contains("$repo_name"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test layout_test`
Expected: FAIL

- [ ] **Step 3: Implement LayoutRenderer**

Create `src/core/layout.rs`:

```rust
pub struct LayoutVar {
    pub repo_name: String,
    pub worktree_path: String,
    pub branch: String,
    pub workspace_name: String,
    pub workspace_dir: String,
    pub lazygit_config: String,
}

pub struct LayoutRenderer;

impl LayoutRenderer {
    pub fn replace_vars(template: &str, vars: &LayoutVar) -> String {
        let mut result = template.to_string();

        // Handle empty lazygit_config: remove lines containing "-ucf" "$lazygit_config"
        if vars.lazygit_config.is_empty() {
            // Remove "-ucf" "$lazygit_config" pair from args
            result = result.replace(r#" "-ucf" "$lazygit_config""#, "");
        }

        result = result.replace("$repo_name", &vars.repo_name);
        result = result.replace("$worktree_path", &vars.worktree_path);
        result = result.replace("$branch", &vars.branch);
        result = result.replace("$workspace_name", &vars.workspace_name);
        result = result.replace("$workspace_dir", &vars.workspace_dir);
        result = result.replace("$lazygit_config", &vars.lazygit_config);
        result
    }

    pub fn render(template: &str, repos: &[LayoutVar]) -> String {
        let marker = "// @repeat-per-repo";
        let Some(marker_pos) = template.find(marker) else {
            // No repeat block, just replace vars with first repo if available
            if let Some(vars) = repos.first() {
                return Self::replace_vars(template, vars);
            }
            return template.to_string();
        };

        // Split into before-marker and after-marker
        let before_marker = &template[..marker_pos];

        // Find the tab block after the marker
        let after_marker = &template[marker_pos + marker.len()..];
        let after_marker = after_marker.trim_start_matches('\n');

        // Find the tab block: from "tab" to its closing "}"
        // We need to track brace depth
        let tab_block = Self::extract_tab_block(after_marker);
        let after_tab = &after_marker[tab_block.len()..];

        // Generate one tab block per repo
        let mut expanded = String::new();
        for (i, vars) in repos.iter().enumerate() {
            if i > 0 {
                expanded.push('\n');
            }
            expanded.push_str(&Self::replace_vars(&tab_block, vars));
        }

        format!("{}{}{}", before_marker.trim_end_matches('\n').to_string() + "\n\n", expanded, after_tab)
    }

    fn extract_tab_block(s: &str) -> &str {
        let mut depth = 0;
        let mut started = false;
        let mut end = 0;

        for (i, ch) in s.char_indices() {
            match ch {
                '{' => {
                    depth += 1;
                    started = true;
                }
                '}' => {
                    depth -= 1;
                    if started && depth == 0 {
                        end = i + 1;
                        break;
                    }
                }
                _ => {}
            }
        }

        &s[..end]
    }

    pub fn default_layout() -> &'static str {
        r#"layout {
    tab name="overview" {
        pane command="zootree" {
            args "list" "--status" "in_progress"
        }
    }

    // @repeat-per-repo
    tab name="$repo_name" {
        pane split_direction="vertical" {
            pane size="60%" command="lazygit" {
                args "-p" "$worktree_path" "-ucf" "$lazygit_config"
            }
            pane size="12%" cwd="$worktree_path"
            pane size="28%" cwd="$worktree_path"
        }
    }
}"#
    }
}
```

- [ ] **Step 4: Export module**

Update `src/core/mod.rs`:

```rust
pub mod copy_files;
pub mod git;
pub mod hook;
pub mod layout;
pub mod name_gen;
```

- [ ] **Step 5: Run tests**

Run: `cargo test --test layout_test`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add src/ tests/
git commit -m "feat: KDL layout renderer with repeat-per-repo and var replacement"
```

---

### Task 10: Zellij Operations

**Files:**
- Create: `src/core/zellij.rs`

- [ ] **Step 1: Implement ZellijOps**

Create `src/core/zellij.rs`:

```rust
use crate::runner::{CommandRunner, CommandSpec};
use anyhow::Result;
use std::collections::HashMap;
use std::path::Path;
use tracing::info;

pub struct ZellijOps<'a, R: CommandRunner> {
    runner: &'a R,
}

impl<'a, R: CommandRunner> ZellijOps<'a, R> {
    pub fn new(runner: &'a R) -> Self {
        Self { runner }
    }

    fn zellij(&self, args: Vec<String>) -> Result<std::process::Output> {
        let spec = CommandSpec {
            program: "zellij".into(),
            args,
            cwd: None,
            env: HashMap::new(),
        };
        self.runner.run(&spec)
    }

    pub fn start_session(&self, session_name: &str, layout_path: &Path) -> Result<()> {
        info!("starting zellij session: {}", session_name);
        self.zellij(vec![
            "--session".into(), session_name.into(),
            "--layout".into(), layout_path.to_string_lossy().into(),
        ])?;
        Ok(())
    }

    pub fn attach_session(&self, session_name: &str) -> Result<()> {
        info!("attaching to zellij session: {}", session_name);
        self.zellij(vec!["attach".into(), session_name.into()])?;
        Ok(())
    }

    pub fn kill_session(&self, session_name: &str) -> Result<()> {
        info!("killing zellij session: {}", session_name);
        self.zellij(vec!["kill-session".into(), session_name.into()])?;
        Ok(())
    }

    pub fn session_exists(&self, session_name: &str) -> Result<bool> {
        let output = self.zellij(vec!["list-sessions".into()])?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(stdout.lines().any(|line| line.trim().starts_with(session_name)))
    }

    pub fn add_tab(&self, session_name: &str, layout_path: &Path, tab_name: &str) -> Result<()> {
        info!("adding tab '{}' to session '{}'", tab_name, session_name);
        self.zellij(vec![
            "--session".into(), session_name.into(),
            "action".into(), "new-tab".into(),
            "--layout".into(), layout_path.to_string_lossy().into(),
            "--name".into(), tab_name.into(),
        ])?;
        Ok(())
    }
}
```

- [ ] **Step 2: Export module**

Update `src/core/mod.rs`:

```rust
pub mod copy_files;
pub mod git;
pub mod hook;
pub mod layout;
pub mod name_gen;
pub mod zellij;
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build`
Expected: compiles

- [ ] **Step 4: Commit**

```bash
git add src/
git commit -m "feat: ZellijOps for session and tab management"
```

---

### Task 11: CLI Definition

**Files:**
- Create: `src/cli/mod.rs`
- Create: `src/cli/repo.rs`
- Create: `src/cli/workspace.rs`
- Create: `src/cli/template.rs`
- Create: `src/cli/prune.rs`
- Modify: `src/main.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Define top-level CLI with clap**

Create `src/cli/mod.rs`:

```rust
pub mod repo;
pub mod workspace;
pub mod template;
pub mod prune;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "zootree", about = "Multi-repo collaborative workspace manager")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    /// Enable verbose output
    #[arg(long, global = true)]
    pub verbose: bool,

    /// Suppress non-error output
    #[arg(long, global = true)]
    pub quiet: bool,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Manage registered repositories
    Repo(repo::RepoArgs),
    /// Create a new workspace
    Create(workspace::CreateArgs),
    /// List workspaces
    List(workspace::ListArgs),
    /// Start a workspace (create worktrees + optional zellij)
    Start(workspace::StartArgs),
    /// Open a workspace in zellij
    Open(workspace::OpenArgs),
    /// Complete a workspace (merge + clean)
    Done(workspace::DoneArgs),
    /// Cancel a workspace (clean)
    Cancel(workspace::CancelArgs),
    /// Manage templates
    Template(template::TemplateArgs),
    /// Clean up archived workspaces
    Prune(prune::PruneArgs),
    /// View logs
    Logs,
}
```

- [ ] **Step 2: Define repo subcommands**

Create `src/cli/repo.rs`:

```rust
use clap::{Args, Subcommand};

#[derive(Args)]
pub struct RepoArgs {
    #[command(subcommand)]
    pub command: RepoCommands,
}

#[derive(Subcommand)]
pub enum RepoCommands {
    /// Register a new repository
    Add {
        /// Repository name
        name: String,
        /// Path to the repository
        #[arg(long)]
        path: String,
        /// Default target branch
        #[arg(long)]
        default_target_branch: Option<String>,
    },
    /// List registered repositories
    List,
    /// Edit a repository config ($EDITOR)
    Edit {
        /// Repository name (interactive if omitted)
        name: Option<String>,
    },
    /// Remove a registered repository
    Remove {
        /// Repository name (interactive if omitted)
        name: Option<String>,
    },
}
```

- [ ] **Step 3: Define workspace subcommands**

Create `src/cli/workspace.rs`:

```rust
use clap::Args;

#[derive(Args)]
pub struct CreateArgs {
    /// Workspace title (required)
    #[arg(long)]
    pub title: Option<String>,

    /// Workspace name (auto-generated if omitted)
    #[arg(long)]
    pub name: Option<String>,

    /// Description
    #[arg(long)]
    pub description: Option<String>,

    /// Repos with optional target branch: repo1:branch1,repo2:branch2
    #[arg(long)]
    pub repos: Option<String>,

    /// Override branch name
    #[arg(long)]
    pub branch: Option<String>,

    /// Use a template
    #[arg(long)]
    pub template: Option<String>,
}

#[derive(Args)]
pub struct ListArgs {
    /// Filter by status: pending, in_progress, done, canceled
    #[arg(long)]
    pub status: Option<String>,
}

#[derive(Args)]
pub struct StartArgs {
    /// Workspace name (interactive if omitted)
    pub name: Option<String>,

    /// Skip launching zellij
    #[arg(long)]
    pub no_zellij: bool,
}

#[derive(Args)]
pub struct OpenArgs {
    /// Workspace name (interactive if omitted)
    pub name: Option<String>,
}

#[derive(Args)]
pub struct DoneArgs {
    /// Workspace name (interactive if omitted)
    pub name: Option<String>,

    /// Skip merge
    #[arg(long)]
    pub no_merge: bool,

    /// Skip worktree cleanup
    #[arg(long)]
    pub no_clean: bool,

    /// Push target branch after merge
    #[arg(long)]
    pub push: bool,

    /// Delete remote feature branch
    #[arg(long)]
    pub delete_remote: bool,

    /// Merge strategy: merge (default), squash, rebase
    #[arg(long)]
    pub strategy: Option<String>,

    /// Force (skip hook failures)
    #[arg(long)]
    pub force: bool,

    /// Preview actions without executing
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Args)]
pub struct CancelArgs {
    /// Workspace name (interactive if omitted)
    pub name: Option<String>,

    /// Skip worktree cleanup
    #[arg(long)]
    pub no_clean: bool,

    /// Force (skip confirmations and hook failures)
    #[arg(long)]
    pub force: bool,
}
```

- [ ] **Step 4: Define template and prune subcommands**

Create `src/cli/template.rs`:

```rust
use clap::{Args, Subcommand};

#[derive(Args)]
pub struct TemplateArgs {
    #[command(subcommand)]
    pub command: TemplateCommands,
}

#[derive(Subcommand)]
pub enum TemplateCommands {
    /// List available templates
    List,
    /// Save a workspace config as a template
    Save {
        /// Template name
        name: String,
        /// Source workspace name
        #[arg(long)]
        from: String,
    },
}
```

Create `src/cli/prune.rs`:

```rust
use clap::Args;

#[derive(Args)]
pub struct PruneArgs {
    /// Prune all archived workspaces without prompting
    #[arg(long)]
    pub all: bool,
}
```

- [ ] **Step 5: Update main.rs and lib.rs**

Update `src/lib.rs`:

```rust
pub mod cli;
pub mod config;
pub mod core;
pub mod runner;
pub mod tui;
```

Update `src/main.rs`:

```rust
use anyhow::Result;
use clap::Parser;
use zootree::cli::{Cli, Commands};

fn main() -> Result<()> {
    let cli = Cli::parse();

    // TODO: init tracing based on cli.verbose / cli.quiet

    match cli.command {
        Commands::Repo(args) => {
            println!("repo command: {:?}", args.command);
        }
        Commands::Create(args) => {
            println!("create workspace");
        }
        Commands::List(args) => {
            println!("list workspaces");
        }
        Commands::Start(args) => {
            println!("start workspace");
        }
        Commands::Open(args) => {
            println!("open workspace");
        }
        Commands::Done(args) => {
            println!("done workspace");
        }
        Commands::Cancel(args) => {
            println!("cancel workspace");
        }
        Commands::Template(args) => {
            println!("template command");
        }
        Commands::Prune(args) => {
            println!("prune");
        }
        Commands::Logs => {
            println!("logs");
        }
    }

    Ok(())
}
```

- [ ] **Step 6: Create stub tui.rs**

Create `src/tui.rs`:

```rust
use anyhow::Result;
use dialoguer::{Input, Select, MultiSelect, Confirm};

pub fn input_required(prompt: &str) -> Result<String> {
    let value: String = Input::new().with_prompt(prompt).interact_text()?;
    Ok(value)
}

pub fn input_optional(prompt: &str) -> Result<Option<String>> {
    let value: String = Input::new()
        .with_prompt(prompt)
        .allow_empty(true)
        .interact_text()?;
    if value.is_empty() { Ok(None) } else { Ok(Some(value)) }
}

pub fn select_one(prompt: &str, items: &[String]) -> Result<usize> {
    let selection = Select::new()
        .with_prompt(prompt)
        .items(items)
        .interact()?;
    Ok(selection)
}

pub fn select_multi(prompt: &str, items: &[String]) -> Result<Vec<usize>> {
    let selections = MultiSelect::new()
        .with_prompt(prompt)
        .items(items)
        .interact()?;
    Ok(selections)
}

pub fn confirm(prompt: &str, default: bool) -> Result<bool> {
    let result = Confirm::new()
        .with_prompt(prompt)
        .default(default)
        .interact()?;
    Ok(result)
}
```

- [ ] **Step 7: Verify it compiles and help works**

Run: `cargo build && cargo run -- --help`
Expected: compiles and shows help with all subcommands

- [ ] **Step 8: Commit**

```bash
git add src/
git commit -m "feat: CLI definition with clap derive for all commands"
```

---

### Task 12: Repo Commands Implementation

**Files:**
- Modify: `src/main.rs`
- Modify: `src/cli/repo.rs`

- [ ] **Step 1: Implement repo add**

Add to `src/cli/repo.rs`:

```rust
use crate::config::ConfigManager;
use crate::config::repo::RepoConfig;
use crate::config::global::HooksConfig;
use crate::tui;
use anyhow::Result;

pub fn handle_repo_command(cmd: &RepoCommands) -> Result<()> {
    let config_mgr = ConfigManager::new()?;
    config_mgr.ensure_dirs()?;

    match cmd {
        RepoCommands::Add { name, path, default_target_branch } => {
            let expanded = shellexpand::tilde(path).into_owned();
            let abs_path = std::fs::canonicalize(&expanded)
                .unwrap_or_else(|_| std::path::PathBuf::from(&expanded));

            if !abs_path.join(".git").exists() && !abs_path.exists() {
                anyhow::bail!("'{}' is not a valid git repository", path);
            }

            let repo_config = RepoConfig {
                path: abs_path.to_string_lossy().into_owned(),
                default_target_branch: default_target_branch.clone(),
                copy_files: Vec::new(),
                hooks: HooksConfig::default(),
                lazygit: None,
                layout: None,
            };
            config_mgr.save_repo_config(name, &repo_config)?;
            println!("repo '{}' registered at {}", name, abs_path.display());
            Ok(())
        }
        RepoCommands::List => {
            let repos = config_mgr.list_repos()?;
            if repos.is_empty() {
                println!("no repos registered");
            } else {
                for name in &repos {
                    let config = config_mgr.load_repo_config(name)?;
                    println!("  {} -> {}", name, config.path);
                }
            }
            Ok(())
        }
        RepoCommands::Edit { name } => {
            let name = match name {
                Some(n) => n.clone(),
                None => {
                    let repos = config_mgr.list_repos()?;
                    if repos.is_empty() {
                        anyhow::bail!("no repos registered");
                    }
                    let idx = tui::select_one("Select repo to edit", &repos)?;
                    repos[idx].clone()
                }
            };
            let path = config_mgr.repos_dir().join(format!("{}.toml", name));
            let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".into());
            std::process::Command::new(&editor).arg(&path).status()?;
            Ok(())
        }
        RepoCommands::Remove { name } => {
            let name = match name {
                Some(n) => n.clone(),
                None => {
                    let repos = config_mgr.list_repos()?;
                    if repos.is_empty() {
                        anyhow::bail!("no repos registered");
                    }
                    let idx = tui::select_one("Select repo to remove", &repos)?;
                    repos[idx].clone()
                }
            };
            config_mgr.remove_repo_config(&name)?;
            println!("repo '{}' removed", name);
            Ok(())
        }
    }
}
```

- [ ] **Step 2: Add shellexpand dependency**

Add to `Cargo.toml`:

```toml
shellexpand = "3"
```

- [ ] **Step 3: Wire up in main.rs**

Update the `Commands::Repo` match arm in `src/main.rs`:

```rust
Commands::Repo(args) => {
    crate::cli::repo::handle_repo_command(&args.command)?;
}
```

- [ ] **Step 4: Verify it works**

Run: `cargo run -- repo list`
Expected: "no repos registered"

Run: `cargo run -- repo add test-repo --path /tmp`
Expected: "repo 'test-repo' registered at /tmp"

- [ ] **Step 5: Commit**

```bash
git add src/ Cargo.toml Cargo.lock
git commit -m "feat: repo add/list/edit/remove commands"
```

---

### Task 13: Workspace Create Command

**Files:**
- Modify: `src/cli/workspace.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Implement --repos parsing**

Add to `src/cli/workspace.rs`:

```rust
use crate::config::workspace::RepoEntry;
use anyhow::Result;

pub fn parse_repos_arg(repos_str: &str) -> Vec<(String, Option<String>)> {
    repos_str.split(',')
        .map(|s| {
            let s = s.trim();
            if let Some((name, branch)) = s.split_once(':') {
                (name.to_string(), Some(branch.to_string()))
            } else {
                (s.to_string(), None)
            }
        })
        .collect()
}
```

- [ ] **Step 2: Write test for repos parsing**

Add to `tests/config_test.rs`:

```rust
use zootree::cli::workspace::parse_repos_arg;

#[test]
fn test_parse_repos_arg() {
    let result = parse_repos_arg("frontend:develop,backend,shared-lib:main");
    assert_eq!(result, vec![
        ("frontend".into(), Some("develop".into())),
        ("backend".into(), None),
        ("shared-lib".into(), Some("main".into())),
    ]);
}

#[test]
fn test_parse_repos_arg_single() {
    let result = parse_repos_arg("frontend:develop");
    assert_eq!(result, vec![("frontend".into(), Some("develop".into()))]);
}
```

- [ ] **Step 3: Run test**

Run: `cargo test test_parse_repos_arg`
Expected: PASS

- [ ] **Step 4: Implement create handler**

Add to `src/cli/workspace.rs`:

```rust
use crate::config::ConfigManager;
use crate::config::workspace::{WorkspaceConfig, WorkspaceStatus, RepoEntry, Event};
use crate::config::template::TemplateConfig;
use crate::core::name_gen::NameGenerator;
use crate::tui;
use chrono::Local;

pub fn handle_create(args: &CreateArgs) -> Result<()> {
    let config_mgr = ConfigManager::new()?;
    config_mgr.ensure_dirs()?;
    let global = config_mgr.load_global_config()?;

    // Title (required)
    let title = match &args.title {
        Some(t) => t.clone(),
        None => tui::input_required("Title")?,
    };

    // Description (optional)
    let description = match &args.description {
        Some(d) => d.clone(),
        None => tui::input_optional("Description (optional)")?.unwrap_or_default(),
    };

    // Repos selection
    let repo_entries = if let Some(repos_str) = &args.repos {
        let parsed = parse_repos_arg(repos_str);
        let mut entries = Vec::new();
        for (name, branch) in parsed {
            let repo_config = config_mgr.load_repo_config(&name)?;
            let target_branch = branch
                .or(repo_config.default_target_branch.clone())
                .ok_or_else(|| anyhow::anyhow!("target branch required for repo '{}'", name))?;
            entries.push(RepoEntry { name, target_branch });
        }
        entries
    } else {
        // Load from template if specified
        let template_repos = if let Some(tmpl_name) = &args.template {
            let tmpl = config_mgr.load_template(tmpl_name)?;
            Some(tmpl.repos)
        } else {
            None
        };

        let all_repos = config_mgr.list_repos()?;
        if all_repos.is_empty() {
            anyhow::bail!("no repos registered. Use 'zootree repo add' first.");
        }

        let defaults: Vec<bool> = all_repos.iter().map(|r| {
            template_repos.as_ref().map_or(false, |tr| tr.contains(r))
        }).collect();

        let selected = tui::select_multi("Select repos", &all_repos)?;
        if selected.is_empty() {
            anyhow::bail!("at least one repo must be selected");
        }

        let mut entries = Vec::new();
        for idx in selected {
            let name = &all_repos[idx];
            let repo_config = config_mgr.load_repo_config(name)?;

            let target_branch = if let Some(default) = &repo_config.default_target_branch {
                default.clone()
            } else {
                tui::input_required(&format!("Target branch for {}", name))?
            };

            entries.push(RepoEntry {
                name: name.clone(),
                target_branch,
            });
        }
        entries
    };

    // Name
    let name_gen = NameGenerator::new();
    let existing: Vec<String> = config_mgr.list_workspaces(None)?
        .iter().map(|w| w.name.clone()).collect();
    let name = match &args.name {
        Some(n) => n.clone(),
        None => name_gen.generate_avoiding(&existing),
    };

    // Branch
    let branch = match &args.branch {
        Some(b) => b.clone(),
        None => format!("{}/{}", global.branch_prefix, name),
    };

    let workspace_dir = format!(
        "{}/{}",
        shellexpand::tilde(&global.workspace_root),
        name
    );

    let now = Local::now().to_rfc3339();

    let workspace = WorkspaceConfig {
        title,
        name: name.clone(),
        description,
        branch,
        workspace_dir,
        created_at: now.clone(),
        layout: None,
        session_mode: "standalone".into(),
        session_name: None,
        repos: repo_entries,
        events: vec![Event {
            action: "created".into(),
            timestamp: now,
            detail: None,
        }],
    };

    config_mgr.save_workspace(&WorkspaceStatus::Pending, &workspace)?;

    // Save as recently template
    let recently = TemplateConfig {
        repos: workspace.repos.iter().map(|r| r.name.clone()).collect(),
        layout: workspace.layout.clone(),
        session_mode: Some(workspace.session_mode.clone()),
    };
    config_mgr.save_template("recently", &recently)?;

    println!("workspace '{}' created (pending)", name);
    println!("  branch: {}", workspace.branch);
    println!("  repos: {}", workspace.repos.iter().map(|r| format!("{}:{}", r.name, r.target_branch)).collect::<Vec<_>>().join(", "));

    Ok(())
}
```

- [ ] **Step 5: Wire up in main.rs**

```rust
Commands::Create(args) => {
    zootree::cli::workspace::handle_create(&args)?;
}
```

- [ ] **Step 6: Verify it compiles**

Run: `cargo build`
Expected: compiles

- [ ] **Step 7: Commit**

```bash
git add src/ tests/
git commit -m "feat: workspace create command with interactive and CLI modes"
```

---

### Task 14: Workspace Start Command

**Files:**
- Modify: `src/cli/workspace.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Implement start handler**

Add to `src/cli/workspace.rs`:

```rust
use crate::core::git::GitOps;
use crate::core::hook::{HookEngine, HookContext};
use crate::core::copy_files;
use crate::core::layout::LayoutRenderer;
use crate::core::zellij::ZellijOps;
use crate::runner::RealRunner;
use std::path::Path;

pub fn handle_start(args: &StartArgs) -> Result<()> {
    let config_mgr = ConfigManager::new()?;
    let global = config_mgr.load_global_config()?;
    let runner = RealRunner;
    let git = GitOps::new(&runner);
    let hook_engine = HookEngine::new(&runner);

    // Select workspace
    let name = match &args.name {
        Some(n) => n.clone(),
        None => {
            let pending = config_mgr.list_workspaces(Some(&WorkspaceStatus::Pending))?;
            if pending.is_empty() {
                anyhow::bail!("no pending workspaces");
            }
            let names: Vec<String> = pending.iter().map(|w| format!("{} - {}", w.name, w.title)).collect();
            let idx = tui::select_one("Select workspace to start", &names)?;
            pending[idx].name.clone()
        }
    };

    let (status, mut workspace) = config_mgr.load_workspace(&name)?;
    if !matches!(status, WorkspaceStatus::Pending) {
        anyhow::bail!("workspace '{}' is not in pending state", name);
    }

    // Create workspace directory
    let ws_dir = shellexpand::tilde(&workspace.workspace_dir).into_owned();
    std::fs::create_dir_all(&ws_dir)?;

    // Create worktrees for each repo
    for repo_entry in &workspace.repos {
        let repo_config = config_mgr.load_repo_config(&repo_entry.name)?;
        let repo_path = shellexpand::tilde(&repo_config.path).into_owned();
        let worktree_path = format!("{}/{}", ws_dir, repo_entry.name);

        tracing::info!("creating worktree for {} at {}", repo_entry.name, worktree_path);
        git.worktree_add(&repo_path, &workspace.branch, &worktree_path, &repo_entry.target_branch)?;

        // Copy files
        let patterns = copy_files::merge_copy_files(&global.copy_files, &repo_config.copy_files);
        if !patterns.is_empty() {
            copy_files::copy_files_to_worktree(
                Path::new(&repo_path),
                Path::new(&worktree_path),
                &patterns,
            )?;
        }

        // Execute post_create hook (repo level overrides global)
        let hook = repo_config.hooks.post_create.as_ref()
            .or(global.hooks.post_create.as_ref());
        if let Some(h) = hook {
            let ctx = HookContext {
                workspace: workspace.name.clone(),
                repo: Some(repo_entry.name.clone()),
                branch: workspace.branch.clone(),
                target_branch: Some(repo_entry.target_branch.clone()),
                worktree_path: Some(worktree_path.clone()),
                workspace_dir: ws_dir.clone(),
            };
            hook_engine.execute(h, &ctx)?;
        }
    }

    // Move to in_progress
    let now = Local::now().to_rfc3339();
    workspace.events.push(Event {
        action: "started".into(),
        timestamp: now,
        detail: None,
    });
    config_mgr.save_workspace(&WorkspaceStatus::Pending, &workspace)?;
    config_mgr.move_workspace(&name, &WorkspaceStatus::Pending, &WorkspaceStatus::InProgress)?;

    // Execute post_start hook
    if let Some(h) = &global.hooks.post_start {
        let ctx = HookContext {
            workspace: workspace.name.clone(),
            repo: None,
            branch: workspace.branch.clone(),
            target_branch: None,
            worktree_path: None,
            workspace_dir: ws_dir.clone(),
        };
        hook_engine.execute(h, &ctx)?;
    }

    println!("workspace '{}' started", name);

    // Launch zellij unless --no-zellij
    if !args.no_zellij {
        launch_zellij(&config_mgr, &global, &workspace, &runner)?;
    }

    Ok(())
}

fn launch_zellij(
    config_mgr: &ConfigManager,
    global: &crate::config::global::GlobalConfig,
    workspace: &WorkspaceConfig,
    runner: &RealRunner,
) -> Result<()> {
    let zellij = ZellijOps::new(runner);

    // Determine layout template
    let layout_name = workspace.layout.as_deref()
        .unwrap_or(&global.default_layout);

    let template_content = {
        let layout_path = config_mgr.base_dir.join("layouts").join(format!("{}.kdl", layout_name));
        if layout_path.exists() {
            std::fs::read_to_string(&layout_path)?
        } else {
            LayoutRenderer::default_layout().to_string()
        }
    };

    // Build LayoutVars for each repo
    let ws_dir = shellexpand::tilde(&workspace.workspace_dir).into_owned();
    let mut vars = Vec::new();
    for repo_entry in &workspace.repos {
        let repo_config = config_mgr.load_repo_config(&repo_entry.name)?;
        let lazygit_config = repo_config.lazygit
            .map(|lg| lg.config)
            .unwrap_or_default();

        vars.push(crate::core::layout::LayoutVar {
            repo_name: repo_entry.name.clone(),
            worktree_path: format!("{}/{}", ws_dir, repo_entry.name),
            branch: workspace.branch.clone(),
            workspace_name: workspace.name.clone(),
            workspace_dir: ws_dir.clone(),
            lazygit_config,
        });
    }

    let rendered = LayoutRenderer::render(&template_content, &vars);

    // Write to temp file
    let tmp_dir = std::env::temp_dir().join("zootree");
    std::fs::create_dir_all(&tmp_dir)?;
    let layout_file = tmp_dir.join(format!("{}.kdl", workspace.name));
    std::fs::write(&layout_file, &rendered)?;

    let session_name = match &workspace.session_mode as &str {
        "shared" => workspace.session_name.clone()
            .ok_or_else(|| anyhow::anyhow!("shared mode requires session_name"))?,
        _ => format!("zootree-{}", workspace.name),
    };

    if zellij.session_exists(&session_name)? {
        zellij.attach_session(&session_name)?;
    } else {
        zellij.start_session(&session_name, &layout_file)?;
    }

    Ok(())
}
```

- [ ] **Step 2: Wire up in main.rs**

```rust
Commands::Start(args) => {
    zootree::cli::workspace::handle_start(&args)?;
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build`
Expected: compiles

- [ ] **Step 4: Commit**

```bash
git add src/
git commit -m "feat: workspace start command with worktree creation and zellij launch"
```

---

### Task 15: Workspace List + Open Commands

**Files:**
- Modify: `src/cli/workspace.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Implement list handler**

Add to `src/cli/workspace.rs`:

```rust
pub fn handle_list(args: &ListArgs) -> Result<()> {
    let config_mgr = ConfigManager::new()?;

    let status_filter = args.status.as_deref().map(|s| match s {
        "pending" => WorkspaceStatus::Pending,
        "in_progress" => WorkspaceStatus::InProgress,
        "done" => WorkspaceStatus::Done,
        "canceled" => WorkspaceStatus::Canceled,
        _ => WorkspaceStatus::Pending, // fallback
    });

    let workspaces = config_mgr.list_workspaces(status_filter.as_ref())?;

    if workspaces.is_empty() {
        println!("no workspaces found");
        return Ok(());
    }

    for ws in &workspaces {
        let repos_str = ws.repos.iter()
            .map(|r| format!("{}:{}", r.name, r.target_branch))
            .collect::<Vec<_>>()
            .join(", ");
        println!("  {} - {} [{}]", ws.name, ws.title, repos_str);
    }

    Ok(())
}
```

- [ ] **Step 2: Implement open handler**

```rust
pub fn handle_open(args: &OpenArgs) -> Result<()> {
    let config_mgr = ConfigManager::new()?;
    let global = config_mgr.load_global_config()?;
    let runner = RealRunner;

    let name = match &args.name {
        Some(n) => n.clone(),
        None => {
            let in_progress = config_mgr.list_workspaces(Some(&WorkspaceStatus::InProgress))?;
            if in_progress.is_empty() {
                anyhow::bail!("no in_progress workspaces");
            }
            let names: Vec<String> = in_progress.iter().map(|w| format!("{} - {}", w.name, w.title)).collect();
            let idx = tui::select_one("Select workspace to open", &names)?;
            in_progress[idx].name.clone()
        }
    };

    let (status, workspace) = config_mgr.load_workspace(&name)?;
    if !matches!(status, WorkspaceStatus::InProgress) {
        anyhow::bail!("workspace '{}' is not in_progress", name);
    }

    launch_zellij(&config_mgr, &global, &workspace, &runner)?;
    Ok(())
}
```

- [ ] **Step 3: Wire up in main.rs**

```rust
Commands::List(args) => {
    zootree::cli::workspace::handle_list(&args)?;
}
Commands::Open(args) => {
    zootree::cli::workspace::handle_open(&args)?;
}
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo build`
Expected: compiles

- [ ] **Step 5: Commit**

```bash
git add src/
git commit -m "feat: workspace list and open commands"
```

---

### Task 16: Workspace Done + Cancel Commands

**Files:**
- Modify: `src/cli/workspace.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Implement done handler**

Add to `src/cli/workspace.rs`:

```rust
pub fn handle_done(args: &DoneArgs) -> Result<()> {
    let config_mgr = ConfigManager::new()?;
    let global = config_mgr.load_global_config()?;
    let runner = RealRunner;
    let git = GitOps::new(&runner);
    let hook_engine = HookEngine::new(&runner);
    let zellij = ZellijOps::new(&runner);

    let name = match &args.name {
        Some(n) => n.clone(),
        None => {
            let in_progress = config_mgr.list_workspaces(Some(&WorkspaceStatus::InProgress))?;
            if in_progress.is_empty() {
                anyhow::bail!("no in_progress workspaces");
            }
            let names: Vec<String> = in_progress.iter().map(|w| format!("{} - {}", w.name, w.title)).collect();
            let idx = tui::select_one("Select workspace to complete", &names)?;
            in_progress[idx].name.clone()
        }
    };

    let (status, mut workspace) = config_mgr.load_workspace(&name)?;
    if !matches!(status, WorkspaceStatus::InProgress) {
        anyhow::bail!("workspace '{}' is not in_progress", name);
    }

    let ws_dir = shellexpand::tilde(&workspace.workspace_dir).into_owned();

    if args.dry_run {
        println!("dry run for workspace '{}':", name);
        if !args.no_merge {
            for repo_entry in &workspace.repos {
                println!("  merge {} -> {}", workspace.branch, repo_entry.target_branch);
            }
        }
        if !args.no_clean {
            println!("  clean worktrees and workspace directory");
        }
        return Ok(());
    }

    // pre_done hook
    if !args.force {
        hook_engine.execute_if_set(&global.hooks.pre_done, &HookContext {
            workspace: workspace.name.clone(),
            repo: None,
            branch: workspace.branch.clone(),
            target_branch: None,
            worktree_path: None,
            workspace_dir: ws_dir.clone(),
        })?;
    }

    for repo_entry in &workspace.repos {
        let repo_config = config_mgr.load_repo_config(&repo_entry.name)?;
        let repo_path = shellexpand::tilde(&repo_config.path).into_owned();
        let worktree_path = format!("{}/{}", ws_dir, repo_entry.name);

        // Check uncommitted changes
        if git.has_uncommitted_changes(&worktree_path)? {
            if !args.force {
                anyhow::bail!(
                    "repo '{}' has uncommitted changes in {}. Commit or stash first, or use --force",
                    repo_entry.name, worktree_path
                );
            }
        }

        // Merge
        if !args.no_merge {
            let strategy = args.strategy.as_deref();
            git.merge(&repo_path, &workspace.branch, &repo_entry.target_branch, strategy)?;
            println!("  merged {} -> {} ({})", workspace.branch, repo_entry.target_branch, repo_entry.name);

            if args.push {
                git.push(&repo_path, &repo_entry.target_branch)?;
                println!("  pushed {} ({})", repo_entry.target_branch, repo_entry.name);
            }

            if args.delete_remote {
                git.delete_remote_branch(&repo_path, &workspace.branch)?;
                println!("  deleted remote branch {} ({})", workspace.branch, repo_entry.name);
            }
        }

        // Clean
        if !args.no_clean {
            // pre_remove hook
            let hook = repo_config.hooks.pre_remove.as_ref()
                .or(global.hooks.pre_remove.as_ref());
            if let Some(h) = hook {
                if !args.force {
                    hook_engine.execute(h, &HookContext {
                        workspace: workspace.name.clone(),
                        repo: Some(repo_entry.name.clone()),
                        branch: workspace.branch.clone(),
                        target_branch: Some(repo_entry.target_branch.clone()),
                        worktree_path: Some(worktree_path.clone()),
                        workspace_dir: ws_dir.clone(),
                    })?;
                }
            }

            git.worktree_remove(&repo_path, &worktree_path, false)?;
            git.delete_local_branch(&repo_path, &workspace.branch, false)?;
        }
    }

    // Remove workspace directory
    if !args.no_clean {
        if Path::new(&ws_dir).exists() {
            std::fs::remove_dir_all(&ws_dir)?;
        }
    }

    // Kill zellij session
    let session_name = match &workspace.session_mode as &str {
        "shared" => workspace.session_name.clone(),
        _ => Some(format!("zootree-{}", workspace.name)),
    };
    if let Some(sn) = &session_name {
        let _ = zellij.kill_session(sn); // ignore error if session doesn't exist
    }

    // Archive
    let now = Local::now().to_rfc3339();
    workspace.events.push(Event {
        action: "done".into(),
        timestamp: now,
        detail: None,
    });
    config_mgr.save_workspace(&WorkspaceStatus::InProgress, &workspace)?;
    config_mgr.move_workspace(&name, &WorkspaceStatus::InProgress, &WorkspaceStatus::Done)?;

    println!("workspace '{}' completed", name);
    Ok(())
}
```

- [ ] **Step 2: Implement cancel handler**

```rust
pub fn handle_cancel(args: &CancelArgs) -> Result<()> {
    let config_mgr = ConfigManager::new()?;
    let global = config_mgr.load_global_config()?;
    let runner = RealRunner;
    let git = GitOps::new(&runner);
    let hook_engine = HookEngine::new(&runner);
    let zellij = ZellijOps::new(&runner);

    let name = match &args.name {
        Some(n) => n.clone(),
        None => {
            let in_progress = config_mgr.list_workspaces(Some(&WorkspaceStatus::InProgress))?;
            if in_progress.is_empty() {
                anyhow::bail!("no in_progress workspaces");
            }
            let names: Vec<String> = in_progress.iter().map(|w| format!("{} - {}", w.name, w.title)).collect();
            let idx = tui::select_one("Select workspace to cancel", &names)?;
            in_progress[idx].name.clone()
        }
    };

    let (status, mut workspace) = config_mgr.load_workspace(&name)?;
    if !matches!(status, WorkspaceStatus::InProgress) {
        anyhow::bail!("workspace '{}' is not in_progress", name);
    }

    let ws_dir = shellexpand::tilde(&workspace.workspace_dir).into_owned();

    // Confirm if uncommitted changes exist
    if !args.force {
        for repo_entry in &workspace.repos {
            let worktree_path = format!("{}/{}", ws_dir, repo_entry.name);
            if Path::new(&worktree_path).exists() && git.has_uncommitted_changes(&worktree_path)? {
                if !tui::confirm(
                    &format!("repo '{}' has uncommitted changes. Continue?", repo_entry.name),
                    false,
                )? {
                    anyhow::bail!("canceled by user");
                }
            }
        }
    }

    // pre_cancel hook
    if !args.force {
        hook_engine.execute_if_set(&global.hooks.pre_cancel, &HookContext {
            workspace: workspace.name.clone(),
            repo: None,
            branch: workspace.branch.clone(),
            target_branch: None,
            worktree_path: None,
            workspace_dir: ws_dir.clone(),
        })?;
    }

    if !args.no_clean {
        for repo_entry in &workspace.repos {
            let repo_config = config_mgr.load_repo_config(&repo_entry.name)?;
            let repo_path = shellexpand::tilde(&repo_config.path).into_owned();
            let worktree_path = format!("{}/{}", ws_dir, repo_entry.name);

            // pre_remove hook
            let hook = repo_config.hooks.pre_remove.as_ref()
                .or(global.hooks.pre_remove.as_ref());
            if let Some(h) = hook {
                if !args.force {
                    let _ = hook_engine.execute(h, &HookContext {
                        workspace: workspace.name.clone(),
                        repo: Some(repo_entry.name.clone()),
                        branch: workspace.branch.clone(),
                        target_branch: Some(repo_entry.target_branch.clone()),
                        worktree_path: Some(worktree_path.clone()),
                        workspace_dir: ws_dir.clone(),
                    });
                }
            }

            if Path::new(&worktree_path).exists() {
                git.worktree_remove(&repo_path, &worktree_path, args.force)?;
            }
            git.delete_local_branch(&repo_path, &workspace.branch, true)?;
        }

        if Path::new(&ws_dir).exists() {
            std::fs::remove_dir_all(&ws_dir)?;
        }
    }

    // Kill zellij session
    let session_name = match &workspace.session_mode as &str {
        "shared" => workspace.session_name.clone(),
        _ => Some(format!("zootree-{}", workspace.name)),
    };
    if let Some(sn) = &session_name {
        let _ = zellij.kill_session(sn);
    }

    // Archive
    let now = Local::now().to_rfc3339();
    workspace.events.push(Event {
        action: "canceled".into(),
        timestamp: now,
        detail: None,
    });
    config_mgr.save_workspace(&WorkspaceStatus::InProgress, &workspace)?;
    config_mgr.move_workspace(&name, &WorkspaceStatus::InProgress, &WorkspaceStatus::Canceled)?;

    println!("workspace '{}' canceled", name);
    Ok(())
}
```

- [ ] **Step 3: Wire up in main.rs**

```rust
Commands::Done(args) => {
    zootree::cli::workspace::handle_done(&args)?;
}
Commands::Cancel(args) => {
    zootree::cli::workspace::handle_cancel(&args)?;
}
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo build`
Expected: compiles

- [ ] **Step 5: Commit**

```bash
git add src/
git commit -m "feat: workspace done and cancel commands with merge and cleanup"
```

---

### Task 17: Template + Prune + Logs Commands

**Files:**
- Modify: `src/cli/template.rs`
- Modify: `src/cli/prune.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Implement template handlers**

Add to `src/cli/template.rs`:

```rust
use crate::config::ConfigManager;
use crate::config::template::TemplateConfig;
use anyhow::Result;

pub fn handle_template_command(cmd: &TemplateCommands) -> Result<()> {
    let config_mgr = ConfigManager::new()?;
    config_mgr.ensure_dirs()?;

    match cmd {
        TemplateCommands::List => {
            let templates = config_mgr.list_templates()?;
            if templates.is_empty() {
                println!("no templates found");
            } else {
                for name in &templates {
                    let tmpl = config_mgr.load_template(name)?;
                    println!("  {} — repos: {}", name, tmpl.repos.join(", "));
                }
            }
            Ok(())
        }
        TemplateCommands::Save { name, from } => {
            let (_, workspace) = config_mgr.load_workspace(from)?;
            let tmpl = TemplateConfig {
                repos: workspace.repos.iter().map(|r| r.name.clone()).collect(),
                layout: workspace.layout.clone(),
                session_mode: Some(workspace.session_mode.clone()),
            };
            config_mgr.save_template(name, &tmpl)?;
            println!("template '{}' saved from workspace '{}'", name, from);
            Ok(())
        }
    }
}
```

- [ ] **Step 2: Implement prune handler**

Add to `src/cli/prune.rs`:

```rust
use crate::config::ConfigManager;
use crate::config::workspace::WorkspaceStatus;
use crate::tui;
use anyhow::Result;
use std::path::Path;

pub fn handle_prune(args: &PruneArgs) -> Result<()> {
    let config_mgr = ConfigManager::new()?;

    let mut archived = Vec::new();
    for status in [WorkspaceStatus::Done, WorkspaceStatus::Canceled] {
        let workspaces = config_mgr.list_workspaces(Some(&status))?;
        for ws in workspaces {
            archived.push((status.clone(), ws));
        }
    }

    if archived.is_empty() {
        println!("no archived workspaces to prune");
        return Ok(());
    }

    let to_prune = if args.all {
        archived
    } else {
        let names: Vec<String> = archived.iter()
            .map(|(s, w)| format!("{} ({:?})", w.name, s))
            .collect();
        let selected = tui::select_multi("Select workspaces to prune", &names)?;
        selected.into_iter().map(|i| archived[i].clone()).collect()
    };

    if to_prune.is_empty() {
        println!("nothing selected");
        return Ok(());
    }

    for (status, ws) in &to_prune {
        let ws_dir = shellexpand::tilde(&ws.workspace_dir).into_owned();

        // Remove workspace directory if it still exists
        if Path::new(&ws_dir).exists() {
            std::fs::remove_dir_all(&ws_dir)?;
            println!("  removed directory: {}", ws_dir);
        }

        // Remove config file
        config_mgr.delete_workspace_config(&ws.name, status)?;
        println!("  pruned: {}", ws.name);
    }

    println!("{} workspace(s) pruned", to_prune.len());
    Ok(())
}
```

- [ ] **Step 3: Implement logs command**

Add to `src/main.rs` in the `Commands::Logs` arm:

```rust
Commands::Logs => {
    let config_dir = dirs::config_dir()
        .ok_or_else(|| anyhow::anyhow!("cannot find config directory"))?
        .join("zootree/logs/zootree.log");
    if config_dir.exists() {
        let status = std::process::Command::new("tail")
            .args(["-f", "-n", "100"])
            .arg(&config_dir)
            .status()?;
        if !status.success() {
            anyhow::bail!("tail exited with error");
        }
    } else {
        println!("no log file found at {}", config_dir.display());
    }
}
```

- [ ] **Step 4: Wire up all commands in main.rs**

```rust
Commands::Template(args) => {
    zootree::cli::template::handle_template_command(&args.command)?;
}
Commands::Prune(args) => {
    zootree::cli::prune::handle_prune(&args)?;
}
```

- [ ] **Step 5: Verify it compiles**

Run: `cargo build`
Expected: compiles

- [ ] **Step 6: Commit**

```bash
git add src/
git commit -m "feat: template, prune, and logs commands"
```

---

### Task 18: Tracing Initialization + Log File

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Set up tracing with file and terminal output**

Update `src/main.rs` to add tracing init before command dispatch:

```rust
use tracing_subscriber::{fmt, EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};
use tracing_appender::rolling;

fn init_tracing(verbose: bool, quiet: bool) -> anyhow::Result<tracing_appender::non_blocking::WorkerGuard> {
    let config_dir = dirs::config_dir()
        .ok_or_else(|| anyhow::anyhow!("cannot find config directory"))?
        .join("zootree/logs");
    std::fs::create_dir_all(&config_dir)?;

    let file_appender = rolling::daily(&config_dir, "zootree.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    let terminal_level = if quiet {
        "error"
    } else if verbose {
        "debug"
    } else {
        "info"
    };

    let terminal_layer = fmt::layer()
        .with_target(false)
        .with_level(true)
        .with_filter(EnvFilter::new(terminal_level));

    let file_layer = fmt::layer()
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_filter(EnvFilter::new("debug"));

    tracing_subscriber::registry()
        .with(terminal_layer)
        .with(file_layer)
        .init();

    Ok(guard)
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let _guard = init_tracing(cli.verbose, cli.quiet)?;

    // ... command dispatch
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build`
Expected: compiles

- [ ] **Step 3: Commit**

```bash
git add src/
git commit -m "feat: tracing init with terminal + file logging"
```

---

### Task 19: End-to-End Smoke Test

**Files:**
- No new files — manual verification

- [ ] **Step 1: Verify help output**

Run: `cargo run -- --help`
Expected: shows all subcommands

Run: `cargo run -- repo --help`
Expected: shows repo subcommands

Run: `cargo run -- create --help`
Expected: shows create options

- [ ] **Step 2: Run all unit tests**

Run: `cargo test`
Expected: all tests pass

- [ ] **Step 3: Verify repo workflow**

```bash
cargo run -- repo add test-frontend --path /tmp
cargo run -- repo list
cargo run -- repo remove test-frontend
```

- [ ] **Step 4: Verify workspace create (CLI mode)**

```bash
# First add repos
cargo run -- repo add test-repo --path <some-git-repo-path>
cargo run -- create --title "test workspace" --repos test-repo:main
cargo run -- list
```

- [ ] **Step 5: Final commit**

```bash
git add -A
git commit -m "chore: end-to-end smoke test verification"
```

---

## Summary

| Task | Component | Description |
|------|-----------|-------------|
| 1 | Scaffold | Cargo project + CommandRunner trait |
| 2 | Config | GlobalConfig + ConfigManager |
| 3 | Config | RepoConfig |
| 4 | Config | WorkspaceConfig + TemplateConfig |
| 5 | Core | Name generator |
| 6 | Core | Git worktree operations |
| 7 | Core | Hook engine |
| 8 | Core | Copy files |
| 9 | Core | Layout renderer |
| 10 | Core | Zellij operations |
| 11 | CLI | CLI definition (clap) |
| 12 | CLI | Repo commands |
| 13 | CLI | Workspace create |
| 14 | CLI | Workspace start |
| 15 | CLI | Workspace list + open |
| 16 | CLI | Workspace done + cancel |
| 17 | CLI | Template + prune + logs |
| 18 | Infra | Tracing initialization |
| 19 | Test | End-to-end smoke test |
