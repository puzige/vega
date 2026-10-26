# #518 validation and delivery evidence

## Scope and fixed versions

- Data layer only; no Fork UI/event wiring, ACP replay or run start.
- User chose complete durable history without executable authority; cutoff only normal done assistant with complete trusted Final.
- Specification: [implementation contract](native-thread-fork.md), v2.2.
- Original implementation `34cf06c54143c01a2b9c3e5f77d1ef87638c0304`.
- Development implementation/test head `9c069d8053042b671d6a0e068e497fc8b7eb77de`.
- [Draft PR #610](https://github.com/puzige/vega/pull/610).
- [Immutable development test source](https://github.com/puzige/vega/blob/9c069d8053042b671d6a0e068e497fc8b7eb77de/crates/vega_conversation/src/threads/fork_tests.rs), Git blob `b70f29c005d64d5ac47a62ac4c8ba316385450b9`.
- [Complete immutable development patch](https://github.com/puzige/vega/commit/9c069d8053042b671d6a0e068e497fc8b7eb77de.patch); preceding tests and fixes remain reachable through commit parents and PR history. Parent read the GitHub source API successfully before cleanup.

## Actual development cloud verification

[Actions 40159988441](https://github.com/puzige/vega/actions/runs/40159988441), exact head `9c069d8053042b671d6a0e068e497fc8b7eb77de`:

| Job | Result |
|---|---|
| Clippy `121069331610` | success, includes fmt and all-target Clippy |
| Nextest `121069331537` | success; 11 tests executed, 11 passed, 0 failed, 0 skipped |
| Required `121069950745` | success |

Parent read the raw Nextest log, not only green status:

```text
Starting 11 tests across 15 binaries
Summary [0.423s] 11 tests run: 11 passed, 0 skipped
```

The same Nextest job ran existing Python suites: 5, 15 and 40 tests, all successful. The 11 Rust development tests cover immutable Unicode/Final prefix and rollback, actual delivered model input/continuations/fork-of-fork, screenshot capture aliases and trailing receipt text, typed inert delegation/legacy guards, wait/list/checkpoint payloads, privacy/fingerprints and escaped credentials, ordinary skill audits, both tool ownership mismatch directions, and shared summary pagination policies. These are real in-process Store/history tests, not provider/desktop/system integration or UI acceptance.

## Failures retained and responsible-layer corrections

1. [Actions 40157031719](https://github.com/puzige/vega/actions/runs/40157031719), head `6a119e65bfc013d0d50f650871e542798cf3aa6a`: Clippy success; Nextest 7 executed / 6 passed / 1 failed / 0 skipped; required failed. Test fixture used nonexistent `/historical-only`, while the production workspace resolver correctly requires an existing directory. Repaired fixture with owned TempDir and retained exact workspace equality/production guard.
2. Independent review at that head found decoded JSON privacy leakage, omitted ordinary skill activation audit facts, and inverse cross-thread tool ownership omission. Fixed responsible layers and added three focused regressions. No reduction of complete-history acceptance or weakened security assertions.
3. [Actions 40158618782](https://github.com/puzige/vega/actions/runs/40158618782), head `32e3f2483160281f9141b9e72f73b44a40fdbf71`: both jobs failed compiling `vega_store::credential_privacy::fields` (E0282 and consequent str/FromSql errors). No behavior test execution; discovery counts unavailable. One explicit owned tuple-vector annotation repaired inference without behavior changes.
4. [Actions 40159013427](https://github.com/puzige/vega/actions/runs/40159013427), head `c03dc0da396e096174b54c7e379281f1e40eff02`: Clippy success; Nextest 10 executed / 9 passed / 1 failed / 0 skipped. Ordinary history contained its existing derived Summary but headless history omitted it. Extracted shared summary assembly with identical latest/after/containing versus before policies, retained original ordinary/headless equality and added an explicit policy-parity regression. A separate latest-versus-before comparison excludes only the deliberate Summary difference, with exact entries/cursors/count checks in the new test.

No unchanged-version retry, ignored test, removed failure assertion or CI change was used to get the development green result.

## Independent review

A non-implementation writer reviewed `6a119e65bfc013d0d50f650871e542798cf3aa6a` and reported the three concrete findings above. Subsequent independent delta reviews at `32e3f2483160281f9141b9e72f73b44a40fdbf71`, `c03dc0da396e096174b54c7e379281f1e40eff02`, and `9c069d8053042b671d6a0e068e497fc8b7eb77de` found the issues resolved statically and no new actionable findings. These reviews do not replace current-head CI.

## Final-version gate — pending

After development green, parent safely merged latest master `ad344a7e8da0f69e6300ec935cc4960e194184e1` into `03df2c076c9a581dc7d63ac4a6b29d6e97bf53e4`; the incoming seven files concern artifact UI/theme/composer and associated documentation, with no overlap/conflict in the fork storage/history implementation. Full final-head CI is still required after cleanup.

Independent cleanup commit `5e91874fac2110d0d0149acc4fe314b42692137a` removes exactly four paths' test-only content: 1,238-line test file, three-line test hook, three-line dev-dependency block, one lockfile dependency edge. Byte comparisons confirm no production semantic changes and workspace tempfile remains for other consumers. Scoped test/support/doctest-marker residue scans and targeted rustfmt/diff checks passed. Original source/results above remain public. Final non-writer review, exact required CI, merge and user acceptance remain pending at this document version.

Local validation throughout is quick static diff/rustfmt only. No local Vega execution, compilation, Cargo check, Clippy, test, packaging or application installation occurred.

## Product boundary and manual acceptance

Forked delegation uses a separate typed inert historical reader. Existing legacy readers explicitly reject unsupported archived delegation before recovery; #519 must consume that API before exposing the Fork UI. Existing ordinary/source cards remain unchanged. Durable model/UI historical facts are preserved; missing ephemeral provider-wire/desktop state is not reconstructed. External full-output path strings are retained only as historical metadata, not read/copied file bytes.

After merge keep Issue open/In review for user acceptance. No installation is implied. Downstream UI checks must cover actual fork entry, selected prefix, source unchanged, sidebar/project workspace, no auto-run, and safe inert delegation display. Rollback must be schema-compatible: keep additive migration/columns and roll back orchestration/consumers coherently rather than deleting stored schema or user data.
