# Vega R19 — Codex UI Parity Phase 1 Delivery

## Freeze

- verified_at_utc: 2026-09-08T15:40:26Z
- verified_at_local: 2026-09-08 23:40:26 CST
- branch: `feat/r19-codex-ui-parity`
- implementation_git_head: `40e6cec` (`feat(R19): implement Codex-parity GPUI shell`)
- implementation_tracked_diff_sha256: `6342d2aff1a030a2fdb5584c0860ae2a031fd9cb5485847519c67318f3ee89a9`
  (R19 implementation diff from the pre-R19 head, excluding documentation)
- task_contract: `vega-r19-codex-parity.md` phase 1; independent GPUI implementation,
  real Vega state/controllers, R15 behavior and route fences preserved
- os_arch: Darwin 24.6.0 arm64
- rustc: `rustc 1.98.0 (88d9e12ae 2026-08-18)`
- cargo: `cargo 1.98.0 (797e8a9bc 2026-08-05)`
- git: `git version 2.55.0`

## Delivered

The normal launch now requests a 1400 × 900 window. Shared theme/layout tokens
freeze the 260px sidebar, 4px main-panel gap, 46px non-Settings header, 820px
readable conversation cap, 736px × 100px-minimum composer, 292px Environment
rail, 1180px responsive breakpoint, 272px bottom-workspace default, and 40px
workspace header. Sidebar rows use the frozen 32px height.

Every non-Settings route mounts one main header with the loaded project label
and durable task title. Review, terminal, Environment, hidden-workspace restore,
sidebar history, and the Review-local commit affordance all retain their real
handlers. Generic navigation arrows use the shared GPUI Kit/Lucide SVG path;
no Unicode interaction glyph or new icon dependency was introduced.

Wide project routes mount an inset Environment card only when a persistent
right workspace is absent. The card renders only authority Vega currently
owns: loaded project label, the live branch selector for an exact project task,
the existing diff action, and the selected-project terminal action. Below the
breakpoint the fixed rail disappears and the header opens a temporary overlay;
the explicit user-collapse choice remains separate from width-driven hiding.
Standalone and stale task/project pairings fail closed and expose no project
actions.

The existing right workspace still owns its tabs, close/move/maximize/hide,
focus, and resize behavior, and replaces Environment while visible. The bottom
workspace remains a sibling below the entire center-plus-right row, so its
272px default spans the conversation and Environment/Review regions. The
composer keeps its input, context, mode, permission, model, thinking,
send/stop, error, and keyboard guards while removing duplicated project/header
chrome and any visible token/cost meter.

## Results

