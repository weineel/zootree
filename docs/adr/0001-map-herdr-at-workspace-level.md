# Map a Terminal environment to a Herdr workspace

Each Herdr-backed zootree Workspace is represented by one Herdr workspace inside a shared persistent Herdr named session, rather than by its own named session or by one Herdr workspace per repository. This aligns both systems' task-level lifecycle boundaries, keeps tabs and panes available for multi-repository topology, and gives zootree one native object to recover, focus, and close.

The target named session is selected explicitly from the Herdr multiplexer configuration and defaults to `default`; zootree does not infer it from the caller's Herdr environment. Successful activation stores both the session name and Herdr workspace ID in opaque state, so later activation and close operations remain deterministic even when the active shell or configuration changes.

The shared Herdr server remains outside zootree's lifecycle ownership. Activation fails with recovery instructions when the configured session is not running, `start` leaves the Workspace available for an `open` retry, and finalization reports an unavailable server only as a close warning; zootree never starts or stops a server that may contain unrelated Herdr workspaces.

zootree drives Herdr through its JSON-producing CLI wrappers and the existing `CommandRunner` boundary, not by implementing the raw socket protocol. The feature needs request/response orchestration but no event subscription, so the CLI keeps session and socket routing inside Herdr while preserving zootree's testable external-command seam.
