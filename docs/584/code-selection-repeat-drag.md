# Code selection autoscroll under repeated held input

Issue [#584](https://github.com/puzige/vega/issues/584), user acceptance follow-up to [PR #585](https://github.com/puzige/vega/pull/585), integrating the existing [#485 / PR #511](https://github.com/puzige/vega/pull/511) code-selection implementation. Contract v1, 2026-10-25. Baseline: `0fe30576d5884df6f7c2e5f75a0bb580eb883b9f`.

## Observed failure and source diagnosis

The user reports that the scrollbar displays correctly but selecting ordinary reply code and dragging to the right edge does not automatically reveal more text. The user confirms ordinary replies, not expanded Thinking. The installed bundle reports v0.1.226; that release includes PR #511. This is not a successful acceptance, and the installed bundle alone is not proof of the exact loaded runtime revision. No application was launched, automated, restarted or modified for diagnosis.

Pinned `gpui-pre-macos 0.3.4` `window.rs::handle_view_event` starts `synthetic_drag` before dispatching the real held MouseMove. `synthetic_drag` repeats the same held MouseMove every 16ms until the native drag identity changes. Vega `code_drag.rs::CodeDragElement::paint` unconditionally calls `CodeDragController::pause` on every held MouseMove; `pause` drops the timer and invalidates its epoch, then `schedule` starts a new 16ms timer. This is a debounce: repeated native events, or real pointer moves faster than the interval, can continually cancel the scheduled advance. The earlier 21-test development result used isolated moves followed by clock advancement; it did not establish progress while repeated held events continue. This source-supported diagnosis must be checked by the new cloud regressions, not described as a local runtime reproduction.

## Acceptance

| ID | Input | Observable result |
| --- | --- | --- |
| R1 | Single left-button drag from ordinary code text into the visible right edge, followed by same-position held moves at the native 16ms cadence | Over multiple intervals, code X advances and the visible logical selection expands while the input stream continues. Outer Y stays fixed; the original character anchor stays fixed. |
| R2 | Held moves faster than 16ms, including changed positions inside the edge region | Progress is not postponed until the moves stop. The tick uses the latest pointer and direction; leftward reversal and scroll bounds remain correct. |
| R3 | During continuous input, move back to the center or outside the code viewport vertically, or release the button before the next advance | The scheduled work stops, pending refresh becomes invalid, subsequent intervals do not advance or extend the selection. A later valid drag can start normally. |
| R4 | Actual selected UTF-8 code copied through the normal copy action; sibling code blocks; shared Thinking viewport | Copies the expected source substring, affects only the originating block, and the shared handler still advances under repeated held input in Thinking. Do not claim native UI acceptance from synthetic GPUI events. |

Preserve [S1–S7](code-horizontal-scroll.md#acceptance) and the existing [D1–D9](../485/vega-issue-485-code-drag-autoscroll.md): no ordinary-Y-to-X mapping, no horizontal wheel leakage, draggable scrollbar, stable block/window ownership, fixed header, exact selection/copy and all focus/content/mount/drag cancellation checks. No changes to parsing, source text, styles, dependencies, framework or global scrolling.

## Minimal implementation and ownership

One dedicated implementation agent owns `code_drag.rs` and necessary new development tests/support; the root owns this contract, Git and delivery evidence. A different agent reviews the stopped exact version. No code changes in other worktrees.

In the held-move capture handler, update pointer positions without unconditionally pausing the existing timer or pending refresh. Once an origin exists, pause immediately when the pointer no longer requests horizontal edge movement; otherwise defer the existing single-instance `schedule`. The scheduled tick reads the latest pointer and validates the same ownership/selection/epoch. Preserve all existing cancel paths. Do not solve starvation by shortening the interval, increasing speed, removing guards, ignoring moves or only deduplicating identical coordinates: changed-position moves can also be faster than 16ms.

Write only the focused new event regressions needed for R1–R4, through production renderers, real GPUI mouse input, real ScrollHandles, controlled clock steps and the normal copy action. Recreate the native repeated-input sequence at the event boundary; do not launch the application or native platform window. Do not restore the old test collection or decide expected selection from actual selected output. Existing GPUI test-support may be enabled for development only. After actual cloud success, fix recoverable source/hash/full SHA/run evidence before removing tests and exclusive support. Final exact head needs independent review and full required CI reporting `no Rust tests` (0 discovered / 0 executed).

Local work is limited to fast static checks, rustfmt, diff and offline metadata. No local Vega execution, compilation, builds, Clippy or tests. All automated behavioral validation uses existing Actions; poll no faster than five minutes unless notified or asked. No CI changes. Do not install, restart or force-quit the running Vega, mutate user data/credentials, or change #548/#580/#485 branch products.

## Status

Contract prepared from source inspection; implementation and cloud behavior validation pending. Issue remains open and In progress. Successful CI and merge still require a new user manual acceptance.
