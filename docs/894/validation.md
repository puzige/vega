# #894 Validation and delivery

## Development behavior evidence

- Baseline: `16df62bdb939543fe144c32c0bfdea5fa84807a4`.
- Validated production/development head: `4db8eb6525059e618d94c263fb5060585e193dbf`.
- [Actions 42137819355](https://github.com/puzige/vega/actions/runs/42137819355): success. Nextest job `127630331326` discovered 9 Rust tests, executed 9, passed 9, failed 0, skipped 0. The three Python groups passed 5 + 15 + 40 tests. Format, all-target Clippy job `127630331037`, and required aggregate job `127631936538` succeeded.
- Existing workflow commands: `cargo nextest list --workspace --message-format json` followed by `cargo nextest run --workspace` for the nonempty suite; `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets -- -D warnings`; three preserved Python script regression groups.
- Recoverable [original test source](https://github.com/puzige/vega/blob/4db8eb6525059e618d94c263fb5060585e193dbf/crates/vega_runtime/src/skills/frontmatter_development_tests.rs), SHA-256 `57ffb6a304047d93327390bdbe0d061097480440112568ba1b8d449b7ef8b25a`. Remote tag `evidence/894-allowed-tools-development-4db8eb65` was read back at the exact head; source availability was verified through GitHub's contents API.
- Regressions cover string/list equivalence and order, absent/null/empty declarations, non-string rejection, field diagnostics, explicit tags and value/key aliases, all special non-finite numeric spellings versus quoted/explicit strings, metadata restrictions, YAML security limits, and actual source discovery/load.

## Failures and review

- Initial Actions 42082371883 failed compiling the temporary discovery fixture because locked ULID 3 exposes `generate()`, not `new()`. Corrected only fixture API usage; no assertions were weakened.
- Actions 42085676996 at `5f9bdc566244849226b3110c5480605df6330c2f` passed 8/8 Rust regressions, but independent review found signed-NaN and alias-key gaps outside that coverage. Those product type/diagnostic defects were fixed, with the ninth regression added before the final development run above.
- Independent nonimplementer review of the stopped final development head closed both remaining P2 findings with no new findings. Unchanged implementation conclusions were reused; static review is not native acceptance.

## Cleanup and final gate

Remove only the 301-line temporary development test file and its exclusive trailing module attributes/declaration. No temporary dependency, fixture, feature, workflow or other support was introduced. Production bytes must match the validated development head after removing that trailing test wiring. Existing Python tests, production assertions and all YAML safety budgets remain unchanged.

Local validation is limited to format/diff/static inspection; no Vega execution, compilation, Rust/Python tests, packaging or installation was performed locally. Final cleanup review and exact final-head required CI must succeed before merge. Final Rust `no Rust tests` (0 discovered / 0 executed) is cleanup evidence, not the behavior result above. Required CI and release evidence are recorded against exact versions in PR #895 and Issue #894.

No user Skill files, configuration or data were edited. `allowed-tools` remains inert declaration data, not a permission grant or filter. User manual acceptance remains pending; keep Issue #894 open / In review after merge.
