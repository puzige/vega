# R11 — Branch preflight worker ownership

Status: narrow integration correction, 2026-09-06. Extends the existing branch preflight/close contract; no Git policy, lease, generation or persistence semantics change.

## Evidence and scope

The integrated workspace suite reported leaked ConversationStream and BranchSelector handles at teardown of `branch_controller_close_during_preflight_clears_exact_pending_then_reopens`. Independent exact and bounded seeded runs did not reproduce that panic. Code inspection proves an unnecessary ownership path: BranchPrepareFence contains strong UI entities, is cloned into the OS worker and is returned in its channel even though preflight only uses branch_id. Cancellation is asynchronous, so a thread may still own these handles after route close and test/UI teardown.

The preflight worker boundary shall accept only the service, opaque BranchId, cancellation token and result sender. Its result contains the permit/error only. The exact BranchPrepareFence stays in the foreground completion task, which applies the existing claim/route/generation checks. List and execute workers are outside this correction.

Verification retains the existing meaningful real-repo close/reopen test and all its assertions/timeouts. A bounded worker-boundary hold demonstrates that closing/dropping the UI releases weak stream/selector handles while the pure-data worker remains alive; release then delegates to the real preflight service and observes cancellation. This is a scheduling fault-injection boundary, not a mocked Git result. No test-only production API is added.

First failure remains recorded as not independently reproduced; strong UI handle retention across the original worker boundary is proven separately. Do not label passing repetitions as a diagnosis or weaken the leak detector.

## Delivery evidence

Source baseline: integration `3782102`, merged locally as `3acc1c0` with identical source tree. Only branch preflight worker ownership, existing caller adaptations, this spec and one ownership test changed. No native UI, provider, network, credential, persistence or Git policy change.

- Original integrated failure: `/private/tmp/vega-r11-final-workspace-tests.log`, app 68 pass / 1 fail, GPUI teardown reported ConversationStream and BranchSelector handles for `tests::branch::branch_controller_close_during_preflight_clears_exact_pending_then_reopens`.
- Before correction, exact `LEAK_BACKTRACE=1 cargo test -p vega tests::branch::branch_controller_close_during_preflight_clears_exact_pending_then_reopens -- --exact --nocapture` passed once (`/private/tmp/vega-r11-branch-leak-exact.log`). Two bounded 30-seed runs, with and without leak tracing, also passed (`/private/tmp/vega-r11-branch-leak-repeat.log`, `/private/tmp/vega-r11-branch-leak-repeat-fast.log`). These passes do not diagnose the original panic. No further seed retries were used.
- Existing production close/reopen and late-result ownership tests remain unchanged except adapting the worker argument/result types. `cargo test -p vega tests::branch:: -- --nocapture`: 8 pass / 0 fail (`/private/tmp/vega-r11-branch-ownership-tests-final.log`).
- Added `tests::branch::branch_prepare_worker_held_after_route_close_does_not_retain_ui`: owned real Git repo and service, actual worker call held with a bounded scheduling gate, actual route close and UI drop, weak UI handles absent before releasing worker, real cancellation outcome, fixture-scoped Git read confirms target was not checked out. The pure-data worker signature excludes UI entities. This is a production-boundary ownership invariant alongside the existing production root-handler E2E, not a reproduction of the original panic.
- First new-test development run observed nested UI entities before GPUI processed their release queue (`/private/tmp/vega-r11-branch-ownership-tests.log`). An ordinary App update flush now occurs after dropping root/stream/selector, as required by GPUI's release lifecycle; no sleep, assertion or timeout was weakened.
- `cargo test -p vega`: 70 pass / 0 fail (`/private/tmp/vega-r11-branch-ownership-app-tests.log`).
- `cargo clippy -p vega --all-targets -- -D warnings`: passed (`/private/tmp/vega-r11-branch-ownership-clippy.log`). Existing upstream `block` future-incompatibility advisory remains.

- `cargo build -p vega`: passed (`/private/tmp/vega-r11-branch-ownership-build.log`).
- `cargo fmt --all -- --check` and `git diff --check`: passed.

Conclusion: unnecessary strong UI retention by the original OS preflight worker is proven by source ownership; the revised boundary and route-close invariant are verified. The exact original intermittent teardown allocation/race remains unproven. List and execute worker ownership remain outside this narrow correction. Root owns final integrated workspace and native acceptance. Spec deviation: none.
