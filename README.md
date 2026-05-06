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

```bash
cargo install --path .
```

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

zootree list                         # List workspaces
  --status pending|in_progress|done|canceled

zootree open [name]                  # Open an existing workspace

zootree done [name]                  # Finish a workspace
  --no-merge                         # Skip merge
  --no-clean                         # Skip cleanup
  --push                             # Push branches
  --delete-remote                    # Delete remote branches
  --force                            # Force execution

zootree cancel [name]                # Cancel a workspace
  --no-clean                         # Skip cleanup
  --force                            # Force execution
```

### Templates

```bash
zootree template list                # List templates
zootree template show <name>         # Show a template
zootree template delete <name>       # Delete a template
```

### Utilities

```bash
zootree prune                        # Clean up orphaned worktrees
zootree logs                         # View logs
```

## Configuration

### Global config (~/.config/zootree/config.toml)

```toml
default_layout = "default"
workspace_root = "~/zootree-workspaces"
branch_prefix = "zootree"
copy_files = [".env"]

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
pre_done = { inline = "echo 'cleaning up' && rm -rf $WORKTREE_PATH" }
```

Available environment variables in hooks:
- `WORKSPACE` - Workspace name
- `REPO` - Repository name
- `BRANCH` - Branch name
- `TARGET_BRANCH` - Target branch
- `WORKTREE_PATH` - Worktree path
- `WORKSPACE_DIR` - Workspace directory

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

## Options

```bash
-v, --verbose    # Verbose output
-q, --quiet      # Quiet output
-h, --help       # Help
--version        # Version
```

## Dependencies

- Git
- Zellij
- LazyGit (optional)
