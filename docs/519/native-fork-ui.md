# Native project top-level Fork UI

## Scope and baseline

User authorized #519, Native project top-level chats only. Baseline: `c589766f74bc23cf9ba3e818d9d403e873abfe7c`. Dependency #518 merged as `429c6a819fc5a30f8280afd2f6ed015d4682bf41`; its final contract is `docs/518/native-thread-fork.md` and `docs/518/validation.md`. No ACP/Codex Fork, user editing (#523), runtime recovery redesign or new dependencies.

Current AGENTS overrides the old issue's mandatory TDD and docs/498 placement. This spec lives in docs/519. No local Vega execution, build or tests. Independent non-writer review and final-head required Actions precede merge; user manual acceptance precedes closing/Done.

## Current-code findings

- `render_rows::MessageActions` carries Copy/timestamp/focus; `phase::final_copy_for_entry` identifies trusted Final but is not itself a backend/top-level eligibility check.
- `threads::fork_thread` validates and atomically copies the durable prefix, including inert delegation, but does not know UI/controller live ownership. Store validation remains authoritative.
- `threads::load_conversation`, hydration/location workers and privacy refresh use legacy history readers. Those readers intentionally refuse any archived delegation. Exposing a button without adapting these consumers would create unusable fork histories.
- `TrustedActionCoordinator` supplies app-level single-flight ownership. `owns_stream_request` and message-location route generation fence asynchronous publication, including away-and-back navigation.

## UI and request contract

1. Final response footer: Copy then a direct GitBranch button, accessible localized label `Branch in new chat` / `在新聊天中分支`. Not a menu item. Existing footer height, Copy geometry, original text copying, timestamps and user-message controls remain unchanged.
2. Each button retains individual hover/focus visibility; a focused message reveals both. No transparent parent concealing focused children. Existing 24px actions stay horizontal in the 28px footer; no extra vertical space is introduced.
3. Emit `ForkRequested { thread_id, message_id }` only for a Native, project-bound, parentless thread with a trusted Final entry. Store additionally rejects workspace-owned/noneligible records. User, Codex/ACP, standalone and child threads have no Fork button.
4. Handler revalidates stream route, current thread identity/backend/project/parent, trusted Final anchor and idle UI/controller/global runtime owner. Running/busy requests produce a bounded localized recovery message. No raw storage errors or paths in UI/logs.
5. Acquire a dedicated Fork trusted-action lease before spawning blocking Store/fork work off UI. The worker also holds the source thread-admission guard through copy so sends in another window cannot race the idle check. Duplicate requests while leased create nothing. Hold lease until work returns, even if navigation changes; never cancel-and-replay unknown storage work.
6. Completion releases only its own lease. Publish/open the created thread through existing navigation only when stream, source ID and route generation still match. A committed fork after navigation remains sidebar-discoverable but must not steal the current route. Failed storage creates no partial thread and does not navigate. No automatic run; existing Composer prediction behavior is not suppressed.

## Historical consumer contract

Add a conversation-layer display-history adapter with latest/restart/before/after/containing-message entry points. Non-archive threads delegate unchanged to existing readers. Archive threads consume #518 typed historical readers, preserving page ordering/cursors, privacy projections, ordinary entries, skill provenance, interrupted tail and current-thread live subagent metadata (for new runs after forking). Restart still performs existing admission-fenced recovery before read; archive references never enter recovery ownership.

Map each archived delegation to a read-only tool card with original call/message identity, actual terminal status/approval, visible historical label and full already-projected input/receipt/agent summaries/checkpoint/references in expandable details. Do not deserialize archived data into `SubagentSnapshot`, resolve external paths, query source child state, or expose open/stop/resume controls. Use a distinct non-invocation tool identity with no snapshot; existing live delegation cards are unchanged. Legacy readers keep their explicit refusal.

Wire initial/restart load, older/newer hydration, message location and privacy refresh to the adapter. Live subagent panel/admission readers remain on their existing non-archive paths.

## Minimal implementation map

- `vega_ui/conversation_stream/{mod,core,phase,render,render_rows}.rs`: event, eligibility, button, bounded Fork failure presentation. `tool_card.rs` adds expandable details only for the inert historical identity.
- `vega/src/trusted_action.rs`, `window/native_subagents.rs`: dedicated lease kind and guarded async handler/subscription, existing navigation for opening.
- `vega_conversation/src/display_history.rs`, `history.rs`, `threads.rs`, `lib.rs`: typed inert adapter, shared restart recovery, loader routing.
- `vega/src/thread_reload.rs`, `window/privacy.rs`: route display-history reads through adapter.
- `vega_i18n/locales/ui.{en,zh-CN}.toml`: action and bounded retry/wait errors only.

## Verification

Local: focused diff/static inspection, rustfmt, whitespace, no added comments/unwrap/expect/dependencies or Rust tests. Cloud: required fmt/Clippy/Nextest and existing Python checks on exact final head. Focused development regressions should exercise archive projection (no executable snapshot, exact details/status/cursors, ordinary parity), eligibility, lease duplicate handling and stale route publication where feasible; preserve actual cloud evidence, and clean any development-only Rust tests/support before merge per current policy. Do not claim visual/keyboard/runtime verification from static checks.

User manual acceptance after merge: eligible Final shows Copy + Fork on hover/focus, keyboard reaches/activates them; ineligible threads/user messages do not expose Fork; selected-prefix result opens, source remains unchanged/sidebar-visible; rapid duplicate click only creates once; running/busy action refuses; stale completion does not navigate; storage failure permits retry; restart/paging/privacy and new continuation of an archive-containing fork retain inert history; light/dark and narrow windows preserve footer/timestamps/scroll.

## Progress

Spec v1 written before product changes. Initial implementation is in progress. Static follow-up identified and addressed historical tool detail visibility, credential-continuation projection parity and cross-window thread admission. Cloud/independent review and manual validation remain pending.
