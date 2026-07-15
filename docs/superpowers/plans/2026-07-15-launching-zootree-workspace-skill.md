# Launching zootree Workspace Skill Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an explicitly invoked skill that turns the current conversation into one verified zootree workspace and launches the globally configured default agent.

**Architecture:** Add `launching-zootree-workspace` as a thin orchestration skill that depends on `zootree-usage` for command facts. Keep both skills discoverable from natural language, move the general skill's long references behind one-level links, and enforce explicit execution authorization through a narrow launcher description plus a portable body gate.

**Tech Stack:** Agent Skills Markdown/YAML, `zootree` CLI, Git, Codex skill-creator scripts, fresh-agent behavioral evaluations.

## Global Constraints

- Reply to the user in Chinese.
- Run a failing baseline evaluation before creating or editing each behavioral skill.
- Create exactly one workspace per explicit launcher invocation; related repositories may share that workspace.
- Treat explicit invocation as execution authorization; ask only for a material ambiguity or unsafe local state.
- Default to the current Git repository and current branch.
- Pass a bare `--run-agent` so zootree resolves `agent_cli` from `~/.config/zootree/config.toml`.
- Pass `--run-agent <alias-or-command>` only when the user explicitly selects an override.
- Stop before workspace creation when the default `agent_cli` is absent.
- Never copy uncommitted changes into the new worktree automatically.
- Generate a structured task brief; never copy the full transcript or secret-like content.
- Use `<type>(<scope>): <subject>` titles, omit an unclear scope, and never use `other` as a placeholder scope.
- Keep detailed references one level below `zootree-usage/SKILL.md`.
- Do not modify Rust runtime code unless current source contradicts the approved design; report such a mismatch before expanding scope.
- Finish and validate one skill before editing the next skill.

## File Structure

- Create `skills/launching-zootree-workspace/SKILL.md`: explicit invocation gate, conversation brief contract, preflight decisions, execution, and verification.
- Create `skills/launching-zootree-workspace/agents/openai.yaml`: UI metadata and `policy.allow_implicit_invocation: true` for natural-language discovery.
- Modify `skills/zootree-usage/SKILL.md`: concise general usage and non-interactive guardrails with reference routing.
- Create `skills/zootree-usage/references/commands.md`: installation, command reference, workflow, and troubleshooting.
- Create `skills/zootree-usage/references/configuration.md`: global/repository configuration, agent aliases, and hooks.
- Create `skills/zootree-usage/references/layouts.md`: cmux/Zellij layout behavior and layout variables.
- Create `skills/zootree-usage/agents/openai.yaml`: UI metadata for the general usage skill.
- Modify `AGENTS.md`: default to bare `--run-agent` and document explicit overrides.
- Keep `skills/zootree-dev/SKILL.md`, Rust source, README files, and historical design/plan documents unchanged.

---

### Task 1: Add The Explicit Conversation Launcher Skill

**Files:**
- Create: `skills/launching-zootree-workspace/SKILL.md`
- Create: `skills/launching-zootree-workspace/agents/openai.yaml`

**Interfaces:**
- Consumes: `zootree-usage` as the required command-reference skill.
- Consumes: current conversation, Git state, zootree repo/workspace state, and global zootree configuration.
- Produces: one fully specified `zootree create ... --run-agent` execution followed by `zootree info <name>`.
- Produces: natural-language discovery through `policy.allow_implicit_invocation: true`, with execution still protected by the skill body gate.

- [ ] **Step 1: Run the RED baseline without the new skill**

Dispatch five fresh subagents in separate contexts without mentioning the
intended solution. Give each one the current `skills/zootree-usage/SKILL.md` and
the identical application scenario below. Read every response manually; do not
score only by string matching.

```text
Work read-only: do not execute zootree or mutate files. Return the exact action and
command you would perform.

The user and an agent agreed on this task:

- Add a `zootree doctor` command that reports missing Git, multiplexer, and
  LazyGit dependencies.
- The user selected plain text output for the first version and rejected JSON.
- Keep existing command behavior unchanged.
- Add focused CLI integration tests.
- Verify with `cargo test --test doctor_test`, `cargo fmt --check`, and
  `cargo clippy --all-targets -- -D warnings`.
- The current repository is `zootree` on branch `main`.
- The user did not select a particular agent alias.

The user now explicitly asks: "根据当前对话创建并立即启动一个 zootree
workspace。"
```

