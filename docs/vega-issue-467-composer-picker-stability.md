# Composer picker stability and empty-conversation context usage

Issue: https://github.com/puzige/vega/issues/467

## Authorization and scope

The user explicitly requested direct execution after the missing Codex handoff was reported. This task therefore proceeds in one coordinated session, with a dedicated subagent writing tests and implementation. This does not waive spec-first, cloud TEST-FIRST, local-execution restrictions, review, or manual acceptance.

Initial inspection baseline: `db111643`. Before authoring the test patch, the isolated branch was updated to `3a97f4d9` on `origin/master` (the intervening change only removes the prediction acceptance hint). Test/production baseline: `3a97f4d9`. Task branch: `feat/467-composer-picker` in an isolated worktree.

Scope is limited to the Composer model trigger's width while its label changes and to context-usage visibility before any conversation message exists. No provider/runtime, accounting, persistence, settings, permission, dependency, packaging, or installed-application changes.

## Acceptance matrix (frozen before implementation)

| Case | Preconditions / action | Observable expected result | Evidence |
|---|---|---|---|
| W1 | Select a model whose name is longer than the effort label; open its tier slider, move to the model list, close, and reopen | For the same model and available row width, the trigger keeps its width and horizontal bounds; adjacent context/permission controls do not move merely because the label changes | GPUI production-render geometry and real picker actions |
| W2 | Repeat with a model name shorter than the effort label | The label fits when room is available and the trigger does not jump when opening or closing | GPUI geometry |
| W3 | Repeat in ordinary and compact layouts, light and dark themes | The same sizing contract holds; no theme-specific width budget | GPUI geometry matrix |
| W4 | Use an exceptionally long model name in a narrow Composer | The trigger still yields to actual width pressure; footer controls remain within the card, and displayed text is clipped/ellipsised inside its own bounds | GPUI geometry; user visual check for clipping |
| W5 | Switch to another selected model, including one without reasoning tiers | Width follows the newly selected model rather than retaining an old model's arbitrary maximum; no-tier selection still opens the model list | Public state updates and picker actions |
| C1 | Open a fresh conversation; optionally edit a draft and load a context estimate/input limit before sending | No context-usage ring, hit target, focus target, tooltip, or reserved gap is rendered | GPUI production-render visibility and layout |
| C2 | Add the first accepted user message; open context usage | The existing usage indicator and tooltip become available even while accounting is unknown; existing estimate/unknown semantics remain unchanged | Public event path, GPUI geometry and actions |
| C3 | Restore a conversation with historical messages | Context usage is available without requiring a new submission | Public history hydration and GPUI visibility |
| C4 | Switch/reset from a populated conversation with an open context tooltip to a fresh empty conversation | The indicator and tooltip are absent; stale hover/open state cannot make an empty conversation show usage | Public conversation replacement and GPUI visibility |
| M1 | After integration, use the official application with the affected model | Opening/closing tier selection does not shift the controls; a new task has no usage ring, and it appears after sending | User manual acceptance, pending |

Geometry comparisons use the same viewport and model, not a hard-coded trigger width. The narrow-row case preserves the full-model-name contract from [the earlier model-name specification](vega-issue-342-model-name-display.md): names are not capped by a constant but yield when space is genuinely exhausted.

## Root causes and call path

- `crates/vega_ui/src/conversation_stream/render.rs::model_trigger_label` changes the trigger from the selected model name to `选择强度` for `ModelPickerLevel::Slider`.
- `render_model_selector` derives the flex element's intrinsic width from only that currently visible label. Its natural width therefore shrinks when the shorter label is rendered; the footer's right-aligned layout moves the preceding controls.
- `render_model_controls` always adds `render_context_usage_indicator` and only uses focus/hover/open state to decide tooltip visibility. It does not gate either surface on actual conversation content, so an empty task shows a meaningless usage control.
- Draft text and context projections do not constitute a sent conversation message. Existing message/history state, not a nonzero estimate or draft text, must determine whether the control exists. The boundary is accepted submission/history content, not the initial submission request. Preserve the Composer's existing empty-conversation classification; restored conversation entries also count when the current history window does not include its first user message.
- `composer_actions.rs::move_composer_focus` unconditionally includes `context_usage_focus`. The same visibility predicate must govern both rendering and this custom focus traversal, so hiding the ring does not leave an invisible keyboard stop.

## Minimal implementation plan

1. A dedicated subagent writes all behavioral tests for the matrix above while production files remain unchanged. Use the real `ConversationStream` render, existing public controller/history/picker boundaries, and GPUI test contexts; no source-string tests, mock rendering, new public production API, or real external application/process tests.
2. Review and commit the test-only patch with this specification. Open a draft PR against `master`, retain the production baseline and test patch, and let existing Actions establish effective RED. Unrelated toolchain, dependency, runner, compile, or test-harness errors are not behavioral RED and must be reported/corrected separately without weakening assertions.
3. After valid RED, freeze test targets/data/assertions and make the smallest production change in `render.rs`: reserve a content-derived label width for the model/effort states, retaining `min_w_0`, flex shrink, truncation, labels, accessibility names, picker anchoring, and existing theme/layout tokens. Do not impose a global fixed width or retain a previous model's width forever.
4. Gate both the context-usage indicator and tooltip on actual conversation content. An empty conversation renders neither the control nor a placeholder. Use the same predicate in `composer_actions.rs::move_composer_focus` to omit the hidden context control from custom keyboard traversal. Retain the existing accounting display, error semantics, keyboard access, and popover behavior once messages exist.
5. Review the diff and run only fast local formatting/difference/static checks. Push the implementation to the same PR and validate the same frozen tests plus latest-head required cloud check. Do not append tests after implementation to retroactively prove it. Any acceptance change starts a separately recorded test-first round.
6. Report actual RED/GREEN evidence and integration status. Keep the Issue open and retain the branch/worktree until user acceptance; code integration is not application installation or manual UI acceptance.

## Verification protocol

Local allowed checks: `git diff --check`, focused static inspection, `cargo fmt --all -- --check` (formatting only; no build/test/runtime). No local Vega execution, `cargo check`, Clippy, test/nextest, build, packaging, or native app restart.

Cloud execution uses the unchanged `.github/workflows/pr-check.yml`: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, the existing three Python checks, `cargo nextest list --workspace --message-format json`, and the normal nonempty `cargo nextest run --workspace`. Required check: `check (fmt, clippy, test)`. Query ongoing runs no more frequently than every five minutes unless completion/failure is notified or the user explicitly requests an immediate check.

RED/GREEN baseline, test commit/patch, original failure, implementation ordering, run/job links, counts if available, and latest-head gate are recorded in the PR and Issue. Not-yet-run checks are pending, not passed.

## Manual acceptance and rollback

Using the official cloud-built application, check the affected long model name and a short name in both workspace layouts. Open the slider, enter the model list, close/reopen, and change models. Check that context/permission controls remain stationary for the same selected model, and that an extremely long name cannot push send/stop outside the Composer. In a new task, edit a draft: no usage indicator should exist. Send a message: the normal indicator and keyboard/click tooltip should become available. Open an existing conversation, then create another new task: the old tooltip must not persist. Check light/dark appearance and normal keyboard/Escape behavior.

Rollback is to revert this task's isolated production change, preserving underlying conversation and accounting data. No data migration is involved.

## Revision record

- 2026-10-18: initial acceptance freeze and implementation plan under explicit user authorization to proceed directly. Cloud RED/GREEN and manual acceptance pending.
