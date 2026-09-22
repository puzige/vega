# Issue #129 — Sidebar titlebar alignment delivery

## Freeze

- Contract: [Issue #129 spec](vega-issue-129-sidebar-alignment.md).
- Branch: `feat/issue-129-sidebar-alignment`.
- Verified at UTC: 2026-09-22 08:06; local: 2026-09-22 16:06 Asia/Shanghai.
- Baseline: current issue worktree baseline; exact Git identity retained in the local evidence manifest.
- Tracked implementation diff SHA-256 (four implementation/test/token-document files): `043e9bb6b37d51da603c5c0f03d9db9de1fbb54e7e04320961d1a51c43762fee`.
- Environment: macOS arm64; rustc 1.98.0; cargo 1.98.0; git 2.55.0.

## Change

Sidebar toolbar now starts at window y=0, uses the shared 46px main-header
height, and cannot flex-shrink. The 18px space below it preserves New Task
y=64 and scroll y=108. Four control hitboxes, icon sizes, horizontal gaps,
existing actions, sidebar footer and scrolling are unchanged.

The spacing token was documented in design guidelines before implementation.
No new dependency, storage schema, state, or public production API was added.
Spec deviations: none.

## Results

Commands below use `RUSTC_WRAPPER= TMPDIR=/tmp` for Cargo compilation because
the first attempt hit an existing sccache server's missing temporary directory.
That first failure is retained; no application assertion was reached in it.
Raw logs and exact build identity live in the persistent local `issue-129`
evidence directory under Documents/Vega/evidence, outside the worktree.

| Requirement | Evidence class | Exact command | Result / bounded footer |
|---|---|---|---|
| First environment attempt | NOT RUN application assertion | `cargo test -p vega production_root_palette_escape_preserves_composer_and_settings_action -- --nocapture` | `sccache: error: Failed to create temp dir`; Cargo failed; `red-palette.log` |
| Detect original y mismatch before fix | Production-root regression | `RUSTC_WRAPPER= TMPDIR=/tmp cargo test -p vega production_root_palette_escape_preserves_composer_and_settings_action -- --nocapture` | EXPECTED FAIL: `toggle-sidebar center 32px must align with header 23px`; `0 passed; 1 failed`; 0.10s; `red-palette-no-cache.log` |
| A1–A5 geometry and existing Search/Back/Forward behavior | E2E-REAL production UI controller, test platform | Same command after fix | PASS: `1 passed; 0 failed`; 1.66s; `green-palette.log` |
| Existing resize, responsive boundaries and sidebar footer | E2E-REAL production UI controller, test platform | `RUSTC_WRAPPER= TMPDIR=/tmp cargo test -p vega r21_shell_mounts_resizable_sidebar_and_exact_environment_boundaries -- --nocapture` | PASS: `1 passed; 0 failed`; 1.77s; `green-shell.log` |

The strengthened existing palette test covers Light/Dark at 1403×860,
1200×760, 960×600, 1229×860 and 1230×860; disabled/enabled history;
sidebar hide/show; thread and empty home draft routes. It asserts four 28px
controls with centered 16px icons and 4px gaps, header centerline within
0.5px (existing header border), New Task y=64/x=12, scroll y=108,
and 12px footer clearance.

## Residuals

- LIMIT: production layout/controller tests prove geometry and behavior on
  GPUI's test platform, not real native pixel appearance.
- NOT RUN: native candidate acceptance. The user's installed Vega is actively
  running another task; it was neither stopped nor replaced. Native screenshot
  acceptance remains with the coordinating agent.
- NOT RUN: cloud PR required check at this handoff; coordinator owns CI and
  integration. This delivery is not merge-ready until native acceptance and
  required cloud checks are satisfied.
- Rollback: revert this issue's change; no data migration.
