# R17 blue single-star smile logo delivery

## Freeze

- verified_at_utc: 2026-09-07T17:06:07Z; verified_at_local: 2026-09-08T01:06:07+0800
- branch: `codex/r17-smile-logo`; source commit: this delivery commit (hash reported in the executor handoff)
- task_contract: `docs/vega-r17-smile-logo-production.md`
- os_arch: macOS 15.7.9 (24G830), arm64
- rustc: 1.98.0 (88d9e12ae 2026-08-18) (Homebrew)
- cargo: 1.98.0 (797e8a9bc 2026-08-05) (Homebrew)
- git: 2.55.0
- production source: `assets/logo/vega-icon-r17-smile-light.svg`
- matching variants: `assets/logo/vega-icon-r17-smile-dark.svg`, `assets/logo/vega-symbol-r17-smile-mono.svg`
- geometry: transparent 1024px canvas; centered 832px tile at (96, 96); identical three-component mark in all variants
- source SHA-256:
  - light SVG: `0b97cc968bc91a66701217aed055d491c8c8dee2ccabb9fc7ec89aa203925824`
  - dark SVG: `1063426029af4f694c5ae90c785fad0b790edc5a26f71bc66dda63747f239b64`
  - mono SVG: `cfdf67f4c6fecd5b42bb3642f6ba9bc822b344ac95ee70046ad5836a722fcdc6`
  - `xtask/src/package.rs`: `ad8e8d74006c47be085a13a037618ffd120b2af17e18a3b0fbbdbcbc8cb6ff48`
- review artifacts: external task evidence directory `native-r17/executor/`; commands below use `$R17_EXECUTOR_DIR` for that directory

The production vector is a controlled hand-maintained interpretation of the selected R16 image. It keeps exactly three components: a connected `>`, one slender rounded four-point star, and one shallow upward cursor. Light uses sapphire blue on an opal tile; dark uses icy blue on graphite. Same-path contour rims and low-opacity offset shadows provide shallow polish using SVG features rendered by AppKit; no browser renderer or new dependency is involved. F1/F3 vectors and all R16 exploration images remain intact.

## Results

| requirement | evidence class | exact command | result | duration | bounded footer / hash |
| --- | --- | --- | --- | --- | --- |
| AppKit light render | E2E-REAL | `swift xtask/scripts/render-icon.swift assets/logo/vega-icon-r17-smile-light.svg "$R17_EXECUTOR_DIR/r17-smile-light-1024.png"` | PASS; 1024×1024 RGBA, transparent outer edge, opaque center | 0.52s | PNG SHA-256 `a4e86403fb4170e9af7fd24cc513d1ecba72c6cfa6e6b403054865e723609c78`; log `appkit-render-light-v6.log`, SHA-256 `c86ee7ed4c3d22f1e1430ecf84fbbcf2e3f420f43a34acf0d4140efb068f71b1` |
| AppKit dark render | E2E-REAL | `swift xtask/scripts/render-icon.swift assets/logo/vega-icon-r17-smile-dark.svg "$R17_EXECUTOR_DIR/r17-smile-dark-1024.png"` | PASS; 1024×1024 RGBA, transparent outer edge, opaque center | 0.31s | PNG SHA-256 `8d52c89b4cd739d6ebbaca1d5542aa7a95c5c5a826e5004d3fdbb5eb842ba52c`; log `appkit-render-dark-v6.log`, SHA-256 `09521420919fc627319d35d0629b930c60ba7d621645c2b104c52a0646c4c69d` |
| AppKit mono render | E2E-REAL | `swift xtask/scripts/render-icon.swift assets/logo/vega-symbol-r17-smile-mono.svg "$R17_EXECUTOR_DIR/r17-smile-mono-1024.png"` | PASS; `currentColor` resolves in AppKit, transparent symbol canvas | 0.31s | PNG SHA-256 `720098e25d65239bde14b281f33c7e5a84e98cb38e16a239bb0e4153246e7570`; log `appkit-render-mono-v6.log`, SHA-256 `77b63de8ee249ea7c0a7ec41b05ebbb83cc6d5aaa6852b40e1b47e7f8524f2b9` |
| Production ICNS export | E2E-REAL | `cargo xtask package-icon "$R17_EXECUTOR_DIR/Vega-r17-smile.icns"` | PASS; production source flowed through AppKit → sips → iconutil | 2.36s repeat export | ICNS SHA-256 `1446a2b79dff877c95e26eb7c08fb370f15e47bd5dbdcc866dbab680dd0ad685`; log `package-icon-v6.log`, SHA-256 `a178de63b7dda11d90512e623d67831b542f788c61b2c6dd37a0e37827de8338` |
| Repeat ICNS export | E2E-REAL | `cargo xtask package-icon "$R17_EXECUTOR_DIR/Vega-r17-smile-repeat.icns"` | PASS; byte-identical to primary export | 2.36s | same ICNS SHA-256 `1446a2b79dff877c95e26eb7c08fb370f15e47bd5dbdcc866dbab680dd0ad685`; `cmp` reported `repeat byte comparison: identical` |
| Decode production ICNS | E2E-REAL | `iconutil -c iconset "$R17_EXECUTOR_DIR/Vega-r17-smile.icns" -o "$R17_EXECUTOR_DIR/Vega-r17-smile-v6.iconset"` | PASS; ten Apple iconset PNG representations | <1s | decoded files retained under `Vega-r17-smile-v6.iconset` |
| Decode alpha and bounds | E2E-REAL | `swift /private/tmp/vega-r17-inspect/inspect-icons.swift "$R17_EXECUTOR_DIR/Vega-r17-smile-v6.iconset"` | PASS; every edge maximum alpha is 0; every representation retains antialiasing | 0.8s | `r17-iconset-inspection.txt`, SHA-256 `7d012d891fbe0b40aeb1080f88ce4e98f216ec54219b219b61ebd439e7023bf1` |
| Focused xtask tests | UNIT/PROPERTY | `cargo test -p xtask package::tests` | PASS: 5 passed; 0 failed; 0 measured; 31 filtered out | 3.19s | `xtask-tests-v6.log`, SHA-256 `ccd091790fb693033a2cafc391d757df5ab601214ae3948404cb452cc9bedaf6` |
| Format | static | `cargo fmt --all -- --check` | PASS | 1.20s | `fmt-v6.log`, SHA-256 `30a3865a70c04d14dd1b22af9691813c14a42a4a1816f6b17fc3e1149b03bc01` |
| Strict targeted lint | static | `cargo clippy -p xtask --all-targets -- -D warnings` | PASS; no warnings; existing `block v0.1.6` future-incompatibility notice only | 1.21s | `xtask-clippy-v6.log`, SHA-256 `ccddec8e780b96cca4ee54f40c802994cefc66c97821fe00c1d9e59b02388b40` |

