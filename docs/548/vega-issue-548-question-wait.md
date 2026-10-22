# AskUserQuestion 等待生命周期修复

Issue: [#548](https://github.com/puzige/vega/issues/548)

Version: 1.2 (pre-freeze test review: preserve acknowledged submission and complete existing W1/W4 coverage). Original analysis baseline: `83fd51d79940516029cdc1eaf8e81566870c877b`. Recovered test-first baseline: `871f91f246e90753743a256cee1eec315e74effc`; current test-first base: `6615748f91273bfb8ee424011b3b5a09b3777d2b` (includes upstream #546 Composer capsules and #547 image-budget fixes; acceptance unchanged).
Branch: `feat/548-question-wait`. Stage: specification / test-first; no implementation or runtime validation yet.

## 1. Goal and evidence

User authorized creating the card and directly fixing missing attention and working-time suspension for Native `ask_user_question`.

Static baseline evidence:

- `vega_runtime/src/agent/tools_exec.rs` awaits the question hook; `vega_conversation/src/agent/user_question_queue.rs` resolves only from a response, cancellation or unavailable interface. This is already an execution barrier, not evidence that the agent continues past an unanswered question.
- `vega_ui/src/conversation_stream/user_questions.rs::install_pending_user_question` sets only activity-group waiting presentation. No question attention reaches `vega/src/app_notifications.rs`.
- `run_activity.rs::label` uses the starting Instant's elapsed time, while `agent/events.rs::terminal_execution_duration` and fallback paths in `agent/entry.rs` independently persist wall elapsed time. All include time awaiting user answers.
- Application notifications currently originate only from `window/notifications.rs::finish_completion_attention`.

The change joins question waiting, visible state, attention and work-time accounting. It must not confuse suspended with completed/failed. Other independent runs remain unaffected.

## 2. Frozen acceptance matrix (before implementation)

All automated checks run in existing GitHub Actions. Local execution/build/test is prohibited. New Rust tests are retained through GREEN, preserved in PR evidence, then removed in a separate delivery commit.

| ID | Setup / action | Observable result and required evidence |
|---|---|---|
| W1 | Native run reaches a valid question; no answer beyond an ordinary permission timeout | Hook remains pending, no subsequent provider request or follow-on tool action, Stop remains possible. Preserve the existing indefinite-wait and solitary-tool-batch contract. Runtime/conversation integration with synthetic provider and real question queue. |
| W2 | Run works, waits, then receives an answer or explicit skip and continues | Work time freezes during waiting, resumes only after resolution, and excludes the whole wait in terminal conversation events and stored message duration. Both answer and skip return exactly one matching result. Test real queue/hook through production timing and terminal persistence, not a mirrored clock algorithm. |
| W3 | Stop/cancel during pending question, including response/cancellation race and listener removal | Wait is released once, pending UI/notification is cleared, no default answer/success fabricated, terminal elapsed excludes the pending interval. Repeated cleanup is harmless. |
| W4 | Two sequential questions, duplicate state delivery, late cleanup from the previous question | Accumulate non-overlapping wait intervals once; stale resolution cannot resume the new wait or remove its notification. A later run starts with fresh timing; independent runs do not share paused state. |
| W5 | Invalid question or unavailable question interface | Recoverable error retains existing semantics; no stuck waiting state, pending notification or permanently paused clock. Invalid response remains pending/editable and does not resume time. |
| U1 | Valid question arrives in active task; navigate away/back while unanswered | Existing question panel and Stop remain accessible; activity reads `Waiting for your answer`, not working/confirmation elapsed. Work duration remains frozen; navigation neither answers nor cancels nor resets accounting. No focus steal or redundant explanatory banner. |
| N1 | Question arrives while another task/application/settings is foreground | Exactly one notification for that request, with task identity and short needs-answer body, using existing system notification API. Click routes to the correct live task/question; no forced activation before click. Keep completion notifications intact. |
| N2 | Same task is foreground; re-render/repeated event; answer/skip/cancel | No redundant system notification in foreground and no repeat on repaint/navigation. Resolve/cancel dismisses outstanding question notification; stale request cleanup does not affect later questions or ordinary completion notifications. |
| N3 | System notifications denied/unavailable | Question remains answerable and execution stays blocked; no permissions changed, fallback auto-answer, crash or erroneous success. Pending UI is authoritative even if OS does not deliver a banner. |

Notification delivery and native focus/OS permissions require user manual acceptance. Automated GPUI platform tests observe production notification decisions, shown/delivered/dismissed notifications and navigation routing. Existing test platform exposes `shown_system_notifications`, `delivered_system_notifications`, `dismissed_system_notifications`, `simulate_system_notification_response` in `TestAppContext`; use it instead of real OS automation.

## 3. Scope / compatibility

- Native questions only; do not change ACP/Codex behavior, permission grants/timeouts, provider billing, shell waiting (#412), automation budget policy, restart recovery, question pagination or question schema.
- `execution_duration_ms` for newly executed Native runs becomes active working time excluding question waits. Existing stored durations stay unchanged; no migration or retrospective guesswork.
- Wall-clock diagnostics/tool latency/billing may remain wall-clock and must not be relabeled as work time. No separate always-visible wall-time counter is required.
- Keep task-level waiting distinct from global pause. Do not terminate independent children or other tasks when a question waits; delegated questions use their actual task identity and normal live stream ownership.
- Retain existing permission waiting behavior and wording; user questions are not permission confirmation. Removing a question must not accidentally clear another reason to wait.
- Preserve [#445's acknowledged-submission contract](../445/vega-issue-445-ask-user-question.md): an accepted answer/skip ends user waiting and dismisses matching attention, but submitted panel values remain disabled until the acknowledged terminal event replaces them with history. Local channel success is not durable completion. Cancellation/drop may clear abandoned pending UI; keep these outcomes distinct.
- No new dependencies, unsafe code, code comments, non-test unwrap/expect, configuration/settings surface, or database tables. Test-only GPUI support may be enabled from the existing dependency and removed after GREEN.

## 4. Implementation plan / ownership

Only the dedicated implementation agent writes production/tests. Parent owns spec, Git/index/commits, PR/CI, review and integration. First phase writes tests only; production changes wait for independently reviewed, valid cloud RED. Minimal missing production API is permitted as interface-missing RED, explicitly not a behavioral assertion failure; do not implement it in the test phase.

1. Conversation timing: introduce a per-run shared active-work clock in `vega_conversation::types` (or equivalent existing production timing boundary), wrap the configured question hook for scoped pause/resume, and pass the same clock to live UI and all terminal duration/fallback persistence paths. Keep timing independent of panel paint/navigation. Cancellation, unavailable hooks and dropped futures release scoped waiting safely. Retain wall-time diagnostic paths intentionally.
2. Live projection: update `types/events.rs` / `agent/entry.rs` / `agent/events.rs` and necessary exhaustive consumers to carry the shared work-time source with RunStarted; `conversation_stream/content.rs` / `run_activity.rs` use it rather than a separate wall elapsed clock. Existing non-Native callers keep compatible defaults.
3. Question state: `user_question_queue.rs` must notify resolution as well as enqueue if needed for prompt cleanup. `conversation_stream/user_questions.rs` / `core.rs` emit an exact question lifecycle signal, distinguish question vs permission waiting and retain panel/Stop semantics. Include call/request identity so stale updates are ignored.
4. Attention: install an idempotent non-render-dependent subscription at stream run ownership/admission, including never-displayed child streams in `window/native_subagents.rs::apply_subagent_update`; window-dependent bindings may remain in `bind_conversation_stream`. Handle in `window/notifications.rs`, reuse foreground test and click navigation, add separately tagged needs-answer notifications in `app_notifications.rs`. Deduplicate by stable run owner/request instance and dismiss only matching resolution/terminal cleanup. Do not suppress legitimate delegated task questions just because completion notifications are suppressed for children.
5. Keep documentation and temporary tests scoped; do not refactor unrelated navigation, automation, panel layout or runtime tools. Any actual API conflict or acceptance gap must be reported before changing the frozen contract.

### Lifecycle clarifications from independent baseline review

These clarify existing W/U/N cases before test freeze; no production changes existed at review.

- A delegated question survives Settings as well as task navigation. Remove only the navigation-triggered `cancel_user_questions` in `core.rs`'s Settings observer; permission policy remains unchanged. Do not resubscribe the queue during navigation, because subscribe replaces/cancels its active owner.
- Queue watch updates are reconciliation wake-ups, not a lossless event log. Reconcile authoritative active/resolved state even when no new pending request exists. Resolution must not acquire the queue mutex recursively: several existing callers already hold it. Avoid deadlock and preserve invalid-response waiting.
- Stable identity consists of run owner/generation plus request instance, not assistant message ID or call ID alone. Steering changes assistant message IDs inside the same run, so retains its clock. New runs get fresh clocks. Foreground-suppressed requests are considered handled, preventing delayed notification on navigation/rebind. W4 verifies a later real run's fresh timing on the same formerly waiting owner, and a reused call ID with a fresh run generation at the app/queue attention boundary. The real pipeline intentionally replays already-persisted matching call IDs; keep that contract and use a new call ID for a new real-pipeline question. Never remove durable history or alter replay policy merely to make a test wait.
- First-wins race semantics remain: cancellation cannot fabricate an answer, but an already accepted answer is allowed to have won before cancellation. Scoped timing owns synchronous request publication and cleanup when the returned future is dropped, even if never polled; stale guards cannot resume newer waits.
- `RunStarted.started_at` stays the original wall-clock origin for automation/diagnostics. Add a compatible Native work-time source rather than change that field's meaning. Start it at runtime start, not during preparation; normal terminal and entry fallback persistence use it, Codex keeps prior behavior.
- A notification click already accepted before resolution may navigate to its task; no automatic answer or focus steal before a click is allowed. After dismissal there must be no routable stale notification tag. Do not redefine ordinary completion click behavior.

### Feasible observation route

- Existing runtime Provider trait and `ConversationRun` entry functions plus `PersistenceActorConfig::with_user_questions` support synthetic providers and the real queue through persistence; use temporary test storage only, never user data or real external processes.
- Queue trait returns a future synchronously and a listener drives existing GPUI projection. Test invalid input/response and cancellation at these real public boundaries.
- GPUI Kit 0.6.1 reexports `#[gpui_kit::test]` with test-support; avoid glob imports shadowing ordinary Rust `#[test]`. TestAppContext notification observations exist in locked gpui-pre 0.3.4. Inspect current APIs for fixture setup; do not restore old tests wholesale.
- When deterministic timing needs an explicit time boundary, test a production-used clock boundary plus real integration wiring; helper-only arithmetic assertions are insufficient to prove saved/live behavior. GPUI/Tokio virtual time does not advance baseline std::time::Instant. Avoid long real sleeps and timing-flaky exact duration comparisons.
- Use a temporary file-backed store for actual PersistenceActor and navigation paths (in-memory-only fixtures do not exercise these contracts). VegaWindow construction starts pricing/updater services; isolate that unrelated startup work with temporary test support rather than run real network/process services.
- Assert shown-system-notification count for deduplication: delivered notifications replace equal tags and can hide duplicate show calls. The locked platform has no denial switch; N3 OS-denied delivery stays explicit manual acceptance, not fabricated simulator coverage.
- Mixed-question-batch isolation must observe both calls' batch-rejected results and absence of follow-on admission/running, not merely an absent output file behind a rejecting permission hook. Permission rejection can otherwise mask a broken solitary-question barrier.

## 5. Validation / delivery

Local allowed: focused static source inspection, `git diff --check`, `cargo fmt --all -- --check`, lightweight manifest/lock inspection. No local Vega execution, compilation, tests, Clippy or packaging.

Cloud: existing draft PR `pr-check` (fmt, workspace all-target Clippy, Python checks and Nextest); poll at most once every five minutes unless notified. Freeze tests after valid RED; do not weaken assertions, add post-hoc tests, ignore failures, or retry to green. Original source, full SHAs, patch and RED/GREEN run/job evidence must remain accessible from PR after squash.

Independent reviewer must review tests before implementation and exact stopped implementation/final versions. After same-tests GREEN: preserve evidence, separate Rust test/support removal commit, verify final latest-head full gate and explicit `no Rust tests` (0 discovered/0 executed). Then ready PR and squash merge, move #548 to In review and leave open for user.

Manual acceptance: trigger a Native question, switch to another task/app, check a single needs-answer notification and click navigation; wait then answer/skip and verify working time did not count the wait. Repeat Stop, back-to-back questions and notifications disabled. Check normal completion notifications still work. Do not install/restart the user's active application as part of this fix.

Rollback: revert the task PR; no schema migration or user-data transformation.

## 6. Interrupted-draft recovery

The preceding execution ended before a test-only READY report, PR or cloud RED. On continuation, no child executions remained owned by the run. The existing draft was preserved as Git stash checkpoint `18939e217c04d9cb961d8b5a1a2eed952ef9c955`, the task branch was fast-forwarded to fetched `871f91f246e90753743a256cee1eec315e74effc`, and the complete draft was reapplied without conflict. The checkpoint is local recovery support, not final delivery evidence.

The upstream change is the Markdown quote repair and its vendored dependency; question, notification and work-duration production paths are unchanged. No acceptance criteria change. A replacement sole writer continues the existing tests and temporary support, with production implementation still prohibited until independently reviewed cloud RED. The parent retains documentation and Git/index ownership. Initial recovered test hashes:

- `crates/vega_conversation/tests/question_wait_lifecycle.rs`: SHA-256 `0813ea70602d94964adc16415f535c433fc273ccceb429c8f1c8eef59d47b35b`.
- `crates/vega/src/window/question_wait_tests.rs`: SHA-256 `44dc6c545c351a68421569b252a59ebb579b3b4fffa61a9f333f509da011db2f`.

These identify an unfinished draft, not frozen or passed tests. Required next steps remain test readiness/review, existing Actions RED, implementation and same-tests GREEN, evidence preservation, independent test deletion, final empty-suite gate and merge.
