# Vega R26 Sidebar sections and hover-reveal controls — delivery

R26 replaces the mixed `SESSIONS / PROJECTS` projection with the quieter
`PINNED / PROJECTS / RECENTS` hierarchy. Pinned tasks are projected globally
and exactly once, while section, project, and task actions stay mounted but
remain visually quiet until their owning surface is hovered, their action group
has keyboard focus, or a related menu is open.

## Freeze

- verified_at_utc: 2026-09-10T10:57:50Z
- verified_at_local: 2026-09-10T18:57:50+08:00
- branch: `feat/r26-sidebar-sections-hover`
- contract commit: `621714d`
- implementation commit: `122de508fd06f31e519a220ef5ad5f8da6b58e40`
- pre-delivery contract-and-implementation diff SHA256:
  `00cca74e4be99a91c8aa7c669b159c824416bfafd5aa3fbfa0a53f01e4828405`
- task contract: `docs/vega-r26-sidebar-sections-hover.md`
- environment: Darwin arm64; rustc 1.98.0; cargo 1.98.0; Git 2.55.0

## Changed surfaces

- `crates/vega_ui/src/sidebar/threads_block/organization/render.rs`: renders
  `PINNED`, `PROJECTS`, and `RECENTS` in order; excludes pinned tasks from their
  former project/recent positions; keeps project ownership metadata on global
  pinned rows; and drives quiet header/project action rails from hover, scoped
  focus, and menu-open state.
- `crates/vega_ui/src/sidebar/threads_block.rs`: tracks section hover and scoped
  contextual focus, preserves timestamp-at-rest task rows, and reveals a task
  trigger on row hover, trigger focus, or open menu. Focus-handle cleanup uses
  linear `HashSet` membership at the 10,000-task service limit.
- `crates/vega_ui/src/sidebar/threads_block/organization/tests.rs`: mounts the
  production Sidebar and proves exact section order, exactly-once membership,
  empty-Pinned omission, persisted pin/unpin reprojection, fixed action bounds,
  rest/hover/focus/menu-open states, and the mouse-focus regression found
  during native acceptance.
- The R26 contract, design guidelines, UI spec, README, and this report record
  the new hierarchy and progressive-disclosure rule.

No dependency, lockfile, schema, migration, provider/runtime, credential,
project-order, or task-persistence change was made.

## Results

| Requirement | Evidence class | Exact command | Result |
|---|---|---|---|
| R26 production projection and contextual actions | E2E-REAL | `cargo test -p vega_ui r26_ -- --nocapture` | PASS, 3/0 |
| Existing task-menu keyboard path | E2E-REAL | `cargo test -p vega_ui task_menu_keyboard_reaches_unread_and_escape -- --nocapture` | PASS, 1/0 |
| Complete UI crate | MIXED | `cargo test -p vega_ui` | PASS, 172/0 |
| Formatting | STATIC | `cargo fmt --all -- --check` | PASS, empty output |
| Strict lint | STATIC | `cargo clippy --all-targets -- -D warnings` | PASS; only the existing external `block v0.1.6` future-incompatibility notice |
| Complete workspace | MIXED | `cargo test --workspace` | PASS, 1004/0 |
| Candidate package | BUILD | `cargo xtask package` | PASS; signed bundle and plist valid |

The final local-`master` installed executable matches the packaged executable
with SHA256
`60ec43008e0f026f3516099106c034777f52c471f236361d1aea3ce588de0c19`.
The immediately previous application bundle is recoverable at
`/tmp/vega-r26-master-install.qWjBQf/Vega.app.previous`.

## Native acceptance

- PASS: Light appearance renders `PINNED / PROJECTS / RECENTS` in the frozen
  order; pinned project tasks carry compact project metadata and are absent
  from their expanded project folders.
- PASS: at rest, section actions, project `+ / …`, and task `…` are visually
  absent while labels, folder state, pin semantics, and timestamps remain.
- PASS: hovering the Projects or Recents header reveals only that header's
  controls; hovering a project reveals its `+ / …`; hovering a task replaces
  its timestamp with `…` without shifting the title.
- PASS: the first native candidate exposed that ordinary mouse focus on an
  expanded project could keep `+ / …` visible after pointer exit. Acceptance
  rejected it; focus tracking now belongs to the contextual action group, and
  the corrected candidate hides the project controls as soon as the pointer
  moves to a child task.
- PASS: keyboard focus and open menus retain the relevant trigger, preserving
  discoverability and pointer travel into popups.
- PASS: Dark appearance remains readable and uses the same quiet/reveal
  behavior. The appearance preference was restored to Follow System.

No push or remote release was performed. Scope deviation: none.
