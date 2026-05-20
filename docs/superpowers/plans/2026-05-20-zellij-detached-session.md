# Zellij Detached Session Support — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When `zootree` runs inside an existing zellij session, create the new workspace session in the background (via `zellij attach --create-background`) and print an attach hint, instead of bailing out.

**Architecture:** A pure decision function `plan_launch(in_zellij, session_exists) -> LaunchPlan` selects one of four behaviors (foreground create / foreground attach / background create / already-running hint). `ZellijOps` gains a non-interactive `start_session_background` primitive. `launch_zellij` reads `is_inside_zellij()` once at the entry layer, passes it as a `bool` parameter (for testability), and dispatches via `match LaunchPlan`. `CommandSpec` gains an `env_remove: Vec<String>` field so we can strip `ZELLIJ` / `ZELLIJ_SESSION_NAME` / `ZELLIJ_PANE_ID` when invoking zellij from inside a nested session.

**Tech Stack:** Rust, anyhow, std::process::Command, existing `CommandRunner` trait + `MockRunner`.

**Spec:** `docs/superpowers/specs/2026-05-20-zellij-detached-session-design.md`

---

## File Structure

| File | Action | Responsibility |
|---|---|---|
| `src/runner.rs` | Modify | Add `env_remove` field to `CommandSpec`; apply it in `RealRunner`; mirror in `MockRunner` |
| `src/core/zellij.rs` | Modify | Add file-level `is_inside_zellij()`, `LaunchPlan` enum, `plan_launch()` pure fn; add `ZellijOps::start_session_background` |
| `src/core/git.rs` | Modify | Add `env_remove: vec![]` to existing 4 `CommandSpec` literals |
| `src/core/hook.rs` | Modify | Add `env_remove: vec![]` to existing `CommandSpec` literal |
| `src/cli/workspace.rs` | Modify | Replace existing `bail!` + interactive `start_session` fallback with new `dispatch_launch` helper that uses `plan_launch`; thread `in_zellij` through callers |
| `tests/zellij_test.rs` | Modify | Add tests: `plan_launch` matrix, `start_session_background` shape + failure, `dispatch_launch` orchestration |

---

## Task 1: Add `env_remove` field to `CommandSpec`

Structural prep: a new field flows through, defaults to empty in all current call sites, and is record/applied correctly. Drives later tasks but causes no behavior change on its own.

**Files:**
- Modify: `src/runner.rs`
- Modify: `src/core/git.rs:17,49,116,147` (4 literals)
- Modify: `src/core/hook.rs:57` (1 literal)
- Modify: `src/core/zellij.rs:17,27` (2 literals in private helpers)
- Test: `tests/zellij_test.rs` (new test added at bottom)

- [ ] **Step 1: Write the failing test**

Append to `tests/zellij_test.rs`:

```rust
use zootree::runner::{CommandRunner, CommandSpec};
use std::collections::HashMap;

#[test]
fn mock_runner_preserves_env_remove() {
    let runner = MockRunner::new();
    runner.push_response(success_output());
    let spec = CommandSpec {
        program: "echo".into(),
        args: vec!["hi".into()],
        cwd: None,
        env: HashMap::new(),
        env_remove: vec!["FOO".into(), "BAR".into()],
    };
    runner.run(&spec).unwrap();

    let calls = runner.take_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].env_remove, vec!["FOO".to_string(), "BAR".to_string()]);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test zellij_test mock_runner_preserves_env_remove`
Expected: FAIL — `CommandSpec` has no field `env_remove` (or import errors).

- [ ] **Step 3: Add field to `CommandSpec` and propagate through both runners**

Edit `src/runner.rs`. Replace the struct and both runner methods so the file becomes:

```rust
use anyhow::Result;
use std::collections::HashMap;
use std::process::{Command, ExitStatus, Output};

pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<String>,
    pub env: HashMap<String, String>,
    pub env_remove: Vec<String>,
}

pub trait CommandRunner {
    fn run(&self, spec: &CommandSpec) -> Result<Output>;

    /// Run a command that needs direct terminal access (interactive TUI).
    /// Inherits stdin/stdout/stderr from the parent process. Returns the
    /// exit status rather than captured output.
    fn run_interactive(&self, spec: &CommandSpec) -> Result<ExitStatus>;
}

pub struct RealRunner;

impl CommandRunner for RealRunner {
    fn run(&self, spec: &CommandSpec) -> Result<Output> {
        let mut cmd = Command::new(&spec.program);
        cmd.args(&spec.args);
        if let Some(cwd) = &spec.cwd {
            cmd.current_dir(cwd);
        }
        for k in &spec.env_remove {
            cmd.env_remove(k);
        }
        for (k, v) in &spec.env {
            cmd.env(k, v);
        }
        let output = cmd.output()?;
        Ok(output)
    }

    fn run_interactive(&self, spec: &CommandSpec) -> Result<ExitStatus> {
        let mut cmd = Command::new(&spec.program);
        cmd.args(&spec.args);
        if let Some(cwd) = &spec.cwd {
            cmd.current_dir(cwd);
        }
        for k in &spec.env_remove {
            cmd.env_remove(k);
        }
        for (k, v) in &spec.env {
            cmd.env(k, v);
        }
        let status = cmd.status()?;
        Ok(status)
    }
}

pub struct MockRunner {
    pub calls: std::cell::RefCell<Vec<CommandSpec>>,
    pub responses: std::cell::RefCell<Vec<Output>>,
}

impl Default for MockRunner {
    fn default() -> Self {
        Self::new()
    }
}

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

impl CommandRunner for MockRunner {
    fn run(&self, spec: &CommandSpec) -> Result<Output> {
        self.calls.borrow_mut().push(CommandSpec {
            program: spec.program.clone(),
            args: spec.args.clone(),
            cwd: spec.cwd.clone(),
            env: spec.env.clone(),
            env_remove: spec.env_remove.clone(),
        });
        let output = self.responses.borrow_mut().remove(0);
        Ok(output)
    }

    fn run_interactive(&self, spec: &CommandSpec) -> Result<ExitStatus> {
        self.calls.borrow_mut().push(CommandSpec {
            program: spec.program.clone(),
            args: spec.args.clone(),
            cwd: spec.cwd.clone(),
            env: spec.env.clone(),
            env_remove: spec.env_remove.clone(),
        });
        let output = self.responses.borrow_mut().remove(0);
        Ok(output.status)
    }
}
```

- [ ] **Step 4: Backfill `env_remove: vec![]` in all existing `CommandSpec` literals**

There are 7 existing literals across 3 files. Each needs `env_remove: vec![],` appended to its struct body.

In `src/core/zellij.rs`, find both literals (in `fn zellij` ~line 17 and `fn zellij_interactive` ~line 27) and add the field. Each currently ends with `env: HashMap::new(),` — add `env_remove: vec![],` after it.

In `src/core/git.rs`, do the same for the 4 literals at lines 17, 49, 116, 147. Each currently ends with `env: HashMap::new(),` — add `env_remove: vec![],`.

In `src/core/hook.rs`, do the same for the literal at ~line 57.

- [ ] **Step 5: Run the failing test plus full test suite**

Run: `cargo test`
Expected: All previous tests pass; the new `mock_runner_preserves_env_remove` passes.

- [ ] **Step 6: Commit**

```bash
git add src/runner.rs src/core/zellij.rs src/core/git.rs src/core/hook.rs tests/zellij_test.rs
git commit -m "feat: add env_remove field to CommandSpec

Threads through RealRunner and MockRunner. All existing literals
backfilled with empty vec. Prep for stripping ZELLIJ env vars when
spawning zellij from inside a nested session."
```

---

## Task 2: Add pure decision module (`is_inside_zellij`, `LaunchPlan`, `plan_launch`)

Pure logic with no IO. Drives the dispatch in Task 4.

**Files:**
- Modify: `src/core/zellij.rs` (add file-level items, before `pub struct ZellijOps`)
- Test: `tests/zellij_test.rs`

- [ ] **Step 1: Write the failing tests**

Append to `tests/zellij_test.rs`:

```rust
use zootree::core::zellij::{plan_launch, LaunchPlan};

#[test]
fn plan_launch_outside_no_session_yields_foreground_create() {
    assert_eq!(plan_launch(false, false), LaunchPlan::ForegroundCreate);
}

#[test]
fn plan_launch_outside_session_exists_yields_foreground_attach() {
    assert_eq!(plan_launch(false, true), LaunchPlan::ForegroundAttach);
}

#[test]
fn plan_launch_inside_no_session_yields_background_create() {
    assert_eq!(plan_launch(true, false), LaunchPlan::BackgroundCreate);
}

#[test]
fn plan_launch_inside_session_exists_yields_already_running_hint() {
    assert_eq!(plan_launch(true, true), LaunchPlan::AlreadyRunningHint);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test zellij_test plan_launch`
Expected: FAIL — `plan_launch` and `LaunchPlan` not found.

- [ ] **Step 3: Add `is_inside_zellij`, `LaunchPlan`, and `plan_launch` to `src/core/zellij.rs`**

In `src/core/zellij.rs`, after the `use` block at the top of the file and before `pub struct ZellijOps`, insert:

```rust
pub fn is_inside_zellij() -> bool {
    std::env::var_os("ZELLIJ").is_some()
        || std::env::var_os("ZELLIJ_SESSION_NAME").is_some()
}

#[derive(Debug, PartialEq, Eq)]
pub enum LaunchPlan {
    /// Not inside a zellij session and target session does not exist —
    /// create + attach in foreground (current default behavior).
    ForegroundCreate,
    /// Not inside a zellij session but target session exists —
    /// attach to it in foreground.
    ForegroundAttach,
    /// Inside a zellij session and target session does not exist —
    /// create the target session in the background, do not attach.
    BackgroundCreate,
    /// Inside a zellij session and target session already exists —
    /// do nothing, just print a hint pointing at `zootree open`.
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
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --test zellij_test plan_launch`
Expected: PASS — all four `plan_launch_*` tests green.

- [ ] **Step 5: Commit**

```bash
git add src/core/zellij.rs tests/zellij_test.rs
git commit -m "feat: add LaunchPlan + plan_launch decision logic

Pure function mapping (in_zellij, session_exists) to one of four launch
plans. is_inside_zellij() reads ZELLIJ / ZELLIJ_SESSION_NAME env vars."
```

---

## Task 3: Add `ZellijOps::start_session_background`

Non-interactive zellij invocation that creates a detached session, with `ZELLIJ*` env stripped so it works from inside another zellij session.

**Files:**
- Modify: `src/core/zellij.rs` (add method to `impl ZellijOps`)
- Test: `tests/zellij_test.rs`

- [ ] **Step 1: Write the failing tests**

Append to `tests/zellij_test.rs`:

```rust
use std::path::Path;

fn failure_output(stderr: &str) -> Output {
    Output {
        status: ExitStatus::from_raw(1 << 8), // wait-status: exit code 1
        stdout: Vec::new(),
        stderr: stderr.as_bytes().to_vec(),
    }
}

#[test]
fn start_session_background_invokes_zellij_with_correct_args_and_env_remove() {
    let runner = MockRunner::new();
    runner.push_response(success_output());
    let zellij = ZellijOps::new(&runner);

    zellij
        .start_session_background("ws-foo", Path::new("/tmp/layout.kdl"))
        .unwrap();

    let calls = runner.take_calls();
    assert_eq!(calls.len(), 1);
    let c = &calls[0];
    assert_eq!(c.program, "zellij");
    assert_eq!(
        c.args,
        vec![
            "-l",
            "/tmp/layout.kdl",
            "attach",
            "--create-background",
            "ws-foo"
        ]
    );
    assert!(c.env_remove.iter().any(|k| k == "ZELLIJ"));
    assert!(c.env_remove.iter().any(|k| k == "ZELLIJ_SESSION_NAME"));
    assert!(c.env_remove.iter().any(|k| k == "ZELLIJ_PANE_ID"));
}

#[test]
fn start_session_background_propagates_failure_with_stderr() {
    let runner = MockRunner::new();
    runner.push_response(failure_output("zellij: layout parse error"));
    let zellij = ZellijOps::new(&runner);

    let err = zellij
        .start_session_background("ws-foo", Path::new("/tmp/layout.kdl"))
        .unwrap_err();
    let msg = format!("{}", err);
    assert!(
        msg.contains("layout parse error"),
        "expected stderr propagated, got: {}",
        msg
    );
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test zellij_test start_session_background`
Expected: FAIL — method `start_session_background` not found on `ZellijOps`.

