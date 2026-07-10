# Create Wizard Module Split Design

## Goal

Split the create wizard implementation into focused Rust modules without changing its keyboard behavior, rendered layout, validation flow, or persisted `CreateDraft` model.

## Constraints

- Preserve the public import path under `tui_app::create_wizard`.
- Preserve all keyboard contracts, including text editing, page navigation, repo selection, cancellation, submission, and Review/Draft scrolling.
- Preserve the two-column, single-column, narrow-terminal, help, error, and viewport layout contracts.
- Do not add fields to or otherwise change the persisted `CreateDraft` model.
- Keep `tests/create_wizard_test.rs` as behavior-level coverage; do not rewrite it around implementation details.
- Do not include unrelated fixes or behavior changes.

## Module Structure

Replace `src/tui_app/create_wizard.rs` with a `src/tui_app/create_wizard/` module directory:

- `mod.rs`: stable public facade, module declarations, public re-exports, `CreateWizardOutcome`, and `run_create_wizard`.
- `state.rs`: `CreateWizardApp`, `CreateStep`, `CreateWizardPage`, construction, state accessors, dynamic page state, and state synchronization.
- `navigation.rs`: step validation, text-page commits, page advancement, back/cancel behavior, after-create selection, Review submission, and event routing that coordinates those behaviors.
- `repo_page.rs`: repo labels, filtering, visible-index calculation, cursor movement/windowing, and repo selection toggling.
- `text_field.rs`: `WizardTextField`, single-line/multiline kind metadata, text extraction, paste, and delegation to `ratatui-textarea`.
- `render.rs`: responsive layout, summary and page rendering, help/errors/context, Review/Draft viewport rendering, wrapping-aware scroll calculations, and render-only formatting.

`mod.rs` will re-export the types and functions currently imported by integration tests and callers, so the refactor does not force API churn. Internal cross-module interfaces should use `pub(super)` rather than enlarging the crate's public API.

## State And Data Flow

`CreateWizardApp` remains the only owner of mutable wizard state. Its fields continue to distinguish persisted draft data from transient UI data such as page index, text buffer, cursors, errors, outcome, and viewport scroll positions.

Event handling follows the existing order:

1. Ignore unsupported key event kinds and handle global interruption semantics.
2. Route text-page keys and paste events through `WizardTextField` while reserving Enter, Esc, and Ctrl+C for wizard actions.
3. Route repo-page movement, filtering, and selection through repo helpers.
4. Route Review and Draft scrolling through the existing plain-key and Alt-key contracts.
5. Commit valid page buffers before navigation, rebuild dynamic pages when repo or after-create state changes, and leave invalid pages in place with the same errors.
6. Validate the full draft and produce the same `CreateWizardOutput` only when Review is submitted.

Rendering reads the same state and may update only existing render-derived viewport measurements used for scroll clamping. It does not mutate persisted draft data.

## Compatibility Strategy

The split is implemented as a mechanical extraction. Methods may move between files and receive `pub(super)` visibility, but their logic, ordering, constants, labels, and match arms remain unchanged. Public methods used by `tests/create_wizard_test.rs` remain available on `CreateWizardApp`, and public helper functions remain available from `tui_app::create_wizard`.

Existing unit tests for wrapping-aware scroll clamping move with the rendering helpers. Integration tests continue to exercise the app through real events and rendered buffers, preserving their value as behavior contracts.

## Error Handling

No new error variants or fallback behavior are introduced. Field validation continues to use `CreateDraftError`; field-level failures remain on the current page, step-level errors remain filtered as before, and Review continues to expose the complete validation result. Cancellation remains represented by `CreateWizardOutcome::Cancelled` and `CancelledByUser` at the existing boundaries.

## Verification

Run a baseline before extraction, then rerun focused tests after each coherent module move. Final verification is:

```bash
cargo fmt --check
cargo test --test create_wizard_test
cargo test
git diff --check
```

The refactor is complete only when the existing create wizard tests pass without weakening assertions and the full suite remains green.
