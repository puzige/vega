# R15 PI sidebar delivery

2026-09-08 · branch `feat/r15-pi-sidebar`

## Delivered

- Added migration `0005_standalone_threads.sql`. `threads.project_id` is now nullable; the rebuild preserves messages, tool calls, token usage, permissions, sidebar groups, memberships, indexes, and foreign keys. Existing R13/R14 organization tables remain readable for compatibility but are no longer rendered as product navigation.
- Added real standalone task creation and listing. A standalone task stores `project_id IS NULL`, has no visible or shared synthetic project, and gets an isolated file-backed scratch root at `scratch/<thread-id>`. Project tasks continue to resolve their registered canonical folder.
- Reworked the mounted sidebar into the PI model: standalone tasks under `SESSIONS`, project folders under `PROJECTS`, one task occurrence, project scoped plus, standalone session plus, project row disclosure, fixed project action hitboxes, independent scrolling, and vector icons.
- Disabled project, Git, Review, branch, artifact, and project file-index routes for standalone tasks. Navigation uses an optional project route; the compatibility empty string is sealed at the primitive store boundary. Scratch mutations use an internal per-thread checkpoint scope without creating a project row or permission namespace.

## Validation

- `cargo check -p vega --all-targets` — passed.
- `cargo fmt --all -- --check` — passed.
- `cargo clippy -p vega_store -p vega_conversation -p vega_ui -p vega --all-targets -- -D warnings` — passed.
- `cargo test -p vega_store --lib` — 94 passed. Includes the version-4 upgrade preservation check and `PRAGMA foreign_key_check = 0`.
- `cargo test -p vega_conversation --lib` — 301 passed. Includes standalone NULL binding, restart, and scratch isolation.
- `cargo test -p vega_ui --lib -- --test-threads=1` — 164 passed, 0 failed, 0 ignored. Superseded R13/R14 group/timeline tests were removed or rewritten as R15 project/task behavior; route, draft, persistence, standalone/project creation, deduplication, and 1200×760/960×600 mount assertions remain covered.
- `rg '#\[ignore' crates/vega_ui crates/vega_conversation crates/vega_store crates/vega` — no matches.
- `git diff --check` — passed.
- `cargo test --workspace --no-fail-fast` — not rerun in this handoff; the parent agent is running the full workspace gate independently. A previous checkout run recorded GPUI `Theme` initialization failures in the `vega --bin vega` environment, so that prior result is not represented as a current pass.

## Residual risks

- The SQL primitive `ThreadRow.project_id: String` still maps nullable SQL `NULL` to an empty string for compatibility with existing consumers. `Thread::is_standalone` and `project_binding` are the intended boundary helpers; new product paths must not compare or persist that value as a project identity.
- Legacy organization service actions and schema remain for data compatibility. They are intentionally unreachable from the R15 sidebar surface; the superseded group/timeline UI assertions were removed or replaced with two-object R15 behavior tests.
- Performance benchmark and soak work was deferred per the R15 scope.
