# Issue 448 · Native parallel subagents

Status: SPEC FROZEN v1 · 2026-10-18 (Asia/Shanghai).
Feature: A3-14. Baseline: `6382c908`. Branch: `feat/448-multi-agent`.

## User decisions and scope

The user requests parallel subtasks within one task: the parent delegates,
observes progress and collects results. Each subagent is an independent durable
conversation which the user can open, read in full, continue and stop. This card
implements the Native backend only. ACP, Codex routing/capabilities and external
agent profiles are outside scope. The research in the latest #448 comment is
context, not authorization to implement its ACP proposal.

V1 permits four concurrently active delegated children per parent run, one
delegation level, and explicit rejection at capacity rather than a hidden queue.
Children use the parent's frozen provider/model/reasoning and workspace. A
standalone child resolves to its parent's actual workspace, never a newly
allocated scratch root merely because its thread ID is new. There
is no new provider setting, role marketplace, remote/worktree isolation or team
mailbox. No new dependencies.

## Acceptance matrix (defined before implementation)

All automated entries start NOT RUN and execute only in GitHub Actions. Tests
use in-process provider/tool substitutes and owned temporary storage; no real
external process/network/application E2E. UI tests use the production stream and
window/controller paths, not only label helpers.

| ID | Preconditions and action | Observable result | Level |
|---|---|---|---|
| M01 | Parent delegates A and B; both providers are gated | Both enter their provider before either gate releases; parent remains responsive | Runtime + conversation |
| M02 | A/B emit interleaved text, thinking, tools and terminals | Independent child messages/tools/history; parent receives child progress, never child text as its own reply | Conversation + app |
| M03 | Spawn four active children, then a fifth; release one | Fifth rejected before creation/provider call; freed slot reusable; one-level depth enforced | Runtime + orchestration |
| M04 | Malformed/oversized spawn, duplicate name, unknown/foreign child ID | Explicit bounded error; no worker or unauthorized state/control | Runtime + storage |
| M05 | Parent calls list/wait with a gated child; then child completes | Live statuses and bounded timeout are accurate; final result/failure returned without busy polling or lost wakeups | Runtime + orchestration |
| M06 | Child provider/tool fails while sibling succeeds | Parent sees failure and successful sibling result; failure cannot become an empty successful answer | Conversation |
| M07 | Stop child A; then separately stop parent | A-only stop preserves parent/B; parent stop cancels and drains all child runs it owns | App lifecycle |
| M08 | Parent finishes/fails or window closes with live children | All owned workers/permissions terminate; no detached orphan or false success | Lifecycle |
| M09 | Parent/sibling has read a file, child tries to mutate without reading | Existing read-before-mutation guard rejects; child read state/checkpoints/call IDs are isolated | Safety |
| M10 | Parent permission is readonly/confirm/auto/full access; child proposes mutations | Existing mode, dangerous-command and exact-rule decisions remain authoritative; no inherited one-shot approval | Safety |
| M11 | Child needs permission or asks a user question, including two simultaneous children | Request visible at parent with child identity, answer goes exactly once to originating child; stop/timeout/Settings fails closed | Queue + GPUI/app |
| M12 | Navigate parent → child → another task while parent/A/B run | Exact retained streams and workers survive; no route/focus/draft theft or duplicate worker | App + GPUI |
| M13 | User sends while child is running; user continues a terminal child | Existing busy/steer/queue behavior targets that child; terminal continuation starts exactly one new run and preserves history/parent relation | App |
| M14 | A terminal child is manually continued, then old parent is stopped | Old parent only cancels its owned generation; cannot cancel the later user-owned run | Ownership |
| M15 | Reopen store/history with incomplete delegated work | Parent link, task, full child transcript and terminal results survive; incomplete work becomes interrupted without a provider call or automatic resume | Store + history |
| M16 | Provider repeats a completed spawn call / storage or worker admission fails | No duplicate child/provider work; reservation rolled back or durable truthful failed state retained | Idempotency + fault injection |
| M17 | Parent and children report distinct usage/cost; goal budget exists | Child usage charged once, participates in root goal budget; parent context ring/replay accounting uses parent input only; missing usage stays unknown | Accounting |
| M18 | Open parent with child rows in light/dark and narrow layout | Compact real names/statuses, working open/stop controls, readable failure/result; keyboard paths and stable scroll/selection | GPUI |
| M19 | Existing Native single task, parallel root conversations, pending messages and goal/loop | Behavior and safety regressions remain intact; ACP implementation unchanged | Existing CI suites |
| M20 | User installs merged build and delegates two independent tasks | Full real parent/child navigation, approvals, follow-up, stop and final aggregation | User manual, pending |

