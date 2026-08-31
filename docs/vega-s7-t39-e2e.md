# Vega S7-T39 E2E evidence

- Card: T39 Bounded stream estimate + Composer live counter (A10-02 / A10-05)
- Branch: `feat/s7-t39-stream-estimate` (from T38 tip `8d9685e`, tree-identical to squashed master `b96fcef`)
- Evidence time: 2026-08-31 (local session; raw log refreshed on every re-run)
- Raw command log: `/private/tmp/vega-s7-t39-workspace-test.log` (ephemeral; this file is the durable summary)
- Fixture scope: `MockProvider` scripts + `tempfile` data roots only. Zero real keys, zero network, zero real fees, zero writes to any real user data root.

## Design placement (S7-T39/C3/C4/C5)

| Concern | Location |
|---|---|
| Provisional estimate kernel (`ceil(chars/4)`, fixed cap `METER_PROVISIONAL_CHAR_CAP = 2^32`, checked everywhere) | `crates/vega_conversation/src/types.rs` — `ConversationMeter`, `MeterSnapshot`, `RunUsageEstimator` |
| In-place calibration on authoritative `UsageUpdated`; tool-boundary/finish/error/interrupt clearing; late-event latch | same `ConversationMeter::apply` |
| Run-start immutable estimator freeze | `crates/vega_conversation/src/pricing.rs` (`PricingAuthority::catalog`) + `crates/vega/src/main.rs` (`RunUsageEstimator::new` right after `agent_controller.begin`, passed into worker alongside run ownership) |
| C4 restart recovery (durable checked aggregate → counter baseline) | `crates/vega_conversation/src/threads.rs` (`thread_usage_seed` over `vega_store::token_usage::aggregate_by_thread`) wired once per route open in `crates/vega/src/main.rs` |
| Composer compact counter (`≈` / `—` / `US$`, k/M tokens, microcent-precision cost) | `crates/vega_ui/src/conversation_stream.rs` — `meter_snapshot()` rendered bottom-right of the Composer; event fence in `feed_meter` |

## E2E-REAL: estimate/calibration/fence/restart journey

Command:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
cargo test -p vega_conversation --test stream_estimate_e2e
```

Result: PASS — **14 passed; 0 failed** (`finished in 0.07s`). Coverage map to the card's acceptance list:

- ASCII/CJK/emoji scalar counting: `ascii_cjk_and_emoji_estimate_by_unicode_scalars` — `"abc"+"中文"+"🦀"` = 6 scalars → 2 tokens (byte counting would give 4; the test pins scalar semantics), plus the cap identity `2^32/4 = 2^30`.
- Exact-cap/+1/−1 rounding: `estimate_rounding_covers_exact_plus_one_and_minus_one` (7/8 chars → 2 tokens; 9 → 3).
- Empty delta silence: `empty_delta_produces_no_noise_and_no_provisional_flag`.
- Thinking/tool-payload exclusion (C3): `thinking_and_tool_payloads_never_enter_the_estimate`.
- Unpriced streaming: `unpriced_model_shows_dash_cost_while_streaming` (`≈100 tok · —`, never `$0`).
- Checked overflow fail-closed (C2/C5): `checked_overflow_latches_the_whole_counter_to_dash` (estimate overflow, usage overflow, cost overflow all latch `—`).
- Multi-round estimate→calibrate without double counting: `two_round_journey_calibrates_in_place_without_double_counting` — real 2-provider-call journey (one tool round) driven through `run_thread_task_with_pricing`; round-1 streaming shows `≈8 tok · ≈US$0.000016`, round-1 usage calibrates to exactly 110_000 tok / 120_000 µ¢, round-2 streams calibrated-base + fresh estimate, final reading `165.0k tok · US$0.18` with zero estimate residue.
- No-usage finish / provider error / cancellation clearing: `rounds_without_usage_clear_provisional_on_finish_error_and_interrupt` (three sub-journeys; error also asserts `run.failed`, cancel asserts `run.interrupted` and an `Interrupted` event).
- Unpriced model + restart seed fail-closed: `unpriced_model_journey_keeps_tokens_but_fails_cost_closed`.
- Late event / route fence: `late_events_after_terminal_cannot_move_the_counter` (duplicate usage, stray text, spurious interrupt and error after terminal leave the calibrated reading untouched; a new `MessageStarted` estimates from zero on the carried baseline).
- Restart recovery from durable aggregate (C4): `restart_restores_calibrated_baseline_from_checked_aggregate` — reopen the store, seed 165_000 tok / 180_000 µ¢, continue estimating on top; legacy mixed rows fail the cost closed (`165.0k tok · —`) and priced usage after an unpriced baseline cannot resurrect a cost.
- 1,000 delta/s zero-IO: `counter_updates_absorb_1_000_deltas_per_second_without_io` — 1,000 apply+snapshot cycles (exactly the per-delta render-path work) complete in the sub-millisecond range (whole test `finished in 0.01s`); any per-delta IO would exceed the 1-second assert by orders of magnitude.
- Frozen-selection journey guard (C3): `journey_events_carry_durable_thread_model_for_frozen_selection` — both requests carry the exact durable thread model; two priced rows total 28 µ¢.

## E2E-REAL: Composer counter projection (GPUI)

Command:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
cargo test -p vega_ui --lib conversation_stream::tests::composer_counter
```

