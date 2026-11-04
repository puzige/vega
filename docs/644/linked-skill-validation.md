# Linked Skill review validation

Follow-up to #644 after the user's Settings evidence showed immediate symlink directories rejected before per-item review. Scope: controlled linked-directory review, target-bound consent, safe runtime reads, and accurate metadata structure diagnostics. No automatic approval, schema broadening, user Skill edits, or user settings/database changes.

## Development evidence

- Base: `075800e0f9bbaab540c9da555ee7b98c84845e2f`.
- Initial development: `099f067a384a14f7728bcb3a1d8fafe1b970f586`; [Actions 41809673321](https://github.com/puzige/vega/actions/runs/41809673321) executed 9 Rust tests: 9 passed, 0 skipped; 60 Python tests passed. Required check failed because test-only error helpers triggered Clippy `useless_format`. Production code did not need changing.
- Green development: `4e61cac5ba132c5a1098c1857302e74626ab82ae`; [Actions 41810931174](https://github.com/puzige/vega/actions/runs/41810931174): fmt/Clippy and required check passed; 9 Rust tests actually executed, 9 passed, 0 skipped; 60 Python tests passed.
- Recoverable tag: `evidence/644-linked-development-4e61cac5`.
- Original [conversation regressions](https://github.com/puzige/vega/blob/4e61cac5ba132c5a1098c1857302e74626ab82ae/crates/vega_conversation/src/skill_settings/linked_development_tests.rs), SHA-256 `0ac840dd5c46da24ab7b3e9cc24410f6584a0476861d88dbe1422c394ca2234b`.
- Original [runtime regressions](https://github.com/puzige/vega/blob/4e61cac5ba132c5a1098c1857302e74626ab82ae/crates/vega_runtime/src/skills/linked_development_tests.rs), SHA-256 `0503143e34b3fd50115c100d1f8d2a2d9cd40f48cbba52a83405fd58370648b0`.

Four conversation regressions use synthetic directories and a real isolated Store/Settings service: explicit review before Composer visibility and runtime activation, replaced link rejecting preview, identical-body retarget requiring review and revocation generation, content/consent changes rejecting tickets. Five runtime regressions cover body/reference/asset confinement, symlinks/hardlinks/traversal, same-byte target/identity changes, frozen content and snapshot restoration, ordinary consent/serialized shape compatibility, cyclic links and strict metadata/name validation.

## Review and local checks

Nonimplementer read-only review of `099f067a…` and incremental `4e61cac5…` found no concrete issues. The lint-only delta changes error formatting, not assertions or behavior. Sending/immediate steering consumers were reviewed statically, not exercised directly by these nine tests; ordinary snapshot compatibility checks omitted-field shape and current restore rather than restoring a base-generated fixture. Native UI and race behavior are not established by static review.

Local checks: `cargo fmt --all -- --check`, `git diff --check`, and parsing both locale TOMLs passed. No local Vega execution, compilation, tests, Clippy, or packaging. No new dependencies or workflow changes.

## Final delivery gate

Remove the two temporary Rust test files (620 lines) and their two exclusive module declarations (6 lines), preserving all production changes. Final clean-head independent delta review and required CI remain pending at this record's creation. Final Rust result must be reported as **no Rust tests (0 discovered / 0 executed)**, not as passing behavioral regressions.

#644 remains open until native user acceptance. Valid linked Skills should become reviewable; format-invalid Skills remain excluded. CI success is not evidence that all user Skills are accepted or that installation/native UI has been verified.
