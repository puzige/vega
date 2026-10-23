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

The extra evidence-bearing harness-correction commit preserves the failed original source and ordering rather than rewriting published history to fit a nominal three-commit target.

## Valid interface-missing RED and freeze

Corrected test-only head: `c1b0297de22a58f3b8ab962a4f0820a0cf8a8c2d`.

- [Actions run 39832474117](https://github.com/puzige/vega/actions/runs/39832474117).
- [Nextest job 120011930458](https://github.com/puzige/vega/actions/runs/39832474117/job/120011930458).
- [Clippy job 120011930053](https://github.com/puzige/vega/actions/runs/39832474117/job/120011930053).

Parent inspected original logs from both completed jobs. Each reports exactly six target-interface diagnostics: five E0026 occurrences for missing `RunStarted.work_clock`, one E0599 for missing `with_work_time_source`; no E0277 or other diagnostic code remains. Format and Python script checks succeeded. Workspace discovery compilation failed before a discovered count was produced; behavioral assertions have not executed. This is accepted **only as minimal target-interface RED**, not observed behavior failure or GREEN.

At that stage, the 26 tests and their expectations were frozen at the corrected head and hashes above, and production implementation began under spec v1.2. No functional assertion success, app installation or manual acceptance was recorded.

## 2026-10-23 recovery and current validation policy

The interrupted production draft was preserved, upstream `9f9fa092a3ca391a65f08419c9206f32979119e6` was merged as `a9e22b1018dbd5be42889fbd1af6dc6e67f7c9b5`, and the draft reapplied without conflicts. A current fetch confirmed no additional upstream delta. Both developmental test files still match the corrected hashes above; static declaration count remains 26, with behavioral execution still unobserved.

Current repository policy (#569/#573) cancels mandatory TDD, frozen-test and RED ordering requirements, while retaining implementation delegation/independent review, no local execution, required cloud CI and no Rust tests in master. Spec v1.3 records that policy alignment without changing product acceptance. Existing useful regressions are retained for actual functional validation; known failing/unexecuted assertions will not be discarded to obtain an empty-suite pass. Genuine harness faults must be diagnosed and corrected without weakening the intended contract.

Upstream then advanced to stage-handoff/recovery commit `5a1ec8ad26fa53fd6210a8e58b6ad85e739dd8bb`; after confirmed writer cessation, parent merged it as `a0db3e838dd60a7488b493bf28e72bbd1e286eaf`. The sole writer resolved the one tool-config-chain conflict by preserving upstream stage and wrapped question hooks, and added question notification lifecycle bindings before normal/child admission. That stage hit its active budget with real artifacts but no READY; a finalizer continues the existing code rather than redoing it.

The only developmental test delta is baseline compatibility: three newly required `SubagentSnapshot` fields receive inactive defaults (`checkpoint: None`, `release_state: String::new()`, `ready_for_review: false`) in the existing GPUI admission fixture. No expectation/name/data/action changed. GPUI source SHA-256 is now `10ac9087f54970f859f6ea4dc42b792d69e27d5eadeedbe7c8efec52400baa56`; conversation source is unchanged. The next committed source/patch will preserve this small adaptation with its exact version.

## Stopped implementation candidate

After a later interruption, parent confirmed no active writer, saved the complete draft as stash `0ac69c7f2614a7f69e12c6e3f3aac01e5bd8438b`, and integrated upstream `6e490aefc18fd0e8a263cbcd6b67570bcbf9348e` as `aa547c4edc5a38bf18cc990902665e1e816d169b`. The draft reapplied without conflicts and both development-test hashes stayed unchanged.

The sole finalizer reported READY and ceased writes. Parent independently reproduced `cargo fmt --all -- --check` and `git diff HEAD --check` (both exit 0). Tracked code diff SHA-256 (`git diff HEAD -- crates`) is `f59631d15a10ebd57dbb60e4d4be215234015fe3b64a88821bc79f70cbdee866`; the new, formerly untracked `types/work_clock.rs` SHA-256 is `468ae2621de18753769c3fa39d3951661fc64cd1ea90c1e75fdef0454424eec6`. These identify the pre-commit candidate, not cloud or review success. The finalizer also corrected fallback error-event duration forwarding; stage-hook and terminal handoff handling remain intact.

Independent review of the committed exact implementation version and functional cloud validation are next. After recording actual results and recoverable source/patch links, remove Rust tests and exclusive support in a separate commit, independently review the final production tree, and obtain latest-head required CI with `no Rust tests` (0 discovered/0 executed). No functional GREEN, merge, install or user acceptance is claimed.
