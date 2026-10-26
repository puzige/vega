# Repeated held-input selection: development-test cleanup

Contract [v1](code-selection-repeat-drag.md), implementation [handoff](selection-repeat-implementation.md), validation [record](selection-repeat-validation.md). This cleanup was performed on `b936560f4665e5535955d8f50a387c6158380c9f`, after integrating latest master `888d6e1f4ef81d4bc2024ebfbb9965ff89b4f1f9`. The successfully tested development source remains an ancestor and is not amended by cleanup.

## Recoverable successful source

- Development head: `90b3b3f2cd3cd21b59c6c30287c7f7e9945276f9`.
- [Run 40115241945](https://github.com/puzige/vega/actions/runs/40115241945): 4 Rust tests discovered / 4 executed / 4 passed / 0 failed / 0 skipped; format, full-target Clippy, Python 5 + 15 + 40 and required check succeeded. These results belong to the development head, not the cleanup head.
- [Original test source](https://github.com/puzige/vega/blob/90b3b3f2cd3cd21b59c6c30287c7f7e9945276f9/crates/vega_ui/src/conversation_stream/code_drag_repeat_tests.rs), Git blob `5a1b97ea0925864e4da72f597a31db49233dfd41`, SHA-256 `14c04f14ade8dc26e6d41a123bb7d9d0538eaec2e7cb2bb6a078ddc57360b0dc`.
- [Full original patch](https://github.com/puzige/vega/commit/90b3b3f2cd3cd21b59c6c30287c7f7e9945276f9.patch) and [PR evidence](https://github.com/puzige/vega/pull/593#issuecomment-6387254042). The root fixed and read back the evidence and source identity before authorizing deletion.

## Exact cleanup

Removed the four-test `code_drag_repeat_tests.rs` module, its fixture, timed input source, observation storage and assertions. Removed all exclusive support:

- `code_drag.rs`: test module declaration and weak controller observation.
- `selection.rs`: text-layout observation.
- `thinking.rs`: private test-only construction helper.
- UI Cargo manifest: development GPUI `test-support` feature.
- Cargo lock: the exclusive ten-package test-support closure, restoring exact latest-base production bytes. Older same-name package versions required by production remain.

Preserved the production held-move change: update the latest pointer without restarting edge work, and pause immediately when horizontal edge movement is no longer requested. All ownership, focus, content, mount, epoch and cancellation checks remain. No parser, source text, style, scrollbar, wheel, CI or Python changes were introduced by cleanup; latest master theme changes remain intact. No other task worktree, installed/running application, user data or credentials were touched.

## Static verification actually executed

All commands below exited 0. No Vega execution, compilation, build, Clippy or tests ran locally.

- `cargo fmt --all -- --check` and `git diff --check`.
- `cargo metadata --offline --locked --format-version 1`: 914 packages, no enabled `test-support` feature.
- Python exact production projection: remove only the enumerated test blocks from successful head `90b3b3f2`; resulting `code_drag.rs`, `selection.rs` and `thinking.rs` bytes equal the cleanup files.
- `selection.rs`, `thinking.rs`, UI Cargo manifest and Cargo lock exactly equal latest base `888d6e1f` bytes. Across production source, manifests, vendor, CI and scripts, the only remaining difference from that base is the move-handler change in `code_drag.rs` (one deleted line, four inserted lines).
- Whole-repository static inventory: 455 Rust source files and 16 Cargo manifests; zero test attributes/cfg, test source/module paths, exclusive observation/fixture helpers, explicit test targets, development dependencies or `test-support`. The only Rustdoc fenced block is the pre-existing `text` diagram in the Markdown stream module; no executable Rustdoc fence remains.

Final exact-head independent review and required CI are still pending the root's cleanup commit. They must establish `no Rust tests` (0 discovered / 0 executed); an empty suite does not repeat the four behavioral tests. Native user acceptance remains pending.

## Cleanup hashes

| File or projection | SHA-256 |
| --- | --- |
| `code_drag.rs` production projection | `92916a7a317efc11f42c1c93d57b0746b8881630ee9265f304419f678b188c27` |
| `selection.rs` production projection | `d9ca8b4c483ad395a4a9af33c912e723c578183a6bbf3e60df6e6a031d82b978` |
| `thinking.rs` production projection | `bb530939350275d5e45c177c91af156b41e061684ed4da436219d8097bf9ee60` |
| UI Cargo manifest | `45f54ccb5909337fc1e31329bf85aaa254a829225e3e952d44ffa3fc5950787d` |
| Cargo lock | `a15fadafca5597ee96d3ad70a8d9013c44f9ebc71474ca85ca8ce1aa9527548f` |
| Binary cleanup diff over Cargo lock and UI crate | `d4df4d664609144fe4cac60eca49a578d5ad31127b03c19cfb56baa11fa0a9fc` |

No specification deviation. Cleanup implementation is stopped for root integration; no Git/index/commit/push operations were performed by the implementation agent.
