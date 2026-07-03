# zootree create Review/Draft Scroll Design

## Goal

When `zootree create` uses the fullscreen wizard, long descriptions can make the
Review page and Draft summary overflow their visible areas. Users must be able
to read the complete Review content and the complete Draft summary before
submitting.

## Scope

This change is limited to the create wizard TUI layer in
`src/tui_app/create_wizard.rs`.

It does not change `CreateDraft`, workspace config serialization, create-flow
validation, repo selection, or submit behavior.

Mouse support is explicitly out of scope for this change. The shared TUI event
framework currently forwards only key, resize, paste, and tick events; adding
mouse support would require a separate change to the framework and all apps that
share it.

## Architecture

`CreateWizardApp` will keep two independent scroll offsets:

- `review_scroll` for the Review page's main content.
- `draft_scroll` for the Draft summary panel.

Both offsets are UI-only state. They are not stored in `CreateDraft` and do not
affect the final `CreateWizardOutput`.

The existing content builders remain the source of truth:

- `summary_lines()` continues to build the full Draft summary.
- `page_content_lines()` continues to build the full Review content.

Rendering applies a viewport scroll to those complete line lists. A small shared
helper will clamp scroll offsets based on content length and render area height,
following the same pattern already used by `InfoApp::render_body`.

## Interaction

On the Review page:

- `j` / `k` and `Up` / `Down` scroll the Review content by one line.
- `PageDown` / `PageUp` scroll the Review content by one page.
- `Home` / `End` jump the Review content to the top or bottom.
- `Enter` still submits.
- `Esc` still goes back.
- `Ctrl+C` still aborts.

The Draft summary uses modified keys so it does not steal normal Review
navigation:

- `Alt+j` / `Alt+k` and `Alt+Down` / `Alt+Up` scroll Draft by one line.
- `Alt+PageDown` / `Alt+PageUp` scroll Draft by one page.
- `Alt+Home` / `Alt+End` jump Draft to the top or bottom.

These scroll bindings apply only where they make sense. Text input pages still
send normal characters such as `j` and `k` to `ratatui-textarea`; existing text
entry behavior must not regress.

The Review help text will advertise both scroll targets, for example:

`enter submit · esc back · up/down review · alt+up/down draft · pg/home/end scroll · ctrl+c abort`

Other pages keep their current help text.

## Rendering

In two-column layout:

- The left step area renders Review content with `review_scroll`.
- The right Draft panel renders summary content with `draft_scroll`.

In single-column layout:

- The top Draft area renders summary content with `draft_scroll`.
- The lower step area renders Review content with `review_scroll`.

The scroll offsets are retained when leaving and returning to Review. This lets a
user inspect content, go back to edit a field, and return without losing their
place.

## Error Handling

Scroll offsets are saturating. Scrolling above the top clamps to zero; scrolling
past the bottom clamps to the maximum valid offset during render. If content fits
within the visible area, scroll keys are harmless no-ops.

Validation errors on Review remain part of the Review content and are reachable
through the same `review_scroll` viewport.

## Testing

Add focused tests in `tests/create_wizard_test.rs`:

- A long description on Review is initially clipped, and `End` reveals the last
  Review line.
- A long description in the Draft summary is initially clipped, and `Alt+End`
  reveals the last Draft line.
- Review scrolling does not change submit behavior: `Enter` still submits.
- Existing text-entry contracts stay intact, especially that `j` / `k` on text
  pages are inserted as text rather than treated as movement.

Run at least:

```bash
cargo fmt --check
cargo test --test create_wizard_test
```
