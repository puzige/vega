# A7-02 · Pi Agent credential import delivery

## Scope

- Added the explicit Settings → Providers action `从 Pi Agent 导入凭据`.
- Added a background service transaction that resolves only the invoking
  user's `$HOME/.pi/agent/models.json`, validates the selected provider, stores
  the credential through Vega's existing owner-only keystore, enables the
  provider, and rolls back the key if config persistence fails.
- Added opened-file type, ownership, permission, size, no-follow, and identity
  checks; selected-provider validation; stale-snapshot conflict handling; and
  content-free errors.
- Added production service coverage and mounted Settings coverage. Test-only
  source injection is behind the narrowly named `test-support` feature, and all
  fixtures use fake keys only.

No automatic synchronization, Pi configuration write, auth-file access, key
rendering, clipboard write, or network request was added.

## Freeze

- verified_at_utc: 2026-09-17T08:49:08Z
- verified_at_local: 2026-09-17 16:49:08 +0800
- branch: `feat/a7-pi-credential-import`
- git_head: local feature branch (OID intentionally omitted from repository evidence)
- tracked_diff_sha256 before this delivery document: `78f528eb2603548a8f54ef97983b880ff64e111daff5fbbf58c3955115ceb4ce`
- task_contract: `docs/vega-a7-pi-credential-import.md`
- os_arch: Darwin arm64
- rustc: 1.98.0
- cargo: 1.98.0
- git: 2.55.0

## Results

| requirement | evidence class | exact command | result | duration | bounded footer/hash |
|---|---|---|---|---:|---|
| Focused production service and security matrix | E2E-REAL + UNIT/PROPERTY | `scripts/cargo-lock.sh test -p vega_conversation provider_settings --lib` | PASS | 15.64s | 16 passed, 0 failed, 296 filtered |
| Mounted Settings action, busy state, success, failure, and redaction | E2E-REAL | `scripts/cargo-lock.sh test -p vega_ui settings::provider_management --lib` | PASS | 0.22s | 8 passed, 0 failed, 327 filtered |
| Source guard mutation | FAULT-INJECTION | `scripts/cargo-lock.sh test -p vega_conversation production_pi_import_rejects_missing_oversized_symlink_nonregular_and_insecure_sources --lib` with the mode guard temporarily removed | EXPECTED FAIL, then restored | 0.06s | insecure-source assertion received `Ok(AppConfig)` instead of `Err(PiSource)` |
| Error-redaction mutation | FAULT-INJECTION | `scripts/cargo-lock.sh test -p vega_conversation production_pi_import_rejects_selected_entry_validation_failures_without_mutation --lib` with the Invalid display temporarily containing the fake fixture key | EXPECTED FAIL, then restored | 0.04s | redaction assertion failed; no fixture secret remains in production code |
| Focused post-mutation verification | E2E-REAL + UNIT/PROPERTY | `scripts/cargo-lock.sh test -p vega_conversation provider_settings --lib` | PASS | 15.64s | 16 passed, 0 failed |
| Workspace regression | BUILD | `scripts/cargo-lock.sh test --workspace` | PASS | approximately 2 minutes | all executed workspace unit, integration, and doc tests passed; only pre-existing load-sensitive tests were ignored |
| Strict workspace lint | BUILD | `scripts/cargo-lock.sh clippy --workspace --all-targets -- -D warnings` | PASS | 48.21s | finished with no warnings; existing future-compatibility notice only |
| Normal conversation build without test-support | BUILD | `scripts/cargo-lock.sh check -p vega_conversation --lib` | PASS | 7.15s | production library compiled without the arbitrary-source constructor |
| Normal UI build | BUILD | `scripts/cargo-lock.sh check -p vega_ui --lib` | PASS | 4.61s | production UI library compiled |
| Formatting and whitespace | STATIC | `cargo fmt --all -- --check && git diff --check` | PASS | ~1s | no output |

## Residuals

- ACCEPTED: No native installed-app acceptance or real Pi credential read was
  performed in this implementation work. The primary agent owns native
  acceptance; the current Pi CPA and opencode-go accounts have insufficient
  upstream balance, so no live model-reply success is claimed.
- ACCEPTED: Test-only source-path injection is available only under
  `test-support`; the normal production build resolves the fixed Pi path at
  explicit invocation time.
- LIMIT: A separate explicit model test and first-message E2E remain required
  to establish network and quota readiness after local credential setup.

Spec deviations: none.
