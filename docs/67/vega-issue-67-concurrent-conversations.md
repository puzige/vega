# Issue 67 · Concurrent conversations

Freeze: 2026-09-21 (Asia/Shanghai). Source: maintainer's explicit second-stage
request on Issue #67 after PR #102. This specification supersedes R5 and
I67-06 of `vega-issue-67-background-run.md`; its navigation guarantees remain.
Baseline: `fcafcef8af3d38b03d87e82942cf03fc3aa88d90`.

## Scenario and contract

A is streaming. The user creates B and sends immediately. Both workers must
enter the provider boundary and remain active simultaneously. Waiting for A,
changing only the rejection text, or silently queueing B is not an implementation.

C1. The window owns active runs keyed by durable thread ID. Admission is
single-flight per thread, including preparation and terminal handshake. A run
owns its exact stream, unique generation, cancellation token, permission queue,
frozen provider/model/reasoning/pricing/tool authority, pending submission,
clock, terminal state and durable writeback. No arbitrary concurrency cap.
Duplicate same-thread submission must preserve run identity and draft and use
precise busy semantics; it must not report a provider failure.

C2. Navigation and Settings never cancel active runs. Returning to A or B before
terminal reuses the exact stream. Events and callbacks validate thread,
generation and stream; stale completions cannot remove a replacement run.
Preparation/spawn failure removes only its own reservation. Each run drains its
own receiver and persists only its own thread. Background completion cannot
change route, selected project, draft, focus or Settings visibility.

C3. Stop targets the current exact thread/run only, including permission cleanup
and pending review. Closing the window cancels every active token and permits
each worker's durable interrupted handshake. Settings fails closed for hidden
permission prompts across all active streams, never auto-approves them.

C4. Automatic context compaction ownership and operation IDs belong to each
run, survive navigation, and project to its retained stream. Usage/accounting
must use the frozen originating model. Navigation invalidation of route loads
must not invalidate a background run. Pending plan reviews are per thread;
review cancellation, persistence and approved continuation cannot affect another
run or depend on a different current route.

C5. Artifact generation observations/proposals must be fenced per run and
retained across navigation; route-local artifact views may stay route-local.
Background event handling must not overwrite the current artifact identity.
Model selection is blocked only by the target thread's run (plus existing
relevant configuration/action guards); selecting B's model cannot change A's
frozen model. Branch/commit mutations remain subject to repository safety:
shared-repository mutations may conservatively stay disabled while any run is
active; this gate must never prevent an ordinary B send solely because A runs.

C6. Manual context operations and trusted repository preparations may retain
existing exclusive action leases and route-bound cancellation where needed.
These auxiliary gates must be documented and scoped explicitly: a primary run
on A is not a reason to reject B's send or B's manual context operation. If a
shared auxiliary action itself owns a lease, reject conflicting actions locally
with existing typed action-busy/unavailable state, never cancel another run.
Audit all controller single-owner assumptions, including load/save callbacks,
manual context, artifact proposals, pending review, model selection, branch and
commit gates, preparation rollback, Settings and Drop.

## Acceptance matrix (written before implementation)

All entries start NOT RUN. Primary evidence uses mounted production VegaWindow,
real controllers/workers and an owned file-backed store; only provider/network
may be gated/mocked. Controller-only assertions cannot prove B admission.

| ID | Risk / precondition | Operation | Observable result | Evidence |
|---|---|---|---|---|
| C67-01 | A gated in provider; B newly created | Submit B through production path | B enters worker/provider before A releases; both active; first run on baseline fails here | E2E-REAL, red then green log |
| C67-02 | A/B active with distinct streams | Interleave text/tool/terminal events | Correct streams and durable thread messages/tools/usage/title; no cross-thread writes | E2E-REAL |
| C67-03 | A/B active | Switch A/B, open/close Settings | Exact entity reused; neither token cancelled by routing; hidden permissions fail closed independently | Mounted production root + permission regression |
| C67-04 | B current, draft/focus set | Finish A in background | B route/stream/draft/focus unchanged; A durable success; reopening A restores result | E2E-REAL |
| C67-05 | A/B active | Stop B; symmetric Stop A | Only target token cancelled and interrupted; peer can finish successfully | E2E-REAL |
| C67-06 | A/B active | Drop window | All tokens cancelled; both durable interrupted handshakes complete | Production lifecycle |
| C67-07 | Same thread already running | Submit duplicate | No second worker; identity/draft intact; precise busy state | Production ingress |
| C67-08 | A/B context events and artifact proposals | Interleave, navigate, then stale generation callback | Each run retains correct ownership; current projection not overwritten; stale events rejected | Production ingress + context/artifact regressions |
| C67-09 | A pending review, B running | Review/continue A or Stop A | Only A cancelled/reviewed/restarted; B untouched; no route steal | Plan/agent regression |
| C67-10 | A active, B idle | Select B model; inspect branch/commit gates | B model selectable, A frozen; unsafe repository mutation remains guarded | Session + branch/commit regressions |
| C67-11 | A active; B preparation/spawn fails | Complete failure then retry B | Only B owner removed; A continues; retry admits one B | Failure injection |
| C67-12 | Installed merged build | Manually send A/B, switch, Stop B, Settings | Real concurrent results and isolation | User manual verification pending; Issue remains Open / In review |

## Implementation and delivery plan

1. Commit this spec/matrix before any test or implementation code. Delegate code
   ownership to one dedicated implementation subagent in the issue worktree.
2. Add C67-01 production-root regression first, run it on the baseline code,
   preserve the failing command/output, then implement C1–C6 and remaining cases.
3. Main agent reviews all ownership boundaries and evidence, runs affected
   regressions and gates, and records results in a companion delivery report.
4. Run `cargo fmt --all -- --check`; through `scripts/cargo-lock.sh`, run clippy
   on affected crates with `--all-targets -- -D warnings`, affected Agent/Stop/
   Settings/permission/context/artifact/session/branch/commit suites and package.
   The maintainer explicitly overrides workspace-wide tests: do not default to
   `cargo test --workspace`. Preserve initial failures and classify residuals.
5. Use gh for Issue/Project/PR. Squash merge to master after relevant gates,
   force rebuild xtask on merged master, package, replace /Applications/Vega.app
   with a recoverable backup and verify binary identity/startup. Keep Issue Open
   and Project In review until the user completes C67-12. No automatic close text.

No new dependencies, schema, provider protocol, public API, permission policy,
notifications or general stream cache. Allowed implementation scope is vega's
agent/window controllers and directly affected UI busy projection/tests only.
Any required contract change must be documented before its implementation.
Rollback uses the preceding installed app backup or a revert of the squash.