| requirement | evidence class | exact command | result | duration | bounded footer/hash |
|---|---|---|---|---:|---|
| Formatting | UNIT/PROPERTY | `cargo fmt --all -- --check` | PASS | 0.95s | exit 0; empty-output SHA-256 `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| Frozen geometry tokens | UNIT/PROPERTY | `cargo test -p vega_theme` | PASS | 0.57s | `8 passed; 0 failed`; log SHA-256 `8177256341dafed8d72b1cac1e0371c6ec514c5ac55c6dc1c95fea5f01d5508c` |
| UI library regressions | E2E-REAL / UNIT/PROPERTY | `cargo test -p vega_ui --lib -- --test-threads=1` | PASS | 10.40s | `167 passed; 0 failed`; log SHA-256 `85843c87de101512fe975fe1ea25b9ee6afb5f4210b6c00b952a03dcf861cafd` |
| App controllers and mounted shell | E2E-REAL / FAULT-INJECTION / UNIT | `cargo test -p vega --all-targets -- --test-threads=1` | PASS | 33.51s | `83 passed; 0 failed`; log SHA-256 `af6d56ff3138be506f0cc9446e8b9d5e4c2e2bae23cc1baa32e18cb0143bdc50` |
| Affected crates lint | UNIT/PROPERTY | `cargo clippy -p vega_theme -p vega_ui -p vega --all-targets -- -D warnings` | PASS | 0.46s | exit 0; log SHA-256 `36d2421f561c17dcc868088c601f0278d162add8c2f38a50e46af57889b46b3c` |
| App target compatibility | UNIT/PROPERTY | `cargo check -p vega --all-targets` | PASS | 2.26s | exit 0; log SHA-256 `09d3e85f6df0a0b722a5839c03549f689869365c673219920a27d3e6ae4c6349` |
| Patch hygiene | UNIT/PROPERTY | `git diff --check` | PASS | <0.1s | exit 0; empty output |
| Full workspace | E2E-REAL / FAULT-INJECTION / UNIT | `cargo test --workspace --no-fail-fast -- --test-threads=1` | INTERRUPTED | 213s | coordinator requested handoff before completion; 278-line partial log SHA-256 `787c95e558d576c9d75fd302e55ea147af76ad971d7a81285e36ddbf1e972677` |

The new mounted-shell E2E uses an owned store/repository and production
`VegaWindow` rendering. It verifies the frozen geometry, collapse and resize
behavior across 1179/1400 widths, temporary overlay lifecycle, project-task /
standalone / project-without-task / empty route truthfulness, live branch row,
real Review and trusted commit controllers, SVG navigation controls, and
Environment replacement by a right workspace. A separate owned-home
subprocess clicks the production Local terminal action and observes a real PTY
in the 272px bottom dock; no test-only success path was added.

### First-failure record

- The first focused mounted-shell run failed before assertions because its new
  fixture omitted `gpui_kit::init`, although normal application startup always
  installs the component theme. The fixture was aligned with the production
  startup path; the focused R19 set then passed 3/3.
- The first app-wide run reached 63/83 before 20 root-view tests exposed the
  same historical shared-fixture gap. Moving the production component
  initialization into the shared root fixture eliminated all 20 failures.
- The next app-wide run reached 80/83; three navigation E2Es showed that the
  sidebar-visible toolbar no longer exposed the real Back action. The toolbar
  now mounts the existing back/forward controls using shared SVG icons. The
  final app-wide run passed 83/83.

## Root-agent macOS acceptance

- verified_at_utc: 2026-09-08T16:09:00Z
- verified_at_local: 2026-09-09 00:09:00 CST
- candidate_head: `2c86322` (`docs(R19): record Codex-parity delivery`)
- packaging: `cargo xtask package` PASS; release build, bundle validation, and
  ad-hoc signing completed successfully
- installed candidate: `/Applications/Vega.app`; its executable SHA-256 is
  `cfdf3db2cab210f62176803349985378beda7b2a53620f0e84f2d1907ef52a52`,
  identical to the packaged candidate; `codesign --verify --deep --strict`
  PASS
- native geometry: the launched standard window reports exactly 1400 x 900
  logical pixels
- light-mode interaction: PASS for project-without-task and project-task
  truthfulness, Environment collapse/reopen, Review replacing Environment,
  real PTY launch, 272px bottom dock spanning center plus right, right/bottom
  move, maximize/restore, hide/header-restore, tab close, and unchanged
  Settings routing
- responsive interaction: PASS at the exact 1179/1180 logical-pixel boundary;
  1179 opens the temporary Environment overlay, 1180 restores the wide rail,
  and an explicit collapse remains collapsed after a 1179 -> 1180 round trip
- dark appearance: PASS for shell, sidebar, composer, header, and Environment
  legibility with semantic dark surfaces and borders
- captured evidence: `vega-r19-candidate-task-light.png`,
  `vega-r19-candidate-review-light.png`,
  `vega-r19-candidate-terminal-light.png`,
  `vega-r19-candidate-1179-overlay.png`, and
  `vega-r19-candidate-dark.png` under the root agent's temporary directory

The root agent's first independent app-wide replay completed 82/83 because the
pre-existing diff refresh test observed a transient `GitFailed` retry terminal
state. The exact test passed immediately in isolation, and a second full
single-threaded replay passed 83/83. No R19 shell assertion failed in either
run; the one-off result is recorded rather than hidden.

### Post-integration install

After the fast-forward integration, local `master` at `d6f26e6` was packaged
again and installed over the accepted candidate. The final
`/Applications/Vega.app` executable SHA-256 is
`ba431a3102627db93ea728ffa3a982896f1f1e80b04f2a6c542a0d2037582859`,
identical to the executable in `master/dist/Vega.app`.
`codesign --verify --deep --strict` passed, the application launched from that
path, and the standard window again reported 1400 x 900 logical pixels. The
final installed-state capture is `vega-r19-master-final.png` in the root
agent's temporary directory.

## Residuals

- PASS: the requested real macOS review is complete. Standalone truthfulness
  remains covered by the mounted production-shell E2E because the host profile
  contained no standalone task and the review deliberately created no durable
  test data.
- LIMIT: the prepared UTM macOS guest booted cleanly to its password-protected
  `admin` login screen. The root agent did not guess credentials, so an
  in-guest Vega launch is not claimed; the VM was left paused after the boot
  check.
- LIMIT: Cargo prints the pre-existing future-incompatibility note for
  `block v0.1.6`; affected-crate clippy still exits zero under `-D warnings`.
- INTERRUPTED: the optional full-workspace command had no observed failure in
  its partial output, but it produced no terminal result and is not claimed as
  PASS. It was stopped on the root agent's instruction so independent review
  and handoff could proceed.
- NOT PERFORMED: no push, external message, schema/store/runtime change, or
  reference-app asset extraction occurred.
