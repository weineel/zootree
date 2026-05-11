# Shell Completions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `clap_complete` dynamic-completion-based shell completions to zootree (bash/zsh/fish/powershell/elvish), covering subcommands, flags, and dynamic values (workspace/repo/template names) with status-aware filtering and zsh/fish description display.

**Architecture:** New `Completions` subcommand calls `clap_complete::generate` to emit shell scripts. New `core::completers` module provides free functions that read from `ConfigManager` and return `Vec<CompletionCandidate>`. Free functions are attached to `Args` fields via `#[arg(add = ArgValueCompleter::new(closure))]`. `main.rs` calls `CompleteEnv::with_factory(...).complete()` before `Cli::parse()` so the env-var-driven dynamic engine handles TAB callbacks. Two existing String-typed flags (`list --status`, `done --strategy`) get migrated to `ValueEnum` to enable native value completion.

**Tech Stack:** Rust, clap 4 + clap_complete 4.5 (`unstable-dynamic` feature), tempfile (dev-dep) for tests.

**Spec:** `docs/superpowers/specs/2026-05-08-cli-completion-design.md`

---

## File Structure

### Created
- `src/cli/completions.rs` — `CompletionsArgs` + `handle_completions` (static script gen)
- `src/core/completers.rs` — `WorkspaceFilter` enum, `complete_workspace` / `complete_repo` / `complete_template` free functions (production); `complete_workspace_with` / `complete_repo_with` / `complete_template_with` (test variants taking `&ConfigManager`)
- `tests/completions_test.rs` — unit tests for completers (workspace status filter, prefix filter, descriptions, fault tolerance) + smoke tests for static script generation

### Modified
- `Cargo.toml` — add `clap_complete = { version = "4.5", features = ["unstable-dynamic"] }`; add `[dev-dependencies] tempfile = "3"`
- `src/cli/mod.rs` — `pub mod completions;` + `Commands::Completions(completions::CompletionsArgs)` variant
- `src/cli/workspace.rs` — `ListArgs.status: Vec<WorkspaceStatus>`; `DoneArgs.strategy: Option<MergeStrategy>` (new local enum); attach `ArgValueCompleter` on `StartArgs.name`, `OpenArgs.name`, `DoneArgs.name`, `CancelArgs.name`, `CreateArgs.template`, `CreateArgs.repos`; add `MergeStrategy` enum with `ValueEnum + Clone`; add `WorkspaceStatus` `ValueEnum` impl (in `src/config/workspace.rs`); update `handle_list` and `handle_done` to consume the enums; remove old `parse_status` helper if its only user is `handle_list`
- `src/cli/repo.rs` — attach `ArgValueCompleter` on `RepoCommands::Edit.name` and `RepoCommands::Remove.name`
- `src/cli/template.rs` — attach `ArgValueCompleter` on `TemplateCommands::Save.from`
- `src/config/workspace.rs` — derive `clap::ValueEnum` on `WorkspaceStatus`
- `src/core/mod.rs` — `pub mod completers;`
- `src/main.rs` — call `clap_complete::CompleteEnv::with_factory(|| Cli::command()).complete()` first thing in `main()`; add `Commands::Completions(args) => zootree::cli::completions::handle_completions(&args)?` route
- `README.md` and `README.zh-CN.md` — new "Shell Completions" / "Shell 补全" section
- `.claude/skills/zootree-dev/SKILL.md` — update architecture tree (new files), Commands enum (new variant), dependencies table (new crate), common dev tasks (mention completer pattern)

---

## Task 1: Add dependencies

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: Add `clap_complete` to `[dependencies]` and `tempfile` to `[dev-dependencies]`**

Edit `Cargo.toml`. Append `clap_complete = { version = "4.5", features = ["unstable-dynamic"] }` to the `[dependencies]` block. Add a `[dev-dependencies]` section if missing:

```toml
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
shellexpand = "3"
clap_complete = { version = "4.5", features = ["unstable-dynamic"] }

[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 2: Verify build succeeds**

Run: `cargo build`
Expected: clean build, possibly with warnings about unused code in newly added crate. No errors.

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "chore: 添加 clap_complete + tempfile 依赖"
```

---

## Task 2: Implement `WorkspaceStatus` `ValueEnum` derive

This unlocks both completion of `list --status` values AND the `Vec<WorkspaceStatus>` migration in Task 8.

**Files:**
- Modify: `src/config/workspace.rs`
- Test: `tests/config_test.rs` (append)

- [ ] **Step 1: Write failing test**

Append to `tests/config_test.rs`:

```rust
#[test]
fn workspace_status_value_enum_parses_kebab_case() {
    use clap::ValueEnum;
    use zootree::config::workspace::WorkspaceStatus;

    assert_eq!(WorkspaceStatus::from_str("pending", false).unwrap(), WorkspaceStatus::Pending);
    assert_eq!(WorkspaceStatus::from_str("in-progress", false).unwrap(), WorkspaceStatus::InProgress);
    assert_eq!(WorkspaceStatus::from_str("done", false).unwrap(), WorkspaceStatus::Done);
    assert_eq!(WorkspaceStatus::from_str("canceled", false).unwrap(), WorkspaceStatus::Canceled);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test config_test workspace_status_value_enum_parses_kebab_case`
Expected: FAIL — `WorkspaceStatus` does not implement `ValueEnum`.

- [ ] **Step 3: Add `ValueEnum` derive**

