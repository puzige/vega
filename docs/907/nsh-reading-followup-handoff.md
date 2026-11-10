# Reading app followup — stopped READY for parent integration review

Worktree: `/Users/puzige/Workspace/worktrees/vega-907-nsh-full`.
User-supplied baseline: `0b5ea426abfd8d4358489e916273d89474fb190f` (no Git command run).
Continues historical handoff SHA-256 `86a0820e2e9f00f5760eabba9d9435c4f40f45515a392c0def47e8b75db99106`, preserved unchanged in `nsh-reading-resumed-handoff.md`.

READY means stopped for parent integration/typecheck/review, NOT production-complete or accepted. No running command/session/background process from this writer. No shared mod/core/render, vendor, Cargo/manifests, backend authorization docs, independent timeline/status/resume batch or other writer file edited.

## Implemented followup

- Read original `target/nsh-app-check.stderr` (SHA-256 `a247a1a67423988a326b8d012cb682f9a257f1d98dc6da392a717d375b5b8458`): five reported errors. Fixed missing Thinking WeakEntity import; cfg-gated primary clipboard to Linux without replacing its behavior; two real NSH LineHeight::Multiple uses; cloned foreground executor before moving AsyncApp into the task. Parent must compile; no error-resolution success is claimed from formatting.
- Preserved prior genuine NSH body, original structured stream blocks/version cache, menu/Fork/Compact and CopyOriginal display_text notice projection. No legacy GPUI body embedded as Native, replacement text engine or raw Markdown identity mapping.
- Consumed actual parent APIs observed during continuation, not invented signatures: `set_reading_selection_listener(area, observer) -> bool`, directed `ReadingSnapshot.endpoints`, `install_reading_selection(&expected, anchor, focus) -> Option<ReadingSnapshot>`.
- Listener registers after body layout acknowledgement. Observation is deferred outside owner/native entity updates, session-bound, serial-coalesced, current-snapshot checked; queued clear does not overwrite a currently nonempty installed selection. Selection-only events never open a menu or write clipboard.
- Retained directed native endpoints through tagged UTF-16/scalar-boundary conversion into logical SourcePositions. Source-continuous replacement retains pending positions, validates actual new tagged node/Arc snapshot and installs after owner/root/revision/layout acknowledgement. Exact retained overlap plus Thinking source_identity fences continuity. Summary/raw domain replacement clears instead of rebinding equal offsets.
- Source-window scroll no longer unconditionally resets top. Surviving source maps to the new block and restores measured pixel inset; removed anchor clamps to first surviving mapped source. Old async row measurements reject changed projection sessions. This is block-level, not intra-block wrapped-line preservation.
- Code copy uses its own real NSH control (no builtin copy id) plus engine-measured transparent GPUI keyboard/AX targets: Tab/Shift-Tab, Enter/Space, AccessibleAction::Click and focus outline. Proxy side effects recheck captured native snapshot, live host geometry, trusted owner/root/revision/tagged document and SourceSpans. Native pointer copy still uses trusted app source. Runtime keyboard/AX and receipt parity are NOT established.
- Tables retain existing StreamTable column weights/minimum widths/alignment and native horizontal scrollbar, not new estimated glyph geometry. Native layout still needs acceptance.
- Original six mapping test functions/assertions preserved; added two source-window block-anchor development tests. All eight UNRUN. Scoped phase.rs change is formatting only.

## Concrete remaining seams / unfinished acceptance

1. **Single-text selection direction/install:** actual `reading.rs::install` rejects `!expected.cross_text`; `text.rs::reading_snapshot` has ordered range only and leaves directed endpoints None. Need parent extend this exact current-snapshot-guarded seam or establish that all body reading requests use the cross-text area. App does not fabricate direction. Listener/remap append/trim/clear/detach/publication ordering and reversed selection require real regressions; no runtime success claimed.
2. **Painted text geometry + link-preview coordinator adapter:** ElementRef::measure requires a layout node; inline spans have no independent layout and are collected into parent AttributedText. Existing link_preview::prepaint_link_targets requires the legacy GPUI TextLayout; no exact-rect registration seam exists in the permitted files. Need current snapshot + text-index/UTF16-range → exact real GpuiLayout visual rects/caret/hit geometry with documented coordinates, clipping and mount/content/batch/scroll/scale currentness. Parent/coordinator owner must expose exact-rect registration preserving hover delay, safe classification, preview cancellation, keyboard/AX, focus and scroll dismissal. Direct native link opening stays enabled; preview parity and wrapped link keyboard/AX are not complete.
3. **Intra-block retained scroll:** block anchor mapping cannot preserve a wrapped line inside a partially trimmed paragraph/code block or after wrap-width change. Same exact geometry/caret/hit seam required. Source trim is not disabled; no average-width fallback.
4. **Streaming fade:** core rebuild_text replaces AttributedText Arc for style/color changes, while reading matches requires exact Arc. Per-frame alpha would invalidate selection snapshots. Need coordinated native selection remap for paint-only changes or approved paint-only identity-preserving opacity path, then reuse existing StreamFade timing/reduced-motion/finish policy. No whole-body fade workaround added.
5. **Shared keyboard copy parent wiring:** route shared render.rs copy_selected_text through ReadingBinding::copy(window,cx), not direct MessageCopy under stream update without current root/window. This writer did not change shared render. Parent clipboard takeover remains defense in depth.
6. Parent must compile/test final current tree; run eight body regressions, integration/Clippy/required CI/independent review and native selection/copy/link/code/table/scroll/keyboard/AX/root-preview-hidden/resize/scale acceptance. Potential compiler/lint errors remain unknown. Tests need preserve/clean under project delivery policy before merge. No full #907 completion claim.

