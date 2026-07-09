# cmux group workspaces Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make cmux-backed zootree workspaces launch as one cmux workspace group containing an anchor workspace plus one workspace per repo.

**Architecture:** Keep zellij on the current `TerminalMultiplexer` path. Add a cmux-specific group launch path in `src/core/multiplexer/cmux.rs`, driven by explicit launch/state structs and called from `src/cli/workspace.rs`. Split cmux default layout rendering into anchor and repo layouts while keeping non-default cmux layouts unsupported until a group-aware multi-template config exists.

**Tech Stack:** Rust, anyhow, serde/toml, serde_json, shlex, clap, zootree `CommandRunner`/`MockRunner`, cmux CLI.

---

## File Structure

- Modify `src/config/workspace.rs`
  - Owns persisted workspace schema.
  - Add group-aware cmux state and keep old `cmux_workspace` as read-compatible state.

- Modify `src/core/cmux_layout.rs`
  - Owns cmux JSON layout rendering.
  - Add separate default anchor and repo layout templates and render helpers.

- Modify `src/core/multiplexer/mod.rs`
  - Owns shared multiplexer launch identity structs.
  - Add cmux group launch/state structs used by CLI and cmux implementation.

- Modify `src/core/multiplexer/cmux.rs`
  - Owns all cmux CLI command sequencing.
  - Add group creation, focus, title lookup, and group delete helpers.

- Modify `src/cli/workspace.rs`
  - Owns workspace lifecycle orchestration.
  - Prepare cmux group launches, persist captured cmux state after launch/open recreation, and route close through group-aware delete.

- Modify `tests/config_test.rs`
  - Covers TOML parse/serialize for new cmux state and old state compatibility.

- Modify `tests/cmux_layout_test.rs`
  - Covers anchor/repo layout behavior and non-default layout unsupported error helper.

- Modify `tests/cmux_test.rs`
  - Covers cmux command sequencing, captured state, focus, lookup, and delete.

- Modify documentation:
  - `README.md`
  - `README.zh-CN.md`
  - `skills/zootree-usage/SKILL.md`
  - `skills/zootree-dev/SKILL.md`

---

### Task 1: Persist group-aware cmux state

**Files:**
- Modify: `src/config/workspace.rs`
- Test: `tests/config_test.rs`

- [ ] **Step 1: Add failing config parse test**

Append this test to `tests/config_test.rs` near `parse_workspace_config_with_multiplexer_state`:

```rust
#[test]
fn parse_workspace_config_with_cmux_group_state() {
    let toml_str = r#"
title = "用户认证功能"
name = "calm-river"
description = "前后端联调 OAuth2 登录"
branch = "zootree/calm-river"
workspace_dir = "~/zootree-workspaces/calm-river"
created_at = "2026-04-28T10:30:00+08:00"

[multiplexer]
kind = "cmux"

[multiplexer_state]
kind = "cmux"
cmux_group = "workspace_group:2"
cmux_anchor_workspace = "workspace:4"

[[multiplexer_state.cmux_repo_workspaces]]
repo = "frontend"
workspace = "workspace:5"

[[multiplexer_state.cmux_repo_workspaces]]
repo = "backend"
workspace = "workspace:6"
"#;

    let config: WorkspaceConfig = toml::from_str(toml_str).unwrap();

    assert_eq!(config.multiplexer_state.kind, Some(MultiplexerKind::Cmux));
    assert_eq!(
        config.multiplexer_state.cmux_group.as_deref(),
        Some("workspace_group:2")
    );
    assert_eq!(
        config.multiplexer_state.cmux_anchor_workspace.as_deref(),
        Some("workspace:4")
    );
    assert_eq!(config.multiplexer_state.cmux_repo_workspaces.len(), 2);
    assert_eq!(config.multiplexer_state.cmux_repo_workspaces[0].repo, "frontend");
    assert_eq!(
        config.multiplexer_state.cmux_repo_workspaces[0].workspace,
        "workspace:5"
    );
    assert_eq!(config.multiplexer_state.cmux_repo_workspaces[1].repo, "backend");
    assert_eq!(
        config.multiplexer_state.cmux_repo_workspaces[1].workspace,
        "workspace:6"
    );
}
```

- [ ] **Step 2: Add failing serialization test**

Append this test to `tests/config_test.rs` near `multiplexer_state_with_cmux_workspace_is_serialized`:

```rust
#[test]
fn group_aware_multiplexer_state_is_serialized_without_legacy_workspace_ref() {
    let config = WorkspaceConfig {
        title: "Group cmux".into(),
        name: "calm-river".into(),
        description: String::new(),
        branch: "zootree/calm-river".into(),
        workspace_dir: "~/zootree-workspaces/calm-river".into(),
        created_at: "2026-04-28T10:30:00+08:00".into(),
        agent_cli: None,
        multiplexer: MultiplexerConfig::default(),
        multiplexer_state: MultiplexerState {
            kind: Some(MultiplexerKind::Cmux),
            cmux_workspace: None,
            cmux_group: Some("workspace_group:2".into()),
            cmux_anchor_workspace: Some("workspace:4".into()),
            cmux_repo_workspaces: vec![
                CmuxRepoWorkspaceState {
                    repo: "frontend".into(),
                    workspace: "workspace:5".into(),
                },
                CmuxRepoWorkspaceState {
                    repo: "backend".into(),
                    workspace: "workspace:6".into(),
                },
            ],
        },
        repos: Vec::new(),
        events: Vec::new(),
    };

    let serialized = toml::to_string(&config).unwrap();

    assert!(serialized.contains("cmux_group = \"workspace_group:2\""));
    assert!(serialized.contains("cmux_anchor_workspace = \"workspace:4\""));
    assert!(serialized.contains("[[multiplexer_state.cmux_repo_workspaces]]"));
    assert!(serialized.contains("repo = \"frontend\""));
    assert!(serialized.contains("workspace = \"workspace:5\""));
    assert!(
        !serialized.contains("cmux_workspace"),
        "new group-aware state should not write legacy cmux_workspace: {serialized}"
    );

    let round_tripped: WorkspaceConfig = toml::from_str(&serialized).unwrap();
    assert_eq!(
        round_tripped.multiplexer_state.cmux_group.as_deref(),
        Some("workspace_group:2")
    );
    assert_eq!(round_tripped.multiplexer_state.cmux_repo_workspaces.len(), 2);
}
```

Update the existing `empty_multiplexer_state_is_not_serialized` and `multiplexer_state_with_cmux_workspace_is_serialized` tests after the implementation if their struct literals need the new fields.

- [ ] **Step 3: Run config tests to verify failure**

Run:

```bash
cargo test --test config_test parse_workspace_config_with_cmux_group_state group_aware_multiplexer_state_is_serialized_without_legacy_workspace_ref
```

Expected: compile fails because `CmuxRepoWorkspaceState`, `cmux_group`, `cmux_anchor_workspace`, and `cmux_repo_workspaces` do not exist.

- [ ] **Step 4: Implement group-aware state types**

In `src/config/workspace.rs`, replace the current `MultiplexerState` definition with this code and add the new repo state struct above it:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CmuxRepoWorkspaceState {
    pub repo: String,
    pub workspace: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MultiplexerState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<MultiplexerKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cmux_workspace: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cmux_group: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cmux_anchor_workspace: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cmux_repo_workspaces: Vec<CmuxRepoWorkspaceState>,
}
```

Then replace `MultiplexerState::is_empty` with:

```rust
impl MultiplexerState {
    pub fn is_empty(&self) -> bool {
        self.kind.is_none()
            && self.cmux_workspace.is_none()
            && self.cmux_group.is_none()
            && self.cmux_anchor_workspace.is_none()
            && self.cmux_repo_workspaces.is_empty()
    }

