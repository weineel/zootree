---
name: launching-zootree-workspace
description: Use only when the user explicitly invokes $launching-zootree-workspace or explicitly asks to launch one zootree workspace from the current conversation.
---

# Launching a zootree workspace

## Execution gate

Run this workflow only after explicit invocation. A discussion about zootree or
a possible development task is not authorization.

Treat explicit invocation as authorization to create and start one workspace.
Do not ask for a second confirmation when the task is unambiguous.

**REQUIRED SUB-SKILL:** Use `zootree-usage` for current command semantics and
non-interactive argument rules.

## Fixed contract

- Create exactly one workspace per invocation.
- Allow multiple repositories only when they belong to the same task.
- Default to the current Git repository and current branch.
- Pass a bare `--run-agent`; use a value only when the user explicitly selected
  an alias or command.
- Never move uncommitted changes into the workspace automatically.
- Ask one focused question only for a material ambiguity or unsafe state.

## Build the task brief

Extract only confirmed, current information from the conversation. Exclude
superseded assumptions, rejected implementation ideas except as explicit
non-goals, unrelated discussion, credentials, tokens, and private keys.

Use this description shape without repeating the one-line title:

```text
Context:
- Minimum background needed by the new agent.

Confirmed decisions:
- Decisions explicitly accepted by the user.

Scope:
- Required implementation or investigation boundaries.

Constraints and non-goals:
- Behavior that must remain unchanged and rejected approaches.

Acceptance criteria:
- Observable completion conditions.

Expected verification:
- Relevant commands and checks.
```

## Preflight before mutation

Inspect the real environment:

```bash
git rev-parse --show-toplevel
git branch --show-current
git status --short
zootree repo list
zootree list --status pending --status in-progress
```

Read `~/.config/zootree/config.toml` when present to resolve `branch_prefix` and
confirm that the default `agent_cli` exists. Use zootree's runtime default
`branch_prefix` when the field is absent.

Inspect relevant diffs when working-tree changes may overlap the requested task.
Inspect `zootree info <name>` for an active workspace that appears to represent
the same task.

Apply these branches:

| Condition | Action |
|---|---|
| Related uncommitted changes | Ask whether to start from committed `HEAD`. |
| Clearly unrelated changes | Continue without touching them. |
| Missing default `agent_cli` and no explicit override | Stop before creation and ask for configuration or an override. |
| Same task already `pending` or `in_progress` | Ask whether to reuse it or create another. |
| Mechanical name collision only | Append `-2`, `-3`, and so on. |
| Several independent tasks | Ask which one task to launch first. |
| Material repo or target-branch ambiguity | Ask one focused question. |

If the current repository is not registered, register it non-interactively with
its repository root, derived repository name, and current branch before create.

## Derive concrete arguments

- Title: infer `<type>(<scope>): <subject>` from the dominant task. Use a source
  module, command, or behavior boundary as scope; omit an unclear scope and
  never use `other`.
- Name: derive a short lowercase hyphenated slug from the subject.
- Branch: combine the configured `branch_prefix` and final workspace name.
- Repositories: use the current branch for the current repository. Add another
  repository only when the conversation clearly places it in the same task;
  otherwise use that repository's configured default target branch.
- Description: use the task brief above.

Replace every documentation variable with a concrete, shell-safe value before
execution.

## Create and launch

Execute the equivalent of:

```bash
zootree create \
  --title "$title" \
  --description "$description" \
  --name "$workspace_name" \
  --branch "$workspace_branch" \
  --repos "$repo_targets" \
  --run-agent
```

Do not append an agent value by default. When the user explicitly selected an
override, use `--run-agent "$agent_override"` instead.

## Verify and report

Run `zootree info "$workspace_name"`. Confirm status, branch, repositories, and
persisted agent selection. Do not inspect cmux or Zellij panes unless create or
info output reports a problem.

Report only the workspace name, title, branch, repository targets, and whether
create, start, and info verification succeeded.
