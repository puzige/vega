# Issue #148 — V1–V4 delivery report

Implementation batch: bounded render probes, stable entry identity, and scroll
anchor preservation. The branch was rebased onto `dc11ad057580e09f62eacb262f5b2dc13c1ec912`.

## Changes

- Production list render callbacks and frame-build samples use bounded rings of
  2,048 samples. The callback counter counts `render_item` invocations; it does
  not count mounted rows, layout passes, or painted rows.
- The real-window probe accepts the fixed fixture sizes 25, 100, 1,000, and
  10,000. Its JSON records an FNV-1a 64-bit fixture fingerprint, fixture
  creation time and source text bytes, Markdown materializations, requested and
  actual viewport dimensions, process RSS, total list callbacks, and separate
  frame-build and row-callback distributions for scroll and stream phases.
- Hydrated user entries now retain their durable message IDs. Each stream entry
  has a route-scoped semantic identity; hydrated and live assistant segments,
  tools, plans, summaries, and skill activations use durable identifiers where
  available. Local-only entries receive per-stream identities.
- Prepending history, invalidating a changed row, and remeasuring after a
  conversation-width change restore the same top entry and in-item pixel offset
  when that entry still exists. Tail-follow mode remains active when the user
  was following the tail.

## Real-window probe

All four probes used the release binary with the same requested 1,200 × 800
window and `ListState` overdraw of 600 px, matching the production list. All
four probe sizes use the same deterministic real-window fixture builder. The
single-draw GPUI callback test uses its own mixed-entry helper, so its fixture
content and hash are not directly comparable to the probe fixtures. Each probe
recorded an actual 1,200 × 800 viewport. Callback totals below cover the full
8-second scroll and 12-second stream phases combined; they are not per-frame
counts. `fixture_source_text_bytes` measures source-text payload, not complete
retained heap memory. RSS is the separate process sample taken when the report
was written.

| Fixture entries | Fixture hash | Build ms | Source text bytes | Markdown materializations | Rendered list entries | Callbacks (20 s) | RSS KiB |
|---:|---|---:|---:|---:|---:|---:|---:|
| 25 | `2f8701a150af4022` | 11.262 | 4,709 | 38 | 26 | 62,396 | 85,696 |
| 100 | `267dae3f7e62936e` | 9.491 | 18,413 | 152 | 101 | 51,920 | 86,608 |
| 1,000 | `c3a7c78de4e4da2c` | 14.603 | 186,461 | 1,520 | 1,001 | 56,037 | 95,216 |
| 10,000 | `74ec546366b48171` | 67.447 | 1,887,551 | 15,200 | 10,001 | 57,581 | 178,656 |

The per-phase callback count and timing samples are:

| Entries | Scroll callbacks (8 s) | Frame p50/p99 µs | Row callback p50/p99 µs | Stream callbacks (12 s) | Frame p50/p99 µs | Row callback p50/p99 µs |
|---:|---:|---:|---:|---:|---:|---:|
| 25 | 30,140 | 4.667 / 10.167 | 2.583 / 29.000 | 32,256 | 3.875 / 10.250 | 11.708 / 25.666 |
| 100 | 24,249 | 5.125 / 11.500 | 14.083 / 31.708 | 27,671 | 4.750 / 13.250 | 14.667 / 38.375 |
| 1,000 | 23,963 | 4.667 / 12.625 | 13.334 / 31.166 | 32,074 | 4.125 / 9.917 | 12.250 / 28.542 |
| 10,000 | 25,143 | 3.625 / 9.833 | 11.250 / 27.125 | 32,438 | 4.083 / 10.167 | 12.625 / 29.583 |

Totals count all callbacks in a phase. Percentiles use that phase's bounded
most-recent sample ring (up to 2,048 samples). The deterministic one-draw GPUI
test separately checks that fixtures with 100 or more entries invoke at most
128 callbacks in the fixed viewport, and that the 10,000-entry count is within
16 callbacks of the 1,000-entry count.

The supplied pre-change file `/tmp/vega-148-baseline-e48e150.json` reports
10,001 rows/items and frame-build plus `item_build_*` samples. It has no fixture
fingerprint, window/viewport dimensions, list callback count, RSS, or fixture
build time. Its `item_build_*` metric is not the new production list-callback
measurement, so these runs do not support a like-for-like before/after timing
or callback comparison. No timing threshold or cross-machine performance claim
is made.

## Focused verification

All commands below were run after the rebase:

- `cargo nextest run -p vega_ui issue148_` — 4 passed, 467 skipped.
- `cargo nextest run -p vega_ui ten_k_mixed_items_trunk_e2e` — 1 passed,
  470 skipped; covers detached streaming, prepend, and tail resume.
- `cargo nextest run -p vega_conversation --test pagination_hydration_e2e` —
  13 passed.
- `cargo nextest run -p vega r49_utility_bar_mounts_above_the_card_only_on_the_new_task_page` —
  1 passed, 199 skipped.
- `cargo fmt --all -- --check` — passed.
- `git diff --check` — passed.

The local workspace-wide suite was not run. This batch does not implement the
message-ID page lookup, stale-thread request fence, or 64-thread route-anchor
LRU from V5–V7. Native acceptance should still exercise 10k scrolling, detached
prepend, dynamic row content, and conversation-column resizing. The callback
instrumentation does not bound retained conversation data or measure GPUI
layout/paint work.
