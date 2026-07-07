# Configurable Terminal Multiplexer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace zellij-only workspace launching with a configurable terminal multiplexer abstraction that supports zellij by default and cmux via JSON layouts.

**Architecture:** Add a `multiplexer` config model and a focused `src/core/multiplexer/` module with zellij and cmux implementations. Keep workspace command orchestration in `src/cli/workspace.rs`, but move external zellij/cmux command decisions behind multiplexer operations and render zellij KDL or cmux JSON before launch.

**Tech Stack:** Rust 2021, clap, serde/toml, serde_json, anyhow, shlex, existing `CommandRunner` / `MockRunner`, existing zootree config manager and CLI tests.

---

## File Structure

- Modify `Cargo.toml`: add `serde_json` dependency and update package metadata from zellij-only wording.
- Modify `src/config/global.rs`: replace `ZellijConfig` with `MultiplexerConfig`, `MultiplexerKind`, `ZellijMultiplexerConfig`, and `CmuxMultiplexerConfig`.
- Modify `src/config/workspace.rs`: replace `zellij` with `multiplexer`, add persisted `MultiplexerState`.
- Modify `src/config/repo.rs`: replace optional `zellij` override with optional `multiplexer` override.
- Modify `src/config/template.rs`: replace template `zellij` config with `multiplexer`.
- Create `src/core/multiplexer/mod.rs`: shared interface, launch context, identity/state helpers, layout name resolution.
- Create `src/core/multiplexer/zellij.rs`: move zellij launch/attach/close behavior behind `ZellijMultiplexer`.
- Create `src/core/multiplexer/cmux.rs`: cmux command wrappers, workspace lookup, launch/open/close.
- Create `src/core/cmux_layout.rs`: cmux JSON template rendering, repeat expansion, variable replacement, default template.
- Modify `src/core/mod.rs`: expose `multiplexer` and `cmux_layout`, remove direct `zellij` module export after migration.
- Modify `src/core/layout.rs`: add reusable agent command builder for cmux command strings.
- Modify `src/cli/workspace.rs`: rename `--no-zellij`, render selected layout type, call selected multiplexer.
- Modify `src/cli/create_flow.rs`, `src/cli/repo.rs`, `src/cli/template.rs`, `src/core/repo_names.rs`, `src/core/worktree_status.rs`, `src/cli/info.rs`, `src/tui_app/info.rs`: update config field names and fixture construction.
- Modify tests under `tests/`: config, zellij multiplexer, cmux multiplexer, cmux layout, create flow, completions, agent CLI, info, start-agent tests.
- Modify `README.md`, `README.zh-CN.md`, `skills/zootree-dev/SKILL.md`, `skills/zootree-usage/SKILL.md`: document configurable multiplexer and cmux layout path.

---

### Task 1: Add Multiplexer Config Types

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/config/global.rs`
- Modify: `src/config/workspace.rs`
- Modify: `src/config/repo.rs`
- Modify: `src/config/template.rs`
- Test: `tests/config_test.rs`

- [ ] **Step 1: Write failing config tests**

Add these imports and tests to `tests/config_test.rs`:

```rust
use zootree::config::global::{MultiplexerKind, MultiplexerConfig};
```

```rust
#[test]
fn parse_global_config_defaults_to_zellij_multiplexer() {
    let config: GlobalConfig = toml::from_str("").unwrap();

    assert_eq!(config.multiplexer.kind, MultiplexerKind::Zellij);
    assert_eq!(config.multiplexer.zellij.layout.as_deref(), Some("default"));
    assert_eq!(config.multiplexer.cmux.layout.as_deref(), Some("default"));
}

#[test]
fn parse_global_config_with_cmux_multiplexer() {
    let toml_str = r#"
workspace_root = "~/zootree-workspaces"
branch_prefix = "zootree"

[multiplexer]
kind = "cmux"

[multiplexer.cmux]
layout = "daily"
"#;

    let config: GlobalConfig = toml::from_str(toml_str).unwrap();

    assert_eq!(config.multiplexer.kind, MultiplexerKind::Cmux);
    assert_eq!(config.multiplexer.cmux.layout.as_deref(), Some("daily"));
    assert_eq!(config.multiplexer.zellij.layout.as_deref(), Some("default"));
}

#[test]
fn parse_workspace_config_with_multiplexer_state() {
    let toml_str = r#"
title = "用户认证功能"
name = "calm-river"
description = "前后端联调 OAuth2 登录"
branch = "zootree/calm-river"
workspace_dir = "~/zootree-workspaces/calm-river"
created_at = "2026-04-28T10:30:00+08:00"

[multiplexer]
kind = "cmux"

[multiplexer.cmux]
layout = "wide"

[multiplexer_state]
kind = "cmux"
cmux_workspace = "workspace:3"
"#;

    let config: WorkspaceConfig = toml::from_str(toml_str).unwrap();

    assert_eq!(config.multiplexer.kind, MultiplexerKind::Cmux);
    assert_eq!(config.multiplexer.cmux.layout.as_deref(), Some("wide"));
    assert_eq!(config.multiplexer_state.kind, Some(MultiplexerKind::Cmux));
    assert_eq!(
        config.multiplexer_state.cmux_workspace.as_deref(),
        Some("workspace:3")
    );
}
```

- [ ] **Step 2: Run the new config tests and verify failure**

Run:

```bash
cargo test --test config_test parse_global_config_defaults_to_zellij_multiplexer parse_global_config_with_cmux_multiplexer parse_workspace_config_with_multiplexer_state
```

Expected: compile failure mentioning missing `MultiplexerKind`, missing `multiplexer`, and missing `multiplexer_state`.

- [ ] **Step 3: Add serde_json dependency**

Modify `Cargo.toml` dependencies:

```toml
serde_json = "1"
```

Also update package metadata to no longer be zellij-only:

```toml
description = "A multi-repo workspace management tool with worktree and terminal multiplexer integration"
keywords = ["workspace", "worktree", "multiplexer", "multi-repo"]
```

- [ ] **Step 4: Replace zellij config in `src/config/global.rs`**

Replace the existing `ZellijConfig` type and `GlobalConfig.zellij` field with:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MultiplexerKind {
    Zellij,
    Cmux,
}

impl Default for MultiplexerKind {
    fn default() -> Self {
        Self::Zellij
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ZellijMultiplexerConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layout: Option<String>,
}

impl Default for ZellijMultiplexerConfig {
    fn default() -> Self {
        Self {
            layout: Some("default".into()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CmuxMultiplexerConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layout: Option<String>,
}

impl Default for CmuxMultiplexerConfig {
    fn default() -> Self {
        Self {
            layout: Some("default".into()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MultiplexerConfig {
    #[serde(default)]
    pub kind: MultiplexerKind,
    #[serde(default)]
    pub zellij: ZellijMultiplexerConfig,
    #[serde(default)]
    pub cmux: CmuxMultiplexerConfig,
}

impl Default for MultiplexerConfig {
    fn default() -> Self {
        Self {
            kind: MultiplexerKind::Zellij,
            zellij: ZellijMultiplexerConfig::default(),
            cmux: CmuxMultiplexerConfig::default(),
        }
    }
}
```

Change `GlobalConfig` to:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GlobalConfig {
    #[serde(default)]
    pub multiplexer: MultiplexerConfig,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_cli: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub agent_cli_alias: BTreeMap<String, String>,
}
```

Change `GlobalConfig::default()` field initialization to:

```rust
multiplexer: MultiplexerConfig::default(),
```

- [ ] **Step 5: Update workspace config**

In `src/config/workspace.rs`, replace `use super::global::ZellijConfig;` with:

```rust
use super::global::{MultiplexerConfig, MultiplexerKind};
```

Add:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct MultiplexerState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<MultiplexerKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cmux_workspace: Option<String>,
}
```

Change `WorkspaceConfig` fields:

```rust
#[serde(default)]
pub multiplexer: MultiplexerConfig,
#[serde(default, skip_serializing_if = "MultiplexerState::is_empty")]
pub multiplexer_state: MultiplexerState,
```

Add this impl below the struct definitions:

```rust
impl MultiplexerState {
    pub fn is_empty(&self) -> bool {
        self.kind.is_none() && self.cmux_workspace.is_none()
    }
}
```

- [ ] **Step 6: Update repo and template config**

In `src/config/repo.rs`, change imports and fields:

```rust
use super::global::{HooksConfig, MultiplexerConfig};
```

```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
pub multiplexer: Option<MultiplexerConfig>,
```

In `src/config/template.rs`, change imports and fields:

```rust
use super::global::MultiplexerConfig;
```

