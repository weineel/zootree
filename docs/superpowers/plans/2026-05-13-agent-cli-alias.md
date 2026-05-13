# `agent_cli_alias` 与 `--run-agent` 接收值 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let users register multiple agent_cli templates by name in `agent_cli_alias`, pass an alias-or-literal value to `--run-agent`, and complete alias names via shell.

**Architecture:** Add a `BTreeMap<String, String>` field `agent_cli_alias` to `GlobalConfig`. Promote `StartArgs.run_agent` from `bool` to `Option<Option<String>>`. Add a one-level `resolve_agent_cli` helper in `core/layout.rs` that both the config's `agent_cli` field and the CLI's `--run-agent` value flow through. Add a dynamic completer that reads the alias map and marks the entry matching `agent_cli` with `(default)`.

**Tech Stack:** Rust, `clap` (4, derive) + `clap_complete` (4, unstable-dynamic), `serde` + `toml`, `BTreeMap`, existing `MockRunner` test infrastructure.

---

## File Structure

| File | Change | Responsibility |
|------|--------|----------------|
| `src/config/global.rs` | Modify | Add `agent_cli_alias: BTreeMap<String, String>` field with serde defaults |
| `src/core/layout.rs` | Modify | Add `resolve_agent_cli(&str, &BTreeMap) -> &str` |
| `src/core/completers.rs` | Modify | Add `complete_agent_cli_alias(_with)` |
| `src/cli/workspace.rs` | Modify | Change `StartArgs.run_agent` type, change `launch_zellij` signature, route through `resolve_agent_cli` |
| `tests/config_test.rs` | Modify | Add 3 tests for alias map (de)serialization |
| `tests/agent_cli_test.rs` | Modify | Add 4 tests for `resolve_agent_cli` |
| `tests/start_agent_test.rs` | Modify | Update helper signature, add 4 alias-routing tests |
| `tests/completions_test.rs` | Modify | Add 5 tests for the new completer |
| `README.md` + `README.zh-CN.md` | Modify | Document `agent_cli_alias` and `--run-agent <ALIAS>` |
| `skills/zootree-usage/SKILL.md` | Modify | Update agent_cli section with alias concept |

---

## Task 1: Add `agent_cli_alias` field to `GlobalConfig`

**Files:**
- Modify: `src/config/global.rs:57-94`
- Test: `tests/config_test.rs`

- [ ] **Step 1: Write the failing test for default empty map**

Add this to `tests/config_test.rs` near the other `test_parse_global_config_*` tests:

```rust
#[test]
fn test_parse_global_config_agent_cli_alias() {
    let toml_str = r#"
agent_cli = "claude"

[agent_cli_alias]
claude = "claude --dangerously-skip-permissions -- $prompt"
gemini = "gemini chat -- $prompt"
"#;
    let config: GlobalConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.agent_cli.as_deref(), Some("claude"));
    assert_eq!(config.agent_cli_alias.len(), 2);
    assert_eq!(
        config.agent_cli_alias.get("claude").map(String::as_str),
        Some("claude --dangerously-skip-permissions -- $prompt")
    );
    assert_eq!(
        config.agent_cli_alias.get("gemini").map(String::as_str),
        Some("gemini chat -- $prompt")
    );
}

#[test]
fn test_parse_global_config_agent_cli_alias_default_empty() {
    let toml_str = "";
    let config: GlobalConfig = toml::from_str(toml_str).unwrap();
    assert!(config.agent_cli_alias.is_empty());
}

#[test]
fn test_serialize_global_config_agent_cli_alias_empty_omitted() {
    let mut cfg = GlobalConfig::default();
    cfg.agent_cli = Some("claude -- $prompt".into());
    let s = toml::to_string(&cfg).unwrap();
    assert!(
        !s.contains("agent_cli_alias"),
        "empty map should be skipped during serialization, got: {}",
        s
    );
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test config_test agent_cli_alias`
Expected: compilation error (`no field 'agent_cli_alias' on type 'GlobalConfig'`)

- [ ] **Step 3: Add the field**

Modify `src/config/global.rs`. Add `BTreeMap` import and the new field with proper serde attrs.

At top of file (after existing `use serde...` line):
```rust
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
```

