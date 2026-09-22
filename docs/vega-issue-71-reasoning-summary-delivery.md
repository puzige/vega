# Issue 71 · Reasoning summary delivery

Task contract: [实时思考摘要标题](vega-issue-71-reasoning-summary.md), R1–R7 / A1–A8. Implementation is ready for architect integration; A9 native/live acceptance and cloud PR gate are pending. No application installation or live provider configuration was changed by the implementation agent.

## Freeze

- verified_at_utc: 2026-09-22T10:51:10.930651+00:00
- verified_at_local: 2026-09-22T18:51:10.930789+08:00
- branch: `feat/issue-71-reasoning-summary`; implementation baseline: `d1e3c75` (architect owns subsequent rebase and commit identity).
- tracked source diff SHA-256: `15415aebff5f66c532780060802405a8dfe273432f7823bdc142ac8887e5694a` (tracked `crates/` changes).
- new Responses module SHA-256: `e3ffef80957cebd25eb48492d158dfdf34f12282eb6b6f92c48a6fee936f9e18`.
- new controller HTTP test SHA-256: `b4f7fbf6710bc8a1452e6ec3cfe7cf61cb716d52354773cc9def5a57d76f8de7`.
- platform/toolchain: Darwin arm64; rustc 1.98.0 (88d9e12ae 2026-08-18) (Homebrew); cargo 1.98.0 (797e8a9bc 2026-08-05) (Homebrew); git version 2.55.0.
- Raw logs, command exit codes/durations and freeze metadata are retained in the task's external `vega-evidence/issue-71` directory, including `regression-results.json` and `source-freeze.json`.

## Resulting behavior and changed files

- `vega_ui/src/conversation_stream/thinking.rs`: separate thinking/summary accumulators and source priority; incremental bounded current-line preview, current ATX/standalone bold heading retention, rolling fragment fallback, UTF-8-safe 96-scalar single-line title, fence exclusion and original expanded body. Both sources share the existing delta/block/view budgets. Existing collapse handlers and message ownership/terminal fences remain intact.
- `vega_runtime/src/openai/responses.rs` (new), `openai/mod.rs`, `provider_check.rs`: explicit Responses request encoder and bounded SSE parser; stateless `store:false`, `reasoning.summary:auto`, tool and image input, explicit `strict:false` as well as `strict:true`, validated effort/off mapping, tool completion before release, usage/refusal, terminal-only success, and delta/done/item/completed snapshot reconciliation without duplicate text. Settings TestModel uses the selected transport. No cross-protocol retry.
- `vega_runtime/src/provider.rs`, `agent/mod.rs`, `agent/loop_.rs`: distinct SummaryDelta forwarding and opaque reasoning replay stored only in run messages; shared reasoning budget and cumulative opaque replay limit (1 MiB / 256 items per run; 256 KiB per response). Responses stream wire buffering is capped before SSE framing at 4 MiB; text reconciliation is capped at 1 MiB and 256 part keys.
- `vega_conversation/src/types/events.rs`, `types/meter.rs`, `types/provider_settings.rs`, `agent/events.rs`, `agent/persistence.rs`, `agent/compaction.rs`, `automatic_titles.rs`, `provider_settings.rs`: preserve summary provenance, exclude summaries from answer estimates and persistence, redact Debug, expose ProviderApi through the types facade, and wire the actual settings probe.
- `vega_store/src/config.rs`, `vega_ui/src/settings/state.rs`, `render_impl.rs`, `provider_management.rs`: backward-compatible provider API config; mouse/keyboard-selectable protocol controls, save/edit/reopen preservation and new-provider reset to Chat Completions. Protocol controls precede input fields so the pre-existing Name→Base URL→Models→Key→Save focus chain remains unchanged.
- `vega/src/app_agent.rs`, `window/context.rs`: selected API is applied to every production provider constructor (conversation, generated titles/commit use, manual context worker).
- Focused regression additions are in runtime openai/agent tests, conversation agent `responses.rs` and provider-settings tests, store config tests, UI thinking and provider-management tests. Existing explicit ProviderConfig test literals gained the compatible default field. No dependency, migration or unrelated product change was added.

