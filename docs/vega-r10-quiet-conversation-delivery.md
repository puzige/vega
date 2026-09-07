# R10 quiet conversation and centralized usage delivery

## Freeze
- verified_at_utc: 2026-09-06T01:42:17.835945+00:00
- verified_at_local: 2026-09-06T09:42:17.835945+08:00
- branch: feat/r10-quiet-conversation
- implementation revision: b5254a6; credential preparation helper: 994c1aa
- full_gate_source_content_sha256: 0e60badea8b1f0db1be2f0be12b6f9c8477809af54a94698e2c7c9d25d2b71fb
- task_contract: docs/vega-r10-quiet-conversation.md, centralized Settings revision
- Darwin arm64
- rustc 1.98.0 (88d9e12ae 2026-08-18) (Homebrew)
- cargo 1.98.0 (797e8a9bc 2026-08-05) (Homebrew)
- git version 2.55.0

## Behavior and changed files

- Settings `usage.rs` consumes the conversation-owned persisted projection, renders lifetime Tokens/estimated cost with explicit unpriced coverage, 365-day scoped activity metrics and Monday-aligned heatmap, UTC model trend with clickable exact daily details, distribution donut, range/mode controls, refresh/loading/error/empty states. Existing theme tokens supply every color and typography size.
- Settings navigation/state/module wiring adds the fifth 使用统计 section and typed reload event/result methods. App database access and initial/reload worker wiring are owned by the parallel usage-data task.
- Conversation render/model keep successful summary projections but give them zero transcript height; failed/interrupted outcomes remain visible. Empty-composer cost meter is hidden while meter APIs/accounting remain. SummaryCard integer currency formatting is reused.
- Credential preparation helper re-arms submission, retains draft/history, shows actionable bounded error; preparing hint is generic.

## Results

| Requirement | Evidence class | Exact command | Result | Log SHA256 |
|---|---|---|---|---|
| UI production handlers/render and regression suite | UNIT/PROPERTY + production UI-handler render | `cargo test -p vega_ui` | PASS | `6a8a02b61823a7f79517df736b7b4601533bbf186a79148f2911d59a2e772efd` |
| Workspace regression suite | mixed existing E2E-REAL and unit | `cargo test --workspace` | PASS | `c7c342ff57e17065e6f0b0503c7fc0825ea3b599e7a0e75bdc36e9068e88900c` |
| Lint | BUILD | `cargo clippy --all-targets -- -D warnings` | PASS | `2050737a298a8f4dbd591813fef8ebd467f4d9c6bd7d037cd392366be17e0658` |
| Native executable compilation | BUILD | `cargo build -p vega` | PASS | `9ac6338074e35fe55dfc66a233a8fe1e3c40f3614d06d0d3ad0f311d2cebd8d0` |
| Formatting | BUILD | `cargo fmt --all -- --check` | PASS (no output; pre-commit also PASS) | n/a |
| Runtime dependency direction | ARCHITECTURE | `cargo tree -p vega_runtime --prefix none` | PASS; no GPUI/vega_ui dependency | n/a |

Bounded raw result footers (full fresh logs remain under `/private/tmp` by the basenames below):

`vega-r10-quiet-ui-tests-final.log`

```text
    Finished `test` profile [unoptimized + debuginfo] target(s) in 3.72s
test result: ok. 140 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.62s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

`vega-r10-quiet-workspace-tests.log`

```text
    Finished `test` profile [unoptimized + debuginfo] target(s) in 37.63s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 62 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 6.20s
test result: ok. 264 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 56.76s
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.42s
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 9.87s
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.06s
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 5.73s
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.39s
test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.09s
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.03s
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.03s
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
test result: ok. 32 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.39s
test result: ok. 100 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.56s
test result: ok. 92 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.32s
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 25 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.06s
test result: ok. 98 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.19s
test result: ok. 140 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.57s
test result: ok. 36 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.10s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.07s
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

`vega-r10-quiet-clippy-final.log`

```text
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.63s
```

`vega-r10-quiet-build.log`

```text
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 15.75s
```

## Residuals

- NOT RUN here: real-provider requests, real credential access, user-file mutations and native screenshot acceptance. Main owns integrated app/native review.
- LIMIT: this worktree build predates parallel credential backend and app Usage worker integration; main reruns final integrated gates.
- LIMIT: recent model series are bounded by the data contract (30 days, explicit other-model bucket); annual metrics are labeled 近365日. Dates use UTC. Estimated cost is persisted pricing output, not an invoice.
- ACCEPTED: upstream `block v0.1.6` future-incompatibility notice appears in successful Rust builds. No failed final gate.
- Superseded approach: initial per-turn disclosure draft was reverted before statistics implementation after user clarified centralized Settings; no disclosure draft remains.
- Spec deviations: none.

## Reference default follow-up

Default trend/distribution range changed from 30 to 7 days after parent reference review. Empty heatmap cells already use `border_subtle` against `bg_hover`. Focused production render/click regression rerun (parent requested no redundant full suite for this default change):

```text
cargo test -p vega_ui settings_usage_real_render -- --nocapture
test settings::usage::tests::settings_usage_real_render_controls_refresh_empty_and_failure ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 139 filtered out; finished in 0.14s
```

- focused_log_sha256: 8aac8bca80a94ae40d62a91bfdf23882664b78fbcf2d02cf9e7ed6a5d8e348aa
- final_usage_module_sha256: 89898284cb2f6680c162009b387cc973e2c2449cc34d8758810d712c7f7879d3

## Native parity follow-up

The rolling activity window now displays exact inclusive start/end dates, replacing misleading sampled month labels. The fixed-size donut overlays the selected-range compact total and Tokens label at its center without interaction handlers. Focused production UI regression verifies date-label positions, donut center/size, and unchanged chart/mode/range interactions.

```text
cargo test -p vega_ui settings_usage_real_render -- --nocapture
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 139 filtered out; finished in 0.15s
cargo build -p vega
Finished `dev` profile [unoptimized + debuginfo] target(s) in 4.21s
```

- vega-r10-usage-native-parity.log SHA256: 4b573a09c8d770f66da0b39c25915756f00f5042b097aa6d408b5d1c89adbcd4
- vega-r10-usage-native-parity-build.log SHA256: cc8d958ce6a2cbad22e84a73822f2738a0ab58a99446a9c37af44a518643491a

## Nonempty header meter follow-up

Removed the remaining visible meter child in the nonempty conversation header. Title/project/branch and trusted review/commit/tail controls are unchanged; accounting snapshots, estimates, calibration and error clearing remain covered by existing regressions. No meter display calls remain in conversation rendering.

```text
cargo test -p vega_ui composer_counter -- --nocapture
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 138 filtered out; finished in 0.02s
cargo build -p vega
Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.18s
```