Modify `src/config/workspace.rs`. Update the `WorkspaceStatus` derive line:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceStatus {
    Pending,
    InProgress,
    Done,
    Canceled,
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --test config_test workspace_status_value_enum_parses_kebab_case`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/config/workspace.rs tests/config_test.rs
git commit -m "feat: WorkspaceStatus 实现 clap::ValueEnum"
```

---

## Task 3: Create completers module — `complete_workspace`

**Files:**
- Create: `src/core/completers.rs`
- Modify: `src/core/mod.rs`
- Test: `tests/completions_test.rs` (new)

- [ ] **Step 1: Add module declaration**

Modify `src/core/mod.rs`. Append:

```rust
pub mod completers;
```

- [ ] **Step 2: Create empty completers module**

Create `src/core/completers.rs`:

```rust
use crate::config::ConfigManager;
use crate::config::workspace::WorkspaceStatus;
use clap_complete::CompletionCandidate;
use std::ffi::OsStr;

#[derive(Debug, Clone, Copy)]
pub enum WorkspaceFilter {
    Pending,
    InProgress,
    Active,  // pending or in_progress
    Any,
}

impl WorkspaceFilter {
    fn statuses(&self) -> &'static [WorkspaceStatus] {
        match self {
            WorkspaceFilter::Pending => &[WorkspaceStatus::Pending],
            WorkspaceFilter::InProgress => &[WorkspaceStatus::InProgress],
            WorkspaceFilter::Active => &[WorkspaceStatus::Pending, WorkspaceStatus::InProgress],
            WorkspaceFilter::Any => &[
                WorkspaceStatus::Pending,
                WorkspaceStatus::InProgress,
                WorkspaceStatus::Done,
                WorkspaceStatus::Canceled,
            ],
        }
    }
}

pub fn complete_workspace_with(
    mgr: &ConfigManager,
    current: &OsStr,
    filter: WorkspaceFilter,
) -> Vec<CompletionCandidate> {
    let prefix = current.to_string_lossy();
    let Ok(workspaces) = mgr.list_workspaces(Some(filter.statuses())) else {
        return vec![];
    };
    workspaces
        .into_iter()
        .filter(|ws| ws.name.starts_with(prefix.as_ref()))
        .map(|ws| {
            let status = mgr
                .load_workspace(&ws.name)
                .map(|(s, _)| format!("{:?}", s).to_lowercase())
                .unwrap_or_default();
            let help = if status.is_empty() {
                ws.title.clone()
            } else {
                format!("{} ({})", ws.title, status)
            };
            CompletionCandidate::new(ws.name).help(Some(help.into()))
        })
        .collect()
}

pub fn complete_workspace(current: &OsStr, filter: WorkspaceFilter) -> Vec<CompletionCandidate> {
    let Ok(mgr) = ConfigManager::new() else { return vec![]; };
    complete_workspace_with(&mgr, current, filter)
}
```

- [ ] **Step 3: Write failing tests**

Create `tests/completions_test.rs`:

```rust
use std::ffi::OsStr;
use chrono::Local;
use tempfile::TempDir;
use zootree::config::ConfigManager;
use zootree::config::workspace::{WorkspaceConfig, WorkspaceStatus};
use zootree::core::completers::{complete_workspace_with, WorkspaceFilter};

fn make_workspace(name: &str, title: &str) -> WorkspaceConfig {
    WorkspaceConfig {
        title: title.into(),
        name: name.into(),
        description: String::new(),
        branch: format!("zootree/{}", name),
        workspace_dir: format!("/tmp/{}", name),
        created_at: Local::now().to_rfc3339(),
        layout: None,
        session_mode: "standalone".into(),
        session_name: None,
        repos: Vec::new(),
        events: Vec::new(),
    }
}

fn make_mgr() -> (TempDir, ConfigManager) {
    let tmp = TempDir::new().unwrap();
    let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
    mgr.ensure_dirs().unwrap();
    (tmp, mgr)
}

fn save(mgr: &ConfigManager, status: WorkspaceStatus, name: &str, title: &str) {
    mgr.save_workspace(&status, &make_workspace(name, title)).unwrap();
}

fn names(cands: &[clap_complete::CompletionCandidate]) -> Vec<String> {
    cands.iter().map(|c| c.get_value().to_string_lossy().into_owned()).collect()
}

#[test]
fn workspace_completer_filters_pending() {
    let (_tmp, mgr) = make_mgr();
    save(&mgr, WorkspaceStatus::Pending, "fix-login", "Fix login");
    save(&mgr, WorkspaceStatus::InProgress, "add-search", "Add search");
    save(&mgr, WorkspaceStatus::Done, "old-feat", "Old feat");

    let cands = complete_workspace_with(&mgr, OsStr::new(""), WorkspaceFilter::Pending);
    assert_eq!(names(&cands), vec!["fix-login"]);
}

#[test]
fn workspace_completer_filters_in_progress() {
    let (_tmp, mgr) = make_mgr();
    save(&mgr, WorkspaceStatus::Pending, "fix-login", "Fix login");
    save(&mgr, WorkspaceStatus::InProgress, "add-search", "Add search");
    save(&mgr, WorkspaceStatus::Done, "old-feat", "Old feat");

    let cands = complete_workspace_with(&mgr, OsStr::new(""), WorkspaceFilter::InProgress);
    assert_eq!(names(&cands), vec!["add-search"]);
}

#[test]
fn workspace_completer_filters_active() {
    let (_tmp, mgr) = make_mgr();
    save(&mgr, WorkspaceStatus::Pending, "fix-login", "Fix login");
    save(&mgr, WorkspaceStatus::InProgress, "add-search", "Add search");
    save(&mgr, WorkspaceStatus::Done, "old-feat", "Old feat");

    let cands = complete_workspace_with(&mgr, OsStr::new(""), WorkspaceFilter::Active);
    let mut got = names(&cands);
    got.sort();
    assert_eq!(got, vec!["add-search", "fix-login"]);
}

#[test]
fn workspace_completer_any_includes_all() {
    let (_tmp, mgr) = make_mgr();
    save(&mgr, WorkspaceStatus::Pending, "a", "A");
    save(&mgr, WorkspaceStatus::InProgress, "b", "B");
    save(&mgr, WorkspaceStatus::Done, "c", "C");
    save(&mgr, WorkspaceStatus::Canceled, "d", "D");

    let cands = complete_workspace_with(&mgr, OsStr::new(""), WorkspaceFilter::Any);
    let mut got = names(&cands);
    got.sort();
    assert_eq!(got, vec!["a", "b", "c", "d"]);
}

#[test]
fn workspace_completer_filters_by_prefix() {
    let (_tmp, mgr) = make_mgr();
    save(&mgr, WorkspaceStatus::Pending, "fix-login", "Fix login");
    save(&mgr, WorkspaceStatus::Pending, "fix-search", "Fix search");
    save(&mgr, WorkspaceStatus::Pending, "add-thing", "Add thing");

    let cands = complete_workspace_with(&mgr, OsStr::new("fix"), WorkspaceFilter::Pending);
    let mut got = names(&cands);
    got.sort();
    assert_eq!(got, vec!["fix-login", "fix-search"]);
}

#[test]
fn workspace_completer_includes_description() {
    let (_tmp, mgr) = make_mgr();
    save(&mgr, WorkspaceStatus::Pending, "fix-login", "Fix login bug");

    let cands = complete_workspace_with(&mgr, OsStr::new(""), WorkspaceFilter::Pending);
    assert_eq!(cands.len(), 1);
    let help = cands[0].get_help().unwrap().to_string();
    assert!(help.contains("Fix login bug"), "help was: {}", help);
    assert!(help.contains("pending"), "help was: {}", help);
}

#[test]
fn workspace_completer_returns_empty_when_dir_missing() {
    let tmp = TempDir::new().unwrap();
    // Do NOT call ensure_dirs
    let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
    let cands = complete_workspace_with(&mgr, OsStr::new(""), WorkspaceFilter::Any);
    assert!(cands.is_empty());
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --test completions_test`
Expected: 7 tests pass (all `workspace_completer_*`).

If `CompletionCandidate::help` does not accept `Some(...)` directly, change the call site to `.help("text".into())` matching the API of `clap_complete = "4.5"` actually resolved by `Cargo.lock`. Verify by `cargo doc --open -p clap_complete` if needed.

- [ ] **Step 5: Commit**

```bash
git add src/core/completers.rs src/core/mod.rs tests/completions_test.rs
git commit -m "feat: workspace 动态补全 (按状态过滤 + 带描述)"
```

---

## Task 4: Add `complete_repo` and `complete_template`

**Files:**
- Modify: `src/core/completers.rs`
- Modify: `tests/completions_test.rs`

- [ ] **Step 1: Write failing tests**

Append to `tests/completions_test.rs`:

```rust
use zootree::config::repo::RepoConfig;
use zootree::config::global::HooksConfig;
use zootree::config::template::TemplateConfig;
use zootree::core::completers::{complete_repo_with, complete_template_with};

fn make_repo(path: &str) -> RepoConfig {
    RepoConfig {
        path: path.into(),
        default_target_branch: None,
        copy_files: Vec::new(),
        hooks: HooksConfig::default(),
        lazygit: None,
        layout: None,
    }
}

#[test]
fn repo_completer_lists_all_with_path_help() {
    let (_tmp, mgr) = make_mgr();
    mgr.save_repo_config("frontend", &make_repo("/work/fe")).unwrap();
    mgr.save_repo_config("backend", &make_repo("/work/be")).unwrap();

    let cands = complete_repo_with(&mgr, OsStr::new(""));
    let mut got = names(&cands);
    got.sort();
    assert_eq!(got, vec!["backend", "frontend"]);

    let frontend = cands.iter().find(|c| c.get_value() == "frontend").unwrap();
    assert!(frontend.get_help().unwrap().to_string().contains("/work/fe"));
}

#[test]
fn repo_completer_filters_by_prefix() {
    let (_tmp, mgr) = make_mgr();
    mgr.save_repo_config("frontend", &make_repo("/work/fe")).unwrap();
    mgr.save_repo_config("backend", &make_repo("/work/be")).unwrap();
    mgr.save_repo_config("docs", &make_repo("/work/docs")).unwrap();

    let cands = complete_repo_with(&mgr, OsStr::new("fr"));
    assert_eq!(names(&cands), vec!["frontend"]);
}

#[test]
fn template_completer_lists_all_with_repos_help() {
    let (_tmp, mgr) = make_mgr();
    mgr.save_template("web", &TemplateConfig {
        repos: vec!["frontend".into(), "backend".into()],
        layout: None,
        session_mode: None,
    }).unwrap();

    let cands = complete_template_with(&mgr, OsStr::new(""));
    assert_eq!(names(&cands), vec!["web"]);
    let help = cands[0].get_help().unwrap().to_string();
    assert!(help.contains("frontend") && help.contains("backend"), "help: {}", help);
}

#[test]
fn template_completer_filters_by_prefix() {
    let (_tmp, mgr) = make_mgr();
    mgr.save_template("web", &TemplateConfig {
        repos: vec!["a".into()], layout: None, session_mode: None,
    }).unwrap();
    mgr.save_template("mobile", &TemplateConfig {
        repos: vec!["b".into()], layout: None, session_mode: None,
    }).unwrap();

    let cands = complete_template_with(&mgr, OsStr::new("m"));
    assert_eq!(names(&cands), vec!["mobile"]);
}

#[test]
fn repo_completer_returns_empty_when_dir_missing() {
    let tmp = TempDir::new().unwrap();
    let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
    assert!(complete_repo_with(&mgr, OsStr::new("")).is_empty());
}

#[test]
fn template_completer_returns_empty_when_dir_missing() {
    let tmp = TempDir::new().unwrap();
    let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
    assert!(complete_template_with(&mgr, OsStr::new("")).is_empty());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test completions_test repo_completer`
Expected: FAIL — `complete_repo_with` does not exist.

- [ ] **Step 3: Implement repo + template completers**

Append to `src/core/completers.rs`:

```rust
pub fn complete_repo_with(mgr: &ConfigManager, current: &OsStr) -> Vec<CompletionCandidate> {
    let prefix = current.to_string_lossy();
    let Ok(names) = mgr.list_repos() else { return vec![]; };
    names
        .into_iter()
        .filter(|n| n.starts_with(prefix.as_ref()))
        .map(|name| {
            let help = mgr
                .load_repo_config(&name)
                .map(|c| c.path)
                .unwrap_or_default();
            let mut cand = CompletionCandidate::new(&name);
            if !help.is_empty() {
                cand = cand.help(Some(help.into()));
            }
            cand
        })
        .collect()
}

pub fn complete_repo(current: &OsStr) -> Vec<CompletionCandidate> {
    let Ok(mgr) = ConfigManager::new() else { return vec![]; };
    complete_repo_with(&mgr, current)
}

pub fn complete_template_with(mgr: &ConfigManager, current: &OsStr) -> Vec<CompletionCandidate> {
    let prefix = current.to_string_lossy();
    let Ok(names) = mgr.list_templates() else { return vec![]; };
    names
        .into_iter()
        .filter(|n| n.starts_with(prefix.as_ref()))
        .map(|name| {
            let help = mgr
                .load_template(&name)
                .map(|t| t.repos.join(", "))
                .unwrap_or_default();
            let mut cand = CompletionCandidate::new(&name);
            if !help.is_empty() {
                cand = cand.help(Some(help.into()));
            }
            cand
        })
        .collect()
}

pub fn complete_template(current: &OsStr) -> Vec<CompletionCandidate> {
    let Ok(mgr) = ConfigManager::new() else { return vec![]; };
    complete_template_with(&mgr, current)
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --test completions_test`
Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/core/completers.rs tests/completions_test.rs
git commit -m "feat: repo/template 动态补全"
```

---

## Task 5: Add `--repos` list-aware completer

`zootree create --repos repo1:branch1,repo2,repo3` is comma-separated. The completer needs to recognize the prefix already typed and only suggest the last unfinished segment.

**Files:**
- Modify: `src/core/completers.rs`
- Modify: `tests/completions_test.rs`

- [ ] **Step 1: Write failing tests**

Append to `tests/completions_test.rs`:

```rust
use zootree::core::completers::complete_repos_list_with;

#[test]
fn repos_list_completer_handles_first_segment() {
    let (_tmp, mgr) = make_mgr();
    mgr.save_repo_config("frontend", &make_repo("/fe")).unwrap();
    mgr.save_repo_config("backend", &make_repo("/be")).unwrap();

    let cands = complete_repos_list_with(&mgr, OsStr::new(""));
    let mut got = names(&cands);
    got.sort();
    assert_eq!(got, vec!["backend", "frontend"]);
}

#[test]
fn repos_list_completer_handles_continuation() {
    let (_tmp, mgr) = make_mgr();
    mgr.save_repo_config("frontend", &make_repo("/fe")).unwrap();
    mgr.save_repo_config("backend", &make_repo("/be")).unwrap();

    let cands = complete_repos_list_with(&mgr, OsStr::new("frontend,"));
    let mut got = names(&cands);
    got.sort();
    assert_eq!(got, vec!["frontend,backend", "frontend,frontend"]);
}

#[test]
fn repos_list_completer_filters_partial_continuation() {
    let (_tmp, mgr) = make_mgr();
    mgr.save_repo_config("frontend", &make_repo("/fe")).unwrap();
    mgr.save_repo_config("backend", &make_repo("/be")).unwrap();

    let cands = complete_repos_list_with(&mgr, OsStr::new("frontend,b"));
    assert_eq!(names(&cands), vec!["frontend,backend"]);
}

#[test]
fn repos_list_completer_skips_branch_segment() {
    let (_tmp, mgr) = make_mgr();
    mgr.save_repo_config("frontend", &make_repo("/fe")).unwrap();

    // current ends with `:`, indicating user is typing branch name; we don't suggest branches.
    let cands = complete_repos_list_with(&mgr, OsStr::new("frontend:"));
    assert!(cands.is_empty());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test completions_test repos_list`
Expected: FAIL — `complete_repos_list_with` does not exist.

- [ ] **Step 3: Implement `complete_repos_list_with`**

Append to `src/core/completers.rs`:

```rust
pub fn complete_repos_list_with(
    mgr: &ConfigManager,
    current: &OsStr,
) -> Vec<CompletionCandidate> {
    let raw = current.to_string_lossy();
    // Split on the last comma to find the segment being edited.
    let (prefix, segment) = match raw.rfind(',') {
        Some(idx) => (&raw[..=idx], &raw[idx + 1..]),
        None => ("", raw.as_ref()),
    };

    // If the segment already contains ':', user is typing a branch name; don't suggest.
    if segment.contains(':') {
        return vec![];
    }

    let Ok(names) = mgr.list_repos() else { return vec![]; };
    names
        .into_iter()
        .filter(|n| n.starts_with(segment))
        .map(|name| {
            let help = mgr
                .load_repo_config(&name)
                .map(|c| c.path)
                .unwrap_or_default();
            let value = format!("{}{}", prefix, name);
            let mut cand = CompletionCandidate::new(value);
            if !help.is_empty() {
                cand = cand.help(Some(help.into()));
            }
            cand
        })
        .collect()
}

pub fn complete_repos_list(current: &OsStr) -> Vec<CompletionCandidate> {
    let Ok(mgr) = ConfigManager::new() else { return vec![]; };
    complete_repos_list_with(&mgr, current)
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --test completions_test`
Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/core/completers.rs tests/completions_test.rs
git commit -m "feat: --repos 列表分段补全"
```

---

## Task 6: Implement `completions` subcommand (static script generation)

**Files:**
- Create: `src/cli/completions.rs`
- Modify: `src/cli/mod.rs`
- Modify: `tests/completions_test.rs`

- [ ] **Step 1: Add module declaration and Commands variant**

Modify `src/cli/mod.rs`:

```rust
pub mod repo;
pub mod workspace;
pub mod template;
pub mod prune;
pub mod completions;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "zootree", about = "Multi-repo collaborative workspace manager")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    #[arg(long, global = true, help = "Enable verbose logging output")]
    pub verbose: bool,

    #[arg(long, global = true, help = "Suppress all output except errors")]
    pub quiet: bool,
}

#[derive(Subcommand)]
pub enum Commands {
    #[command(about = "Manage registered repositories")]
    Repo(repo::RepoArgs),
    #[command(about = "Create a new workspace")]
    Create(workspace::CreateArgs),
    #[command(about = "List workspaces")]
    List(workspace::ListArgs),
    #[command(about = "Start a pending workspace (create worktrees and launch zellij)")]
    Start(workspace::StartArgs),
    #[command(about = "Open an in-progress workspace in zellij")]
    Open(workspace::OpenArgs),
    #[command(about = "Complete a workspace (merge, clean up worktrees)")]
    Done(workspace::DoneArgs),
    #[command(about = "Cancel a workspace (discard worktrees without merging)")]
    Cancel(workspace::CancelArgs),
    #[command(about = "Manage workspace templates")]
    Template(template::TemplateArgs),
    #[command(about = "Remove archived workspace directories and configs")]
    Prune(prune::PruneArgs),
    #[command(about = "Show log file location")]
    Logs,
    #[command(about = "Generate shell completion script")]
    Completions(completions::CompletionsArgs),
}
```

- [ ] **Step 2: Create the completions module**

Create `src/cli/completions.rs`:

```rust
use anyhow::Result;
use clap::{Args, CommandFactory};
use clap_complete::Shell;

use crate::cli::Cli;

#[derive(Args)]
pub struct CompletionsArgs {
    #[arg(value_enum, help = "Target shell")]
    pub shell: Shell,
}

pub fn handle_completions(args: &CompletionsArgs) -> Result<()> {
    let mut cmd = Cli::command();
    let bin = cmd.get_name().to_string();
    clap_complete::generate(args.shell, &mut cmd, bin, &mut std::io::stdout());
    Ok(())
}

pub fn generate_to(shell: Shell, buf: &mut Vec<u8>) {
    let mut cmd = Cli::command();
    let bin = cmd.get_name().to_string();
    clap_complete::generate(shell, &mut cmd, bin, buf);
}
```

`generate_to` is a small testable helper that lets us assert the script content without going through stdout.

- [ ] **Step 3: Write smoke tests for static generation**

Append to `tests/completions_test.rs`:

```rust
use clap_complete::Shell;
use zootree::cli::completions::generate_to;

#[test]
fn generates_zsh_script() {
    let mut buf = Vec::new();
    generate_to(Shell::Zsh, &mut buf);
    let s = String::from_utf8(buf).unwrap();
    assert!(s.contains("compdef") && s.contains("zootree"), "zsh script: {}", &s[..s.len().min(200)]);
}

#[test]
fn generates_bash_script() {
    let mut buf = Vec::new();
    generate_to(Shell::Bash, &mut buf);
    let s = String::from_utf8(buf).unwrap();
    assert!(s.contains("complete") && s.contains("zootree"));
}

#[test]
fn generates_fish_script() {
    let mut buf = Vec::new();
    generate_to(Shell::Fish, &mut buf);
    let s = String::from_utf8(buf).unwrap();
    assert!(s.contains("complete") && s.contains("zootree"));
}

#[test]
fn generates_powershell_script() {
    let mut buf = Vec::new();
    generate_to(Shell::PowerShell, &mut buf);
    let s = String::from_utf8(buf).unwrap();
    assert!(s.contains("Register-ArgumentCompleter") && s.contains("zootree"));
}

#[test]
fn generates_elvish_script() {
    let mut buf = Vec::new();
    generate_to(Shell::Elvish, &mut buf);
    let s = String::from_utf8(buf).unwrap();
    assert!(s.contains("zootree"));
}
```

- [ ] **Step 4: Wire `Commands::Completions` route in `main.rs`**

Modify `src/main.rs`. Inside the `match command { ... }` block in the `run` function, before the closing brace, add:

```rust
        Commands::Completions(args) => {
            zootree::cli::completions::handle_completions(&args)?;
        }
```

(Place it between `Commands::Logs` arm and the closing `}`.)

- [ ] **Step 5: Build and run tests**

Run: `cargo test --test completions_test`
Expected: all tests pass (workspace + repo + template + repos_list + 5 generates_*).

Run: `cargo build`
Expected: clean build.

Verify static generation manually:

Run: `cargo run -- completions zsh | head -5`
Expected: zsh script header (e.g. `#compdef zootree`).

- [ ] **Step 6: Commit**

```bash
git add src/cli/completions.rs src/cli/mod.rs src/main.rs tests/completions_test.rs
git commit -m "feat: 添加 completions 子命令 (静态脚本生成)"
```

---

## Task 7: Wire `CompleteEnv` dynamic interceptor in `main.rs`

This makes the static-generated scripts call back to zootree on TAB.

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Add `CompleteEnv::complete()` as the very first call in `main()`**

Modify `src/main.rs`. Add the import and call:

```rust
use anyhow::Result;
use clap::{CommandFactory, Parser};
use clap_complete::CompleteEnv;
use tracing_appender::rolling;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, Layer};
use zootree::cli::{Cli, Commands};

fn init_tracing(
    verbose: bool,
    quiet: bool,
) -> anyhow::Result<tracing_appender::non_blocking::WorkerGuard> {
    // ... unchanged ...
}

fn main() {
    // Dynamic completion interceptor: if the COMPLETE env var is set, this
    // resolves the candidates and exits before any other side effects (no tracing,
    // no log files). Must run before Cli::parse().
    CompleteEnv::with_factory(Cli::command).complete();

    let cli = Cli::parse();

    let _guard = match init_tracing(cli.verbose, cli.quiet) {
        Ok(guard) => guard,
        Err(e) => {
            eprintln!("Error: failed to initialize tracing: {}", e);
            std::process::exit(1);
        }
    };

    if let Err(e) = run(cli.command) {
        tracing::error!("{:#}", e);
        std::process::exit(1);
    }
}
```

(Keep `init_tracing` and `run` functions as-is; only `main()` changes and a `use` line is added.)

- [ ] **Step 2: Build**

Run: `cargo build`
Expected: clean build.

- [ ] **Step 3: Manually verify the dynamic interceptor**

Run: `COMPLETE=zsh cargo run -- -- zootree start ''`

Expected: completion output (the format depends on `clap_complete` version, often a list of candidate names). If you have no workspaces, the output may be empty for dynamic completion but should still show static subcommands when probing different positions.

Run: `COMPLETE=zsh cargo run -- -- zootree st`
Expected: candidate `start` appears among output (static subcommand match).

- [ ] **Step 4: Commit**

```bash
git add src/main.rs
git commit -m "feat: main 中接入 CompleteEnv 动态补全拦截器"
```

---

## Task 8: Attach `ArgValueCompleter` on workspace name args

**Files:**
- Modify: `src/cli/workspace.rs`

- [ ] **Step 1: Add imports and attach completers**

Modify `src/cli/workspace.rs`. Add at the top of the imports section:

```rust
use clap_complete::ArgValueCompleter;
use crate::core::completers::{
    complete_workspace, complete_repos_list, complete_template, WorkspaceFilter,
};
```

Update the `Args` structs (only the affected fields shown):

```rust
#[derive(Args)]
pub struct CreateArgs {
    #[arg(long, help = "Workspace title (interactive if omitted)")]
    pub title: Option<String>,
    #[arg(long, help = "Workspace name (auto-generated if omitted)")]
    pub name: Option<String>,
    #[arg(long, help = "Workspace description")]
    pub description: Option<String>,
    #[arg(
        long,
        help = "Comma-separated repos, optionally with branch: repo1:branch1,repo2",
        add = ArgValueCompleter::new(|c: &std::ffi::OsStr| complete_repos_list(c))
    )]
    pub repos: Option<String>,
    #[arg(long, help = "Git branch name for worktrees (defaults to <prefix>/<name>)")]
    pub branch: Option<String>,
    #[arg(
        long,
        help = "Template name to use for repo selection",
        add = ArgValueCompleter::new(|c: &std::ffi::OsStr| complete_template(c))
    )]
    pub template: Option<String>,
}

#[derive(Args)]
pub struct StartArgs {
    #[arg(
        help = "Workspace name to start (interactive if omitted)",
        add = ArgValueCompleter::new(|c: &std::ffi::OsStr| complete_workspace(c, WorkspaceFilter::Pending))
    )]
    pub name: Option<String>,
    #[arg(long, help = "Skip launching zellij session after start")]
    pub no_zellij: bool,
}

#[derive(Args)]
pub struct OpenArgs {
    #[arg(
        help = "Workspace name to open (interactive if omitted)",
        add = ArgValueCompleter::new(|c: &std::ffi::OsStr| complete_workspace(c, WorkspaceFilter::InProgress))
    )]
    pub name: Option<String>,
}

#[derive(Args)]
pub struct DoneArgs {
    #[arg(
        help = "Workspace name to complete (interactive if omitted)",
        add = ArgValueCompleter::new(|c: &std::ffi::OsStr| complete_workspace(c, WorkspaceFilter::InProgress))
    )]
    pub name: Option<String>,
    #[arg(long, help = "Skip merging branches back to target")]
    pub no_merge: bool,
    #[arg(long, help = "Keep worktrees and workspace directory")]
    pub no_clean: bool,
    #[arg(long, help = "Push target branch to remote after merge")]
    pub push: bool,
    #[arg(long, help = "Delete remote feature branch after merge")]
    pub delete_remote: bool,
    #[arg(long, help = "Merge strategy, available: squash(default), rebase, merge")]
    pub strategy: Option<String>,  // Will be migrated to enum in Task 10
    #[arg(long, help = "Skip hooks and uncommitted-changes check")]
    pub force: bool,
    #[arg(long, help = "Show what would be done without executing")]
    pub dry_run: bool,
}

#[derive(Args)]
pub struct CancelArgs {
    #[arg(
        help = "Workspace name to cancel (interactive if omitted)",
        add = ArgValueCompleter::new(|c: &std::ffi::OsStr| complete_workspace(c, WorkspaceFilter::Active))
    )]
    pub name: Option<String>,
    #[arg(long, help = "Keep worktrees and workspace directory")]
    pub no_clean: bool,
    #[arg(long, help = "Skip hooks and confirmation prompts")]
    pub force: bool,
}
```

Leave `ListArgs.status` and `DoneArgs.strategy` alone in this task — they are migrated in Tasks 9 and 10.

- [ ] **Step 2: Build to verify the attribute syntax**

Run: `cargo build`
Expected: clean build.

If `add = ...` does not compile with the closure form, fall back to a free `fn` reference. Define small adapter functions at the bottom of the file, e.g.:

```rust
fn complete_start_workspace(c: &std::ffi::OsStr) -> Vec<clap_complete::CompletionCandidate> {
    complete_workspace(c, WorkspaceFilter::Pending)
}
```

Then use `add = ArgValueCompleter::new(complete_start_workspace)`.

- [ ] **Step 3: Manually verify dynamic completion**

Set up a test workspace (or run against your real config). Then:

```bash
COMPLETE=zsh cargo run -- -- zootree start ''
```

Expected: pending workspace names appear in output.

```bash
COMPLETE=zsh cargo run -- -- zootree open ''
```

Expected: in-progress workspace names appear.

- [ ] **Step 4: Commit**

```bash
git add src/cli/workspace.rs
git commit -m "feat: workspace 命令挂载动态补全器"
```

---

## Task 9: Attach `ArgValueCompleter` on repo and template args

**Files:**
- Modify: `src/cli/repo.rs`
- Modify: `src/cli/template.rs`

- [ ] **Step 1: Modify `src/cli/repo.rs`**

Add import:

```rust
use clap_complete::ArgValueCompleter;
use crate::core::completers::complete_repo;
```

Update the `Edit` and `Remove` variants:

```rust
#[derive(Subcommand)]
pub enum RepoCommands {
    #[command(about = "Register a new repository")]
    Add {
        #[arg(long, help = "Custom name for the repo (defaults to directory name)")]
        name: Option<String>,
        #[arg(help = "Path to the git repository")]
        path: String,
        #[arg(long, help = "Default target branch for merging (e.g. main, develop)")]
        default_target_branch: Option<String>,
    },
    #[command(about = "List registered repositories")]
    List,
    #[command(about = "Edit a repository config file")]
    Edit {
        #[arg(
            help = "Name of the repo to edit (interactive if omitted)",
            add = ArgValueCompleter::new(|c: &std::ffi::OsStr| complete_repo(c))
        )]
        name: Option<String>,
    },
    #[command(about = "Unregister a repository")]
    Remove {
        #[arg(
            help = "Name of the repo to remove (interactive if omitted)",
            add = ArgValueCompleter::new(|c: &std::ffi::OsStr| complete_repo(c))
        )]
        name: Option<String>,
    },
}
```

- [ ] **Step 2: Modify `src/cli/template.rs`**

Add import:

```rust
use clap_complete::ArgValueCompleter;
use crate::core::completers::{complete_workspace, WorkspaceFilter};
```

Update `Save` variant:

```rust
#[derive(Subcommand)]
pub enum TemplateCommands {
    #[command(about = "List saved templates")]
    List,
    #[command(about = "Save a workspace as a template")]
    Save {
        #[arg(help = "Name for the new template")]
        name: String,
        #[arg(
            long,
            help = "Workspace name to save as template",
            add = ArgValueCompleter::new(|c: &std::ffi::OsStr| complete_workspace(c, WorkspaceFilter::Any))
        )]
        from: String,
    },
}
```

- [ ] **Step 3: Build**

Run: `cargo build`
Expected: clean build.

- [ ] **Step 4: Manually verify**

```bash
COMPLETE=zsh cargo run -- -- zootree repo edit ''
```
Expected: registered repo names appear.

- [ ] **Step 5: Commit**

```bash
git add src/cli/repo.rs src/cli/template.rs
git commit -m "feat: repo/template 命令挂载动态补全器"
```

---

## Task 10: Migrate `DoneArgs.strategy` to `MergeStrategy` enum

**Files:**
- Modify: `src/cli/workspace.rs`

- [ ] **Step 1: Define `MergeStrategy` enum and update `DoneArgs`**

Modify `src/cli/workspace.rs`. Add the enum after the imports and before `parse_repos_arg`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
#[clap(rename_all = "kebab-case")]
pub enum MergeStrategy {
    Squash,
    Rebase,
    Merge,
}

impl MergeStrategy {
    pub fn as_str(self) -> &'static str {
        match self {
            MergeStrategy::Squash => "squash",
            MergeStrategy::Rebase => "rebase",
            MergeStrategy::Merge => "merge",
        }
    }
}
```

Update `DoneArgs.strategy`:

```rust
#[derive(Args)]
pub struct DoneArgs {
    #[arg(
        help = "Workspace name to complete (interactive if omitted)",
        add = ArgValueCompleter::new(|c: &std::ffi::OsStr| complete_workspace(c, WorkspaceFilter::InProgress))
    )]
    pub name: Option<String>,
    #[arg(long, help = "Skip merging branches back to target")]
    pub no_merge: bool,
    #[arg(long, help = "Keep worktrees and workspace directory")]
    pub no_clean: bool,
    #[arg(long, help = "Push target branch to remote after merge")]
    pub push: bool,
    #[arg(long, help = "Delete remote feature branch after merge")]
    pub delete_remote: bool,
    #[arg(long, value_enum, help = "Merge strategy (default: squash)")]
    pub strategy: Option<MergeStrategy>,
    #[arg(long, help = "Skip hooks and uncommitted-changes check")]
    pub force: bool,
    #[arg(long, help = "Show what would be done without executing")]
    pub dry_run: bool,
}
```

- [ ] **Step 2: Update `handle_done` consumer**

Find the existing line (around line 560):

```rust
let strategy = args.strategy.as_deref();
```

Replace with:

```rust
let strategy = args.strategy.map(MergeStrategy::as_str);
```

This produces `Option<&'static str>`, matching `git.merge`'s signature `strategy: Option<&str>`.

