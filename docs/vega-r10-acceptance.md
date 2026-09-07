# R10 acceptance contract

User direction (2026-09-06): concentrate usage statistics in Settings using the inspected ZCode experience; remove conversation-tail cost/token/duration indicators. Remove future macOS Keychain dependency. Default to native user interaction acceptance after terminating old Vega instances. Performance testing remains deferred.

## Settings usage reference

The inspected ZCode Usage page has an application-usage heading, horizontal totals band, annual Token activity heatmap, daily/weekly/cumulative activity controls, 7/30-day trend and model distribution, and an explicit refresh button. The lower card pairs a donut with model names, token totals, and percentages. Neutral rounded surfaces and consistent model colors connect the charts. This reference showed Token usage; currency costs were not observed.

Vega uses actual persisted provider-call usage. Its cost summary must preserve the existing priced/unavailable distinction and must not turn missing prices into zero. Lifetime metrics and selected-range charts must have explicit scopes. UTC day boundaries must be visible. Unsupported conversation duration metrics must not be invented. Empty and error states are distinct. Successful conversation summaries have no visible card or reserved gap; errors and interruption/recovery controls remain visible.

## Implementation ownership

- Credentials: dedicated credential-store and runtime agent; provider Settings methods and active Keychain guidance.
- Usage data: dedicated store aggregation, shared conversation types/controller and application worker wiring agent.
- Usage UI: dedicated Settings rendering/controls and transcript cleanup agent.
- Main: contract, independent review, sequential integration, native acceptance and evidence reporting.

## Acceptance gates

1. Relevant production store/controller/UI-handler tests pass, followed by integrated formatting, lint, workspace tests and build. Record any pre-existing failure separately.
2. Before native acceptance, terminate only identified old Vega instances and verify the process inventory. Launch one owned bundle built from the integrated source.
3. Inspect Usage at normal and compact window widths, scroll through all cards, switch activity and date controls, refresh, and leave/reopen Settings. Verify no cropped controls and no invented chart data. An owned seeded database used by a test is fixture evidence, never real-provider evidence.
4. Compare visible totals against read-only persisted accounting aggregates. Verify unknown pricing does not become an exact cost.
5. Verify missing local credentials produce a prompt Settings recovery error, retain the draft, and create no durable request or model traffic. Verify local credential save/reload and owner-only permissions without exposing values.
6. Verify the dependency graph contains no Keychain credential backend and current UI does not request Keychain authorization. Existing OS Keychain items are neither read, migrated, nor removed.
7. A real model response is accepted only when actually observed with user-supplied local credentials. Missing credentials leave that check pending; mock/fixture success is not a substitute.

## Current status

Implementation is integrated locally. Main combined workspace test run passed 913 tests, with zero failures or ignored tests. Strict clippy and application build passed. Subsequent presentation-only refinements were checked with focused production UI tests and rebuilt; they do not replace the recorded full-workspace snapshot.

## Intermediate credential native acceptance

- Built credential implementation in an owned app bundle, with an owned configuration copy and no migrated credentials. Verified old Vega processes were absent before launching.
- Actual Send click with a missing local key displayed the fixed Settings recovery error and retained the entered draft. Read-only database verification found zero durable messages for the owned empty test conversation and an unchanged usage row count of 15.
- Actual Settings form submission saved an explicitly non-real test value for an owned `example.invalid` provider. The test provider showed stored status; the original configured provider correctly showed a re-entry requirement.
- File metadata confirmed 0700 credential directory and 0600 credential file. After quitting and relaunching the owned app, the stored/missing statuses were preserved. No real key was accessed and no model response is claimed.
- Closed this intermediate app before final combined acceptance.

## Usage native acceptance

- Inspected the actual ZCode Usage page and switched activity/range controls before freezing the reference.
- Combined Vega bundle loaded the existing usage database through the application worker. Native total was 26.7k Tokens and estimated US$0.016625; a read-only accounting query independently matched 15 calls, 21,991 input and 4,709 output Tokens. The legacy internal `Microcents` type represents millionths of USD.
- Clicked the 30-day chart at September 5: displayed exact `26700 Tokens` for the stored model and date, matching the database. Seven-day and 30-day date endpoints changed correctly. Weekly activity, cumulative activity, refresh and Settings leave/reopen were exercised.
- Inspected 960×600 and 1280×750 windows, both light and dark themes, scrolling through the trend, model distribution and refresh button. Final date refinement displays the inclusive rolling-year endpoints; the donut center shows the selected-range total and Tokens label.
- Existing failed history shows the failure status without a statistical footer. Existing completed archived history keeps reply/tool content without completed cost cards. Native inspection found one additional nonempty-header meter; final source `46e76b3` removes it, and the rebuilt app was reopened on the same archived conversation to verify the header is clean while branch, Review and commit actions remain.
- Final native process inventory contains exactly one Vega instance, the owned R10 bundle. It is left open on Settings Usage.

## Main verification freeze

- Verified at: 2026-09-06 01:57 UTC / 09:57 Asia/Shanghai.
- Final application source: `46e76b3`; subsequent main acceptance/status commit is documentation only. Branch: `codex/vega-review-integration`.
- `cargo test --workspace`: 913 passed, 0 failed, 0 ignored at combined snapshot `b854613`; raw log `vega-r10-main-workspace-tests.log`.
- After default range/date/donut refinements: `cargo test -p vega_ui usage -- --nocapture`, 2 passed; strict `cargo clippy --all-targets -- -D warnings` passed; raw logs `vega-r10-main-usage-parity.log` and `vega-r10-main-clippy-final.log`.
- After final header removal: `cargo test -p vega_ui composer_counter`, 2 passed; `cargo build -p vega` passed; raw logs `vega-r10-main-meter-final.log` and `vega-r10-main-build-final-header.log`. Formatting checked at final documentation commit.
- Dependency tree contains no keyring/keyring-core/Apple native keyring backend. The existing upstream `block 0.1.6` future-compatibility notice remains informational.
- Raw logs and bundle provenance are retained externally; provenance records unsigned/signed binary hashes and the source commit without any credential values.

## Limits

- Real provider/network response remains pending manual local-key entry. No old Keychain item was read or migrated; no real credentials were supplied during this acceptance.
- Costs are estimates from existing pricing records, not provider invoices. Unpriced-call coverage is explicit. Dashboard days are UTC, and activity/peak/streak facts are scoped to the displayed year.
- Performance, live multi-model native chart data and real IME acceptance were not run. Multi-model, missing-data, empty, overflow and corrupt accounting cases have owned production-controller/UI test evidence.
- Local integration only: no remote push, master merge, release or installed-app replacement.