## Requirement evidence

| Requirement | Evidence class | Evidence |
|---|---|---|
| A1 | production GPUI | Fixed label first failed; split heading updates and rendered toggle passed after implementation. |
| A2 | production HTTP/controller + GPUI | Summary remains a distinct event through controller; mixed thinking→summary→thinking keeps summary title; body and persisted answer contain no summary. |
| A3 | GPUI + UNIT | Chinese/English fragments, rolling long lines, split ATX/bold headings, inline identifiers/arithmetic, empty markers, fenced text and 96-scalar bound. |
| A4 | production GPUI + HTTP/runtime | Existing I61 event ordering, keyboard/mouse toggle and terminal ownership fences all pass; cancellation and failed/incomplete/EOF Responses streams release no tool calls. |
| A5 | production Settings + HTTP service | Old config defaults to Chat; selected API roundtrips, keyboard/mouse selection saves and reopens; adding after editing Responses resets to Chat; saved Responses TestModel actually POSTs /responses. |
| A6 | E2E-REAL owned HTTP/controller | Real provider implementation streams summary, executes owned read tool, sends function_call_output and opaque reasoning next round, returns answer, persists usage (24 input / 6 output / 4 cached), reopens history without summary/replay. Loopback server replaces only provider network. |
| A7 | FAULT-INJECTION HTTP/runtime + UNIT | Partial arguments with EOF/failed/incomplete/error emit no ToolUse/Done; cancellation discards pending events; empty/partial deltas reconcile done snapshots; snapshots-only text/summary is recovered once. |
| A8 | production/runtime + GPUI + UNIT | Shared summary/thinking ceilings, UTF-8 boundaries, cumulative replay ceiling, Debug redaction and history exclusion. |
| A9 | NOT RUN by implementation agent | Architect owns real provider/native application acceptance and screenshot evidence. Existing pre-implementation probes do not substitute for it. |

## Commands and raw result footers

Focused command: `cargo test -p vega_runtime -p vega_conversation -p vega_ui -p vega_store --lib i71_ -- --nocapture`, exit 0, `focused-i71-final.log` (16 tests across four crates):

```text
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 469 filtered out; finished in 0.05s
test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 207 filtered out; finished in 0.04s
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 139 filtered out; finished in 0.00s
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 431 filtered out; finished in 0.06s
```

Additional production Settings Responses HTTP wiring test: `cargo test -p vega_conversation --lib i71_settings_test_model_uses_saved_responses_transport -- --nocapture`, exit 0, `settings-responses-http.log`:

```text
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 470 filtered out; finished in 0.10s
```

The later Settings focus-order fix was validated by the complete Settings group below (including the new protocol selection test). The additional HTTP wiring test was added before final clippy/fmt completed. Thus 17 distinct I71 tests have executed successfully; repeated inclusion in broader groups is not counted as additional independent evidence.

