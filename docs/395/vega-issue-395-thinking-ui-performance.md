# Long thinking UI responsiveness

## Source and scope

Issue #395 records the 2026-10-15 user request to fix slow animations and scrolling both with thinking collapsed and after expanding long thinking, and to audit virtual scrolling. Observed in installed Vega 0.1.122. A short sample found GPUI/Taffy layout activity on the main thread; it does not establish the complete runtime bottleneck. Static review finds unconditional full-source copies/Markdown synchronization on thinking deltas and full-body construction inside a height-capped disclosure.

The user-authorized [2026-10-16 sliding-window follow-up](vega-issue-395-thinking-sliding-window.md) supersedes prefix retention with bounded latest-content retention. Its eviction, omission indication, source-position and retained-copy rules take precedence over the corresponding older assumptions below; this document remains the responsiveness and viewport-rendering baseline.

This task owns UI processing, notification and disclosure virtualization. Runtime/provider limits and diagnostic changes belong to #393 and are not changed here. No new dependencies, schema changes, runtime configuration changes, user-content rewriting, or unsolicited UI copy.

## Behavior contract

1. Collapsed or ancestor-hidden thinking retains the existing bounded source and incremental preview/title/state. It must not parse or materialize a Markdown body, copy the cumulative full source on each delta, or invalidate outer list measurements when no visible geometry/state changes. Necessary title, truncation, completion and failure transitions remain visible. Reopening displays current retained content, including deltas received while collapsed.
2. Expanded thinking must render a bounded viewport plus small overscan rather than one layout tree containing all retained text. A single very long paragraph or code block must not defeat the bound. Use existing Markdown, typography, wrapping and copy/selection semantics; do not solve performance by silently dropping content or rendering all content as plain text.
3. Parsing/preparation for opening large source must not perform unbounded synchronous work in the UI event handler. Use cached/incremental work, background preparation or bounded scheduled work with stale-result protection. Closing, newer source, task switch or drop cannot apply an obsolete prepared body. Preserve memory bounds.
4. Preserve short-content natural height and existing maximum disclosure height; scrolling remains local while consumable and hands off to the conversation at the boundary. Preserve user reading position, follow tail only if already at the bottom, and keep collapse/reopen behavior consistent. Resize/wrapping, UTF-8, Markdown links and code, selected-text copy and complete-source copy remain correct.
5. Audit the outer conversation list and notification chain. Prevent thinking-only deltas with unchanged visible state from rebuilding/re-measuring unrelated messages. Preserve final presentation, hydration, message anchors, tool ordering, status and animation lifecycle. Optimize only evidenced paths relevant to this bug.

## Acceptance matrix (before implementation)

| Case | Operation and observable outcome | Evidence |
| --- | --- | --- |
| Collapsed stream | Append many small deltas to long thinking; source remains exact within existing budget, body parse/materialization stays idle; unchanged title/state causes no redundant notification/outer measurement | Production-path GPUI/model regression, cloud |
| Parent hidden | Fold stage/run while thinking is marked open; further deltas do not prepare hidden body; reopen gets newest content | Production-path regression, cloud |
| Open long content | Open mixed paragraphs/list/code at display budget; only visible rows plus overscan are materialized, even with one long paragraph/code block; opening leaves event loop available | Production-path bounded-work regression, cloud |
| Streaming and stale work | Append while open, close before preparation completes, then reopen/change source; only newest applicable body is published | State-transition regression, cloud |
| Scroll and resize | Scroll middle and tail, stream more, resize narrow/wide, collapse/reopen; reading anchor and boundary propagation hold | GPUI regression, cloud; user manual check |
| Copy and empty/error | UTF-8 source, selected substring, source copy, empty body, truncation and terminal failure preserve content/state semantics | Existing and focused regressions, cloud |
| Outer virtualization | Long mixed conversation with hidden and visible thinking; stable unrelated entries are not invalidated by invisible deltas | Focused integration regression, cloud |

Avoid elapsed-time thresholds and combinatorial tests. Prefer deterministic production-path counters, bounded rendered nodes and event-loop/state invariants. Existing assertions remain unless this contract explicitly changes their semantics; document necessary test expectation adjustments.

## Implementation and ownership

The dedicated implementation agent owns `crates/vega_ui/src/conversation_stream/` and directly necessary UI helper/test modules, plus the delivery record. The main agent owns this spec, coordination, review, integration and GitHub delivery. The executor first audits actual latest-base code, writes focused regression cases, then implements deferred source processing, scoped notifications and bounded rendering. Report any incompatible selection/virtualization API before substituting a degraded behavior. Repository AGENTS/exec-guide/design guidelines apply; no new code comments.

Main checkout could not fast-forward because an unrelated untracked document would be overwritten. It is left intact. The task worktree was created from freshly fetched `origin/master`, without carrying any unrelated local files.

## Validation and delivery

Local validation is limited to formatting and static/diff checks. Do not run local Rust compilation, Clippy, tests or packaging. Cloud `pr-check` runs `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo nextest run --workspace`, including new regressions. Record exact results and distinguish not-run checks from success.

After green required checks, squash merge via PR, set Issue to In review, and wait for user manual validation. Keep Issue/worktree open until that result. No app restart or installation is part of this task. Rollback is a revert of the scoped PR; no data migration is involved.

## Audited design refinement

The native outer List already virtualizes entries. Its boundary does not bound the work inside an expanded thinking entry. Replacing that List is unnecessary. The audit additionally found repeated `set_thinking` notifications, per-second run-clock notifications sharing the geometry-invalidation path, and an unconditional active-assistant remeasurement on conversation renders even when the Markdown model does not change. Animated stage titles request frames in their current view, which can be the conversation when rendered through the static stage renderer. Retain existing Reduce Motion/window activity/visibility guards and isolate decorative work where needed.

The implementation may prepare the thinking body on a background worker using the pinned text-system APIs, then project only visible visual fragments through a custom element. Retain the existing ScrollHandle and geometry tokens. Long paragraphs require visual-line bounds; long unwrapped code requires bounded horizontal fragments without inserting semantic newlines. Full prepared selection text and global byte offsets must preserve copy/selection while offscreen fragments are absent. One worker per card at most, coalesced source changes, and source/visibility/width generations fence publication. The UI thread must not create one element per retained line or copy the full prepared document every frame. General answer/tool Markdown remains outside this renderer change.
