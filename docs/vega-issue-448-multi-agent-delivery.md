# Issue 448 · Native subagent implementation handoff

Status: latest local correction cut; local format/static checks only. CI8 passed format/Clippy and ran 3139 tests with two fixture failures, as recorded below. The current checkpoint-layout and history-observation fixture corrections await latest-head cloud validation.
Scope: frozen [specification v1.1](vega-issue-448-multi-agent.md), including parent deletion, retained standalone workspace and metadata privacy cleanup. Native only; no ACP behavior or capability changes.

## Implemented behavior

Eligible Native Execute root runs expose `spawn_agent`, `list_agents`, `wait_agent` and `cancel_agent`. A scoped supervisor polls the parent and independent child conversation pipelines together. Capacity is reserved before creation; admission persists the thread/task/relationship and awaits registration of the real App active run and stream before returning a spawn receipt. Parent termination cancels and drains the generations it owns.

Each child uses the frozen model/provider/reasoning/pricing/permission policy, an isolated Tools/read witness/checkpoint identity, fresh request queues, and fresh history/replay state. The launch snapshot retains only the child settings it needs; it does not retain the parent's completed approval receipts, credential exception, replay registry or computer-use owner. Child provider input is additionally checked without the parent's credential exception. Children cannot dispatch another level or activate computer-use.

The parent shows a run-owned group with actual names/states, Open/Stop, expandable bounded results, and the original child permission/question responders. Opening a running child reuses its registered stream and normal Composer queue behavior. After the initial generation terminates, the group releases its live request bindings; a later user run keeps the conversation relationship while using independent run ownership.

The additive migration stores the parent link, original delegated generation/result and usage attribution. Cold parent history recovery interrupts incomplete dead children while preserving live workers. History groups load only with their owning parent message/page. Deleting a parent detaches its children and preserves their transcripts. Standalone children retain their original scratch workspace identity after deletion of the parent; no workspace files are moved or deleted.

Reviewing either parent or child includes its delegation name, task and result copies. Explicit cleanup clears them within the existing transaction and current thread's exception policy, while full transcripts keep their existing per-thread scope. Text or lifecycle changes invalidate the preview; running/cancelling delegations reject cleanup before the redactor runs. UTF-8 bounds, nonempty name/task values, name uniqueness and truthful result truncation survive redaction. Only this explicit privacy cleanup can revise an original delegated result.

## Accounting scopes

| Display/account | Actual scope |
|---|---|
| Child transcript/counter | Its own original provider-call rows/events; subsequent user runs remain on that child |
| Parent live Composer cumulative counter | Parent events plus original delegated-generation usage events; the child event is counted through the cumulative branch only |
| Parent restored cumulative counter | Own thread rows plus rows attributed by `delegated_parent_thread_id` |
| Parent per-run TaskCostSummary | Own assistant-message rows plus rows attributed by both parent thread and parent assistant message; later child user runs are excluded |
| Per-run tool count/duration | Existing parent tool audit count and parent wall time; child tools are available in their own conversation |
| Parent context ring/current round/replay | Parent's own context accounting, input estimate and replay identity; child usage does not replace them |
| Root goal budget | Original token row IDs charged once to the root claim, including child and compaction usage; parallel pending provider calls are counted independently |

No token rows are copied. Missing pricing remains unavailable. Delegated primary usage completeness and each attributed row's original cache-presence fact are persisted separately from the numeric cache columns; an absent cache value cannot become a fabricated zero in the parent summary. Missing child usage makes root goal usage incomplete and prevents budget continuation through the existing unknown-use policy. Parent-run and cumulative totals exclude later user-owned child generations from the old parent.

## Written regression matrix

At the initial implementation review cut, the tests below had not run. CI4, CI6, CI7 and CI8 subsequently executed earlier heads; their actual results and failures are recorded below. CI8 passed 3137 tests, including the delegated-start and receipt-reader regressions and seven of CI7's nine failing main-chain cases. The two remaining permission/continuation fixtures have been corrected and strengthened locally but have not run at this cut. The tests use in-process providers/queues and owned temporary storage, with no real network or installed-App validation. Names are searchable in the corresponding modules.

