# List Card Output Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Change `zootree list` to default to a compact multi-line card output while preserving the existing single-line format behind `--oneline`.

**Architecture:** Keep `handle_list` as the command coordinator and move text layout into private formatter helpers in `src/cli/workspace.rs`. Introduce a small internal `ListWorkspaceItem` model so the output functions can be unit tested without touching config files or terminal state.

**Tech Stack:** Rust, clap derive, existing `ConfigManager`, existing `WorkspaceConfig` / `WorkspaceStatus`, Cargo tests.

---

## File Structure

- Modify `src/cli/workspace.rs`
  - Add `ListArgs::oneline`.
  - Add private `ListWorkspaceItem`.
  - Add `render_list_cards`, `render_list_oneline`, and `format_repo_targets`.
  - Refactor `handle_list` to collect items and choose a formatter.
  - Add module-level formatter tests in the existing `#[cfg(test)] mod tests`.
- Modify `README.md`
  - Document that default `zootree list` is human-oriented card output.
  - Document `zootree list --oneline` for `fzf` / scripts.
- Modify `README.zh-CN.md`
  - Mirror the English README update in Chinese.

No new public module is needed. The formatter helpers stay private because they are implementation details of the CLI command.

---

### Task 1: Add failing formatter tests

**Files:**
- Modify: `src/cli/workspace.rs`

- [ ] **Step 1: Add test helpers inside the existing `#[cfg(test)] mod tests`**

Insert these helpers after `success_output`:

```rust
    fn list_workspace(
        status: WorkspaceStatus,
        name: &str,
        title: &str,
        branch: &str,
        workspace_dir: &str,
        repos: Vec<RepoEntry>,
    ) -> ListWorkspaceItem {
        ListWorkspaceItem {
            status,
            workspace: WorkspaceConfig {
                title: title.into(),
                name: name.into(),
                description: String::new(),
                branch: branch.into(),
                workspace_dir: workspace_dir.into(),
                created_at: "2026-06-23T10:00:00+08:00".into(),
                zellij: ZellijConfig::default(),
                repos,
                events: Vec::new(),
            },
        }
    }

    fn repo(name: &str, target_branch: Option<&str>) -> RepoEntry {
        RepoEntry {
            name: name.into(),
            target_branch: target_branch.map(str::to_string),
        }
    }
```

- [ ] **Step 2: Add the oneline compatibility test**

Add this test in the same test module:

```rust
    #[test]
    fn render_list_oneline_matches_legacy_format() {
        let items = vec![
            list_workspace(
                WorkspaceStatus::InProgress,
                "pure-vine",
                "List output redesign",
                "zootree/pure-vine",
                "/Users/lijufeng/zootree-workspaces/pure-vine",
                vec![repo("zootree", Some("main"))],
            ),
            list_workspace(
                WorkspaceStatus::Pending,
                "calm-river",
                "Pending work",
                "zootree/calm-river",
                "/Users/lijufeng/zootree-workspaces/calm-river",
                vec![repo("frontend", None)],
            ),
        ];

        let out = render_list_oneline(&items);

        assert_eq!(
            out,
            "  pure-vine (in_progress) - List output redesign [zootree:main] /Users/lijufeng/zootree-workspaces/pure-vine\n  calm-river (pending) - Pending work [frontend:*]\n"
        );
    }
```

- [ ] **Step 3: Add card output tests**

Add these tests in the same test module:

```rust
    #[test]
    fn render_list_cards_includes_branch_title_repos_and_dir_for_in_progress() {
        let items = vec![list_workspace(
            WorkspaceStatus::InProgress,
            "pure-vine",
            "zootree list 每项都堆在一行显示再窄屏时可视化效果太差",
            "zootree/pure-vine",
            "/Users/lijufeng/zootree-workspaces/pure-vine",
            vec![repo("zootree", Some("main"))],
        )];

        let out = render_list_cards(&items);

        assert_eq!(
            out,
            "pure-vine  [in_progress]  zootree/pure-vine\n  title: zootree list 每项都堆在一行显示再窄屏时可视化效果太差\n  repos: zootree:main\n  dir:   /Users/lijufeng/zootree-workspaces/pure-vine\n"
        );
    }

    #[test]
    fn render_list_cards_omits_dir_for_pending() {
        let items = vec![list_workspace(
            WorkspaceStatus::Pending,
            "calm-river",
            "Pending work",
            "zootree/calm-river",
            "/Users/lijufeng/zootree-workspaces/calm-river",
            vec![repo("frontend", None)],
        )];

        let out = render_list_cards(&items);

        assert_eq!(
            out,
            "calm-river  [pending]  zootree/calm-river\n  title: Pending work\n  repos: frontend:*\n"
        );
    }

    #[test]
    fn render_list_cards_separates_items_with_blank_line() {
        let items = vec![
            list_workspace(
                WorkspaceStatus::Pending,
                "one",
                "First",
                "zootree/one",
                "/tmp/one",
                vec![repo("frontend", Some("main"))],
            ),
            list_workspace(
                WorkspaceStatus::Pending,
                "two",
                "Second",
                "zootree/two",
                "/tmp/two",
                vec![repo("backend", Some("develop"))],
            ),
        ];

        let out = render_list_cards(&items);

        assert_eq!(
            out,
            "one  [pending]  zootree/one\n  title: First\n  repos: frontend:main\n\ntwo  [pending]  zootree/two\n  title: Second\n  repos: backend:develop\n"
        );
    }

    #[test]
    fn render_list_cards_shows_none_when_repos_empty() {
        let items = vec![list_workspace(
            WorkspaceStatus::Done,
            "empty-repos",
            "No repos",
            "zootree/empty-repos",
            "/tmp/empty-repos",
            Vec::new(),
        )];

        let out = render_list_cards(&items);

        assert_eq!(
            out,
            "empty-repos  [done]  zootree/empty-repos\n  title: No repos\n  repos: (none)\n"
        );
    }
```

