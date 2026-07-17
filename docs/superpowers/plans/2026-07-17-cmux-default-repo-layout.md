# cmux Default Repo Layout Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Change the default cmux repo workspace to a 50/50 layout with the agent on the left and two regular shells on the right, without launching lazygit.

**Architecture:** Change only the default JSON returned by `default_cmux_repo_layout()`. Keep the renderer's `$lazygit_command` support intact for explicit custom templates, and keep the existing empty-agent-to-shell fallback.

**Tech Stack:** Rust 2021, serde_json, Cargo integration tests, Markdown documentation.

## Global Constraints

- The root horizontal split is exactly `0.5`.
- The left terminal is the focused agent surface; without `--run-agent`, it becomes a focused regular shell.
- The right half contains two vertically split regular shells.
- The default cmux repo layout does not launch lazygit.
- Anchor workspace behavior, zellij behavior, cmux lifecycle, and custom lazygit template rendering remain unchanged.

---

## File Structure

- Modify `tests/cmux_layout_test.rs`: lock the new default topology and keep lazygit renderer coverage with an explicit custom template.
- Modify `src/core/cmux_layout.rs`: update only `default_cmux_repo_layout()`.
- Modify `src/cli/workspace.rs`: align the launch preparation unit test with the no-lazygit default contract.
- Modify `README.md`: document the new English cmux layout and single-repo agent position.
- Modify `README.zh-CN.md`: mirror the English behavior in Chinese.
- Modify `skills/zootree-usage/references/layouts.md`: keep the repo-local usage skill aligned with runtime behavior.

### Task 1: Update the default cmux repo layout contract

**Files:**
- Modify: `tests/cmux_layout_test.rs:229`
- Modify: `src/core/cmux_layout.rs:99`
- Modify: `src/cli/workspace.rs:1767`
- Modify: `README.md:312`
- Modify: `README.zh-CN.md:304`
- Modify: `skills/zootree-usage/references/layouts.md:14`

**Interfaces:**
- Consumes: `render_cmux_repo_layout(template: &str, repo: &CmuxLayoutVar, agent_command: Option<&str>) -> Result<String>` and the existing empty-agent fallback.
- Produces: `default_cmux_repo_layout() -> &'static str` with the confirmed 50/50 three-terminal topology.

- [x] **Step 1: Write failing structural tests for the new default layout**

Replace `repo_layout_runs_lazygit_and_single_repo_agent` and `repo_layout_without_agent_keeps_shell_bottom` in `tests/cmux_layout_test.rs` with:

```rust
#[test]
fn default_repo_layout_places_agent_left_and_two_shells_right() {
    let repo = vars().remove(0);
    let rendered = render_cmux_repo_layout(
        default_cmux_repo_layout(),
        &repo,
        Some("codex --prompt 'Fix login'"),
    )
    .unwrap();
    let value: Value = serde_json::from_str(&rendered).unwrap();
    let commands = collect_string_field(&value, "command");

    assert_eq!(value["direction"], "horizontal");
    assert_eq!(value["split"], 0.5);
    assert!(!commands.iter().any(|command| command.contains("lazygit")));

    let left_surfaces = repo_left_surfaces(&value);
    assert_eq!(left_surfaces.len(), 1);
    assert_eq!(left_surfaces[0]["name"], "agent");
    assert_eq!(left_surfaces[0]["command"], "codex --prompt 'Fix login'");
    assert_eq!(left_surfaces[0]["cwd"], "/tmp/fair-fox/api");
    assert_eq!(left_surfaces[0]["focus"], true);

    for surfaces in [
        repo_right_top_surfaces(&value),
        repo_right_bottom_surfaces(&value),
    ] {
        assert_eq!(surfaces.len(), 1);
        assert_eq!(surfaces[0]["name"], "shell");
        assert_eq!(surfaces[0]["cwd"], "/tmp/fair-fox/api");
        assert!(surfaces[0].get("command").is_none());
    }

    assert_no_empty_command(&value);
    assert_no_unresolved_vars(&value);
    assert_valid_cmux_split_tree(&value);
}

#[test]
fn default_repo_layout_without_agent_uses_three_shells() {
    let repo = vars().remove(0);
    let rendered = render_cmux_repo_layout(default_cmux_repo_layout(), &repo, None).unwrap();
    let value: Value = serde_json::from_str(&rendered).unwrap();
    let commands = collect_string_field(&value, "command");

    assert!(commands.is_empty());
    let left_surfaces = repo_left_surfaces(&value);
    assert_eq!(left_surfaces.len(), 1);
    assert_eq!(left_surfaces[0]["name"], "shell");
    assert_eq!(left_surfaces[0]["cwd"], "/tmp/fair-fox/api");
    assert_eq!(left_surfaces[0]["focus"], true);
    assert!(left_surfaces[0].get("command").is_none());

    for surfaces in [
        repo_right_top_surfaces(&value),
        repo_right_bottom_surfaces(&value),
    ] {
        assert_eq!(surfaces.len(), 1);
        assert_eq!(surfaces[0]["name"], "shell");
        assert_eq!(surfaces[0]["cwd"], "/tmp/fair-fox/api");
        assert!(surfaces[0].get("command").is_none());
    }

    assert_no_empty_command(&value);
}
```

