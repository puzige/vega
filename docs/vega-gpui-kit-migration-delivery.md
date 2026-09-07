# Vega GPUI Kit 0.6 migration delivery

## Scope

The workspace now consumes `gpui-kit = 0.6.0` as the single GPUI entry point.
GPUI Kit re-exports the matching GPUI and platform APIs, so the application
keeps the existing window, element, action, and visual-test code paths while
moving their namespace to `gpui_kit`. The Kit component layer is initialized at
each production app entry and is used by the Sidebar settings control.

No runtime, store, database, credential, or conversation-event behavior was
changed. No additional direct dependency was added beyond the requested Kit
package.

## Dependency changes

- The workspace dependency `gpui` was replaced with
  `gpui_kit = { package = "gpui-kit", version = "=0.6.0", default-features = false, features = ["component", "test-support"] }`.
- The direct Zed git dependencies `gpui` and `gpui_platform` were removed.
- `crates/vega`, `crates/vega_ui`, `crates/vega_theme`, and `xtask` now use
  `gpui_kit.workspace = true`; the application and probe call
  `gpui_kit::application()`.
- `Cargo.lock` now resolves the Kit-managed `gpui-pre` 0.3.4 family and
  `gpui-base`/`gpui-component` 0.6.0. There is no Zed git GPUI package in the
  resolved graph.

## Compatibility and implementation

The first facade experiment kept the dependency key named `gpui`. GPUI Kit
also re-exports a module named `gpui`, so existing `use gpui::*` imports made
the crate path ambiguous (E0659), including the `actions!` macro expansion.
The final migration uses the unambiguous `gpui_kit` crate name throughout
Rust sources and attributes such as `#[gpui_kit::test]`. All GPUI types still
come from the Kit re-export, so no second GPUI implementation is linked.

`gpui_kit::init(cx)` is called before normal window creation, the hidden render
benchmark entry, and the `xtask` native probe. The main app maps Vega's light
and dark theme changes to `gpui_kit::component::Theme`, keeping the component
button's palette aligned with the existing Vega appearance. The organization
sidebar visual fixture initializes the component layer before mounting the
real Sidebar.

The bottom Sidebar settings entry is now a real
`gpui_kit::component::button::Button` with Kit ghost/small styling, tooltip,
accessibility label, and the existing `SettingsOpen` route callback. Other
Vega controls remain on their existing GPUI element implementations so the
migration does not rewrite the UI surface mechanically.

## Validation

| Requirement | Exact command | Result |
|---|---|---|
| Dependency graph metadata | `cargo metadata --no-deps --format-version 1` | PASS |
| Formatting | `cargo fmt --all -- --check` | PASS |
| UI compile | `cargo check -p vega_ui` | PASS |
| Application compile | `cargo check -p vega --all-targets` | PASS |
| Probe/packaging compile | `cargo check -p xtask` | PASS |
| UI tests | `cargo test -p vega_ui` | PASS, 166 passed, 0 failed |
| UI lint | `cargo clippy -p vega_ui --all-targets -- -D warnings` | PASS |
| Application lint | `cargo clippy -p vega --all-targets -- -D warnings` | PASS |
| Single GPUI source check | `cargo tree -e normal | rg 'gpui|zed|gpui-pre'` | PASS, Kit plus one gpui-pre family |

The existing `block` future-incompatibility notice is unchanged and is not a
warning emitted by Vega crates or this migration.

## Follow-up component migration list

- Introduce small adapters for Vega theme tokens to component semantic tokens,
  so larger Kit controls can share the existing light/dark palette exactly.
- Migrate the remaining settings action rows after their keyboard and pointer
  contracts have component-level coverage.
- Evaluate Kit Checkbox, Select, and Tooltip for the composer and onboarding
  surfaces; preserve the current display-only behavior where no safe runtime
  action exists.
- Add native light/dark screenshots after each larger component migration.