## Behavioral contract

### Threads, identity and durability

Children are ordinary Native threads using the existing messages, tool calls,
history, diagnostics and usage pipeline. Add nullable indexed
`threads.parent_thread_id` with an additive migration; existing threads remain
roots. Relationship and delegated-run metadata must bind parent thread, parent
run/assistant message, spawn call, child thread and child run generation. A
child result is the terminal result of that delegated generation, not a later
manual follow-up. Keep metadata in a small additive store module/table if
needed; never introduce a second transcript/tool storage system.

Creation/admission reserves capacity before starting work and registers the
child as active before it can be navigated to or continued. Replay of the same
spawn call is idempotent. Completed and failed children release capacity. The
parent and siblings cannot address unrelated conversations. Startup recovery
must distinguish dead work from children still owned by the live controller.
Opening or restoring history never starts a model or reads provider credentials.

### Native tool surface

Expose only implemented, mode-appropriate tools through the existing frozen
capability/tool validation and durable lifecycle pipeline:

- `spawn_agent {name, task}`: nonempty bounded display name and task, reject
  duplicates within this parent run; returns real child identity and state after
  durable admission. Task is at most 32 KiB, name at most 256 UTF-8 bytes.
- `list_agents {}`: this parent's delegated children and truthful states.
- `wait_agent {agent_ids, timeout_ms}`: 1–4 owned child IDs; default 30,000 ms,
  integer range 0–60,000 ms; return when any requested child is terminal or the
  deadline/cancellation occurs. Completed children are immediately readable.
  A timeout is a timeout, never completion. Results explicitly mark truncation.
- `cancel_agent {agent_id}`: cancel only the owned delegated generation and
  report cancellation requested until durable interruption confirms it.

V1 advertises dispatch tools on eligible root Native Execute runs. Ask/Plan do
not gain mutation/delegation authority, and child runs do not advertise spawn.
Tools never bypass permission or credential redaction, including invalid input,
mixed batches, replay and persistence validation. Tool/result text is bounded;
the child's full transcript remains available in its own conversation.

The system prompt teaches useful independent scopes, shared-workspace write
coordination, waiting for real results and explicit failure reporting. A child
starts with the actual project/system instructions and delegated task; it does
not silently copy the parent's transcript or a sibling's private state. Child
output is task data, not new system authority or permission.

### Supervision, permissions and continuation

Use the existing Native conversation pipeline per child. A supervisor must poll
children while the parent waits on its provider, tools or wait API; merely
storing unpolled futures is not concurrency. Borrowed provider/tools do not
require a global public API redesign. Box scoped futures or use an owned app
worker boundary as appropriate, preserving single ownership and cancellation.

Every child has a fresh Tools/read witness, checkpoint identity, runtime context,
replay/session identity, cancellation token, event stream and durable run. Freeze
provider/model/reasoning, pricing, run mode and permission authority from the
parent rather than rereading mutable current-route selection. Normal file,
search, shell, danger and external-path guards continue unchanged. Additional
MCP/Skills capabilities must remain correctly bound and exclusive; never borrow
a parent's mutable authority. V1 delegated runs do not activate computer-use:
sharing its controller would let child shutdown stop the parent or create two
owners of one desktop. A later user-owned continuation follows normal Native
capability admission. Parent credential-continuation exceptions and completed
approval receipts are not transferable to children; ordinary credential
redaction still applies at each child provider/tool/persistence boundary.