- [ ] **Step 4: Run the focused test command and verify failure**

Run:

```bash
cargo test render_list_
```

Expected: compilation fails because `ListWorkspaceItem`, `render_list_oneline`, and `render_list_cards` are not defined yet. The failure should include errors like:

```text
cannot find type `ListWorkspaceItem` in this scope
cannot find function `render_list_oneline` in this scope
cannot find function `render_list_cards` in this scope
```

- [ ] **Step 5: Commit the failing tests**

```bash
git add src/cli/workspace.rs
git commit -m "test: cover list output formats"
```

---

### Task 2: Implement formatter helpers

**Files:**
- Modify: `src/cli/workspace.rs`

- [ ] **Step 1: Add the private list item model**

Place this near `CurrentRepoDefault`, before list rendering helpers:

```rust
#[derive(Debug, Clone, PartialEq)]
struct ListWorkspaceItem {
    status: WorkspaceStatus,
    workspace: WorkspaceConfig,
}
```

- [ ] **Step 2: Add shared repo formatting**

Place this near `format_status`:

```rust
fn format_repo_targets(repos: &[RepoEntry]) -> String {
    if repos.is_empty() {
        return "(none)".into();
    }

    repos
        .iter()
        .map(|r| format!("{}:{}", r.name, r.target_branch.as_deref().unwrap_or("*")))
        .collect::<Vec<_>>()
        .join(", ")
}
```

- [ ] **Step 3: Add the oneline formatter**

Place this after `format_repo_targets`:

```rust
fn render_list_oneline(items: &[ListWorkspaceItem]) -> String {
    let mut out = String::new();

    for item in items {
        let ws = &item.workspace;
        let status_str = format_status(&item.status);
        let repos_str = format_repo_targets(&ws.repos);

        if matches!(item.status, WorkspaceStatus::InProgress) {
            out.push_str(&format!(
                "  {} ({}) - {} [{}] {}\n",
                ws.name, status_str, ws.title, repos_str, ws.workspace_dir
            ));
        } else {
            out.push_str(&format!(
                "  {} ({}) - {} [{}]\n",
                ws.name, status_str, ws.title, repos_str
            ));
        }
    }

    out
}
```

- [ ] **Step 4: Add the compact card formatter**

Place this after `render_list_oneline`:

```rust
fn render_list_cards(items: &[ListWorkspaceItem]) -> String {
    let mut out = String::new();

    for (idx, item) in items.iter().enumerate() {
        if idx > 0 {
            out.push('\n');
        }

        let ws = &item.workspace;
        out.push_str(&format!(
            "{}  [{}]  {}\n",
            ws.name,
            format_status(&item.status),
            ws.branch
        ));
        out.push_str(&format!("  title: {}\n", ws.title));
        out.push_str(&format!("  repos: {}\n", format_repo_targets(&ws.repos)));

        if matches!(item.status, WorkspaceStatus::InProgress) {
            out.push_str(&format!("  dir:   {}\n", ws.workspace_dir));
        }
    }

    out
}
```

- [ ] **Step 5: Run focused tests and verify pass**

Run:

```bash
cargo test render_list_
```

Expected: all four `render_list_...` tests pass.

- [ ] **Step 6: Commit formatter implementation**

```bash
git add src/cli/workspace.rs
git commit -m "feat: add list output formatters"
```

---

### Task 3: Wire `--oneline` into `zootree list`

**Files:**
- Modify: `src/cli/workspace.rs`

- [ ] **Step 1: Add the `--oneline` flag to `ListArgs`**

Change `ListArgs` to:

```rust
#[derive(Args)]
pub struct ListArgs {
    #[arg(
        long,
        value_enum,
        help = "Filter by status (repeatable: pending, in_progress, done, canceled)"
    )]
    pub status: Vec<WorkspaceStatus>,

    #[arg(long, help = "Use the legacy one-line output format")]
    pub oneline: bool,
}
```

