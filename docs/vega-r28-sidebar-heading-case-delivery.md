# Vega R28 Sidebar heading case — delivery

R28 changes the visible Sidebar section labels from all caps to ordinary title
case: `Pinned`, `Projects`, and `Recents`. No geometry or behavior changed.

## Freeze

- verified_at_utc: 2026-09-11T01:47:42Z
- verified_at_local: 2026-09-11T09:47:42+08:00
- branch: `feat/r28-sidebar-heading-case`
- contract commit: `b4df55a038e59394541fc1fd00db3bb2b18374f1`
- implementation commit: `21cbbe32ea4e073d4d9784695068cead615469bb`
- pre-delivery diff SHA256:
  `71938b3055f1d433f807548bfc3699d87c40bb8eb54953718ef5f7a777f3f7ed`
- task contract: `docs/vega-r28-sidebar-heading-case.md`

## Results

| Requirement | Exact command | Result |
|---|---|---|
| Production title-case labels and order | `cargo test -p vega_ui r28_sidebar_section_labels_use_title_case -- --nocapture` | PASS, 1/0 |
| Retained Projects label | `cargo test -p vega_ui production_sidebar_refreshes_real_checkout_and_rejects_removed_results -- --nocapture` | PASS, 1/0 |
| R15/R26/R27/R28 organization regressions | focused organization suite | PASS, 10/0 |
| Complete UI crate | `cargo test -p vega_ui` | PASS, 174/0 |
| Formatting | `cargo fmt --all -- --check` | PASS |
| Strict lint | `cargo clippy --all-targets -- -D warnings` | PASS |
| Complete workspace | `cargo test --workspace` | PASS |
| Candidate package | `cargo xtask package` | PASS |

Native macOS acceptance confirmed that `Pinned`, `Projects`, and `Recents`
render in title case and keep the existing color, size, spacing, order, and row
geometry. The final local-`master` installed executable SHA256 is
`d81e736ae3e28f551e89342f7a53a12bc1b860ba1370b03430b4058ba49b795a`.
The previous app is recoverable at
`/tmp/vega-r28-master-install.1a0Lh3/Vega.app.previous`.

No push or remote release was performed. Scope deviation: none.
