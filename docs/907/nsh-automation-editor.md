# Automation editor NSH migration (#907)

## Scope and authority

Implement the complete automation editor leaf under the authorized full migration in
[nsh-full.md](nsh-full.md). Baseline: `bf0be9adb642077b2cebdb6f081b3ecf01209dba`.
Only `crates/vega_ui/src/automation_editor.rs`, new files below
`crates/vega_ui/src/automation_editor/`, and this document may change. Main owns
fetch/integration, shared NSH wiring, Git/index, builds and CI. Existing unrelated
MCP/settings work must remain intact. No nested agents, local build/test, app
execution, installation, packaging or release.

## Acceptance (recorded before implementation)

- Production editor content is entirely NSH Rust 0.4, using the existing
  `Surface<Snapshot, Request>`. No old GPUI visible input, button, details or page
  wrapper remains. GPUI entities remain internal controller/carrier/host state.
- Preserve public `AutomationEditor` methods/signatures, `Focusable`, `Render`,
  actions and all `AutomationEditorEvent`/submission contracts. Window callers
  retain their trusted automation service and save completion handling.
- Preserve thread/kind/identity addressing, base/latest revision semantics,
  pending request identity, dirty/conflict/cleared states, elapsed updates,
  save/revert and identity replacement via `OpenLatest`. Late save completions
  cannot mutate a different pending request or editor.
- NSH owns the visible editable draft. Shared synchronized text/composition state
  is updated before queued notifications. Save atomically claims the exact current
  draft only when changed, latest exists, no conflict/pending/composition, nonempty
  after trim and at most 16384 UTF-8 bytes. Freeze a claimed draft until matching
  completion; failures retain the exact text and allow retry.
- Old surface/session, programmatic-reset epoch, base/latest revision, pending
  request and observed draft fences reject stale actions. Routine projections,
  theme/language/elapsed changes do not overwrite more recent native edits.
- Preserve native multiline selection, clipboard/undo/IME, Tab/Shift-Tab,
  keyboard button activation, save/close shortcuts, workspace focus entry,
  visible focus indication, accessible field/action labels, narrow layout,
  existing theme tokens and i18n status/error/details text.

## Implementation

Keep the authoritative automation controller in `automation_editor.rs`; add a
private leaf presentation module. Use the existing `TextInput` solely as the
public API's internal carrier; synchronize accepted native drafts into it, and
observe external carrier mutations without reflecting delayed observations back
over newer native drafts. Dirty/save checks read shared draft state, not a queued
carrier snapshot.

Use a mutex-protected shared draft with text, composition, edit revision, reset
epoch and claim state. Native events update it synchronously, then notify the
controller through the bounded asynchronous ordered transport. Requests carry
surface identity plus the displayed authority and observed draft for destructive
actions. Recheck authority and atomically claim on the controller thread. Native
field values subscribe to local edit signals and programmatic-reset epochs, not
ordinary watch text snapshots, preserving current text/selection across refresh.

Create one Surface lazily, retain it across renders, publish current authority and
localized display projection, synchronize locale/appearance, and forward the
existing carrier focus entry into the NSH native field. Render returns only the
Surface entity. Keep trusted save service events and matching completion guards.

## Verification and handoff

Allowed checks: scoped rustfmt, scoped diff/whitespace review, pinned registry API
inspection and source contract review. Add focused development-only Rust tests
for draft byte/IME/claim/reset and stale-authority boundaries; these are **not run
locally** and must be removed with their exclusive support before merge per
repository policy. Main owns compile/tests, required CI, independent review,
final Rust-test cleanup and user native/cross-platform checks. No unrun check is
reported as passed. Exact final file hashes and remaining validation gaps follow.

## READY implementation evidence

Implemented the native field, save/revert actions, status/error/details projection,
IME-aware native shortcuts and focus forwarding. Controller events and service
ownership remain unchanged. Native change notifications are coalesced (at most one
outstanding change notification) and actions are serialized with an explicit
acknowledgment, so the shared bounded queue does not accumulate per-keystroke text
snapshots. Field instances are keyed by reset epoch; late events from discarded
fields cannot overwrite a reverted/refreshed draft. Routine authority/theme/locale
updates do not rebind the native field value or discard selection/undo.

