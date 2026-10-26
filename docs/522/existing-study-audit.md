# Existing Harness² study: frozen comparison audit

Status: existing evidence revalidated and independently reviewed on 2026-10-23; no new experiment or model request. This is the already completed `search-03` continuation, generation `resume-20261022-01`, not a rerun using the new #556 runtime.

## Provenance and completeness

The continuation completed at 2026-10-22T02:54:46.473Z. Its controller PID is no longer live and its process-exit receipt matches the generation. Search status is `partial`: 64 logical optimization executions, 65 physical attempts, 61 delivered logical executions and two failed physical attempts. It nevertheless produced eligible candidates under the previously frozen selection policy. Candidate selection and frozen holdout are distinct from a claim that all search tasks succeeded.

The selected global candidate is `runbook-current / agg1`, 510 strategy bytes, harness SHA256 `07c219190dbed2c21eef78e0ee73708d09ed5198efc2ada461f098aea1ae964e`. The selector ranked 9 eligible entries from the original 16-task search. Selection policy SHA256 is `8a4224f8e49560ccb3ffa3e3757488dfef001757e5474e2d6b96bb84704b2048`. The three approved skill bodies and original task split remain fixed. Holdout did not edit or search candidates.

The original 8 holdout tasks × 2 arms × 3 repetitions produced all 48 distinct logical run IDs and 49 physical attempts. Both arms used model `gpt-6.1-sol`, reasoning effort `max`, as fixed in the existing plan's `identity.model`. The predeclared alternating arm order, plan identity, generation and current frozen source/library/model/policy hashes match. Plan ID: `a42c682cc491001483ef0941014b7e97780f729d817f199a47ec449c5625ff52`.

Read-only verification used the existing controller's `read_run` and `freeze_plan`, plus the existing continuation worker's `read_attempt` and retry-inclusive `summarize`. Every canonical search/holdout manifest, native record, artifact and archived attempt validated, and recomputed summaries matched the reports. No launcher, provider, CPA query or notification was invoked. The existing holdout report SHA256 is `0798cdc27383235307a224b4020a07038370991c8200b0ede104e84e272b6603`; controller SHA256 is `57c7cde7d9f0e0db055a4e38dcfc1ee87966d0f647eaf9be6c9dd329d4b8222d`; continuation worker SHA256 is `20180208656f5f7a28e3087b831db2c712d0b9f59d7667ff294707f0868d2b6c`.

The non-implementer independently repeated the original-file validation and aggregate calculation, rebuilt all 48 input requests against the frozen plan, recomputed the 9-entry candidate ranking without holdout input, and obtained the same selected candidate. Controller/worker hashes match the original producer. This review validates the recorded comparison, not unobserved provider behavior or a supplier invoice.

## Holdout results, including retry overhead

| Metric | Baseline | Frozen candidate |
| --- | ---: | ---: |
| Delivered logical trials | 24 / 24 | 24 / 24 |
| Physical attempts | 25 | 24 |
| Failed physical attempts | 1 | 0 |
| Model requests | 150 | 132 |
| Tool executions | 125 | 108 |
| Input tokens | 298,985 | 262,386 |
| Output tokens | 11,408 | 13,104 |
| Trial latency sum, ms | 1,174,604 | 1,135,261 |
| Per-arm median trial latency, ms | 46,180 | 46,899 |
| Recorded retry backoff, ms | 2,070 | 0 |
| Total native configured-price cost, USD | Unknown | 0.414075 |
| Known native cost subtotal, USD | 0.432769 | 0.414075 |
| Attempts with unknown cost | 1 | 0 |

The latency sums include the original failed attempt and recorded backoff. The frozen arm is lower by 39,343 ms across 24 logical trials; this is a sum of trial durations, not whole-study wall time. It is faster in 17 of the 24 matched task/repetition pairs, with median paired difference −1,716.5 ms. The separate per-arm median is higher for frozen by 719 ms. These different summaries demonstrate that a single global speedup claim would conceal task-dependent outcomes.

All 24 pairs, including the 7 slower frozen outcomes, are retained in [paired-existing.csv](paired-existing.csv), SHA256 `eee2e34372e81b59cd2d7b1892a6198dd3d602b8612856ac629a06c3255c009f`. Differences are frozen minus baseline; every latency includes that logical trial's physical attempts and backoff.

| Task | Baseline mean of 3, ms | Frozen mean of 3, ms | Frozen faster pairs |
| --- | ---: | ---: | ---: |
| stock-threshold | 59,090.0 | 47,909.0 | 3 / 3 |
| coverage-window | 43,726.0 | 36,343.0 | 3 / 3 |
| manifest-target | 48,893.3 | 42,425.0 | 3 / 3 |
| quoted-token | 55,792.7 | 52,757.0 | 3 / 3 |
| empty-primary | 57,022.0 | 53,452.0 | 2 / 3 |
| missing-field | 42,045.3 | 57,088.3 | 0 / 3 |
| table-zero | 41,614.7 | 41,554.0 | 2 / 3 |
| status-ready | 43,350.7 | 46,892.0 | 1 / 3 |

## Time and cost boundaries

The original namespace started at 2026-10-21T16:48:19.973077+00:00. Original start to terminal is **36,386,499.923 ms (10h06m26.499923s)**, including the overnight stopped gap. The resumed interval began at 2026-10-22T01:36:08.807Z and lasted **4,717,666 ms (1h18m37.666s)**. The resumed search report interval is 2,399,922 ms, excluding the subsequent freeze; the holdout report interval is 2,316,898 ms. These are phase intervals, not substitutes for the original-start wall clock. Earlier probes/search-01/search-02 and engineering effort are outside these namespace intervals and must not be treated as free.

Search native configured-price subtotal is USD 1.155866, with two unknown-cost attempts; total search cost remains unknown, including incomplete channel/search accounting. Baseline holdout cost is also unknown. Known subtotals are not complete bills, and neither failed request nor missing usage is assigned zero. Full search-cost amortization cannot be calculated from these records. Search cost, study/engineering wall time and subsequent per-task savings must stay separate.

## Supported conclusion

This existing microtask comparison is complete and reusable. It shows unchanged observed holdout delivery, fewer requests/tools/input tokens and mixed, modest latency differences under one fixed model/effort and small task set. It does not establish a general speedup, statistical generality, provider-independent causality, or faster desktop delegation/review/CI delivery. Both arms used the historical native microtask worker, which has no desktop supervisor handoff path.

Do not start another namespace to obtain a more favorable result, drop the failed attempt, change the candidate after viewing holdout, or automatically adopt it into daily Vega settings. The new #556 runtime still needs its own correctness evidence and representative delivery measurements under the [full-clock contract](../556/stage-handoff-recovery.md#speed-evaluation-contract). No early interruption is credited as delivery.
