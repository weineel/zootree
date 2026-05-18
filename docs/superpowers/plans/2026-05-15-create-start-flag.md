# `zootree create --start` / `--run-agent` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `--start` and `--run-agent` flags to `zootree create` so users can create a workspace and immediately enter the start flow in one command.

**Architecture:** Extend `CreateArgs` in `src/cli/workspace.rs` with two new fields whose clap configuration mirrors `StartArgs`. At the end of `handle_create`, if either flag is set, construct a `StartArgs` and call `handle_start` directly — no refactor of `handle_start` itself.

**Tech Stack:** Rust, clap (derive API), clap_complete with `ArgValueCompleter`.

---

## File Structure

- **Modify:** `src/cli/workspace.rs` — add two fields to `CreateArgs`; append a small block at the end of `handle_create`. No new files. No changes elsewhere.

---

### Task 1: Extend `CreateArgs` with `--start` and `--run-agent`

**Files:**
- Modify: `src/cli/workspace.rs` (the `CreateArgs` struct around line 552)

- [ ] **Step 1: Add the two fields to `CreateArgs`**

Locate the `CreateArgs` struct (currently ends with the `template` field around line 577). Append the two new fields **before** the closing `}`:

```rust
    #[arg(long, help = "Start the workspace immediately after creation")]
    pub start: bool,
    #[arg(
        long,
        num_args = 0..=1,
        default_missing_value = "",
        value_name = "ALIAS_OR_CMD",
        help = "Launch agent_cli in the designated pane after start (implies --start)",
        add = ArgValueCompleter::new(|c: &std::ffi::OsStr| complete_agent_cli_alias(c)),
    )]
    pub run_agent: Option<Option<String>>,
```

The clap configuration for `run_agent` is intentionally identical to the one already on `StartArgs` (around line 598). The `complete_agent_cli_alias` import already exists at the top of the file.

- [ ] **Step 2: Build to verify the new args parse cleanly**

Run: `cargo build`
Expected: succeeds with no errors. Warnings about unused fields are acceptable at this point — the next task wires them up.

- [ ] **Step 3: Commit**

```bash
git add src/cli/workspace.rs
git commit -m "feat(cli): add --start and --run-agent flags to create"
```

---

### Task 2: Trigger `handle_start` from `handle_create`

**Files:**
- Modify: `src/cli/workspace.rs` (`handle_create`, end of function around line 194)

- [ ] **Step 1: Append the start-trigger block at the end of `handle_create`**

Locate the end of `handle_create`. Currently the last statements before `Ok(())` are the `println!` calls that print `branch:` and `repos:` (around lines 184–192). Insert this block **after** those `println!` calls and **before** `Ok(())`:

```rust
    let should_start = args.start || args.run_agent.is_some();
    if should_start {
        let start_args = StartArgs {
            name: Some(name.clone()),
            no_zellij: false,
            run_agent: args.run_agent.clone(),
        };
        handle_start(&start_args)?;
    }
```

Notes:
- `name` is the local `String` already bound earlier in `handle_create` (around line 141), so `name.clone()` is correct.
- `StartArgs` is defined in the same file (around line 590); no import needed.
- Do not pass through `--no-zellij`; per the spec it stays hard-coded `false`.

- [ ] **Step 2: Build to verify**

Run: `cargo build`
Expected: succeeds with no warnings related to the new code.

- [ ] **Step 3: Run existing tests**

Run: `cargo test`
Expected: all existing tests pass (the two `warn_or_bail` tests at the bottom of the file, plus anything else in the workspace).

- [ ] **Step 4: Commit**

```bash
git add src/cli/workspace.rs
git commit -m "feat(cli): wire create --start/--run-agent into handle_start"
```

---

### Task 3: Manual verification

No new automated tests are added (the change is parameter pass-through with no isolatable pure logic — see spec §Testing). Run through this checklist manually using a scratch repo configured in `~/.config/zootree/`. After each step, verify expected behavior, then `Ctrl-C` / kill the zellij session and `zootree cancel <name>` to clean up before the next case.

- [ ] **Case 1 — regression: plain create stays pending**

Run: `cargo run -- create --title t1 --repos <some-repo>`
Expected: workspace created, status `pending`, no zellij session launched. `zootree list` shows it as pending.

- [ ] **Case 2 — `--start` only, no agent**

Run: `cargo run -- create --title t2 --repos <some-repo> --start`
Expected: workspace created, then start flow runs (worktree created, zellij session launched). No agent_cli pane runs an agent.

- [ ] **Case 3 — `--run-agent` bare implies `--start`, uses global default**

Precondition: `agent_cli` is set in `~/.config/zootree/config.toml`.
Run: `cargo run -- create --title t3 --repos <some-repo> --run-agent`
Expected: workspace created, start flow runs, agent_cli pane launches the configured global default.

- [ ] **Case 4 — `--run-agent <alias-or-cmd>` passthrough**

Run: `cargo run -- create --title t4 --repos <some-repo> --run-agent claude`
Expected: workspace created, start flow runs, agent pane launches `claude` (resolved via `agent_cli_alias` if it is an alias).

- [ ] **Case 5 — `--start` inside a zellij session bails**

Run from inside an existing zellij session: `cargo run -- create --title t5 --repos <some-repo> --start`
Expected: create succeeds, then start fails with the existing `already inside a zellij session (ZELLIJ is set)` error from `launch_zellij`. Workspace is left in `pending` status, recoverable with `zootree start <name>` from a regular terminal.

- [ ] **Step: Final commit (if any tweaks needed during verification)**

If any case revealed an issue and required a fix, commit it with a focused message. Otherwise skip.

---

## Self-Review

- **Spec coverage:** §CLI 参数 → Task 1. §行为 + 行为矩阵 + 错误处理 → Task 2 (the `should_start` branch implements all four matrix rows; `?` propagation handles error cases; ZELLIJ check inherited via `handle_start`). §测试 manual checklist → Task 3 (cases 1–5 map 1:1 to the spec's checklist). All sections covered.
- **Placeholder scan:** No TBD/TODO; every code step shows full code; every command is concrete (`cargo build`, `cargo test`, `cargo run -- ...`).
- **Type consistency:** `StartArgs` field set used in Task 2 (`name`, `no_zellij`, `run_agent`) matches the existing struct definition at line 590. `args.run_agent: Option<Option<String>>` matches what Task 1 declares. `name` is the existing `String` local in `handle_create`.
