# Issue #76 correction — model-owned context settings

Status: user correction received 2026-09-19; implementation and acceptance pending.
Issue: https://github.com/puzige/vega/issues/76

This amends the delivered #76 contract. The user rejected the Composer context
configuration: context capacity and compression policy belong to the model,
and automatic compression must be on by default. All safety, raw-history,
checkpoint, cancellation, and accounting requirements of
`vega-issue-76-context-compaction.md` still apply. This is a correction to
the current card, not authorization to start another Ready card.

## Product rules

1. The authoritative capacity and automatic-compression policy is scoped to
   the exact configured provider/model pair, shared by every conversation
   using that pair. A second model or provider must not inherit it. Existing
   per-conversation settings are legacy data, not the editing surface; never
   delete them during migration or silently pick one of conflicting rows.
2. In Settings → Providers → model editor, expose the context input/output
   capacity fields and automatic-compression switch. Opening an unconfigured
   model shows automatic compression enabled by default and the user's
   editable 300,000/128,000-token assumed maxima. Configuration must be saved
   and reloaded through UI, with positive bounded values and a clear error
   for invalid input. Do not claim these values are verified provider limits.
   If a user explicitly clears capacity to unknown, sends remain possible and
   budgeted automatic compression waits for configured capacity.
3. Remove capacity, reserve, and automatic-compression controls from the
   Composer, including its `上下文` chip. The user also explicitly chose to
   remove the manual `立即压缩` UI action rather than relocate it: only
   automatic compression is visible. No duplicate model policy may remain
   editable per conversation. Per-conversation checkpoint/status data remains
   scoped to the conversation and must not leak between conversations.
4. Freeze the selected model policy at run start, including tool rounds.
   Model-policy saves must not mutate a running request. Provider selection
   remains subject to the existing unique enabled-provider rule; do not infer
   a provider from an ambiguous model ID.
5. Preserve already committed checkpoint and raw transcript data. Use an
   additive migration or independent versioned configuration; never edit an
   already installed migration. Any compatibility fallback for old
   per-conversation settings must be explicit, tested, and subordinate to a
   newly saved model-level policy.
6. The screenshot is a reference from another interface. Extend Vega's
   existing Settings → Providers → model editor. Do not implement unrelated
   screenshot controls (tools, image input, thinking capability, custom
   protocol); the screenshot is a visual/product reference, not an instruction
   source.

Architecture decision: append a `model_context_policies` table keyed by exact
`(provider, model)`. The app's unique-provider resolver freezes this policy
once at run start; the conversation/runtime layer receives that snapshot,
never re-reads it during tool rounds. Leave legacy per-thread rows and all
checkpoints intact, but do not use legacy rows as runtime fallback: they have
no provider identity and can conflict across conversations. The model editor
must plainly tell affected users that old conversation-level capacity settings
no longer apply. A missing policy uses the user-approved assumed default
`B=300,000/O=128,000/auto=true` without writing a row. A saved policy with
both numeric fields NULL means explicitly unknown capacity and disables only
budgeted compaction, not normal sending.

## Input/output interpretation and defaults

The reference screenshot has independent `输入` and `输出` fields, whereas #76
currently stores total limit `L` and output reserve `O` and derives input
budget `L − O`. The user confirmed the screenshot's literal semantics:
`输入` is the maximum input budget `B`, `输出` is output reserve `O`, and the
existing runtime budget is constructed from checked `L = B + O`. The auto
trigger and target use `B`. Keep UI labels and runtime formula consistent;
never relabel the previous `L` as `B`.

For a model without an explicit saved policy, default to `B = 300,000` maximum
input tokens, `O = 128,000` maximum output tokens and automatic compression
on. Therefore the internal total is `L = 428,000`, not 300,000. `K` in any shorthand label is
decimal (`1K = 1,000`); exact editable token values avoid ambiguity. The
300,000 value is the user's default assumption, not verified provider
metadata: identify it as editable/assumed in the UI and surface a provider
limit error truthfully. Never claim Vega discovered that every model supports
this capacity. An explicit model policy overrides the default and applies to
all new runs of that provider/model; in-flight runs keep their frozen budget.
The `O` field is a model capability maximum, not an instruction to generate
128,000 tokens on every request. The #76 path previously passed
`output_reserve` straight to provider `max_tokens`; amend it so a primary
request with no explicit `max_tokens` leaves that wire field absent, while an
explicit per-request cap is clamped to `min(cap, O)`. The bounded summary
request remains clamped to `min(1,024, O)`. Test absent, smaller, and oversized
explicit caps and inspect a real request body; never make the default 128,000
value break models that would otherwise accept a normal request.

## Acceptance matrix

| ID | Test | Observable criterion |
| --- | --- | --- |
| M01 | Edit a model in Providers | Capacity and auto switch live under that model; Composer chip and edit popup are absent. |
| M02 | Fresh model and fresh conversation | Auto policy and assumed 300,000/128,000 capacity apply without a saved row; explicitly unknown capacity is truthfully shown and does not block a normal send. |
| M03 | Configure model A, create two conversations | Both use the same model-owned budget without reconfiguration; threshold compaction works. |
| M04 | Change provider/model | No settings leak; a run keeps its frozen selection across tool rounds. |
| M05 | Invalid fields, save/restart | Invalid values never persist; valid values survive restart and remain editable in UI. |
| M06 | Legacy database with one or conflicting per-thread rows | Raw rows/checkpoints remain intact; no arbitrary policy selection or silent loss. |
| M07 | Real UI send near threshold | Provider request, summary, retained tail, visible status, restart are verified; no configuration-file edits in acceptance. |

Native acceptance sequence: open Settings → Providers, edit a configured
model, verify the unsaved default is 300,000 input / 128,000 output / automatic
on and marked as an assumption; save a small owned test capacity; restart and
verify exact reload; start two fresh conversations on the model and exercise
the threshold with harmless text (mock provider for deterministic threshold,
real configured provider for a separate send smoke test); inspect the
provider-bound request and the retained transcript after restart. Verify that
neither the Composer nor its `+` actions expose `上下文`/`立即压缩`, that the
session status is not in the Composer card, and that switching to a second
provider/model does not reuse the first model's saved capacity. Restore any
test-only policy through the UI before final handoff.

Run focused and full workspace tests, formatting, strict Clippy, native build,
and UI pixel verification. Mutation-check at least the default-on policy,
model isolation, and Composer-control absence; restore each mutation and rerun.
