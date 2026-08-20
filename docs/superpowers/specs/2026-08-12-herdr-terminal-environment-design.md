# Herdr terminal environment design

## Context

zootree currently activates Terminal environments through built-in Zellij and cmux adapters. Herdr 0.8.0 exposes persistent named sessions containing workspaces, tabs, and panes through a JSON-producing CLI. This feature adds Herdr without changing the public `TerminalEnvironment::activate` / `close` lifecycle facade.

## Goals

- Add `herdr` as a third built-in terminal-environment adapter.
- Preserve the existing Workspace lifecycle, `--run-agent`, partial-success, warning, and opaque-state contracts.
- Build a deterministic multi-repository Herdr topology using public CLI wrappers.
- Recover, focus, and close one owned Herdr workspace without taking ownership of its shared named session or Git worktrees.

## Non-goals

- Do not change the default or recommended multiplexer.
- Do not add a Herdr layout DSL or reuse Zellij KDL/cmux JSON.
- Do not start, supervise, stop, or delete Herdr named sessions.
- Do not use Herdr worktree creation or removal.
- Do not repair or reset a user-modified existing Herdr topology.
- Do not launch one agent per repository.
- Do not implement the Herdr raw socket protocol or event subscriptions.

## Confirmed decisions

### Native object mapping

One zootree Workspace maps to one Herdr workspace. Tabs and panes express the multi-repository terminal topology inside that object. Multiple zootree Workspaces may coexist in the same configured Herdr named session.

### Named session selection

`multiplexer.herdr.session` selects the target named session and defaults to `default`. zootree does not infer the target from `HERDR_SESSION`, `HERDR_SOCKET_PATH`, or the caller's focused Herdr UI. Successful activation stores both the session name and Herdr workspace ID in the opaque terminal-environment state; stored state remains authoritative for later activation and close operations.

### Server lifecycle

zootree does not start, stop, or supervise a Herdr server. If the configured named session is not running, activation returns an actionable error. A failed `start` keeps the Workspace `in_progress` for a later `zootree open` retry. An unavailable server during `done` or `cancel` produces a close warning and does not undo archival.

### Tab topology

Every Herdr workspace contains:

- one `overview` tab, reusing the initial tab and root pane returned by `herdr workspace create`;
- one tab per repository, labeled with the repository name and rooted at its worktree path.

Single-repository Workspaces still contain both the `overview` and repository tabs. This keeps the topology consistent with existing terminal adapters and avoids separate single- and multi-repository lifecycle paths.

### Default pane topology

The built-in Herdr layout follows the current cmux default rather than the older Zellij layout:

```text
overview tab
├── left:  zootree info <workspace> --watch
└── right: primary terminal

repository tab
├── left:  primary terminal
└── right
    ├── top:    shell
    └── bottom: shell
```

Every split uses a `0.5` ratio. A primary terminal falls back to an ordinary shell when no agent is assigned to it. The built-in layout does not launch LazyGit.

### Agent placement

Herdr preserves the existing `--run-agent` routing contract:

- with one repository, the agent runs in that repository tab's primary terminal;
- with multiple repositories, the agent runs in the `overview` tab's primary terminal;
- repository tabs do not each receive an agent in the multi-repository case;
- without `--run-agent`, every primary terminal remains an ordinary shell.

### Agent launch and name

The adapter resolves `agent_cli` and aliases through the existing zootree command-template contract, substitutes `$prompt`, and submits the resulting shell-safe command with `herdr pane run`. It does not require a Herdr-supported canonical agent kind.

After Herdr detects the process as an agent, zootree assigns the live agent name `zt-<workspace-name>`. The name is normalized to Herdr's `[a-z][a-z0-9_-]{0,31}` contract; inputs that cannot fit are truncated with a deterministic short hash suffix. Detection or rename failure produces an activation warning rather than rolling back a command that is already running. The name is ephemeral and is not part of the stored recovery identity.

### Focus and client attachment

Workspace, tab, and pane creation uses `--no-focus`. Only after the complete topology and any requested agent command have been created does activation focus the Herdr workspace.

When invoked outside Herdr, successful `start` and `open` attach an interactive client to the configured named session. When invoked from a Herdr pane, zootree never nests another Herdr client: it focuses the workspace when the caller belongs to the target session, or returns a warning with the target attach command when the caller belongs to another session. `start` and `open` share this behavior through the same activation seam.

### Display label and recovery

A Herdr workspace uses the label `<title> · zootree:<name>`, combining a human-readable task title with a deterministic zootree suffix.

Activation reconciles inside the stored named session in this order:

1. get the stored Herdr workspace ID;
2. if that ID is stale, find an exact unique match for the stored label;
3. when no stored label is available, find an exact unique match for the label derived from the current Workspace;
4. adopt a unique match, create on no match, and refuse to guess when multiple exact matches exist.

Canonical opaque state stores `session`, `workspace_id`, and `label`. Tab and pane IDs are creation-time handles and are not persisted because later activation and close operate on the Herdr workspace boundary.

### Reusing an existing environment

Activation that finds an existing Herdr workspace adopts and focuses it without inspecting, repairing, or recreating its tabs and panes. User layout changes are preserved. Commands such as `zootree info` and the requested agent are not launched again; a non-empty agent intent produces the same ignored-request warning as the existing adapters. The initial default topology is applied only while creating a new Herdr workspace.

### Layout configuration scope

The first Herdr adapter supports only the built-in default topology. `HerdrMultiplexerConfig` contains only `session` and does not expose a `layout` field. Zellij KDL and cmux JSON are not reused, and this feature does not introduce a third layout DSL. Custom Herdr topology can be designed separately if a concrete need emerges.

### Creation transaction

Creating the Herdr workspace, renaming its initial tab, building every tab and pane, starting `zootree info`, and submitting any requested agent command form one transaction. Failure in any of those steps closes the newly created Herdr workspace; a rollback failure is attached to the original error.

Agent detection and live-name assignment are post-launch enhancements and produce warnings rather than rollback. Final focus and interactive client attachment are presentation steps: their failure returns an actionable warning while preserving the complete environment and canonical state.

### Close reconciliation

Close always operates in the stored named session. It prefers the stored workspace ID, then uses an exact unique stored-label match, and finally uses an exact unique currently derived label when no stored label exists. No match is already closed; ambiguity produces a warning and refuses to guess. Server, inspection, and close failures are warnings after Workspace archival.

The adapter closes only the owned Herdr workspace. It never stops the shared named session and never invokes Herdr's Git worktree removal commands; Git checkout ownership remains in zootree's existing `GitOps` lifecycle.

### Compatibility defaults

Herdr is added as an explicit `MultiplexerKind` without changing existing selection behavior. Zellij remains the serde and runtime default, cmux retains its current recommended positioning in user documentation, and existing configurations require no migration. Herdr can be reconsidered as a recommended or default adapter only after its recovery, attachment, and agent-detection behavior has real-world validation.

### Version and output contract

Activation requires Herdr `0.8.0` or newer and checks `herdr --version` before any mutating command. A missing or older binary produces an actionable error. Command responses are decoded from their documented JSON fields; IDs are never scraped from display text. Invalid response shapes fail activation and participate in creation rollback when mutation has begun. Close does not enforce the version gate and instead attempts best-effort cleanup so a later downgrade cannot prevent archival.

### Agent detection deadline

After submitting an agent command, zootree waits up to five seconds for Herdr to recognize an agent in the target pane. It assigns the live name as soon as detection succeeds and does not wait for an idle or ready state. Timeout, unsupported-agent detection, and rename failure produce warnings. The deadline is fixed in the first release and applies only to a requested agent in a newly created environment.

### Configuration inheritance

The Herdr session follows the existing multiplexer configuration lifecycle. At Workspace creation, zootree copies the selected template multiplexer configuration or the then-current global multiplexer configuration into `WorkspaceConfig`. Initial activation reads that frozen Workspace value; successful activation then makes the session in opaque stored state authoritative. Later global configuration changes affect only subsequently created Workspaces and never migrate an active environment across Herdr sessions.

### Initial focus

After successful creation, an agent request makes its target pane the active location: the repository primary pane for one repository or the `overview` primary pane for multiple repositories. Without an agent request, the `overview` info pane remains active. This selection uses creation-time tab and pane IDs and does not depend on successful agent detection or naming. Focus errors remain presentation warnings.

### Stored state envelope

Herdr uses the existing version `1` terminal-environment envelope because that version already discriminates `adapter` and carries an adapter-private `payload`. Its payload contains non-empty `session`, `workspace_id`, and `label` fields and rejects unknown fields. Corrupt or unusable payloads produce a warning and fall back to reconciliation in the configured session by the currently derived label; adding an adapter does not change the envelope schema.

### Integration layer

zootree uses Herdr's CLI wrappers through the existing `CommandRunner` abstraction. It does not open the Unix socket or implement protocol `19`. Every session-scoped request uses an explicit `herdr --session <session> ...` prefix, while interactive presentation uses `herdr session attach <session>`.

## Configuration

`MultiplexerKind` gains `Herdr`, while its default remains `Zellij`. `MultiplexerConfig` gains a defaulted `herdr` section:

```toml
[multiplexer]
kind = "herdr"

[multiplexer.herdr]
session = "default"
```

The Rust model is conceptually:

```rust
pub struct HerdrMultiplexerConfig {
    pub session: String,
}
```

The struct rejects unknown fields and defaults `session` to `default`. An empty session is invalid at activation. Template and Workspace configuration serialization use the same complete `MultiplexerConfig` shape as the existing adapters.

## Stored state

Successful activation writes:

```toml
[multiplexer_state]
version = 1
adapter = "herdr"

[multiplexer_state.payload]
session = "default"
workspace_id = "w7"
label = "Support Herdr · zootree:rare-moon"
```

`HerdrStatePayload` is private to `core::terminal_environment::herdr`. When a stored ID resolves, the adapter records the current label returned by Herdr in the next canonical state; this preserves recovery if the user renamed the workspace after creation.

## Module boundaries

The stable public lifecycle facade remains unchanged:

```text
src/core/terminal_environment/mod.rs
├── decode opaque state and choose adapter
├── terminal_environment/zellij.rs
├── terminal_environment/cmux.rs
└── terminal_environment/herdr.rs
    ├── validate Herdr selection and state
    ├── derive label, topology, commands, and agent placement
    ├── reconcile activate/close
    └── translate adapter outcomes to Activation/CloseReport

src/core/multiplexer/mod.rs
├── multiplexer/zellij.rs
├── multiplexer/cmux.rs
└── multiplexer/herdr.rs
    ├── build CommandSpec values
    ├── execute Herdr CLI wrappers
    ├── decode documented JSON result/error shapes
    └── implement creation rollback
```

`core::multiplexer::herdr` remains crate-private and exposes only the narrow commands/outcomes needed by the Herdr terminal-environment adapter. Workspace CLI handlers do not parse Herdr IDs, labels, tabs, panes, or errors.

Caller context used to avoid nested clients is captured at the lifecycle boundary and kept testable. `HERDR_ENV`, the caller socket/session context, and the configured target session influence presentation only; they never select the target environment.

When `HERDR_ENV=1`, zootree compares the injected `HERDR_SOCKET_PATH` with the configured session's `socket_path` from `herdr session list --json`. A match means the caller is already inside the target session. A mismatch means another session; missing or malformed caller/session evidence is treated conservatively as an unknown Herdr session, so zootree never starts a nested client and instead returns the explicit attach instruction.

## Native command flow

### Discovery and adoption

Activation first runs `herdr --version`, then targets the selected session explicitly:

```text
herdr --session <session> workspace get <stored-id>
herdr --session <session> workspace list
```

Workspace records are decoded from `.result.workspace` or `.result.workspaces`. Label matching is exact. A resolved stored ID wins even if its current label differs; otherwise a unique stored or derived label is adopted. Existing topology is not listed or reconciled.

### Creation

The imperative creation sequence is:

```text
herdr --session <session> workspace create \
  --cwd <workspace-dir> --label <label> --no-focus
herdr --session <session> tab rename <initial-tab-id> overview

# Overview: initial info pane plus a new primary pane on the right.
herdr --session <session> pane split <overview-root-pane-id> \
  --direction right --ratio 0.5 --cwd <workspace-dir> --no-focus
herdr --session <session> pane run <overview-root-pane-id> \
  <shell-safe-zootree-info-command>

# Repeated once per repository.
herdr --session <session> tab create \
  --workspace <workspace-id> --cwd <worktree-path> \
  --label <repo-name> --no-focus
herdr --session <session> pane split <repo-root-pane-id> \
  --direction right --ratio 0.5 --cwd <worktree-path> --no-focus
herdr --session <session> pane split <repo-right-pane-id> \
  --direction down --ratio 0.5 --cwd <worktree-path> --no-focus
```

The root pane of each repository tab is its left primary terminal. The first split creates the right-hand shell area; splitting that pane downward yields the two right-hand shells. All user paths and command strings are passed as discrete `CommandSpec` arguments rather than concatenated into an executable shell invocation.

`zootree info <workspace> --watch` is shell-joined with the existing quoting utilities before `pane run`. The resolved agent command uses the existing `resolve_agent_cli`, `build_prompt`, and shell-safe command builder.

### Agent launch and naming

For one repository, the agent command is submitted to that repository tab's root pane. For multiple repositories, it is submitted to the overview right pane. The adapter then polls by target pane for at most five seconds and, when detected, assigns the normalized `zt-<workspace-name>` live name:

```text
herdr --session <session> agent get <pane-id>
herdr --session <session> agent rename <pane-id> <agent-name>
```

