# Question-wait takeover and delivery

Issue [#548](https://github.com/puzige/vega/issues/548), existing draft PR [#565](https://github.com/puzige/vega/pull/565).

## Authorized recovery checkpoint

The user explicitly confirmed the old execution stopped and authorized this conversation to take over. The coordination comment uses `question-wait-takeover-20261024-vega`. This is a collaboration marker, not an atomic lock. The original clean worktree and all source commits remain intact.

After fetching and safely updating local master, the parent created isolated branch `feat/548-question-wait-takeover` from original PR head `093c9756a0be9f4ef06a92e0ae18adf323a5d65a` and merged current master `21b9f974215c1b6e3ba2802e9f5699428106feeb` without conflicts. Stopped integrated code is `6978d3bdcb8938d829ab8d96e83e1567b818f16c`. The existing PR branch will receive only non-force, ancestry-preserving updates, retaining its review and cloud history. No other card is being taken over.

The previous Agent scheduling blocker is historical: a new independent read-only review was successfully admitted in this conversation. Admission is not review success. The latest affected production paths include upstream UI content/run-activity changes and must be reviewed and validated on their actual integrated content.

## Unchanged contract and ordered plan

The acceptance matrix W1–W5 / U1 / N1–N3 and lifecycle clarifications in [the main spec](vega-issue-548-question-wait.md) remain authoritative and unchanged. Current repository risk-based validation policy supersedes historical mandatory RED/TDD wording. This continuation adds no product behavior or new dependency.

1. Independently review stopped integrated production code, preserving indefinite question waiting, accepted-answer timing, same-clock continuation, stale-owner cleanup, child question attention and foreground suppression.
2. Run the existing 28 developmental regressions in the existing draft PR Actions on the current integrated content. Earlier run `39934121113` remains evidence only for its original tested content: 28 executed/passed, 60 Python passed, fmt/Clippy/aggregate succeeded. It is not the current-final-head gate.
3. Only after review and current integrated functional validation, assign one dedicated writer to remove the two Rust test files and exclusive support listed in main spec §5. Preserve production WorkClock/time-source wiring, request-resolution callbacks, lifecycle ownership, UI state, subscriptions and notifications. Remove exclusive feature/dev-dependency/lock edges only. No production-equivalence shortcut, failing-test removal or assertion weakening.
4. Preserve accessible original source, complete commit identities, original cloud output and actual counts; independently review cleanup delta and audit no Rust test/doctest/support residue. Existing Python tests/fixtures, workflows, migrations, resources and benchmarks remain unchanged.
5. Run the complete latest cleanup-head required CI with explicit `no Rust tests` (0 discovered / 0 executed). Merge only after independent review and required success, then keep the Issue open and move to In review for user manual native acceptance.

## Validation and safety boundary

Local validation is static source, diff and format inspection only. No local Vega execution, compilation, cargo check, Clippy, tests or packaging. All automated execution uses existing GitHub Actions; status polling is no more frequent than every five minutes unless notified. The parent owns Git/index/commits, PR/CI and docs; the dedicated writer owns only the assigned code cleanup, and the independent reviewer writes nothing.

No runtime database/configuration modifications, CUA acceptance, installation or restart. OS banners/permission denial, focus, actual native timing presentation and notification clicks remain user manual acceptance. Merge and cloud success are not installation or user acceptance. Rollback is reverting the eventual task PR; no schema migration or user-data transformation is introduced.
