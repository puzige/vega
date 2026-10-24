# Codex terminal reply copy — implementation contract v1

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
3. Fallback only for `TaskBackend::Codex` and `StreamEntry::Assistant { failure: None, copy, .. }`, whose original text is nonempty and which is not the active message. Compare entry index and, where available, durable message identity against `active_agent_message`, so a prior segment of the current message cannot become copyable. An active different message must not hide older completed replies.
4. Switch only the render wrapper and footer estimate to the unified predicate. Do not change `final_copy_for_entry`, response metadata, copy contents, UI action layout, or input handling.
5. If actual interfaces contradict this plan or lifecycle invalidation is insufficient, report the exact gap before broadening production scope.

## Acceptance and regression matrix

| Case | Required behavior |
|---|---|
| Native trusted Final | Exact trusted Final slice, not aggregate commentary; unchanged owner validity |
| Native without trusted Final / stale owner | No new fallback |
| Codex done / hydrated done | Nonempty whole original reply is copy source |
| Codex interrupted / hydrated interrupted | Nonempty partial reply is copy source |
| Active Codex message, including its earlier segments | No fallback source |
| Older completed reply while another reply runs | Remains copyable |
| Failed assistant, empty reply, non-assistant, invalid index | No fallback source |
| Native/Codex action visibility | Hidden by default; existing hover/focus and keyboard activation/name/tooltip retained |
| User messages / code blocks | Unchanged |
| Scroll and layout | Render and estimate share predicate; existing 28px footer, sizes, light/dark and narrow layouts retained |

No Codex assistant_responses rows or synthetic Final boundaries. Copying does not add Fork capability.

## Validation and delivery

This is a small UI eligibility fallback with no data mutation or new input handling. Use focused static tracing of the matrix, formatting/diff checks, independent non-writer implementation review and existing full PR Actions. No temporary Rust test suite is required solely for this predicate change; no tests or test-only support may enter master. If investigation finds a new complex boundary, reassess necessary cloud-only regression coverage rather than weakening acceptance.

Local execution, builds, cargo check/Clippy/tests and app installation/restart are prohibited. Full required CI must pass on the exact final head before squash merge. CI empty Rust inventory means `no Rust tests`, not behavior-test success. Native clipboard, hover, keyboard and visual/scroll integration remain user manual acceptance after merge; no automated result substitutes for them.

Merge leaves #517 open in In review, parent #498 remains In progress for subsequent work. No other child task starts under this claim. Revert the unified helper/call-site change together if needed.

## Current evidence

Specification and static source investigation only; implementation and cloud validation pending. No Vega execution, compilation or tests run locally.
