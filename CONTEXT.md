# zootree

zootree manages multi-repo development workspaces and the terminal environments attached to them.

## Language

**Workspace**:
A named multi-repository development task managed by zootree, including its lifecycle state and one Terminal environment.
_Avoid_: Project, task workspace

**Reopen**:
The lifecycle action that returns a done or canceled Workspace to in progress so work can continue.
_Avoid_: Restore, resume, reactivate

**Workspace repository**:
A registered repository included in a Workspace. For an in-progress Workspace, membership includes its development checkout and its place in any existing Terminal environment.
_Avoid_: Attached repo, workspace member

**Workspace instruction index**:
A workspace-root guidance document that directs an agent to the available repository-local instructions for every Workspace repository. It is derived from current Workspace repository membership and does not copy those instructions.
_Avoid_: Combined instructions, copied instructions

**Target branch**:
The existing branch in a registered repository from which a Workspace branch starts and into which it is later integrated.
_Avoid_: Base branch, destination branch

**Workspace branch**:
The development branch named by a Workspace and created in each Workspace repository. Its ownership is established only when zootree creates it.
_Avoid_: Feature branch, worktree branch

**Terminal environment**:
The complete set of terminals managed as one lifecycle object for a zootree workspace, independent of the terminal multiplexer's native object model.
_Avoid_: Multiplexer session, terminal session

**Global configuration**:
The user-managed settings that apply across zootree workspaces and registered repositories.
_Avoid_: Config directory, repository configuration

## Lifecycle boundary

Workspace workflows activate or close a Terminal environment through the single public lifecycle facade. Zellij and cmux reconciliation, layout preparation, runtime references, and command translation remain internal implementation details.

Activation is idempotent: it adopts one uniquely identified existing environment or creates one when none exists, and refuses ambiguous matches. Close is idempotent and best-effort after the workspace reaches its final status.

Stored runtime references are opaque recovery hints, not the Terminal environment's identity.