    pub fn has_cmux_group_state(&self) -> bool {
        self.cmux_group.is_some() || self.cmux_anchor_workspace.is_some()
    }
}
```

- [ ] **Step 5: Update test imports**

At the top of `tests/config_test.rs`, update the workspace import to include the new state type:

```rust
use zootree::config::workspace::{
    CmuxRepoWorkspaceState, MultiplexerState, RepoEntry, WorkspaceConfig, WorkspaceStatus,
};
```

Keep any existing imported names from this list that are already present.

- [ ] **Step 6: Run config tests**

Run:

```bash
cargo test --test config_test
```

Expected: all config tests pass.

- [ ] **Step 7: Commit**

```bash
git add src/config/workspace.rs tests/config_test.rs
git commit -m "feat: persist cmux group state"
```

---

### Task 2: Split cmux default layouts into anchor and repo renderers

**Files:**
- Modify: `src/core/cmux_layout.rs`
- Test: `tests/cmux_layout_test.rs`

- [ ] **Step 1: Add failing anchor and repo layout tests**

Append these tests to `tests/cmux_layout_test.rs`:

```rust
#[test]
fn anchor_layout_runs_info_and_multi_repo_agent() {
    let rendered = render_cmux_anchor_layout(
        default_cmux_anchor_layout(),
        &vars(),
        Some("codex --prompt 'Fix login'"),
    )
    .unwrap();
    let value: Value = serde_json::from_str(&rendered).unwrap();
    let commands = collect_string_field(&value, "command");
    let cwds = collect_string_field(&value, "cwd");

    assert!(commands.contains(&"zootree info fair-fox --watch".to_string()));
    assert!(commands.contains(&"codex --prompt 'Fix login'".to_string()));
    assert!(cwds.contains(&"/tmp/fair-fox".to_string()));
    assert_no_empty_command(&value);
    assert_no_unresolved_vars(&value);
    assert_valid_cmux_split_tree(&value);
}

#[test]
fn anchor_layout_without_multi_repo_agent_uses_shell_on_right() {
    let rendered = render_cmux_anchor_layout(default_cmux_anchor_layout(), &vars(), None).unwrap();
    let value: Value = serde_json::from_str(&rendered).unwrap();
    let commands = collect_string_field(&value, "command");

    assert!(commands.contains(&"zootree info fair-fox --watch".to_string()));
    assert!(!commands.iter().any(|command| command.contains("codex")));
    assert_no_empty_command(&value);
}

#[test]
fn repo_layout_runs_lazygit_and_single_repo_agent() {
    let repo = vars().remove(0);
    let rendered = render_cmux_repo_layout(
        default_cmux_repo_layout(),
        &repo,
        Some("codex --prompt 'Fix login'"),
    )
    .unwrap();
    let value: Value = serde_json::from_str(&rendered).unwrap();
    let commands = collect_string_field(&value, "command");
    let cwds = collect_string_field(&value, "cwd");

    assert!(commands.contains(&"lazygit -p /tmp/fair-fox/api".to_string()));
    assert!(commands.contains(&"codex --prompt 'Fix login'".to_string()));
    assert!(cwds.iter().any(|cwd| cwd == "/tmp/fair-fox/api"));
    assert_no_empty_command(&value);
    assert_no_unresolved_vars(&value);
    assert_valid_cmux_split_tree(&value);
}

#[test]
fn repo_layout_without_agent_keeps_shell_bottom() {
    let repo = vars().remove(0);
    let rendered = render_cmux_repo_layout(default_cmux_repo_layout(), &repo, None).unwrap();
    let value: Value = serde_json::from_str(&rendered).unwrap();
    let commands = collect_string_field(&value, "command");

    assert!(commands.contains(&"lazygit -p /tmp/fair-fox/api".to_string()));
    assert!(!commands.iter().any(|command| command.contains("codex")));
    assert_no_empty_command(&value);
}
```

- [ ] **Step 2: Add failing lazygit config test**

Append this test to `tests/cmux_layout_test.rs`:

```rust
#[test]
fn repo_layout_passes_lazygit_config_when_present() {
    let mut repo = vars().remove(0);
    repo.lazygit_config = "/tmp/lazygit.yml".into();

    let rendered = render_cmux_repo_layout(default_cmux_repo_layout(), &repo, None).unwrap();
    let value: Value = serde_json::from_str(&rendered).unwrap();
    let commands = collect_string_field(&value, "command");

    assert!(commands.contains(
        &"lazygit -p /tmp/fair-fox/api -ucf /tmp/lazygit.yml".to_string()
    ));
}
```

- [ ] **Step 3: Run cmux layout tests to verify failure**

Run:

```bash
cargo test --test cmux_layout_test anchor_layout_runs_info_and_multi_repo_agent anchor_layout_without_multi_repo_agent_uses_shell_on_right repo_layout_runs_lazygit_and_single_repo_agent repo_layout_without_agent_keeps_shell_bottom repo_layout_passes_lazygit_config_when_present
```

Expected: compile fails because `default_cmux_anchor_layout`, `default_cmux_repo_layout`, `render_cmux_anchor_layout`, and `render_cmux_repo_layout` do not exist.

- [ ] **Step 4: Implement anchor and repo layout templates**

In `src/core/cmux_layout.rs`, add these functions after `default_cmux_layout()`:

```rust
pub fn default_cmux_anchor_layout() -> &'static str {
    r#"{
  "direction": "horizontal",
  "split": 0.5,
  "children": [
    {
      "pane": {
        "surfaces": [
          {
            "type": "terminal",
            "name": "info",
            "command": "zootree info $workspace_name --watch",
            "cwd": "$workspace_dir",
            "focus": true
          }
        ]
      }
    },
    {
      "pane": {
        "surfaces": [
          {
            "type": "terminal",
            "name": "agent",
            "command": "$agent_command",
            "cwd": "$workspace_dir"
          },
          {
            "type": "terminal",
            "name": "shell",
            "cwd": "$workspace_dir"
          }
        ]
      }
    }
  ]
}"#
}

pub fn default_cmux_repo_layout() -> &'static str {
    r#"{
  "direction": "horizontal",
  "split": 0.38,
  "children": [
    {
      "pane": {
        "surfaces": [
          {
            "type": "terminal",
            "name": "lazygit",
            "command": "$lazygit_command",
            "cwd": "$worktree_path",
            "focus": true
          }
        ]
      }
    },
    {
      "direction": "vertical",
      "split": 0.5,
      "children": [
        {
          "pane": {
            "surfaces": [
              {
                "type": "terminal",
                "name": "shell",
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
                "name": "agent",
                "command": "$agent_command",
                "cwd": "$worktree_path"
              },
              {
                "type": "terminal",
                "name": "shell",
                "cwd": "$worktree_path"
              }
            ]
          }
        }
      ]
    }
  ]
}"#
}
```

- [ ] **Step 5: Implement render helpers**

In `src/core/cmux_layout.rs`, add these functions after `render_cmux_layout(...)`:

```rust
pub fn render_cmux_anchor_layout(
    template: &str,
    repos: &[CmuxLayoutVar],
    agent_command: Option<&str>,
) -> Result<String> {
    let Some(vars) = repos.first() else {
        anyhow::bail!("cmux anchor layout requires at least one repo");
    };
    let mut value: Value = serde_json::from_str(template)?;
    expand_value(&mut value, repos, Some(vars))?;
    replace_extra_vars(&mut value, agent_command.unwrap_or(""), "")?;
    prune_empty(&mut value);
    normalize_layout_tree(&mut value);
    Ok(serde_json::to_string(&value)?)
}