| IDs | New assertions / file |
|---|---|
| M01/M02/M05/M17 | `agent/tests/subagents.rs::m01_m02_m05_m17_parallel_children_enter_before_release_and_persist_separately`: both gated children enter `chat_stream` before release, root reaches wait, distinct child transcripts/events, originating usage rows, combined cost/goal budget, original result preserved after un-attributed child follow-up usage |
| M03/M04/M05 | `agent/subagents.rs::tests::m03_m04_m05_capacity_names_scope_wait_timeout_and_wakeup`: four reservations, fifth rejection, completed-slot reuse, duplicate names, foreign wait rejection, honest zero-timeout, notification wakeup and cancellation; store tests bind names/capacity to parent generation |
| M03/M19 | `vega_runtime/agent/subagents.rs::tests::m03_m19_delegation_changes_replay_capability_only_for_execute`: tool surface changes the replay capability digest; Ask/Plan do not gain it |
| M04 | Runtime strict/bounded input tests, escaped-secret preparation test, and conversation `m04_escaped_owner_secret_never_creates_child_metadata_or_provider_work`: decoded owner secret is rejected, no child/provider work, input audit is `{}` |
| M04/M19 codec | Conversation `m04_m19_subagent_proposals_and_approvals_accept_only_exact_authority_and_valid_input`: all four valid tool inputs are proposed/approved; malformed inputs, empty spawn/wait/cancel approvals, extra fields, non-Readonly/Always/note/danger/remember grants and unknown tools remain rejected |
| M05/M16 | Conversation `m05_m16_delegated_preparation_reuses_only_its_exact_initial_instruction`: a real scripted Native provider consumes a precreated task once in Running/Cancelling state, including after the parent assistant becomes done; missing/foreign/changed user, task, parent, generation, detached/terminal identity, absent persisted-user admission and credential continuation are rejected without provider work or durable source changes |
| M06/M15 | Conversation `m06_m15_child_failure_and_sibling_result_are_durable`: one provider failure, sibling success, durable nonempty failed result after reopen, missing provider usage remains unknown |
| M06/M15 | Supervisor `m06_m15_failed_and_interrupted_terminals_commit_with_recovery_or_rollback_together`: both Failed/Interrupted results, usage completeness and transcript/tool cleanup commit durably; injected cleanup failure rolls all of them back |
| M07/M08 | Conversation `m07_m08_stop_one_child_then_parent_while_waiting_drains_every_owned_generation`: child-only stop preserves sibling, parent Stop while inside wait drains owned work, no streaming rows remain |
| M07/M08/M16 | Conversation `m07_m08_m16_stop_while_admission_ack_is_pending_never_starts_provider_or_reports_spawn_success`: cancellation while host admission is pending returns interruption, no successful spawn or child provider call, durable terminal metadata |
| M08/M16 | Conversation `m08_m16_natural_parent_end_and_rejected_admission_do_not_leave_live_work`: natural parent completion and rejected admission leave two terminal children; child admission rejection produces two scoped failures with no child provider calls, while the uncancelled parent continues and completes |
| M08 | Conversation `m08_parent_provider_failure_after_both_children_enter_cancels_and_drains_durably`: both children are waiting inside their provider before the parent failure gate releases; the failed parent cancels/drains both, persists interruption and leaves no live/streaming rows |
| M09/M10 | Conversation `m09_m10_child_read_witness_and_permission_modes_remain_independent`: external checkpoint storage, all four permission modes × Write/Edit, two real child terminal events matched to durable owner/status/output, valid Write proposals matched to stored audits, both target files checked, and Edit rejected specifically for FileNotRead despite the parent read |
| M11 | GPUI `m11_two_child_permissions_share_the_original_responder_and_stop_is_scoped`, `m11_m14_two_child_questions_route_once_and_terminal_group_unbinds_later_requests`, `m11_settings_fails_child_permission_and_question_closed`: two simultaneous requests, originating queue response, peer remains pending, Settings/timeout fails closed |
| M02/M07/M12/M13/M14 | App `tests/subagents.rs::m02_m07_m12_m13_m14_native_child_admission_navigation_queue_and_generation_ownership`: root MessageStarted and Admit in one batch, child single flight, real event/stream routing, parent→child→other→child, draft retained, busy queue uses child generation, old Stop cannot affect later controller generation, window cleanup cancels it |
| M07 routing | App `m07_child_route_composer_stop_preserves_parent_run`: the current child Composer stops its own run while its parent stays active; window cleanup cancels the remaining parent. The navigation test also rejects a parent-group Stop from the child route before navigating to the parent and applying that Stop |
| M14 | GPUI terminal group releases the child stream before a later user question, so that later request is not rendered by the old parent |
| M13/M14/M17 | Conversation `m13_m14_m17_terminal_child_normal_native_followup_preserves_history_and_original_ownership`: the original provider text and stored assistant/result are exactly Child result once; a terminal child runs a real scripted follow-up through the normal Native entry, receives that history once in the strict authority-none cold-prefix observation, starts exactly one new run, retains its relationship/result/message, excludes new usage from the old parent and ignores the old parent's cancellation token. Combined with the App queue/controller test above, this covers continuation across both boundaries |
| M15/M18/M19 | Conversation `subagents.rs` tests recover dead children on parent reopen, skip live children, attach separate historical runs only to their loaded owner pages, preserve child history/root reachability on parent deletion |
| M19 deletion | Conversation `m19_running_root_without_children_retains_existing_delete_semantics` permits the existing ordinary-root delete path; child/parent-with-live-child guards remain asserted. The existing App automatic-worker archive/delete regression is unchanged |
| M15 | `m15_standalone_child_inherits_parent_actual_scratch_root`: parent/child share the real scratch root; deleting parent and reopening child retains that root and a synthetic file |
| M16 | Conversation `m16_repeated_completed_spawn_is_reused_and_creation_failure_rolls_back_reservation`: repeated call ID reuses its durable tool result and performs one child run; injected delegation insert failure rolls back child creation |
| M17 | Conversation `m17_child_missing_cache_is_unknown_in_parent_run_summary`; automation `m17_parallel_provider_usage_and_delegated_budget_are_counted_once`; GPUI `m17_child_live_usage_changes_parent_total_without_changing_parent_context_or_round_estimate` |
| M18 | GPUI `m18_group_result_keyboard_controls_and_layout_follow_real_height`: result keyboard toggle, Open/Stop events, dynamic list measurement, result/peer geometry in light/dark themes and wide/narrow windows |
| M19 | Existing Native, parallel roots, pending-message, goal/loop, permission, replay and UI suites remain enabled for the workspace CI run. Mechanical Thread/HistoryPage/StreamEntry updates retain existing assertions |
| M19 fixture compatibility | `m19_synthetic_receipt_reader_keeps_json_with_runtime_advisory_and_rejects_other_tails`: scripted provider fixtures accept the first JSON object only with an empty tail or the master's runtime-notice tail; unrelated trailing text is rejected and the existing two-child receipt assertions remain intact |
| M19 privacy | Store `credential_privacy.rs::m19_*`: parent/child coverage of all three copies, read-before-clean immutability, stale metadata and exception-CAS rejection, pre-message active-run fence, transcript/metadata rollback on injected failure, identity/state/other-thread preservation, expanded/empty replacement bounds, deterministic collision names and child-only collision preserving a sibling. Conversation privacy test covers real review labels/findings and current-thread exception isolation through cleanup |
| M19 result safety | Runtime `m19_redacted_results_fail_closed_only_for_unsafe_subagent_terminals` covers JSON-breaking redaction, non-object/oversize output, fixed credential-reader failure and unchanged unrelated/Cancelled/Rejected/valid results. Conversation `m19_hook_completion_reader_failure_persists_a_recoverable_failed_terminal` drives a real runtime hook that makes its synthetic reader fail on return, persists the actual lifecycle through the existing conversation validator and reloads its terminal audit |

