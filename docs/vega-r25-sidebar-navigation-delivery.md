# Vega R25 Sidebar navigation correction — delivery

R25 replaces the blue Sidebar navigation selection with Codex-style neutral
surfaces, removes the redundant project Chevron, and lets the folder icon
communicate collapsed and expanded state. It also retires the R24 edge-to-edge
Settings strip in favor of a normal inset, rounded navigation row.

## Freeze

- verified_at_utc: 2026-09-10T10:16:18Z
- verified_at_local: 2026-09-10T18:16:18+08:00
- branch: `feat/r25-sidebar-navigation-polish`
- contract commit: `963a1af`
- implementation commit: `7a986e86e65fb671f9cdcde3c8e5dc36b52dadd0`
- pre-delivery contract-and-implementation diff SHA256:
  `ce7b4728dbd4970a04149d4a3f3fd06ee5d6496e0f80a4067dd82ac3b85c2293`
- task contract: `docs/vega-r25-sidebar-navigation.md`
- environment: Darwin arm64; rustc 1.98.0; cargo 1.98.0; Git 2.55.0

## Changed surfaces

- `crates/vega_theme/src/lib.rs`: defines the shared Light/Dark hover and active
  navigation neutrals (`#F3F3F3` / `#EDEDED`, `#282828` / `#303030`).
- `crates/vega_ui/src/icons.rs`: exposes the shared open-folder icon.
- `crates/vega_ui/src/sidebar/projects_block.rs`: removes the independent
  Chevron, renders `Folder` or `FolderOpen`, keeps full-row pointer and keyboard
  toggling, exposes expanded state accessibly, aligns child task titles, and
  uses neutral selected styling with an 8px radius.
- Sidebar session, organization, projection, New Task, Search, and Settings
  paths consume the same neutral interaction tokens and navigation radius.
- `crates/vega_ui/src/sidebar/mod.rs`: restores the Settings surface to a 32px
  row inset 12px from the left, right, and bottom; the row contains a Settings
  icon, left-aligned label, and trailing `⌘,` hint.
- `crates/vega/src/window/workspace.rs`: replaces the R24 edge-to-edge
  assertions with production-mounted inset geometry, project disclosure,
  alignment, hit-target, and route checks.

No dependency, lockfile, migration, runtime/provider, store, credential, or
database change was made. Vega blue remains available for primary actions,
focus, Agent/AI identity, and intentional unread/pinned semantics.

## Results

| Requirement | Evidence class | Exact command | Result |
|---|---|---|---|
| Theme token contract | UNIT | `cargo test -p vega_theme` | PASS, 9/0 |
| Mounted project disclosure and alignment | E2E-REAL | `cargo test -p vega_ui r15_sidebar_has_only_projects_and_standalone_sessions -- --nocapture` | PASS, 1/0 |
| Production Sidebar widths, Settings geometry, and route | E2E-REAL | `cargo test -p vega window::workspace::tests::r21_shell_mounts_resizable_sidebar_and_exact_environment_boundaries -- --exact` | PASS, 1/0; 240/304/365px |
| Formatting | STATIC | `cargo fmt --all -- --check` | PASS, empty output |
| Strict lint | STATIC | `cargo clippy --all-targets -- -D warnings` | PASS; only the existing external `block v0.1.6` future-incompatibility notice |
| Complete workspace | MIXED | `cargo test --workspace` | PASS, 1001/0 |
| Candidate package | BUILD | `cargo xtask package` | PASS; signed bundle and plist valid |

The final local-`master` installed executable matches the packaged executable
with SHA256
`14b19802f7b7a624b9b73536c7567ef74aa2095a08a110665e12eba46858b0d5`.
The immediately previous application bundle is recoverable at
`/tmp/vega-r25-master-install.eG9Ni9/Vega.app.previous`.

## Native acceptance

- PASS: Light appearance uses a neutral gray selected project surface with
  neutral primary text and folder icon; no blue selection wash remains.
- PASS: collapsed and expanded project rows visibly use closed and open folder
  icons without a separate left Chevron. Accessibility labels change between
  `已收起` and `已展开` after full-row pointer toggles.
- PASS: child task titles align with the project label after the removed
  disclosure slot, while project add and more controls retain their compact hit
  targets.
- PASS: Settings is an inset, rounded row with icon, label, and shortcut; its
  route still opens General Settings.
- PASS: Dark appearance uses the specified neutral selected surface and remains
  readable. The appearance preference was restored to Follow System.

No push or remote release was performed. Scope deviation: none.
