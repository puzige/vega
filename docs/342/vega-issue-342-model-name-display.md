# Issue #342 — The Composer model trigger must show the full model name

Status: frozen for implementation, 2026-10-13.

## User decision and evidence

The user reported that the model name cannot be shown in full, with two screenshots: the Composer footer showing `deepseek-v4.1-f…`, and the tier card whose model row shows the same model untruncated.

The user was asked directly whether the trigger must always show the complete model name, and answered **yes**. The same question about the trigger's `选择强度` label while the tier slider is open was explicitly ruled **out of scope** by the user ("第二个不用处理"), so this task changes no label rule.

Issue: https://github.com/puzige/vega/issues/342

The implementation baseline is current master `c2fef5e`. The screenshots do not identify a build version; they are evidence of the defect, not of an independently verified installed build.

## Root cause

`render_model_selector` (`crates/vega_ui/src/conversation_stream/render.rs`) caps the trigger box with

```rust
.max_w(px(if self.compact_workspace { 98. } else { 150. }))
```

and truncates the label inside it. The cap is a fixed pixel budget unrelated to the model name: with `px_2` horizontal padding the content box is 134px in the ordinary layout and 82px when the right workspace panel is open, while the label is whatever the provider calls the model.

The user's screenshots pin this exactly. In the trigger capture the text run ends at 133 logical px (measured 266 device px at 2×) — one pixel inside the 134px content box, which is why the ellipsis appears after `deepseek-v4.1-f` and not earlier. The tier card's model row renders the same name in full because that surface bounds its text by the card's content band instead of a fixed trigger budget.

The cap is not frozen by any spec: it arrived with the initial `R1-R17` UI integration commit `7e39798`, and no `docs/` spec states a trigger width for the model selector.

## Contract

1. The model trigger shows the selected model's **complete** name whenever the footer row has room for it. The trigger's width follows the label's natural width; it is not capped by a constant.
2. The trigger still yields when the row is genuinely short of room. Truncation remains possible and must happen only as a last resort under real width pressure — never because of a fixed pixel budget. The `+`, permission/backend status, context-usage, and send/stop controls keep their current sizes and order, and no control may be pushed outside the Composer card.
3. Truncation, when it does occur, still ellipsises inside the trigger's own box: no painted text may spill past the trigger's bounds.
4. The compact-workspace layout (`compact_workspace == true`, the right workspace panel open) follows the same rule as the ordinary layout. The two layouts differ only in how much room they have, not in whether the name is capped by a constant.
5. The label rule itself is unchanged: the tier-slider state still reads `选择强度`, the pending-save state still reads `保存中…`, and an empty model still reads `模型`. This task changes only how much room the trigger takes, not what it says.
6. Picker behaviour is unchanged: the trigger still opens the tier slider or the model list per the existing level rules, the layers still hang from the trigger's own wrapper (`bottom: 100%` plus `COMPOSER_PICKER_TRIGGER_GAP`, `right_0()`), and both keep their existing widths and height bound.
7. No new dependency, token, database migration, preference, provider/ACP runtime behaviour, or accessibility regression. The trigger keeps its `composer-model` selector and its `aria_label`, which continues to carry the full, untruncated label so assistive technology is unaffected by any visual truncation.

## Acceptance matrix

| ID | Production path and action | Expected result | Evidence |
|---|---|---|---|
| A1 | Render the Composer with a model name longer than the old 150px budget and measure the trigger's own bounds against the shaped width of the full label | The trigger is wide enough for the complete name; the label is not truncated | GPUI production render regression (`issue342_trigger_shows_the_full_name_in_both_layouts`), run by cloud PR Check |
| A2 | Same as A1 with the right workspace panel open (`compact_workspace == true`) | The same full-name rule holds; the old 98px budget does not bound it | Same regression, compact half |
| A3 | Render a name far longer than the whole footer row in a narrow window | The trigger still ellipsises inside its own box and every footer control stays inside the Composer card | Scoped GPUI render/geometry regression |
| A4 | Render the ordinary and compact layouts in Light and Dark | Both themes follow the same width rule; no theme-specific branch appears | Scoped GPUI render coverage; the trigger's sizing reads no theme value |
| A5 | Trigger the tier-slider, pending-save and empty-model states | The labels are still `选择强度`, `保存中…` and `模型`; only the box width changed | Existing `model_trigger_label` coverage plus scoped render assertions |
| A6 | Open the picker in both layouts and both levels | Layers keep their existing width, height bound and trigger anchoring | Existing R59/R61/R64 picker regressions |
| A7 | After merge, inspect the official app in the user's affected conversation | The trigger shows `deepseek-v4.1-flash` in full | User manual acceptance, NOT RUN until supplied |

## Implementation plan

1. Add the focused failing production regressions first (A1–A3), so the defect is demonstrated before the fix.
2. Replace the fixed `max_w` budget in `render_model_selector` with width that follows the label, keeping `min_w_0`, `flex_shrink_1` and `truncate` so real width pressure still yields instead of overflowing the card.
3. Keep the label projection, the picker layers and the `aria_label` untouched; touch only the trigger's own sizing.
4. Run the affected card-specific and nearby regressions locally, plus formatting and diff checks — subject to the 2026-10-13 local-validation ruling (Vega runs no local compile, build or packaging; only fast, low-CPU checks are allowed locally). A regression that would trigger a cold or heavy compile is left to cloud PR Check and reported as not run locally rather than as passed.
5. Review the final diff, open one PR referencing this Issue without automatic-close keywords, wait for the required cloud gate, merge, and move the card to `In review`. Await the user's manual acceptance before closing or cleaning this worktree.

## Boundaries and verification

Implementation belongs to a dedicated subagent in the isolated task worktree. Do not change unrelated cards, runtime config/credentials, the installed application, or user Git branches. Do not add code comments, dependencies, blanket test skips, weaker assertions, or local workspace-wide tests.

The new regressions in `crates/vega_ui/src/conversation_stream/tests/issue342_model_name_width.rs` were written with the fix already applied and are executed by cloud PR Check; no local compile or test run is performed for this card, so the red-before-fix demonstration is stated as a contract of the assertions rather than as a locally recorded run.

No spec freezes the removed cap, so no spec revision is needed beyond this document. The screenshots in the Issue are the user's own evidence; no new native capture is required for the merge gate, and native screenshot geometry stays a separate evidence class from the automatic render tests.

## Revision record

- 2026-10-13: initial freeze. User answered "yes" to "must the trigger always show the full model name?" and ruled the `选择强度` label question out of scope.
- 2026-10-13: revised the evidence and implementation-plan wording to the 2026-10-13 local-validation ruling (no local compile/build/packaging; the card's regressions run in cloud PR Check), and named the regression functions in the acceptance matrix.
