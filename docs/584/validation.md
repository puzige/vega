# Code scrolling validation

Issue [#584](https://github.com/puzige/vega/issues/584), PR [#585](https://github.com/puzige/vega/pull/585).

## Development verification

Original development head: `89e75717df40b167553f0b04c47ae97b91ad5d21`, based on `b2248996cc68497ef050539fd69f05cc723e4c0a`.

- [First Actions run 39998956494](https://github.com/puzige/vega/actions/runs/39998956494): cancelled by the corrective fixture push; no behavior success is claimed.
- Independent review of that exact head found no production P1/P2 defect and one development fixture error: the test window did not mount `TextSelectionLayer`, which the real Vega window mounts before its content. The normal text-selection positive control was therefore not correctly connected. Correct the fixture without changing its behavior assertions; retain the first run's actual result separately.
- Independent review passed full corrected-fixture head `73b5ce5b9acf26ecf94ade3b766b1ebd6b7d4a3c` before cloud compilation. [Actions 39999442658](https://github.com/puzige/vega/actions/runs/39999442658) then failed in both Clippy and Nextest compilation: `thinking_body.rs` still used its outer vertical `ScrollFade`/`tracked_scroll_edge_fade` after their imports were removed during extraction (`E0433`, `E0599`). This is an implementation integration error, not an assertion failure. Rust discovery did not complete and no Rust test execution result was obtained. Cloud fmt and the three Python suites (5 + 15 + 40 = 60 executed) passed. The correction restores the still-required imports; all behavior assertions remain unchanged.
- [Actions 40000485051](https://github.com/puzige/vega/actions/runs/40000485051), head `3039b9cc6a1f3b816f6cd3a25c3bb7035e5c9a21`: cloud fmt/full Clippy passed; Rust 9 discovered and 9 executed, 7 passed, 2 failed, 0 skipped. Both failures are the thumb-drag assertion in the shared fixture. Read-only source diagnosis found the input exactly on the excluded bottom edge of the real thumb, so the down event invoked a track click rather than starting a drag. Move the input inside the real thumb and retain all displacement/selection assertions; no production scrollbar change follows from this fixture failure.
- A separate independent review found a genuine production S2 gap: pinned ScrollableMask only consumes changed X, allowing a zero-X horizontal gesture tail to bubble its original Y into the ancestor. Contract v1.3 specifies this already-required isolation explicitly. Replace the mask with a single local viewport wheel handler reusing OngoingScroll recognition, retain horizontal ownership through filtered-zero deltas, and add behavior regressions for tail/termination/new vertical gesture/recognizer timeout.
- [Actions 40003782199](https://github.com/puzige/vega/actions/runs/40003782199), head `791537897d7b949618ea1e26b8a46b414531c1b0`: cloud full Clippy passed; Rust 11 discovered/executed, 10 passed, 1 failed, 0 skipped. Both thumb-drag/track assertions and all wheel/tail/lifecycle checks passed. The remaining Markdown positive text-selection assertion failed because its input y=18 lay inside the scrollbar track: a single 20 px line plus 12 px bottom padding gives a 32 px viewport and a 16 px track starts at y=16. Thinking has 12 px top and bottom padding (44 px viewport, track starts at y=28), explaining its successful same-coordinate test. The fixture correction chooses each real text line center using the existing typography/padding tokens; all assertions and production behavior stay intact.
- The v1.3 continuation replaces the mask with one Hidden viewport Bubble handler, preserving horizontal ownership on filtered-zero tails, releasing it on terminal input or line input, and retaining pinned recognizer timeout behavior. The thumb fixture now uses an interior point; all original displacement/selection assertions remain. Two lifecycle tests bring the development suite to 11 declarations, pending new Actions execution. Original header/Thinking geometry, keyed handles, explicit scrollbar mode, and default-off fade compatibility are unchanged. No installed-application verification has occurred.

Root local checks on the development version: `cargo fmt --all -- --check`, `git diff --check`, `cargo metadata --no-deps --offline --locked --format-version 1` exited 0. A standard-library TOML comparison confirmed all 914 existing lock package versions/sources/checksums unchanged; ten packages and associated edges were introduced solely for pinned GPUI test support. The implementation writer's scoped format and static results are in [implementation.md](implementation.md).

No local Vega execution, build, Clippy, tests, packaging, installation, restart or database mutation. Synthetic GPUI events in Actions do not claim physical-device or installed-app validation.

## Test evidence and final cleanup

Successful development head: `58f569ac3d516dacb8cfa28d589c9c0e4b4c2d9b`.

[Actions 40005438002](https://github.com/puzige/vega/actions/runs/40005438002) completed successfully: fmt, full-target Clippy, three Python suites (5 + 15 + 40 = 60 executed/passed), and required check. [Nextest job 120592041007](https://github.com/puzige/vega/actions/runs/40005438002/job/120592041007) reported **11 discovered / 11 executed / 11 passed / 0 failed / 0 skipped**. The exact full head also received independent static review GO.

Accessible original sources, verified through GitHub API:

- [Markdown/shared event tests](https://github.com/puzige/vega/blob/58f569ac3d516dacb8cfa28d589c9c0e4b4c2d9b/crates/vega_ui/src/conversation_stream/code_viewport_tests.rs), SHA256 `60d3499062ed29d0f6f36213e39c1c1915f143089e6266108dba6a3e6b5ef03c`.
- [Thinking event tests](https://github.com/puzige/vega/blob/58f569ac3d516dacb8cfa28d589c9c0e4b4c2d9b/crates/vega_ui/src/conversation_stream/thinking_code_viewport_tests.rs), SHA256 `4f5ad35f93daa03190330b817c7e9feb01ba3d6048dff27682713b4581ecd12b`.
- [Original commit patch](https://github.com/puzige/vega/commit/58f569ac3d516dacb8cfa28d589c9c0e4b4c2d9b.patch). The successful commit remains an ancestor of the cleanup commit; the source does not depend on a temporary artifact.

[PR evidence comment](https://github.com/puzige/vega/pull/585#issuecomment-6372320036) fixes this exact source, hashes, run and counts and was read back before cleanup. Cleanup is limited to development Rust test modules, conditional observations, dev test-support and the ten-package exclusive lock closure. See [cleanup audit](test-cleanup.md). The final complete head requires a separate independent review and full required CI with `no Rust tests` (0 discovered/0 executed). Its exact SHA, run and actual result are recorded on the PR after that immutable head completes; empty-suite CI is not the eleven-test behavior validation above.

## Remaining manual acceptance

After the user chooses to install a released build, verify ordinary vertical wheel scrolling over long code, horizontal/diagonal/Shift input and boundary containment, hover/always scrollbar visibility and thumb dragging, selection/copy, fixed code header, and expanded Thinking. Application installation/restart is not part of this task's execution authorization. Keep the issue open and In review until explicit user acceptance.
