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
- Complete **Choose the agent** before creation.
- Never move uncommitted changes into the workspace automatically.
- Ask one focused question at a time for agent selection, a material ambiguity,
  or an unsafe state.

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
zootree list --status pending --status in-progress --status done --status canceled --oneline
zootree list --status pending --status in-progress
```

Read `~/.config/zootree/config.toml` when present to resolve `branch_prefix`.
Run `zootree config agents --json` to inspect the default agent and aliases. Use
zootree's runtime default `branch_prefix` when the field is absent.

Inspect relevant diffs when working-tree changes may overlap the requested task.
Use the all-status `--oneline` list for mechanical name collision handling, and
the active-only list for same-task `pending`/`in_progress` detection. Inspect
`zootree info <name>` only for a suspected active duplicate, not an archived
name collision.

Apply these branches:

| Condition | Action |
|---|---|
| Related uncommitted changes | Ask whether to start from committed `HEAD`. |
| Clearly unrelated changes | Continue without touching them. |
| Missing default `agent_cli` and no explicit override | List configured aliases and the custom-command option; require an explicit selection before creation. |
| Same task already `pending` or `in_progress` | Ask whether to reuse it or create another. |
| Mechanical name collision only | Append `-2`, `-3`, and so on. |
| Several independent tasks | Ask which one task to launch first. |
| Material repo or target-branch ambiguity | Ask one focused question. |

If the current repository is not registered, register it non-interactively with
its repository root, derived repository name, and current branch before create.

## Choose the agent

Skip this question when the invocation already contains a concrete agent alias
or literal command.

Build the choices from `zootree config agents --json`:

1. Put `default` first. Show its `value`, `kind`, and resolved `command`.
2. List every remaining entry from `aliases` with its command template.
3. Offer **Custom command**: the user describes the agent, mode, permissions,
   and other desired behavior in natural language.

Ask the user to accept the default, select an alias, or describe the custom
command. The question is complete only when one concrete launch mode is known.

For a custom description, generate a literal command template in real time.
Check the selected executable's local help when a requested flag is uncertain,
include `$prompt` exactly once, and make the final value shell-safe. Ask a
focused follow-up only when the executable, permission level, or requested mode
cannot be inferred safely.

Map the result to CLI arguments as follows:

- Default: pass a bare `--run-agent` so zootree resolves `agent_cli`.
- Selected non-default alias: pass `--run-agent "$agent_alias"`.
- Generated command: pass `--run-agent "$generated_agent_command"`.

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

## Verify and report

Run `zootree info "$workspace_name"`. Confirm status, branch, repositories, and
persisted agent selection. Do not inspect cmux or Zellij panes unless create or
info output reports a problem.

Report only the workspace name, title, branch, repository targets, and whether
create, start, and info verification succeeded.