Result: PASS — **2 passed; 0 failed**. Narrow render-layer tests per the T37 convention (assert state projection + display text, not pixels):

- `composer_counter_projects_estimate_calibration_and_fences`: visible `—` before any data; frozen estimator; CJK/emoji streaming `≈1 tok · ≈US$0.000002`; in-place calibration to `110 tok · US$0.00012`; late delta fenced; restart baseline `1.2M tok · US$0.18`.
- `composer_counter_error_path_clears_provisional`: controller failure (`apply_agent_error`) clears the provisional reading back to `0 tok · —`.

## Unit-level: types lib tests

Command:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
cargo test -p vega_conversation --lib
```

Result: PASS — **246 passed; 0 failed; 0 ignored** (includes the existing T38 usage suite; the meter's pure kernel is additionally exercised through the E2E file above).

## Gates

```sh
export PATH="$HOME/.cargo/bin:$PATH"
cargo fmt --all -- --check          # PASS (clean)
cargo clippy --all-targets -- -D warnings   # PASS (0 warnings)
cargo test --workspace              # PASS (729 passed; 0 failed across 25 suites)
cargo build --workspace             # PASS
git diff --check                    # PASS (no whitespace errors)
rg -n '#[0-9a-fA-F]{6}' crates/vega_ui   # 0 lines (baseline unchanged; no new hard-coded colors)
```

Full `cargo test --workspace` raw log: `/private/tmp/vega-s7-t39-workspace-test.log`.

## HUMAN PENDING (cannot be automated; explicitly not faked)

- **P2 (<16ms streaming first-pixel)**: measured via `cargo xtask bench` embed points and a real window walkthrough — the counter update path is checked-arithmetic only (no IO, no lock beyond the entity), but the end-to-end latency number requires the real window (S8 hardware bucket per ui-spec §5).
- **P3 zero re-layout of frozen conversation area**: the counter is a fixed bottom-right text row using `Typography::SIDEBAR`/`text_tertiary` tokens and the S3 delta-batching path is untouched, but the pixel-level frame comparison is a walkthrough item.
- **真实窗口观感**：compact counter 在 960×600 最小窗口与 Light/Dark 双主题下的真实字体渲染与金额对齐观感（ui-spec §6 走查项）。

## Deviations

- None beyond the frozen S7 SDD deviations (char-approximation instead of tiktoken; `US$` instead of `¥`). No new dependencies, no DDL, no provider-wire changes, no estimate persistence. `vega_conversation` re-exports `PricingCatalog`/`ModelPricingSpec`/`RateSpec`/`UsageCounts` so the app/UI receive the frozen capability through `vega_conversation` only.

## Continuity note (executor handoff)

This card was resumed mid-implementation: the previous executor run was interrupted by a provider outage after producing the 6-file implementation draft (meter state machine, estimator freeze, run wiring, counter UI, `thread_usage_seed`). The resuming executor kept that design, fixed the unfinished `run_agent_worker` pricing plumbing (compile error), wired the C4 restart seed into the route-open path (previously dead code), closed a C4 streaming-cost gap (the provisional cost segment now estimates even before the first authoritative usage, while unpriced history still fails closed), added the full E2E + GPUI test files, and produced this evidence document.
