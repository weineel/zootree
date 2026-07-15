# Launching zootree workspace skill design

## Context

The repository currently ships two skills:

- `zootree-usage` documents end-user commands and gives agents guardrails for
  non-interactive execution.
- `zootree-dev` documents the Rust architecture and project development
  conventions.

`zootree-usage` already explains how to create a workspace and run an agent,
but it does not define a dedicated, explicit workflow that turns the current
conversation into one executable task. It also mixes the high-frequency agent
guardrails with installation, command reference, configuration, hook, and
layout documentation. That makes its responsibility broad and loads more
context than most invocations require.

The new capability should let a user explicitly request one action: summarize
the decisions in the current conversation, create one zootree workspace, start
the configured default agent, and verify the result.

## Goals

- Add an explicitly triggered conversation-to-workspace skill.
- Treat explicit invocation as authorization to create and start the workspace.
- Create exactly one workspace per invocation, with one or more strongly related
  repositories.
- Generate a compact, structured agent brief instead of copying the transcript.
- Use complete non-interactive zootree arguments and avoid the create wizard.
- Use the default `agent_cli` from `~/.config/zootree/config.toml` by passing a
  bare `--run-agent` flag.
- Verify successful creation and launch with `zootree info <workspace>`.
- Refactor `zootree-usage` for progressive disclosure without changing its
  existing command semantics.
- Keep the skill package portable across agents that support the common Agent
  Skills format.

## Non-goals

- Do not modify zootree's Rust runtime behavior.
- Do not launch a workspace merely because a conversation mentions zootree.
- Do not create multiple independent workspaces from one invocation.
- Do not copy or move uncommitted changes into a new worktree automatically.
- Do not embed the complete conversation in the workspace description.
- Do not hard-code `codexd` or another agent alias as the default.
- Do not duplicate the complete zootree command reference in the new skill.
- Do not refactor `zootree-dev` as part of this change.

## Skill boundaries

The target structure is:

```text
skills/
├── launching-zootree-workspace/
│   ├── SKILL.md
│   └── agents/openai.yaml
└── zootree-usage/
    ├── SKILL.md
    ├── agents/openai.yaml
    └── references/
        ├── commands.md
        ├── configuration.md
        └── layouts.md
```

### `launching-zootree-workspace`

This skill owns orchestration:

- enforce explicit invocation;
- extract one task from the current conversation;
- inspect the local repository and zootree state;
- resolve only material ambiguities;
- generate title, description, name, branch, and repository arguments;
- execute `zootree create ... --run-agent`;
- verify with `zootree info`.

Its frontmatter description must be narrow enough to trigger only when the user
explicitly invokes the skill or explicitly asks to launch a zootree workspace
from the current conversation. The body must repeat that condition as a hard
execution gate.

The skill must declare `zootree-usage` as a required sub-skill for command facts
instead of reproducing the general command reference.

### `zootree-usage`

This skill remains the source of truth for general zootree usage and
non-interactive agent execution. Its main `SKILL.md` retains:

- core concepts;
- the non-interactive execution contract;
- required arguments and command-order traps;
- a compact create-and-run example;
- routing instructions for optional references.

Installation and the extended command reference move to `references/commands.md`.
Global and repository configuration, aliases, and hooks move to
`references/configuration.md`. Multiplexer and layout details move to
`references/layouts.md`.

References remain one level below `SKILL.md` and are read only when the current
request needs them.

### Frontmatter portability

Do not add `disable-model-invocation`. It is not part of the currently validated
portable frontmatter used by this repository. Keep
`policy.allow_implicit_invocation: true` in the new skill's
`agents/openai.yaml` so Codex can discover it from an explicit natural-language
request as well as `$skill` invocation. Use the narrow description and body
execution gate to prevent an ordinary zootree discussion from authorizing a
workspace mutation.

Generate `agents/openai.yaml` for the new skill and the refactored usage skill so
their UI metadata stays aligned with `SKILL.md`. Leave implicit invocation
enabled for the general `zootree-usage` skill.

## Invocation and authorization

The workflow runs only when the user:

- explicitly invokes `$launching-zootree-workspace`; or
- explicitly asks to create and immediately launch a zootree workspace from the
  current conversation.

Invocation itself is authorization to perform the local create/start operation.
The skill does not show a second confirmation when all material fields are
unambiguous. It pauses for one question only when proceeding would require a
meaningful user decision.

## Task brief contract

The workspace title and description together become the prompt for the launched
agent. The title is already prepended by zootree, so the description must not
repeat it.

The generated description uses this shape:

```text
Context:
- Why the task exists and the minimum relevant background.

Confirmed decisions:
- Decisions the user explicitly accepted.

Scope:
- Required implementation or investigation boundaries.

Constraints and non-goals:
- Behavior that must remain unchanged.
- Explicit exclusions.

Acceptance criteria:
- Observable completion conditions.

Expected verification:
- Relevant commands or checks.
```

Include only facts and decisions relevant to the new agent. Exclude exploratory
dead ends, rejected approaches, superseded assumptions, unrelated conversation,
credentials, tokens, private keys, and other secrets.

## Workspace scope

One invocation creates exactly one workspace.

- The current Git repository and current branch are the default repository and
  target branch.
- Add another repository only when the conversation clearly establishes that it
  participates in the same task.
- Use the other repository's configured default target branch unless the user
  explicitly selected a different branch.
- If the conversation contains independent tasks, stop and ask which single task
  to launch first.
- If repository membership or target branch has multiple materially different
  interpretations, ask one focused question.