`instruction(&App) -> &str` and `input() -> Entity<TextInput>` remain compatible
internal-carrier APIs. The carrier synchronizes on controller notification/save;
NSH shared draft is authoritative immediately for dirty state and save claims.
Existing window callers do not read `instruction` or mutate `input`; they use
key/dirty/refresh/save completion/Focusable. External programmatic carrier edits
are still recognized before controller actions and queued change synchronization.

### Changed files and SHA-256

- `crates/vega_ui/src/automation_editor.rs`:
  `3cf55a854acfc9f502f9caa166a4a834fb448b7a6d32fe0e7575102da533af45`
- `crates/vega_ui/src/automation_editor/nsh_editor.rs`:
  `5a267aa124207f57c8151c066f5068d827d7c5e63f98d097555248c58e6fa8e1`
- `crates/vega_ui/src/automation_editor/nsh_editor/tests.rs`:
  `3efad39f17824ca850317b5679041ebfc37d6cc5dc6d0abb4084e074da45d7d4`
- `docs/907/nsh-automation-editor.md` (this document; final hash reported separately
  to avoid a self-referential digest).

### Executed checks

- `rustfmt --check --edition 2024 --config skip_children=true` on the three scoped
  Rust files: passed. Formatter: `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`.
- Scoped `git diff --check` and new-file `git diff --no-index --check /dev/null
  <file>`: no whitespace diagnostics. The first new-file observation wrapper
  incorrectly required exit 0; corrected to accept exit 1 for an ordinary diff
  with no diagnostics. This was an observation-harness error, not a code failure.
- Static baseline comparison: public function signatures, key/submission/event
  definitions, Focusable, elapsed/timestamp helpers, localized task details and
  matching-pending completion guard unchanged.
- Pinned registry inspection: native multiline/value/readonly/composition/layout,
  element references, scoped keyboard registration, reactive variant styling and
  existing Surface/OrderedCommands contracts checked against NSH 0.4.0 sources.
- Production scoped source scan: no comments, `unwrap()`/`expect()`, `try_send`,
  panic fallback, GPUI content wrapper or rendered carrier input introduced.
- Seven development tests added for UTF-8 byte limits/raw text, composition and
  duplicate/stale claims, failure/retry retention, epoch reset, clean refresh
  races, surface/key/revision/request fencing and cleared/conflict/pending actions.
  **Tests have not been executed.**

### Main integration review follow-up

Independent review found two P2 regressions in the initial snapshot: an input field recreated by a clean background refresh could replay a previously consumed focus request, and the host no longer handled exported SaveAutomation/CloseAutomationEditor actions. Main fixed both: focus consumption is shared outside epoch-keyed field replacements and deferred until mount; the host-only GPUI container restores the original action handlers with only the NSH Surface as content. An eighth regression covers once-only focus consumption across replacement. Independent incremental review closed both findings with no new concrete defect.

Reviewed final Rust SHA-256: automation_editor.rs `21f7a28fa25c1cc262e79bb9743ea88ab8ce01c5bdb4bae733d369eb12fa321f`; nsh_editor.rs `b49af963fc7c8440502542e22d5ae36f29113abeb374ddafd05329c37059928a`; tests.rs `da676b8d0baf625b084c27f6f554fcfde0e0ced740adc6f583e28b0b2651062d`. Scoped rustfmt/diff passed; these eight regressions and final integrated compilation remain unrun pending other module writers. Static review does not establish native focus/shortcut behavior.

### Integration and remaining gates

Main advanced the shared workspace HEAD during implementation from the supplied
baseline to `9b9541eb88274e1020ca024a2d45fbb80a72720e`; no Git/index/commit operation
was performed by this writer. Final source comparison is against the supplied
baseline; unrelated main changes were not written or restored by this writer.

No compile, Clippy, Rust test, app, packaging or CI run was executed here. Thus
compilation/type/lint validation is **unverified**, not passed. Main should run
its coordinated relevant build and the seven development tests, arrange
non-implementer review, archive/remove `nsh_editor/tests.rs` and its exclusive
`#[cfg(test)] mod tests` wiring before merge, then validate the final integrated
head with required CI. This implementation has no additional dependency or
shared-wiring request. No independently verified product blocker is known.

Native keyboard/IME/focus, clipboard/undo, narrow geometry, scroll behavior,
light/dark and language switching, close/reopen and macOS/Windows/Linux runtime
behavior remain for integration/user validation. Static source checks do not
claim those runtime acceptances passed.