- [ ] **Step 3: Build**

Run: `cargo build`
Expected: clean build.

- [ ] **Step 4: Run all tests**

Run: `cargo test`
Expected: all existing tests still pass.

- [ ] **Step 5: Manually verify completion of strategy values**

```bash
COMPLETE=zsh cargo run -- -- zootree done myws --strategy ''
```

Expected: candidates `squash`, `rebase`, `merge`.

- [ ] **Step 6: Commit**

```bash
git add src/cli/workspace.rs
git commit -m "feat: --strategy 改为 MergeStrategy 枚举 (启用值补全)"
```

---

## Task 11: Migrate `ListArgs.status` to `Vec<WorkspaceStatus>`

**Files:**
- Modify: `src/cli/workspace.rs`

- [ ] **Step 1: Update `ListArgs.status` field type**

Change:

```rust
#[derive(Args)]
pub struct ListArgs {
    #[arg(long, help = "Filter by status [available: pending, in_progress, done, canceled]")]
    pub status: Vec<String>,
}
```

To:

```rust
#[derive(Args)]
pub struct ListArgs {
    #[arg(long, value_enum, help = "Filter by status (repeatable)")]
    pub status: Vec<WorkspaceStatus>,
}
```

(Make sure `WorkspaceStatus` is imported at the top of the file; it already is via `use crate::config::workspace::{WorkspaceConfig, WorkspaceStatus, ...}`.)