| Exact command | Actual result | Elapsed | Raw log / SHA-256 |
|---|---|---|---|
| `cargo test -p vega_runtime --lib openai::tests` | test result: ok. 43 passed; 0 failed; 3 ignored; 0 measured; 169 filtered out; finished in 0.10s | 22.17s | `regression-openai.log` / `36f5cc9af61c49936f2573a4a35f9dc6c427fc821e33fda3ee6aaeaef61f76ae` |
| `cargo test -p vega_runtime --lib reasoning` | test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 205 filtered out; finished in 0.03s | 0.41s | `regression-reasoning.log` / `af0a6b4efbf0c29f948327c35b80b393f328d7ffcd8a5a38829a42ff0535ed28` |
| `cargo test -p vega_ui --lib i61_` | test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 433 filtered out; finished in 0.20s | 26.34s | `regression-i61.log` / `166972cb03efddaf61a7cd08b7d7e614c3b6bd90c22719c24589f2bfbf9bf06a` |
| `cargo test -p vega_ui --lib settings::` | test result: ok. 48 passed; 0 failed; 0 ignored; 0 measured; 389 filtered out; finished in 1.01s | 8.05s | `regression-settings-final.log` / `167a692f535bd008a712cd18f0c4388f45a6d6cc68682b4ad5dbf37e03b0d62f` |
| `cargo test -p vega_conversation --lib provider_settings` | test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; 454 filtered out; finished in 15.64s | 58.67s | `regression-provider-settings.log` / `7748d7ce970fe5b49fdc0b7d9bf6eceaa51a9b307af5298f1e886d545d04f794` |
| `cargo test -p vega_conversation --lib agent::tests::stream_persistence` | test result: ok. 12 passed; 0 failed; 1 ignored; 0 measured; 457 filtered out; finished in 0.24s | 0.62s | `regression-persistence.log` / `801de788921f7f140416d93b95cc2afb09383bd6ebf77dcec051d636932e8b5a` |
| `cargo test -p vega_store --lib config::` | test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 131 filtered out; finished in 0.01s | 5.41s | `regression-config.log` / `de0a52afe67028bdffa32ef6fe46730fc76fd4344c2de2bb794700560edf8542` |
| `cargo clippy --workspace --all-targets -- -D warnings` | exit 0 | 5.06s | `clippy-final.log` / `86fbe80588ecf80997f12992b81781af3d8ffd80b7bf20ce4391802b9983e687` |
| `cargo fmt --all -- --check` | exit 0 | 1.9s | `fmt-final.log` / `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |

`git diff --check`: exit 0. `cargo check --workspace` also passed during development; it is supplementary, not a separate release gate. Cargo reports the existing future-incompatibility advisory for `block v0.1.6`; clippy itself has zero warnings under `-D warnings`.

## First-failure retention and corrections

- `red-gpui.log`: expected red, 1 failed / 431 filtered; the production stream received split `## 检` + `查条件`, but title remained `思考过程` rather than `检查条件`. This is the feature regression. The architect clarified that later body prose must retain the current heading, so the test's later-update example uses a second heading; the original red cause and first assertion are unchanged.
- `focused-i71.log`: compilation failed with E0502 in the new Settings test's focus setup; cloning the FocusHandle before mutable focus access fixed the test harness borrow.
- `focused-i71-2.log`: Settings test selected the default General section, so its provider selector was absent. Setting the owned test view to Providers fixed the setup; other focused cases had passed.
- `focused-i71-3.log`: E0283 in the new runtime budget test; explicit `collect::<Vec<_>>()` fixed type inference.
- `regression-settings.log`: 47 passed / 1 failed because inserting new protocol controls between Base URL and Models changed the frozen Tab chain. Production controls and focus ordering were moved before the fields; original assertions were preserved. `regression-settings-final.log`: 48 passed.
- Failures were not retried without changes and their raw output remains available.

## Residuals and integration ownership

- Spec deviations: **none**.
- Native/live acceptance, application packaging/signing/install, cloud required checks, PR merge and cleanup: **NOT RUN / pending architect**. No claim of installation or final issue closure.
- Existing ignored timing tests remain ignored: three OpenAI tests with <500/1000/2000 ms wall-clock budgets, and the conversation 16 ms stall-flush test. No new ignored tests were added and no existing assertion was relaxed. These are not reported as passing.
- Responses is explicitly opt-in. A profile requesting original Chat reasoning replay or Zhipu's thinking/off semantics is rejected before HTTP with a clear incompatibility error; the settings form explains this restriction. The user's current saved profile/config is left unchanged. Provider summary events may wrap raw reasoning at the proxy; the UI labels/preview do not claim an extra model-generated concise explanation.
- The app build must be made after the architect's latest-base integration, preserving overlapping UI work (including any thinking-scroll change). No shared app or shared build target was touched.
