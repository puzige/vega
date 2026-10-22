# Question-wait test-first review record

Issue: [#548](https://github.com/puzige/vega/issues/548)

## Stopped recovery draft

Stage: pre-RED test review. Production implementation has not begun. No Rust compilation or test execution, draft PR, RED or GREEN exists at this checkpoint.

- Branch: `feat/548-question-wait`.
- Base, independently fetched and checked: `871f91f246e90753743a256cee1eec315e74effc`.
- Specification: [v1.1](vega-issue-548-question-wait.md), SHA-256 `55ab06c5f2f026ae34ec0710fad85ed91b0ded4b1d01a2eac0803dafa9152025`.
- Writer: `548-recovery-writer`; explicitly READY and stopped code writes.
- Code scope: 13 tracked support/manifest/lock files and two untracked test files, excluding parent-owned documentation.
- Sorted code content-manifest SHA-256, independently reproduced by the parent: `92b5b6ff07083655b87f12cb7875e3f03f376245aac19800dca5a762aec07def`.
- Writer-reported patch SHA-256: `6ddf31fadaeb873b57786c744b79253e4f09731596c751e7b3ddf388296c149b`; stable patch ID `1b4084f6903a0dbade272030193f40a12c2c5055`.
- Conversation test SHA-256: `b1702104c45500e428718b238ed9e854072f679e57486110d0fd9896069849b2`.
- Window test SHA-256: `ee5c9ffdbc1705b6ae13808ad22b8e0b84cb0e1ce3eef4069a289cfec031890e`.

Static declarations: 16 conversation tests and 10 GPUI tests. These are source inventory counts, not discovered or executed test counts. The parent reran `git diff --check` and `cargo fmt --all -- --check`, both successfully. The writer also reports successful offline locked Cargo metadata resolution; metadata does not compile or execute tests.

The draft expects missing `PersistenceActorConfig::with_work_time_source` and optional `ConversationEvent::RunStarted.work_clock` APIs. Only observed target-interface failures may later qualify as interface-missing RED; unrelated harness/compiler/environment failures do not. All behavioral assertions remain unexecuted.

Independent read-only test review by `548-test-review` against this fixed content version returned **NOT READY**, with the three findings below. It independently reproduced the content manifest and stable patch ID; the writer's named temporary raw-patch bundle was unavailable, so its raw patch SHA remains writer-reported. No source writes or executions occurred during review.

## Parent diagnostic: submitted values are not durable completion

The existing [AskUserQuestion contract](../445/vega-issue-445-ask-user-question.md), sections 6–7 and especially the submission paragraph, requires submitted values to remain visible with repeat actions disabled until the acknowledged terminal event. A successful local response-channel send is not durable success. Current production `UserQuestionPanel::advance` marks the panel submitted; `conversation_stream/content.rs` removes it via `finish_user_question` on `ToolCallFinished`. The conversation event pipeline awaits `PersistenceActor::event` before forwarding the converted event.

The stopped test draft instead requires panel absence after only queue resolution in three scenarios:

- `background_question_notifies_once_and_click_routes_to_its_live_task`, line 494.
- `unavailable_notification_platform_leaves_question_blocked_and_answerable`, line 750.
- `delegated_question_uses_child_identity_without_completion_suppression`, line 820.

Each scenario submits or skips, polls a `RuntimeUserQuestionResponse::Answered`, then asserts `!fixture.panel_present(cx)` without supplying an acknowledged `ToolCallFinished` event. This would force an unintended regression of the existing persistence/UI contract. It is a pre-freeze test-expectation defect, not a product failure, valid RED, or newly authorized product behavior.

Required correction before test freeze:

1. Separate unresolved user attention from submitted-panel retention. Answer/explicit skip ends the user wait and dismisses the matching question notice; it does not claim the answer is durably stored.
2. Assert the submitted panel/values remain non-actionable before the matching terminal event, without repeated answers or repeat notifications. Resume work-time accounting independently.
3. Feed the matching acknowledged terminal result with the correct answer or skip payload, then assert panel removal. Keep the real-pipeline live UI assertion after provider continuation, where terminal persistence acknowledgment has already occurred.
4. Preserve cancellation/drop cleanup and first-wins semantics, all duration and notification assertions, and permission-wait independence. Do not weaken those boundaries to accommodate the panel contract.

## Independent review: mixed-batch admission observation

`question_wait_lifecycle.rs:802–859` tests a question plus write using `RejectPermissionHook`, but checks only the question's batch-rejected result and absence of the write output file. A broken batch barrier could send the write to ordinary authorization and still pass because the hook rejects it. Parent inspection confirms this masking risk; baseline `agent/loop_.rs` applies `USER_QUESTION_BATCH_REJECTED` to every call in the mixed batch before permission admission.

Before freeze, assert exactly one batch-rejected result for each call, no Approved/Running transition for the write, and no permission-hook admission. Keep the absent-file check as an additional side-effect assertion. This is an observation correction for existing W1/W5, not new product scope.

## Independent review: fresh owner and reused identity

`question_wait_tests.rs:542–570` uses distinct `first`/`second` call IDs; `question_wait_lifecycle.rs:436–520` performs its later runs only on an independent thread that never waited. Queue listener-replacement coverage with a reused ID does not establish active-clock or app-notification ownership.

Before freeze, exercise a later run on the same formerly waiting task/stream with a fresh run/request identity. Verify fresh work duration, one new notification, and that stale cleanup cannot resolve the new question or dismiss its notification. Preserve the independent-run case and sequential distinct-ID regressions. This completes existing W4 and the v1.1 identity clarification.

Parent follow-on API diagnosis: the real conversation pipeline restores `tool_calls::terminal_results` into `completed_tool_results` (`agent/pipeline.rs:890–939`). The runtime deliberately replays a prior matching call ID (`agent/loop_.rs:2953–3002`), preserving #445's replay identity. Therefore a second real run with the same durable call ID cannot be required to ask afresh. Use a new call ID in the real-pipeline later-run clock case; exercise the reused ID with a fresh controller generation at the actual app/queue notification boundary. Do not delete store history or alter replay semantics to make a fixture wait.

## Disposition

Parent checked all three findings against source evidence. Specification v1.2 explicitly preserves the prior acknowledgment contract and spells out these existing W1/W4 observations; no new product acceptance or implementation is introduced. The original sole writer will make bounded test/support corrections, stop, and provide a new content identity. Independent delta review must approve before draft PR/cloud RED. Production changes remain prohibited until reviewed cloud RED.

The review found no additional definite unrelated type/API defect statically. GPUI scheduling, watchdog behavior, type/borrow correctness and all-target warnings remain unverified until Actions. Native OS notification denial and actual focus/banner behavior remain manual acceptance. All temporary Rust tests/support are retained until same-tests GREEN, then removed separately with durable PR evidence.

## Resumption after interrupted correction

At `2026-10-22T17:11:55Z`, the resumed parent found no owned child executions. Both test hashes still matched the stopped recovery draft above; the interrupted correction had not changed them. Existing artifacts and the prior review are preserved, not restarted. After fetching, the branch fast-forwarded to `2ba904f7074115de5ee89cc18c376c083f803357`; the only upstream change was `docs/554/vega-issue-554-panel-motion-study.md`, so no tested production path or acceptance changed. A replacement sole writer will complete the three bounded pre-freeze corrections. Parent retains documentation, Git/index and CI ownership. No production repair, PR or cloud RED exists yet.

The correction writer was stopped after more than 20 minutes without new artifacts or a READY report; interruption was confirmed before any replacement. Its completed work is retained: submitted-panel observation/correct acknowledgment payloads, direct mixed-batch rejection/permission-admission assertions, and app-level fresh-generation/reused-ID isolation. The unfinished real-pipeline later-run fixture still reused the durable `waiting` ID and required a fresh question, conflicting with the replay evidence above. A bounded successor must change only that later real call to a fresh ID and format the four touched files; no restart or discarded regressions. The stopped conversation test SHA-256 is `3b27e3a07d42139d6c0a45666e94f9f3f42c712a222f6578de7cc2315d221047`; window test is `9dc30c713fe41b7c7d48b7f9cfd6040fb0c94fd4ca84cb190ee0c5071220fc70`. Parent `git diff --check` passed; `cargo fmt --all -- --check` exited 1 for formatting only. No type/test execution occurred.

## Corrected, stopped version for delta review

The bounded successor completed `waiting-later` for the real pipeline and formatting only, returned READY and stopped writes. The parent independently reran `git diff --check` and `cargo fmt --all -- --check`, both exit 0; all 26 original declaration names remain. Sorted 15-file code manifest SHA-256: `7cd9cfba51e9adb978be01c82a6e3e7e4c61aa5d3dd542b5fb74deab006241ec`. Conversation test SHA-256: `dd447212b3fe8292e97637d83e01799c241840b1bc2f9d19b393498c83798385`; window test: `328eafb6936923cda51c5b3544371dd6f033207fcad3f16e1ac006a771ef5f2f`. Independent read-only delta review returned NOT READY solely for the terminal-card fixture collision below; submitted-panel retention, direct mixed-batch isolation and real later-run timing corrections were sound statically, with all prior regressions retained. This is not cloud evidence.

Parent's remaining fixture question: the app reused-ID case sends `ToolCallProposed` for the old terminal `first` card in the same stream. `content.rs:1311–1320` treats an existing non-pending proposal as corrupt; `tool_card.rs:269–279` then cannot match it as an answerable request. Verify the fixture uses a legitimate fresh history projection or an attention-only boundary that does not require changing terminal-card/replay semantics. Production must not be changed merely to satisfy this setup.

The independent reviewer confirmed the collision and identified a fixture-only route: after the old run finishes, call existing `replace_history_window` with a valid target-containing non-tool page before the later `MessageStarted`. This clears the visible card index but preserves the same stream/queue/listener and all stale-generation handles; empty pages are not accepted. The original bounded successor is applying only this correction. Before that change, parent fetched and fast-forwarded to `6615748f91273bfb8ee424011b3b5a09b3777d2b`; upstream #546 changes Composer controls and #547 changes image/context budget, not question lifecycle. The question clock/notification acceptance and both test source hashes were unchanged by the sync. Existing Actions on this updated base must still supply the first valid RED.

The fixture-only change is now stopped and READY: `question_wait_tests.rs` imports the history projection types and, after the old run finishes, asserts successful replacement with a completed non-tool `run` entry. Same stream/queue/listener and old handles remain. Current window SHA-256: `816c7c291fc98175b72fea0844222bafe06ce5a890bcd78542b7e3d72c9ab0d6`; conversation remains `dd447212b3fe8292e97637d83e01799c241840b1bc2f9d19b393498c83798385`. Parent independently verified sorted 15-code-file manifest `d0a72e9f2737bcd9798824f1b2d46dd47c8b5a19367a744c1e65852de7826e0f` and reran format/whitespace checks, both exit 0. The same independent reviewer returned **READY for cloud RED** after reviewing only this final delta and updated-base applicability. It verified successful target-containing hydration, removal of the old card index, unchanged stream/queue/listener and stale-owner assertions, preserved question/Stop rendering, and unchanged mixed-batch/replay branches. No blocker remains in this static review; compilation, test discovery and all behavior assertions are still unexecuted. Next stage is the first test-only draft PR and existing Actions; anticipated missing clock APIs are not yet observed RED.
