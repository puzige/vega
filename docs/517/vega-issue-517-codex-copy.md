# Codex terminal reply copy — implementation contract v2

Date: 2026-10-24. Issue #517, parent #498.
Baseline: `77fa384abf6f2cc0c022e1a0023db12201371354`.
Claim: `codex-copy-takeover-20261024T1750Z`.

## Goal and scope

Completed and interrupted Codex assistant replies expose the existing hover/focus copy action. Native trusted Final copying remains unchanged. No Fork, visibility redesign, new labels, dependencies, persistence, or code-block changes.

This contract follows current AGENTS.md and execution guide §7.2, replacing the issue's historical mandatory TEST-FIRST instruction. Ticket artifacts live in docs/517/ under the current directory rule, rather than the issue's historical docs/498/ location.

## Verified cause and interfaces

- `conversation_stream/phase.rs::final_copy_for_entry` requires trusted response metadata and matching copy owner. Native records that metadata in `vega_conversation/src/agent/events.rs`; Codex does not fabricate it.
- `vega/src/codex_agent.rs` accumulates `agent_message_chunk` separately from thoughts; `finish_codex_message` persists done/interrupted/failed state, and completion/interruption events finish the live message.
- `conversation_stream/content.rs::finish_agent_message` finalizes the Markdown stream, invalidates its entry and clears `active_agent_message`; history hydration marks failed assistant entries with `failure`.
- `render.rs` wraps replies using `final_copy_for_entry`, and `message_anchors.rs` uses the same predicate for the 28px footer estimate. Both must switch together.

## Minimal implementation plan

1. Add `ConversationStream::copy_source_for_entry(index) -> Option<&MessageCopy>` in phase.rs.
2. Return the existing `final_copy_for_entry(index)` first, unchanged.
3. Fallback only for `TaskBackend::Codex` and `StreamEntry::Assistant { failure: None, copy, .. }`, whose original text is nonempty and which is not the active message. Compare entry index and, where available, durable message identity against `active_agent_message`, so a prior segment of the current message cannot become copyable. An active different message must not hide older completed replies. Reject a message present in existing `run_failures` as well: failure exclusion is message-wide, even when an earlier live segment still has `failure: None`.
4. Switch the render wrapper and footer estimate to the unified predicate. Do not change `final_copy_for_entry`, response metadata, copy contents, UI action layout, or input handling.
5. In `content.rs::finish_agent_message`, after clearing the active message, invalidate every assistant entry whose durable message identity matches the finished Codex message, using existing `invalidate_item` to remove measured heights and remeasure while preserving the scroll anchor. Keep Native behavior unchanged and do not invalidate unrelated messages or tools. This covers terminal events while the current index is `usize::MAX` after a tool boundary. Existing current-entry stream finalization remains unchanged.
6. Also invalidate matching Codex assistant entries when `finish_run_activity` changes message terminal status (especially a later failure), so existing cached footer heights cannot survive a newly remembered message failure. Prefer one small shared helper for matching/invalidation; no new state or general cache mechanism.
7. If actual interfaces contradict this plan, report the exact gap before broadening production scope.

## Acceptance and regression matrix

| Case | Required behavior |
|---|---|
| Native trusted Final | Exact trusted Final slice, not aggregate commentary; unchanged owner validity |
| Native without trusted Final / stale owner | No new fallback |
| Codex done / hydrated done | Nonempty original text of that rendered assistant entry is copy source; tool-separated entries are not concatenated |
| Codex interrupted / hydrated interrupted | Nonempty partial reply is copy source |
| Active Codex message, including its earlier segments | No fallback source |
| Older completed reply while another reply runs | Remains copyable |
| Failed assistant or any segment of a remembered failed message, empty reply, non-assistant, invalid index | No fallback source |
| Native/Codex action visibility | Hidden by default; existing hover/focus and keyboard activation/name/tooltip retained |
| User messages / code blocks | Unchanged |
| Scroll and layout | Render and estimate share predicate; existing 28px footer, sizes, light/dark and narrow layouts retained; all matching Codex assistant cached heights clear at termination/status changes, including a tool-boundary terminal with no current text entry |

No Codex assistant_responses rows or synthetic Final boundaries. Copying does not add Fork capability.

## Validation and delivery

This is a bounded UI eligibility fallback using existing message identities, failure tracking and anchor-preserving invalidation; no persistence or new input handling. v2 explicitly addresses segmented lifecycle rather than ignoring the static finding. Use focused static tracing of all entries across done/interrupted/failed transitions (including tool-boundary sentinel and unrelated messages), formatting/diff checks, independent non-writer implementation review and existing full PR Actions. No temporary Rust suite is required for reusing these existing UI mechanisms; no tests or test-only support may enter master. Clipboard/scroll/keyboard integration remains explicitly user-unverified until manual acceptance.

Local execution, builds, cargo check/Clippy/tests and app installation/restart are prohibited. Full required CI must pass on the exact final head before squash merge. CI empty Rust inventory means `no Rust tests`, not behavior-test success. Native clipboard, hover, keyboard and visual/scroll integration remain user manual acceptance after merge; no automated result substitutes for them.

Merge leaves #517 open in In review, parent #498 remains In progress for subsequent work. No other child task starts under this claim. Revert the unified helper/call-site change together if needed.

## Current evidence

Specification and static source investigation only; implementation and cloud validation pending. No Vega execution, compilation or tests run locally.

v2 revision: implementer found tool-separated segments, entry-local terminal invalidation and failure annotation at content.rs:1461, 1603–1618 and 1669–1682; parent independently confirmed these against baseline. Existing `run_failures` and `invalidate_item` avoid new state or failure presentation changes. v1 three-file scope is expanded only to the necessary content.rs lifecycle invalidation. Original product acceptance is preserved, not relaxed; source remains per rendered entry as in the issue's `entry.copy` fallback.
