# Issue 448 · Native subagent implementation handoff

Status: implementation review cut; automated checks NOT RUN except local format/static checks.
Scope: frozen [specification](vega-issue-448-multi-agent.md), including the parent-deletion and retained standalone-workspace clarifications. Native only; no ACP behavior or capability changes.

## Implemented behavior

Eligible Native Execute root runs expose `spawn_agent`, `list_agents`, `wait_agent` and `cancel_agent`. A scoped supervisor polls the parent and independent child conversation pipelines together. Capacity is reserved before creation; admission persists the thread/task/relationship and awaits registration of the real App active run and stream before returning a spawn receipt. Parent termination cancels and drains the generations it owns.

Each child uses the frozen model/provider/reasoning/pricing/permission policy, an isolated Tools/read witness/checkpoint identity, fresh request queues, and fresh history/replay state. The launch snapshot retains only the child settings it needs; it does not retain the parent's completed approval receipts, credential exception, replay registry or computer-use owner. Child provider input is additionally checked without the parent's credential exception. Children cannot dispatch another level or activate computer-use.

The parent shows a run-owned group with actual names/states, Open/Stop, expandable bounded results, and the original child permission/question responders. Opening a running child reuses its registered stream and normal Composer queue behavior. After the initial generation terminates, the group releases its live request bindings; a later user run keeps the conversation relationship while using independent run ownership.

The additive migration stores the parent link, original delegated generation/result and usage attribution. Cold parent history recovery interrupts incomplete dead children while preserving live workers. History groups load only with their owning parent message/page. Deleting a parent detaches its children and preserves their transcripts. Standalone children retain their original scratch workspace identity after deletion of the parent; no workspace files are moved or deleted.

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

All tests below are **NOT RUN**. They use in-process providers/queues and owned temporary storage; no real process/network/App validation was performed. The listed test names are searchable in the corresponding modules.

| IDs | New assertions / file |
|---|---|
| M01/M02/M05/M17 | `agent/tests/subagents.rs::m01_m02_m05_m17_parallel_children_enter_before_release_and_persist_separately`: both gated children enter `chat_stream` before release, root reaches wait, distinct child transcripts/events, originating usage rows, combined cost/goal budget, original result preserved after un-attributed child follow-up usage |
| M03/M04/M05 | `agent/subagents.rs::tests::m03_m04_m05_capacity_names_scope_wait_timeout_and_wakeup`: four reservations, fifth rejection, completed-slot reuse, duplicate names, foreign wait rejection, honest zero-timeout, notification wakeup and cancellation; store tests bind names/capacity to parent generation |
| M03/M19 | `vega_runtime/agent/subagents.rs::tests::m03_m19_delegation_changes_replay_capability_only_for_execute`: tool surface changes the replay capability digest; Ask/Plan do not gain it |
| M04 | Runtime strict/bounded input tests, escaped-secret preparation test, and conversation `m04_escaped_owner_secret_never_creates_child_metadata_or_provider_work`: decoded owner secret is rejected, no child/provider work, input audit is `{}` |
| M06/M15 | Conversation `m06_m15_child_failure_and_sibling_result_are_durable`: one provider failure, sibling success, durable nonempty failed result after reopen, missing provider usage remains unknown |
| M06/M15 | Supervisor `m06_m15_failed_and_interrupted_terminals_commit_with_recovery_or_rollback_together`: both Failed/Interrupted results, usage completeness and transcript/tool cleanup commit durably; injected cleanup failure rolls all of them back |
| M07/M08 | Conversation `m07_m08_stop_one_child_then_parent_while_waiting_drains_every_owned_generation`: child-only stop preserves sibling, parent Stop while inside wait drains owned work, no streaming rows remain |
| M07/M08/M16 | Conversation `m07_m08_m16_stop_while_admission_ack_is_pending_never_starts_provider_or_reports_spawn_success`: cancellation while host admission is pending returns interruption, no successful spawn or child provider call, durable terminal metadata |
| M08/M16 | Conversation `m08_m16_natural_parent_end_and_rejected_admission_do_not_leave_live_work`: natural parent completion and rejected admission leave terminal children; rejected admission starts no child provider |
| M08 | Conversation `m08_parent_provider_failure_after_both_children_enter_cancels_and_drains_durably`: both children are waiting inside their provider before the parent failure gate releases; the failed parent cancels/drains both, persists interruption and leaves no live/streaming rows |
| M09/M10 | Conversation `m09_m10_child_read_witness_and_permission_modes_remain_independent`: parent read does not authorize child Edit; readonly/confirm/auto/full-access child mutation decisions remain independent |
| M11 | GPUI `m11_two_child_permissions_share_the_original_responder_and_stop_is_scoped`, `m11_m14_two_child_questions_route_once_and_terminal_group_unbinds_later_requests`, `m11_settings_fails_child_permission_and_question_closed`: two simultaneous requests, originating queue response, peer remains pending, Settings/timeout fails closed |
| M02/M07/M12/M13/M14 | App `tests/subagents.rs::m02_m07_m12_m13_m14_native_child_admission_navigation_queue_and_generation_ownership`: root MessageStarted and Admit in one batch, child single flight, real event/stream routing, parent→child→other→child, draft retained, busy queue uses child generation, old Stop cannot affect later controller generation, window cleanup cancels it |
| M14 | GPUI terminal group releases the child stream before a later user question, so that later request is not rendered by the old parent |
| M13/M14/M17 | Conversation `m13_m14_m17_terminal_child_normal_native_followup_preserves_history_and_original_ownership`: a terminal child runs a real scripted follow-up through the normal Native entry, receives its original history, starts exactly one new run, retains its relationship/result/message, excludes new usage from the old parent and ignores the old parent's cancellation token. Combined with the App queue/controller test above, this covers continuation across both boundaries |
| M15/M18/M19 | Conversation `subagents.rs` tests recover dead children on parent reopen, skip live children, attach separate historical runs only to their loaded owner pages, preserve child history/root reachability on parent deletion |
| M15 | `m15_standalone_child_inherits_parent_actual_scratch_root`: parent/child share the real scratch root; deleting parent and reopening child retains that root and a synthetic file |
| M16 | Conversation `m16_repeated_completed_spawn_is_reused_and_creation_failure_rolls_back_reservation`: repeated call ID reuses its durable tool result and performs one child run; injected delegation insert failure rolls back child creation |
| M17 | Conversation `m17_child_missing_cache_is_unknown_in_parent_run_summary`; automation `m17_parallel_provider_usage_and_delegated_budget_are_counted_once`; GPUI `m17_child_live_usage_changes_parent_total_without_changing_parent_context_or_round_estimate` |
| M18 | GPUI `m18_group_result_keyboard_controls_and_layout_follow_real_height`: result keyboard toggle, Open/Stop events, dynamic list measurement, result/peer geometry in light/dark themes and wide/narrow windows |
| M19 | Existing Native, parallel roots, pending-message, goal/loop, permission, replay and UI suites remain enabled for the workspace CI run. Mechanical Thread/HistoryPage/StreamEntry updates retain existing assertions |