In the `GlobalConfig` struct, add the new field after `agent_cli`:
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_cli: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub agent_cli_alias: BTreeMap<String, String>,
}
```

In the `Default` impl, add the new field:
```rust
impl Default for GlobalConfig {
    fn default() -> Self {
        Self {
            zellij: ZellijConfig::default(),
            workspace_root: default_workspace_root(),
            branch_prefix: default_branch_prefix(),
            copy_files: Vec::new(),
            hooks: HooksConfig::default(),
            log: LogConfig::default(),
            agent_cli: None,
            agent_cli_alias: BTreeMap::new(),
        }
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --test config_test agent_cli_alias`
Expected: 3 PASS

Run: `cargo test --test config_test`
Expected: all existing config tests still pass.

- [ ] **Step 5: Commit**

```bash
git add src/config/global.rs tests/config_test.rs
git commit -m "feat(config): add agent_cli_alias map to GlobalConfig

Empty map is skipped during serialization. Used by upcoming alias
resolver."
```

---

## Task 2: Add `resolve_agent_cli` helper in `core/layout.rs`

**Files:**
- Modify: `src/core/layout.rs` (after `build_agent_cli_kdl`)
- Test: `tests/agent_cli_test.rs`

- [ ] **Step 1: Write failing tests**

Append to `tests/agent_cli_test.rs`:

```rust
use std::collections::BTreeMap;
use zootree::core::layout::resolve_agent_cli;

#[test]
fn resolve_returns_alias_value_when_key_matches() {
    let mut map = BTreeMap::new();
    map.insert("safe".to_string(), "claude -- $prompt".to_string());
    assert_eq!(resolve_agent_cli("safe", &map), "claude -- $prompt");
}

#[test]
fn resolve_returns_input_when_key_missing() {
    let mut map = BTreeMap::new();
    map.insert("safe".to_string(), "claude -- $prompt".to_string());
    assert_eq!(
        resolve_agent_cli("gemini chat -- $prompt", &map),
        "gemini chat -- $prompt"
    );
}

#[test]
fn resolve_returns_input_with_empty_alias_map() {
    let map = BTreeMap::new();
    assert_eq!(resolve_agent_cli("anything", &map), "anything");
}

#[test]
fn resolve_does_not_chain_aliases() {
    let mut map = BTreeMap::new();
    map.insert("a".to_string(), "b".to_string());
    map.insert("b".to_string(), "real -- $prompt".to_string());
    // resolve("a") returns "b" (the literal value), NOT "real -- $prompt"
    assert_eq!(resolve_agent_cli("a", &map), "b");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test agent_cli_test resolve_`
Expected: compile error (`no function 'resolve_agent_cli' in module 'core::layout'`)

- [ ] **Step 3: Add the function**

Edit `src/core/layout.rs`. At the top, add `BTreeMap` import:

```rust
use crate::config::workspace::WorkspaceConfig;
use std::collections::BTreeMap;
```

Append after `build_agent_cli_kdl` (at end of file):

```rust
/// Resolve an agent_cli value against the alias map (single level).
///
/// If `value` is a key in `alias_map`, returns the alias's template; otherwise
/// returns `value` unchanged so it can be used as a literal command string.
pub fn resolve_agent_cli<'a>(
    value: &'a str,
    alias_map: &'a BTreeMap<String, String>,
) -> &'a str {
    alias_map.get(value).map(String::as_str).unwrap_or(value)
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --test agent_cli_test resolve_`
Expected: 4 PASS

Run: `cargo test --test agent_cli_test`
Expected: all existing agent_cli tests still pass.

- [ ] **Step 5: Commit**

```bash
git add src/core/layout.rs tests/agent_cli_test.rs
git commit -m "feat(layout): add resolve_agent_cli helper

Single-level lookup: returns alias template if value is a key, else
returns the input string unchanged."
```

---

## Task 3: Change `StartArgs.run_agent` type and route through resolver

This task changes the public API of `StartArgs` and the internal signature of `launch_zellij`. The completer is wired in but its function body is added in Task 4 — for this task, use `clap_complete::ArgValueCompleter::new(|_| Vec::new())` as a placeholder so we can compile. Task 4 will replace the placeholder.

**Files:**
- Modify: `src/cli/workspace.rs` — `StartArgs` struct (line 570-581), `handle_start` call site (line 322-324), `launch_zellij` signature + body (line 417-506)
- Test: `tests/start_agent_test.rs`

- [ ] **Step 1: Update the test helper signature in `tests/start_agent_test.rs`**

Replace the existing `render_with_rule` helper to accept the new triple-state input plus an alias map. Keep all 5 existing test bodies working by adapting their calls (they currently pass `bool` + `Option<&str>`; we'll wrap into the new shape).

Replace the helper at `tests/start_agent_test.rs:25-66`:

```rust
fn render_with_rule(
    workspace: &WorkspaceConfig,
    run_agent: Option<Option<&str>>,
    agent_cli: Option<&str>,
    alias_map: &std::collections::BTreeMap<String, String>,
) -> anyhow::Result<String> {
    let agent_cli_tpl: Option<String> = match run_agent {
        None => None,
        Some(value) => {
            let raw = match value {
                Some(s) if !s.is_empty() => s,
                _ => agent_cli.ok_or_else(|| {
                    anyhow::anyhow!("--run-agent requires agent_cli in global config")
                })?,
            };
            let resolved = zootree::core::layout::resolve_agent_cli(raw, alias_map);
            Some(resolved.to_string())
        }
    };

    let (overview_kdl, repo_kdl_for_first) = match agent_cli_tpl.as_deref() {
        None => (String::new(), String::new()),
        Some(tpl) => {
            let prompt = build_prompt(workspace);
            let kdl = build_agent_cli_kdl(tpl, &prompt)?;
            if workspace.repos.len() == 1 {
                (String::new(), kdl)
            } else {
                (kdl, String::new())
            }
        }
    };

    let mut vars = Vec::new();
    for (i, repo) in workspace.repos.iter().enumerate() {
        vars.push(LayoutVar {
            repo_name: repo.name.clone(),
            worktree_path: format!("{}/{}", workspace.workspace_dir, repo.name),
            branch: workspace.branch.clone(),
            workspace_name: workspace.name.clone(),
            workspace_dir: workspace.workspace_dir.clone(),
            lazygit_config: String::new(),
            overview_agent_cli: overview_kdl.clone(),
            repo_agent_cli: if i == 0 {
                repo_kdl_for_first.clone()
            } else {
                String::new()
            },
        });
    }

    Ok(LayoutRenderer::render(
        LayoutRenderer::default_layout(),
        &vars,
    ))
}
```

Update the 5 existing test bodies to pass an empty `BTreeMap` and the new run_agent shape:

`run_agent_with_one_repo_injects_into_repo_pane_only`:
```rust
let map = std::collections::BTreeMap::new();
let rendered = render_with_rule(
    &ws,
    Some(Some("")),
    Some("claude --dangerously-skip-permissions -- $prompt"),
    &map,
)
.unwrap();
```

`run_agent_with_two_repos_injects_into_overview_only`:
```rust
let map = std::collections::BTreeMap::new();
let rendered = render_with_rule(&ws, Some(Some("")), Some("claude -- $prompt"), &map).unwrap();
```

`no_run_agent_keeps_layout_clean`:
```rust
let map = std::collections::BTreeMap::new();
let rendered = render_with_rule(&ws, None, Some("claude -- $prompt"), &map).unwrap();
```

`run_agent_without_agent_cli_errors`:
```rust
let map = std::collections::BTreeMap::new();
let err = render_with_rule(&ws, Some(Some("")), None, &map).unwrap_err();
```

`agent_cli_with_embedded_prompt_token`:
```rust
let map = std::collections::BTreeMap::new();
let rendered = render_with_rule(&ws, Some(Some("")), Some("claude --prompt=$prompt"), &map).unwrap();
```

- [ ] **Step 2: Run helper tests to verify the existing 5 still pass**

Run: `cargo test --test start_agent_test`
Expected: 5 PASS (the helper was changed but semantics preserved).

- [ ] **Step 3: Change `StartArgs.run_agent` type + add placeholder completer**

Edit `src/cli/workspace.rs`. Replace lines 579-580 (`StartArgs.run_agent` field) with:

```rust
    #[arg(
        long,
        num_args = 0..=1,
        default_missing_value = "",
        value_name = "ALIAS_OR_CMD",
        help = "Launch agent_cli in the designated pane (alias name or literal command)",
        add = ArgValueCompleter::new(|c: &std::ffi::OsStr| zootree_complete_agent_cli_placeholder(c)),
    )]
    pub run_agent: Option<Option<String>>,
