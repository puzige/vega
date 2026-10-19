# Issue #78 follow-up — hover copy implementation evidence

## Freeze

- Contract: [双侧消息悬停复制](vega-issue-78-hover-copy.md).
- Branch: `feat/issue-78-hover-copy`; base `c0a444c`.
- Verified at UTC: 2026-09-22T10:37:32.002543+00:00.
- Platform: macOS arm64; isolated worktree target, no shared target or new dependencies.
- Production change: private UI source buffer stores exact user text and assistant Markdown. Live, history, demo and benchmark append paths use that buffer. The handler reads it at activation time; rendering clones only an Rc. Stable per-entry IDs survive virtual-list index changes. No persistence or cross-crate source API changes.
- Shared Copy SVG and existing 24px icon button; full-width group spans body, gap and action. User action aligns right and assistant action left. Opacity changes preserve layout, with keyboard-only focus visibility.

## Results

| Requirement | Evidence class | Exact command | Result |
|---|---|---|---|
| C1–C5 regression first | production GPUI handler/render | `cargo test -p vega_ui issue78_hover_copy -- --nocapture` | Initial: 0 passed, 3 failed because copy actions were absent (`02-red.log`). |
| C1–C5 implementation | production GPUI handler/render | `cargo test -p vega_ui issue78 -- --nocapture` | 8 passed, 0 failed (`06-focused.log`, 0.25s test execution). |
| C1–C6, historical hydration, attachments, live timeline, 10k list | production GPUI/controller regression | `cargo test -p vega_ui conversation_stream::tests -- --nocapture` | 199 passed, 0 failed (`07-stream-regressions.log`, 4.65s test execution). |
| Keyboard activation, distinct clipboard sentinel before Enter and Space | production GPUI handler/render | `cargo test -p vega_ui issue78_hover_copy_geometry_pointer_path_and_keyboard -- --nocapture` | 1 passed, 0 failed (`08-keyboard-sentinel.log`, 0.02s test execution). |
| Format | static | `cargo fmt --all -- --check` | PASS (exit 0, empty `09-fmt.log`). |
| Candidate | build/package | `cargo build -p vega`; `cargo xtask package` | PASS, debug build 43.70s; signed release package complete (`10-build.log`, `11-package.log`). No install/launch performed. |

Real GPUI tests read the clipboard, compare exact raw user trailing newlines and assistant Markdown URLs/fences, check successive live deltas, preserve the unsent Composer draft, and ensure empty failed answers have no copy action. Drawing tests cover both roles, Light/Dark and 320px content width, mouse path from body across the gap to the button, moving out after clicking, fixed geometry, and keyboard activation. Painted-quad alpha observes actual visibility after focus; initial un-focused SVG visibility remains part of native acceptance.

## Failure ledger

- `01-red.log`: test compilation E0716 (temporary debug selector requires static lifetime); fixed test selector ownership before obtaining the behavioral red.
- `02-red.log`: expected three missing-action failures, retained.
- `03-green-attempt.log`: missing private MessageCopy import in benchmark module; fixed.
- `05-focused.log`: actual mouse-focus visibility regression, fixed to keyboard-only `focus_visible`; old direct `draw` fixture had no GPUI current view for a new focusable control. Switched the bubble geometry fixture to a mounted `EntryView`; all original geometry/text assertions remain unchanged.
- All original failed outputs remain in the local evidence directory. No ignored or weakened assertions; no retries hide failures.

## Candidate

- Built source: `5f2b8675b08e25b1044ebc3a18164989a42aa6b6`.
- Binary SHA-256: `52650a7983f7b393703f749d200f9cd6b4b6416243a44955aadd8a3b3e704a47`.
- `codesign --verify --deep --strict --verbose=2 dist/Vega.app`: PASS (`12-codesign.log`).
- This candidate is based on `c0a444c` and does not include Issue #141. Integration/rebase requires a fresh candidate identity; do not present this as latest master.

## Log hashes

- `01-red.log`: `242cd94d8095e18a62e53053b568a76b7df877e24742e29c58e0efa28fe16b98`
- `02-red.log`: `4bf23d9e275fc0d23f66b6b64c884224bbea35949f1fd52e311335955f70e1c1`
- `03-green-attempt.log`: `1fe92ab84fec3c2f61d51568850acd4e909d635c5d87dfe97b759cb5bf6cabf3`
- `04-green-attempt.log`: `3572a7c3d5c70d96d02edc56060c6492ecd93af5cb2be367cbaa13629d5a7ec6`
- `05-focused.log`: `f142b8924cebbcf967953d0f87c4414b5bf80c3e09bea563dc42a5ba810eb1d6`
- `06-focused.log`: `e05fa3a4a2c996c284d33b342bdaf8df312edecd15807660d8bb519f21bd489c`
- `07-stream-regressions.log`: `eb4399d04e82ff0824906eb6f034c0be0de0732b89505c8df5ecd2553bb83aca`
- `08-keyboard-sentinel.log`: `61d5a0e6136bbf80286f1a118fe4f6dcf439f4b38991500ba72d6a81aa189dbb`
- `09-fmt.log`: `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`
- `10-build.log`: `a925e977091fdbb9df86e6877db0b4e47358baa57c49e5a2d832e173cd9e2ef9`
- `11-package.log`: `8cba7a021b3c96dab5280723756f2450b540ae201dfe867d1eadbf9a8f20ee7d`
- `12-codesign.log`: `8f286a083210fac02910d8e7b36e183450bb435b1c9bdb93a17a85c98a09225e`

## Residuals

- NOT RUN: native app installation, actual OS clipboard, initial-hover SVG screenshots, history reopen after restart, full-window theme/narrow-window screenshots. Main agent owns exclusive native acceptance and records installation identity separately.
- NOT RUN: cloud required checks and merge. Local focused tests do not replace the required PR gate.
- LIMIT: GPUI tests prove focus and activation; synthesized native keyboard events are not valid evidence for GPUI focus.
- Spec deviations: none.
