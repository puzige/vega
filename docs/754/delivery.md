# Streaming credential boundary delivery

## Implemented

- Preserve the complete trailing token run and retain whole credential findings.
- Compute the emission boundary on raw stream text, then redact only the safe
  prefix. Candidate context is not truncated after 256 bytes.
- No changes to broker destination authorization, exception policy, user data or
  configuration. Already-redacted persisted text cannot be reconstructed.

## Development evidence

Production implementation and independent read-only review source:
`7f68714664e47253f2c4aab55d0a38e1260bf0e3`.
Independent review reported no blocking findings; it did not run code.

Successful development head:
`f399c189cfce2124dd402f8a5f99a33943362c7f`.
Only a test-loop lint changed from the reviewed implementation.

- [Preserved development test source](https://github.com/puzige/vega/blob/f399c189cfce2124dd402f8a5f99a33943362c7f/crates/vega_conversation/src/agent/credential_stream_tests.rs)
  (Git blob `9cf2fcaef8d36c37aa3898dad7cacabd984165cb`, 4645 bytes;
  API accessibility confirmed before removal).
- [Complete development patch](https://github.com/puzige/vega/commit/f399c189cfce2124dd402f8a5f99a33943362c7f.patch)
- [Successful Actions run](https://github.com/puzige/vega/actions/runs/40841132432)
- Required gate, formatting and Clippy passed. Nextest discovered and executed
  2 tests, 2 passed, 0 skipped. Python suites ran 5, 15 and 40 tests, all passed.
- Regressions cover all two-way Unicode-safe splits and character-wise streaming
  of task paths, embedded prefixes, independent/common credentials, long tokens,
  Bearer, assignments, JWT and known exact text with punctuation/whitespace;
  all three channels and interleaved EOF flushing are exercised.
- Initial run `40840240095` passed both runtime regressions but failed Clippy on
  a development-test `needless_range_loop`. Corrected using `enumerate` without
  removing or weakening assertions; production content unchanged.

## Final cleanup and verification

Development-only Rust source and its `cfg(test)` module wiring are removed after
preserving the successful source/result above, as required by project policy.
No test dependency or fixture was added. The final cleanup must leave the two
production files byte-identical to the successful development source apart from
removing that module wiring.

Local checks: affected-file `rustfmt --check`, `git diff --check`, static diff and
Rust test/wiring inventory. No local Vega execution, build or test was performed.
Final-head required CI remains pending at the time this record is written; its
exact SHA, final zero-test counts and merge result are recorded in PR #755 and
Issue #754 rather than claiming a not-yet-observed pass here.

## Acceptance and limits

Unterminated tokens/candidate tails may remain buffered until a delimiter or
terminal flush. Long pending tails are rescanned; latency/memory were not measured.
No route allowlist or weakening of exact/format detection was introduced.
User acceptance on the merged application remains pending; Issue #754 stays open
and is moved to In review only after final required CI and merge. No application
installation or existing conversation rewrite is included.