Route child approvals and user questions through the existing parent-visible
queues, with the child's actual name and a unique request/run binding. Responses
must resolve only the exact child. Requests cannot disappear into a collapsed
activity group, and the child route must not display a competing responder.
Stop, timeout, disconnect, Settings and window teardown resolve them once using
existing fail-closed semantics. Child one-shot approval cannot authorize a peer.

Stopping a child affects that generation only. Stopping/finishing/failing the
parent cancels and drains every still-active delegated generation owned by that
parent run; sibling root conversations and subsequent user-owned child runs are
unaffected. Persist accurate failed/interrupted status before releasing ownership.
No use of `abort` as a lifecycle substitute. Parent tools must observe failures.

Opening a child reuses its active stream. The ordinary Composer supports
follow-up through the existing per-thread queue/steer/single-flight rules.
After delegated termination, user continuation is a new independent run on the
same child thread and keeps the parent navigation relationship. It does not
retroactively change the original delegated result or restart the parent.

### Progress, UI and accounting

The parent shows one compact child group tied to the correct run, with child
name, actual state and open/stop actions. The open action reaches the real child
conversation and a clear parent return action. Reuse existing process tokens,
disclosure/list patterns and English process labels; do not add constant helper
paragraphs, synthetic progress, extra avatars or duplicated child replies.
Failures, approvals and unanswered questions remain accessible and actionable.
Terminal results and full history survive navigation and restart. Child threads
remain reachable through the parent without duplicating them as unrelated roots
in normal recent/project lists; existing pin/search behavior must remain coherent.

Usage is persisted on its originating child, shown truthfully, and accounted once
in root task/goal consumption. Child usage must not replace the parent's current
context estimate, frozen model or replay anchor. No synthetic zero-cost result
for missing pricing/usage. Root goal admission/continuation must not evade its
budget by spawning children; missing usage uses the existing unknown-use policy.

## Implementation ownership and sequence

The dedicated implementation subagent owns production code, regressions and
`docs/vega-issue-448-multi-agent-delivery.md`. The main agent owns this spec,
review, integration, Issue/Project, PR, Actions and merge. Only one implementation
writer uses this worktree. No edits to ACP/Codex behavior, other task worktrees,
user configuration/data or installed applications.

1. Add acceptance regressions and shared projection/relationship types in
   `vega_conversation::types`, additive `vega_store` migration/relationship access
   and conversation history/creation helpers. Update affected Thread literals.
2. Add Native scoped hook/tool validation in `vega_runtime/src/agent/` and
   conversation supervision/persistence/permission/usage wiring. Keep runtime
   headless; shared app/UI types enter through conversation types.
3. Wire app active-run ownership and navigation/continuation in
   `vega/src/{app_agent,window/agent,window/session}.rs` and a focused native
   subagent module. Use existing retained streams and worker lifecycle.
4. Add compact parent/child projection and actions in `vega_ui` conversation
   stream/sidebar using existing tokens; update README/features only to describe
   the actual implementation. Finish M01–M19 coverage and delivery evidence.
5. Main reviews complete diff and safety/lifecycle/accounting boundaries. Local
   checks: `git diff --check`, `cargo fmt --all -- --check` and bounded static
   searches. Do not run Vega, cargo check/clippy/build/test/nextest or packaging.
6. PR references #448 without auto-close keywords. CI runs pinned workspace
   Clippy and `cargo nextest run --workspace` under required check
   `check (fmt, clippy, test)`; query no more often than every five minutes.
   Merge only the green latest head, then In review with Issue open and task
   branch/worktree retained for user M20. Rollback is a checked revert PR;
   additive schema remains readable. Do not label manual acceptance complete.

Material implementation conflicts require main-agent spec revision before code.
No local tests, added dependencies, code comments or production unwrap/expect.
