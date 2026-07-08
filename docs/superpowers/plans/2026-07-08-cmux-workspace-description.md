# cmux Workspace Description Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Set cmux workspace description to the zootree workspace title whenever zootree creates a cmux workspace.

**Architecture:** Add launch metadata to `MultiplexerLaunch` so CLI preparation owns the source of truth for display metadata. cmux creation consumes that metadata as `--description`, while zellij and existing cmux select/close paths keep their current behavior.

**Tech Stack:** Rust, anyhow, existing `CommandRunner` / `MockRunner` test pattern, `cargo test`.

---

## File Structure

- `src/core/multiplexer/mod.rs`: extend `MultiplexerLaunch` with `description: String`.
- `src/cli/workspace.rs`: populate `MultiplexerLaunch.description` from `WorkspaceConfig.title` in both zellij and cmux preparation paths.
- `src/core/multiplexer/cmux.rs`: pass `launch.description` to `cmux workspace create --description`.
- `tests/cmux_test.rs`: update the launch helper and command assertions for cmux create and recreate.

No new source files are needed. No cmux group configuration is part of this task.

### Task 1: cmux Create Description

**Files:**
- Modify: `tests/cmux_test.rs:26-134`
- Modify: `src/core/multiplexer/mod.rs:8-16`
- Modify: `src/cli/workspace.rs:705-712`
- Modify: `src/cli/workspace.rs:828-835`
- Modify: `src/core/multiplexer/cmux.rs:89-100`

- [ ] **Step 1: Write the failing cmux command tests**

Update `tests/cmux_test.rs` helper and expected cmux argument vectors:

```rust
fn launch() -> MultiplexerLaunch {
    MultiplexerLaunch {
        workspace_name: "fair-fox".into(),
        display_name: "zootree-fair-fox".into(),
        description: "Fix cmux sidebar copy".into(),
        workspace_dir: "/tmp/fair-fox".into(),
        layout_name: "default".into(),
        rendered_layout: r#"{"pane":{"surfaces":[{"type":"terminal"}]}}"#.into(),
        layout_file: "/tmp/default.cmux.json".into(),
    }
}
```

In `launch_invokes_cmux_new_workspace`, change the expected args to:

```rust
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
    r#"{"pane":{"surfaces":[{"type":"terminal"}]}}"#,
    "--focus",
    "true",
]
```

In `launch_or_open_recreates_when_persisted_workspace_ref_cannot_be_selected`, change `calls[1].args` to the same expected create args:

```rust
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
    r#"{"pane":{"surfaces":[{"type":"terminal"}]}}"#,
    "--focus",
    "true",
]
```

- [ ] **Step 2: Run the targeted test to verify the expected failure**

Run:

```bash
cargo test --test cmux_test launch_invokes_cmux_new_workspace -- --exact
```

Expected: compile failure first, because `MultiplexerLaunch` has no `description` field yet. The relevant error should mention:

```text
struct `MultiplexerLaunch` has no field named `description`
```

- [ ] **Step 3: Add launch metadata to the shared multiplexer contract**

Modify `src/core/multiplexer/mod.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultiplexerLaunch {
    pub workspace_name: String,
    pub display_name: String,
    pub description: String,
    pub workspace_dir: PathBuf,
    pub layout_name: String,
    pub rendered_layout: String,
    pub layout_file: PathBuf,
}
```

- [ ] **Step 4: Populate launch description from workspace title**

In `src/cli/workspace.rs`, update the `MultiplexerLaunch` returned by `prepare_zellij_launch`:

```rust
Ok(MultiplexerLaunch {
    workspace_name: workspace.name.clone(),
    display_name: multiplexer_display_name(workspace),
    description: workspace.title.clone(),
    workspace_dir: ws_dir.into(),
    layout_name: layout_name.into(),
    rendered_layout: rendered,
    layout_file,
})
```

In the same file, update the `MultiplexerLaunch` returned by `prepare_cmux_launch`:

```rust
Ok(MultiplexerLaunch {
    workspace_name: workspace.name.clone(),
    display_name: multiplexer_display_name(workspace),
    description: workspace.title.clone(),
    workspace_dir: ws_dir.into(),
    layout_name: layout_name.into(),
    rendered_layout: rendered,
    layout_file,
})
```

- [ ] **Step 5: Pass description to cmux workspace creation**

Modify `src/core/multiplexer/cmux.rs` inside `launch_and_capture_workspace`:

```rust
let output = self.cmux(vec![
    "workspace".into(),
    "create".into(),
    "--name".into(),
    launch.display_name.clone(),
    "--description".into(),
    launch.description.clone(),
    "--cwd".into(),
    launch.workspace_dir.to_string_lossy().into_owned(),
    "--layout".into(),
    launch.rendered_layout.clone(),
    "--focus".into(),
    "true".into(),
])?;
```

- [ ] **Step 6: Run the targeted cmux tests**

Run:

```bash
cargo test --test cmux_test
```

Expected: all tests in `tests/cmux_test.rs` pass. The existing `open_selects_persisted_workspace_ref` test should still assert only:

```rust
vec!["workspace", "select", "workspace:7"]
```

This confirms the select/open path does not rename or update description for an existing cmux workspace.

- [ ] **Step 7: Run a broader compile/test check for affected code**

Run:

```bash
cargo test --tests
```

Expected: all integration tests pass. If unrelated tests fail, capture the failing test names and stderr before deciding whether they are baseline failures.

- [ ] **Step 8: Check formatting and whitespace**

Run:

```bash
cargo fmt --check
git diff --check
```

Expected: both commands exit successfully with no output.

- [ ] **Step 9: Commit the implementation**

Run:

```bash
git status --short
git add src/core/multiplexer/mod.rs src/core/multiplexer/cmux.rs src/cli/workspace.rs tests/cmux_test.rs
git commit -m "feat: set cmux workspace description"
```

Expected: commit includes only the implementation and tests for cmux workspace description.

## Self-Review

- Spec coverage: Task 1 covers `--description <workspace.title>` on create, keeps `--name zootree-<workspace.name>`, leaves select/open unchanged, and tests recreate-after-missing-ref.
- Placeholder scan: no unfinished markers or unspecified test steps remain.
- Type consistency: `MultiplexerLaunch.description` is introduced before production code reads it, and all plan snippets use the same field name and `String` type.
