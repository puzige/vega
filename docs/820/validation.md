# #820 validation and handoff

## Scope

Spec v2 is authoritative: user explicitly removed automatic historical migration from product scope and handles desired transfer before restarting. No real user runtime data was read, copied, deleted or reset. This delivery is unified home/YAML configuration, not a historical transfer certification.

## Development behavioral evidence

- Development GREEN source: `8053f643b8d1bc72cb78917ad196f2522e5d338d`.
- Durable remote tag: `evidence/820-yaml-tests-8053f643`.
- [Actions run 41286962324](https://github.com/puzige/vega/actions/runs/41286962324); Nextest job `124790112419`, Clippy `124790112396`.
- Actual Rust discovery 12; executed 12; passed 12; failed 0; skipped 0. The log echoes both workflow branches; the executed branch is the nonempty suite, not `no Rust tests`.
- Preserved Python suites: 5, 15 and 40 tests passed.

| Source at the tag | SHA-256 |
| --- | --- |
| `crates/vega_store/tests/yaml_configuration.rs` | `6ce4ad8592f932ed3df766c3954a47c07bf5d90225482c846b63d1d02920fb15` |
| `crates/vega_store/tests/mcp_yaml_authority.rs` | `1b135482f499a768f17bff43be324e73779d40b407e17d118e4d49b1ae12fdde` |
| `crates/vega_conversation/tests/mcp_yaml_template.rs` | `914dc3817b027c3b7de17b978acaa110d45688a17fd4dc3645cf870022edfccb` |

Coverage: invalid root isolation; duplicate/version/tag/alias/document rejection without replacement; byte-exact no-op and comment preservation; provider reordering and environment-entry unknown fields; external edit conflict and stable file locking; model/reasoning merge and grant clearing; interrupted MCP publication with stable IDs/revisions; no file-forged activation or tombstone resurrection; conflicting publication blocking; actual copied MCP authorization template load.

Earlier CI runs are not GREEN: compilation integration defects were corrected without dropping assertions; run `41286441528` first executed the suite with 11 pass/1 provider reorder failure. The production fix keeps the syntax node live until insertion, then removes the original. Same 12 tests subsequently passed at the GREEN source above.

Temporary Rust test files have been preserved remotely before final cleanup per repository policy. They add no dev dependencies/exclusive production support. Cleanup removes only those files; production must remain byte-identical to the GREEN source. Final exact-head required CI remains necessary; its empty Rust set is not behavioral success.

## Independent review

- Original non-writer full implementation review: `24d1b26e5b74735a9b2560a3f0825705c32fccdd` against `c6568bb821b24c7512f73dc3b5ea3b2fb3f16f0b`.
- Two P2 findings: MCP copied template still TOML; environment-list editing lost notes/unknown fields. Fixed with actual service/list regressions before GREEN.
- Subsequent narrow delta re-review attempts repeatedly failed/interrupted without a conclusion, all released with no writes/unknown effects. Resume was refused by execution state. Do not describe this as approved or bypass required independent review. A working non-writer review route is needed for changes after the reviewed source (including the tooling path delta and syntax-node lifecycle fix).

## Local checks and remaining acceptance

Only quick static/format/metadata/diff checks ran locally. No local Vega execution, compilation, build, Clippy, Rust/Python tests or packaging.

Final-head CI and independent delta review are pending at document creation. No merge, release or user acceptance claimed. Keep PR #824 draft and #820 In progress until gates are satisfied. Native settings/restart, actual external MCP/authentication, user manual transfer/history and unsupported-platform behavior remain user acceptance items; synthetic tests do not certify real installation data.
