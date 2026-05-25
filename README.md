# zootree

A multi-repo workspace management tool. Built on Git Worktree + Zellij + LazyGit.

[中文文档](README.zh-CN.md)

## Features

- **Multi-repo management** - Work on the same branch across multiple repositories simultaneously
- **Workspaces** - Create, manage, and clean up workspaces
- **Zellij integration** - Automatically launch a well-organized terminal environment
- **Hook system** - Custom hooks support (simple/file/inline)
- **File copying** - Automatically copy config files to worktrees
- **Template system** - Save and reuse workspace configurations

## Installation

### Install prebuilt binaries via shell script

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/weineel/zootree/releases/download/v0.0.1/zootree-installer.sh | sh
```

### Install prebuilt binaries via Homebrew

```sh
brew install weineel/tap/zootree
```

### Install from source

```bash
cargo install --path .
```

## Shell Completions

zootree supports completion for subcommands, flags, and dynamic values
(workspace names, repo names, template names) on bash, zsh, fish,
PowerShell, and elvish.

### Install

Append one line to your shell's rc file. The script is regenerated on every
shell start, so it stays in sync with the installed `zootree` binary — no
manual refresh after upgrades.

| Shell | Add to | Line |
|-------|--------|------|
| bash  | `~/.bashrc` | `eval "$(zootree completions bash)"` |
| zsh   | `~/.zshrc` (after `compinit`) | `eval "$(zootree completions zsh)"` |
| fish  | `~/.config/fish/config.fish` | `zootree completions fish \| source` |
| PowerShell | `$PROFILE` | `zootree completions powershell \| Out-String \| Invoke-Expression` |
| elvish | `~/.config/elvish/rc.elv` | `eval (zootree completions elvish \| slurp)` |

Restart your shell (or `source` the rc file) to activate.

### What gets completed

- All subcommands and flags
- `zootree start <TAB>` — pending workspaces
- `zootree open <TAB>` / `zootree done <TAB>` — in-progress workspaces
- `zootree cancel <TAB>` — pending or in-progress workspaces
- `zootree repo edit <TAB>` / `zootree repo remove <TAB>` — registered repos
- `zootree template save --from <TAB>` — any workspace
- `zootree create --template <TAB>` — saved templates
- `zootree create --repos <TAB>` — registered repos (comma-separated)
- `zootree list --status <TAB>` — workspace status values (pending, in-progress, done, canceled)
- `zootree done --strategy <TAB>` — merge strategy values (squash, rebase, merge)

zsh and fish additionally show a brief description (workspace title + status,
repo path, or template repos) next to each candidate.

### Troubleshooting

If completions don't activate after install, verify the dynamic interceptor:

```bash
COMPLETE=zsh zootree -- zootree start ''
```

This should output candidates one per line. If empty, ensure you have
workspaces with the expected status (`zootree list`).

## Quick Start

### 1. Initialize config directory

```bash
mkdir -p ~/.config/zootree
```

### 2. Add repositories

```bash
# Interactive
zootree repo add

# From path (name auto-extracted)
zootree repo add ~/projects/myrepo

# With options
zootree repo add ~/projects/myrepo --name myrepo --default-target-branch develop
```

### 3. Create a workspace

```bash
# Interactive
zootree create

# With options
zootree create --title "New feature" --repos frontend:feature/abc,backend:feature/abc
```

### 4. Start a workspace

```bash
zootree start
# Or specify by name
zootree start my-workspace
```

### 5. Finish a workspace

```bash
# Merge branches and clean up
zootree done

# Merge only, no cleanup
zootree done --no-clean

# Merge and push
zootree done --push
```

## Command Reference

### Repository Management

```bash
zootree repo add <path>              # Add a repository
zootree repo list                    # List repositories
zootree repo remove <name>           # Remove a repository
```

### Workspaces

```bash
zootree create [options]             # Create a workspace
  --title <title>                    # Title
  --name <name>                      # Workspace name
  --repos <repos>                    # Repo list (repo:branch format)
  --branch <branch>                  # Branch name
  --template <name>                  # Use a template

zootree start [name]                 # Start a workspace
  --no-zellij                        # Don't launch Zellij
  --run-agent [alias|command]        # Launch a coding agent (see Configuration → Agent CLI)

zootree list                         # List workspaces
  --status pending|in-progress|done|canceled

zootree open [name]                  # Open an existing workspace

zootree done [name]                  # Finish a workspace
  --no-merge                         # Skip merge
  --no-clean                         # Skip cleanup
  --push                             # Push branches
  --force                            # Force execution

zootree cancel [name]                # Cancel a workspace
  --no-clean                         # Skip cleanup
  --force                            # Force execution
```

### Templates

```bash
zootree template list                # List templates
zootree template save <name> --from <workspace>
                                    # Save a workspace as a template
