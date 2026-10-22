# #307 runtime test harness lint correction

## Evidence and authorization

Parent verified Actions run [39804246948](https://github.com/puzige/vega/actions/runs/39804246948) at `f1793c3d141c7e2e7e8f05ab15730c59d68390e1`: Nextest 13/13 passed, 0 skipped; Clippy rejected only `manual_noop_waker` at `preparation_session_tests.rs:159` under `-D warnings`, recommending `std::task::Waker::noop()`. These CI results are parent-supplied evidence, not a local execution or an independently queried run in this correction.

Original frozen test SHA-256: `d1ade2c6d9b47ea0f711493158e43e92e312123891b0e48a981c54198ee2526d`.

## Exact correction

Only `crates/vega_runtime/src/computer_use/preparation_session_tests.rs` changes:

- Remove `Wake` from imports and delete the custom `NoopWake` struct and its empty `Wake::wake` implementation.
- Replace `Waker::from(Arc::new(NoopWake))` with `Waker::noop()`.
- Pass the resulting borrowed waker directly to `Context::from_waker(waker)` instead of borrowing the former owned waker.

Both wakers intentionally do nothing on wake. The helper still polls the same future once and asserts `Poll::Pending`. All test names, inputs and assertion predicates remain unchanged. This is an authorized TEST-HARNESS lint correction, not an acceptance change, assertion relaxation or production fix. No lint suppression is added.

## Verification boundary

Local validation is restricted to targeted rustfmt and static diff checks; no compilation, Clippy, tests or application execution. Independent semantic-equivalence review and CI rerun are pending. The previous Nextest result does not establish that the corrected file has passed CI. Production and the separate UI test patch are untouched; no commit or push is performed by this writer.