pub fn render_cmux_repo_layout(
    template: &str,
    repo: &CmuxLayoutVar,
    agent_command: Option<&str>,
) -> Result<String> {
    let repos = std::slice::from_ref(repo);
    let mut value: Value = serde_json::from_str(template)?;
    expand_value(&mut value, repos, Some(repo))?;
    let lazygit_command = lazygit_command(repo);
    replace_extra_vars(&mut value, agent_command.unwrap_or(""), &lazygit_command)?;
    prune_empty(&mut value);
    normalize_layout_tree(&mut value);
    Ok(serde_json::to_string(&value)?)
}
```

Then add these private helpers near `replace_vars(...)`:

```rust
fn lazygit_command(vars: &CmuxLayoutVar) -> String {
    if vars.lazygit_config.is_empty() {
        format!("lazygit -p {}", vars.worktree_path)
    } else {
        format!(
            "lazygit -p {} -ucf {}",
            vars.worktree_path, vars.lazygit_config
        )
    }
}

fn replace_extra_vars(value: &mut Value, agent_command: &str, lazygit_command: &str) -> Result<()> {
    match value {
        Value::Object(map) => {
            for child in map.values_mut() {
                replace_extra_vars(child, agent_command, lazygit_command)?;
            }
        }
        Value::Array(items) => {
            for item in items {
                replace_extra_vars(item, agent_command, lazygit_command)?;
            }
        }
        Value::String(s) => {
            *s = s
                .replace("$agent_command", agent_command)
                .replace("$lazygit_command", lazygit_command);
        }
        _ => {}
    }
    Ok(())
}
```

- [ ] **Step 6: Update empty-command pruning for shell fallback**

In `src/core/cmux_layout.rs`, update `prune_empty` so a surface with empty `command` is removed, but the sibling shell surface in the same pane remains. Keep the existing behavior:

```rust
if map
    .get("command")
    .and_then(Value::as_str)
    .is_some_and(str::is_empty)
{
    return true;
}
```

No code change is required for this line if it already exists. The new templates rely on the existing sibling shell surface to avoid empty panes.

- [ ] **Step 7: Update imports in tests**

At the top of `tests/cmux_layout_test.rs`, replace the existing cmux layout import with:

```rust
use zootree::core::cmux_layout::{
    default_cmux_anchor_layout, default_cmux_layout, default_cmux_repo_layout, render_cmux_anchor_layout,
    render_cmux_layout, render_cmux_repo_layout, CmuxLayoutVar,
};
```

Run `cargo fmt` later to wrap this import.

- [ ] **Step 8: Run cmux layout tests**

Run:

```bash
cargo test --test cmux_layout_test
```

Expected: all cmux layout tests pass.

- [ ] **Step 9: Commit**

```bash
git add src/core/cmux_layout.rs tests/cmux_layout_test.rs
git commit -m "feat: add cmux group layouts"
```

---

### Task 3: Add cmux group launch and captured state types

**Files:**
- Modify: `src/core/multiplexer/mod.rs`
- Test: `tests/cmux_test.rs`

- [ ] **Step 1: Add compile-failing test helper usage**

In `tests/cmux_test.rs`, update the import to include the new types:

```rust
use zootree::core::multiplexer::{
    cmux::CmuxMultiplexer, CmuxGroupLaunch, CmuxRepoWorkspaceLaunch, LaunchOutcome,
    MultiplexerIdentity, MultiplexerLaunch, TerminalMultiplexer,
};
```

Then append this helper function after `launch()`:

```rust
fn group_launch() -> CmuxGroupLaunch {
    CmuxGroupLaunch {
        workspace_name: "fair-fox".into(),
        group_name: "Fix cmux sidebar copy".into(),
        anchor_name: "zootree-fair-fox".into(),
        anchor_description: "Fix cmux sidebar copy".into(),
        anchor_cwd: "/tmp/fair-fox".into(),
        anchor_layout: r#"{"pane":{"surfaces":[{"type":"terminal","name":"info"}]}}"#.into(),
        repo_workspaces: vec![
            CmuxRepoWorkspaceLaunch {
                repo_name: "api".into(),
                workspace_name: "zootree-fair-fox-api".into(),
                description: "api".into(),
                cwd: "/tmp/fair-fox/api".into(),
                layout: r#"{"pane":{"surfaces":[{"type":"terminal","name":"api"}]}}"#.into(),
            },
            CmuxRepoWorkspaceLaunch {
                repo_name: "web".into(),
                workspace_name: "zootree-fair-fox-web".into(),
                description: "web".into(),
                cwd: "/tmp/fair-fox/web".into(),
                layout: r#"{"pane":{"surfaces":[{"type":"terminal","name":"web"}]}}"#.into(),
            },
        ],
    }
}
```

- [ ] **Step 2: Run cmux test build to verify failure**

Run:

```bash
cargo test --test cmux_test --no-run
```

Expected: compile fails because `CmuxGroupLaunch` and `CmuxRepoWorkspaceLaunch` do not exist.

- [ ] **Step 3: Add shared cmux group launch structs**

In `src/core/multiplexer/mod.rs`, add this import near the existing `PathBuf` import:

```rust
use crate::config::workspace::CmuxRepoWorkspaceState;
```

Then add these structs after `MultiplexerLaunch`:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CmuxRepoWorkspaceLaunch {
    pub repo_name: String,
    pub workspace_name: String,
    pub description: String,
    pub cwd: PathBuf,
    pub layout: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CmuxGroupLaunch {
    pub workspace_name: String,
    pub group_name: String,
    pub anchor_name: String,
    pub anchor_description: String,
    pub anchor_cwd: PathBuf,
    pub anchor_layout: String,
    pub repo_workspaces: Vec<CmuxRepoWorkspaceLaunch>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CmuxCapturedGroupState {
    pub group: String,
    pub anchor_workspace: String,
    pub repo_workspaces: Vec<CmuxRepoWorkspaceState>,
}
```

- [ ] **Step 4: Run cmux test build**

Run:

```bash
cargo test --test cmux_test --no-run
```

Expected: cmux tests compile. Existing runtime tests may still pass because the new helper is unused.

- [ ] **Step 5: Commit**

```bash
git add src/core/multiplexer/mod.rs tests/cmux_test.rs
git commit -m "feat: model cmux group launches"
```

---

### Task 4: Implement cmux group command sequencing

**Files:**
- Modify: `src/core/multiplexer/cmux.rs`
- Test: `tests/cmux_test.rs`

- [ ] **Step 1: Add failing group creation test**

Append this test to `tests/cmux_test.rs`:

```rust
#[test]
fn launch_group_creates_anchor_group_and_repo_workspaces() {
    let runner = MockRunner::new();
    runner.push_response(success_output(b"workspace:4\n"));
    runner.push_response(success_output(b"workspace_group:2\n"));
    runner.push_response(success_output(b"workspace:5\n"));
    runner.push_response(success_output(b""));
    runner.push_response(success_output(b"workspace:6\n"));
    runner.push_response(success_output(b""));
    let cmux = CmuxMultiplexer::new(&runner);

    let state = cmux.launch_group_and_capture_state(&group_launch()).unwrap();

    assert_eq!(state.group, "workspace_group:2");
    assert_eq!(state.anchor_workspace, "workspace:4");
    assert_eq!(state.repo_workspaces.len(), 2);
    assert_eq!(state.repo_workspaces[0].repo, "api");
    assert_eq!(state.repo_workspaces[0].workspace, "workspace:5");
    assert_eq!(state.repo_workspaces[1].repo, "web");
    assert_eq!(state.repo_workspaces[1].workspace, "workspace:6");

    let calls = runner.take_calls();
    assert_eq!(calls.len(), 6);
    assert_eq!(
        calls[0].args,
        vec![
            "workspace",
            "create",
            "--name",
            "zootree-fair-fox",
            "--description",
            "Fix cmux sidebar copy",
            "--cwd",
            "/tmp/fair-fox",
            "--layout",
            r#"{"pane":{"surfaces":[{"type":"terminal","name":"info"}]}}"#,
            "--focus",
            "true"
        ]
    );
    assert_eq!(
        calls[1].args,
        vec![
            "workspace-group",
            "create",
            "--name",
            "Fix cmux sidebar copy",
            "--from",
            "workspace:4"
        ]
    );
    assert_eq!(
        calls[2].args,
        vec![
            "workspace",
            "create",
            "--name",
            "zootree-fair-fox-api",
            "--description",
            "api",
            "--cwd",
            "/tmp/fair-fox/api",
            "--layout",
            r#"{"pane":{"surfaces":[{"type":"terminal","name":"api"}]}}"#,
            "--focus",
            "false"
        ]
    );
    assert_eq!(
        calls[3].args,
        vec![
            "workspace-group",
            "add",
            "--group",
            "workspace_group:2",
            "--workspace",
            "workspace:5"
        ]
    );
    assert_eq!(
        calls[4].args,
        vec![
            "workspace",
            "create",
            "--name",
            "zootree-fair-fox-web",
            "--description",
            "web",
            "--cwd",
            "/tmp/fair-fox/web",
            "--layout",
            r#"{"pane":{"surfaces":[{"type":"terminal","name":"web"}]}}"#,
            "--focus",
            "false"
        ]
    );
    assert_eq!(
        calls[5].args,
        vec![
            "workspace-group",
            "add",
            "--group",
            "workspace_group:2",
            "--workspace",
            "workspace:6"
        ]
    );
}
```

