# Question-wait cloud evidence

Related: [#548](https://github.com/puzige/vega/issues/548), [draft PR #565](https://github.com/puzige/vega/pull/565).

## Test-first source and preconditions

- Production-unmodified baseline: `6615748f91273bfb8ee424011b3b5a09b3777d2b`.
- Original reviewed test commit: `129f3c0d3171a62fbbd56c83298f7076f1237493`.
- [Original patch](https://github.com/puzige/vega/commit/129f3c0d3171a62fbbd56c83298f7076f1237493.patch), [conversation source](https://github.com/puzige/vega/blob/129f3c0d3171a62fbbd56c83298f7076f1237493/crates/vega_conversation/tests/question_wait_lifecycle.rs), [GPUI source](https://github.com/puzige/vega/blob/129f3c0d3171a62fbbd56c83298f7076f1237493/crates/vega/src/window/question_wait_tests.rs).
- [Pre-freeze independent review](vega-issue-548-question-wait-test-review.md) approved the stopped test-only version. Static declarations: 16 conversation plus 10 GPUI; not discovered/executed counts.
- Local validation: only format/diff/static inspection. No local Vega compilation, execution, tests, Cargo check or Clippy.

## First cloud diagnostic: mixed target and harness errors

Head: `129f3c0d3171a62fbbd56c83298f7076f1237493`.

- [Actions run 39830453184](https://github.com/puzige/vega/actions/runs/39830453184).
- [Nextest job 120004897247](https://github.com/puzige/vega/actions/runs/39830453184/job/120004897247): Python script checks succeeded; workspace discovery compilation failed.
- [Clippy job 120004897649](https://github.com/puzige/vega/actions/runs/39830453184/job/120004897649): format succeeded; all-target Clippy compilation failed.
- Required aggregate failed. No behavioral assertions executed; Rust discovered count was not obtained.

Original target errors: E0026, missing `ConversationEvent::RunStarted.work_clock`; E0599, missing `PersistenceActorConfig::with_work_time_source`. An unrelated E0277 also occurred at conversation test line 128: `u64: rusqlite::types::FromSql` is not implemented. Therefore this mixed run is **not accepted as the valid RED gate** and does not permit production implementation.

The parent inspected both job logs and locked rusqlite 0.40.2. Its unsigned-64 reader requires the absent `fallible_uint` feature; SQLite native `i64` is supported unconditionally. The GPUI final stored-duration query used the same unsupported reader. [Diagnosis recorded before repair](https://github.com/puzige/vega/pull/565#issuecomment-6348186092).

## Bounded harness correction before clean RED

Only the two test duration reads changed: read `i64`, then checked-convert with `u64::try_from`. SQL, parameters, exact duration values, all assertions and all 26 declaration names are unchanged. Negative values still fail rather than wrap/clamp. No production code or dependency feature changed. Independent read-only delta review returned READY; parent verified exact diff/hashes, `git diff --check` and `cargo fmt --all -- --check`, both exit 0.

Corrected source SHA-256:

- Conversation: `05834db6e1c88d02dee1e40e395f373bc1fc0a6c1deaa4aac1664c73a3128cf5`.
- GPUI: `58778887abe77a8351c1403f267e948d753d5e297dcd669b5b5409e220c6ac13`.

Next stage: existing Actions on the corrected test-only head, followed by a verified target-interface RED before production repair. Same-tests GREEN, evidence preservation, separate Rust-test/support removal, final empty-suite gate and merge remain pending. The extra evidence-bearing harness-correction commit preserves the failed original source and ordering rather than rewriting published history to fit a nominal three-commit target.
