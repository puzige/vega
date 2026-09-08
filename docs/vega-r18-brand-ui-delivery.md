# R18 — Brand UI / Icon Delivery

## Freeze

- verified_at_utc: 2026-09-08T09:02Z
- verified_at_local: 2026-09-08 17:02 CST
- branch: `feat/r18-brand-icons`
- implementation_git_head: `32e2b18` (`fix(R18): clarify terminal and folder actions`)
- implementation_tracked_diff_sha256: `a7ba974d156422dd87dd9d173e548e9a8dcdd29b7e6a6fe56c4b4e873fdd9400` (tracked diff excluding this delivery record)
- task_contract: R18 Vega brand UI/icon baseline; R15 IA and behavior preserved
- os_arch: macOS arm64
- rustc: `rustc 1.98.0 (88d9e12ae 2026-08-18)`
- cargo: `cargo 1.98.0 (797e8a9bc 2026-08-05)`

## Delivered

`vega_theme` now exposes the R18 brand token family (`brand_primary`,
`brand_primary_strong`, `brand_soft`, `brand_on_accent`) and maps `accent` plus
active selection to the R17 sapphire/ice-blue logo palette. Semantic
success/danger/warning colors remain unchanged.

`vega_ui::icons` keeps the existing public API while routing generic symbols to
`gpui_kit::component::{Icon, IconName}`. The embedded `gpui-kit-assets` bundle
provides Lucide-style 24px SVGs with round caps/joins; the wrapper fixes every
icon to a 16px optical container. Folder is the standard outline, FolderPlus
uses one static Lucide folder-plus SVG, and visible disclosure glyphs in the
composer, settings and diff controls use shared Chevron SVGs. Pin and Shield
also use static Lucide paths through GPUI's inline SVG data renderer because
those names are absent from the 0.6.0 bundle. No generic icon uses the
previous fractional `PathBuilder` geometry.

The workspace enables the `gpui-kit` `assets` feature, and normal Vega startup,
the render bench and the xtask probe install `gpui_kit::assets::Assets` through
`Application::with_assets(...)` before any icon can render.

The PI mounted organization renderer and legacy ProjectsBlock consume the same
Folder/Chevron language. Selected projects, selected tasks, composer project
context, mode/model/thinking selections and the send action use brand tokens;
permission and status semantics stay intact. R15 Project/Session IA, storage,
navigation and behavior were not changed.

## Results

| requirement | evidence class | exact command | result | bounded footer/hash |
|---|---|---|---|---|
| Formatting | UNIT/PROPERTY | `cargo fmt --all -- --check` | PASS | exit 0; pre-commit hook and direct check |
| Theme token assertions | UNIT/PROPERTY | `cargo test -p vega_theme` | PASS | `7 passed; 0 failed` |
| UI library tests | E2E-REAL / UNIT/PROPERTY | `cargo test -p vega_ui --lib -- --test-threads=1` | PASS | `167 passed; 0 failed` |
| Affected crate lint | UNIT/PROPERTY | `cargo clippy -p vega_theme -p vega_ui --all-targets -- -D warnings` | PASS | exit 0; only dependency future-incompat note |
| App target compatibility | UNIT/PROPERTY | `cargo check -p vega --all-targets` | PASS | exit 0 after enabling embedded assets |
| UI color hardcode guard | UNIT/PROPERTY | `rg -n "0x[0-9A-Fa-f]{6,8}|#[0-9A-Fa-f]{6,8}" crates/vega_ui/src || true` | PASS | empty output |
| Disclosure glyph guard | UNIT/PROPERTY | `rg -n "[▾▸⌃]" crates/vega_ui/src crates/vega/src || true` | PASS | empty output |

## Residuals

- ACCEPTED: native app launch, mounted-window pixel review and 16px raster
  inspection remain with the root agent, as requested for final integration.
- LIMIT: Cargo printed the existing dependency future-incompatibility note for
  `block v0.1.6`; it did not produce a warning or failure in the affected
  crates.
- NOT RUN: full workspace test/clippy was outside this executor's requested
  affected-crate gate; the required vega check and affected UI/theme gates ran.