- [ ] **Step 2: Add failing parser tests**

Append these tests to `tests/cmux_test.rs`:

```rust
#[test]
fn parse_workspace_group_ref_finds_group_ref() {
    let output = success_output(b"created workspace_group:9\n");

    assert_eq!(
        CmuxMultiplexer::<MockRunner>::parse_workspace_group_ref(&output).as_deref(),
        Some("workspace_group:9")
    );
}

#[test]
fn parse_unique_group_match_finds_exact_unique_name_from_json() {
    let stdout = br#"{
  "groups": [
    { "ref": "workspace_group:2", "name": "Fix cmux sidebar copy" },
    { "ref": "workspace_group:3", "name": "Other work" }
  ],
  "window_ref": "window:1"
}"#;

    assert_eq!(
        CmuxMultiplexer::<MockRunner>::parse_unique_group_match(
            stdout,
            "Fix cmux sidebar copy"
        )
        .as_deref(),
        Some("workspace_group:2")
    );
}

#[test]
fn parse_unique_group_match_rejects_duplicate_names() {
    let stdout = br#"{
  "groups": [
    { "ref": "workspace_group:2", "name": "Fix cmux sidebar copy" },
    { "ref": "workspace_group:3", "name": "Fix cmux sidebar copy" }
  ]
}"#;

    assert_eq!(
        CmuxMultiplexer::<MockRunner>::parse_unique_group_match(
            stdout,
            "Fix cmux sidebar copy"
        ),
        None
    );
}
```

- [ ] **Step 3: Run cmux tests to verify failure**

Run:

```bash
cargo test --test cmux_test launch_group_creates_anchor_group_and_repo_workspaces parse_workspace_group_ref_finds_group_ref parse_unique_group_match_finds_exact_unique_name_from_json parse_unique_group_match_rejects_duplicate_names
```

Expected: compile fails because the group methods do not exist.

- [ ] **Step 4: Implement group parsers**

In `src/core/multiplexer/cmux.rs`, update the import list:

```rust
use super::{
    CmuxCapturedGroupState, CmuxGroupLaunch, LaunchOutcome, MultiplexerIdentity,
    MultiplexerLaunch, TerminalMultiplexer,
};
use crate::config::workspace::CmuxRepoWorkspaceState;
use serde_json::Value;
```

Then add these methods inside `impl<'a, R: CommandRunner> CmuxMultiplexer<'a, R>` after `parse_workspace_ref(...)`:

```rust
pub fn parse_workspace_group_ref(output: &std::process::Output) -> Option<String> {
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .flat_map(str::split_whitespace)
        .find(|token| Self::is_workspace_group_ref(token))
        .map(str::to_string)
}

fn is_workspace_group_ref(token: &str) -> bool {
    let Some(id) = token.strip_prefix("workspace_group:") else {
        return false;
    };
    !id.is_empty() && id.chars().all(|ch| ch.is_ascii_digit())
}

pub fn parse_unique_group_match(stdout: &[u8], group_name: &str) -> Option<String> {
    let value: Value = serde_json::from_slice(stdout).ok()?;
    let groups = value.get("groups")?.as_array()?;
    let matches = groups
        .iter()
        .filter_map(|group| {
            let name = group
                .get("name")
                .or_else(|| group.get("title"))
                .and_then(Value::as_str)?;
            if name != group_name {
                return None;
            }
            group
                .get("ref")
                .or_else(|| group.get("workspace_group"))
                .or_else(|| group.get("id"))
                .and_then(Value::as_str)
                .filter(|value| Self::is_workspace_group_ref(value))
                .map(str::to_string)
        })
        .collect::<Vec<_>>();

    if matches.len() == 1 {
        Some(matches[0].clone())
    } else {
        None
    }
}
```

- [ ] **Step 5: Implement group launch**

Add these methods inside the same impl block:

```rust
fn create_workspace(
    &self,
    name: &str,
    description: &str,
    cwd: &std::path::Path,
    layout: &str,
    focus: bool,
) -> Result<String> {
    let output = self.cmux(vec![
        "workspace".into(),
        "create".into(),
        "--name".into(),
        name.into(),
        "--description".into(),
        description.into(),
        "--cwd".into(),
        cwd.to_string_lossy().into_owned(),
        "--layout".into(),
        layout.into(),
        "--focus".into(),
        focus.to_string(),
    ])?;
    let output = Self::ensure_success(output, "cmux workspace create")?;
    Self::parse_workspace_ref(&output)
        .ok_or_else(|| anyhow::anyhow!("cmux workspace create did not return a workspace ref"))
}

fn create_group(&self, name: &str, anchor_workspace: &str) -> Result<String> {
    let output = self.cmux(vec![
        "workspace-group".into(),
        "create".into(),
        "--name".into(),
        name.into(),
        "--from".into(),
        anchor_workspace.into(),
    ])?;
    let output = Self::ensure_success(output, "cmux workspace-group create")?;
    Self::parse_workspace_group_ref(&output)
        .ok_or_else(|| anyhow::anyhow!("cmux workspace-group create did not return a group ref"))
}

fn add_workspace_to_group(&self, group: &str, workspace: &str) -> Result<()> {
    let output = self.cmux(vec![
        "workspace-group".into(),
        "add".into(),
        "--group".into(),
        group.into(),
        "--workspace".into(),
        workspace.into(),
    ])?;
    Self::ensure_success(output, "cmux workspace-group add")?;
    Ok(())
}

pub fn launch_group_and_capture_state(
    &self,
    launch: &CmuxGroupLaunch,
) -> Result<CmuxCapturedGroupState> {
    let anchor_workspace = self.create_workspace(
        &launch.anchor_name,
        &launch.anchor_description,
        &launch.anchor_cwd,
        &launch.anchor_layout,
        true,
    )?;
    let group = self.create_group(&launch.group_name, &anchor_workspace)?;

    let mut repo_workspaces = Vec::new();
    for repo in &launch.repo_workspaces {
        let workspace = self.create_workspace(
            &repo.workspace_name,
            &repo.description,
            &repo.cwd,
            &repo.layout,
            false,
        )?;
        self.add_workspace_to_group(&group, &workspace)?;
        repo_workspaces.push(CmuxRepoWorkspaceState {
            repo: repo.repo_name.clone(),
            workspace,
        });
    }

    Ok(CmuxCapturedGroupState {
        group,
        anchor_workspace,
        repo_workspaces,
    })
}
```

- [ ] **Step 6: Run cmux tests**

Run:

```bash
cargo test --test cmux_test launch_group_creates_anchor_group_and_repo_workspaces parse_workspace_group_ref_finds_group_ref parse_unique_group_match_finds_exact_unique_name_from_json parse_unique_group_match_rejects_duplicate_names
```