- [ ] **Step 2: Refactor `handle_list` to use the formatter helpers**

Replace the body of `handle_list` with:

```rust
pub fn handle_list(args: &ListArgs) -> Result<()> {
    let config_mgr = ConfigManager::new()?;

    let status_filter: Vec<WorkspaceStatus> = if args.status.is_empty() {
        vec![WorkspaceStatus::Pending, WorkspaceStatus::InProgress]
    } else {
        args.status.clone()
    };

    let workspaces = config_mgr.list_workspaces(Some(status_filter.as_slice()))?;

    if workspaces.is_empty() {
        println!("no workspaces found");
        return Ok(());
    }

    let mut items = Vec::with_capacity(workspaces.len());
    for ws in workspaces {
        let (status, _) = config_mgr.load_workspace(&ws.name)?;
        items.push(ListWorkspaceItem {
            status,
            workspace: ws,
        });
    }

    let output = if args.oneline {
        render_list_oneline(&items)
    } else {
        render_list_cards(&items)
    };
    print!("{}", output);

    Ok(())
}
```

- [ ] **Step 3: Run focused tests**

Run:

```bash
cargo test render_list_
```

Expected: all formatter tests still pass.

- [ ] **Step 4: Run full test suite**

Run:

```bash
cargo test
```

Expected: all tests pass.

- [ ] **Step 5: Manually check current workspace output**

Run:

```bash
cargo run --quiet -- list --status in-progress
```

Expected: output uses compact cards. It should include lines shaped like:

```text
pure-vine  [in_progress]  zootree/pure-vine
  title: zootree list 每项都堆在一行显示再窄屏时可视化效果太差
  repos: zootree:main
  dir:   /Users/lijufeng/zootree-workspaces/pure-vine
```

- [ ] **Step 6: Manually check legacy output**

Run:

```bash
cargo run --quiet -- list --status in-progress --oneline
```

Expected: output uses the old one-line shape. It should include lines shaped like:

```text
  pure-vine (in_progress) - zootree list 每项都堆在一行显示再窄屏时可视化效果太差 [zootree:main] /Users/lijufeng/zootree-workspaces/pure-vine
```

- [ ] **Step 7: Commit CLI wiring**

```bash
git add src/cli/workspace.rs
git commit -m "feat: default list to card output"
```

---

### Task 4: Document `--oneline` usage

**Files:**
- Modify: `README.md`
- Modify: `README.zh-CN.md`

- [ ] **Step 1: Update the English command list**

In `README.md`, under the Workspaces command list, change the list section from:

```text
zootree list                         # List workspaces
  --status pending|in-progress|done|canceled
```

to:

```text
zootree list                         # List workspaces as compact cards
  --status pending|in-progress|done|canceled
  --oneline                          # Use legacy one-line output for fzf/scripts
```

- [ ] **Step 2: Update the Chinese command list**

In `README.zh-CN.md`, under the workspace command list, change the list section from:

```text
zootree list                         # 列出工作空间
  --status pending|in-progress|done|canceled
```

to:

```text
zootree list                         # 以紧凑卡片形式列出工作空间
  --status pending|in-progress|done|canceled
  --oneline                          # 使用旧版单行输出，便于 fzf/脚本处理
```

- [ ] **Step 3: Run README grep verification**

Run:

```bash
rg -n -- "--oneline|compact cards|紧凑卡片" README.md README.zh-CN.md
```

Expected: output includes the new `--oneline` lines in both README files and the compact card wording.

- [ ] **Step 4: Run full tests again**

Run:

```bash
cargo test
```

Expected: all tests pass after documentation edits.

- [ ] **Step 5: Commit documentation**

```bash
git add README.md README.zh-CN.md
git commit -m "docs: document list oneline mode"
```

---

### Task 5: Final verification

**Files:**
- No source edits expected.

- [ ] **Step 1: Check git status**

Run:

```bash
git status --short
```

Expected: no output.

- [ ] **Step 2: Run full test suite**

Run:

```bash
cargo test
```

Expected: all tests pass.

- [ ] **Step 3: Verify default human output**

Run:

```bash
cargo run --quiet -- list --status in-progress
```

Expected: compact card output, with branch on the first line and `dir:` only for `in_progress` items.

- [ ] **Step 4: Verify legacy machine-friendly output**

Run:

```bash
cargo run --quiet -- list --status in-progress --oneline
```

Expected: old one-line output, preserving leading two spaces, status in parentheses, ` - ` before title, repos in square brackets, and workspace dir appended only for `in_progress`.

- [ ] **Step 5: Report results**

Include in the final implementation summary:

```text
Implemented compact card output for `zootree list`, added `--oneline` for the legacy format, and documented both modes.

Verification:
- cargo test
- cargo run --quiet -- list --status in-progress
- cargo run --quiet -- list --status in-progress --oneline
```