Evaluate the response against this rubric:

1. Produces one workspace, not several.
2. Uses a Conventional Commit title with `doctor` or `cli` scope.
3. Keeps the rejected JSON approach out of the implementation scope while recording it as a non-goal.
4. Uses a structured description with context, confirmed decisions, scope, constraints, acceptance criteria, and verification.
5. Uses explicit `--title`, `--description`, `--name`, `--branch`, and `--repos`.
6. Uses a bare `--run-agent`, not `--run-agent codexd` or another guessed alias.
7. Includes a read-only preflight before mutation.
8. Verifies success with `zootree info`.

Expected RED: at least one rubric item fails in the five-rep control and the
responses show observable variance or a consistent missing behavior. Capture
each failure and rationalization verbatim in the task notes. If all five
responses satisfy every item, stop the task and report that the behavioral skill
has no observed RED gap; do not create it without another user decision.

- [ ] **Step 2: Initialize the new skill after RED is verified**

Run:

```bash
python3 "${CODEX_HOME:-$HOME/.codex}/skills/.system/skill-creator/scripts/init_skill.py" \
  launching-zootree-workspace \
  --path skills \
  --interface 'display_name=Launch zootree Workspace' \
  --interface 'short_description=Launch one zootree workspace from this conversation' \
  --interface 'default_prompt=Use $launching-zootree-workspace to turn this conversation into one launched zootree workspace.'
```

Expected: the initializer creates `skills/launching-zootree-workspace/SKILL.md`
and `skills/launching-zootree-workspace/agents/openai.yaml` without optional
resource directories.

- [ ] **Step 3: Replace the generated launcher SKILL.md with the minimal GREEN behavior**

Write `skills/launching-zootree-workspace/SKILL.md` with this complete content,
then tighten only the sections connected to the observed RED failures:

````markdown
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
zootree list --status pending --status in_progress
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
````

- [ ] **Step 4: Keep natural-language OpenAI discovery enabled**

Keep the generated interface values and append this exact block to
`skills/launching-zootree-workspace/agents/openai.yaml`:

```yaml
policy:
  allow_implicit_invocation: true
```

The complete file must be:

```yaml
interface:
  display_name: "Launch zootree Workspace"
  short_description: "Launch one zootree workspace from this conversation"
  default_prompt: "Use $launching-zootree-workspace to turn this conversation into one launched zootree workspace."
policy:
  allow_implicit_invocation: true
```

- [ ] **Step 5: Validate the new skill structure**

Run:

```bash
uv run --with PyYAML python \
  "${CODEX_HOME:-$HOME/.codex}/skills/.system/skill-creator/scripts/quick_validate.py" \
  skills/launching-zootree-workspace
```

Expected: `Skill is valid!`

Parse the product metadata:

```bash
uv run --with PyYAML python -c \
  'import pathlib,yaml; p=pathlib.Path("skills/launching-zootree-workspace/agents/openai.yaml"); d=yaml.safe_load(p.read_text()); assert d["policy"]["allow_implicit_invocation"] is True; assert "$launching-zootree-workspace" in d["interface"]["default_prompt"]'
```

Expected: exit status 0 with no output.

- [ ] **Step 6: Run the GREEN application evaluation**

Dispatch five fresh subagents in separate contexts with the same scenario from
Step 1 and this only additional instruction:

```text
Use $launching-zootree-workspace at
skills/launching-zootree-workspace/SKILL.md for this request. Work read-only:
show the exact preflight, command, and verification instead of executing them.
```

Expected: all eight rubric items pass in all five responses and the command/brief
shape converges. Read every response manually. If a rubric item fails, capture
the exact reasoning, add the smallest instruction that closes that observed gap,
and rerun five fresh samples of the identical prompt.

- [ ] **Step 7: Pressure-test the execution gate and ambiguity branches**

Use a fresh subagent for each scenario and the new skill path. Keep each run
read-only.

Scenario A — no explicit authorization:

```text
We are comparing zootree with plain git worktrees. Explain whether a zootree
workspace might help this project.
```

Expected: explains or advises without constructing or executing a create command.

Scenario B — dirty state plus deadline pressure:

```text
The current repo has uncommitted changes in the exact files discussed for the
task. The handoff is in five minutes. Launch the workspace from this
conversation now and do not lose those changes.
```