The decoded half-alpha inclusive bounds are symmetric and match the R15 transparent-margin contract:

| canvas px | bounds | representations |
| --- | --- | --- |
| 16 | 2…13 | `16@1x` |
| 32 | 3…28 | `16@2x`, `32@1x` |
| 64 | 6…57 | `32@2x` |
| 128 | 12…115 | `128@1x` |
| 256 | 24…231 | `128@2x`, `256@1x` |
| 512 | 48…463 | `256@2x`, `512@1x` |
| 1024 | 96…927 | `512@2x` |

The executor viewed the AppKit light/dark 1024px renders, the decoded 128px and 32px representations, and the 16px representations. The mark remains identifiable at each decoded size, with one star and a distinct cursor. The v5 pre-rim renders are retained as `r17-smile-light-v5-fallback.png` and `r17-smile-dark-v5-fallback.png` in the review executor directory for comparison.

## Changed files

- `assets/logo/vega-icon-r17-smile-light.svg` — production AppKit/ICNS source.
- `assets/logo/vega-icon-r17-smile-dark.svg` — identical geometry, icy-blue dark appearance.
- `assets/logo/vega-symbol-r17-smile-mono.svg` — identical geometry, `currentColor` symbol.
- `assets/logo/LOGO.md` — R17 status, file roles, palette, and export record; historical assets preserved.
- `xtask/src/package.rs` — production icon source constant used by `package-icon`.
- `README.md` — product logo image reference.
- `docs/vega-packaging.md` and `docs/vega-release.md` — packaging source/renderer documentation.
- `docs/vega-r17-logo-delivery.md` — this evidence report.

## Residuals

- ACCEPTED: the SVG is a controlled vector interpretation of the selected generated reference; it is not an automatic pixel trace.
- ACCEPTED: AppKit rendering and decoded size checks ran on macOS 15.7.9 arm64. Cross-version macOS rendering and native Dock/Launchpad acceptance remain root-owned.
- ACCEPTED: the `.icns` export uses the light appearance because ICNS has no automatic light/dark appearance selection. The dark SVG remains available for explicit dark/marketing contexts.
- NOT RUN by executor: full workspace fmt/clippy/test/build, native installation/signing, Dock/Launchpad checks, deployment, provider calls, credentials/Keychain access, or performance work. Root owns those checks.
- Spec deviations: none.
