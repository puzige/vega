# DiffView production NSH migration

## Scope and baseline

Independent leaf scope at `9b9541eb88274e1020ca024a2d45fbb80a72720e`. Writes are limited to `crates/vega_ui/src/diff_view/*` and this document. ImagePreview, shared `crate::nsh`, dependencies, application integration, Git/index, builds/tests, application execution, packaging and installation are not owned here. Main already synchronized the workspace and owns compilation, independent review and final integration.

## Acceptance plan (before implementation)

| Area | Required observable behavior | Verification |
| --- | --- | --- |
| Production surface | Header/head/counts, refresh/layout/navigation/close controls, retained-snapshot failure, empty/loading states, accordion rows, projection retry, code and metadata are NSH elements. GPUI only hosts Surface and exported action/key context. | Scoped static inspection; main compilation; three-desktop manual checks |
| Safe controller | Public actions, events and methods remain compatible. Existing IO-free controller emits at most the original single-expanded-file lazy request; original application route/epoch/request/snapshot Apply/Defer/Drop fences are untouched. No UI file reads, parsing or new service route. | Public API inventory; state-transition diff; caller inspection |
| Prepared content | Keep `prepare_projection`, `layout_projection_rows`, reconciliation, exact-current-file, pairing, navigation, span and label helpers as production pure functions in `render.rs`. Preserve every PreparedRow variant, exact span text, syntax weight/italic/color, monofont, old/new numbers, +/- markers, equal side-by-side halves and missing-newline rows. | Helper diff; native text API inspection; main regressions and manual comparison |
| Virtualization | Native NSH `virtual_list` with 24-point rows and matching estimate. Selected hunk navigation reaches its row using native `ElementRef.scroll_to`; no old GPUI list painting. | Pinned NSH 0.4 source inspection; manual large projection navigation |
| Snapshot identity | Delayed actions retain displayed presentation/row identity, generation and file identity. Recycled rows never capture an index that is later applied to the newest snapshot. A replaced snapshot invalidates old requests even with the same generation; file must still belong. | Static command-path audit; main stale/recycle regression |
| Invalidation | Shared presentation version changes with actual row/projection/layout/snapshot changes; watch snapshots reuse shared rows. Hunk navigation, focus and refresh progress do not deep-copy or scan all rows. | State/snapshot audit; main large projection regression |
| Failure and refresh | First load, background refresh, explicit retry, projection pending/error/retry and failed refresh retaining usable snapshot stay truthful. Retry checks the exact expanded file and current projection error; refresh disables duplicate retry as before. | Controller comparison; manual pending/error/retry matrix |
| Keyboard/focus/lifetime | Public Focusable forwards the stable host focus to native controls. Close/previous/next/layout host actions and native keyboard remain safe; Tab/Shift-Tab and focus indication work. Unmount/recovery never owns or cancels background services. | Static focus/action audit; main keyboard and three-desktop manual checks |
| Environment/recovery | Theme/i18n tokens preserved; native failure recovery is independently hosted and reload only recreates presentation, never replays business actions. | Static recovery audit; main fault injection/manual checks |

## Pinned API evidence

Registry source `nsh-core-0.4.0/src/element/virtual_list.rs` exposes `virtual_list(count, Fn(Position))`, reactive count and keys, `ListLayout::estimated_item_size(Points)`, and recycled positions. `element_ref.rs` exposes `ElementRef::focus` and `scroll_to(Point<Points>, animated)`. `builder.rs` exposes nested Text children for inline spans; `style.rs` exposes font family, size, weight and italic. No dependency change is needed.

## Verification ownership

This leaf may run scoped rustfmt and read-only static checks, not compile or execute tests. Risk-based regressions for main: exact mixed Unicode/whitespace/span reconstruction; unequal addition/deletion pairing; missing newline; stale same-generation snapshot and recycled-row commands; single expanded file; pending/error/retry; retained snapshot refresh failure; hunk navigation in both layouts; native focus and exported actions; recovery without service replay. If Rust development regressions are added by integration, retain evidence and remove test-only code before final merge per repository rules.