- [ ] **Step 3: Implement `start_session_background`**

In `src/core/zellij.rs`, inside `impl<'a, R: CommandRunner> ZellijOps<'a, R>`, add this method (place it near `start_session`):

```rust
pub fn start_session_background(
    &self,
    session_name: &str,
    layout_path: &Path,
) -> Result<()> {
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

(Imports `CommandSpec`, `HashMap`, `bail`, `info`, `Path` are already in scope based on the file's existing `use`s — verify they cover this; if any is missing, add it.)

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --test zellij_test start_session_background`
Expected: PASS — both new tests green.

- [ ] **Step 5: Commit**

```bash
git add src/core/zellij.rs tests/zellij_test.rs
git commit -m "feat: add ZellijOps::start_session_background

Wraps 'zellij -l <layout> attach --create-background <name>' as a
non-interactive call. Strips ZELLIJ / ZELLIJ_SESSION_NAME /
ZELLIJ_PANE_ID so the spawn works from inside a nested zellij session.
Propagates stderr on non-zero exit."
```

---

## Task 4: Refactor `launch_zellij` to dispatch via `LaunchPlan`

Replaces the existing `bail!` (line 441) and the `start_session` → `attach_session` implicit fallback (lines 551-557) with an explicit four-way match. To keep dispatch testable without the full `launch_zellij` fixture overhead (ConfigManager + tempdir + on-disk layout file), extract the dispatch portion into a small pure-ish helper `dispatch_launch` that receives all already-computed inputs. `launch_zellij` keeps prep work (layout render, file write, session_name compute) and ends by calling `dispatch_launch`.

**Files:**
- Modify: `src/cli/workspace.rs` (function signature + body + 2 call sites; new helper)
- Test: `tests/zellij_test.rs` (new `dispatch_launch` integration tests targeting the small helper)

### Test setup investigation

Current `tests/start_agent_test.rs` does not invoke `launch_zellij` directly — it only tests layout rendering. Building a fixture for the full `launch_zellij` would require a tempdir-backed `ConfigManager` plus on-disk workspace state. To avoid that cost while still gaining regression protection on the dispatch decision, we test `dispatch_launch` (signature designed below) using `MockRunner`. The `plan_launch` matrix is already covered in Task 2; here we verify the dispatch wiring matches that decision.

- [ ] **Step 1: Write the failing tests**

Append to `tests/zellij_test.rs`:

```rust
fn stdout_output(stdout: &[u8]) -> Output {
    Output {
        status: ExitStatus::from_raw(0),
        stdout: stdout.to_vec(),
        stderr: Vec::new(),
    }
}

#[test]
fn dispatch_launch_inside_zellij_no_session_creates_background() {
    use zootree::cli::workspace::dispatch_launch;
    let runner = MockRunner::new();
    // session_exists -> list-sessions returns lines without our session
    runner.push_response(stdout_output(b"other-session\n"));
    // start_session_background succeeds
    runner.push_response(success_output());

    let zellij = ZellijOps::new(&runner);
    dispatch_launch(
        &zellij,
        "fair-fox",                  // workspace_name (used in printed hint)
        "zootree-fair-fox",          // session_name
        Path::new("/tmp/layout.kdl"),
        true,                        // in_zellij
    )
    .unwrap();

    let calls = runner.take_calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].args, vec!["list-sessions"]);
    assert_eq!(calls[1].program, "zellij");
    assert!(calls[1].args.contains(&"--create-background".to_string()));
    assert!(calls[1].args.contains(&"zootree-fair-fox".to_string()));
}

#[test]
fn dispatch_launch_inside_zellij_session_exists_invokes_only_list_sessions() {
    use zootree::cli::workspace::dispatch_launch;
    let runner = MockRunner::new();
    // list-sessions includes our session
    runner.push_response(stdout_output(b"zootree-fair-fox\nother-session\n"));

    let zellij = ZellijOps::new(&runner);
    dispatch_launch(
        &zellij,
        "fair-fox",
        "zootree-fair-fox",
        Path::new("/tmp/layout.kdl"),
        true,
    )
    .unwrap();

    let calls = runner.take_calls();
    assert_eq!(calls.len(), 1, "should only call list-sessions, no follow-up");
    assert_eq!(calls[0].args, vec!["list-sessions"]);
}

#[test]
fn dispatch_launch_outside_zellij_no_session_calls_start_session() {
    use zootree::cli::workspace::dispatch_launch;
    let runner = MockRunner::new();
    runner.push_response(stdout_output(b""));    // list-sessions empty
    runner.push_response(success_output());      // start_session (interactive) ok

    let zellij = ZellijOps::new(&runner);
    dispatch_launch(
        &zellij,
        "fair-fox",
        "zootree-fair-fox",
        Path::new("/tmp/layout.kdl"),
        false,
    )
    .unwrap();

    let calls = runner.take_calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].args, vec!["list-sessions"]);
    assert!(
        calls[1]
            .args
            .iter()
            .any(|a| a == "--new-session-with-layout"),
        "expected start_session via --new-session-with-layout, got {:?}",
        calls[1].args
    );
}
```