```

### Utilities

```bash
zootree prune                        # Clean up orphaned worktrees
zootree logs                         # View logs
```

## Configuration

zootree reads configuration from `~/.config/zootree/`. Quick map:

| File / Field | Purpose |
|---|---|
| `config.toml` | Global defaults: workspace root, branch prefix, file copying, hooks, agent CLI |
| `repos/<name>.toml` | Per-repo overrides: path, target branch, copy files, hooks, lazygit config |
| `layouts/<name>.kdl` | Custom zellij layouts referenced from `[zellij].layout` |
| `[hooks]` blocks | Shell commands run at workspace/repo lifecycle events |
| `agent_cli` / `agent_cli_alias` | Coding agent template launched by `zootree start --run-agent` |

### Global config (~/.config/zootree/config.toml)

```toml
workspace_root = "~/zootree-workspaces"
branch_prefix = "zootree"
copy_files = [".env"]
agent_cli = "claude --dangerously-skip-permissions -- $prompt"

[zellij]
layout = "default"

[hooks]
post_create = "echo created"
post_start = "echo started"
pre_done = "echo done"
pre_cancel = "echo canceled"
pre_remove = "echo removed"

[log]
max_files = 5
```

### Repo config (~/.config/zootree/repos/<name>.toml)

```toml
path = "~/projects/myrepo"
default_target_branch = "develop"
copy_files = [".env.local", ".vscode/settings.json"]

[hooks]
post_create = "npm install"

[lazygit]
config = "~/projects/myrepo/.lazygit.yml"
```

### Hook formats

```toml
# Simple command
post_create = "echo hello"

# File script
pre_remove = { file = "~/.config/zootree/hooks/cleanup.sh" }

# Inline script
pre_done = { inline = "echo 'cleaning up' && rm -rf $ZOOTREE_WORKTREE_PATH" }
```

Available environment variables in hooks:
- `ZOOTREE_WORKSPACE` - Workspace name
- `ZOOTREE_REPO` - Repository name
- `ZOOTREE_BRANCH` - Branch name
- `ZOOTREE_TARGET_BRANCH` - Target branch
- `ZOOTREE_WORKTREE_PATH` - Worktree path
- `ZOOTREE_WORKSPACE_DIR` - Workspace directory

### Layout templates (~/.config/zootree/layouts/<name>.kdl)

```kdl
layout {
  tab {
    pane { name "frontend" }
    pane { name "backend" }
  }
}
```

Available variables:
- `@REPO_NAME@` - Repository name
- `@WORKTREE_PATH@` - Worktree path
- `@BRANCH@` - Branch name
- `@WORKSPACE_NAME@` - Workspace name
- `@WORKSPACE_DIR@` - Workspace directory

### Agent CLI

`agent_cli` is a command template for launching a coding agent in a zellij pane. The template is parsed with shell-style word splitting, and `$prompt` is substituted with the workspace's `title` (joined with `description` by a newline if present). `$prompt` may also be embedded inside a token, e.g. `--prompt=$prompt`.

The rendered command runs in:

- **1 repo** → the repo tab's bottom-right pane
- **≥2 repos** → the overview tab's last pane

Without `--run-agent`, those placeholder panes fall back to a regular shell.

#### Aliases

`agent_cli` accepts either a literal command template or the name of an entry in `agent_cli_alias`:

```toml
agent_cli = "claude"   # references alias "claude"

[agent_cli_alias]
claude = "claude --dangerously-skip-permissions -- $prompt"
claude-safe = "claude -- $prompt"
gemini = "gemini chat -- $prompt"
codex = "codex --skip-confirm -- $prompt"
```

Alias lookup is single-level: a value not present in `agent_cli_alias` is used as a literal command (no warning).

#### Using `--run-agent`

```bash
zootree start ws                                        # no agent
zootree start ws --run-agent                            # use agent_cli (default, "claude" here)
zootree start ws --run-agent claude-safe                # switch to alias claude-safe
zootree start ws --run-agent="codex --skip -- $prompt"  # literal command
```

- Place `--run-agent` after the workspace name. `zootree start --run-agent ws` would treat `ws` as the alias value and leave the positional name empty (interactive picker).
- Shell completion (`--run-agent <TAB>`) lists all alias names; the entry matching the current `agent_cli` value is annotated `(default)`.

## Options

```bash
-v, --verbose    # Verbose output
-q, --quiet      # Quiet output
-h, --help       # Help
-V, --version    # Version
```

## Dependencies

- Git
- Zellij
- LazyGit (optional)

## Contributing

首次克隆本仓库后，执行一次 `cargo check --tests` 以激活 pre-commit hook：

```bash
cargo check --tests
```

cargo-husky 会把 `.cargo-husky/hooks/pre-commit` 安装到 `.git/hooks/`。此后 `git commit` 会自动对已暂存的 `.rs` 文件运行 `rustfmt` 并把格式化结果纳入本次提交——CI 的 `cargo fmt --check` 通常就不会再被格式问题绊住。

若需要临时跳过 hook（例如紧急修复），可用 `git commit --no-verify`。
