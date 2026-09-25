# Issue #148 — Long conversation scrolling

Source: https://github.com/puzige/vega/issues/148

Status: frozen for implementation, 2026-09-25.

Baseline: `origin/master` at `684ce441`.

## Decision

Keep the existing variable-height GPUI `list`. It already virtualizes one
`StreamEntry` per list item, pages older history, follows the tail, and
remeasures rows after content changes. This issue validates those paths and
fills the missing observability, stable identity, anchor, and target-location
gaps; it does not replace the list with a different virtual-list component.

The 10k E2E currently proves data and follow/prepend behavior, but its
`frozen_rematerializations == 0` assertion measures Markdown block caching. It
does not measure list-row callbacks or GPUI element/layout counts. Benchmark
reports must name the counter actually measured and must not label callback
counts as mounted or painted rows.

## Contract

- A fixed-window mixed-entry fixture covers 25, 100, 1,000, and 10,000
  entries. The production list callback count is measured separately from
  fixture construction and from Markdown materialization. Once the fixture is
  taller than the viewport, callback work stays bounded by the visible range
  plus configured pixel overdraw; it does not grow with total entry count.
- Performance output reports fixture size/hash, window and viewport dimensions,
  build profile, callback count, frame-build and row-callback p50/p99, and
  process RSS. Fixture creation time and retained conversation data are
  reported separately. No cross-machine frame-rate threshold is inferred from
  timing samples. Existing measurements from the deferred S8-T44 spike are not
  a comparable baseline for this variable-height implementation.
- Samples retained by production counters are bounded. The counter measures
  render callbacks only; it does not claim GPUI layout or paint work unless
  those are measured independently.
- Every navigable entry has a stable route-scoped identity. Hydration keeps
  durable sequence/message identity instead of replacing it with the current
  array index. Live-to-history reconciliation and prepending older pages do not
  change the identity of an entry.
- Viewport preservation uses the stable identity of the top visible entry plus
  its pixel offset. Prepending history, changing an entry's height, and
  remeasuring after the conversation column width changes preserve that same
  visible anchor when it still exists. Tail following and the existing Resume
  tail behavior remain unchanged.
- A message-ID location request either scrolls to the matching loaded entry or
  loads a history page containing that durable message before scrolling. A
  target that does not belong to the thread returns an explicit not-found
  result; it must not silently scroll to an index or the top of the list.
  Superseded requests are fenced when the active thread changes.
- Switching between cached conversation routes preserves each route's
  in-memory anchor. A process restart continues to open the newest history page
  under the current startup policy; locating an older durable message after a
  restart uses the same message-ID request path rather than persisting transient
  list indexes.
- Copy, selection, focused controls, nested tool/thinking scrolling, and
  disclosure state keep their existing behavior when rows leave and re-enter
  the virtualized viewport.

## Acceptance matrix

| ID | Setup/action | Expected evidence |
|---|---|---|
| V1 | Render the same deterministic mixed fixture at 25/100/1,000/10,000 entries in one fixed viewport | Exact entry/content totals; callback count stays viewport-bounded for the three long fixtures and is not reported as mounted rows |
| V2 | Run the real-window probe before and after implementation | Reproducible JSON includes fixture hash, viewport, callback/frame distributions, RSS, and separate fixture-build timing; no unsupported universal FPS claim |
| V3 | Prepend older pages while detached from tail | The same stable entry remains at the same pixel offset; page sequence and list count remain correct |
| V4 | Expand/collapse a tool or thinking row, stream content, and resize the conversation column | The same stable anchor remains visible; changed row geometry is remeasured, including previously measured offscreen rows |
| V5 | Request a loaded and an unloaded durable message ID | Loaded target is revealed; unloaded target loads the containing page and is revealed; unknown/foreign target returns not-found; thread changes fence stale results |
| V6 | Switch away from and back to a cached thread; scroll a row out and back into the viewport | Per-thread anchor, copy text, selection/focus, nested scroll state, and disclosure state remain coherent |
| V7 | Open a thread after app restart | Startup keeps the existing newest-page policy; an older target remains locatable by message ID |

Automated GPUI tests cover V1, V3–V7 and observable geometry. The real-window
probe covers V2 and reports measurements without a timing pass/fail threshold.
Native acceptance after merge checks 10k continuous scrolling, dynamic content,
selection/copy, nested scrolling, resize, and route switching on the user's Mac.

## Baseline gaps

- The current E2E has 10,000 mixed entries but does not count list callbacks.
- The current bench uses one fixed 10,000-entry fixture; its frame and row
  samples are unbounded vectors, and its output has no RSS or callback count.
- Hydration discards some durable `seq` and message IDs when constructing
  `StreamEntry`; `ListState` navigation is index-based.
- `ListState::remeasure()` is available for width-driven height changes, but
  the stream currently invalidates changed rows individually and does not
  remeasure the full list when the conversation column width changes.
- Existing prepend anchoring is index plus in-item offset. It protects the
  current page boundary but cannot identify the same semantic entry after an
  arbitrary mutation.

## Plan and ownership

The main agent owns this contract, review, PR, and integration. A dedicated
implementation agent owns code and focused GPUI/store regression tests. Start
with a baseline probe; add failing deterministic row-count, identity,
unloaded-target, and anchor tests; then implement only through the existing
history projection and list APIs. Run the exact package-level nextest filters
for changed behavior, formatting, and diff checks. Cloud PR checks gate the
squash merge. Do not run the local workspace suite.

## Non-scope

Do not build the #72 navigation UI, change message rendering, cap persisted
history, or claim that UI virtualization bounds the memory held by retained
message data. Do not rewrite the native list or introduce a timing threshold
without measurements from a fixed device and protocol.
