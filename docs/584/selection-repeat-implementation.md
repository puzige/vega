# Repeated held-input selection: implementation handoff

Contract: [v1](code-selection-repeat-drag.md). Baseline `0fe30576d5884df6f7c2e5f75a0bb580eb883b9f`; branch `feat/584-code-selection-autoscroll`. Implementation handed to the root as an uncommitted, stopped snapshot. No Git/index/commit/push operations were performed by the implementation agent.

## Production change

`code_drag.rs::CodeDragElement::paint` no longer calls `pause` for every held move. It updates the latest pointer, preserves the existing single timer/pending refresh at a horizontal edge, and pauses immediately in the center or outside the viewport's vertical extent. The unchanged timer reads the latest pointer and direction. Existing ownership, focus, content identity, selection, mount, epoch, cancellation and bounds checks remain intact. No changes to interval, speed, wheel routing, scrollbar, renderer behavior or dependencies used by production.

The source diagnosis is the interaction between this unconditional debounce and pinned macOS `handle_view_event` / `synthetic_drag`, which repeats held moves every 16ms. A real pointer delivering faster changed positions has the same starvation risk. This was diagnosed from source; no native application reproduction was performed.

## New development regression scope

Four new GPUI event tests use the production ordinary Markdown renderer and actual Thinking component, real scroll/selection handles and normal `CopySelectedText` action. The historical 19/21-test collection was not restored.

| Test | Contract coverage |
| --- | --- |
| `native_cadence_held_moves_keep_scrolling_and_copy_exact_code` | R1/R4: 16ms same-position source, two progress checkpoints during the stream, stable nonzero anchor, right/left bounds, exact UTF-8 suffix/prefix copy, sibling X and outer Y isolation. |
| `faster_changed_position_moves_progress_then_pause_and_resume` | R2/R3: 8ms alternating positions, right progress and left reversal during the stream, center pause with repeated stationary input, same-gesture edge reentry. |
| `leaving_code_y_or_releasing_stops_repeated_drag_work` | R3: vertical exit or button release with remaining X range, later intervals leave offset/selection unchanged, subsequent valid gesture progresses. |
| `shared_thinking_viewport_progresses_during_repeated_held_moves` | R4: production Thinking viewport progresses under the 16ms source with sibling/outer isolation. |

The repeating source is spawned before the initial edge move. `run_until_parked` polls its timer first, with explicit assertions that registration occurred, executor time is unchanged and no repeated event has yet fired. Only then is the initial held move dispatched. Both progress checkpoints assert increasing source-event counts while its task is retained, actual X movement, logical cursor movement and a stable anchor. The 8ms source alternates coordinates; this cannot pass by deduplicating identical events. The source remains active until the test explicitly stops it.

Pinned scheduler implementation was checked rather than relying on a stale executor comment: `gpui-pre-scheduler 0.3.4::TestScheduler::run` only drains runnable work; `run_with_clock_advancement` is separate and unused here. `advance_clock` visits deadlines within the requested interval and runs ready work. These are synthetic event-boundary regressions, not execution of native AppKit input or user acceptance.

## Development-only support and cleanup boundary

- `code_drag_repeat_tests.rs`: four tests, fixture and input source.
- `code_drag.rs`: test module plus read-only weak controller observation after production prepaint.
- `selection.rs`: read-only text layout observation after production prepaint.
- `thinking.rs`: private `cfg(test)` construction helper; no production public API.
- UI Cargo manifest: existing GPUI development `test-support` feature only.
- Cargo lock: ten test-support package identities added; all pre-existing name/version/source/checksum identities retained. No production dependency upgrade.

After actual cloud success, preserve the exact source SHA/hash/results and then remove this complete support set. The production move-handler change remains. The final head still requires independent review, static absence checks and required CI with zero Rust tests.

## Executed local checks

All checks were static; no Vega execution, compilation, build, Clippy or tests occurred.

- `cargo fmt --all`, then `cargo fmt --all -- --check`: exit 0.
- `git diff --check`: exit 0.
- `cargo metadata --offline --format-version 1`: exit 0; 924 resolved packages. The existing 914 production packages remain; ten support package identities were added.
- `cargo metadata --no-deps --offline --locked --format-version 1`: exit 0 after the lock update.
- Python static inspection of lock identities and metadata: exit 0; GPUI test-support is a UI development dependency and no existing package identity changed.

Cloud behavior and required CI results are pending the root's PR dispatch. The running Vega, user data/credentials and other task worktrees were not touched. No specification deviation.

## Independent-review fixture correction

Review of development head `86e766f621af4ecce5f5836d6731f0346e4ec007` identified an input-geometry defect in the exact-copy fixture. At maximum X, the initial `visible.right() - 4px` pointer can still lie inside the last glyph, so the production floor-based resolver correctly stops before that glyph. Reaching the scroll boundary alone does not mean the pointer selected the complete line.

The correction preserves the initial visible-edge progress sequence and all original exact-copy expectations. After reaching maximum X, the still-active source is moved through a real held event to `visible.right() + 4px`, with preconditions proving that the point lies beyond the laid-out line end and remains inside the code's Y extent. A real snapshot must then report `CODE.len()` before the unchanged UTF-8 suffix comparison. The left edge additionally requires snapshot cursor key zero before the unchanged prefix comparison. No production/resolver behavior changed, and no failed assertion was weakened. This source-supported fixture correction preceded receipt of that head's cloud behavior result; it does not claim a cloud failure or pass.

The first formatting check after this correction reported only a multiline formatting difference. Scoped rustfmt corrected it; the final formatting and diff checks below pass. No local execution, compilation or tests occurred.

## Cloud fixture compilation correction

The root retrieved the original [run 40114672905](https://github.com/puzige/vega/actions/runs/40114672905) Nextest log. Although the overall run was cancelled by a later push, Nextest job `120933385507` failed at 2026-10-24 17:10:54 UTC. Compilation reported E0689 at the fixture's `count.get().is_multiple_of(2)`: the captured `Cell::new(0)` counter had no concrete numeric type at that method call. Python groups 5 / 15 / 40 passed; Rust discovery did not finish and no behavioral execution result was obtained. The overall cancellation does not erase that failure.

The fixture counter is now initialized with `0usize`. This fixes the test-support type inference at its source without changing input timing, observations, behavioral assertions or production code. The second run had the same source-level ambiguity, so this correction is based on the first compiler evidence, not a blind retry. Final formatting and diff checks pass; cloud validation of the corrected source remains pending. No local compilation or tests were performed.

## Snapshot hashes

| File | SHA-256 |
| --- | --- |
| `code_drag.rs` | `c08d9aa458c7dc3a8c9fea1ae59403513a00a84e67de3d7f3638907fc78b2c8c` |
| `code_drag_repeat_tests.rs` | `14c04f14ade8dc26e6d41a123bb7d9d0538eaec2e7cb2bb6a078ddc57360b0dc` |
| `selection.rs` | `acc225ee14d51fe253e64a5fd86ab21ece20a87f528c4f73cbfd68e6598ab303` |
| `thinking.rs` | `a25f95171bc2f4afb222bee01407383bdc777fa7b894ad6e35583ccd09343cbf` |
| UI Cargo manifest | `3f27cebbfc927e48576b21a78e2684c05ff4da14c2a70cb56e7851cd175edde5` |
| Cargo lock | `31710312a40ca25ef52a99c97d10dea6a5f5c1078518c59c9acd70177ae006f8` |
