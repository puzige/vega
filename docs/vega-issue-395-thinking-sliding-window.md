# Thinking retention follows the latest content

## Source and supersession

Issue #395 follow-up, authorized by the user on 2026-10-16. PR #396 bounded background preparation and viewport rendering, but the existing retention policy still keeps the oldest prefix and discards later thinking after a display budget is exhausted. The user requests a sliding window that keeps the latest content. This document supersedes prefix retention and the “Thinking display limit reached” presentation in the earlier thinking contracts. Other behavior in [the responsiveness spec](vega-issue-395-thinking-ui-performance.md) remains applicable.

This is a UI retention and rendering change. It does not change provider output, model context or reasoning limits, task completion, answer/tool content, persistence, dependencies, or application installation. Thinking remains memory-only. The existing external issue stays open for user acceptance.

## Behavior contract

### Retain recent content within the existing budgets

- Retain a contiguous recent suffix of each thinking source, prioritizing new content over its oldest content. Keep the existing 256 KiB shared raw-thinking/summary budget per card and 1 MiB view budget. The view budget is shared across messages; older cards must yield capacity so the newest card is never permanently starved.
- Keep at most 256 retained thinking bodies. When older bodies leave the window, release their source, copy buffers and prepared rendering data. Preserve tool and answer records and their relative order. Preserve the minimum truthful stage/title metadata needed for the existing timeline; do not keep an unbounded secondary archive of evicted text or prepared bodies.
- A large individual event must preserve its newest suffix, too. The existing 64 KiB delta constant must not make the UI silently keep an older prefix or concatenate text across a dropped middle. It may serve as a processing or eviction batch size. The retained window remains bounded even when one event exceeds the entire card budget.
- Prefer whole paragraphs or completed Markdown blocks when selecting an eviction boundary. For a single oversized paragraph, code block or code line, advance at a UTF-8 boundary and keep its latest text. Preserve known fenced-code context in rendering metadata; synthetic context must not enter copied source. Avoid per-delta full Markdown parsing just to find a boundary.
- Title/preview continues to reflect incoming content after the first eviction. Preserve summary precedence and source switching; raw and summary together stay bounded, and a full raw buffer cannot prevent a later summary from appearing.

### Keep folded processing cheap and visible work bounded

- Folded, ancestor-hidden and offscreen bodies must not prepare or parse Markdown. Incremental append/title/budget accounting is allowed. Eviction must not introduce a cumulative full-source copy or front-shift on every small delta; use amortized eviction or chunked storage.
- Maintain the existing outer native list and inner viewport rendering. One oversized retained block cannot create one UI element per line/character. Keep at most one preparation task per card, coalesce incoming changes, and retain the existing background shaping boundary.
- Treat front eviction as a source-window change, not an ordinary append. Background results must carry enough source/window identity to avoid restoring discarded text or attaching old offsets to a new window. Sustained streaming must still make forward visible progress; repeatedly rejecting all work indefinitely is not acceptable.
- If completed preparation still overlaps the current retained source, it may publish only that accurately mapped intersection while preparing newer content. If incoming data replaces the entire retained window before a worker can finish, there is no valid intersection: discard the obsolete result, keep the current title and a neutral pending body, and prepare the newest window. This extreme input-rate case cannot promise a current full body before data slows sufficiently; it must not show evicted content or block the UI. Cover both overlapping continuous progress and complete-window replacement explicitly.
- Release obsolete prepared/copy state when an entire card is evicted. Bounded in-flight snapshots may finish, but their results cannot resurrect an evicted card's content.

### Preserve reading and copy behavior

- When the reader is at the bottom, continue following the latest content as the window slides. When reading retained middle content, preserve the same logical content and its viewport offset. If the reading anchor itself is evicted, clamp to the earliest retained content; do not jump to the bottom. Preserve resize, collapse/reopen and inner-to-outer scroll boundary behavior.
- Preserve a retained code/table block's horizontal scroll when preceding blocks leave the window. A single oversized unwrapped code line is a narrower limit: if that same line loses its own prefix, its horizontal origin changes. Reuse the horizontal offset and clamp it to the new width; this release does not guarantee the same source byte remains at the same horizontal screen position in that case. Vertical source anchors and tail following still apply. Do not generalize this limit to unaffected code blocks or claim complete horizontal byte-position preservation.
- Selection/copy offsets must belong to the displayed retained snapshot. Preserve a selection when its selected content is still retained and can be mapped accurately; clear it when its content is evicted or the source switches. Never copy unrelated text through stale offsets. “Copy original” copies the retained source exactly, without synthetic Markdown repair markers or an omission message.
- Replace the existing cap warning with the concise, truthful “Earlier thinking omitted” indication only after earlier content has actually been discarded. It is informational, not a task failure. Do not add a permanent counter, internal budget status, or recovery control for content that is not retained. Reuse existing styling and disclosure interactions.