### Explicit remaining coverage limits

- M08 has separate conversation cancel/drain tests (including gated parent provider failure) and App window-cleanup cancellation assertions. There is no new single App/provider test that destroys the window while admission acknowledgement is pending and then inspects drained durable child rows.
- M13/M14 continuation coverage combines the App queue/controller/generation test and a real scripted provider follow-up through the normal conversation entry. It does not drive both stages through a single App Composer fixture.
- M09/M10 add delegated read-witness and permission-mode coverage. Dangerous command, exact-rule, external-path and completed one-shot approval behavior rely on the existing permission/tool safety suites; the new file does not duplicate that full matrix for every child.
- M18 adds geometric/interaction assertions, not rendered screenshot evidence. Selection/scroll regressions are retained in the existing suites. Pin/search reachability is statically preserved by filtering only default thread lists; no new dedicated child pin/search UI test was added.
- Compilation, Clippy, all new/existing automated regressions and M20 real user acceptance remain pending. No item above is a pass claim.

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

- Store: `migrations/0027_subagents.sql`, `src/{lib,threads,subagents,recovery,token_usage}.rs`.
- Runtime: `src/agent/{mod,loop_,mcp_registry,tools_exec,subagents}.rs`, `src/lib.rs`.
- Tools: `src/tools.rs` (`Tools::isolated`).
- Conversation: `src/{lib,threads,history,summary,subagents}.rs`, `types/{mod,thread,events,tool_calls,subagents}.rs`, `agent/{mod,entry,persistence,pipeline,subagents}.rs`, `agent/tests/{mod,subagents}.rs`, `automation/{mod,runtime,tests}.rs`.
- App: `src/{main,app_agent,native_subagents,tests}.rs`, `window/{mod,artifact,render,native_subagents}.rs`, new `tests/subagents.rs`.
- UI: `conversation_stream/{mod,core,model,content,render,render_rows,message_anchors,subagents}.rs`, `sidebar/mod.rs`, `tool_card.rs`.
- Mechanical compatibility: existing App/UI Thread and HistoryPage fixtures and exhaustive StreamEntry test labels, including `window/workspace.rs` fixture values.
- Documentation: README index, A3-14 feature row and this delivery. The main agent's specification amendments remain in the shared worktree.

The implementation writer stops at this cut for main-agent review and Actions diagnostics. The remaining test combinations above are recorded explicitly rather than represented as completed acceptance.