Exact raw API evidence and requested geometry contract: `docs/907/nsh-reading-backend-contract.md`.

## Validation actually run

- Tool: `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`.
- Scoped `rustfmt --edition 2024 --config skip_children=true` on changed body.rs/body_tests.rs/thinking_body.rs: success.
- First broad scoped `rustfmt --check` found a pre-existing single long ReadingOwner call in phase.rs. Read and formatted that call only; no behavior change.
- Final `rustfmt --edition 2024 --check --config skip_children=true` on selection_menu.rs, body.rs, body_tests.rs, model.rs, phase.rs, context_control.rs, render_rows.rs, thinking.rs, thinking_body.rs: exit 0.
- Same final command chained read-only Python assertions: eight test declarations and all original six names; Linux primary cfg; two NSH LineHeight constructors; WeakEntity/source-domain binding; actual listener/install/directed-endpoint/currentness APIs; source continuity and anchor mapper; code AX/tab handlers; no unconditional top reset; display_text CopyOriginal; no conflict markers, new code comments or trailing whitespace in changed body/test/Thinking-body files: exit 0. These are source observations, not Rust compilation or executed tests.
- No Cargo/build/rustc/test/native app/Git/network/CI command run. Parent's historical five-error log does not validate this delta.
- Atomic edit mismatch had no effects; inspected exact current text before corrected edit. Historical no-change/target-changed observations were treated as data and not replayed blindly.

## Stopped SHA-256

Paths relative to `crates/vega_ui/src/conversation_stream/` unless noted:

| Path | SHA-256 |
|---|---|
| selection_menu.rs (unchanged followup) | `7fa6fca9ec7604125a3af5173cdc80e96c930bbf103bedd0d499793681d81d11` |
| selection_menu/body.rs | `ad5343a7b4e1a6b17d608ec795d6ea87c16ed72a79aa4e2838b99e1fc7e338eb` |
| selection_menu/body_tests.rs | `e7b0711c724a281220a541891a211384662d7735c67cd3c0ef9491c9f4e2dcbb` |
| model.rs (unchanged followup) | `36c838210d65629f456b7afa944ddf55110756cd6bfc2a65b2c29040f8337ff9` |
| phase.rs (format only) | `7de75f2e475c2bad6ecb53ca28a081362e83459bcb35b1312e41cef3f75fc9dc` |
| context_control.rs (unchanged) | `25ff77d2807f4d266e29d361d6da89a1694e35985c19164a4038027ebda4cb9d` |
| render_rows.rs (unchanged followup) | `cf2e5b9e160b612007087eebce2b2bd29905497deded9d61570b2daf947b2298` |
| thinking.rs (unchanged followup) | `8c2a6d163b9e86f728721d4504d2e89c8e81bddb0e7d79d690879c01fd74bc17` |
| thinking_body.rs | `abf18a46090b30798dcad13ba87974bfc5da672e7ff4433b9105051896deb805` |
| docs/907/nsh-reading-core.md | `f9e979ca2c3d0c5a0ba56547a4af3b8faee4c0cbb6db6e538ce09ec9a2fd6e21` |
| docs/907/nsh-reading-backend-contract.md | `bf352de104c369bf1893a1b076a78a3e8aa22b4d0a08551e672f61416738fdcb` |

The body hash was independently recomputed with Python hashlib from current 83,348-byte content and agreed with shasum; current source contains the actual remap/install implementation. Future changes by parent invalidate these identities.
