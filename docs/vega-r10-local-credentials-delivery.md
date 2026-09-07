# R10 local credential delivery

## Freeze
- verified_at_utc: 2026-09-06T01:36:00Z; local: 2026-09-06 09:36 +08:00.
- Source: `feat/r10-local-credentials`, rebased against fetched `origin/master` before edits; includes companion credential-error UI commit plus the owned R10 implementation delta.
- tracked_diff_sha256 before this delivery document: `f8f4345a43efca3042f7b7bdc8c52b31e1e26563898d758ffd9b891fb934d74a`.
- task_contract: `docs/vega-r10-local-credentials.md` (written before implementation).
- macOS arm64; rustc 1.98.0; cargo 1.98.0; git 2.55.0.

## Change

Credentials now live in `<resolved config.toml parent>/credentials/credentials.toml`, a standalone plaintext TOML `keys` map. The directory is 0700 and the file is 0600; this is filesystem access control, not encryption. `key_ref` remains a non-secret reference in config. The trusted configuration root is caller-selected and canonicalized; the owned credential subtree uses Unix descriptor-relative no-follow IO, ownership/type/mode/link checks, bounded parsing, process-wide locking, exclusive temporary files, sync and atomic rename. Malformed files remain intact. Direct `libc` was approved for this boundary; keyring/keyring-core and their now-unused lockfile dependencies are removed.

Settings receives the app's explicit config path for both config and credential persistence. It caches actual available references when loaded/saved, so `key_ref` alone cannot produce a stored badge. Snapshot-only constructors have no credential root and cannot silently write the real profile. Production task preparation and commit-draft provider construction use the same path's parent. Missing/unreadable keys stop before the durable runtime, produce the fixed re-entry error, and retain the draft for retry. Provider configuration/construction failures are not mislabeled credential failures.

## Results

| requirement | evidence class | exact command | result / bounded footer |
|---|---|---|---|
| Formatting | STATIC | `cargo fmt --all -- --check` | PASS, exit 0 |
| Strict workspace lint | STATIC | `cargo clippy --all-targets -- -D warnings` | PASS, finished dev profile in 13.91s including build-lock wait |
| Store + Settings filesystem behavior | E2E-REAL + filesystem security invariants | `cargo test -p vega_store -p vega_ui` | 93 store + 138 UI passed; zero ignored |
| Production task preparation | E2E-REAL | `cargo test -p vega --bin vega` | 63 passed, zero failed, 9.70s |
| App build | BUILD | `cargo build -p vega` | PASS, dev profile 40.72s |
| Dependency removal | STATIC | `cargo tree -p vega_store --depth 1` | libc/rusqlite/serde/thiserror/toml/ulid; no keyring |

Owned tests exercise real Settings submit/config persistence/local credential read using one explicit root, reopen and missing-key badge metadata, CRUD/overwrite/delete, concurrent updates, permissions, malformed/oversized file preservation, content-free errors and symlink/hardlink/traversal rejection. The app worker missing/malformed test uses the production credential branch with no provider override and asserts zero durable messages. The commit provider rejects the same owned missing/malformed credential path. Companion rendered UI regression proves the fixed error preserves the draft.

Raw logs under `/private/tmp` (no real keys were used):
- `vega-r10-credentials-store-ui.log`, SHA256 `03fb629b396a731ea69687349033747d22e98336a89dae18fa970a838328a75d`.
- `vega-r10-credentials-vega-final.log`, SHA256 `a9d974610d65924c90f0bba70dce507eb2936f150d52b09509422564896fde95`.
- `vega-r10-credentials-clippy-pass.log`, SHA256 `f84fe74dfefa38a178a707d61ef70e3ad0f3c886e06d1dd3f64f89690fc9fdcd`.
- `vega-r10-credentials-build.log`, SHA256 `78b2b5ae29bb6727d149cfb78b3119430fd50c7aed3af31d14680f61df47c21c`.

First failures retained: initial check used the old ULID constructor spelling (corrected to the pinned crate's `generate` API); initial strict lint found an unused binding/dead fallback provider, later lint found an unnecessary let-return (all corrected). The first affected test run (`vega-r10-credentials-tests.log`) had 62 pass and one existing diff-refresh `GitFailed` failure; that exact diff test passed isolated (`vega-r10-credentials-diff-retry.log`), and the final whole app test target passed 63/63. No assertion or timeout was weakened.

## Residuals and manual re-entry
- NOT RUN here: native UI/real-provider end-to-end acceptance, installed app replacement, full workspace test suite after integration. Main owns those gates. No real provider request, real user config edit, old credential extraction/deletion, or performance test occurred.
- Old OS credential entries remain untouched and are never read automatically. Users must open Settings, edit the provider, enter the key again and save. A missing local credential shows `需重新输入 API Key`; preparation displays the fixed Settings/retry instruction.
- LIMIT: plaintext storage relies on Unix owner-only filesystem permissions; non-Unix fails closed. The caller-selected config root is trusted, and same-user adversarial ancestor replacement is outside the boundary. The lock serializes threads within one process; concurrent separate app processes have no merge guarantee.
- LIMIT: config and credential files are independently atomic, not a cross-file transaction. If config persistence fails after credential persistence, the credential may already be saved; the input remains available for retry.
- Existing upstream `block` crate future-compatibility warning remains; strict current lint/build pass.
