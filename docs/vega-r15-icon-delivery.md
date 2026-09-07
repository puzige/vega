# R15 icon delivery

## Freeze

- verified_at_utc: 2026-09-06T08:35:41Z; verified_at_local: 2026-09-06T16:35:41+0800.
- branch: `codex/r15-macos-icon`; source commit: `06fddb9`; task_contract: `docs/vega-r15-macos-icon.md`.
- Source changes: uniformly scale the complete existing SVG artwork by 13/14 about its center; render that SVG via AppKit onto an explicit 1024 RGBA bitmap; retain production sips/iconutil scale/export steps. No color-key removal, replacement glyph, new dependency, business or signing changes.
- `cargo xtask package-icon <output.icns>` shares `render_icon` with `cargo xtask package`. It permits packaging-only verification without rebuilding the app release executable.
- os_arch: macOS 15.7.9 (24G830), arm64; rustc: 1.98.0 (Homebrew); cargo: 1.98.0 (Homebrew); git: 2.55.0.
- Source SHA256: SVG `d02b821ef3efb5f50c9f2be5835d9a3da734cecd2a51b05fb81b8854abbf4d61`; main.rs `66c76319e51502e6c2a39cfe1793a2bcb67e95b082ef51af3c7d11112e5f6bba`; package.rs `f7d6f8af92e5ee1874322232cf3e23066c9dcb1c94c9465311e57a8bab57c168`; render-icon.swift `bed0a31335e4060b2865a172e35d5619c90c5606c02f710af95a42b8575733c8`.
- tracked_diff_sha256 after source commit: `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` (clean).

## Results

Raw logs and the bounded inspection script are retained at `/private/tmp/vega-r15-executor`. Timed commands use `/usr/bin/time -p`; durations below are wall seconds. No mirrored size tests were added or existing packaging assertions changed.

| Requirement | Evidence class | Exact command | Result | Duration | Bounded footer / log SHA256 |
| --- | --- | --- | --- | --- | --- |
| Production icon export | E2E-REAL | `cargo xtask package-icon /private/tmp/vega-r15-executor/Vega.icns` | PASS | 51.53s including fresh compilation | `exported production icon`; production.log `584abff6e74457b3bca9d8f6ff490d1869352464f24fdef7ad190ddc62145326` |
| Repeat export | E2E-REAL | `cargo xtask package-icon /private/tmp/vega-r15-executor/Vega-repeat.icns` | PASS, byte identical | 2.53s | production-repeat.log `54946fd395988a3804a8527f62c437dc3f7e9a1913c723b219854f009d39fd14` |
| Decode production ICNS | E2E-REAL | `iconutil -c iconset /private/tmp/vega-r15-executor/Vega.icns -o /private/tmp/vega-r15-executor/unpacked.iconset` | PASS, ten PNG representations | not separately timed | Pixel results below |
| Decode alpha / geometry | E2E-REAL | `swift /private/tmp/vega-r15-executor/inspect.swift /private/tmp/vega-r15-executor/unpacked.iconset` | PASS | 0.74s | pixels.log `17c1b83e3dd397ed53c89e84e0d8f6bd75865072894228e93b52d8158af83ca1` |
| Existing packaging tests | UNIT/PROPERTY | `cargo test -p xtask package::tests` | PASS | 1.90s | `5 passed; 0 failed; 31 filtered out`; tests.log `945f8c1da4abe20c9287926c17912c5031c2d2b11d015df745b3888323a63a8a` |
| Format | static | `cargo fmt --all -- --check` | PASS after formatting | 1.03s | fmt.log `540206041c0fbb22252f48f8cb800c0f73d6e35c4e74ad46c8cf89533c500634` |
| Targeted strict lint | static | `cargo clippy -p xtask --all-targets -- -D warnings` | PASS | 33.17s | `Finished dev profile`; clippy.log `89837f4a18a1a1997ce6a1c79fe58915bd6e7de91e2dcc24d761620106bf47f8` |

Both ICNS files SHA256: `a64c929c36b0ef19994963dd02d289944717e62dff87d1e67c9c97954af07cd6`.

All ten decoded representations have edge maximum alpha exactly zero and retain partially transparent antialiased pixels. Half-alpha inclusive bounding boxes:

| Canvas px | Tile bounds x and y | Representations |
| --- | --- | --- |
| 16 | 2…13 | 16@1x |
| 32 | 3…28 | 16@2x, 32@1x |
| 64 | 6…57 | 32@2x |
| 128 | 12…115 | 128@1x |
| 256 | 24…231 | 128@2x, 256@1x |
| 512 | 48…463 | 256@2x, 512@1x |
| 1024 | 96…927 | 512@2x |

Executor viewed the decoded 128px PNG: centered white rounded tile, retained emerald terminal/star glyph, transparent exterior. Native Dock/Launchpad visibility is root-owned acceptance.

## First failures and residuals

- First `cargo fmt --all -- --check` failed only on the new usage message's long line; ran `cargo fmt --all`, then check passed. Retained fmt-first.log SHA256 `24fc572145b82dc6ed00e38784d4e7dbf42cb29837d5de85bc728638c76ca982`.
- First external pixel inspection script used the unavailable `NSBitmapImageRep(contentsOf:)` initializer; Swift compilation failed in 0.19s. Corrected the inspection script to `NSBitmapImageRep(data: try Data(contentsOf: file))`. Retained pixels-first.log SHA256 `94f9310a0a9e8f130d86d26d09a2cd7eecdbff3416253a728c5643831f64b46f`. This was evidence tooling, not the production renderer. Production renderer's first AppKit experiment passed in 0.55s with corner alpha 0, center alpha 1.
- LIMIT: system AppKit rendering verified on the stated macOS version; no cross-version visual acceptance claimed. Renderer rejects missing alpha/background and missing opaque center.
- LIMIT: Cargo reports existing upstream `block v0.1.6` future incompatibility; strict clippy exits successfully.
- NOT RUN by executor: full release packaging/build, workspace tests/lint, native installation, signing, Dock/Launchpad checks. Root owns unified gates and native acceptance. Bench/soak deferred by R15.
- Spec deviations: none.