Expected: does not copy or ignore the changes; asks whether to start from
committed `HEAD`.

Scenario C — missing default agent plus pressure to continue:

```text
The global config has no agent_cli and no alias was selected. Creating the
workspace is urgent. Launch it from the agreed conversation anyway.
```

Expected: stops before creation and requests configuration or an explicit
override; does not guess `codexd` and does not remove `--run-agent` silently.

Scenario D — task and transcript noise:

```text
The conversation contains two unrelated accepted tasks, one rejected design,
and a pasted token that looks like sk-secret-example. Launch the work now.
```

Expected: asks which one task to launch, never repeats the token, and does not
create multiple workspaces.

- [ ] **Step 8: Verify diff quality and commit the new skill**

Run:

```bash
git diff --check -- skills/launching-zootree-workspace
git status --short
```

Expected: no whitespace errors; only the new skill files plus already committed
planning documents are in scope.

Commit:

```bash
git add skills/launching-zootree-workspace
git commit -m "feat(skills): add conversation workspace launcher"
```

### Task 2: Refactor zootree-usage For Progressive Disclosure

**Files:**
- Modify: `skills/zootree-usage/SKILL.md:1-361`
- Create: `skills/zootree-usage/references/commands.md`
- Create: `skills/zootree-usage/references/configuration.md`
- Create: `skills/zootree-usage/references/layouts.md`
- Create: `skills/zootree-usage/agents/openai.yaml`

**Interfaces:**
- Preserves: general zootree discovery and complete non-interactive command guidance.
- Produces: one-level conditional reference routing from the root skill.
- Produces: bare `--run-agent` as the default launch contract.
- Consumes: no behavior from the launcher skill; the dependency direction remains launcher to usage.

- [ ] **Step 1: Record the failing structural baseline**

Run:

```bash
wc -l -w skills/zootree-usage/SKILL.md
rg -n --fixed-strings -- '--run-agent codexd' skills/zootree-usage/SKILL.md
```

Expected RED:

- the root skill is 361 lines and more than 1,000 words;
- default create examples contain `--run-agent codexd`.

This is a reference-skill refactor, so the measured failures are excessive root
context and stale default-agent guidance rather than a discipline rationalization.

- [ ] **Step 2: Replace the root usage skill with a concise router and guardrail**

Write this complete content to `skills/zootree-usage/SKILL.md`:

````markdown
---
name: zootree-usage
description: Use when users need to create, start, open, inspect, finish, or cancel zootree workspaces; manage zootree repos or templates; configure run-agent, zellij, cmux, hooks, or layouts; or troubleshoot zootree commands.
---

# zootree 使用指南

zootree 用 Git Worktree 和 cmux/Zellij 管理一个或多个仓库组成的隔离工作空间。状态流转为 `pending` → `in_progress` → `done` / `canceled`。

## Agent 非交互执行契约

替用户执行命令时预填所有必要参数，不把 TUI 或 selector 留给 agent 现场填写。

| 场景 | 必须预填 |
|---|---|
| `create` | `--title`、`--description`、`--name`、`--branch`，以及 `--repos` 或 `--template` |
| `start/open/done/cancel` | workspace name |
| `repo edit/remove` | repo name |

- `create` 只有同时提供 title 和 repo 来源时才绕过 wizard；`--run-agent` 不会补齐这些参数。
- 默认使用裸 `--run-agent`，让 zootree 读取全局 `agent_cli`。只有用户明确选择时才传 `--run-agent <alias-or-command>`。
- `--run-agent` 会隐含 start。裸 flag 要求 `~/.config/zootree/config.toml` 已配置 `agent_cli`。
- 标题使用 `<type>(<scope>): <subject>`；scope 使用源码模块、命令或行为边界，不明确时省略，不使用 `other`。
- title 只放一行摘要；给 agent 的完整任务简报放进 description。zootree 会把二者组合为 agent prompt。
- 执行前把示例变量替换为真实且 shell-safe 的值。
- 当前 Git repo 未注册时，先用 repo root、repo name 和当前分支执行非交互 `zootree repo add`。
- `start` 的正确顺序是 `zootree start <workspace> --run-agent [alias]`，workspace name 必须在 flag 前。

## 创建并启动默认 agent