- [ ] **Step 2: Update `handle_list`**

Find the existing block (around line 263):

```rust
    let status_filter: Vec<WorkspaceStatus> = if args.status.is_empty() {
        vec![WorkspaceStatus::Pending, WorkspaceStatus::InProgress]
    } else {
        args.status.iter().map(|s| parse_status(s)).collect::<Result<Vec<_>>>()?
    };
```

Replace with:

```rust
    let status_filter: Vec<WorkspaceStatus> = if args.status.is_empty() {
        vec![WorkspaceStatus::Pending, WorkspaceStatus::InProgress]
    } else {
        args.status.clone()
    };
```

- [ ] **Step 3: Remove now-unused `parse_status` helper**

Search for any remaining callers:

Run: `grep -n "parse_status" src/`
If only `handle_list` was using it, delete the `fn parse_status(...)` definition. If other callers exist, keep it.

- [ ] **Step 4: Build and test**

Run: `cargo test`
Expected: all tests pass.

- [ ] **Step 5: Manually verify**

```bash
COMPLETE=zsh cargo run -- -- zootree list --status ''
```

Expected: candidates `pending`, `in-progress`, `done`, `canceled`.

```bash
cargo run -- list --status pending --status in-progress
```

Expected: workspaces in either status are listed (no error about unknown status string).