```rust
#[serde(default)]
pub multiplexer: MultiplexerConfig,
```

- [ ] **Step 7: Update current config tests from zellij to multiplexer**

In `tests/config_test.rs`, replace `[zellij]` test TOML blocks with:

```toml
[multiplexer]
kind = "zellij"

[multiplexer.zellij]
layout = "default"
```

Replace assertions like:

```rust
assert_eq!(config.zellij.layout, Some("default".into()));
```

with:

```rust
assert_eq!(config.multiplexer.kind, MultiplexerKind::Zellij);
assert_eq!(config.multiplexer.zellij.layout, Some("default".into()));
```

For template config tests, replace:

```rust
assert_eq!(config.zellij.layout, Some("default".into()));
assert_eq!(config.zellij.session_mode, Some("standalone".into()));
```

with:

```rust
assert_eq!(config.multiplexer.kind, MultiplexerKind::Zellij);
assert_eq!(config.multiplexer.zellij.layout, Some("default".into()));
```

- [ ] **Step 8: Run config tests**

Run:

```bash
cargo test --test config_test
```

Expected: config tests pass or compile errors only in files still referring to `ZellijConfig`. Those remaining compile errors are handled by Task 2.

- [ ] **Step 9: Commit config model**

```bash
git add Cargo.toml src/config/global.rs src/config/workspace.rs src/config/repo.rs src/config/template.rs tests/config_test.rs
git commit -m "feat: add multiplexer config model"
```

---

### Task 2: Update Config Consumers and Fixtures

**Files:**
- Modify: `src/cli/create_flow.rs`
- Modify: `src/cli/repo.rs`
- Modify: `src/cli/template.rs`
- Modify: `src/core/repo_names.rs`
- Modify: `src/core/worktree_status.rs`
- Modify: `src/cli/info.rs`
- Modify: `src/tui_app/info.rs`
- Modify: tests that construct `GlobalConfig`, `WorkspaceConfig`, `RepoConfig`, or `TemplateConfig`

- [ ] **Step 1: Run compile check to list stale zellij references**

Run:

```bash
cargo check --tests
```

Expected: compile errors mentioning `ZellijConfig`, `.zellij`, `no_zellij`, or `crate::core::zellij`.

- [ ] **Step 2: Update create flow config imports and draft mapping**

In `src/cli/create_flow.rs`, replace:

```rust
use crate::config::global::ZellijConfig;
```

with:

```rust
use crate::config::global::{MultiplexerConfig, MultiplexerKind};
```

Where a draft currently creates:

```rust
multiplexer: MultiplexerConfig::default(),
multiplexer_state: Default::default(),
```

Also change `workspace_from_draft` to accept the selected multiplexer config:

```rust
pub fn workspace_from_draft(
    draft: &CreateDraft,
    created_at: impl Into<String>,
    agent_cli: Option<String>,
    multiplexer: MultiplexerConfig,
) -> WorkspaceConfig {
```

Inside that function, set:

```rust
multiplexer,
multiplexer_state: Default::default(),
```

In `src/cli/workspace.rs`, update the create call from:

```rust
let workspace = workspace_from_draft(&output.draft, Local::now().to_rfc3339(), agent_cli);
```

to:

```rust
let workspace = workspace_from_draft(
    &output.draft,
    Local::now().to_rfc3339(),
    agent_cli,
    global.multiplexer.clone(),
);
```

This makes new workspaces remember the configured multiplexer at creation time instead of re-reading mutable global defaults on subsequent start/open commands.

Where repo config creates `zellij: None`, replace with:

```rust
multiplexer: None,
```
- [ ] **Step 3: Update repo CLI config creation**

In `src/cli/repo.rs`, replace repo config initializers:

```rust
zellij: None,
```

with:

```rust
multiplexer: None,
```

- [ ] **Step 4: Update template save/load**

In `src/cli/template.rs`, replace:

```rust
zellij: workspace.zellij.clone(),
```

with:

```rust
multiplexer: workspace.multiplexer.clone(),
```

- [ ] **Step 5: Update pure helper fixtures**

In `src/core/repo_names.rs` and `src/core/worktree_status.rs`, replace imports:

```rust
use crate::config::global::ZellijConfig;
```

with:

```rust
use crate::config::global::MultiplexerConfig;
```

Replace workspace fixture fields:

```rust
zellij: ZellijConfig::default(),
```

with:

```rust
multiplexer: MultiplexerConfig::default(),
multiplexer_state: Default::default(),
```

Replace repo fixture fields:

```rust
zellij: None,
```

with:

```rust
multiplexer: None,
```

- [ ] **Step 6: Update info render fixtures**

In `src/cli/info.rs` and `src/tui_app/info.rs` tests, replace `ZellijConfig` imports with `MultiplexerConfig`, then replace `WorkspaceConfig` fixture fields:

```rust
multiplexer: MultiplexerConfig::default(),
multiplexer_state: Default::default(),
```

Replace `RepoConfig` fixture fields:

```rust
multiplexer: None,
```

- [ ] **Step 7: Update test fixtures across `tests/`**

Use:

```bash
rg -n "ZellijConfig|zellij:" tests src -g '!target'
```

For every `WorkspaceConfig` literal, ensure it contains:

```rust
multiplexer: MultiplexerConfig::default(),
multiplexer_state: Default::default(),
```

For every `RepoConfig` literal, ensure it contains:

```rust
multiplexer: None,
```

For every `TemplateConfig` literal, ensure it contains:

```rust
multiplexer: MultiplexerConfig::default(),
```

- [ ] **Step 8: Run compile check**

Run:

```bash
cargo check --tests
```

Expected: remaining errors should be in zellij launch wiring and tests, not config struct construction.

- [ ] **Step 9: Commit consumer migration**

```bash
git add src tests
git commit -m "refactor: migrate config consumers to multiplexer fields"
```

---

### Task 3: Create Zellij Multiplexer Module

**Files:**
- Create: `src/core/multiplexer/mod.rs`
- Create: `src/core/multiplexer/zellij.rs`
- Modify: `src/core/mod.rs`
- Modify: `tests/zellij_test.rs`

- [ ] **Step 1: Write failing zellij multiplexer tests**

In `tests/zellij_test.rs`, replace the import:

```rust
use zootree::core::zellij::{plan_launch, LaunchPlan, ZellijOps};
```

with:

```rust
use zootree::core::multiplexer::{
    zellij::{plan_launch, LaunchPlan, ZellijMultiplexer},
    MultiplexerIdentity, MultiplexerLaunch, TerminalMultiplexer,
};
```

Replace `ZellijOps::new(&runner)` with:

```rust
ZellijMultiplexer::new(&runner, false)
```

Add a helper:

```rust
fn launch() -> MultiplexerLaunch {
    MultiplexerLaunch {
        workspace_name: "fair-fox".into(),
        display_name: "zootree-fair-fox".into(),
        workspace_dir: "/tmp/fair-fox".into(),
        layout_name: "default".into(),
        rendered_layout: "layout {}".into(),
        layout_file: "/tmp/layout.kdl".into(),
    }
}

fn identity() -> MultiplexerIdentity {
    MultiplexerIdentity {
        workspace_name: "fair-fox".into(),
        display_name: "zootree-fair-fox".into(),
        cmux_workspace: None,
    }
}
```

Change kill test body to:

```rust
let zellij = ZellijMultiplexer::new(&runner, false);
zellij.close(&identity()).unwrap();
```

- [ ] **Step 2: Run zellij test and verify failure**

Run:

```bash
cargo test --test zellij_test
```

Expected: compile failure because `core::multiplexer` does not exist.

- [ ] **Step 3: Add `src/core/multiplexer/mod.rs`**

Create `src/core/multiplexer/mod.rs`:

```rust
pub mod zellij;

use crate::config::global::MultiplexerKind;
use anyhow::Result;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultiplexerLaunch {
    pub workspace_name: String,
    pub display_name: String,
    pub workspace_dir: PathBuf,
    pub layout_name: String,
    pub rendered_layout: String,
    pub layout_file: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultiplexerIdentity {
    pub workspace_name: String,
    pub display_name: String,
    pub cmux_workspace: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchOutcome {
    Launched,
    Attached,
    AlreadyRunning,
    BackgroundCreated,
}

pub trait TerminalMultiplexer {
    fn kind(&self) -> MultiplexerKind;
    fn launch(&self, launch: &MultiplexerLaunch) -> Result<LaunchOutcome>;
    fn open(
        &self,
        launch: &MultiplexerLaunch,
        identity: &MultiplexerIdentity,
    ) -> Result<LaunchOutcome>;
    fn close(&self, identity: &MultiplexerIdentity) -> Result<()>;
}
```

