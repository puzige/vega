# R38 delivery

R38 moves the complete Sidebar organization column inward by one 4px rhythm
step. The Pinned surface now keeps an 8px logical application-edge inset while
its title and all other section content remain on one leading grid.

## Freeze

- verified_at_utc: 2026-09-11T09:27:41Z
- verified_at_local: 2026-09-11T17:27:41+08:00
- contract commit: `58b3560`
- implementation commit: `0eabb2e`
- task contract: `docs/vega-r38-sidebar-content-inset.md`
- os_arch: macOS arm64

## Results

| Requirement | Exact command | Result |
|---|---|---|
| Mounted R38 geometry | `cargo test -p vega_ui r38_organization_content_keeps_eight_pixel_edge_inset_across_themes_and_widths -- --nocapture` | PASS, 1/1 |
| Complete UI crate | `cargo test -p vega_ui --lib -- --test-threads=1` | PASS, 184/184 |
| Formatting | `cargo fmt --all -- --check` | PASS |
| Strict workspace lint | `cargo clippy --workspace --all-targets -- -D warnings` | PASS |
| Complete workspace, serial | `cargo test --workspace -- --test-threads=1` | PASS |
| Candidate package | `cargo xtask package` | PASS |

## Native acceptance

- PASS: installed light-theme screenshot shows a visible application-edge
  inset around the selected Pinned surface, matching the Codex reference's
  breathing room more closely.
- PASS: Pinned, Projects, and Recents headings and content remain one aligned
  leading column; the selected row's trailing edge is unchanged.
- PASS: Pinned title remains 8px inside its complete rounded surface, with no
  clipping.
- PASS: Light/Dark and minimum-width geometry are covered by mounted tests.

Installed candidate executable SHA256:
`8ed0d844a6de9ad17fb57d9fe99fc6a00ea0131f1f5a8f832c5ec15da2314d61`.
The previous app is recoverable at `/tmp/vega-r38-previous.Vega.app`.
No remote push performed.
