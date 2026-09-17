# A7-01 · Pricing gate before first-message materialization

## Freeze

- Verified at: 2026-09-17T11:42:46Z / 2026-09-17T19:42:46+0800.
- Branch: `feat/a7-pricing-draft-preflight`, based on A7 integration `fad78fb` (fetched and rebased against `origin/master` before edits).
- Platform: macOS; rustc/cargo 1.98.0; Git 2.55.0.
- Contract: `docs/vega-a7-first-message-preflight.md` rule 8, extending R69 first-submit ordering only when exact draft pricing is unavailable.

## Implementation

- A draft with a ready provider/credential now checks its exact model against the existing in-memory `PricingAuthority` **before** `materialize_draft`. Missing/pending/invalid pricing still routes to Pricing Settings, but the text, draft id, model, and project stay in memory and no empty task is inserted.
- The existing durable-thread pricing gate still runs at start and retains its behavior. Its visible repair projection is shared with the new draft gate; no pricing data or mutation policy changed.
- The mounted production-window regression sends from a real home composer with an enabled test provider and owned credential, observes Pricing Settings, emits a Settings-originated price mutation, clicks the visible “Back to app” control, checks retained state and zero writes/calls, then retries through the normal mocked provider boundary. The fixture now binds the same app actions as `main.rs`, allowing that Back control to work in the mounted test. The price form's keystrokes are not native-tested here; the production Settings event/controller path is exercised.

## Results

| Requirement | Evidence | Exact command | Bounded result |
|---|---|---|---|
| Mounted pricing repair + retry | E2E-REAL (owned temp store/config/repo; mocked provider boundary) | `scripts/cargo-lock.sh test -p vega --bin vega tests::r69::a7_unpriced_first_submit_preserves_draft_through_pricing_repair -- --exact --nocapture` | `1 passed; 0 failed; 137 filtered out` |
| R69/A7 regression | E2E-REAL and existing production tests | `scripts/cargo-lock.sh test -p vega --bin vega tests::r69 -- --nocapture` | `25 passed; 0 failed; 113 filtered out` |
| Full workspace | Mixed existing tests | `scripts/cargo-lock.sh test --workspace --quiet` | 1237 passed, 9 ignored, 0 failed; 5 doctests passed, 0 failed |
| Formatting | Static | `cargo fmt --all -- --check` | exit 0 |
| Strict lint | Static | `scripts/cargo-lock.sh clippy --workspace --all-targets -- -D warnings` | exit 0; upstream `block v0.1.6` future-incompat notice only |

## Falsification

Temporarily deleted only the new draft `select_exact` check, leaving the mounted test and existing durable gate intact. The exact test above failed at its first persistence assertion: `an unpriced draft writes no empty task`, `left: 1`, `right: 0` (exit 101). Restored the guard; exact test then passed 1/1 and the full gates above passed. The deliberately broken code was not committed.

## Residuals

- **NOT RUN here:** installed-app native pixels or live `hy3` provider E2E. The primary agent owns packaging, installation, and that acceptance after integration.
- **No spec deviation:** no new dependency, config/schema change, user credential access, or automatic deletion of historical empty tasks.
