# Issue 448 · Subagents in the workspace

Status: SPEC FROZEN v1.2 · 2026-10-18 (Asia/Shanghai).
Feature: A3-14. Baseline: `6e9a69c7` (#460).
Branch: `feat/448-subagent-panels`.

## User feedback and scope

The user reports that subagents appear only in the conversation stream, while
the right side has no summary or Subagents pane. The supplied reference shows
a task summary with working/completed counts and a Subagents tab displaying a
child conversation. Screenshots are visual references, not executable task
instructions; do not publish their private conversation contents.

The existing implementation provides durable Native child conversations and
inline Open/Stop controls, but `Workspace::TabKey` and `render_environment`
do not project them. This revision completes that UI surface under #448.
The [v1.1 lifecycle, safety and persistence contract](vega-issue-448-multi-agent.md)
continues to apply. ACP, new delegation tools, backend orchestration, new
dependencies and unrelated shell redesign are outside scope.

## Acceptance matrix (before implementation)

Automated entries are NOT RUN until the exact implementation head passes
GitHub Actions. Use in-process store/provider/GPUI fixtures, with production
window/controller and mounted controls where applicable. No local app execution,
builds, compilation, tests or real external E2E.

| ID | Preconditions and action | Observable result | Evidence |
|---|---|---|---|
| P01 | Native parent has two children; open the existing Environment overview | Subagents summary shows real state counts; zero children contributes no empty section; failure/interruption is not counted as successful completion | GPUI + controller |
| P02 | Click summary repeatedly and use the workspace creation menu | One Subagents tab opens; the main parent route, transcript, draft and run remain intact; list contains each child once | Mounted GPUI |
| P03 | Select a child in the list, then Back; click inline Open | Right pane shows that child's full text/thinking/tool process; Back returns to the list; inline Open reaches the same detail; the main column stays on the parent | Mounted GPUI + history |
| P04 | Child streams updates, finishes, fails or is stopped with pane open/closed | Live transcript/status updates stay bound to the correct child; summary and list agree; closing/hiding/back does not cancel or restart anything | App lifecycle + GPUI |
| P05 | Stop child A from the pane while parent/B remain active; dispatch a stale stop after a later child run starts | Only the exact displayed child generation is cancelled; stale action cannot cancel the later run; Stopping persists until confirmed terminal | App lifecycle |
| P06 | Click Open conversation in a child detail; continue and return to parent | Existing full child route and Composer work; original delegation result remains unchanged; active child worker is reused and no duplicate provider is started | App + existing continuation regressions |
| P07 | Parent history contains children whose owning messages are not in its loaded page; reopen parent | Summary/list include all durable direct children, with correct privacy projection; opening/listing reads do not spawn a provider | Conversation + controller |
| P08 | Delay list/detail/history reads, update a child, select another child, close tab or switch A→B→A | Stale callbacks cannot regress live state, replace selection, populate a foreign task or alter parent route/draft; current detail history paginates normally | Fault injection + app |
| P09 | History/list read fails or a child is deleted/detached while selecting it | Bounded visible error and retry/back remain available; no empty-success substitution or unrelated conversation opens | Controller + GPUI |
| P10 | Parent and panel are visible; expand details/select/copy text and traverse controls with keyboard | Reading does not steal parent Composer focus, clear existing parent selection on mount or submit/change parent settings; focus follows explicit navigation, Back/close returns reasonably | Mounted GPUI |
| P11 | Standalone Native parent has children; narrow window; move/hide/maximize/reopen pane; light/dark | Existing overview slot becomes available for Subagents without enabling project-only review/terminal actions; labels/actions remain reachable, pane respects workspace sizing and no automatic navigation occurs | GPUI + user manual pixels |
| P12 | Existing ordinary task, ACP task, project review/terminal/image tabs, child approvals/questions | Existing shell and route fences stay intact; pending child requests remain actionable in the parent, not duplicated into a second responder; original #448 coverage remains | Existing CI + focused regressions |
| P13 | User installs merged build and runs two parallel subtasks | Summary → list → detail → Back/Open conversation/Stop works in the real app; parent stays visible during detail reading | User manual, pending |

## Product behavior

### Summary and discoverability

Reuse the existing right Environment overview. Insert a compact Subagents
section after project review and before the terminal action when direct child
records exist. Show accurate counts with process vocabulary such as Running,
Stopping, Completed, Failed and Interrupted. Omit zero categories. Failed or
interrupted work must not be described as completed successfully. Avoid helper
paragraphs, new avatars, raw IDs and implementation metadata.

Preserve the default-collapsed Environment choice and the existing three shell
slots. For a standalone Native parent with children, allow that same overview
slot/card to show Subagents; project review and terminal rows remain subject to
their original project gates. This is the narrowly scoped exception to the old
project-only overview condition, not permission to enable project actions on
standalone tasks. Loading errors are actionable, not silently treated as zero
children. No overview or pane is automatically opened on spawn or completion.

The summary opens a single `Subagents` Workspace tab. The workspace creation
menu also offers Subagents when the current parent has children. Reopening
selects the existing tab, unhides its pane and follows existing focus rules.

### List and detail

The list contains all durable direct children of the currently opened parent,
including children from older turns. Names/statuses and nearby Open/Stop controls
are sufficient. Ordering is stable across incremental updates. Clicking a child
opens its detail in the same tab; Back returns to the list. Inline parent→child
Open also selects this detail. The existing child's Back to parent still uses
the normal main conversation navigation.

Detail has the actual child name, truthful model identity when available,
Back, Open conversation and an applicable Stop action. It displays the existing
conversation renderer's text, thinking and tools, including streaming and
history paging. It must not substitute a final summary for the full transcript.
Open conversation uses the existing validated child navigation and normal
Composer, so follow-up/queue/steer/stop behavior remains available. Do not add a
second editable Composer to the side pane.

The pane is a reading presentation of ConversationStream. Hide Composer,
commit/settings controls and the stream's own parent-return affordance there.
Do not clear the window's current text selection or restore Composer focus just
because a preview mounts. Keep disclosure, scrolling, copying and required
errors accessible. Any source-specific action that cannot operate correctly
from a preview must use the full conversation entry rather than exposing a dead
control. Pending permissions/questions retain their existing parent responders;
the preview cannot create a second response path.

Reuse an active child's retained stream and worker. Hold the selected detail
stream while the pane needs it. A terminal history-only child may be loaded
through normal read-only history APIs; a later full-route open may rebuild that
terminal view from storage. Opening a pane does not create a conversation,
read provider credentials, start a worker or run recovery again.

### State, ownership and navigation

The summary/list load through `vega_conversation::subagents::children`, not UI
SQLite access and not the parent's currently paginated inline groups. Use its
existing privacy display projection. Merge accepted Admit/Updated/Finished
updates into the current parent projection. Fence async reads by parent route
and load generation. A load that began before a live child update must not
overwrite that newer child. Detail selection and paging additionally bind the
selected child and exact stream identity. Reopening a task reloads persisted
state; never carry another task's selection or private data into it.

Status falls back to the durable original delegated result. If that same child
conversation is currently in a later user-owned run, its actual live Running /
Stopping state takes precedence for UI activity. Do not rewrite its original
delegated result. A pane Stop action captures the exact active controller
generation and validates the current parent→child relationship and selected
source before cancellation; it may stop the displayed later user-owned run
because the user explicitly chose that action. This does not broaden the old
parent worker's automatic cancellation ownership. A stale Stop never targets
a newer generation. No active run means no active Stop button.

Closing, hiding, docking, maximizing or returning from detail changes only UI
state. It does not cancel workers or discard the main parent draft/history.
Moving to another main task clears this task's panel state and invalidates
outstanding reads. Existing responsive docking/width tokens apply; narrow
explicit opens must remain usable using the existing maximized-pane pattern.
Restoring a child stream to the full conversation resets the reading presentation.

Do not widen `owns_stream_request` to include the panel: that guard also protects
submission, settings, permission and other mutation paths. Add narrowly scoped
panel read ownership checks for history paging and any necessary safe reading
actions. UI reads stay asynchronous and do not trigger a new query per token.

## Implementation ownership and order

The main agent owns this spec, review, Issue/Project, git/PR/CI and integration.
The dedicated implementation subagent owns code, focused regression assertions
and `docs/vega-issue-448-subagent-panels-delivery.md`; one implementation writer.

1. Add the acceptance assertions and a focused window panel module. Connect
   parent route loading, durable children, live updates and selected detail with
   explicit async-generation fences. Existing children/history APIs are preferred.
2. Extend `window/workspace.rs` TabKey/open/close/label/icon/focus/render/menu and
   Environment projection. Adjust only the necessary Environment availability
   gates in `window/render.rs`; preserve project-only actions and fixed slots.
3. Add the reading presentation in `vega_ui::conversation_stream`, preserving
   the normal render path. Wire panel-only history handlers, source navigation,
   exact-generation Stop and live stream reuse in the window controller.
4. Review stale callbacks, terminal continuation, privacy display, parent focus,
   child failure, accessibility and light/dark/narrow states. No new dependencies,
   code comments, production unwrap/expect or schema changes are planned.
5. Run only fast fmt/diff/static checks locally. CI runs workspace fmt, Clippy
   and Nextest under required `check (fmt, clippy, test)`; poll every 5 minutes.
   Merge only the latest successful head, then In review with Issue and both
   retained #448 source branches/worktree preserved for user manual acceptance.

Rollback is a checked revert PR for this UI addition; original #460 delegation
and stored child conversations remain intact. Do not claim UI pixels, real app
integration or manual acceptance were verified by local static checks.