## Acceptance matrix, frozen before implementation

| Case | Operation and required observation | Evidence |
| --- | --- | --- |
| Continuous folded overflow | Stream beyond the card budget in small UTF-8 deltas; newest sentinel and title advance, retained source/copy match, memory stays bounded, and body preparation remains idle | Production-path GPUI/model regression, cloud |
| Large event and Markdown | Oversized event, one giant paragraph/code line, fenced code spanning an eviction and multiline Markdown; keep the latest contiguous suffix, valid UTF-8 and truthful copy | Focused retention and production-render regression, cloud |
| Cross-card/view pressure | Exhaust 1 MiB and more than 256 bodies across message/tool/answer boundaries; oldest thinking yields capacity and newest text still appears; tool/answer order remains | Production-path regression, cloud |
| Summary switch | Fill raw thinking then receive and overflow a summary; summary takes precedence and latest summary remains visible within the shared budget | Production-path regression, cloud |
| Scroll and selection | Read at bottom and in retained middle, then evict; follow tail or preserve logical anchor as appropriate; evicted anchor clamps; retained selection maps, evicted selection clears; copy matches visible retained source | GPUI regression, cloud; user manual check |
| Worker lifecycle | Evict while preparation is in flight, hide/reopen, replace source and recycle outer entry; stale work cannot resurrect discarded text, reopening shows latest content, and visible streaming progresses | Deterministic state-transition regression, cloud |
| Existing responsiveness | Folded unchanged state avoids redundant geometry invalidation; expanded giant content renders only viewport plus overscan; final/tool states and disclosure boundary behavior remain | Existing #395 and related regressions, cloud |

Avoid timing thresholds and duplicated coverage. Update only assertions that explicitly encode the superseded prefix, admission-stop, or maximum retained-body semantics. Preserve unrelated security, content, ordering, virtualization and copy assertions. Record such intentional test changes in the delivery document. Hydration clears this memory-only window as before; no storage migration or network fixture is needed.

## Implementation ownership and sequence

The main agent owns this spec, review, integration and GitHub delivery. A dedicated implementation agent owns `crates/vega_ui/src/conversation_stream/` and directly necessary existing UI helpers/tests, plus `docs/vega-issue-395-sliding-window-delivery.md`. A narrowly scoped, UI-free production source-position mapping helper in `crates/vega_markdown/` is allowed where the existing stream facade lacks those offsets. It must use the existing parser semantics and dependencies and preserve other Markdown behavior. A separate reviewer is read-only. There is one writer for production and test code.

First design and write focused regression cases from the matrix, then implement retention/accounting and eviction-aware prepared-source identity, then verify scroll/selection and notification behavior. Report an incompatible existing API or a needed contract tradeoff before substituting degraded behavior. No new dependencies, public test-only APIs or code comments.

The prepared body's offsets refer to rendered text, not raw Markdown. Anchor/selection mapping must use source positions or an equivalently exact mapping; subtracting evicted raw bytes or searching for repeated text is insufficient. The mapping work belongs to background preparation. Batch eviction and copy-buffer compaction are allowed so small deltas do not shift the entire retained source repeatedly.

The clean existing task worktree is reused on `feat/thinking-sliding-window`, starting from freshly fetched `origin/master` containing PR #396. The earlier feature branch remains available; the main checkout and unrelated worktrees are untouched.

## Validation and delivery

Local checks are restricted to fast formatting/static/diff checks. No local compilation, build, Clippy, Rust tests or packaging. Cloud PR Check runs format, Clippy and workspace nextest including the new regressions; retain first-failure evidence and do not weaken tests to make CI pass. Poll CI at most once per five minutes, except an explicit completion/failure notification.

After required checks are green, squash merge through a PR, set #395 to In review and wait for user manual validation. Keep the issue and worktree until that result. Report merged code separately from the installed app. Rollback is a revert of the scoped PR; no data migration is involved.