### Explicit remaining coverage limits

- M08 has separate conversation cancel/drain tests (including gated parent provider failure) and App window-cleanup cancellation assertions. There is no new single App/provider test that destroys the window while admission acknowledgement is pending and then inspects drained durable child rows.
- M13/M14 continuation coverage combines the App queue/controller/generation test and a real scripted provider follow-up through the normal conversation entry. It does not drive both stages through a single App Composer fixture.
- M09/M10 add delegated read-witness and permission-mode coverage. Dangerous command, exact-rule, external-path and completed one-shot approval behavior rely on the existing permission/tool safety suites; the new file does not duplicate that full matrix for every child.
- M18 adds geometric/interaction assertions, not rendered screenshot evidence. Selection/scroll regressions are retained in the existing suites. Pin/search reachability is statically preserved by filtering only default thread lists; no new dedicated child pin/search UI test was added.
- Latest local fixture corrections and M20 real user acceptance remain pending. CI8 left the two permission/continuation fixture failures recorded below; this matrix is not a latest-head pass claim.

## Review feedback mapping

| Review item | Implementation |
|---|---|
| Replay capability digest must match advertised tools | `RuntimeToolConfig::replay_capability_digest` and runtime loop both apply `with_subagents` |
| Parent reopen must recover dead children without interrupting live ones | Database-scoped/refcounted live-run guard plus parent child recovery |
| Failed final must survive recovery | Exact child-message CAS finish, usage completeness and transcript recovery in one transaction; failure before recovery. `recover_thread_in_transaction` reuses that transaction instead of issuing nested BEGIN; direct Failed/Interrupted commit and injected rollback assertions cover the review fix |
| Child PK must not be rewritten after insertion | Create a draft with its final child ID before materialization |
| Delete parent preserves child history and workspace | `ON DELETE SET NULL`, active-child guard and persisted standalone workspace owner |
| MessageStarted and Admit may share one poll batch | Root events applied first; admission checks the root's recorded first assistant message |
| Variable group height invalidates virtual item | Parent observes the group; group observes active child request streams |
| Pagination must preserve owner order | History projection filters delegations to assistant messages present on that page |
| Later child prompts must not leak to original parent | Terminal snapshot releases stream/observer bindings and delegated request state |
| Escaped owner secret must be checked after decoding | Canonical JSON recheck before runtime preparation; decoded name/task recheck without ancestor exceptions before creation |
| Wait cancellation must be a durable cancellation | Dedicated stable cancellation output/status accepted by recovery validation; failed output prefix remains tool-specific |
| Wait returns any terminal, not all | Provider fixtures continue waiting for remaining IDs; independent-stop fixture keeps B gated until explicit parent Stop |
| M17 is per-run as well as cumulative | Origin token-row attribution by root message, pending provider count, original cache-presence flags and separate cumulative-only UI projection |
| New controls need names/expanded state/stable focus geometry | Child name in accessible Open/Stop labels; result chevron/aria-expanded; reserved focus border |
| Metadata copies must join existing privacy cleanup | Parent-or-child fields, identity/lifecycle fingerprint, active-run fence and bounded collision-safe cleanup share the original immediate transaction; source navigation remains `None` for metadata and current-thread exceptions remain isolated |

