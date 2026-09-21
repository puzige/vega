# Issue 67 · Navigation must not interrupt the active conversation

> Second-stage update (2026-09-21): R5 and I67-06 are superseded by
> [Concurrent conversations](vega-issue-67-concurrent-conversations.md).
> Different threads must now run concurrently; this document preserves the
> first-stage navigation history and its remaining guarantees.

Freeze: 2026-09-21, Asia/Shanghai. Source: GitHub Issue #67 plus the user's
clarification in the implementation session. The reported failure is broader
than creating a new task: opening another task or Settings also interrupts the
currently running conversation.

## Actual behavior and root cause

`VegaWindow::render` treats route/view replacement as agent teardown. Opening
Settings calls `cancel_active_agent`; replacing `stream_view` for a new or
existing task does the same and times out the departing permission projection.
The provider/tool worker therefore receives cancellation even though the user
did not press Stop and the window remains alive.

The application currently owns one `AppAgentController::active` run per window.
That owner already retains the originating `ConversationStream`, cancellation
token and generation until the durable terminal handshake. Completion already
fences the current route through `current_cached_stream_for_thread`, so an
inactive run can finish without navigating or overwriting another task. The
fix must use those existing ownership boundaries rather than introduce a
second execution model.

## Contract

R1. Navigating from a running task to a new lazy draft, another durable task,
or Settings must not cancel the active run, resolve its cancellation token, or
project an interrupted terminal solely because the route changed. Provider,
tool, store and audit work continues under the original frozen run authority.

R2. The active run retains its exact `ConversationStream` while it is off
route. Returning to the originating task before terminal completion reuses
that entity, including streamed text/tool rows, composer running state and a
pending permission card. A non-active departing stream keeps the existing
bounded draft/navigation behavior and must not become a new general stream
cache.

R3. Background events and terminal reloads belong only to the originating
thread. They must never change `OpenedThread`, selected project, Settings
visibility, the current task's stream, composer draft or focus when another
route is current. After terminal completion, reopening the originating task
projects the durable result normally.

R4. Explicit Stop for the exact current active task and window teardown still
cancel the run and retain the existing durable interrupted handshake. Runtime
failures and existing safety cancellation paths remain terminal. Merely
opening a page is not a cancellation signal.

R5. This card preserves the existing single-window single-active-run policy.
A second task can be opened and edited while the first runs, but starting a
second agent run remains unavailable until the first reaches terminal. A
refused second submission must retain its draft and must not cancel or replace
the first run. Per-task parallel execution, activity badges and background
notifications are separate product work.

R6. Opening Settings does not change the frozen provider/model/permission/
Skill/MCP authority of the active run. Settings changes continue to affect
future runs according to their existing contracts. The existing fail-closed
rule for a permission prompt hidden by Settings is not broadened by this card;
the primary run itself must not be route-cancelled.

This contract supersedes the R12 navigation statement that every task change
must drop all worker-owning entities. Only the exact active agent stream is
retained, and only until its terminal handshake; ordinary task streams remain
draft-only cached state.

## Acceptance matrix

| ID | Requirement / risk | Precondition | Action | Expected observable result | Evidence level |
|---|---|---|---|---|---|
| I67-01 | New-task navigation does not cancel | Production root, durable task A, gated real conversation worker running | Use the real new-task route while A is blocked, then release it | A token never becomes cancelled; A reaches durable success; new draft stays current and receives no A content | Mounted production controller + owned store + `MockProvider` network seam (`E2E-REAL`) |
| I67-02 | Existing-task switch and return retain live owner | A running; durable B exists | Open B, then return to A before terminal | B is isolated; A reuses the exact originating stream and still shows running/live state | Mounted production root (`E2E-REAL`) |
| I67-03 | Settings does not cancel | A running | Open Settings, allow at least one event/worker step, close Settings | Token remains live; Settings stays current until closed; A completes and reopens with durable output | Mounted production root (`E2E-REAL`) |
| I67-04 | Late background completion cannot steal route | A running, B/new draft current | Let A finish while away | Current route/project/draft are unchanged; only A store/audit rows advance | Controller/store regression (`E2E-REAL`) |
| I67-05 | Explicit terminal controls remain authoritative | A running | Press Stop; separately drop a window fixture | Exact token cancels and terminal interruption/cleanup remains unchanged | Existing production controller regressions + focused assertions |
| I67-06 | Single-flight refusal is non-destructive | A running, B has a nonempty composer draft | Attempt B submit | B draft remains; A token/run identity remains unchanged; no second provider worker starts | Mounted controller regression |

The regression must first fail on the current baseline by observing route-driven
cancellation. Tests may gate a mock network response, but must exercise the real
`VegaWindow`, `ConversationStream`, controller, worker and file-backed store;
an isolated token-only unit test is insufficient.

## Implementation boundary

- `crates/vega/src/app_agent.rs`: expose the exact active stream lookup needed
  by route rendering without changing the single-flight controller model.
- `crates/vega/src/window/render.rs`: remove page-driven agent cancellation,
  retain/reuse only the exact active stream, and keep stale non-active stream
  permission cleanup.
- `crates/vega/src/tests/agent.rs` (or an equivalently focused production-root
  test module): add the red/green navigation and Settings regressions.
- Update the conflicting R12 navigation sentence and produce
  `docs/vega-issue-67-background-run-delivery.md` with commands, results and
  residuals.

No schema, public conversation API, dependency, provider protocol, execution
policy, permission auto-approval, or general multi-stream cache is authorized.
Do not weaken generation/run/route fences. Follow the repository cargo lock for
all builds/tests and preserve any first failing evidence.
