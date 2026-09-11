# Vega R27 Sidebar row alignment — delivery

R27 removes the redundant visible Pin glyph from production `PINNED` rows and
puts Pinned titles, project-child titles, and Recents titles on one stable
content column. Project metadata and contextual action tails now reserve fixed
widths, while project folders use a deliberate 8px icon-to-label gap.

## Freeze

- verified_at_utc: 2026-09-11T01:35:10Z
- verified_at_local: 2026-09-11T09:35:10+08:00
- branch: `feat/r27-sidebar-row-alignment`
- contract commit: `a0e41baf724cd6e6fffdc2d921894535cc71f742`
- implementation commit: `b76f90d831fa34496bab56784ccf980514476a21`
- pre-delivery contract-and-implementation diff SHA256:
  `4ed7b8dc59478684db50e8bc56e5c191a67037dfde9c2c96d0abfa93fd2a14a5`
- task contract: `docs/vega-r27-sidebar-row-alignment.md`
- environment: Darwin arm64; rustc 1.98.0; cargo 1.98.0; Git 2.55.0

## Changed surfaces

- `crates/vega_theme/src/lib.rs`: freezes the shared 32px navigation title
  origin and the fixed 85px pinned-project metadata column.
- `crates/vega_ui/src/sidebar/threads_block.rs`: removes the production Pin
  glyph and empty icon slot, consumes the shared title token, and prevents
  project metadata from resizing the row tail.
- `crates/vega_ui/src/sidebar/threads_block/organization/render.rs`: routes
  production Pinned rows through the no-indicator renderer and gives folder
  icons an 8px gap before their labels.
- `crates/vega_ui/src/sidebar/threads_block/organization/projections.rs`:
  applies the same geometry to retained projection paths.
- `crates/vega_ui/src/sidebar/threads_block/organization/tests.rs`: mounts the
  production Sidebar and proves shared title origins, absent Pin glyphs,
  8 + 16 + 8 folder geometry, fixed metadata width, and stable hover bounds.

No dependency, lockfile, schema, migration, provider/runtime, credential,
project-order, pin command, or persistence change was made.

## Results

| Requirement | Evidence class | Exact command | Result |
|---|---|---|---|
| R27 mounted production geometry | E2E-REAL | `cargo test -p vega_ui r27_sidebar_rows_share_title_origin_and_keep_stable_metadata_columns -- --nocapture` | PASS, 1/0 |
| Theme geometry tokens | UNIT | `cargo test -p vega_theme` | PASS, 10/0 |
| Formatting | STATIC | `cargo fmt --all -- --check` | PASS, empty output |
| Strict workspace lint | STATIC | `cargo clippy --workspace --all-targets --all-features -- -D warnings` | PASS; only the existing external `block v0.1.6` future-incompatibility notice |
| Complete workspace, serial | MIXED | `cargo test --workspace -- --test-threads=1` | PASS |
| Candidate package | BUILD | `cargo xtask package` | PASS; signed bundle and plist valid |

The first concurrent complete-workspace run exposed two unrelated
`git_workspace` process-timing failures. Both isolated reruns passed, and the
subsequent serial complete-workspace run passed. This did not affect R27's
mounted Sidebar test.

The final local-`master` installed executable matches the packaged executable
with
SHA256
`9be8351f3e27dc7bd9bed94db569ef76f3ee47c5000f00877cc45635cf1aacf7`.
The immediately previous application bundle is recoverable at
`/tmp/vega-r27-master-install.ZNU0PI/Vega.app.previous`.

## Native acceptance

- PASS: production Pinned rows contain neither a blue Pin glyph nor an empty
  placeholder; the section heading alone communicates pinned membership.
- PASS: Pinned, expanded project-child, and Recents task titles share the same
  32px content origin.
- PASS: short and long project metadata stay in a fixed 85px column and do not
  move the action/time tail.
- PASS: project rows use 8px outer inset, a 16px folder, and an 8px
  icon-to-label gap, so project labels begin on the same 32px title column.
- PASS: contextual controls remain quiet at rest, and automated mounted bounds
  prove that hover does not shift title, metadata, or tail columns.
- PASS: Dark appearance remains readable with the same geometry. The
  appearance preference was restored to Follow System.

No push or remote release was performed. Scope deviation: none.