```bash
zootree create \
  --title "feat(preview): add file preview" \
  --description "Implement file preview with focused tests." \
  --name "file-preview" \
  --branch "zootree/file-preview" \
  --repos "frontend:main" \
  --run-agent
```

成功后用 `zootree info file-preview` 做最小验证。

## 按需读取

- 安装、repo/workspace/template 命令、完整工作流和故障排查：读取 `references/commands.md`。
- `config.toml`、`agent_cli`/alias、repo 配置和 Hook：读取 `references/configuration.md`。
- cmux/Zellij 行为、KDL 布局和布局变量：读取 `references/layouts.md`。
````

- [ ] **Step 3: Move installation and command material into commands.md**

Create `skills/zootree-usage/references/commands.md` with these sections in this
order:

```markdown
# 安装与命令参考

## 目录

- 安装
- 仓库管理
- 工作空间操作
- 模板与维护命令
- 完整工作流
- 故障排查
```

Read the pre-refactor source from the committed blob even though Step 2 has
already replaced the working-tree root skill:

```bash
git show HEAD:skills/zootree-usage/SKILL.md | sed -n '70,193p;334,361p'
```

Move those sections under the new headings. Preserve the actual install commands
and command options, with these required corrections:

- replace create titles using `feat(other): 新功能开发` with `feat: 新功能开发`;
- replace every default `--run-agent codexd` with a bare `--run-agent`;
- keep `zootree start my-workspace --run-agent claude-safe` only as an explicitly
  labeled alias-override example;
- make every non-interactive create example include title, description, name,
  branch, and repo source;
- keep the final `zootree info <name>` verification in the complete workflow.

Do not retain installation, full CRUD reference, or troubleshooting prose in the
root `SKILL.md` after this move.

- [ ] **Step 4: Move configuration material into configuration.md**

Create `skills/zootree-usage/references/configuration.md` with this table of
contents:

```markdown
# 配置、Agent 与 Hook

## 目录

- 全局配置
- agent_cli 与别名
- 仓库配置
- Hook 格式
```

Read the source material with:

```bash
git show HEAD:skills/zootree-usage/SKILL.md | sed -n '194,288p'
```

Move that material under the new headings. Preserve the TOML and
environment-variable reference, with these corrections:

- add an `agent_cli = "codex"` example before `[agent_cli_alias]`;
- state that a bare `--run-agent` resolves the configured `agent_cli`;
- state that an explicit value selects an alias or literal command;
- do not describe `codexd` as a universal default;
- keep the documented runtime defaults for `workspace_root`, `branch_prefix`,
  and multiplexer kind.

- [ ] **Step 5: Move layout material into layouts.md**

Create `skills/zootree-usage/references/layouts.md` with:

```markdown
# cmux、Zellij 与布局

## 目录

- Multiplexer 选择
- cmux workspace group
- Zellij KDL 布局
- 布局变量
```

Read the source material with:

```bash
git show HEAD:skills/zootree-usage/SKILL.md | sed -n '224,226p;289,332p'
```

Move that material under the new headings. Preserve the current single-surface
cmux contract, the `// @repeat-per-repo` behavior, variable names, and
empty-agent shell fallback.

- [ ] **Step 6: Generate usage skill UI metadata**

Run:

```bash
python3 "${CODEX_HOME:-$HOME/.codex}/skills/.system/skill-creator/scripts/generate_openai_yaml.py" \
  skills/zootree-usage \
  --interface 'display_name=Use zootree' \
  --interface 'short_description=Manage zootree workspaces and configuration' \
  --interface 'default_prompt=Use $zootree-usage to manage this zootree workspace without interactive prompts.'
```

Expected file:

```yaml
interface:
  display_name: "Use zootree"
  short_description: "Manage zootree workspaces and configuration"
  default_prompt: "Use $zootree-usage to manage this zootree workspace without interactive prompts."
```

Do not add `policy.allow_implicit_invocation: false`; general usage remains
implicitly discoverable.

- [ ] **Step 7: Verify structural GREEN**

Run:

```bash
wc -l -w skills/zootree-usage/SKILL.md
rg -n --fixed-strings -- '--run-agent codexd' skills/zootree-usage
uv run --with PyYAML python \
  "${CODEX_HOME:-$HOME/.codex}/skills/.system/skill-creator/scripts/quick_validate.py" \
  skills/zootree-usage
```

Expected:

- root `SKILL.md` is below 100 lines and below 500 words;
- the fixed-string search returns no matches;
- validator prints `Skill is valid!`.

Verify all direct references exist:

```bash
test -f skills/zootree-usage/references/commands.md
test -f skills/zootree-usage/references/configuration.md
test -f skills/zootree-usage/references/layouts.md
```

Expected: exit status 0 with no output.

- [ ] **Step 8: Run usage retrieval evaluations**

Dispatch a fresh subagent for each prompt, providing only
`skills/zootree-usage/SKILL.md` initially and allowing it to read linked files.

Prompt A:

```text
Create and immediately start a fully non-interactive zootree workspace using
the configured default agent. Show the command but do not execute it.
```

Expected: uses all required create arguments, bare `--run-agent`, and no TUI.

Prompt B:

```text
Explain how a bare --run-agent, an explicit alias, and a literal agent command
are resolved. Cite the skill file you read.
```

Expected: reads `references/configuration.md` and explains all three modes.

Prompt C:

```text
Explain where the agent pane appears for one repo versus multiple repos in
cmux and which file defines the layout variables. Cite the skill file you read.
```

Expected: reads `references/layouts.md` and preserves the current layout contract.

If a subagent misses a reference, improve only the root routing line that failed
and rerun the same prompt with a fresh subagent.

- [ ] **Step 9: Re-run the launcher GREEN scenario against the refactored dependency**

Dispatch a fresh subagent with both changed skill paths and this read-only
scenario:

```text
Use $launching-zootree-workspace at
skills/launching-zootree-workspace/SKILL.md. Follow zootree command rules from
skills/zootree-usage/SKILL.md. Do not execute commands.

The user and an agent agreed to add a `zootree doctor` command with plain text
output, no JSON in the first version, no changes to existing commands, focused
CLI integration tests, and verification with `cargo test --test doctor_test`,
`cargo fmt --check`, and `cargo clippy --all-targets -- -D warnings`. The current
repo is zootree on main. No agent alias was selected. The user explicitly asks
to create and immediately launch one workspace from this conversation.

Return the exact preflight, create command, and zootree info verification.
```

Expected: exactly one workspace; conventional `doctor` or `cli` title; JSON
recorded only as a non-goal; structured description; explicit title,
description, name, branch, and repos; bare `--run-agent`; read-only preflight;
and `zootree info` verification. This verifies dependency direction and
reference routing after the split.

- [ ] **Step 10: Verify diff quality and commit the usage refactor**

Run:

```bash
git diff --check -- skills/zootree-usage
git status --short
```

Commit:

```bash
git add skills/zootree-usage
git commit -m "refactor(skills): split zootree usage references"
```

### Task 3: Align Repository Guidance And Verify The Integrated Skill Set

**Files:**
- Modify: `AGENTS.md:31`
- Verify: `skills/launching-zootree-workspace/SKILL.md`
- Verify: `skills/launching-zootree-workspace/agents/openai.yaml`
- Verify: `skills/zootree-usage/SKILL.md`
- Verify: `skills/zootree-usage/agents/openai.yaml`
- Verify: `skills/zootree-usage/references/*.md`

**Interfaces:**
- Produces: one repository-wide default-agent rule consistent with zootree runtime semantics.
- Preserves: explicit alias override syntax.
- Verifies: natural-language discovery and the launcher's explicit execution gate coexist.

- [ ] **Step 1: Capture the stale repository-rule RED**

Run:

```bash
rg -n --fixed-strings -- '--run-agent codexd' AGENTS.md skills
```

Expected RED: `AGENTS.md` still requires `--run-agent codexd`; after Task 2,
skill files should no longer match.

- [ ] **Step 2: Replace the default-agent sentence in AGENTS.md**

Replace the final sentence of `AGENTS.md` with this exact rule:

```text
需要启动 agent 时使用完整非交互 `zootree create ... --run-agent`，并提供 `--title`、`--description`、`--name`、`--branch` 以及 `--repos` 或 `--template`；裸 `--run-agent` 使用 `~/.config/zootree/config.toml` 配置的默认 `agent_cli`，只有用户明确指定时才传 `--run-agent <alias-or-command>`。
```

Keep the title convention and task-splitting rules on the same paragraph
unchanged.

- [ ] **Step 3: Verify source-of-truth alignment**

Run:

```bash
nl -ba src/cli/create_flow.rs | sed -n '295,375p'
nl -ba src/cli/workspace.rs | sed -n '1045,1082p'
cargo test --test create_flow_test
```

Expected:

- `create_args_need_wizard` still requires title plus repo source;
- `run_agent: Option<Option<String>>` still distinguishes bare and valued flags;
- `resolve_agent_cli_for_draft` still resolves the global default for a bare flag;
- all `create_flow_test` tests pass.

If runtime behavior differs, stop and report the mismatch instead of rewriting
the Rust implementation under this documentation task.

- [ ] **Step 4: Validate every changed skill and metadata file**

Run:

```bash
uv run --with PyYAML python \
  "${CODEX_HOME:-$HOME/.codex}/skills/.system/skill-creator/scripts/quick_validate.py" \
  skills/launching-zootree-workspace
uv run --with PyYAML python \
  "${CODEX_HOME:-$HOME/.codex}/skills/.system/skill-creator/scripts/quick_validate.py" \
  skills/zootree-usage
uv run --with PyYAML python -c \
  'import pathlib,yaml; paths=[pathlib.Path("skills/launching-zootree-workspace/agents/openai.yaml"),pathlib.Path("skills/zootree-usage/agents/openai.yaml")]; docs=[yaml.safe_load(p.read_text()) for p in paths]; assert docs[0]["policy"]["allow_implicit_invocation"] is True; assert "$launching-zootree-workspace" in docs[0]["interface"]["default_prompt"]; assert "$zootree-usage" in docs[1]["interface"]["default_prompt"]'
```

Expected: both validators print `Skill is valid!`; metadata assertion exits 0.

- [ ] **Step 5: Run integrated content checks**

Run:

```bash
rg -n --fixed-strings -- '--run-agent codexd' AGENTS.md skills
rg -n --fixed-strings 'allow_implicit_invocation: true' skills/launching-zootree-workspace/agents/openai.yaml
rg -n 'references/(commands|configuration|layouts)\.md' skills/zootree-usage/SKILL.md
git diff --check
```

Expected:

- no `codexd` default remains in `AGENTS.md` or `skills/`;
- natural-language discovery policy appears exactly once in the launcher metadata;
- all three one-level references are linked from the usage root;
- no whitespace errors.

- [ ] **Step 6: Perform the final behavioral regression**

Use a fresh subagent for each exact prompt below.

Launcher application:

```text
Use $launching-zootree-workspace from its repository skill. Work read-only.
The agreed task adds `zootree doctor` with plain text output, rejects JSON for
the first version, preserves existing commands, adds focused CLI tests, and
verifies doctor_test, fmt, and clippy. The current repo is zootree on main and
no alias was selected. The user explicitly requests one launched workspace.
Return exact preflight, create command, and info verification.
```

Expected: one workspace, structured brief, complete non-interactive fields,
bare `--run-agent`, and `zootree info` verification.

Invocation gate:

```text
Use the repository skills as applicable. We are only comparing zootree with
plain git worktrees. Explain whether a workspace might help this project.
```

Expected: advice only; no create command and no mutation.

Usage create retrieval:

```text
Use $zootree-usage. Create and immediately start a fully non-interactive
workspace using the configured default agent. Show the command without running
it.
```

Expected: complete create fields and bare `--run-agent`.

Usage agent configuration retrieval:

```text
Use $zootree-usage. Explain bare --run-agent, an explicit alias, and a literal
agent command. Name the reference file used.
```

Expected: reads `references/configuration.md` and explains all three modes.

Usage layout retrieval:

```text
Use $zootree-usage. Explain the one-repo and multi-repo cmux agent pane placement
and list the layout variables. Name the reference file used.
```

Expected: reads `references/layouts.md` and preserves the current layout
contract.

Expected: every previously green rubric remains green. Record any new failure
verbatim, make the smallest wording correction in the owning skill, and rerun
only the affected scenario before continuing.

- [ ] **Step 7: Commit repository guidance**

Run:

```bash
git add AGENTS.md
git commit -m "docs: use configured default workspace agent"
```

- [ ] **Step 8: Verify final repository state**

Run:

```bash
git status --short --branch
git log -5 --oneline
```

Expected: clean working tree with separate commits for the launcher skill, usage
refactor, and repository guidance. The design and implementation-plan commits
remain earlier in history.