## Delivery status

Leaf implementation complete; ready for main integration and non-implementer review, not production acceptance. Acceptance plan was recorded before code.

### Implemented

- `nsh_view.rs` owns the NSH header, all controls, empty/loading/refresh failures, every PreparedRow rendering branch, native virtual list, exact inline code spans, focus forwarding and independent recovery Surface. The GPUI container has no product painting, only sizing, Surface embedding and existing exported action/key context.
- `mod.rs` retains the five action types, constants, DiffLayout, three event structures/EventEmitter implementations, PreparedRow/PreparedLine structures and stable public Focusable. It adds private presentation ownership and shared invalidation/scroll state.
- `state.rs` retains public methods and original service/event decisions. Row rebuild now publishes `Arc<Vec<PreparedRow>>`; row/presentation version invalidates on snapshot/projection/layout rebuild, including replacement snapshots with unchanged generation. Navigation changes only scroll revision/target. Row callbacks retain displayed file identity and version, never a newest-list index. File membership and exact expanded-file retry are rechecked by the original controller plus presentation command gate.
- `render.rs` retains all original production pure preparation and label helpers with unchanged bodies; obsolete GPUI painting and private click adapters with no external callers were removed. No application controller or caller changes.
- List rows and estimate are 24 points. Native scrolling uses measured viewport and nearest-visible-row arithmetic: `top = row × 24`; above viewport targets `top`; below viewport targets `top + 24 − height`; otherwise keeps offset, clamped to zero. Unmeasured height defers until layout/watch-scroll reports bounds. Row keys are `(row_revision, ordinal)` within a presentation; their binding only subscribes to row revision. Recycled content rebuild subscribes to Position plus row/theme/language version, not all watch publications.
- Stable host Focusable forwards explicit focus into the NSH Surface and its native root; opening without explicit focus does not focus revision zero. Native Escape/[/] coexist with preserved host Close/Previous/Next/Layout actions. Layout and every action remain accessible through native controls. Recovery only recreates presentation and invalidates its surface identity; it neither owns nor cancels services nor replays business commands.

### Static evidence

Run against the scoped final Rust files using rustfmt `1.9.0-stable (88d9e12ae1 2026-08-18)`:

- `rustfmt --check --edition 2024 --config skip_children=true` on the four owned Rust files: exit 0.
- `git diff --check -- crates/vega_ui/src/diff_view docs/907/nsh-diff-view.md`: exit 0.
- Read-only Python inventory against baseline: all 12 public controller method signatures and public language helper match; actions/constants/layout/event structures/EventEmitter/Focusable match; preparation helper bodies match after removing prohibited doc comments and adding the module import.
- Translation inventory: all 45 referenced keys exist in both English and Chinese locale tables.
- Scoped prohibited-pattern inventory: no GPUI uniform list/old painting helpers, file IO, unwrap/expect, comments, try_send, or Rust test scaffolding.
- Byte comparison: `crates/vega/src/diff_controller.rs` and `crates/vega/src/window/diff.rs` unchanged from baseline. Removed private painting/click helpers have no remaining external callers.

No builds, Clippy, tests, CI, application execution, package/install, screenshots or three-desktop interaction checks ran in this leaf. No Rust development regressions were added because execution is expressly reserved for main; the risk-based regression matrix above remains unverified and must not be reported as passed. Rust formatting establishes syntax formatting only, not type checking or native behavior. Main must compile the integrated source and verify native list/recycling/selection geometry, focus/key dispatch, accessibility, narrow-window wrapping, theme/i18n changes and recovery on macOS/Windows/Linux. No unsupported essential API blocker was identified in the pinned source; runtime behavior remains pending. No dependency/version changes, Git/index mutations, subagents or ImagePreview writes were made.