- [ ] **Step 6: Commit**

```bash
git add src/cli/workspace.rs
git commit -m "feat: --status 改为 Vec<WorkspaceStatus> (启用值补全)"
```

---

## Task 12: Add README sections (English + 中文)

**Files:**
- Modify: `README.md`
- Modify: `README.zh-CN.md`

- [ ] **Step 1: Read current READMEs**

Run: `cat README.md | head -80`
Identify a good insertion point: after "Installation" section, before "Usage". If no clear demarcation exists, insert near the top after the project intro.

- [ ] **Step 2: Add "Shell Completions" section to `README.md`**

Insert after the Installation section:

````markdown
## Shell Completions

zootree supports completion for subcommands, flags, and dynamic values
(workspace names, repo names, template names) on bash, zsh, fish,
PowerShell, and elvish.

### Install

| Shell | Command |
|-------|---------|
| bash  | `zootree completions bash > ~/.local/share/bash-completion/completions/zootree` |
| zsh   | `zootree completions zsh > "${fpath[1]}/_zootree"` |
| fish  | `zootree completions fish > ~/.config/fish/completions/zootree.fish` |
| PowerShell | `zootree completions powershell \| Out-String \| Invoke-Expression` (add to `$PROFILE`) |
| elvish | `zootree completions elvish > ~/.config/elvish/lib/zootree.elv` (add `use zootree` to `rc.elv`) |

