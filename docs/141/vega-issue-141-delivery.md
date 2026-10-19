# Issue #141 implementation evidence

Implementation handoff report; native candidate acceptance, cloud CI, integration
and card closure remain main-owned. Contract:
[Environment starts collapsed](vega-issue-141-environment-default-collapsed.md).

## Freeze

- Branch: `feat/environment-default-collapsed`, fetched/rebased by main before edits.
- Environment: Darwin arm64; Rust/Cargo 1.98.0.
- Evidence manifest identifier: `issue-141-2026-09-22`; raw logs remain outside
  the worktree. Final source/log hashes and verification time follow below.

## Implementation

The constructor initialized `environment_collapsed` to false, so a project route
on a wide viewport automatically mounted Environment. The sole production change
sets that field to true; `environment_overlay_open` remains false. All assignments
were inspected: only the explicit header toggle can change collapsed to false;
X closes it, and navigation/layout paths only close the overlay. No persistent
preference or migration is added, so every newly constructed window (including
restart) uses the collapsed default.

The new production-root regression leaves constructor state untouched, checks
rail/overlay absence and unselected painted quads, navigates task→project draft→
task, resizes narrow→wide, manually opens the rail, and constructs a second
window that must still start collapsed. Existing open-panel tests now explicitly
click the production header toggle before their original geometry/action/focus
assertions, including the Issue #69 regression. The project-draft Review absence
assertion runs after reveal so it still proves route fencing.

Changed production: `crates/vega/src/window/mod.rs` (one boolean).
Changed tests: `crates/vega/src/window/workspace.rs`, `crates/vega/src/tests/r69.rs`.
Updated design guidelines §10 and added task contract/report. Spec deviations: none.

## Results

Verified at UTC: 2026-09-22T10:34:16.214774+00:00

Final three-Rust-file diff SHA-256: `e9a5903cdb0e2b2daf53eb34b80db06d416aebcbe053009e7a1abc24f0a3ac29`.

| Command | Result | Raw log |
|---|---|---|
| `cargo test -p vega --bin vega issue141_environment_starts_collapsed_until_requested -- --nocapture` (before fix) | Exit 101: 0 passed, 1 failed, `new project window must start collapsed` | `red-default-test.log` |
| Same command (after fix) | Exit 0: 1 passed, 0 failed/ignored; 0.17s | `green-default-test.log` |
| `cargo test -p vega --bin vega -- --test-threads=1` | Exit 0: 198 passed, 0 failed/ignored; 73.59s | `vega-bin-tests.log` |
| `cargo test -p vega --bin vega r69_a7_project_draft_lists_and_switches_without_materializing -- --nocapture` | Exit 0: 1 passed, 0 failed/ignored; 0.80s | `final-draft-fixture-test.log` |
| `cargo fmt --all -- --check` | Exit 0, no output | `final-fmt.log` |
| `cargo xtask package` | RUNNING at handoff; exit status will be in `package.exit` | `package.log` |

The full binary run used the final production change and all explicit-open
fixtures; after it compiled, one draft fixture's reveal was moved before its
existing Review-absence assertion to keep that assertion non-vacuous. The final
focused draft test above validates that exact ordering. No other Rust changes
followed the full run. Production-root tests prove default construction and
rendered state; actual process restart/pixels remain native acceptance work.

| Log | SHA-256 |
|---|---|
| `red-default-test.log` | `58d1857e9cd2ca0fbfd9c4cf282b8c6e7baf61622c40809ff1df483dde6df9e3` |
| `green-default-test.log` | `56cc4c413d519bec8897b2fac1067c980cedd85af764c79f5d67288170c2a2ac` |
| `vega-bin-tests.log` | `1a77a2cf367f780149caf002b386c8db2c2b63dbb68ded1480efdf0032554056` |
| `final-draft-fixture-test.log` | `c7660d0e2dc85c27ebaca1354f533172219303305eb1e4b4960fdeaa12d81902` |
| `final-fmt.log` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `environment-assignment-audit.txt` | `4bd19134cd40cf0e66caff208928183475437f0b1624aecb7fa684c146743daa` |

## Residuals

- Main-owned/PENDING: final native startup/restart screenshots, manual toggle
  acceptance, cloud required checks, integration and Issue/Project closure.
- No user configuration, store schema or persistent Environment preference change.
- Existing dependency warning: `block v0.1.6` future incompatibility.
- Historical default-open test scenarios remain covered through explicit manual
  reveal; geometry, focus, rail/overlay switching, terminal and route assertions
  remain intact. No ignored/deleted tests or weakened assertions.