- [ ] **Step 4: Add `src/core/multiplexer/zellij.rs`**

Create `src/core/multiplexer/zellij.rs` by moving the behavior from `src/core/zellij.rs` and renaming the type:

```rust
use super::{LaunchOutcome, MultiplexerIdentity, MultiplexerLaunch, TerminalMultiplexer};
use crate::config::global::MultiplexerKind;
use crate::runner::{CommandRunner, CommandSpec};
use anyhow::{bail, Result};
use std::collections::HashMap;
use std::path::Path;
use tracing::info;

pub fn is_inside_zellij() -> bool {
    std::env::var_os("ZELLIJ").is_some() || std::env::var_os("ZELLIJ_SESSION_NAME").is_some()
}

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
        (false, true) => LaunchPlan::ForegroundAttach,
        (true, false) => LaunchPlan::BackgroundCreate,
        (true, true) => LaunchPlan::AlreadyRunningHint,
    }
}

fn session_list_line_matches(line: &str, session_name: &str) -> bool {
    line.split_whitespace().next() == Some(session_name)
}

pub struct ZellijMultiplexer<'a, R: CommandRunner> {
    runner: &'a R,
    in_zellij: bool,
}

impl<'a, R: CommandRunner> ZellijMultiplexer<'a, R> {
    pub fn new(runner: &'a R, in_zellij: bool) -> Self {
        Self { runner, in_zellij }
    }

    fn zellij(&self, args: Vec<String>) -> Result<std::process::Output> {
        self.runner.run(&CommandSpec {
            program: "zellij".into(),
            args,
            cwd: None,
            env: HashMap::new(),
            env_remove: vec![],
        })
    }

    fn zellij_interactive(&self, args: Vec<String>) -> Result<()> {
        let status = self.runner.run_interactive(&CommandSpec {
            program: "zellij".into(),
            args,
            cwd: None,
            env: HashMap::new(),
            env_remove: vec![],
        })?;
        if !status.success() {
            let reason = status
                .code()
                .map(|c| format!("exit code {}", c))
                .unwrap_or_else(|| "terminated by signal".into());
            bail!("zellij exited with {}", reason);
        }
        Ok(())
    }

    fn start_session(&self, session_name: &str, layout_path: &Path) -> Result<()> {
        info!("starting zellij session: {}", session_name);
        self.zellij_interactive(vec![
            "--new-session-with-layout".into(),
            layout_path.to_string_lossy().into(),
            "--session".into(),
            session_name.into(),
        ])
    }

    fn start_session_background(&self, session_name: &str, layout_path: &Path) -> Result<()> {
        info!("starting zellij session in background: {}", session_name);
        let output = self.runner.run(&CommandSpec {
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
        })?;
        if !output.status.success() {
            bail!(
                "zellij background session create failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(())
    }

    fn attach_session(&self, session_name: &str) -> Result<()> {
        info!("attaching to zellij session: {}", session_name);
        self.zellij_interactive(vec!["attach".into(), session_name.into()])
    }

    fn session_exists(&self, session_name: &str) -> Result<bool> {
        let output = self.zellij(vec!["list-sessions".into()])?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(stdout
            .lines()
            .any(|line| session_list_line_matches(line, session_name)))
    }

    fn dispatch_launch(&self, launch: &MultiplexerLaunch) -> Result<LaunchOutcome> {
        let session_exists = self.session_exists(&launch.display_name)?;
        let plan = plan_launch(self.in_zellij, session_exists);
        match plan {
            LaunchPlan::ForegroundCreate => {
                self.start_session(&launch.display_name, &launch.layout_file)?;
                Ok(LaunchOutcome::Launched)
            }
            LaunchPlan::ForegroundAttach => {
                self.attach_session(&launch.display_name)?;
                Ok(LaunchOutcome::Attached)
            }
            LaunchPlan::BackgroundCreate => {
                self.start_session_background(&launch.display_name, &launch.layout_file)?;
                println!("zellij session '{}' is running in background.", launch.display_name);
                println!(
                    "Run `zootree open {}` (outside zellij) to attach.",
                    launch.workspace_name
                );
                Ok(LaunchOutcome::BackgroundCreated)
            }
            LaunchPlan::AlreadyRunningHint => {
                println!("zellij session '{}' already exists.", launch.display_name);
                println!(
                    "Run `zootree open {}` (outside zellij) to attach.",
                    launch.workspace_name
                );
                Ok(LaunchOutcome::AlreadyRunning)
            }
        }
    }
}

impl<R: CommandRunner> TerminalMultiplexer for ZellijMultiplexer<'_, R> {
    fn kind(&self) -> MultiplexerKind {
        MultiplexerKind::Zellij
    }

    fn launch(&self, launch: &MultiplexerLaunch) -> Result<LaunchOutcome> {
        self.dispatch_launch(launch)
    }

    fn open(
        &self,
        launch: &MultiplexerLaunch,
        _identity: &MultiplexerIdentity,
    ) -> Result<LaunchOutcome> {
        self.dispatch_launch(launch)
    }

    fn close(&self, identity: &MultiplexerIdentity) -> Result<()> {
        info!("killing zellij session: {}", identity.display_name);
        let _ = self.zellij(vec![
            "delete-session".into(),
            "--force".into(),
            identity.display_name.clone(),
        ]);
        Ok(())
    }
}
```

- [ ] **Step 5: Expose the new module**

In `src/core/mod.rs`, add:

```rust
pub mod multiplexer;
```

Keep `pub mod zellij;` until `workspace.rs` is migrated in Task 6, then remove it.

- [ ] **Step 6: Update zellij tests to call trait methods**

In `tests/zellij_test.rs`, replace dispatch tests that import `zootree::cli::workspace::dispatch_launch` with direct calls:

```rust
let zellij = ZellijMultiplexer::new(&runner, true);
zellij.launch(&launch()).unwrap();
```

For outside zellij:

```rust
let zellij = ZellijMultiplexer::new(&runner, false);
zellij.launch(&launch()).unwrap();
```

Keep the existing command assertions for `list-sessions`, `--create-background`, and `--new-session-with-layout`.

- [ ] **Step 7: Run zellij tests**

Run:

```bash
cargo test --test zellij_test
```

Expected: pass after import and call migration.

- [ ] **Step 8: Commit zellij multiplexer**

```bash
git add src/core/mod.rs src/core/multiplexer/mod.rs src/core/multiplexer/zellij.rs tests/zellij_test.rs
git commit -m "refactor: move zellij behind multiplexer interface"
```

---

### Task 4: Add Cmux JSON Layout Renderer

**Files:**
- Create: `src/core/cmux_layout.rs`
- Modify: `src/core/mod.rs`
- Modify: `src/core/layout.rs`
- Test: `tests/cmux_layout_test.rs`

- [ ] **Step 1: Write failing cmux layout tests**

Create `tests/cmux_layout_test.rs`:

```rust
use zootree::config::workspace::{RepoEntry, WorkspaceConfig};
use zootree::core::cmux_layout::{render_cmux_layout, CmuxLayoutVar};
use zootree::config::global::{MultiplexerConfig, MultiplexerKind};

fn workspace(repos: Vec<&str>) -> WorkspaceConfig {
    WorkspaceConfig {
        title: "Fix auth".into(),
        name: "fair-fox".into(),
        description: "OAuth callback".into(),
        branch: "zootree/fair-fox".into(),
        workspace_dir: "/tmp/fair-fox".into(),
        created_at: "2026-07-07T10:00:00+08:00".into(),
        agent_cli: None,
        multiplexer: MultiplexerConfig::default(),
        multiplexer_state: Default::default(),
        repos: repos
            .into_iter()
            .map(|name| RepoEntry {
                name: name.into(),
                target_branch: Some("main".into()),
            })
            .collect(),
        events: Vec::new(),
    }
}

fn vars() -> Vec<CmuxLayoutVar> {
    vec![
        CmuxLayoutVar {
            repo_name: "api".into(),
            worktree_path: "/tmp/fair-fox/api".into(),
            branch: "zootree/fair-fox".into(),
            workspace_name: "fair-fox".into(),
            workspace_dir: "/tmp/fair-fox".into(),
            lazygit_config: String::new(),
            overview_agent_command: String::new(),
            repo_agent_command: "claude --print 'Fix auth'".into(),
        },
        CmuxLayoutVar {
            repo_name: "web".into(),
            worktree_path: "/tmp/fair-fox/web".into(),
            branch: "zootree/fair-fox".into(),
            workspace_name: "fair-fox".into(),
            workspace_dir: "/tmp/fair-fox".into(),
            lazygit_config: String::new(),
            overview_agent_command: String::new(),
            repo_agent_command: String::new(),
        },
    ]
}

#[test]
fn default_cmux_layout_is_valid_json() {
    let rendered = render_cmux_layout(zootree::core::cmux_layout::default_cmux_layout(), &vars()).unwrap();
    serde_json::from_str::<serde_json::Value>(&rendered).unwrap();
}

#[test]
fn repeat_per_repo_expands_once_per_repo() {
    let rendered = render_cmux_layout(zootree::core::cmux_layout::default_cmux_layout(), &vars()).unwrap();
    assert!(rendered.contains("/tmp/fair-fox/api"));
    assert!(rendered.contains("/tmp/fair-fox/web"));
    assert!(rendered.contains("lazygit -p /tmp/fair-fox/api"));
    assert!(rendered.contains("lazygit -p /tmp/fair-fox/web"));
}

#[test]
fn empty_agent_command_surfaces_are_removed() {
    let rendered = render_cmux_layout(zootree::core::cmux_layout::default_cmux_layout(), &vars()).unwrap();
    assert!(!rendered.contains(r#""command":"""#));
    assert!(!rendered.contains(r#""command": "" "#.trim()));
}

#[test]
fn repo_agent_command_is_preserved_for_single_repo_in_repo_area() {
    let one_repo = vec![vars().remove(0)];
    let rendered = render_cmux_layout(zootree::core::cmux_layout::default_cmux_layout(), &one_repo).unwrap();
    assert!(rendered.contains("claude --print"));
}
```