After installing, restart your shell or `source` the relevant rc file.

### What gets completed

- All subcommands and flags
- `zootree start <TAB>` — pending workspaces
- `zootree open <TAB>` / `zootree done <TAB>` — in-progress workspaces
- `zootree cancel <TAB>` — pending or in-progress workspaces
- `zootree repo edit <TAB>` / `zootree repo remove <TAB>` — registered repos
- `zootree template save --from <TAB>` — any workspace
- `zootree create --template <TAB>` — saved templates
- `zootree create --repos <TAB>` — registered repos (comma-separated)
- `zootree list --status <TAB>` — workspace status values
- `zootree done --strategy <TAB>` — merge strategy values

zsh and fish additionally show a brief description (workspace title + status,
repo path, or template repos) next to each candidate.

### Troubleshooting

If completions don't activate after install, verify the dynamic interceptor:

```bash
COMPLETE=zsh zootree -- zootree start ''
```

This should output candidates one per line. If empty, ensure you have
workspaces with the expected status (`zootree list`).
````

- [ ] **Step 3: Add equivalent section to `README.zh-CN.md`**

Insert at the corresponding location:

````markdown
## Shell 补全

zootree 支持 5 种 shell 的补全：bash、zsh、fish、PowerShell、elvish，
覆盖子命令、flag 名以及动态值（workspace 名、repo 名、template 名）。