```

Add a placeholder completer near the top of `src/cli/workspace.rs` (right after the `use` block, around line 21):

```rust
fn zootree_complete_agent_cli_placeholder(
    _current: &std::ffi::OsStr,
) -> Vec<clap_complete::CompletionCandidate> {
    Vec::new()
}
```

(Task 4 will delete this placeholder and switch to the real completer.)

- [ ] **Step 4: Update `handle_start` call site (line 322-324)**

Find the call:
```rust
if !args.no_zellij {
    launch_zellij(&config_mgr, &global, &workspace, &runner, args.run_agent)?;
}
```

Change to:
```rust
if !args.no_zellij {
    launch_zellij(&config_mgr, &global, &workspace, &runner, args.run_agent.clone())?;
}
```

(`Option<Option<String>>` is not `Copy`, so we clone — `StartArgs` is consumed once.)

- [ ] **Step 5: Update `launch_zellij` signature + body**

Change the signature at line 417-423:
```rust
fn launch_zellij(
    config_mgr: &ConfigManager,
    global: &crate::config::global::GlobalConfig,
    workspace: &WorkspaceConfig,
    runner: &RealRunner,
    run_agent: Option<Option<String>>,
) -> Result<()> {
```

Replace the agent block at lines 456-471:
```rust
    let agent_cli_tpl: Option<String> = match run_agent.as_ref() {
        None => None,
        Some(value) => {
            let raw_owned;
            let raw: &str = match value.as_deref() {
                Some(s) if !s.is_empty() => s,
                _ => {
                    raw_owned = global.agent_cli.as_deref().ok_or_else(|| {
                        anyhow::anyhow!(
                            "--run-agent requires agent_cli in global config (~/.config/zootree/config.toml)"
                        )
                    })?;
                    raw_owned
                }
            };
            let resolved = crate::core::layout::resolve_agent_cli(raw, &global.agent_cli_alias);
            Some(resolved.to_string())
        }
    };

    let (overview_kdl, repo_kdl_for_first) = match agent_cli_tpl.as_deref() {
        None => (String::new(), String::new()),
        Some(tpl) => {
            let prompt = crate::core::layout::build_prompt(workspace);
            let kdl = crate::core::layout::build_agent_cli_kdl(tpl, &prompt)?;
            if workspace.repos.len() == 1 {
                (String::new(), kdl)
            } else {
                (kdl, String::new())
            }
        }
    };
```

Replace the warning block at lines 498-506:
```rust
    if run_agent.is_some()
        && !template_content.contains("$overview_agent_cli")
        && !template_content.contains("$repo_agent_cli")
    {
        tracing::warn!(
            "--run-agent is set but layout '{}' contains no $overview_agent_cli or $repo_agent_cli placeholder; agent_cli will not be executed",
            layout_name
        );
    }
```

- [ ] **Step 6: Run all tests + verify behavior**

Run: `cargo build`
Expected: compiles clean.

Run: `cargo test --test start_agent_test`
Expected: 5 PASS (existing tests still green with new shape).

Run: `cargo test`
Expected: full suite passes.

- [ ] **Step 7: Add 4 new alias-routing tests**

Append to `tests/start_agent_test.rs`:

```rust
fn alias_map(pairs: &[(&str, &str)]) -> std::collections::BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[test]
fn run_agent_with_alias_resolves_template() {
    let ws = make_workspace(vec!["frontend"]);
    let map = alias_map(&[("safe", "claude -- $prompt")]);
    let rendered = render_with_rule(&ws, Some(Some("safe")), None, &map).unwrap();

    let (_, repo_section) = split_overview_and_repo_tabs(&rendered);
    assert!(
        repo_section.contains(r#"command="claude""#),
        "alias 'safe' resolved to claude template: {}",
        repo_section
    );
}

#[test]
fn run_agent_with_unknown_alias_falls_back_to_literal() {
    let ws = make_workspace(vec!["frontend"]);
    let map = alias_map(&[("safe", "ignored -- $prompt")]);
    let rendered =
        render_with_rule(&ws, Some(Some("gemini chat -- $prompt")), None, &map).unwrap();

    let (_, repo_section) = split_overview_and_repo_tabs(&rendered);
    assert!(
        repo_section.contains(r#"command="gemini""#),
        "literal command used: {}",
        repo_section
    );
    assert!(
        !repo_section.contains("ignored"),
        "alias 'safe' value not used: {}",
        repo_section
    );
}

#[test]
fn run_agent_alias_template_takes_precedence_over_agent_cli() {
    let ws = make_workspace(vec!["frontend"]);
    let map = alias_map(&[("bar", "bar -- $prompt")]);
    let rendered = render_with_rule(
        &ws,
        Some(Some("bar")),
        Some("foo -- $prompt"),
        &map,
    )
    .unwrap();

    let (_, repo_section) = split_overview_and_repo_tabs(&rendered);
    assert!(
        repo_section.contains(r#"command="bar""#),
        "alias takes precedence: {}",
        repo_section
    );
    assert!(
        !repo_section.contains(r#"command="foo""#),
        "agent_cli not used: {}",
        repo_section
    );
}

#[test]
fn run_agent_bare_with_agent_cli_uses_default() {
    let ws = make_workspace(vec!["frontend"]);
    let map: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
    let rendered = render_with_rule(
        &ws,
        Some(Some("")),
        Some("claude -- $prompt"),
        &map,
    )
    .unwrap();

    let (_, repo_section) = split_overview_and_repo_tabs(&rendered);
    assert!(
        repo_section.contains(r#"command="claude""#),
        "bare flag uses agent_cli: {}",
        repo_section
    );
}
```

- [ ] **Step 8: Run new tests**

Run: `cargo test --test start_agent_test`
Expected: 9 PASS (5 existing + 4 new).

- [ ] **Step 9: Commit**

```bash
git add src/cli/workspace.rs tests/start_agent_test.rs
git commit -m "feat(start): --run-agent accepts alias name or literal command

StartArgs.run_agent is now Option<Option<String>>. Bare --run-agent
falls back to global agent_cli; values flow through resolve_agent_cli
to look up alias entries (one level)."
```

---

## Task 4: Add `complete_agent_cli_alias` completer + tests

**Files:**
- Modify: `src/core/completers.rs`
- Modify: `src/cli/workspace.rs` (replace placeholder, remove placeholder fn)
- Test: `tests/completions_test.rs`

- [ ] **Step 1: Write failing tests**

Append to `tests/completions_test.rs`:

```rust
use zootree::config::global::GlobalConfig;
use zootree::core::completers::complete_agent_cli_alias_with;

fn save_global(mgr: &ConfigManager, cfg: &GlobalConfig) {
    mgr.save_global_config(cfg).unwrap();
}

#[test]
fn agent_cli_alias_completer_returns_all_when_no_prefix() {
    let (_tmp, mgr) = make_mgr();
    let mut cfg = GlobalConfig::default();
    cfg.agent_cli_alias.insert("claude".into(), "claude -- $prompt".into());
    cfg.agent_cli_alias.insert("gemini".into(), "gemini -- $prompt".into());
    cfg.agent_cli_alias.insert("codex".into(), "codex -- $prompt".into());
    save_global(&mgr, &cfg);

    let cands = complete_agent_cli_alias_with(&mgr, OsStr::new(""));
    let n = names(&cands);
    assert_eq!(n, vec!["claude", "codex", "gemini"]); // BTreeMap order
}

#[test]
fn agent_cli_alias_completer_filters_by_prefix() {
    let (_tmp, mgr) = make_mgr();
    let mut cfg = GlobalConfig::default();
    cfg.agent_cli_alias.insert("claude".into(), "x".into());
    cfg.agent_cli_alias.insert("claude-safe".into(), "y".into());
    cfg.agent_cli_alias.insert("gemini".into(), "z".into());
    save_global(&mgr, &cfg);

    let cands = complete_agent_cli_alias_with(&mgr, OsStr::new("claude"));
    let n = names(&cands);
    assert_eq!(n, vec!["claude", "claude-safe"]);
}

#[test]
fn agent_cli_alias_completer_marks_default_when_agent_cli_matches_alias_key() {
    let (_tmp, mgr) = make_mgr();
    let mut cfg = GlobalConfig::default();
    cfg.agent_cli = Some("claude".into());
    cfg.agent_cli_alias.insert("claude".into(), "claude -- $prompt".into());
    cfg.agent_cli_alias.insert("gemini".into(), "gemini -- $prompt".into());
    save_global(&mgr, &cfg);

    let cands = complete_agent_cli_alias_with(&mgr, OsStr::new(""));
    let claude = cands
        .iter()
        .find(|c| c.get_value().to_string_lossy() == "claude")
        .expect("claude candidate present");
    let claude_help = claude.get_help().expect("help text").to_string();
    assert!(
        claude_help.starts_with("(default)"),
        "claude marked as default: {}",
        claude_help
    );

    let gemini = cands
        .iter()
        .find(|c| c.get_value().to_string_lossy() == "gemini")
        .expect("gemini candidate present");
    let gemini_help = gemini.get_help().expect("help text").to_string();
    assert!(
        !gemini_help.starts_with("(default)"),
        "gemini not marked: {}",
        gemini_help
    );
}

#[test]
fn agent_cli_alias_completer_no_default_marker_when_agent_cli_is_literal() {
    let (_tmp, mgr) = make_mgr();
    let mut cfg = GlobalConfig::default();
    cfg.agent_cli = Some("claude --skip -- $prompt".into());
    cfg.agent_cli_alias.insert("claude".into(), "claude -- $prompt".into());
    save_global(&mgr, &cfg);

    let cands = complete_agent_cli_alias_with(&mgr, OsStr::new(""));
    for c in &cands {
        let h = c.get_help().map(|h| h.to_string()).unwrap_or_default();
        assert!(
            !h.starts_with("(default)"),
            "no candidate marked default when agent_cli is literal: {}",
            h
        );
    }
}

#[test]
fn agent_cli_alias_completer_empty_when_no_aliases() {
    let (_tmp, mgr) = make_mgr();
    let cfg = GlobalConfig::default();
    save_global(&mgr, &cfg);

    let cands = complete_agent_cli_alias_with(&mgr, OsStr::new(""));
    assert!(cands.is_empty());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test completions_test agent_cli_alias_completer`
Expected: compile error (`no function 'complete_agent_cli_alias_with' in module 'core::completers'`).

- [ ] **Step 3: Add the completer**

Append to `src/core/completers.rs`:

```rust
pub fn complete_agent_cli_alias_with(
    mgr: &ConfigManager,
    current: &OsStr,
) -> Vec<CompletionCandidate> {
    let prefix = current.to_string_lossy();
    let Ok(global) = mgr.load_global_config() else {
        return vec![];
    };
    let default_tpl = global.agent_cli.as_deref();

    global
        .agent_cli_alias
        .iter()
        .filter(|(name, _)| name.starts_with(prefix.as_ref()))
        .map(|(name, tpl)| {
            let is_default = default_tpl == Some(name.as_str());
            let help = if is_default {
                format!("(default) {}", tpl)
            } else {
                tpl.clone()
            };
            CompletionCandidate::new(name).help(Some(help.into()))
        })
        .collect()
}

pub fn complete_agent_cli_alias(current: &OsStr) -> Vec<CompletionCandidate> {
    let Ok(mgr) = ConfigManager::new() else {
        return vec![];
    };
    complete_agent_cli_alias_with(&mgr, current)
}
```

- [ ] **Step 4: Wire the real completer into `StartArgs`**

Edit `src/cli/workspace.rs`:

Add `complete_agent_cli_alias` to the import from `crate::core::completers` (line 5-7):
```rust
use crate::core::completers::{
    complete_agent_cli_alias, complete_repos_list, complete_template, complete_workspace,
    WorkspaceFilter,
};
```

Remove the placeholder function `zootree_complete_agent_cli_placeholder` added in Task 3.

Update the `StartArgs.run_agent` field's `add` attribute:
```rust
    #[arg(
        long,
        num_args = 0..=1,
        default_missing_value = "",
        value_name = "ALIAS_OR_CMD",
        help = "Launch agent_cli in the designated pane (alias name or literal command)",
        add = ArgValueCompleter::new(|c: &std::ffi::OsStr| complete_agent_cli_alias(c)),
    )]
    pub run_agent: Option<Option<String>>,
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --test completions_test agent_cli_alias_completer`
Expected: 5 PASS.

Run: `cargo test`
Expected: full suite green.

- [ ] **Step 6: Manually verify completion (optional sanity check)**

Run:
```bash
cargo build
COMPLETE=zsh ./target/debug/zootree start --run-agent ''
```
Expected: prints zsh completion script (does not execute completer; just confirms registration). Smoke check that the binary builds.

- [ ] **Step 7: Commit**

```bash
git add src/core/completers.rs src/cli/workspace.rs tests/completions_test.rs
git commit -m "feat(completion): dynamic completer for --run-agent alias names

Lists alias keys from agent_cli_alias; entry whose name equals the
agent_cli string value is annotated with '(default)' in help."
```

---

## Task 5: Update README + zootree-usage skill

**Files:**
- Modify: `README.md` (agent_cli section)
- Modify: `README.zh-CN.md` (mirror section)
- Modify: `skills/zootree-usage/SKILL.md`

- [ ] **Step 1: Locate the agent_cli sections**

Run:
```bash
grep -n "agent_cli\|--run-agent" README.md README.zh-CN.md skills/zootree-usage/SKILL.md
```
Expected: a handful of hits in each file. Note line numbers for editing.

- [ ] **Step 2: Update `README.zh-CN.md`**

Find the existing `agent_cli` paragraph in the global config example. Add the alias map and updated `--run-agent` examples. Use this content as the new section (place where existing `agent_cli` doc lives — adapt heading level to surrounding text):

```markdown
### agent_cli 与别名

`agent_cli` 既可以是字面量命令模板，也可以是 `agent_cli_alias` 中已注册的别名。`--run-agent`
默认使用该字段；也可显式传入别名或字面量。

```toml
agent_cli = "claude"   # 引用 alias "claude"

[agent_cli_alias]
claude = "claude --dangerously-skip-permissions -- $prompt"
claude-safe = "claude -- $prompt"
gemini = "gemini chat -- $prompt"
codex = "codex --skip-confirm -- $prompt"
```

启动用法：

```bash
zootree start ws                              # 不启动 agent
zootree start ws --run-agent                  # 用 agent_cli 默认（这里是 "claude"）
zootree start ws --run-agent claude-safe      # 切换到 alias claude-safe
zootree start ws --run-agent="codex --skip -- $prompt"  # 直接传字面量
```

- 别名解析为一层：`agent_cli_alias` 中找不到 key 时，原字符串作字面量命令使用，**不会**报错或警告。
- shell 补全（`--run-agent <TAB>`）会列出所有 alias 名，与 `agent_cli` 字段值匹配的那条
  在描述里以 `(default)` 标记。
\```
```

- [ ] **Step 3: Update `README.md` (English mirror)**

Add the equivalent English section in the same place:

```markdown
### agent_cli and aliases

`agent_cli` accepts either a literal command template or the name of an entry in
`agent_cli_alias`. `--run-agent` defaults to this field; you can also pass an alias
name or a literal command directly.

```toml
agent_cli = "claude"   # references alias "claude"

[agent_cli_alias]
claude = "claude --dangerously-skip-permissions -- $prompt"
claude-safe = "claude -- $prompt"
gemini = "gemini chat -- $prompt"
codex = "codex --skip-confirm -- $prompt"
```

Usage:

```bash
zootree start ws                              # no agent
zootree start ws --run-agent                  # use agent_cli ("claude" here)
zootree start ws --run-agent claude-safe      # switch to alias claude-safe
zootree start ws --run-agent="codex --skip -- $prompt"  # literal command
```

- Alias lookup is single-level: a value not present in `agent_cli_alias` is used as
  a literal command (no warning).
- Shell completion (`--run-agent <TAB>`) lists all alias names; the one matching the
  `agent_cli` string value is annotated `(default)` in its help text.
\```
```

- [ ] **Step 4: Update `skills/zootree-usage/SKILL.md`**

Locate any existing `agent_cli` / `--run-agent` paragraphs and update them to mention:
1. `agent_cli_alias` exists as a `[agent_cli_alias]` table for reusable templates.
2. `agent_cli` field can be either a literal or an alias key.
3. `--run-agent` accepts an optional value (alias or literal) and supports completion.

If the skill has no agent_cli section, add a short subsection mirroring the README content (Chinese — the skill is in Chinese).

- [ ] **Step 5: Verify rendering**

Run:
```bash
grep -A 5 "agent_cli_alias" README.md README.zh-CN.md skills/zootree-usage/SKILL.md
```
Expected: each file has the new section.

- [ ] **Step 6: Commit**

```bash
git add README.md README.zh-CN.md skills/zootree-usage/SKILL.md
git commit -m "docs: document agent_cli_alias and --run-agent value usage"
```

---

## Task 6: Final verification

- [ ] **Step 1: Run full test suite**

Run: `cargo test`
Expected: all tests pass (existing + new).

- [ ] **Step 2: Run clippy + check**

Run: `cargo clippy --all-targets -- -D warnings`
Expected: no warnings.

Run: `cargo check --tests`
Expected: clean.

- [ ] **Step 3: Manual smoke test (optional but recommended)**

Set up a temp config:
```bash
mkdir -p /tmp/zt-smoke/config
cat > /tmp/zt-smoke/config/config.toml <<'EOF'
agent_cli = "claude"

[agent_cli_alias]
claude = "echo claude $prompt"
gemini = "echo gemini $prompt"
EOF
```

Then verify each scenario builds correctly (compile-only smoke):
```bash
cargo run -- start --help
```
Expected: `--run-agent` shown with `[ALIAS_OR_CMD]` value placeholder.

- [ ] **Step 4: Update zootree-dev skill self-check**

Per `skills/zootree-dev/SKILL.md` self-iteration rules, run:
```bash
find src -type f -name "*.rs" | sort
grep "^\[dependencies" Cargo.toml -A 100 | grep -v "^\[" | grep -v "^$" | grep -v "^#"
```
Expected: file tree and dependencies match what's already documented in `skills/zootree-dev/SKILL.md` (this change adds no new files or deps). No skill update needed.

- [ ] **Step 5: Final commit (only if any wrap-up tweaks needed)**

```bash
git status
# if anything is uncommitted, add + commit; otherwise skip.
```

---

## Self-Review

**Spec coverage check:**
- §1 数据模型 (BTreeMap field, serde defaults, examples) → Task 1 ✓
- §2 CLI 解析 (Option<Option<String>>, num_args=0..=1, default_missing_value) → Task 3 ✓
- §3 解析流程 (resolve_agent_cli + launch_zellij rewrite) → Task 2 + Task 3 ✓
- §4 错误处理 (--run-agent + None agent_cli, unknown alias falls back, warning preserved) → Task 3 (logic) + tests ✓
- §5 补全 (alias names, (default) marker, ConfigManager::with_base_dir injection) → Task 4 ✓
- §6 测试 (start_agent + agent_cli + config + completion tests) → Tasks 1-4 ✓
- §7 文档 + skill (README, zootree-usage, zootree-dev no-op) → Task 5 + Task 6 step 4 ✓
- §8 兼容性 (existing single-string agent_cli still works) → covered by existing tests preserved in Task 3 step 1-2 ✓

**Placeholder scan:** No "TBD", no "implement later", no "similar to Task N", no "add appropriate error handling". Each step shows code or exact commands. ✓

**Type consistency:**
- `resolve_agent_cli<'a>(value: &'a str, alias_map: &'a BTreeMap<String, String>) -> &'a str` — same signature in Task 2 (definition), Task 3 (caller in `launch_zellij` and helper), Task 2 tests, Task 3 tests. ✓
- `StartArgs.run_agent: Option<Option<String>>` — Task 3 step 3 (definition), Task 3 step 4 (clone at call site), Task 3 step 5 (`launch_zellij` signature). ✓
- `complete_agent_cli_alias` / `complete_agent_cli_alias_with` — Task 4 step 3 (definitions), Task 4 step 4 (use in workspace.rs), Task 4 step 1 (test). Names match. ✓
- `BTreeMap<String, String>` — Task 1 (field), Task 2 (helper signature + tests), Task 3 (call site, tests, helper), Task 4 (tests build the map). Same throughout. ✓
- `agent_cli_alias` field name — Task 1 (struct field), Task 4 step 3 (`global.agent_cli_alias`), tests. ✓