If the current repository is not registered with zootree, register it
non-interactively before creation using its repository root, derived name, and
current branch.

## Derived fields

Generate fields without asking when their derivation is mechanical:

- `title`: use `<type>(<scope>): <subject>`. Infer the Conventional Commit type
  from the dominant task. Use a source module, command, or behavior boundary as
  the scope. Omit the scope when it is unclear; never use `other` as a
  placeholder.
- `name`: derive a short lowercase hyphenated slug from the subject.
- `branch`: use `<configured branch_prefix>/<name>`.
- `repos`: use `repo:target-branch` entries for all strongly related
  repositories.
- `description`: use the structured task brief contract above.

For a mechanical name collision, append `-2`, `-3`, and so on, and keep the
branch consistent with the selected name.

## Read-only preflight

Before mutating state, inspect:

- Git repository root and current branch;
- working-tree status and relevant diffs when needed;
- zootree repository registration and target-branch configuration;
- global `branch_prefix` and default `agent_cli`;
- existing workspace names and active workspace summaries.

Apply these decisions:

- If uncommitted changes appear related to the task, stop and ask whether the new
  workspace should still start from committed `HEAD`.
- If uncommitted changes are clearly unrelated, continue without touching them.
- If the global default `agent_cli` is missing, stop before workspace creation
  and ask the user to configure it or explicitly select an alias/command.
- If an active `pending` or `in_progress` workspace clearly represents the same
  task, ask whether to reuse it or create another.
- If only the generated name collides with an unrelated workspace, choose the
  next numeric suffix automatically.

## Execution

The default command shape is:

```bash
zootree create \
  --title "<conventional title>" \
  --description "<structured task brief>" \
  --name "<workspace name>" \
  --branch "<configured prefix>/<workspace name>" \
  --repos "<repo:target-branch,...>" \
  --run-agent
```

Do not pass a value after `--run-agent` by default. A bare flag selects the
configured global `agent_cli`. Use `--run-agent <alias-or-command>` only when the
user explicitly selected it.

All values must be shell-safe. Documentation placeholders must be replaced with
concrete values before execution.

## Verification and response

After `zootree create` succeeds, run:

```bash
zootree info <workspace-name>
```

Confirm the workspace status, branch, repositories, and persisted agent
selection. Do not inspect cmux or Zellij panes unless the create output or info
output indicates a problem.

The final response reports only:

- workspace name;
- task title;
- workspace branch;
- repositories and target branches;
- whether creation, start, and `zootree info` verification succeeded.

## Existing instruction updates

Update `AGENTS.md` and `zootree-usage` so their default launch examples use a
bare `--run-agent`. Preserve explicit aliases only in examples that are
specifically demonstrating alias selection.

The repository rule becomes:

- default: `--run-agent`, using the configured global `agent_cli`;
- explicit override: `--run-agent <alias-or-command>`.

## Evaluation-driven implementation

Skill work follows RED-GREEN-REFACTOR.

### RED baseline

Run fresh-agent scenarios against the current skills before authoring the new
skill. At minimum, test a conversation containing confirmed decisions,
superseded alternatives, acceptance criteria, and enough repository context to
launch one task.

Record whether the baseline agent:

- recognizes explicit authorization;
- creates exactly one workspace;
- generates a structured brief instead of copying the transcript;
- uses a bare `--run-agent`;
- performs the required preflight and verification.

If the control already satisfies every evaluation criterion, stop and report
that a new behavioral skill is redundant. Do not author a skill without an
observed gap; consider a minimal discoverability wrapper only after discussing
that result with the user.

### GREEN and REFACTOR

Create the minimal new skill that addresses observed baseline failures. Test it
with fresh-agent scenarios covering:

- complete context and successful direct launch;
- explicit alias override;
- non-explicit zootree discussion;
- multiple independent tasks;
- related uncommitted changes;
- missing default `agent_cli`;
- an active duplicate task;
- a mechanical workspace-name collision;
- transcript noise, rejected alternatives, and secret-like content;
- successful `zootree info` verification.

Then refactor `zootree-usage` for progressive disclosure and repeat the launch
evaluations plus representative general-usage retrieval tasks.

Do not batch unverified skill changes. Finish and validate one skill change
before moving to the next.

## Structural validation

- Validate each changed skill with the skill validator in an environment that
  provides its YAML dependency.
- Confirm skill names, frontmatter fields, descriptions, and reference paths.
- Regenerate and inspect `agents/openai.yaml` after the final `SKILL.md` content
  is stable.
- Keep each reference directly linked from the root `SKILL.md`.
- Run `git diff --check` for all changed documentation.
- Recheck the real create gate and default-agent resolution in the Rust source
  so documentation remains aligned with runtime behavior.

No Rust test suite is required unless implementation uncovers and changes a
runtime mismatch.

## Acceptance criteria

- The new skill triggers only on explicit user intent.
- The new skill's OpenAI metadata sets `policy.allow_implicit_invocation: true`
  so explicit natural-language requests remain discoverable.
- One explicit invocation can safely create and launch one workspace without a
  second confirmation when the task is unambiguous.
- The launched agent receives the confirmed task decisions, scope, constraints,
  acceptance criteria, and verification guidance without receiving the full
  transcript.
- Default launch uses the global `agent_cli` through a bare `--run-agent`.
- Material ambiguity or unsafe local state produces one focused question before
  mutation.
- Successful execution is verified with `zootree info`.
- `zootree-usage` remains behaviorally correct while loading detailed reference
  material only when needed.
- Repository instructions no longer require `codexd` as the default agent.
