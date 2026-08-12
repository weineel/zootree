# zootree

zootree manages multi-repo development workspaces and the terminal environments attached to them.

## Language

**Terminal environment**:
The complete set of terminals managed as one lifecycle object for a zootree workspace, independent of the terminal multiplexer's native object model.
_Avoid_: Multiplexer session, terminal session

## Lifecycle boundary

Workspace workflows activate or close a Terminal environment through the single public lifecycle facade. Zellij and cmux reconciliation, layout preparation, runtime references, and command translation remain internal implementation details.

Activation is idempotent: it adopts one uniquely identified existing environment or creates one when none exists, and refuses ambiguous matches. Close is idempotent and best-effort after the workspace reaches its final status.

Stored runtime references are opaque recovery hints, not the Terminal environment's identity.
