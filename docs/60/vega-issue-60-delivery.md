# Issue 60 executor delivery

Contract: [R1–R5](vega-issue-60-unpriced-chat.md). Status: executor gates passed;
workspace gates, native acceptance and integration remain main-agent owned.

## Freeze and final executor gates

- Verified UTC 2026-09-19T02:30:13Z / local 2026-09-19 10:30:13 +08:00.
- Source-content hash (`git ls-files -m -o --exclude-standard crates Cargo.lock |
  sort | xargs shasum -a 256 | shasum -a 256`):
  `3cbdf2cf95d5885c66aa9e0bc8b5fdd74bece403d98ec0d5f16625def7f9ddc9`.
- Source frozen and executor Cargo ownership returned after these commands.

| Exact command | Result | Log / SHA-256 |
| --- | --- | --- |
| `scripts/cargo-lock.sh test -p vega automatic_title_production_worker_records_real_http_wire -- --test-threads=1` | exit 0; 1 passed, 0 failed, 160 filtered; 0.23s | `http-isolated-recheck.log` / `576863b734902de7146646165fd991db2980130b19e2460c71d5f691f2da63f4` |
| `scripts/cargo-lock.sh test -p vega -- --test-threads=1` | exit 0; 161 passed, 0 failed; 50.16s | `app-recheck.log` / `024ed3f9e7049d84974dc974faa75b5d2db5b759712042936e5b7b3434ab9ccf` |
| `scripts/cargo-lock.sh fmt --all -- --check` | exit 0 | `fmt-recheck.log` / `a62e43dcbf31d1e88921dcaa3c2c6f849a08d61522d991b5f4dcaa97579d3962` |
| `scripts/cargo-lock.sh clippy -p vega --all-targets -- -D warnings` | exit 0; 14.46s | `clippy-recheck.log` / `e2d4db8d7ac11ca7032783d05234a574708d10fcff9025e3800c5a65b469d9e7` |

Clippy reports the existing dependency `block v0.1.6` future-Rust compatibility
notice, not a failing project lint. Full workspace/build/native gates are not
claimed by this executor report.

## Scope

- `app_agent.rs`: optional committed pricing snapshot; Ready uses authority,
  Saving uses previous; other states or missing exact model return None.
- `window/agent.rs`: remove pricing-only first/durable/approved-plan run gates;
  pass the same optional snapshot to runtime and meter.
- `window/session.rs`: configured, uniquely resolved model membership and
  selection no longer depend on prices; non-price owner/security gates retained.
- `conversation_stream/{core,content}.rs`: update model-catalog documentation.
- `tests/{pricing,r69}.rs`: production-path optional-price regressions and
  explicitly superseded price-refusal expectations.

## Evidence and first failures

Raw logs remain in local evidence directory `vega-issue60-focused.7T8O3J`.
No existing failure log was overwritten.

1. `p1-p2-before.log`: 0 passed / 2 failed before production changes; missing
   configured unpriced model and forced Pricing route reproduced.
2. `p1-p2-after.log`: 1 passed / 1 failed; new assertion used `completed` instead
   of protocol status `done`. Corrected expectation, not production behavior.
3. `matrix-first.log`: test compilation failed because `PricingSaveOutcome`
   lacked qualification; fixed test reference. `matrix-rerun.log`: 5 passed.
4. `app-first.log`: intermediate application suite 158 passed.
5. `app-final.log`: 159 passed / 2 failed. P7 expected 1500 instead of 15;
   S7 currency contract defines each stored Microcents unit as 1/1,000,000 USD.
   Fixture $1/million × (10 input + 5 output) = $0.000015 = 15 stored units.
   Only this new expectation was corrected; production pricing was untouched.
   The other failure was the existing #65 loopback HTTP fixture's socket read
   returning WouldBlock. That source was not changed. Isolated original test
   subsequently passed 1/1 in 0.23 seconds (`http-isolated-recheck.log`).

## Matrix coverage boundaries

| Cases | Evidence | Classification |
| --- | --- | --- |
| P1/P2/P6 | `issue60_configured_unpriced_model_is_selectable`, `issue60_unpriced_first_submit_reaches_provider`: draft and persisted model selection, same durable ID, exact body/status, streaming unknown cost, 21 actual tokens, NULL provenance, reopened aggregate/summary | Production app/controller with owned configuration and mock network |
| P3 | `issue60_unpriced_approved_plan_executes`: actual approved-plan start | Production handler with owned plan fixture |
| P4/P9 | `issue60_unavailable_pricing_states_allow_chat_and_title`: three controller states, three body requests plus one title, malformed bytes retained | Production send chain with injected controller state; not native loading-state interaction |
| P5 | `issue60_pricing_snapshot_uses_committed_ready_or_saving_previous_only`: real service authority, dirty draft/previous snapshot and immutable later-save result | Focused controller/service test; not full request during concurrent save |
| P7 | `a7_unpriced_first_submit_then_optional_pricing_preserves_task_identity`: UI Settings mutation then second send, exact priced row 15, prior row remains unpriced, mixed total unavailable | Production app/controller |
| P8 | Existing app regressions for provider readiness, credential failure, permission/mode, owner and route fences | Application regression suite; no new permission bypass |
| N1/N2 | Reserved for main's installed application evidence | NOT RUN by executor |

## Residuals

- No dependencies, schema, user configuration, credential or production pricing
  formulas changed. Legacy unknown-price storage is retained; no historical repricing.
- Mock-provider evidence does not prove a real remote model response.
- Existing #65 HTTP fixture showed one transient socket-read failure; retain
  first failure and rerun results rather than claiming no instability occurred.
- Not committed, pushed, installed or merged by this executor.
