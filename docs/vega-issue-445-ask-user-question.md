# Vega AskUserQuestion

Issue: [#445](https://github.com/puzige/vega/issues/445). Feature IDs: A3-18 / A2-22.

Version: 1.1, 2026-10-17. Implementation baseline: `3705c687`, branch `feat/445-ask-user-question`. Status: implementation contract frozen; automated and manual acceptance remain pending.

## 1. Product decisions and scope

The user wants Vega itself to ask structured questions, using Codex as a reference. This card implements the Native agent tool and its complete answer loop. It does not implement ACP elicitation or change the Codex backend.

Explicit user decisions: the question panel appears above the composer, shows one question at a time, and waits indefinitely. The user can skip a question or stop the task. Navigating away is not a response and must not resolve the request.

V1 provides one to three questions, single selection, a client-provided Other answer, optional notes, previous/next navigation, and durable read-only question/answer history. Asking is non-mutating and available in Ask, Plan, and Execute, under every permission mode. An answer never grants a tool permission or changes the task mode.

Excluded: multi-selection, Markdown/HTML option previews, number widgets, secret/credential entry, URL elicitation, automatic timeout, background auto-answer, and a new settings surface. Questions are ordinary task choices; credential collection continues through the existing credential flow.

The reference is the public [Codex handler](https://github.com/openai/codex/blob/ce0323f2f2d3b25e82806cd555237da3db4e1897/codex-rs/core/src/tools/handlers/request_user_input.rs) and its [schema](https://github.com/openai/codex/blob/ce0323f2f2d3b25e82806cd555237da3db4e1897/codex-rs/core/src/tools/handlers/request_user_input_spec.rs): a suspended tool call, client-supplied Other, and answers keyed by question ID. Vega owns its UI and mode policy; this is not a copy of private desktop code or styling.

## 2. Acceptance matrix

All automated rows run in GitHub Actions. No local Vega execution, compilation, or tests are allowed. Status for every row below is pending.

| Case | Preconditions and action | Observable result | Evidence |
|---|---|---|---|
| Normal answer | Native agent calls the tool; user selects an option and submits | Panel appears above composer; exactly one matching tool result reaches the next provider request; stored question and selected label match | Runtime mock-provider + conversation storage integration |
| Waiting | Valid request receives no user action; advance simulated time beyond permission timeout | No empty answer, timeout, subsequent provider call, or successful terminal result; Stop stays available | Queue/runtime test |
| Pagination | Three questions; answer first, move forward/back, edit, skip one, submit | Drafts survive page changes; answer map uses stable IDs; skipped question has an empty answer array | GPUI + runtime |
| Other and note | Enter a custom answer; separately select an option and add a note | Custom/note text round-trips with the agreed prefix; recommendation display does not alter the original option label | GPUI + codec |
| Input validation | Empty/excess questions, duplicate IDs/text/labels, missing fields, invalid option counts, unexpected fields, oversized payload | Recoverable tool error; no panel, no waiting latch, no raw input in diagnostics; model can correct its call | Runtime parameterized cases |
| Response validation | Unknown question ID/option, multiple selected labels, oversized answer | Request remains pending and editable; invalid answer is not delivered or recorded as success | Queue + GPUI |
| Skip | User explicitly skips current question, including every question | Empty arrays only for explicitly skipped questions; task continues once after final submission | GPUI + conversation |
| Stop/races | Stop before/after enqueue or race submit against cancellation; click submit twice | First terminal decision wins; no duplicate tool result, no stale action or request hang; cancellation does not become skipped success | Queue + runtime |
| Navigation | Leave a running chat for another chat/settings and return | Same question/page/draft remains pending; other chats do not receive it; no automatic focus theft | App/controller + GPUI |
| Failure | Persistence fails while finishing an answered call; worker/request owner disappears | No subsequent provider call and no success claim; existing actionable run failure is shown; responder is released | Existing persistence fault seam + queue |
| History | Finish answer/skip, reopen chat; separately hydrate interrupted/nonterminal call | Read-only human-readable question/answer history; interrupted remains interrupted; no resurrected responder | Conversation hydration + ToolCard |
| Mode and regression | Ask/Plan/Execute and each permission mode; ordinary tool approvals and message queue | Question available throughout; mutation permission rules unchanged; queued text is never treated as an answer | Runtime definitions + app guards |
| Mixed batch | Provider emits a question together with a write/bash/other call, in either order, or several question calls | Entire batch receives recoverable rejection; no question/permission hook or tool worker runs; provider must send one question call and observe its answer before choosing further tools | Runtime mock-provider |
| Accessibility/layout | Keyboard-only, IME composition, long text, small window, light/dark | Controls reachable, focus visible, text input keys preserved, panel content scrolls without hiding submit/Stop | GPUI assertions + user manual acceptance |

## 3. Tool and answer contract

The sole advertised tool is `ask_user_question`. No additional alias schema is exposed. Its description asks for questions only when user input materially affects the task and prohibits using answers as permission approval.

A question must be the only tool call in its provider response. If a response contains a question and any other call (including a second question), reject the entire batch without invoking tools or hooks. Return a bounded recoverable validation result instructing the model to submit one question call, combining up to three questions where appropriate, then wait for its answer before selecting subsequent tools. Use the existing content-free mixed-batch audit path; the rejection cannot fall through to executing preselected mutations.

Input is an object with exactly `questions`. Each of one to three questions has exactly `id`, `header`, `question`, and `options`; each option has exactly `label` and `description`. IDs are unique nonempty snake_case strings, up to 64 ASCII characters; headers are nonempty and at most 12 Unicode characters. Question text is nonempty and unique. Each question has two to four distinct nonempty option labels. The model does not add Other; the client always supplies it. A recommended label may end in ` (Recommended)`; the UI displays a badge while preserving the complete original label for answers.

Input JSON and serialized response are each limited to 64 KiB; reject overflow rather than truncate structured JSON. This is a task-specific resource boundary, not a request to change general tool limits. Other and note text must be nonempty after trimming when used; preserve meaningful original text. Unsupported fields, including secret and preview flags, fail validation rather than being silently ignored. Validation errors identify the field/rule without reflecting private input.

Output follows the Codex-style shape:

```json
{"answers":{"approach":{"answers":["Option A (Recommended)","user_note: Keep the existing layout"]},"scope":{"answers":[]}}}
```

A selected option contributes its exact source label. A custom answer or optional note contributes one `user_note: <text>` entry. A single-choice question cannot return several option labels. Every requested ID appears exactly once; a skipped question has an empty array. Unknown IDs/options or invented answers are rejected before consuming the responder. There is no implicit default answer. The UI may focus the first option for navigation but must not submit it without an explicit user action.

## 4. Runtime, ownership, and persistence

Use a headless `RuntimeUserQuestionHook` separate from permissions, injected through `RuntimeToolConfig`. Conversation exposes the shared view types through `vega_conversation::types`; it may re-export the underlying runtime structs as existing architecture requires, without creating a runtime-to-conversation dependency.

Use a dedicated question queue with a first-wins response latch, cancellation token, and request ownership. A question lease cannot inherit permission's ten-minute timeout. Cancellation, actual owner destruction, and run failure release the wait; a mere render, page change, chat navigation, or settings navigation does not. A missing/unavailable hook returns an explicit tool error, never a successful empty answer. Queue requests retain the exact call identity and are bounded to the active runtime call; sequential calls receive independent latches.

The tool follows existing Proposed → Approved → Running → Finished persistence semantics. Approved here means the non-mutating call can run, not a user grant. The UI response remains attached to the original thread/run/call. Repeated or stale submissions cannot affect a later call. Pending questions block automatic steering/delivery where the existing permission wait is a guard; unrelated queued text does not answer the question. Preserve the existing user-controlled queue/Stop behavior.

Persist the validated question input and complete structured answer through the existing `tool_calls.input_json` / `output_text` and acknowledged event pipeline. Add the tool to the strict input/result/recovery allowlists; do not bypass them. No new table or migration is needed. An answered call is fed back to the provider only after its terminal persistence acknowledgement. If that write fails, the run fails using the existing persistence error path and cannot continue on an unrecorded answer.

Use live queue state to enable answering. Historical rows only create read-only projections. A recovered nonterminal question is interrupted, never a live prompt. Cancellation and errors are not normalized into an empty successful answer. Preserve existing credential redaction, replay identity, and terminal-history rules; never emit question/answer contents in tracing or Debug output.

## 5. Interaction

The panel is part of the composer area, immediately above its normal text surface and inside the same width/occlusion layout. Use shared theme/layout/typography tokens and existing input/button primitives. The panel must not overlay messages or force the message list to the bottom. It must remain visible while required, outside collapsible tool/activity details.

Show current question text, options with their supplied descriptions, Other, page position, and the relevant actions. Product controls use the existing English UI convention; model and user text stays unchanged. No explanatory implementation subtitle, permission-style warning, permanent success banner, or countdown is added.

`Back` revisits prior questions without losing drafts. `Next` advances when the current page has an answer; `Skip` explicitly marks only that question unanswered and advances (or submits the batch on the last page). The last primary action is `Submit`. Earlier skipped answers may be edited before submission. Unanswered questions cannot be silently submitted: each must contain an answer or an explicit skip. A custom answer and optional note use one text input; selecting Other makes that field required. For a selected option, an `Add note` action reveals the optional input.

Submission disables repeat actions while the terminal event is acknowledged. Do not claim success when a local channel send merely succeeds. Retain submitted values while awaiting the terminal event; show a local retryable validation error if the latch is still live, otherwise the actual interrupted/failed state. On durable completion, replace the live panel with the read-only question/answer result in the tool history. History displays skipped, interrupted, and failed states truthfully and never includes live buttons.

Keep composer draft text, queued messages, and Stop intact. No focus is stolen when the question arrives. Tab reaches the panel controls; focus styling is visible. In the panel's option-navigation context, arrows move, number keys choose, and Enter advances/submits. Text input and IME composition retain their own key handling. Escape returns focus to the composer without skipping or cancelling; Stop remains the explicit task-cancellation action. Long question content scrolls locally and cannot hide panel actions or the existing task Stop control.

## 6. Implementation plan and ownership

Only the dedicated implementation subagent writes code. The main Codex agent owns this spec, review, integration, PR, and CI. No new dependencies, unrelated refactors, code comments, or non-test unwrap/expect are introduced.

1. Runtime: add `agent/user_questions.rs`, strict schema and codecs, hook and cancellable execution. Update `agent/mod.rs`, `tools_exec.rs`, and required `loop_.rs` wiring. Keep standard mutation/permission branches intact.
2. Conversation: add the question facade in `types/`, queue in `agent/`, and hook injection through `PersistenceActorConfig` / `entry.rs`. Update `types/events.rs`, `types/tool_calls.rs`, and `agent/pipeline.rs` strict proposal/result/replay/hydration handling. Reuse acknowledged persistence.
3. UI: add a dedicated question panel; attach queue/listener/entity to `ConversationStream`; render above composer using its measured layout. Add read-only question projection to `ToolCard`. Register scoped keyboard actions and use existing text input facilities.
4. App: wire the stream question queue through `app_agent.rs` and `window/agent.rs` config construction; include pending questions in relevant automatic queue/steer guards and terminal cleanup. Preserve the live-stream cache across navigation.
5. Add production-root regression coverage from §2. Review exact changed surfaces and all strict matches, then static format/diff checks. Do not expand public production API solely for tests.

If the frozen contract conflicts with an actual API, report the concrete contradiction before changing the design. More than one writer must not edit the same implementation scope.

## 7. Validation, manual acceptance, rollback

Allowed locally: `git diff --check`, `cargo fmt --all -- --check`, focused source inspection. Required cloud command is the existing PR gate: `cargo clippy --workspace --all-targets -- -D warnings` and `cargo nextest run --workspace`, plus formatting. Relevant test packages: `vega_runtime`, `vega_conversation`, `vega_ui`, and `vega`. No real external-service/process/application E2E is added. Record logs against the actual PR head and query CI no more frequently than every five minutes unless notified.

Manual acceptance after merge: in a Native task ask for two concrete choices; answer one with an option/note and the other with Other; navigate away/back before submitting; verify the continued response and reopened history. Repeat explicit skip and Stop. Check keyboard, IME, narrow window, and both themes. Real provider/tool selection and native focus remain user acceptance items.

Rollback is a PR reverting this card's code and schema registration; existing generic tool history remains readable. No destructive data migration is involved. Merge only after the latest head passes `check (fmt, clippy, test)`. Keep Issue open, set In review after merge, and retain task branch/worktree until the user explicitly passes manual acceptance.

## 8. Change record

- 2026-10-17 v1.0: user clarified Vega-native scope and chose composer pagination with indefinite waiting. Codex ACP priority from the earlier Issue research is not part of this implementation contract.
- 2026-10-17 v1.1: code review found the existing loop executes a response's prepared calls serially. Require questions to occupy their own tool batch so no preselected action executes before the model observes the user's answer.