Expected: selected cmux tests pass.

- [ ] **Step 7: Commit**

```bash
git add src/core/multiplexer/cmux.rs tests/cmux_test.rs
git commit -m "feat: create cmux workspace groups"
```

---

### Task 5: Implement cmux group focus and delete

**Files:**
- Modify: `src/core/multiplexer/cmux.rs`
- Test: `tests/cmux_test.rs`

- [ ] **Step 1: Add failing focus/delete tests**

Append these tests to `tests/cmux_test.rs`:

```rust
#[test]
fn focus_group_uses_persisted_group_ref() {
    let runner = MockRunner::new();
    runner.push_response(success_output(b""));
    let cmux = CmuxMultiplexer::new(&runner);

    cmux.focus_group_or_find("Fix cmux sidebar copy", Some("workspace_group:2"))
        .unwrap();

    let calls = runner.take_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].args, vec!["workspace-group", "focus", "workspace_group:2"]);
}

#[test]
fn focus_group_falls_back_to_unique_title_match() {
    let runner = MockRunner::new();
    runner.push_response(failure_output(b"group not found"));
    runner.push_response(success_output(
        br#"{"groups":[{"ref":"workspace_group:7","name":"Fix cmux sidebar copy"}]}"#,
    ));
    runner.push_response(success_output(b""));
    let cmux = CmuxMultiplexer::new(&runner);

    let found = cmux
        .focus_group_or_find("Fix cmux sidebar copy", Some("workspace_group:2"))
        .unwrap();

    assert_eq!(found.as_deref(), Some("workspace_group:7"));
    let calls = runner.take_calls();
    assert_eq!(calls[0].args, vec!["workspace-group", "focus", "workspace_group:2"]);
    assert_eq!(calls[1].args, vec!["workspace-group", "list", "--json"]);
    assert_eq!(calls[2].args, vec!["workspace-group", "focus", "workspace_group:7"]);
}

#[test]
fn delete_group_uses_persisted_group_ref() {
    let runner = MockRunner::new();
    runner.push_response(success_output(b""));
    let cmux = CmuxMultiplexer::new(&runner);

    cmux.delete_group("Fix cmux sidebar copy", Some("workspace_group:2"))
        .unwrap();

    let calls = runner.take_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].args, vec!["workspace-group", "delete", "workspace_group:2"]);
}

#[test]
fn delete_group_without_ref_uses_unique_title_match() {
    let runner = MockRunner::new();
    runner.push_response(success_output(
        br#"{"groups":[{"ref":"workspace_group:7","name":"Fix cmux sidebar copy"}]}"#,
    ));
    runner.push_response(success_output(b""));
    let cmux = CmuxMultiplexer::new(&runner);

    cmux.delete_group("Fix cmux sidebar copy", None).unwrap();

    let calls = runner.take_calls();
    assert_eq!(calls[0].args, vec!["workspace-group", "list", "--json"]);
    assert_eq!(calls[1].args, vec!["workspace-group", "delete", "workspace_group:7"]);
}

#[test]
fn delete_group_without_unique_match_skips_delete() {
    let runner = MockRunner::new();
    runner.push_response(success_output(
        br#"{"groups":[
            {"ref":"workspace_group:7","name":"Fix cmux sidebar copy"},
            {"ref":"workspace_group:8","name":"Fix cmux sidebar copy"}
        ]}"#,
    ));
    let cmux = CmuxMultiplexer::new(&runner);

    cmux.delete_group("Fix cmux sidebar copy", None).unwrap();

    let calls = runner.take_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].args, vec!["workspace-group", "list", "--json"]);
}
```

- [ ] **Step 2: Run tests to verify failure**

Run:

```bash
cargo test --test cmux_test focus_group_uses_persisted_group_ref focus_group_falls_back_to_unique_title_match delete_group_uses_persisted_group_ref delete_group_without_ref_uses_unique_title_match delete_group_without_unique_match_skips_delete
```

Expected: compile fails because `focus_group_or_find` and `delete_group` do not exist.

- [ ] **Step 3: Implement group lookup, focus, and delete**

Add these methods to `src/core/multiplexer/cmux.rs` inside the cmux impl block:

```rust
fn find_group_by_name(&self, group_name: &str) -> Result<Option<String>> {
    let output = self.cmux(vec![
        "workspace-group".into(),
        "list".into(),
        "--json".into(),
    ])?;
    let output = Self::ensure_success(output, "cmux workspace-group list")?;
    Ok(Self::parse_unique_group_match(&output.stdout, group_name))
}

fn focus_group_ref(&self, group: &str) -> Result<()> {
    let output = self.cmux(vec![
        "workspace-group".into(),
        "focus".into(),
        group.into(),
    ])?;
    Self::ensure_success(output, "cmux workspace-group focus")?;
    Ok(())
}

pub fn focus_group_or_find(
    &self,
    group_name: &str,
    group_ref: Option<&str>,
) -> Result<Option<String>> {
    if let Some(group) = group_ref {
        let output = self.cmux(vec![
            "workspace-group".into(),
            "focus".into(),
            group.into(),
        ])?;
        if output.status.success() {
            return Ok(None);
        }
        tracing::warn!(
            "cmux group '{}' could not be focused: {}; trying title lookup",
            group,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let Some(group) = self.find_group_by_name(group_name)? else {
        tracing::warn!(
            "cmux group '{}' not found uniquely; skipping focus",
            group_name
        );
        return Ok(None);
    };
    self.focus_group_ref(&group)?;
    Ok(Some(group))
}

pub fn delete_group(&self, group_name: &str, group_ref: Option<&str>) -> Result<()> {
    let group = match group_ref {
        Some(group) => group.to_string(),
        None => match self.find_group_by_name(group_name)? {
            Some(group) => group,
            None => {
                tracing::warn!(
                    "cmux group '{}' not found uniquely; skipping cmux group delete",
                    group_name
                );
                return Ok(());
            }
        },
    };

    let output = self.cmux(vec![
        "workspace-group".into(),
        "delete".into(),
        group,
    ])?;
    Self::ensure_success(output, "cmux workspace-group delete")?;
    Ok(())
}
```

- [ ] **Step 4: Run selected cmux tests**

Run:

```bash
cargo test --test cmux_test focus_group_uses_persisted_group_ref focus_group_falls_back_to_unique_title_match delete_group_uses_persisted_group_ref delete_group_without_ref_uses_unique_title_match delete_group_without_unique_match_skips_delete
```

Expected: selected tests pass.

- [ ] **Step 5: Run all cmux tests**

Run:

```bash
cargo test --test cmux_test
```

Expected: all cmux tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/core/multiplexer/cmux.rs tests/cmux_test.rs
git commit -m "feat: manage cmux workspace groups"
```

---

### Task 6: Prepare cmux group launches from workspace data

**Files:**
- Modify: `src/cli/workspace.rs`
- Test: add helper tests inside `src/cli/workspace.rs` test module

- [ ] **Step 1: Add failing helper test for multi-repo launch preparation**

Inside `#[cfg(test)] mod tests` in `src/cli/workspace.rs`, add this test near existing helper tests:

```rust
#[test]
fn prepare_cmux_group_launch_places_multi_repo_agent_in_anchor() {
    let temp = tempfile::tempdir().unwrap();
    let config_mgr = ConfigManager::with_base_dir(temp.path().to_path_buf());
    config_mgr.ensure_dirs().unwrap();
    config_mgr
        .save_repo_config("api", &repo_config("/repo/api"))
        .unwrap();
    config_mgr
        .save_repo_config("web", &repo_config("/repo/web"))
        .unwrap();

    let mut global = GlobalConfig::default();
    global.agent_cli = Some("codex -- $prompt".into());
    let mut workspace = list_workspace(
        WorkspaceStatus::InProgress,
        "fair-fox",
        "Fix cmux sidebar copy",
        "zootree/fair-fox",
        "/tmp/fair-fox",
        vec![repo("api", Some("main")), repo("web", Some("main"))],
    )
    .workspace;
    workspace.multiplexer.kind = MultiplexerKind::Cmux;

    let launch = prepare_cmux_group_launch(&config_mgr, &global, &workspace, Some(Some("".into())))
        .unwrap();

    assert_eq!(launch.group_name, "Fix cmux sidebar copy");
    assert_eq!(launch.anchor_name, "zootree-fair-fox");
    assert_eq!(launch.repo_workspaces.len(), 2);
    assert!(launch.anchor_layout.contains("zootree info fair-fox --watch"));
    assert!(launch.anchor_layout.contains("codex"));
    assert!(!launch.repo_workspaces[0].layout.contains("codex"));
    assert!(!launch.repo_workspaces[1].layout.contains("codex"));
}
```

- [ ] **Step 2: Add failing helper test for single-repo launch preparation**

Append this test in the same test module:

```rust
#[test]
fn prepare_cmux_group_launch_places_single_repo_agent_in_repo_workspace() {
    let temp = tempfile::tempdir().unwrap();
    let config_mgr = ConfigManager::with_base_dir(temp.path().to_path_buf());
    config_mgr.ensure_dirs().unwrap();
    config_mgr
        .save_repo_config("api", &repo_config("/repo/api"))
        .unwrap();

    let mut global = GlobalConfig::default();
    global.agent_cli = Some("codex -- $prompt".into());
    let mut workspace = list_workspace(
        WorkspaceStatus::InProgress,
        "fair-fox",
        "Fix cmux sidebar copy",
        "zootree/fair-fox",
        "/tmp/fair-fox",
        vec![repo("api", Some("main"))],
    )
    .workspace;
    workspace.multiplexer.kind = MultiplexerKind::Cmux;

    let launch = prepare_cmux_group_launch(&config_mgr, &global, &workspace, Some(Some("".into())))
        .unwrap();

    assert!(launch.anchor_layout.contains("zootree info fair-fox --watch"));
    assert!(!launch.anchor_layout.contains("codex"));
    assert_eq!(launch.repo_workspaces.len(), 1);
    assert!(launch.repo_workspaces[0].layout.contains("codex"));
    assert!(launch.repo_workspaces[0].layout.contains("lazygit -p /tmp/fair-fox/api"));
}
```

- [ ] **Step 3: Add failing unsupported-layout test**

Append this test in the same test module:

```rust
#[test]
fn prepare_cmux_group_launch_rejects_non_default_layout() {
    let temp = tempfile::tempdir().unwrap();
    let config_mgr = ConfigManager::with_base_dir(temp.path().to_path_buf());
    config_mgr.ensure_dirs().unwrap();
    config_mgr
        .save_repo_config("api", &repo_config("/repo/api"))
        .unwrap();

    let global = GlobalConfig::default();
    let mut workspace = list_workspace(
        WorkspaceStatus::InProgress,
        "fair-fox",
        "Fix cmux sidebar copy",
        "zootree/fair-fox",
        "/tmp/fair-fox",
        vec![repo("api", Some("main"))],
    )
    .workspace;
    workspace.multiplexer.kind = MultiplexerKind::Cmux;
    workspace.multiplexer.cmux.layout = Some("wide".into());

    let err = prepare_cmux_group_launch(&config_mgr, &global, &workspace, None).unwrap_err();
    let msg = format!("{:#}", err);

    assert!(
        msg.contains("group-aware cmux currently supports only layout = \"default\""),
        "unexpected error: {msg}"
    );
}
```

- [ ] **Step 4: Run workspace helper tests to verify failure**

Run:

```bash
cargo test cli::workspace::tests::prepare_cmux_group_launch_places_multi_repo_agent_in_anchor cli::workspace::tests::prepare_cmux_group_launch_places_single_repo_agent_in_repo_workspace cli::workspace::tests::prepare_cmux_group_launch_rejects_non_default_layout
```

Expected: compile fails because `prepare_cmux_group_launch` does not exist.

- [ ] **Step 5: Update workspace imports**

In `src/cli/workspace.rs`, update the cmux layout import:

```rust
use crate::core::cmux_layout::{
    default_cmux_anchor_layout, default_cmux_layout, default_cmux_repo_layout,
    render_cmux_anchor_layout, render_cmux_layout, render_cmux_repo_layout, CmuxLayoutVar,
};
```

Update the multiplexer import:

```rust
use crate::core::multiplexer::{
    cmux::CmuxMultiplexer,
    zellij::{is_inside_zellij, ZellijMultiplexer},
    CmuxGroupLaunch, CmuxRepoWorkspaceLaunch, MultiplexerIdentity, MultiplexerLaunch,
    TerminalMultiplexer,
};
```

- [ ] **Step 6: Add cmux workspace naming helpers**

Add these helpers near `multiplexer_display_name(...)`:

```rust
fn cmux_anchor_workspace_name(workspace: &WorkspaceConfig) -> String {
    multiplexer_display_name(workspace)
}

fn cmux_repo_workspace_name(workspace: &WorkspaceConfig, repo_name: &str) -> String {
    format!("{}-{}", multiplexer_display_name(workspace), repo_name)
}
```

- [ ] **Step 7: Implement cmux group launch preparation**

Add this function after `prepare_cmux_launch(...)`:

```rust
fn prepare_cmux_group_launch(
    config_mgr: &ConfigManager,
    global: &GlobalConfig,
    workspace: &WorkspaceConfig,
    run_agent: Option<Option<String>>,
) -> Result<CmuxGroupLaunch> {
    let multiplexer = selected_multiplexer_config(workspace, global);
    let layout_name = multiplexer.cmux.layout.as_deref().unwrap_or("default");
    if layout_name != "default" {
        anyhow::bail!(
            "group-aware cmux currently supports only layout = \"default\"; workspace '{}' selected '{}'",
            workspace.name,
            layout_name
        );
    }

    let ws_dir = shellexpand::tilde(&workspace.workspace_dir).into_owned();
    let agent_cli_tpl = resolve_run_agent_template(global, run_agent.as_ref())?;
    let prompt = crate::core::layout::build_prompt(workspace);
    let agent_command = match agent_cli_tpl.as_deref() {
        Some(tpl) => Some(crate::core::layout::build_agent_cli_command(tpl, &prompt)?),
        None => None,
    };
    let single_repo = workspace.repos.len() == 1;

    let mut vars = Vec::new();
    for repo_entry in &workspace.repos {
        let repo_config = config_mgr.load_repo_config(&repo_entry.name)?;
        let lazygit_config = repo_config.lazygit.map(|lg| lg.config).unwrap_or_default();
        vars.push(CmuxLayoutVar {
            repo_name: repo_entry.name.clone(),
            worktree_path: format!("{}/{}", ws_dir, repo_entry.name),
            branch: workspace.branch.clone(),
            workspace_name: workspace.name.clone(),
            workspace_dir: ws_dir.clone(),
            lazygit_config,
            overview_agent_command: String::new(),
            repo_agent_command: String::new(),
        });
    }

    let anchor_agent = if single_repo {
        None
    } else {
        agent_command.as_deref()
    };
    let repo_agent = if single_repo {
        agent_command.as_deref()
    } else {
        None
    };

    let anchor_layout =
        render_cmux_anchor_layout(default_cmux_anchor_layout(), &vars, anchor_agent)?;

    let repo_workspaces = vars
        .iter()
        .map(|repo| {
            let layout = render_cmux_repo_layout(default_cmux_repo_layout(), repo, repo_agent)?;
            Ok(CmuxRepoWorkspaceLaunch {
                repo_name: repo.repo_name.clone(),
                workspace_name: cmux_repo_workspace_name(workspace, &repo.repo_name),
                description: repo.repo_name.clone(),
                cwd: repo.worktree_path.clone().into(),
                layout,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(CmuxGroupLaunch {
        workspace_name: workspace.name.clone(),
        group_name: workspace.title.clone(),
        anchor_name: cmux_anchor_workspace_name(workspace),
        anchor_description: workspace.title.clone(),
        anchor_cwd: ws_dir.into(),
        anchor_layout,
        repo_workspaces,
    })
}
```

