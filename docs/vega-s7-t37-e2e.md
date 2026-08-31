# Vega S7-T37 E2E evidence

- Card: T37 Pricing settings + custom persistence (A1-12 / A10-03)
- Branch: `feat/s7-t37-pricing-settings`
- SDD commit at execution: `bacaaa120484c7abd3cdd1730df6c91d3579b65d`
- Implementation commit: local branch `HEAD` (amended after this evidence refresh); no push or PR was performed
- Evidence time: 2026-08-31T01:04:29Z / 2026-08-31T09:04:29+08:00
- Final tracked implementation diff SHA-256 (excluding this evidence file): `c2aea591f1de5c1ec7c508885a910b4e71b4f682d943e1ca079f528cfaccf5ac`
- Raw command log: `/tmp/vega-s7-t37-gates.log` (ephemeral; this file is the durable summary)

Recompute the implementation hash from the frozen SDD commit and the local
implementation `HEAD`:

```sh
git diff --binary bacaaa120484c7abd3cdd1730df6c91d3579b65d..HEAD -- . ':!docs/vega-s7-t37-e2e.md' | shasum -a 256
```

## E2E-REAL: owned headless pricing lifecycle

Command:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
cargo test -p vega_conversation pricing::tests::owned_data_root_crud_restart_and_explicit_recovery_e2e -- --exact
```

Result: PASS. One owned temporary data root exercised missing-file built-in seed, custom Add/Update/Delete, process-style service restart, malformed-byte preservation, explicit Reload, and missing-target reseed. No real Vega data root, API key, provider, network, or external application was touched.

Supporting safety invariants remained focused rather than expanding a Cartesian test matrix: dynamic built-in policy membership, a real `UpdateDeepSeek` save/reload with eight distinct rates and locked UTC windows/cap, desired document retained cap, codec-valid policy forgery preservation, exact ambiguous-recovery error classification, and post-commit/disconnect persistent warning.

## E2E-REAL + FAULT-INJECTION: Settings/controller/provider preflight

Command:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
cargo test -p vega --bin vega tests::pricing_settings_and_agent_preflight_production_e2e -- --exact
```

Result: PASS. The production `VegaWindow` controller used a file-backed temporary Store and a real temporary Git project. A durable thread whose exact model was absent was rejected before agent generation, artifact generation, worker start, configuration/Keychain/provider construction, and provider request; both UserMessage and ApprovedPlan start paths stayed recoverable. The test then opened the production Settings entity, started a typed custom save, closed Settings while the save owner was blocked, and proved controller ownership survived while the old Settings entity was dropped.

The single narrow fault injection drops the already-started save worker result after the atomic save. Production disconnect handling performed one read-only reconciliation, retained the exact `DurabilityUnknownReconciled` notice through Settings close/reopen, and did not retry the write. Reopening Settings created a fresh entity from controller authority containing the exact custom model. A subsequent run made exactly one MockProvider request and exactly one agent worker start. No real provider, network, key, Keychain, or user pricing file was used.

The existing narrow GPUI journey also rendered one long safe CJK model identifier at exactly 960x600 under both Light and Dark themes, exercised Tab/Shift-Tab and Esc, and proved a double Space activation produced only the first mutation after the synchronous production-style Saving projection.

## Gates

Final rerun commands:

```sh
export PATH="$HOME/.cargo/bin:$PATH" && cargo fmt --all -- --check
export PATH="$HOME/.cargo/bin:$PATH" && cargo clippy --all-targets -- -D warnings
export PATH="$HOME/.cargo/bin:$PATH" && cargo test --workspace
export PATH="$HOME/.cargo/bin:$PATH" && cargo build --workspace
git diff --check
```

Result: PASS. The final full workspace run completed 701 tests with 0 failures and 1 explicitly ignored real-Keychain test. `cargo clippy --all-targets -- -D warnings`, formatting, build, and diff check passed.

Red-line review:

- Dependency change is internal only: `vega_conversation -> vega_token`; no new third-party crate.
- No DDL or event variant was added; `Cargo.lock` changed only for the internal workspace edge.
- `vega_token` and `vega_conversation` remain headless; Settings performs no pricing file I/O.
- Pricing paths and exact file bytes remain private below the controller; public Debug projections are content-bounded/redacted.
- No non-test `unwrap`/`expect`, hard-coded UI color, real key, or provider request was added.

## Residuals and boundaries

- Same-user complete path swap between the service's two safe captures remains the accepted T37 TOCTOU residual; the implementation does not claim race freedom.
- T37 only freezes the immutable pricing-selection and provider-call timestamp contract for T38. It does not yet calculate usage cost or persist calibrated usage.
- The production Settings list still eagerly projects validated entries up to the frozen 1,000-entry / 1 MiB caps. This is bounded, but virtualization is a later performance refinement if measurement shows it is necessary.
- Real provider billing accuracy, real API keys, and dogfood remain human-only S7 acceptance work.