### 安装

| Shell | 一次性命令 |
|-------|-----------|
| bash  | `zootree completions bash > ~/.local/share/bash-completion/completions/zootree` |
| zsh   | `zootree completions zsh > "${fpath[1]}/_zootree"` |
| fish  | `zootree completions fish > ~/.config/fish/completions/zootree.fish` |
| PowerShell | `zootree completions powershell \| Out-String \| Invoke-Expression`（加到 `$PROFILE`）|
| elvish | `zootree completions elvish > ~/.config/elvish/lib/zootree.elv`（在 `rc.elv` 中加 `use zootree`）|

安装后重启 shell 或重新 source 配置文件即可生效。

### 补全范围

- 所有子命令和 flag
- `zootree start <TAB>` — pending 状态的 workspace
- `zootree open <TAB>` / `zootree done <TAB>` — in_progress 状态的 workspace
- `zootree cancel <TAB>` — pending 或 in_progress 状态的 workspace
- `zootree repo edit <TAB>` / `zootree repo remove <TAB>` — 已注册的 repo
- `zootree template save --from <TAB>` — 任意 workspace
- `zootree create --template <TAB>` — 已保存的 template
- `zootree create --repos <TAB>` — 已注册的 repo（逗号分隔列表）
- `zootree list --status <TAB>` — workspace 状态值
- `zootree done --strategy <TAB>` — 合并策略值