## Local evidence and CI handoff

Local commands performed on this worktree, without compiling or running Vega:

```text
cargo fmt --all
exit 0; no output

cargo fmt --all -- --check
exit 0; no output

git diff --check
exit 0; no output
```

Static inspection checked module/enum/field wiring, active-generation fences, bounded/redacted tool input/results, originating usage attribution and absence of production unwrap/expect or newly added production comments. No dependencies or runtime configuration were changed. `codex_agent/tests.rs` contains only the mechanical `Thread.parent_thread_id = None` fixture field; `codex_agent.rs` and `vega_acp` are untouched.

Required cloud validation remains:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace
```

Use the existing PR workflow and latest-head required check. Do not treat local rustfmt parsing as successful compilation or mark any automated test green without Actions output. This handoff creates no commit, push, PR or merge. M20 remains for user manual acceptance after the required checks and merge.

## Changed file groups

- Store: `migrations/0027_subagents.sql`, `src/{lib,threads,subagents,recovery,token_usage,credential_privacy}.rs`.
- Runtime: `src/agent/{mod,loop_,mcp_registry,tools_exec,subagents}.rs`, `src/lib.rs`.
- Tools: `src/tools.rs` (`Tools::isolated`).
- Conversation: `src/{lib,threads,history,summary,subagents,privacy}.rs`, `types/{mod,thread,events,tool_calls,subagents}.rs`, `agent/{mod,entry,persistence,pipeline,subagents}.rs`, `agent/tests/{mod,subagents}.rs`, `automation/{mod,runtime,tests}.rs`.
- App: `src/{main,app_agent,native_subagents,tests}.rs`, `window/{mod,artifact,render,native_subagents}.rs`, new `tests/subagents.rs`.
- UI: `conversation_stream/{mod,core,model,content,render,render_rows,message_anchors,subagents}.rs`, `sidebar/mod.rs`, `tool_card.rs`.
- Mechanical compatibility: existing App/UI Thread and HistoryPage fixtures and exhaustive StreamEntry test labels, including `window/workspace.rs` fixture values.
- Documentation: README index, A3-14 feature row and this delivery. The main agent's specification amendments remain in the shared worktree.

The implementation writer stops at this cut for main-agent review and Actions diagnostics. The remaining test combinations above are recorded explicitly rather than represented as completed acceptance.

## CI correction · run 39086493630

[First PR Check run](https://github.com/puzige/vega/actions/runs/39086493630), head `b3cbf7f63d688f90cc9804e31d00a30bfa48194c`, failed compilation in both the Clippy and Nextest jobs at `vega_store/src/subagents.rs::active_count`: SQLite `row.get(0)` inferred `usize`, which does not implement `rusqlite::types::FromSql` (E0277). Automated tests did not run; this run is not validation of their assertions.

The helper now reads the SQL count as `i64`, then uses checked `usize::try_from`; negative or out-of-range values return `IntegralValueOutOfRange` rather than being truncated. Static inspection of the newly added store reads found no other SQL-to-`usize` conversion: delegation timestamps use `i64`, flags use `bool`, relationship fields use optional strings, and usage aggregation reuses the existing signed-row validation and checked arithmetic.

Local verification after the correction remains format/diff/static only. A new latest-head cloud Clippy/Nextest run is required; no build or test was run locally.

## CI correction · run 39086920531

[Second PR Check run](https://github.com/puzige/vega/actions/runs/39086920531), head `5e6ecd82e96affc5baafb360e45275a5e4c988ce`, did not compile successfully and did not start automated tests. The full diagnostic log reports two E0308 errors in the conversation subagent tests, E0624 for the UI's private `automation_button`, and Clippy `cmp_owned` in subagent input preparation; the remaining error lines are compilation summaries of those diagnostics.

The two priced test calls now put `None` in `persisted_user_message_id` and `Some(pricing())` in `pricing_catalog`, preserving the cost/cache assertions. The other Native-entry calls in that test module were checked against the actual function signature. The existing keyboard button helper is shared only with sibling conversation-stream modules through `pub(super)`. Subagent argument checking now binds the decoded canonical JSON string once before comparing the redacted string with it; both raw-input and decoded-input credential checks remain in place.

The new live-run guard's immediately nested `if let` was also changed to its equivalent let-chain during static lint review, without adding lint allowances or changing its cleanup behavior.

Remote fetch completed and the local branch matched the remote head before editing. Local verification is still format/diff/static only; a new latest-head cloud run must confirm compilation and execute the tests.

## CI correction · run 39087392860 · Clippy

At head `351eeedf436d0e397d730c88d4708f2e22d17b18`, [Clippy job 117556724285](https://github.com/puzige/vega/actions/runs/39087392860/job/117556724285) failed with `useless_conversion`: the subagent button's debug selector converted an existing `String` into the same type. It now returns `id.clone()` directly. Static inspection of the newly added subagent modules found no other `clone().into()` or `to_string().into()` conversions of this kind.

The branch matched the remote head after fetch. Local format/diff checks are the only checks performed for this correction. At the initial Clippy handoff, the run's Nextest job was still running; its completed result is recorded below.

## CI correction · run 39087392860 · Nextest

[Nextest job 117556724435](https://github.com/puzige/vega/actions/runs/39087392860/job/117556724435) subsequently failed during compilation of the App test target. No tests ran. Its three diagnostics were a missing `ComposerSubmitted` import (E0422), a root `ActiveAgentRun` test fixture missing the three new optional identity fields (E0063), and access to a moved boxed admission after constructing the child stream (E0382). The separate linker `__eh_frame` warning was not the cause of this compilation failure.

The test imports `ComposerSubmitted` explicitly and the ordinary root fixture uses `None` for all three identity fields. Repository-wide inspection found the production constructor already initializes them and the other occurrence is a destructuring pattern with `..`. After the original admission guards pass, the App now explicitly destructures `SubagentAdmission` before moving its thread/queues into the stream constructor. The original cancellation token, snapshot, task and queue generation remain separately owned for active registration, parent binding and cancellation when acknowledgement fails.

The pending Clippy correction is retained. Fetch preserved the existing worktree edits. Local validation remains format/diff/static only; cloud compilation and automated test execution are pending a new latest-head run.

## Baseline integration · 995357bc

The three implementation commits were rebased onto `995357bc` (`feat(A2-21): refine goal status strip and add editor (#458)`). The render subscription conflict keeps the shared `bind_conversation_stream` path and includes the new `AutomationEditorRequested` subscription for both root and delegated streams. Master's compact automation strip, editor visibility/size synchronization and editor refresh on run events are preserved.

The new automation icon button has a different signature. Back to parent now uses a local text button with its existing focus handle, accessible name, Enter/Space activation and reserved focus border, leaving the automation button private. Static inspection confirmed the automation run claim and usage APIs still match delegated budget checks and pending provider accounting. This integration has format/diff evidence only; compilation and automated tests require the latest-head cloud check.

## CI correction · run 39088126961 · Clippy

At head `0c8da2af4fbd844506db44d841dfd659e8ba77cd`, [Clippy job 117558908765](https://github.com/puzige/vega/actions/runs/39088126961/job/117558908765) failed to compile the production App binary: `native_subagents.rs` imported `futures`, which is available only as an App dev-dependency, causing E0433/E0432 and E0599 for `.boxed()`.

The Native host now uses the standard-library `Pin<Box<dyn Future<Output = Result<(), VegaError>> + Send + 'static>>` return type and `Box::pin`, matching the trait's boxed future without changing dependencies. The original oneshot acknowledgement and cancellation-biased select remain intact. Nextest was still pending when this Clippy correction was made; its completed result is recorded below. Local validation remains format/diff/static only.

## Privacy compatibility · specification v1.1

`credential_privacy::fields`, review labels and the source fingerprint now include the scoped delegation copies. Metadata uses no message anchor, so child review cannot navigate into the parent transcript. The fingerprint also includes delegation identity, state, truncation, usage completeness and creation metadata. Existing exception reads, approvals and provider admission are unchanged.

Cleanup rejects active delegated rows even before a child assistant message exists. Inside the same immediate transaction as transcript cleanup, it redacts scoped copies and preserves identity/run state. Names stay within 256 UTF-8 bytes and use a stable child-ID hash suffix only when needed to avoid another name in the same parent run; nonempty tasks stay within 32 KiB, results within 16 KiB, and any new result truncation sets `truncated` while preserving an earlier truncation flag. A child-only cleanup leaves sibling and parent transcripts untouched.

Six store regressions and one conversation review/exception regression were added for this slice, using only synthetic text and owned temporary storage. They have not been compiled or run locally. Latest-head cloud Clippy/Nextest must validate them; local format/diff/static checks remain the only local evidence.

## CI correction · run 39088126961 · Nextest

[CI4 Nextest](https://github.com/puzige/vega/actions/runs/39088126961), at earlier head `0c8da2af4fbd844506db44d841dfd659e8ba77cd`, reported **3064 tests run: 2804 passed, 260 failed, 5 skipped**. The failures comprise 255 stack-overflow SIGABRTs, four schema assertions that still expected the earlier 38-table schema, and the App child Stop assertion described below. These are executed-test results, separate from CI4 Clippy's production-binary compilation failure.

The new unboxed parent pipeline was stored inline in both the Native entry state and generic supervisor future, including the ordinary Native branch without a supervisor. Runtime and event-processor futures now enter the join through `LocalBoxFuture`, the combined pipeline is also boxed, and `Supervisor::run` takes that boxed borrowed future directly. Provider/tools lifetimes remain scoped; parent/children are still polled together, and parent completion still cancels then drains children. The existing Native and delegated execution suites must confirm the stack regression is resolved in cloud CI.

The exact schema table set now includes `delegated_runs`, and all four current-schema assertions expect 39 tables while retaining migration/data-preservation checks. The App fixture had issued a parent-stream Stop while the child route was current, correctly failing the stream ownership guard. It now asserts that rejection, navigates to the parent before the group Stop, and retains the later-generation fence. A separate child-route Composer Stop assertion covers stopping only that run.

These corrections and the v1.1 privacy slice have only local format/diff/static evidence. They have not been compiled or tested locally and are not covered by the earlier CI4 result. Latest-head cloud Clippy and Nextest remain required.

## CI correction · run 39089207379

[CI5](https://github.com/puzige/vega/actions/runs/39089207379), at head `e3aeb75402b0aa955d5a7942fd248acb1b9b6fd4`, failed compilation in both jobs with the same E0063: delegated admission's `AutomationRunOwner` initializer omitted the `started_at` field added by the goal editor integration. Nextest did not execute tests, so CI5 provides no executed-test evidence for the future-size correction or privacy regressions.

Delegated admission now initializes `started_at: None` alongside `claim: None`, matching an ordinary owner before its own run event. It does not inherit the parent's goal clock or create a child automation claim. Static inspection of all six owner constructors confirmed the other five already initialize the field. Only local format/diff/static checks were performed after this correction; latest-head cloud Clippy and Nextest remain required.

## Baseline integration · 23c1d4f0

The existing three commits were rebased onto `23c1d4f0` (`fix(A3-03): keep long tasks progressing with bounded tool context (#459)`) without textual conflicts. Static inspection retained master's bounded tool context, progress/recovery reporters, raw audit output and runtime-result projection. All feature tool results use the common execution path, which observes recovery evidence and constructs model messages through `runtime_tool_result`; master's existing computer-use supersession branch remains unchanged.

Subagent capability definitions and replay digest, raw/canonical input credential checks, cancellation outputs and exact delegated generation guards remain present. The master provider additions are private `ChatMessage` metadata initialized by its constructors; the subagent fixtures already use those constructors, and `RuntimeToolResult` gains no new fields. No fixture field adaptation was needed. Native runtime/processor/pipeline heap boundaries and the child automation owner's `started_at: None` remain intact. Latest-head cloud compilation and tests are still required.

Integration inspection also found a pre-existing credential-failure mismatch: generic result redaction produced `Tool error: credential safety check unavailable` with Failed status, while the subagent conversation codec requires the `Tool error: tool 'subagent' failed:` prefix. A private runtime postprocessor now converts only that failure or a successful non-object/invalid JSON/over-128-KiB result into a fixed safe Failed result, clearing inapplicable truncation/exit/duration metadata. It preserves call identity, reuse, approval and remember-rule facts and leaves ordinary cancellation/rejection unchanged. It runs at the three existing question-result validation points before recovery observation and model projection; the invalid write/bash-only branch and conversation codec are unchanged.

The three caller paths are persisted-result reuse, mixed Skill batch rejection and ordinary live completion. Each keeps `recovery.observe` after safety validation and the existing `runtime_tool_result` model projection. The unit regression exercises the postprocessor with `reused=true` and checks retained audit facts; the conversation regression covers a fresh live terminal through persistence and reopen validation. It does not claim a separate full replay/persistence journey for the reused normalization case. Master's bounded-body classifier remains limited to read/grep/glob/bash, so it does not trim subagent receipts.

Two new in-process regressions cover the real runtime-hook reader transition through persisted/reopened terminal validation and the minimal postprocessing cases. They have not been compiled or run locally. The integration and these corrections have format/diff/static evidence only and require latest-head cloud Clippy/Nextest.

## CI correction · run 39090036232

[CI6](https://github.com/puzige/vega/actions/runs/39090036232), at head `43474495a7bf36e8fad94c632dec961bc70d0ab9`, passed format and full-workspace all-targets Clippy. Nextest reported **3135 tests run: 3120 passed, 15 failed, 5 skipped** in 224.493 seconds, with **zero stack overflows**. All six store privacy cases, conversation privacy, runtime-hook reader failure, result postprocessing and child Composer Stop passed. This is executed cloud evidence for that head.

Ten conversation delegation cases failed or timed out because `validate_runtime_proposal` had not admitted valid nonempty subagent inputs; `spawn_agent` stopped at the strict proposal audit before child work began. The four tool names now have an exact input branch. Their approval codec requires complete valid input, Once/ReadonlyTool and no note, danger or remember grant. `{}` remains available only as the existing safe proposal for unavailable/cancelled/rejected calls; it cannot approve an empty spawn/wait/cancel. A new positive/negative codec regression preserves these boundaries without broadening unknown tools.

The existing automatic-worker deletion regression exposed an overbroad own-thread live guard. It now applies to real children, while the live-child guard still protects a parent; ordinary roots without active children retain their previous cancel/delete behavior. The original App assertion is unchanged, and a new conversation regression covers that ordinary-root boundary.

The navigation fixture attempted manual `begin` while the terminal handler's existing asynchronous `finish_pending_run` queue recovery still held its operation gate. The fixture now waits for that exact gate and busy projection to settle, preserving the paused queued message, its content and original queue generation. It then asserts a distinct manual generation with independent queued/delegated identity. Production queue admission, ownership guards and test time limits are unchanged.

The remaining three failures were precise schema assertions. All current exact table sets now include `delegated_runs`, including the later S7 assertion after its version check; S7 expects migration 27 and 39 tables with matching text. Static repository inspection found no other stale current-schema counts/sets; explicit older migration fixtures retain their original versions.

These corrections and the two new codec/deletion regressions have only local format/diff/static evidence. Latest-head cloud Clippy and Nextest must confirm them; no Vega code or test was compiled or executed locally.

## CI correction · run 39091044866

[CI7](https://github.com/puzige/vega/actions/runs/39091044866), at head `6c26eb1dde134769b3c938b4d4273cc65a151975` on baseline `23c1d4f0`, passed format and full-workspace all-targets Clippy in 2m40s. Nextest reported **`Summary [240.781s] 3137 tests run: 3128 passed, 9 failed, 5 skipped`**, with **zero stack overflows**. The strict proposal/approval codec, ordinary-root deletion, App queue and current-schema corrections passed. The nine remaining failures are delegated conversation main-chain cases; the earlier fifteen failing test cases are not all validated by this result.

Static tracing found that the child lifecycle correctly passed its precreated user ID to the Native entry, but preparation treated every existing user as a Plan approval instruction. A delegated task therefore failed before its provider request. Preparation now has a separate delegated admission branch that requires the original parent/child generation, the actual parent relationship and assistant owner, a done/text initial user row whose content equals both task copies, and an unconsumed sequence. Only Running/Cancelling metadata may enter. A parent assistant already marked done by steering remains valid. Delegated admission cannot borrow credential continuation, and the existing Plan approval checks remain unchanged.

The single parsed spawn receipt had a separate fixture cause: master's initial progress reminder appends a `<vega_runtime_notice>` to the final projected tool message. The synthetic providers had attempted to decode that entire string as JSON. They now read the first JSON object and accept only an empty suffix or the controlled notice suffix. Production result projection and progress behavior are unchanged; both-child receipt/provider assertions and existing time limits are retained.

A child admission returning Cancelled also reached the parent's runtime as cancellation even when that parent tool-call token remained live. Only the spawn reply branch now converts that child-local rejection to a scoped subagent failure. An already cancelled parent token still returns Cancelled. The rejected-admission regression asserts two interrupted children, two failed dispatches and normal parent completion; the pending-admission parent Stop regression retains interruption/drain assertions and also checks that no second parent request starts.

The new delegated-start regression uses a real scripted Native entry for successful Running/Cancelling admission and a rejected second consumption. Fifteen malformed or missing identity/task/user/generation cases assert no provider call and unchanged parent/child source fingerprints and child relationship/activity fields. A separate receipt-reader regression rejects unexpected trailing text. These corrections have only local format/diff/static evidence; no Vega code or test was compiled or executed locally. Latest-head cloud Clippy and Nextest remain required.

## CI correction · run 39092631237

[CI8](https://github.com/puzige/vega/actions/runs/39092631237), at head `d4b3139de0747700551d58cacdc3a52e8d3f9329` on baseline `23c1d4f0`, passed format and full-workspace all-targets Clippy (`Finished dev profile ... in 1m 39s`). Nextest reported **`Summary [150.567s] 3139 tests run: 3137 passed, 2 failed, 5 skipped`**, with **zero stack overflows**. The two new delegated-start/receipt-reader cases passed, as did the concurrency, durable sibling failure, child/parent Stop, pending admission, rejected admission, gated parent failure, replay and cache-usage cases.

The permission/read-witness fixture used one temporary directory for both the project and database. Its default checkpoint directory was consequently inside the project root, which `MutationContext::new` correctly rejects before a tool proposal. The fixture now uses the existing separate project/data setup and retains both directories, writes the same lib.rs fixture, and asserts that the checkpoint location is outside the workspace. All four permission modes and both operations remain. Both live terminal events must match their durable child/message/status/output; valid Write proposals must match stored audits. Both Write targets follow the permission result, and each Edit must record FileNotRead despite the parent's prior read. Failure messages identify the permission/operation branch and safe child states.

That fixture also assumed the last User message was always the task and a previous result always had Tool role. Master's existing projection represents non-replayable tool results as authority-none `vega_history_observation` User messages. The scripted providers now identify only their controlled task rows and recognize an existing operation by either its ordinary tool-call ID or version-1, authority-none, tool-result observation with that same ID. They receive the complete original requests. Production checkpoint fencing, permissions and model projection are unchanged.

The continuation fixture similarly expected a prior assistant reply to retain Assistant role in the model request. The existing cold-prefix contract wraps it as an authority-none assistant-text observation. The test now requires the original provider text delta, durable assistant text and delegated result to equal Child result exactly once, then requires the complete version/authority/kind/text observation exactly once in the follow-up request. Original records, independent generation, cancellation and usage assertions remain in place.

Only the conversation test file and this delivery document changed for this correction. The current fixture changes and strengthened assertions have local format/diff/static evidence only. No Vega code or test was compiled or executed locally; latest-head cloud Clippy and Nextest remain required.