Polling ends immediately on successful detection. Timeout and rename errors become warnings. The implementation must not derive a Herdr canonical kind from the arbitrary `agent_cli` command.

### Focus and attachment

After construction commits, activation selects the requested landing location. An agent target selects its tab and pane; without an agent, the initial overview/info pane remains active. The adapter then focuses the Herdr workspace.

Outside Herdr, it invokes `herdr session attach <session>` through `CommandRunner::run_interactive`. Inside the target session it returns after focusing. Inside another Herdr session it does not nest a client and returns a warning containing the explicit attach command.

Focus, navigation, or interactive attach failure is a presentation warning. Activation still returns canonical state so the caller persists ownership and later cleanup remains deterministic.

### Close

Close uses the same stored-ID and exact-label lookup order without the version gate. It issues only:

```text
herdr --session <stored-session> workspace close <workspace-id>
```

A missing target is success. Ambiguous lookup, an unavailable session, malformed JSON, and close failure become `CloseReport.warnings`. No close path invokes `session stop`, `server stop`, or `worktree remove`.

## Error and recovery contract

| Scenario | Result |
|---|---|
| `herdr` missing or older than 0.8.0 | Activation error before mutation |
| Configured session/server not running | Actionable activation error with `herdr session attach <session>` |
| Stored ID exists | Adopt it and refresh canonical label/state |
| Stored ID stale, exact unique label exists | Adopt it and warn about stale state |
| No matching workspace | Create a complete new environment |
| Multiple exact label matches | Activation error; do not create or guess |
| Tab/pane/info/agent submission fails during creation | Close the new Herdr workspace and return the original error with rollback context |
| Agent detection or naming fails | Preserve environment and return warning |
| Focus or client attachment fails | Preserve environment/state and return recovery warning |
| Existing environment receives agent intent | Ignore it and return warning |
| Final close cannot find target | Treat as already closed |
| Final close cannot safely identify or close target | Preserve final Workspace status and return warning |

`zootree start` retains its existing caller contract: worktrees and `in_progress` state survive activation errors, and `zootree open <name>` retries activation. `--no-multiplexer` still skips only that `start` invocation.

## Testing strategy

### Configuration tests

- `HerdrMultiplexerConfig::default().session == "default"`.
- Global, template, and Workspace TOML parse and round-trip `kind = "herdr"` and the Herdr session.
- Missing Herdr configuration remains compatible with the Zellij default.
- Unknown Herdr configuration fields and an unknown multiplexer kind fail clearly.
- Herdr v1 opaque state round-trips without exposing its payload through config APIs.

### Command translation unit tests

`src/core/multiplexer/herdr.rs` tests exact program, argv, cwd, environment, interactive mode, exit handling, and JSON parsing for:

- version, get/list/create/focus/close workspace commands;
- named-session listing and caller socket comparison;
- initial-tab rename and repository-tab creation;
- both pane split directions with literal `0.5` ratios;
- info and agent `pane run` commands;
- agent get/rename and bounded detection outcomes;
- interactive named-session attachment;
- malformed responses, JSON error responses, and rollback after every post-create failure point.

### Lifecycle contract tests

`tests/terminal_environment_test.rs` covers Herdr only through `TerminalEnvironment`:

- single- and multi-repository topology and agent placement;
- no-agent shell topology;
- canonical v1 payload creation;
- stored-ID reuse without topology repair or repeated agent launch;
- stale ID recovery by stored label and corrupt/unknown-state recovery by derived label;
- exact-match ambiguity refusal;
- missing/old binary and stopped-session errors before creation;
- creation rollback and rollback-error context;
- detection/name and presentation failures as warnings with state preserved;
- inside-target, inside-other-session, and outside-Herdr presentation behavior;
- close by stored ID, close by unique label, missing-target success, ambiguity, and server/command warnings.

Workspace caller tests retain the existing adapter-neutral assertions for partial-success `start`, retrying `open`, persisted activation state, and best-effort close after `done`/`cancel`.

## Documentation and project synchronization

Implementation must update both `README.md` and `README.zh-CN.md` with:

- the Herdr configuration example and 0.8.0 minimum;
- the requirement to start/attach the configured named session separately;
- default tabs/panes, agent placement/name, focus behavior, and lack of custom layouts;
- confirmation that Zellij remains the default and cmux remains recommended.

Adding `terminal_environment/herdr.rs`, `multiplexer/herdr.rs`, a new config type, or a version-parsing dependency is a structural change and therefore also requires updating `skills/zootree-dev/SKILL.md` after implementation.

## Verification gate

The implementation is not complete until these pass:

```bash
cargo fmt --check
cargo clippy -- -D warnings
cargo test
```
