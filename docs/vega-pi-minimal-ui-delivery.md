# Vega PI minimal UI delivery

## Scope and reference principles

This first pass brings the default Vega shell closer to the restrained PI-Desktop
chrome while preserving Vega's existing actions and event paths. The visual
reference was the supplied review screenshot and the read-only PI-Desktop
chrome/composer styles.

- Keep the light shell quiet: white main surface, light gray sidebar surface,
  low-contrast labels, light borders, and restrained shadows from the existing
  Vega theme tokens.
- Use a compact sidebar at 275 px with lightweight new-task, search, and
  visibility controls, followed by `SESSIONS` and `PROJECTS` labels.
- Keep the main empty state open and quiet, with the composer docked near the
  bottom and capped at 768 px. Preserve the existing model, permission,
  branch, attachment, execute, and send controls.
- Keep collapsed-sidebar behavior, project grouping, current-branch display,
  settings access, and session creation on their existing event paths.

No runtime, store, database, credential, business-event contract, performance
benchmark, or dependency changes are included.

## Changed files

- `crates/vega_theme/src/lib.rs`: adjusted light surfaces, sidebar width and
  padding, composer radius, and composer max-width tokens.
- `crates/vega_ui/src/sidebar/**`: simplified sidebar chrome and labels while
  retaining navigation, project, session, grouping, and settings behavior.
- `crates/vega_ui/src/conversation_stream/render.rs`: applied the composer
  width token and moved the empty conversation composer to a bottom dock with
  the existing controls intact.
- `crates/vega_ui/src/navigation.rs`: exposed the existing sidebar visibility
  control for the compact sidebar header.
- `crates/vega/src/window/render.rs`: placed the existing no-thread start
  action in a bottom composer-like dock without inventing thread controls.

## Validation

The following checks passed on the delivery branch:

- `cargo fmt --all -- --check`
- `cargo test -p vega_ui` — 166 passed, 0 failed
- `cargo clippy -p vega_ui --all-targets -- -D warnings`
- `cargo clippy -p vega --all-targets -- -D warnings`
- `cargo check -p vega --all-targets`
- `cargo build --release -p vega`

The release binary was copied into the local app bundle and ad-hoc signed for
the window check. The native macOS window was checked in both Light and Dark
themes. The no-thread home state showed the title and description in the open
main area with the start action docked at the bottom. An empty selected session
showed the bottom composer with its folder/branch row, input, add action,
execute action, permission control, model, thinking mode, and send action.

## Coverage limits and deviations

- The window check was a local packaged-bundle smoke check; it did not exercise
  a live model response, a long transcript, or every project/session mutation.
- The repository does not contain a screenshot artifact from that check; the
  visual result was inspected directly in the native window.
- The no-thread home state has no real conversation controller, so it keeps the
  existing clickable start action rather than fabricating model or permission
  controls. Once a session exists, the real composer controls are used.
- The visible sidebar header keeps the requested new-task, search, and
  visibility controls. Back/forward remain on their existing keyboard and
  collapsed-window paths.