Replace the existing repo surface helper with these three helpers:

```rust
fn repo_left_surfaces(value: &Value) -> &Vec<Value> {
    value["children"][0]["pane"]["surfaces"]
        .as_array()
        .expect("left surfaces")
}

fn repo_right_top_surfaces(value: &Value) -> &Vec<Value> {
    value["children"][1]["children"][0]["pane"]["surfaces"]
        .as_array()
        .expect("right top surfaces")
}

fn repo_right_bottom_surfaces(value: &Value) -> &Vec<Value> {
    value["children"][1]["children"][1]["pane"]["surfaces"]
        .as_array()
        .expect("right bottom surfaces")
}
```

- [x] **Step 2: Run the focused test and verify RED**

Run:

```bash
cargo test --test cmux_layout_test default_repo_layout_places_agent_left_and_two_shells_right -- --exact
```

Expected: FAIL because the current root split is `0.38`, the left surface is `lazygit`, and the agent is in the bottom-right pane.

- [x] **Step 3: Preserve custom lazygit renderer coverage**

Add this helper before the string-collection helpers in `tests/cmux_layout_test.rs`:

```rust
fn lazygit_surface_template() -> &'static str {
    r#"{
  "pane": {
    "surfaces": [
      {
        "type": "terminal",
        "name": "lazygit",
        "command": "$lazygit_command",
        "cwd": "$worktree_path"
      }
    ]
  }
}"#
}
```

In `repo_layout_passes_lazygit_config_when_present` and `repo_layout_quotes_lazygit_paths_with_spaces`, replace `default_cmux_repo_layout()` with `lazygit_surface_template()`. This keeps path quoting and `-ucf` rendering covered without requiring lazygit in the default layout.

- [x] **Step 4: Implement the minimal default template change**

Replace `default_cmux_repo_layout()` in `src/core/cmux_layout.rs` with:

```rust
pub fn default_cmux_repo_layout() -> &'static str {
    r#"{
  "direction": "horizontal",
  "split": 0.5,
  "children": [
    {
      "pane": {
        "surfaces": [
          {
            "type": "terminal",
            "name": "agent",
            "command": "$agent_command",
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

- [x] **Step 5: Run the cmux layout tests and verify GREEN**

Run:

```bash
cargo test --test cmux_layout_test
```

Expected: all `cmux_layout_test` tests pass.

If the full suite exposes the existing single-repo launch assertion that requires lazygit, change only that assertion to `assert!(!launch.repo_workspaces[0].layout.contains("lazygit"));`. Keep its agent and anchor assertions unchanged.

- [x] **Step 6: Update user-facing layout documentation**

In `README.md`, change the repo workspace bullets to:

```markdown
- Each repo workspace uses a 50/50 split: `--run-agent` runs on the left, and two regular shells are stacked on the right.
- Without `--run-agent`, the left terminal also falls back to a regular shell. The default cmux repo layout does not launch lazygit.
```

Change the cmux single-repo Agent CLI location to:

```markdown
- **1 repo** -> the repo workspace's left terminal
```

In `README.zh-CN.md`, use the matching text:

```markdown
- 每个 repo workspace 使用 50/50 分栏：使用 `--run-agent` 时 agent 在左侧运行，右侧上下各是一个普通 shell。
- 不加 `--run-agent` 时，左侧也回退为普通 shell。cmux 默认 repo 布局不再启动 lazygit。
```

Change the cmux single-repo Agent CLI location to:

```markdown
- **1 个 repo** -> repo workspace 左侧 terminal
```

In `skills/zootree-usage/references/layouts.md`, replace the two repo layout bullets with:

```markdown
- group 内每个 repo 有一个 workspace，使用 50/50 分栏：agent 在左侧，右侧上下各是一个普通 shell。
- 单 repo 时，`--run-agent` 在 repo workspace 左侧 terminal 运行 agent；不加时左侧回退为普通 shell。cmux 默认 repo 布局不启动 lazygit。
```

- [x] **Step 7: Run complete verification**

Run:

```bash
cargo fmt --check
cargo clippy -- -D warnings
cargo test
git diff --check
```

Expected: every command exits with status 0 and no warnings or whitespace errors.

- [x] **Step 8: Review scope and commit**

Confirm `git diff --stat` contains only the implementation, tests, plan/spec update, and three documentation files. Then commit:

```bash
git add src/core/cmux_layout.rs src/cli/workspace.rs tests/cmux_layout_test.rs README.md README.zh-CN.md skills/zootree-usage/references/layouts.md docs/superpowers/specs/2026-07-17-cmux-default-repo-layout-design.md docs/superpowers/plans/2026-07-17-cmux-default-repo-layout.md
git commit -m "fix(cmux): update default repo layout"
```