(The fourth case — outside zellij, session exists, calls `attach_session` — is already exercised indirectly because `plan_launch` is unit-tested in Task 2, and `attach_session` is unchanged. Skip the redundant test.)

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test zellij_test dispatch_launch`
Expected: FAIL — `dispatch_launch` not found / not public.

- [ ] **Step 3: Extract `dispatch_launch` helper and update `launch_zellij`**

In `src/cli/workspace.rs`, do these three edits:

**(a)** Add the new `dispatch_launch` helper near `launch_zellij`. Make it `pub` so the test crate can reach it:

```rust
pub fn dispatch_launch<R: zootree::runner::CommandRunner>(
    zellij: &zootree::core::zellij::ZellijOps<R>,
    workspace_name: &str,
    session_name: &str,
    layout_file: &std::path::Path,
    in_zellij: bool,
) -> anyhow::Result<()> {
    let session_exists = zellij.session_exists(session_name)?;
    let plan = zootree::core::zellij::plan_launch(in_zellij, session_exists);

    match plan {
        zootree::core::zellij::LaunchPlan::ForegroundCreate => {
            zellij.start_session(session_name, layout_file)?;
        }
        zootree::core::zellij::LaunchPlan::ForegroundAttach => {
            zellij.attach_session(session_name)?;
        }
        zootree::core::zellij::LaunchPlan::BackgroundCreate => {
            zellij.start_session_background(session_name, layout_file)?;
            println!(
                "zellij session '{}' is running in background.",
                session_name
            );
            println!(
                "Run `zootree open {}` (outside zellij) to attach.",
                workspace_name
            );
        }
        zootree::core::zellij::LaunchPlan::AlreadyRunningHint => {
            println!("zellij session '{}' already exists.", session_name);
            println!(
                "Run `zootree open {}` (outside zellij) to attach.",
                workspace_name
            );
        }
    }

    Ok(())
}
```

(Adjust `zootree::` prefixes to match existing imports in the file. If `CommandRunner`, `ZellijOps`, `LaunchPlan`, `plan_launch` are already imported via `use`, drop the prefix.)

**(b)** Modify `launch_zellij`:

- Delete the bail at lines 441-446:

```rust
// REMOVE:
if std::env::var("ZELLIJ").is_ok() {
    anyhow::bail!(
        "already inside a zellij session (ZELLIJ is set); cannot start a new session. \
         Use a regular terminal to run 'zootree start'"
    );
}
```

- Add `in_zellij: bool` parameter to the signature:

```rust
fn launch_zellij(
    config_mgr: &ConfigManager,
    global: &crate::config::global::GlobalConfig,
    workspace: &WorkspaceConfig,
    runner: &RealRunner,
    run_agent: Option<Option<String>>,
    in_zellij: bool,
) -> Result<()> {
```

- Replace the tail (lines 551-557, the `match zellij.start_session(...)` block) with a single call to `dispatch_launch`:

```rust
dispatch_launch(&zellij, &workspace.name, &session_name, &layout_file, in_zellij)?;
Ok(())
```

**(c)** Update both `launch_zellij` call sites to pass `is_inside_zellij()`:

At ~line 334 (start command):
```rust
launch_zellij(
    &config_mgr,
    &global,
    &workspace,
    &runner,
    args.run_agent.clone(),
    crate::core::zellij::is_inside_zellij(),
)?;
```

At ~line 422 (open command):
```rust
launch_zellij(
    &config_mgr,
    &global,
    &workspace,
    &runner,
    None,
    crate::core::zellij::is_inside_zellij(),
)?;
```

- [ ] **Step 4: Run new + full tests**

Run: `cargo test`
Expected: New `dispatch_launch_*` tests pass. All previously green tests still pass.

If a previously green test now fails because it asserted against the old `bail!` message, that test is asserting deprecated behavior — read it carefully and update it (or remove if redundant). Don't paper over genuine regressions — investigate if a non-zellij test fails.

- [ ] **Step 5: Commit**

```bash
git add src/cli/workspace.rs tests/zellij_test.rs
git commit -m "feat: nested-aware launch via dispatch_launch helper

Replaces the hard 'cannot start inside zellij' bail and the implicit
start_session->attach_session fallback with an explicit plan_launch
match in a new dispatch_launch helper. launch_zellij keeps prep work
and ends by calling dispatch_launch. When inside a zellij session, the
new workspace session is created in the background and an attach hint
is printed; when outside, behavior is unchanged."
```

---

## Task 5: Full test pass + manual smoke

- [ ] **Step 1: Run the full test suite**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 2: Run clippy if the project uses it**

Run: `cargo clippy --all-targets -- -D warnings`
Expected: No warnings. (If the project doesn't gate on clippy, skip — check `mise.toml` / CI config.)

- [ ] **Step 3: Manual smoke — outside zellij (foreground path)**

In a regular terminal (not inside zellij), pick or create a workspace named `<ws>` and run:

```bash
zootree start <ws>
```

Expected: zellij takes over the terminal as before (current behavior preserved).

Detach with the usual zellij keybind (default `Ctrl-o d`).

- [ ] **Step 4: Manual smoke — inside zellij (background path)**

From inside a zellij session, with the same workspace **not currently running** as a zellij session (use `zellij ls` to confirm or `zellij delete-session zootree-<ws> --force` if needed), run:

```bash
zootree start <ws>
```

Expected output (approximate):
```
zellij session 'zootree-<ws>' is running in background.
Run `zootree open <ws>` (outside zellij) to attach.
```

Verify: `zellij ls` should now list `zootree-<ws>` as an active session. Then in a separate terminal (outside zellij), run `zootree open <ws>` and confirm you can attach and see the panes (and any agent output if `--run-agent` was used).

- [ ] **Step 5: Manual smoke — already-running path**

Still inside zellij, with `zootree-<ws>` already running from Step 4, run again:

```bash
zootree start <ws>
```

Expected output (approximate):
```
zellij session 'zootree-<ws>' already exists.
Run `zootree open <ws>` (outside zellij) to attach.
```

No new zellij command should be invoked beyond `list-sessions`.

- [ ] **Step 6: Commit any final tweaks (if needed)**

If clippy or smoke tests surfaced minor fixes, commit them as a follow-up:

```bash
git add -A
git commit -m "chore: post-smoke cleanups for nested-aware launch"
```

---

## Out of scope (deferred — do not do in this plan)

- `--background` flag to force background creation outside zellij.
- New `zootree attach <name>` subcommand (`open` already covers).
- Smart routing for `add_tab` (`workspace add-repo`).
- Resurrection UX for dead zellij sessions.