- [ ] **Step 8: Run workspace helper tests**

Run:

```bash
cargo test cli::workspace::tests::prepare_cmux_group_launch_places_multi_repo_agent_in_anchor cli::workspace::tests::prepare_cmux_group_launch_places_single_repo_agent_in_repo_workspace cli::workspace::tests::prepare_cmux_group_launch_rejects_non_default_layout
```

Expected: selected helper tests pass.

- [ ] **Step 9: Commit**

```bash
git add src/cli/workspace.rs
git commit -m "feat: prepare cmux group launches"
```

---

### Task 7: Wire cmux group launch, open, and close into workspace lifecycle

**Files:**
- Modify: `src/cli/workspace.rs`
- Test: `tests/cmux_test.rs`
- Test: helper tests inside `src/cli/workspace.rs` if added in Task 6

- [ ] **Step 1: Add failing test for state conversion**

Inside `#[cfg(test)] mod tests` in `src/cli/workspace.rs`, append:

```rust
#[test]
fn apply_cmux_group_state_replaces_legacy_workspace_ref() {
    let mut workspace = list_workspace(
        WorkspaceStatus::InProgress,
        "fair-fox",
        "Fix cmux sidebar copy",
        "zootree/fair-fox",
        "/tmp/fair-fox",
        vec![repo("api", Some("main"))],
    )
    .workspace;
    workspace.multiplexer_state.cmux_workspace = Some("workspace:old".into());

    apply_cmux_group_state(
        &mut workspace,
        crate::core::multiplexer::CmuxCapturedGroupState {
            group: "workspace_group:2".into(),
            anchor_workspace: "workspace:4".into(),
            repo_workspaces: vec![CmuxRepoWorkspaceState {
                repo: "api".into(),
                workspace: "workspace:5".into(),
            }],
        },
    );

    assert_eq!(workspace.multiplexer_state.kind, Some(MultiplexerKind::Cmux));
    assert_eq!(
        workspace.multiplexer_state.cmux_group.as_deref(),
        Some("workspace_group:2")
    );
    assert_eq!(
        workspace.multiplexer_state.cmux_anchor_workspace.as_deref(),
        Some("workspace:4")
    );
    assert!(workspace.multiplexer_state.cmux_workspace.is_none());
    assert_eq!(workspace.multiplexer_state.cmux_repo_workspaces.len(), 1);
}
```

Add this import inside the test module if needed:

```rust
use crate::config::workspace::CmuxRepoWorkspaceState;
```

- [ ] **Step 2: Run test to verify failure**

Run:

```bash
cargo test cli::workspace::tests::apply_cmux_group_state_replaces_legacy_workspace_ref
```

Expected: compile fails because `apply_cmux_group_state` does not exist.

- [ ] **Step 3: Implement state conversion helper**

In `src/cli/workspace.rs`, update the multiplexer import to include captured state:

```rust
use crate::core::multiplexer::{
    cmux::CmuxMultiplexer,
    zellij::{is_inside_zellij, ZellijMultiplexer},
    CmuxCapturedGroupState, CmuxGroupLaunch, CmuxRepoWorkspaceLaunch, MultiplexerIdentity,
    MultiplexerLaunch, TerminalMultiplexer,
};
```

Add this helper near `multiplexer_identity(...)`:

```rust
fn apply_cmux_group_state(workspace: &mut WorkspaceConfig, state: CmuxCapturedGroupState) {
    workspace.multiplexer_state.kind = Some(MultiplexerKind::Cmux);
    workspace.multiplexer_state.cmux_workspace = None;
    workspace.multiplexer_state.cmux_group = Some(state.group);
    workspace.multiplexer_state.cmux_anchor_workspace = Some(state.anchor_workspace);
    workspace.multiplexer_state.cmux_repo_workspaces = state.repo_workspaces;
}
```

- [ ] **Step 4: Run state conversion test**

Run:

```bash
cargo test cli::workspace::tests::apply_cmux_group_state_replaces_legacy_workspace_ref
```

Expected: selected test passes.

- [ ] **Step 5: Replace launch dispatch with kind-specific preparation**

In `src/cli/workspace.rs`, replace the entire body of `launch_multiplexer(...)` with this code. cmux group launch no longer produces a single `MultiplexerLaunch`, so the function must branch before preparing launch data.

```rust
fn launch_multiplexer(
    config_mgr: &ConfigManager,
    global: &GlobalConfig,
    workspace: &WorkspaceConfig,
    runner: &RealRunner,
    run_agent: Option<Option<String>>,
) -> Result<()> {
    let config = selected_multiplexer_config(workspace, global);
    match config.kind {
        MultiplexerKind::Zellij => {
            let launch = prepare_zellij_launch(config_mgr, global, workspace, run_agent)?;
            let zellij = ZellijMultiplexer::new(runner, is_inside_zellij());
            zellij.launch(&launch)?;
        }
        MultiplexerKind::Cmux => {
            let cmux = CmuxMultiplexer::new(runner);
            let group_launch =
                prepare_cmux_group_launch(config_mgr, global, workspace, run_agent)?;
            if let Some(group) = workspace.multiplexer_state.cmux_group.as_deref() {
                if let Some(found_group) =
                    cmux.focus_group_or_find(&group_launch.group_name, Some(group))?
                {
                    let mut updated = workspace.clone();
                    updated.multiplexer_state.kind = Some(MultiplexerKind::Cmux);
                    updated.multiplexer_state.cmux_group = Some(found_group);
                    config_mgr.save_workspace(&WorkspaceStatus::InProgress, &updated)?;
                    return Ok(());
                }
                return Ok(());
            }

            if let Some(found_group) = cmux.focus_group_or_find(&group_launch.group_name, None)? {
                let mut updated = workspace.clone();
                updated.multiplexer_state.kind = Some(MultiplexerKind::Cmux);
                updated.multiplexer_state.cmux_group = Some(found_group);
                config_mgr.save_workspace(&WorkspaceStatus::InProgress, &updated)?;
                return Ok(());
            }

            let captured = cmux.launch_group_and_capture_state(&group_launch)?;
            let mut updated = workspace.clone();
            apply_cmux_group_state(&mut updated, captured);
            config_mgr.save_workspace(&WorkspaceStatus::InProgress, &updated)?;
        }
    }
    Ok(())
}
```

This intentionally replaces the previous single-workspace cmux launch path. The persisted legacy `cmux_workspace` is not used for group focus.

- [ ] **Step 6: Wire close for cmux**

In `close_multiplexer(...)`, replace the `MultiplexerKind::Cmux` arm with:

```rust
MultiplexerKind::Cmux => {
    let cmux = CmuxMultiplexer::new(runner);
    cmux.delete_group(
        &workspace.title,
        workspace.multiplexer_state.cmux_group.as_deref(),
    )?;
}
```

- [ ] **Step 7: Remove the old generic cmux launch preparation path**

After the new launch path compiles, remove `prepare_cmux_launch(...)` if it is no longer called by production code. Also remove `prepare_multiplexer_launch(...)` if no production code calls it after `launch_multiplexer(...)` switches to kind-specific preparation.

Keep `default_cmux_layout` and `render_cmux_layout` for existing tests until a later cleanup decides whether to remove legacy renderer coverage. The production cmux launch path must use `prepare_cmux_group_launch(...)`.

- [ ] **Step 8: Run targeted tests**

Run:

```bash
cargo test --test cmux_test
cargo test --test cmux_layout_test
cargo test cli::workspace::tests::prepare_cmux_group_launch_places_multi_repo_agent_in_anchor cli::workspace::tests::prepare_cmux_group_launch_places_single_repo_agent_in_repo_workspace cli::workspace::tests::prepare_cmux_group_launch_rejects_non_default_layout cli::workspace::tests::apply_cmux_group_state_replaces_legacy_workspace_ref
```

Expected: all targeted tests pass.

- [ ] **Step 9: Commit**

```bash
git add src/cli/workspace.rs tests/cmux_test.rs tests/cmux_layout_test.rs
git commit -m "feat: wire cmux groups into workspace lifecycle"
```

---

### Task 8: Update docs and zootree skills

**Files:**
- Modify: `README.md`
- Modify: `README.zh-CN.md`
- Modify: `skills/zootree-usage/SKILL.md`
- Modify: `skills/zootree-dev/SKILL.md`

- [ ] **Step 1: Update README cmux layout section**

In `README.md`, replace the current cmux layout template section with text that states:

```markdown
### cmux group layout

When `[multiplexer] kind = "cmux"`, zootree creates one cmux workspace group per zootree workspace.

- The group name is the zootree workspace title.
- The group contains one anchor workspace plus one workspace per repo.
- The anchor workspace runs `zootree info <workspace> --watch` on the left.
- With multiple repos, `--run-agent` runs in the anchor workspace on the right.
- Each repo workspace runs `lazygit -p <worktree_path>` on the left and shells on the right.
- With a single repo, `--run-agent` runs in that repo workspace's bottom-right terminal.

Group-aware cmux currently supports only `layout = "default"`. Non-default cmux layouts return a clear error until a group-aware multi-template layout configuration exists.
```

- [ ] **Step 2: Update README Agent CLI section**

In `README.md`, replace the cmux/zellij location bullets under Agent CLI with:

```markdown
For zellij, the rendered command runs in:

- **1 repo** → the repo tab's bottom-right pane
- **≥2 repos** → the overview tab's last pane

For cmux, the rendered command runs in:

- **1 repo** → the repo workspace's bottom-right terminal
- **≥2 repos** → the anchor workspace's right terminal
```

- [ ] **Step 3: Update Chinese README with equivalent content**

In `README.zh-CN.md`, add the Chinese equivalent:

```markdown
### cmux group 布局

当 `[multiplexer] kind = "cmux"` 时，zootree 会为每个 zootree workspace 创建一个 cmux workspace group。

- group name 使用 zootree workspace title。
- group 内包含一个 anchor workspace，以及每个 repo 一个 workspace。
- anchor workspace 左侧运行 `zootree info <workspace> --watch`。
- 多 repo 且使用 `--run-agent` 时，agent 运行在 anchor workspace 右侧。
- 每个 repo workspace 左侧运行 `lazygit -p <worktree_path>`，右侧是 shell。
- 单 repo 且使用 `--run-agent` 时，agent 运行在该 repo workspace 的右下 terminal。

group-aware cmux 当前只支持 `layout = "default"`。非 default cmux layout 会返回明确错误，直到后续支持 group-aware 多模板配置。
```

- [ ] **Step 4: Update usage skill**

In `skills/zootree-usage/SKILL.md`, update the cmux section with this concise description:

```markdown
cmux 模式会为一个 zootree workspace 创建一个 cmux workspace group。group name 使用 workspace title；group 内有一个 anchor workspace 用于 `zootree info` 和多 repo agent，另外每个 repo 一个 workspace，repo workspace 左侧运行 lazygit、右侧运行 shell。单 repo 的 `--run-agent` 运行在 repo workspace 右下 terminal。当前 cmux group 模式只支持 `layout = "default"`。
```

- [ ] **Step 5: Update dev skill**

In `skills/zootree-dev/SKILL.md`, update:

- Project architecture if files were added or removed.
- Core design pattern section to mention cmux group-aware helpers in `src/core/multiplexer/cmux.rs`.
- Coding convention section to mention `MultiplexerState` now stores cmux group refs and repo workspace refs.

Use wording like:

```markdown
- **cmux group state**: cmux mode maps one zootree workspace to one cmux workspace group. Runtime refs live in `WorkspaceConfig.multiplexer_state`: `cmux_group`, `cmux_anchor_workspace`, and `cmux_repo_workspaces`. Legacy `cmux_workspace` remains readable for older configs but new group-aware saves should not write it.
```

- [ ] **Step 6: Run docs grep sanity check**

Run:

```bash
rg -n "cmux layout templates|single cmux workspace|overview tab|repo tab" README.md README.zh-CN.md skills/zootree-usage/SKILL.md skills/zootree-dev/SKILL.md
```

Expected: no stale wording claims cmux default creates one workspace containing all repos. Mentions of zellij tabs may remain in zellij-specific sections.

- [ ] **Step 7: Commit**

```bash
git add README.md README.zh-CN.md skills/zootree-usage/SKILL.md skills/zootree-dev/SKILL.md
git commit -m "docs: document cmux group workspaces"
```

---

### Task 9: Final verification and cleanup

**Files:**
- Verify all changed files.

- [ ] **Step 1: Format**

Run:

```bash
cargo fmt
```

Expected: command exits 0 and formats Rust files.

- [ ] **Step 2: Run targeted tests**

Run:

```bash
cargo test --test config_test
cargo test --test cmux_layout_test
cargo test --test cmux_test
cargo test cli::workspace::tests
```

Expected: all targeted tests pass.

- [ ] **Step 3: Run full test suite**

Run:

```bash
cargo test
```

Expected: full test suite passes.

- [ ] **Step 4: Check warnings and stale legacy path references**

Run:

```bash
rg -n "prepare_cmux_launch|cmux_workspace|default_cmux_layout|render_cmux_layout|workspace create" src tests README.md README.zh-CN.md skills
```

Expected:

- `cmux_workspace` appears only for legacy config compatibility tests, legacy field definition, and migration/fallback handling.
- `default_cmux_layout` / `render_cmux_layout` may remain only if existing tests still cover the legacy renderer as an isolated renderer.
- Production workspace launch no longer uses the old single cmux workspace path.

- [ ] **Step 5: Inspect final diff**

Run:

```bash
git diff --stat
git diff -- src/config/workspace.rs src/core/cmux_layout.rs src/core/multiplexer/mod.rs src/core/multiplexer/cmux.rs src/cli/workspace.rs
```

Expected: diff is limited to cmux group state, cmux group command sequencing, group layouts, workspace lifecycle wiring, tests, and docs.

- [ ] **Step 6: Commit formatting or final cleanup if needed**

If `cargo fmt` or documentation cleanup changed files after the earlier commits, run:

```bash
git add .
git commit -m "chore: finalize cmux group workspaces"
```

Expected: commit is created only if there are remaining tracked changes.

---

## Self-Review Notes

- Spec coverage:
  - One cmux group per zootree workspace: Tasks 3, 4, 7.
  - Group name uses workspace title: Tasks 4, 6.
  - Anchor workspace plus repo workspaces: Tasks 3, 4, 6.
  - Anchor `zootree info`: Tasks 2, 6.
  - Single-repo and multi-repo agent placement: Tasks 2, 6.
  - Group delete on done/cancel: Tasks 5, 7.
  - Persist refs: Tasks 1, 7.
  - Non-default layout unsupported: Tasks 2, 6, 8.
  - Docs and skill updates: Task 8.

- Placeholder scan:
  - This plan avoids placeholder markers and defines concrete file paths, code snippets, commands, and expected outcomes.

- Type consistency:
  - `CmuxRepoWorkspaceState` is defined in `src/config/workspace.rs`.
  - `CmuxGroupLaunch`, `CmuxRepoWorkspaceLaunch`, and `CmuxCapturedGroupState` are defined in `src/core/multiplexer/mod.rs`.
  - `CmuxMultiplexer` owns cmux CLI command sequencing and parser helpers.