- [ ] **Step 2: Run layout tests and verify failure**

Run:

```bash
cargo test --test cmux_layout_test
```

Expected: compile failure because `core::cmux_layout` does not exist.

- [ ] **Step 3: Add cmux agent command helper to `src/core/layout.rs`**

Add:

```rust
pub fn build_agent_cli_command(agent_cli_tpl: &str, prompt: &str) -> anyhow::Result<String> {
    let tokens = shlex::split(agent_cli_tpl)
        .ok_or_else(|| anyhow::anyhow!("failed to parse agent_cli: {}", agent_cli_tpl))?;
    if tokens.is_empty() {
        anyhow::bail!("agent_cli is empty");
    }

    let substituted: Vec<String> = tokens
        .into_iter()
        .map(|t| t.replace("$prompt", prompt))
        .collect();

    shlex::try_join(substituted.iter().map(String::as_str))
        .map_err(|e| anyhow::anyhow!("failed to join agent_cli: {}", e))
}
```

- [ ] **Step 4: Create `src/core/cmux_layout.rs`**

Create `src/core/cmux_layout.rs`:

```rust
use anyhow::Result;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CmuxLayoutVar {
    pub repo_name: String,
    pub worktree_path: String,
    pub branch: String,
    pub workspace_name: String,
    pub workspace_dir: String,
    pub lazygit_config: String,
    pub overview_agent_command: String,
    pub repo_agent_command: String,
}

pub fn default_cmux_layout() -> &'static str {
    r#"{
  "direction": "horizontal",
  "children": [
    {
      "pane": {
        "surfaces": [
          {
            "type": "terminal",
            "command": "zootree info $workspace_name --watch",
            "cwd": "$workspace_dir"
          },
          {
            "type": "terminal",
            "command": "$overview_agent_command",
            "cwd": "$workspace_dir"
          }
        ]
      }
    },
    {
      "zootree_repeat_per_repo": {
        "direction": "vertical",
        "children": [
          {
            "pane": {
              "surfaces": [
                {
                  "type": "terminal",
                  "command": "lazygit -p $worktree_path",
                  "cwd": "$worktree_path"
                }
              ]
            }
          },
          {
            "pane": {
              "surfaces": [
                {
                  "type": "terminal",
                  "cwd": "$worktree_path"
                },
                {
                  "type": "terminal",
                  "command": "$repo_agent_command",
                  "cwd": "$worktree_path"
                }
              ]
            }
          }
        ]
      }
    }
  ]
}"#
}

pub fn render_cmux_layout(template: &str, repos: &[CmuxLayoutVar]) -> Result<String> {
    let mut value: Value = serde_json::from_str(template)?;
    expand_value(&mut value, repos, repos.first())?;
    prune_empty(&mut value);
    Ok(serde_json::to_string(&value)?)
}

fn expand_value(value: &mut Value, repos: &[CmuxLayoutVar], workspace_vars: Option<&CmuxLayoutVar>) -> Result<()> {
    match value {
        Value::Object(map) => {
            if let Some(repeat) = map.remove("zootree_repeat_per_repo") {
                let expanded = repos
                    .iter()
                    .map(|repo| {
                        let mut item = repeat.clone();
                        expand_value(&mut item, repos, Some(repo))?;
                        Ok(item)
                    })
                    .collect::<Result<Vec<_>>>()?;
                *value = Value::Array(expanded);
                return Ok(());
            }
            for child in map.values_mut() {
                expand_value(child, repos, workspace_vars)?;
            }
        }
        Value::Array(items) => {
            for item in items.iter_mut() {
                expand_value(item, repos, workspace_vars)?;
            }
            flatten_arrays(items);
        }
        Value::String(s) => {
            if let Some(vars) = workspace_vars {
                *s = replace_vars(s, vars);
            }
        }
        _ => {}
    }
    Ok(())
}

fn flatten_arrays(items: &mut Vec<Value>) {
    let mut flattened = Vec::new();
    for item in std::mem::take(items) {
        match item {
            Value::Array(nested) => flattened.extend(nested),
            other => flattened.push(other),
        }
    }
    *items = flattened;
}

fn replace_vars(input: &str, vars: &CmuxLayoutVar) -> String {
    input
        .replace("$workspace_name", &vars.workspace_name)
        .replace("$workspace_dir", &vars.workspace_dir)
        .replace("$repo_name", &vars.repo_name)
        .replace("$worktree_path", &vars.worktree_path)
        .replace("$branch", &vars.branch)
        .replace("$lazygit_config", &vars.lazygit_config)
        .replace("$overview_agent_command", &vars.overview_agent_command)
        .replace("$repo_agent_command", &vars.repo_agent_command)
}

fn prune_empty(value: &mut Value) -> bool {
    match value {
        Value::Object(map) => {
            let empty_command = map
                .get("command")
                .and_then(Value::as_str)
                .is_some_and(str::is_empty);
            if empty_command {
                return true;
            }

            let keys = map.keys().cloned().collect::<Vec<_>>();
            for key in keys {
                let remove = map.get_mut(&key).is_some_and(prune_empty);
                if remove {
                    map.remove(&key);
                }
            }

            if let Some(Value::Array(surfaces)) = map.get("surfaces") {
                if surfaces.is_empty() {
                    return true;
                }
            }
            if let Some(Value::Array(children)) = map.get("children") {
                if children.is_empty() {
                    return true;
                }
            }
            false
        }
        Value::Array(items) => {
            let mut retained = Vec::new();
            for mut item in std::mem::take(items) {
                if !prune_empty(&mut item) {
                    retained.push(item);
                }
            }
            *items = retained;
            false
        }
        _ => false,
    }
}
```

- [ ] **Step 5: Expose cmux layout module**

In `src/core/mod.rs`, add:

```rust
pub mod cmux_layout;
```

- [ ] **Step 6: Run cmux layout tests**

Run:

```bash
cargo test --test cmux_layout_test
```

Expected: tests pass. If JSON ordering changes assertions, keep assertions substring-based.

- [ ] **Step 7: Commit cmux layout renderer**

```bash
git add Cargo.toml src/core/mod.rs src/core/layout.rs src/core/cmux_layout.rs tests/cmux_layout_test.rs
git commit -m "feat: add cmux layout renderer"
```

---

### Task 5: Add Cmux Multiplexer Operations

**Files:**
- Create: `src/core/multiplexer/cmux.rs`
- Modify: `src/core/multiplexer/mod.rs`
- Test: `tests/cmux_test.rs`

- [ ] **Step 1: Write failing cmux operation tests**

Create `tests/cmux_test.rs`:

```rust
use std::collections::HashMap;
use std::os::unix::process::ExitStatusExt;
use std::process::{ExitStatus, Output};
use zootree::core::multiplexer::{
    cmux::CmuxMultiplexer, LaunchOutcome, MultiplexerIdentity, MultiplexerLaunch,
    TerminalMultiplexer,
};
use zootree::runner::MockRunner;

fn success_output(stdout: &[u8]) -> Output {
    Output {
        status: ExitStatus::from_raw(0),
        stdout: stdout.to_vec(),
        stderr: Vec::new(),
    }
}

fn launch() -> MultiplexerLaunch {
    MultiplexerLaunch {
        workspace_name: "fair-fox".into(),
        display_name: "zootree-fair-fox".into(),
        workspace_dir: "/tmp/fair-fox".into(),
        layout_name: "default".into(),
        rendered_layout: r#"{"pane":{"surfaces":[{"type":"terminal"}]}}"#.into(),
        layout_file: "/tmp/default.cmux.json".into(),
    }
}

fn identity(cmux_workspace: Option<&str>) -> MultiplexerIdentity {
    MultiplexerIdentity {
        workspace_name: "fair-fox".into(),
        display_name: "zootree-fair-fox".into(),
        cmux_workspace: cmux_workspace.map(str::to_string),
    }
}

#[test]
fn launch_invokes_cmux_new_workspace() {
    let runner = MockRunner::new();
    runner.push_response(success_output(b"workspace:7\n"));
    let cmux = CmuxMultiplexer::new(&runner);

    assert_eq!(cmux.launch(&launch()).unwrap(), LaunchOutcome::Launched);

    let calls = runner.take_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].program, "cmux");
    assert_eq!(
        calls[0].args,
        vec![
            "new-workspace",
            "--name",
            "zootree-fair-fox",
            "--cwd",
            "/tmp/fair-fox",
            "--layout",
            r#"{"pane":{"surfaces":[{"type":"terminal"}]}}"#,
            "--focus",
            "true"
        ]
    );
    assert_eq!(calls[0].env, HashMap::new());
}

#[test]
fn close_uses_persisted_workspace_ref() {
    let runner = MockRunner::new();
    runner.push_response(success_output(b""));
    let cmux = CmuxMultiplexer::new(&runner);

    cmux.close(&identity(Some("workspace:7"))).unwrap();

    let calls = runner.take_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0].args,
        vec!["close-workspace", "--workspace", "workspace:7"]
    );
}

#[test]
fn open_selects_persisted_workspace_ref() {
    let runner = MockRunner::new();
    runner.push_response(success_output(b""));
    let cmux = CmuxMultiplexer::new(&runner);

    assert_eq!(
        cmux.open(&launch(), &identity(Some("workspace:7"))).unwrap(),
        LaunchOutcome::Attached
    );

    let calls = runner.take_calls();
    assert_eq!(
        calls[0].args,
        vec!["select-workspace", "--workspace", "workspace:7"]
    );
}
```

- [ ] **Step 2: Run cmux tests and verify failure**

Run:

```bash
cargo test --test cmux_test
```

Expected: compile failure because `multiplexer::cmux` does not exist.

- [ ] **Step 3: Expose cmux module**

In `src/core/multiplexer/mod.rs`, add:

```rust
pub mod cmux;
```

- [ ] **Step 4: Create `src/core/multiplexer/cmux.rs`**

Create:

```rust
use super::{LaunchOutcome, MultiplexerIdentity, MultiplexerLaunch, TerminalMultiplexer};
use crate::config::global::MultiplexerKind;
use crate::runner::{CommandRunner, CommandSpec};
use anyhow::{bail, Result};
use std::collections::HashMap;

pub struct CmuxMultiplexer<'a, R: CommandRunner> {
    runner: &'a R,
}

impl<'a, R: CommandRunner> CmuxMultiplexer<'a, R> {
    pub fn new(runner: &'a R) -> Self {
        Self { runner }
    }

    fn cmux(&self, args: Vec<String>) -> Result<std::process::Output> {
        self.runner.run(&CommandSpec {
            program: "cmux".into(),
            args,
            cwd: None,
            env: HashMap::new(),
            env_remove: vec![],
        })
    }

    fn ensure_success(output: std::process::Output, context: &str) -> Result<std::process::Output> {
        if !output.status.success() {
            bail!(
                "{} failed: {}",
                context,
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(output)
    }

    pub fn parse_workspace_ref(output: &std::process::Output) -> Option<String> {
        let stdout = String::from_utf8_lossy(&output.stdout);
        stdout
            .lines()
            .map(str::trim)
            .find(|line| line.starts_with("workspace:") || !line.is_empty())
            .map(str::to_string)
    }
}

impl<R: CommandRunner> TerminalMultiplexer for CmuxMultiplexer<'_, R> {
    fn kind(&self) -> MultiplexerKind {
        MultiplexerKind::Cmux
    }

    fn launch(&self, launch: &MultiplexerLaunch) -> Result<LaunchOutcome> {
        let output = self.cmux(vec![
            "new-workspace".into(),
            "--name".into(),
            launch.display_name.clone(),
            "--cwd".into(),
            launch.workspace_dir.to_string_lossy().into_owned(),
            "--layout".into(),
            launch.rendered_layout.clone(),
            "--focus".into(),
            "true".into(),
        ])?;
        Self::ensure_success(output, "cmux new-workspace")?;
        Ok(LaunchOutcome::Launched)
    }

    fn open(
        &self,
        launch: &MultiplexerLaunch,
        identity: &MultiplexerIdentity,
    ) -> Result<LaunchOutcome> {
        if let Some(workspace) = &identity.cmux_workspace {
            let output = self.cmux(vec![
                "select-workspace".into(),
                "--workspace".into(),
                workspace.clone(),
            ])?;
            Self::ensure_success(output, "cmux select-workspace")?;
            return Ok(LaunchOutcome::Attached);
        }
        self.launch(launch)
    }

    fn close(&self, identity: &MultiplexerIdentity) -> Result<()> {
        let Some(workspace) = &identity.cmux_workspace else {
            tracing::warn!(
                "cmux workspace id missing for '{}'; skipping cmux close",
                identity.display_name
            );
            return Ok(());
        };
        let output = self.cmux(vec![
            "close-workspace".into(),
            "--workspace".into(),
            workspace.clone(),
        ])?;
        Self::ensure_success(output, "cmux close-workspace")?;
        Ok(())
    }
}
```

This task intentionally skips name fallback. Task 8 adds workspace state persistence and fallback lookup once workspace wiring exists.

- [ ] **Step 5: Run cmux tests**

Run:

```bash
cargo test --test cmux_test
```

Expected: tests pass.

- [ ] **Step 6: Commit cmux operations**

```bash
git add src/core/multiplexer/mod.rs src/core/multiplexer/cmux.rs tests/cmux_test.rs
git commit -m "feat: add cmux multiplexer operations"
```

---

### Task 6: Wire Multiplexer Launch Into Workspace Start and Open

**Files:**
- Modify: `src/cli/workspace.rs`
- Modify: `src/cli/mod.rs`
- Modify: `src/core/mod.rs`
- Test: `tests/zellij_test.rs`
- Test: `tests/completions_test.rs`

- [ ] **Step 1: Write failing CLI argument test**

In the existing `#[cfg(test)] mod tests` in `src/cli/workspace.rs`, add:

```rust
#[derive(Parser)]
struct TestStartCli {
    #[command(flatten)]
    args: StartArgs,
}

#[test]
fn start_args_accept_no_multiplexer() {
    let cli = TestStartCli::parse_from(["test", "--no-multiplexer", "fair-fox"]);
    assert!(cli.args.no_multiplexer);
    assert_eq!(cli.args.name.as_deref(), Some("fair-fox"));
}

#[test]
fn start_args_reject_no_zellij() {
    let result = TestStartCli::try_parse_from(["test", "--no-zellij", "fair-fox"]);
    assert!(result.is_err());
}
```

- [ ] **Step 2: Run workspace tests and verify failure**

Run:

```bash
cargo test start_args_accept_no_multiplexer start_args_reject_no_zellij
```

Expected: compile failure or clap failure because `no_multiplexer` does not exist.

- [ ] **Step 3: Rename CLI field**

In `src/cli/workspace.rs`, change:

```rust
#[arg(long, help = "Skip launching zellij session after start")]
pub no_zellij: bool,
```

to:

```rust
#[arg(long, help = "Skip launching the configured terminal multiplexer after start")]
pub no_multiplexer: bool,
```

Replace all `args.no_zellij` with `args.no_multiplexer`.

- [ ] **Step 4: Update command descriptions**

In `src/cli/mod.rs`, change command descriptions:

```rust
#[command(about = "Start a pending workspace (create worktrees and launch terminal multiplexer)")]
Start(workspace::StartArgs),
#[command(about = "Open an in-progress workspace in terminal multiplexer")]
Open(workspace::OpenArgs),
```

- [ ] **Step 5: Replace workspace zellij imports**

In `src/cli/workspace.rs`, replace:

```rust
use crate::core::zellij::ZellijOps;
```

with:

```rust
use crate::config::global::{MultiplexerConfig, MultiplexerKind};
use crate::core::cmux_layout::{default_cmux_layout, render_cmux_layout, CmuxLayoutVar};
use crate::core::multiplexer::{
    cmux::CmuxMultiplexer,
    zellij::{is_inside_zellij, ZellijMultiplexer},
    MultiplexerIdentity, MultiplexerLaunch, TerminalMultiplexer,
};
```

If `GlobalConfig` is already imported separately, merge imports so `GlobalConfig`, `MultiplexerConfig`, and `MultiplexerKind` come from one use statement.

- [ ] **Step 6: Add selected config helpers in `workspace.rs`**

Add helper functions near `write_default_layout`:

```rust
fn selected_multiplexer_config(
    workspace: &WorkspaceConfig,
    _global: &GlobalConfig,
) -> MultiplexerConfig {
    let mut config = workspace.multiplexer.clone();
    if let Some(kind) = &workspace.multiplexer_state.kind {
        config.kind = kind.clone();
    }
    config
}

fn multiplexer_display_name(workspace: &WorkspaceConfig) -> String {
    format!("zootree-{}", workspace.name)
}

fn multiplexer_identity(workspace: &WorkspaceConfig) -> MultiplexerIdentity {
    MultiplexerIdentity {
        workspace_name: workspace.name.clone(),
        display_name: multiplexer_display_name(workspace),
        cmux_workspace: workspace.multiplexer_state.cmux_workspace.clone(),
    }
}
```

- [ ] **Step 7: Split layout rendering by kind**

Replace `launch_zellij` with `prepare_multiplexer_launch` and `launch_multiplexer`:

```rust
fn prepare_multiplexer_launch(
    config_mgr: &ConfigManager,
    global: &GlobalConfig,
    workspace: &WorkspaceConfig,
    run_agent: Option<Option<String>>,
) -> Result<MultiplexerLaunch> {
    let multiplexer = selected_multiplexer_config(workspace, global);
    match multiplexer.kind {
        MultiplexerKind::Zellij => prepare_zellij_launch(config_mgr, global, workspace, run_agent),
        MultiplexerKind::Cmux => prepare_cmux_launch(config_mgr, global, workspace, run_agent),
    }
}
```

Move the existing KDL rendering body from `launch_zellij` into:

```rust
fn prepare_zellij_launch(
    config_mgr: &ConfigManager,
    global: &GlobalConfig,
    workspace: &WorkspaceConfig,
    run_agent: Option<Option<String>>,
) -> Result<MultiplexerLaunch> {
    let multiplexer = selected_multiplexer_config(workspace, global);
    let layout_name = multiplexer.zellij.layout.as_deref().unwrap_or("default");

    let template_content = if layout_name == "default" {
        write_default_layout(&config_mgr.base_dir)
    } else {
        let layout_path = config_mgr
            .base_dir
            .join("layouts")
            .join(format!("{}.kdl", layout_name));
        if layout_path.exists() {
            std::fs::read_to_string(&layout_path)?
        } else {
            anyhow::bail!("zellij layout '{}' not found at {}", layout_name, layout_path.display());
        }
    };

    let ws_dir = shellexpand::tilde(&workspace.workspace_dir).into_owned();
    let agent_cli_tpl = resolve_run_agent_template(global, run_agent.as_ref())?;
    let (overview_kdl, repo_kdl_for_first) = build_zellij_agent_fragments(workspace, agent_cli_tpl.as_deref())?;

    let mut vars = Vec::new();
    for (i, repo_entry) in workspace.repos.iter().enumerate() {
        let repo_config = config_mgr.load_repo_config(&repo_entry.name)?;
        let lazygit_config = repo_config.lazygit.map(|lg| lg.config).unwrap_or_default();
        vars.push(LayoutVar {
            repo_name: repo_entry.name.clone(),
            worktree_path: format!("{}/{}", ws_dir, repo_entry.name),
            branch: workspace.branch.clone(),
            workspace_name: workspace.name.clone(),
            workspace_dir: ws_dir.clone(),
            lazygit_config,
            overview_agent_cli: overview_kdl.clone(),
            repo_agent_cli: if i == 0 { repo_kdl_for_first.clone() } else { String::new() },
        });
    }

    let rendered = LayoutRenderer::render(&template_content, &vars);
    let layout_dir = config_mgr.base_dir.join("layouts");
    std::fs::create_dir_all(&layout_dir)?;
    let layout_file = layout_dir.join("recently.kdl");
    std::fs::write(&layout_file, &rendered)?;

    Ok(MultiplexerLaunch {
        workspace_name: workspace.name.clone(),
        display_name: multiplexer_display_name(workspace),
        workspace_dir: ws_dir.into(),
        layout_name: layout_name.into(),
        rendered_layout: rendered,
        layout_file,
    })
}
```

Add helper functions used above:

```rust
fn resolve_run_agent_template(
    global: &GlobalConfig,
    run_agent: Option<&Option<String>>,
) -> Result<Option<String>> {
    match run_agent {
        None => Ok(None),
        Some(value) => {
            let raw = match value.as_deref() {
                Some(s) if !s.is_empty() => s.to_string(),
                _ => global.agent_cli.clone().ok_or_else(|| {
                    anyhow::anyhow!(
                        "--run-agent requires agent_cli in global config (~/.config/zootree/config.toml)"
                    )
                })?,
            };
            Ok(Some(crate::core::layout::resolve_agent_cli(&raw, &global.agent_cli_alias).to_string()))
        }
    }
}

fn build_zellij_agent_fragments(
    workspace: &WorkspaceConfig,
    agent_cli_tpl: Option<&str>,
) -> Result<(String, String)> {
    match agent_cli_tpl {
        None => Ok((String::new(), String::new())),
        Some(tpl) => {
            let prompt = crate::core::layout::build_prompt(workspace);
            let kdl = crate::core::layout::build_agent_cli_kdl(tpl, &prompt)?;
            if workspace.repos.len() == 1 {
                Ok((String::new(), kdl))
            } else {
                Ok((kdl, String::new()))
            }
        }
    }
}
```

- [ ] **Step 8: Add cmux launch preparation**

Add:

```rust
fn write_default_cmux_layout(base_dir: &Path) -> String {
    let content = default_cmux_layout().to_string();
    let path = base_dir.join("layouts").join("default.cmux.json");
    let _ = std::fs::create_dir_all(path.parent().unwrap());
    let _ = std::fs::write(&path, &content);
    content
}

fn prepare_cmux_launch(
    config_mgr: &ConfigManager,
    global: &GlobalConfig,
    workspace: &WorkspaceConfig,
    run_agent: Option<Option<String>>,
) -> Result<MultiplexerLaunch> {
    let multiplexer = selected_multiplexer_config(workspace, global);
    let layout_name = multiplexer.cmux.layout.as_deref().unwrap_or("default");
    let template_content = if layout_name == "default" {
        write_default_cmux_layout(&config_mgr.base_dir)
    } else {
        let layout_path = config_mgr
            .base_dir
            .join("layouts")
            .join(format!("{}.cmux.json", layout_name));
        if layout_path.exists() {
            std::fs::read_to_string(&layout_path)?
        } else {
            anyhow::bail!("cmux layout '{}' not found at {}", layout_name, layout_path.display());
        }
    };

    let ws_dir = shellexpand::tilde(&workspace.workspace_dir).into_owned();
    let agent_cli_tpl = resolve_run_agent_template(global, run_agent.as_ref())?;
    let prompt = crate::core::layout::build_prompt(workspace);
    let agent_command = match agent_cli_tpl.as_deref() {
        Some(tpl) => crate::core::layout::build_agent_cli_command(tpl, &prompt)?,
        None => String::new(),
    };

    let mut vars = Vec::new();
    for (i, repo_entry) in workspace.repos.iter().enumerate() {
        let repo_config = config_mgr.load_repo_config(&repo_entry.name)?;
        let lazygit_config = repo_config.lazygit.map(|lg| lg.config).unwrap_or_default();
        let single_repo = workspace.repos.len() == 1;
        vars.push(CmuxLayoutVar {
            repo_name: repo_entry.name.clone(),
            worktree_path: format!("{}/{}", ws_dir, repo_entry.name),
            branch: workspace.branch.clone(),
            workspace_name: workspace.name.clone(),
            workspace_dir: ws_dir.clone(),
            lazygit_config,
            overview_agent_command: if !single_repo { agent_command.clone() } else { String::new() },
            repo_agent_command: if single_repo && i == 0 { agent_command.clone() } else { String::new() },
        });
    }

    let rendered = render_cmux_layout(&template_content, &vars)?;
    let layout_dir = config_mgr.base_dir.join("layouts");
    std::fs::create_dir_all(&layout_dir)?;
    let layout_file = layout_dir.join("recently.cmux.json");
    std::fs::write(&layout_file, &rendered)?;

    Ok(MultiplexerLaunch {
        workspace_name: workspace.name.clone(),
        display_name: multiplexer_display_name(workspace),
        workspace_dir: ws_dir.into(),
        layout_name: layout_name.into(),
        rendered_layout: rendered,
        layout_file,
    })
}
```