zsh 与 fish 还会在候选项旁显示简短描述（workspace 标题 + 状态、
repo 路径、或 template 涵盖的 repo 列表）。

### 故障排查

补全无响应时，可直接验证动态拦截器：

```bash
COMPLETE=zsh zootree -- zootree start ''
```

应当每行输出一个候选项。若为空，确认是否有匹配状态的 workspace（`zootree list`）。
````

- [ ] **Step 4: Commit**

```bash
git add README.md README.zh-CN.md
git commit -m "docs: 添加 Shell 补全章节 (英文 + 中文)"
```

---

## Task 13: Update zootree-dev skill

The skill file at `.claude/skills/zootree-dev/SKILL.md` documents the architecture; it must reflect the new files and commands.

**Files:**
- Modify: `.claude/skills/zootree-dev/SKILL.md`

- [ ] **Step 1: Update the project architecture tree**

Find the tree under "## 项目架构". Add the two new files:

```
src/
├── main.rs
├── lib.rs
├── cli/
│   ├── mod.rs
│   ├── repo.rs
│   ├── workspace.rs
│   ├── template.rs
│   ├── prune.rs
│   └── completions.rs       # 新增: completions 子命令 (静态脚本生成)
├── config/
│   └── ...
├── core/
│   ├── ...
│   └── completers.rs        # 新增: 动态补全候选生成器
├── runner.rs
└── tui.rs
```

- [ ] **Step 2: Update the关键依赖 table**

Add the row:

```
| `clap_complete` (4.5, unstable-dynamic) | shell 补全脚本生成 + 动态补全引擎 |
```

Add `tempfile` to dev-deps mention if there's a relevant section.

- [ ] **Step 3: Update Commands enum mention**

Find "命令路由" section's example. Add the new variant in the conceptual list:

```rust
match cli.command {
    Commands::Repo(args) => ...,
    Commands::Create(args) => ...,
    // ... existing variants ...
    Commands::Completions(args) => zootree::cli::completions::handle_completions(&args)?,
}
```

- [ ] **Step 4: Add a new "常见开发任务" entry for completers**

Append after the existing entries:

```markdown
### 给新命令添加动态补全

1. 确认候选数据来源（workspace/repo/template）；如需新增类别，在 `src/core/completers.rs` 中新增 `complete_<thing>_with(mgr, current)` 和 `complete_<thing>(current)`，遵循「失败返回 vec![]」原则
2. 在对应 `Args` 字段加 `add = ArgValueCompleter::new(|c: &OsStr| complete_<thing>(c))`
3. 在 `tests/completions_test.rs` 添加：基本列表、前缀过滤、描述包含正确字段三个测试
4. 静态值（如固定枚举）改为 `clap::ValueEnum`，clap 自动补全
```

- [ ] **Step 5: Verify with the skill's own checklist**

Run the checklist commands from the skill's "更新检查清单":

```bash
find src -type f -name "*.rs" | sort
grep -A 30 "enum Commands" src/cli/mod.rs
grep "^\[dependencies" Cargo.toml -A 30
```

Confirm output matches what's in the skill file.

- [ ] **Step 6: Commit**

```bash
git add .claude/skills/zootree-dev/SKILL.md
git commit -m "docs(skill): 同步 completions/completers 到 zootree-dev skill"
```

---

## Task 14: Final verification — full test pass + manual smoke

**Files:**
- (none — verification only)

- [ ] **Step 1: Run full test suite**

Run: `cargo test`
Expected: all tests pass.

- [ ] **Step 2: Build release**

Run: `cargo build --release`
Expected: clean build.

- [ ] **Step 3: Manual smoke test — static script generation for each shell**

```bash
./target/release/zootree completions bash | head -3
./target/release/zootree completions zsh | head -3
./target/release/zootree completions fish | head -3
./target/release/zootree completions powershell | head -3
./target/release/zootree completions elvish | head -3
```

Expected: each command emits a non-empty script header for the corresponding shell.

- [ ] **Step 4: Manual smoke test — dynamic completion**

If you have at least one pending workspace, run:

```bash
COMPLETE=zsh ./target/release/zootree -- zootree start ''
```

Expected: pending workspace names listed.

```bash
COMPLETE=zsh ./target/release/zootree -- zootree list --status ''
```

Expected: `pending`, `in-progress`, `done`, `canceled` listed.

```bash
COMPLETE=zsh ./target/release/zootree -- zootree done myws --strategy ''
```

Expected: `squash`, `rebase`, `merge` listed.

- [ ] **Step 5: Manual smoke test — install in your zsh and try TAB**

```bash
./target/release/zootree completions zsh > "${fpath[1]}/_zootree"
exec zsh
zootree st<TAB>
zootree start <TAB>
```

Expected: subcommand `start` completes; pending workspace names completed.

- [ ] **Step 6: Mark plan complete**

Update task #8 (`Transition to implementation`) to `completed` in the host TodoList. The implementation is done. No additional commit needed for this step.

---

## Final Notes

- Each task ends with a commit, so `git log` will show 13 incremental commits matching this plan.
- If `cargo build` fails on `add = ArgValueCompleter::new(...)` syntax due to clap-derive limitations on the resolved version, fall back to free-fn references (Task 8 Step 2 documents the fallback). All other code remains unchanged.
- If `clap_complete` resolves to a version where `CompletionCandidate::help` accepts `impl Into<StyledStr>` directly (without `Some(...)`), strip the `Some` wrapper. The tests assert behavior, not API form.
- If `MergeStrategy::as_str` returning `&'static str` causes a lifetime mismatch in `git.merge`, change `git.merge`'s signature is out of scope; instead introduce a local `let strategy_str = args.strategy.map(|s| s.as_str()); let strategy = strategy_str.as_deref();` (note: this is unnecessary — `Option<&'static str>` already satisfies `Option<&str>` via reborrow).
