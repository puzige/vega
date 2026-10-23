# Code block horizontal scrolling

Issue [#584](https://github.com/puzige/vega/issues/584). Contract v1.2, 2026-10-24. Clarifies existing selection clearing on scrollbar mouse-down; scrolling acceptance is unchanged.
Baseline: `73977c89eba718da4b2573227c44beb8bcff6911`.

## Problem and scope

`render_rows::markdown_blocks` renders fenced code with native `overflow_x_scroll` and edge fades but no draggable scrollbar. Fixed GPUI 0.3.4 remaps vertical deltas onto a single horizontal overflow area by default and leaves the event propagating; the outer List then consumes the original Y delta. The `thinking_body::BodyElement` horizontal code path uses the same native default. This is code evidence consistent with the user's screenshots, not a locally executed reproduction.

Fix code block scrolling in ordinary Markdown (streaming/history/selectable rendering) and expanded Thinking. Preserve exact text, selection/copy, stable per-block state, header position, theme and virtualized layout. Tables may share the existing Thinking horizontal surface; do not independently redesign table rendering. No dependency upgrades, global scrolling changes, application launch/restart/install, database edits, or unrelated branch modifications. PR #511's text-selection edge autoscroll is a separate task and is not claimed here. #548 and #580 remain untouched.

## Acceptance

| ID | Input / precondition | Observable result |
| --- | --- | --- |
| S1 | Long code under a vertical conversation List; ordinary Y-only line or pixel wheel | Code X stays unchanged; outer Y changes normally. |
| S2 | X-only or horizontal-dominant diagonal gesture over code | Only code X moves; outer list/Thinking Y stays unchanged. Gesture axis remains stable through precise-input wobble/inertia. |
| S3 | Horizontal input at either code edge, including no X overflow | No outer Y movement or axis remapping. Offset clamps to valid bounds. Ordinary Y still passes. |
| S4 | After substantial X movement, point at the unchanged viewport edge and scroll | Event capture still covers the viewport, not the shifted content. |
| S5 | Hover/always-visible horizontal scrollbar; mouse down/move/up on thumb | Real code ScrollHandle moves horizontally; outer Y does not change, and dragging does not start or extend text selection. Track clicks work; no invalid thumb when content fits. Respect system auto-hide preference. |
| S6 | Two code blocks and rerender/incremental append | Stable independent offsets; no reused handle across block/window identities. Header/copy stays outside scrolled body. |
| S7 | Actual Markdown code and Thinking code rendering | Same wheel isolation and drag behavior; text is not wrapped or rewritten. Existing selection and copy remain usable outside scrollbar. |

Shift+wheel uses the horizontal delta supplied by the platform. Do not claim physical-device Shift behavior from synthetic input. Test horizontal events with modifiers as well as unmodified X input; native device acceptance remains user manual testing. Horizontal gestures intentionally remain consumed at boundaries so their Y residue cannot move ancestors. Clicking scrollbar chrome may clear a pre-existing text selection under the existing global mouse-down rule; this fix must not start or extend a selection by dragging the thumb. It does not introduce a new selection-retention contract.

## Minimal implementation plan and ownership

The root owns this spec, Git/index, PR and evidence. One implementation subagent owns production changes and scoped development tests; a different agent reviews the stopped version.

1. Reuse pinned `gpui_kit::base::ScrollableMask` and `Scrollbar::horizontal` with one stable `ScrollHandle`. Native content must suppress Y-to-X remapping (`lock_scroll_axis` / `restrict_scroll_to_axis` or equivalent verified pinned API). Mask and scrollbar are siblings of the scrolled content inside a stable viewport, not children translated by its offset. Avoid replacing selection handlers.
2. Share the smallest code viewport composition between `conversation_stream/render_rows.rs` and `conversation_stream/thinking_body.rs`, with private module wiring if needed. Preserve the existing horizontal fades when the viewport deliberately uses Hidden overflow for input ownership: allow one explicit horizontal viewport flag in `scroll_edge_fade.rs`, default off for all other callers. Ordinary blocks need keyed persistent handle state; Thinking already stores per-block handles. Preserve native sizing and use existing theme tokens; no extra explanatory product text. Keep scrollbar unobscured by fades or selection surfaces and preserve header outside scroll bounds.
3. Add focused development GPUI event regressions through the production viewport/renderer and actual parent List/ScrollHandle. Exercise wheel X/Y/diagonal/edges and real thumb drag input, checking offsets and parent position, plus multiple blocks/state. Do not test copied decisions or source strings. Existing GPUI test-support feature may be enabled development-only; no new dependency or framework patch. Do not restore old test sets. Remove tests and exclusive feature/lockfile support only after cloud results and accessible source evidence are fixed.
4. Local checks only rustfmt, diff, static manifests/lock accounting. No Vega, build, cargo check, Clippy or tests locally. Existing PR Actions run all automated validation. Final test-free head must pass required CI; independent review must cover full final head. No CI changes.

## Verification status

Pending implementation and cloud verification. User manual acceptance follows merged code and normal user-controlled installation; neither CI nor merge means the running application has changed.