- [ ] **Step 9: Add launch/open dispatcher**

Add:

```rust
fn launch_multiplexer(
    config_mgr: &ConfigManager,
    global: &GlobalConfig,
    workspace: &WorkspaceConfig,
    runner: &RealRunner,
    run_agent: Option<Option<String>>,
) -> Result<()> {
    let config = selected_multiplexer_config(workspace, global);
    let launch = prepare_multiplexer_launch(config_mgr, global, workspace, run_agent)?;
    let identity = multiplexer_identity(workspace);
    match config.kind {
        MultiplexerKind::Zellij => {
            let zellij = ZellijMultiplexer::new(runner, is_inside_zellij());
            zellij.launch(&launch)?;
        }
        MultiplexerKind::Cmux => {
            let cmux = CmuxMultiplexer::new(runner);
            cmux.open(&launch, &identity)?;
        }
    }
    Ok(())
}
```

- [ ] **Step 10: Replace start/open call sites**

In `handle_start`, replace:

```rust
if !args.no_zellij {
    launch_zellij(
        &config_mgr,
        &global,
        &workspace,
        &runner,
        args.run_agent.clone(),
        crate::core::zellij::is_inside_zellij(),
    )?;
}
```

with:

```rust
if !args.no_multiplexer {
    launch_multiplexer(&config_mgr, &global, &workspace, &runner, args.run_agent.clone())?;
}
```

In `handle_open`, replace the old `launch_zellij` call with:

```rust
launch_multiplexer(&config_mgr, &global, &workspace, &runner, None)?;
```

- [ ] **Step 11: Remove old zellij module export after workspace migration**

After `src/cli/workspace.rs` no longer imports `crate::core::zellij`, remove from `src/core/mod.rs`:

```rust
pub mod zellij;
```

Keep `src/core/zellij.rs` unreferenced for this task only if deletion causes noisy diff. If unreferenced and tests pass, delete it in Task 9 cleanup.

- [ ] **Step 12: Run targeted tests**

Run:

```bash
cargo test start_args_accept_no_multiplexer start_args_reject_no_zellij
cargo test --test zellij_test
cargo check --tests
```

Expected: tests pass; remaining failures, if any, should be direct references to deleted zellij fields in docs tests or fixtures.

- [ ] **Step 13: Commit workspace launch wiring**

```bash
git add src/cli/workspace.rs src/cli/mod.rs src/core/mod.rs tests/zellij_test.rs tests/completions_test.rs
git commit -m "feat: wire workspace launch through multiplexer"
```

---

### Task 7: Close Multiplexer on Done and Cancel

**Files:**
- Modify: `src/cli/workspace.rs`
- Test: existing workspace tests where practical

- [ ] **Step 1: Add close helper**

In `src/cli/workspace.rs`, add near `launch_multiplexer`:

```rust
fn close_multiplexer(
    global: &GlobalConfig,
    workspace: &WorkspaceConfig,
    runner: &RealRunner,
) -> Result<()> {
    let config = selected_multiplexer_config(workspace, global);
    let identity = multiplexer_identity(workspace);
    match config.kind {
        MultiplexerKind::Zellij => {
            let zellij = ZellijMultiplexer::new(runner, is_inside_zellij());
            zellij.close(&identity)?;
        }
        MultiplexerKind::Cmux => {
            let cmux = CmuxMultiplexer::new(runner);
            cmux.close(&identity)?;
        }
    }
    Ok(())
}
```

- [ ] **Step 2: Replace done zellij cleanup**

In `handle_done`, remove:

```rust
let zellij = ZellijOps::new(&runner);
```

Remove the session name match block and replace it with:

```rust
if let Err(e) = close_multiplexer(&global, &workspace, &runner) {
    tracing::warn!(
        "failed to close terminal multiplexer for workspace '{}': {}",
        workspace.name,
        e
    );
}
```

- [ ] **Step 3: Replace cancel zellij cleanup**

In `handle_cancel`, remove:

```rust
let zellij = ZellijOps::new(&runner);
```

Replace the zellij session cleanup block with the same `close_multiplexer` call:

```rust
if let Err(e) = close_multiplexer(&global, &workspace, &runner) {
    tracing::warn!(
        "failed to close terminal multiplexer for workspace '{}': {}",
        workspace.name,
        e
    );
}
```

- [ ] **Step 4: Run compile and zellij tests**

Run:

```bash
cargo check --tests
cargo test --test zellij_test
```

Expected: no references to `ZellijOps` remain in `src/cli/workspace.rs`.

- [ ] **Step 5: Commit close wiring**

```bash
git add src/cli/workspace.rs
git commit -m "refactor: close workspace multiplexer generically"
```

---

### Task 8: Persist Cmux Workspace Identity and Add Name Fallback

**Files:**
- Modify: `src/core/multiplexer/cmux.rs`
- Modify: `src/cli/workspace.rs`
- Test: `tests/cmux_test.rs`

- [ ] **Step 1: Add failing cmux fallback tests**

Append to `tests/cmux_test.rs`:

```rust
#[test]
fn close_without_id_skips_when_lookup_has_no_match() {
    let runner = MockRunner::new();
    runner.push_response(success_output(b"workspace:1 other\n"));
    let cmux = CmuxMultiplexer::new(&runner);

    cmux.close(&identity(None)).unwrap();

    let calls = runner.take_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].args, vec!["list-workspaces"]);
}

#[test]
fn close_without_id_closes_unique_name_match() {
    let runner = MockRunner::new();
    runner.push_response(success_output(b"workspace:4 zootree-fair-fox\n"));
    runner.push_response(success_output(b""));
    let cmux = CmuxMultiplexer::new(&runner);

    cmux.close(&identity(None)).unwrap();

    let calls = runner.take_calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].args, vec!["list-workspaces"]);
    assert_eq!(
        calls[1].args,
        vec!["close-workspace", "--workspace", "workspace:4"]
    );
}
```

- [ ] **Step 2: Run cmux tests and verify failure**

Run:

```bash
cargo test --test cmux_test close_without_id_skips_when_lookup_has_no_match close_without_id_closes_unique_name_match
```

Expected: second test fails because close currently skips without id.

- [ ] **Step 3: Implement name lookup in cmux multiplexer**

Add to `src/core/multiplexer/cmux.rs`:

```rust
fn parse_unique_workspace_match(stdout: &[u8], display_name: &str) -> Option<String> {
    let stdout = String::from_utf8_lossy(stdout);
    let matches = stdout
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let handle = parts.next()?;
            let rest = parts.collect::<Vec<_>>().join(" ");
            if rest == display_name || line.trim_end().ends_with(display_name) {
                Some(handle.to_string())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    if matches.len() == 1 {
        Some(matches[0].clone())
    } else {
        None
    }
}

fn find_workspace_by_name(&self, display_name: &str) -> Result<Option<String>> {
    let output = self.cmux(vec!["list-workspaces".into()])?;
    let output = Self::ensure_success(output, "cmux list-workspaces")?;
    Ok(parse_unique_workspace_match(&output.stdout, display_name))
}
```

Replace the `None` close branch with:

```rust
let workspace = match &identity.cmux_workspace {
    Some(workspace) => workspace.clone(),
    None => match self.find_workspace_by_name(&identity.display_name)? {
        Some(workspace) => workspace,
        None => {
            tracing::warn!(
                "cmux workspace '{}' not found uniquely; skipping cmux close",
                identity.display_name
            );
            return Ok(());
        }
    },
};
```

Then close using `workspace`.

- [ ] **Step 4: Persist cmux identity after launch/open**

In `src/core/multiplexer/cmux.rs`, add public launch function:

```rust
pub fn launch_and_capture_workspace(&self, launch: &MultiplexerLaunch) -> Result<Option<String>> {
    let output = self.cmux(vec![
        "new-workspace".into(),
        "--name".into(),
        launch.display_name.clone(),
        "--cwd".into(),
        launch.workspace_dir.to_string_lossy().into_owned(),
        "--layout".into(),
        launch.rendered_layout.clone(),
        "--focus".into(),
        "true".into(),
    ])?;
    let output = Self::ensure_success(output, "cmux new-workspace")?;
    Ok(Self::parse_workspace_ref(&output))
}
```

Change `launch` to:

```rust
self.launch_and_capture_workspace(launch)?;
Ok(LaunchOutcome::Launched)
```

In `src/cli/workspace.rs`, after cmux launch in `launch_multiplexer`, capture the workspace ref:

```rust
let cmux = CmuxMultiplexer::new(runner);
let cmux_workspace = cmux.launch_and_capture_workspace(&launch)?;
if let Some(cmux_workspace) = cmux_workspace {
    let mut updated = workspace.clone();
    updated.multiplexer_state.kind = Some(MultiplexerKind::Cmux);
    updated.multiplexer_state.cmux_workspace = Some(cmux_workspace);
    config_mgr.save_workspace(&WorkspaceStatus::InProgress, &updated)?;
}
```

Use this block only for `MultiplexerKind::Cmux`. Keep zellij state unset.

- [ ] **Step 5: Run cmux tests and compile**

Run:

```bash
cargo test --test cmux_test
cargo check --tests
```

Expected: cmux tests pass and workspace compile succeeds.

- [ ] **Step 6: Commit cmux identity persistence**

```bash
git add src/core/multiplexer/cmux.rs src/cli/workspace.rs tests/cmux_test.rs
git commit -m "feat: persist cmux workspace identity"
```

---

### Task 9: Remove Old Zellij Symbols and Update CLI Tests

**Files:**
- Delete: `src/core/zellij.rs`
- Modify: `src/core/mod.rs`
- Modify: all remaining source and test files with `zellij` config names

- [ ] **Step 1: Search stale symbols**

Run:

```bash
rg -n "ZellijConfig|\\.zellij|no_zellij|launch_zellij|dispatch_launch|core::zellij|src/core/zellij" src tests
```

Expected: matches exist before cleanup.

- [ ] **Step 2: Delete stale zellij module**

Delete `src/core/zellij.rs` and ensure `src/core/mod.rs` does not contain:

```rust
pub mod zellij;
```

- [ ] **Step 3: Update remaining source references**

For each stale config field:

```rust
workspace.zellij
global.zellij
repo_config.zellij
template.zellij
```

replace with:

```rust
workspace.multiplexer
global.multiplexer
repo_config.multiplexer
template.multiplexer
```

For stale `no_zellij`, replace with `no_multiplexer`.

- [ ] **Step 4: Update completions and create flow tests**

In `tests/completions_test.rs`, replace `ZellijConfig` fixture construction with:

```rust
MultiplexerConfig {
    kind: MultiplexerKind::Zellij,
    zellij: Default::default(),
    cmux: Default::default(),
}
```

In `tests/create_flow_test.rs`, replace assertions:

```rust
assert_eq!(workspace.zellij.session_mode.as_deref(), Some("standalone"));
assert!(config.zellij.is_none());
```

with:

```rust
assert_eq!(workspace.multiplexer.kind, MultiplexerKind::Zellij);
assert!(config.multiplexer.is_none());
```

- [ ] **Step 5: Run stale-symbol check**

Run:

```bash
rg -n "ZellijConfig|\\.zellij|no_zellij|launch_zellij|dispatch_launch|core::zellij" src tests
```

Expected: no output.

- [ ] **Step 6: Run full test compile**

Run:

```bash
cargo check --tests
```

Expected: no compile errors.

- [ ] **Step 7: Commit cleanup**

```bash
git add -A src tests
git commit -m "refactor: remove zellij-only config symbols"
```

---

### Task 10: Documentation and Skill Updates

**Files:**
- Modify: `README.md`
- Modify: `README.zh-CN.md`
- Modify: `skills/zootree-dev/SKILL.md`
- Modify: `skills/zootree-usage/SKILL.md`

- [ ] **Step 1: Update README CLI snippets**

In `README.md`, replace:

```text
--no-zellij                        # Don't launch Zellij
```

with:

```text
--no-multiplexer                   # Don't launch the configured terminal multiplexer
```

In `README.zh-CN.md`, replace:

```text
--no-zellij                        # 不启动 Zellij
```

with:

```text
--no-multiplexer                   # 不启动已配置的终端复用器
```

- [ ] **Step 2: Update README config table and examples**

In both READMEs, replace layout table rows with:

```markdown
| `layouts/<name>.kdl` | Custom zellij layouts referenced from `[multiplexer.zellij].layout` |
| `layouts/<name>.cmux.json` | Custom cmux JSON layouts referenced from `[multiplexer.cmux].layout` |
```

Chinese version:

```markdown
| `layouts/<name>.kdl` | 自定义 zellij 布局，供 `[multiplexer.zellij].layout` 引用 |
| `layouts/<name>.cmux.json` | 自定义 cmux JSON 布局，供 `[multiplexer.cmux].layout` 引用 |
```

Replace config example:

```toml
[multiplexer]
kind = "zellij"

[multiplexer.zellij]
layout = "default"

[multiplexer.cmux]
layout = "default"
```

- [ ] **Step 3: Update dependency wording**

In both READMEs, replace dependency list:

```markdown
- Zellij
```

with:

```markdown
- Zellij or cmux
```

Chinese:

```markdown
- Zellij 或 cmux
```

- [ ] **Step 4: Update zootree-dev skill**

In `skills/zootree-dev/SKILL.md`, update architecture tree:

```text
│   ├── multiplexer/
│   │   ├── mod.rs      # TerminalMultiplexer trait + shared launch types
│   │   ├── zellij.rs   # zellij standalone session implementation
│   │   └── cmux.rs     # cmux workspace implementation
│   ├── cmux_layout.rs  # cmux JSON layout renderer
```

Replace the zellij config convention with:

```markdown
- **multiplexer 分组**: 所有终端复用器配置统一在 `MultiplexerConfig` 中（`src/config/global.rs`），字段用 `#[serde(default)]` 嵌入各配置 struct；默认 `kind = "zellij"`，cmux 使用 `layouts/<name>.cmux.json`。
```

Update testing convention:

```markdown
所有涉及 git、zellij、cmux 或 shell 的操作使用 `MockRunner`。
```

- [ ] **Step 5: Update zootree-usage skill**

In `skills/zootree-usage/SKILL.md`, replace `--no-zellij` examples with:

```bash
zootree start --no-multiplexer
```

Replace `[zellij]` config example with:

```toml
[multiplexer]
kind = "zellij"

[multiplexer.zellij]
layout = "default"

[multiplexer.cmux]
layout = "default"
```

Replace troubleshooting text:

```markdown
- **终端复用器未启动**: 确认已配置的 `zellij` 或 `cmux` 在 PATH 中，或使用 `--no-multiplexer` 跳过。
```

- [ ] **Step 6: Search docs for stale zellij-only text**

Run:

```bash
rg -n "\\[zellij\\]|--no-zellij|Zellij-only|zellij 布局|zellij layouts" README.md README.zh-CN.md skills/zootree-dev/SKILL.md skills/zootree-usage/SKILL.md
```

Expected: no stale config or flag references. Mentions of zellij as one supported multiplexer are allowed.

- [ ] **Step 7: Commit docs**

```bash
git add README.md README.zh-CN.md skills/zootree-dev/SKILL.md skills/zootree-usage/SKILL.md
git commit -m "docs: document configurable terminal multiplexer"
```

---

### Task 11: Final Verification

**Files:**
- All changed files

- [ ] **Step 1: Run formatting**

Run:

```bash
cargo fmt
```

Expected: command exits 0.

- [ ] **Step 2: Run full tests**

Run:

```bash
cargo test
```

Expected: all tests pass.

- [ ] **Step 3: Run stale symbol checks**

Run:

```bash
rg -n "ZellijConfig|\\.zellij|no_zellij|launch_zellij|dispatch_launch|core::zellij" src tests
```

Expected: no output.

Run:

```bash
rg -n "\\[zellij\\]|--no-zellij" README.md README.zh-CN.md skills/zootree-dev/SKILL.md skills/zootree-usage/SKILL.md
```

Expected: no output.

- [ ] **Step 4: Inspect final diff**

Run:

```bash
git status --short
git log --oneline -8
```

Expected: working tree clean after task commits; recent commits match task boundaries.

- [ ] **Step 5: Record verification**

If all verification passes, note these commands in the final handoff:

```text
cargo fmt
cargo test
rg -n "ZellijConfig|\\.zellij|no_zellij|launch_zellij|dispatch_launch|core::zellij" src tests
rg -n "\\[zellij\\]|--no-zellij" README.md README.zh-CN.md skills/zootree-dev/SKILL.md skills/zootree-usage/SKILL.md
```
