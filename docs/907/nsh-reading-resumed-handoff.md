# Resumed #907 reading writer — stopped, incomplete integration

Worktree: `/Users/puzige/Workspace/worktrees/vega-907-nsh-full`.
User-supplied baseline HEAD: `0b5ea426abfd8d4358489e916273d89474fb190f`; deliberately no Git command to recheck or change it. Seven original scoped file hashes matched the user's recovery record before edits. Existing menu/commands/owner work was preserved rather than recreated.

## Implemented in this execution

- Explicit GPUI IntoElement import for the selection menu's derive ambiguity; unused closure context renamed.
- Restored CopyOriginal `display_text()` notice projection, including the menu's captured and current original snapshot, rather than raw `original_text()`.
- StreamModel retains original structured RenderNode Arcs by block id/version alongside existing stream blocks/lines/cache; old Arcs are reused when unchanged. No whole-source parse per token.
- Added `selection_menu/body.rs`: concrete NSH retained Document/Block/Inline representation rendered with real NSH text/span, list/quote/table, syntax-highlighted code, native scrollbars and app-owned copy controls. No legacy GPUI text is wrapped in Native. Real GpuiTextEngine unchanged.
- Replaced main `render_markdown_content` production body and Thinking prepared GPUI body with the shared NSH body Surface. Thinking uses incremental MarkdownStream and retained visible-source prefix/start, rebuilding only when its retained window/source changes; removed its former body scrollbar and stage GPUI scroll tracking.
- ReadingRequest handler consumes actual exact snapshots. Side effects check trusted owner/root, source/generation, live mount/geometry, engine layout acknowledgment, current native snapshot or mounted tagged document, UTF-16 boundaries, fragment identifiers and source mapping. Primary selection is handled separately. Synthetic list markers and structural separators are explicit display fragments, never source-string identity guesses.
- ReadingBinding retains validated SourceSpans, display content/range and logical source endpoints. `observe_selection` is supplied for parent wiring, with immediate saved-snapshot invalidation on clear, deferred owner-safe application state updates, no clipboard or menu side effect. `copy(window,cx)` defers outside the stream/Thinking entity update and validates the actual current snapshot.
- Six new pure mapping regressions in `body_tests.rs`, UNRUN.

## Why concrete retained content instead of high-level TextView render_custom

TextView Renderer wraps even custom inline output in a View (`nsh-core/element/renderer.rs:122–136`). Core text collection makes a non-text child a U+FFFC attachment (`core/mod.rs:5669–5722`). A custom inline callback therefore cannot supply genuine selectable tagged spans. The body consumes the concrete TextView Document API and constructs actual nested NSH text leaves; nested test_ids are retained in the exact AttributedText Arc. Lists/code use nested tagged text, since root test_id is not included in AttributedText.test_ids.

## Required parent next steps / not complete

1. Run scoped current-code Cargo/typechecking and the new development tests; no compile success is claimed. User restricted this writer to scoped rustfmt/static checks. Preserve development regression evidence and clean under project delivery rules.
2. Add and wire the platform selection listener to this exact body AppView and `ReadingBinding::observe_selection`. Current `ReadingIntent` exposes Copy/ContextMenu/PrimarySelection only. Continuous drag/select-all/clear/detach state observation is not complete without this seam.
3. Shared `conversation_stream/render.rs::copy_selected_text` currently directly invokes MessageCopy under stream update without window/root checks. Parent must delegate NSH copies to the binding's `copy(window,cx)`. This writer did not touch shared render/mod/core.
4. Supply a same-Surface exact-native-node/Arc UTF-16 endpoint install/remap API. Native SelectionModel follows text keys and clears when an old endpoint disappears (`selection.rs:246–293`); AppView currently only reads/current-checks selection snapshots (`app.rs:394–410`). Retained logical SourcePosition is available but cannot restore native selection/highlighting, direction or source-trim anchor through the current API. On source-window rebuild non-follow scrolling currently resets to top; this is NOT acceptance-preserving source-trim behavior.
5. Native code-copy/link keyboard and AX proxy, link-preview parity, code/table layout parity, streaming fade, pending-node selection across append, root/preview/hidden/resize/scale, native scrollbar and selection acceptance remain unverified or incomplete. Link-preview/fade integration is not complete. Do not call the full #907 migration finished or ready for final acceptance.

Ordinary block-anchor scrolling uses engine-measured viewport-relative row geometry and actual native offsets; it is not guessed character width and is not claimed to solve source-window remap. Code copy uses an app-owned NSH control with a retained binding id; built-in TextView `copy` is never invoked. Alternative feasible API: `code_block_actions([TextViewCodeAction::new("vega-copy-source")])` reports a non-copy id before clipboard writes. Parent clipboard takeover is defense in depth.

## Validation actually executed

Final `rustfmt --version` readback: `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`.

Final `rustfmt --edition 2024 --config skip_children=true` and `rustfmt --check` on selection_menu.rs, selection_menu/body.rs, selection_menu/body_tests.rs, model.rs, render_rows.rs, thinking.rs and thinking_body.rs: success, no output.

Read-only Python source assertions for actual bridge usage, UTF-16 converter presence, real NSH spans, no old body Native embedding, model-derived display, notice projection, retained version-cache use, no high-level custom-inline attachment renderer, no private Spawned access and six regressions: success. No conflict markers/trailing whitespace in the checked files. The first diagnostic matched `previous.remove` literally and failed after rustfmt split the expression across lines; evidence readback confirmed the cache implementation and the same assertion was rerun as a whitespace-insensitive regex, without product changes. This is a static observation check, not a test or compiler.

No Cargo/rustc compilation, tests, app, Git, network, native acceptance or CI run. No vendor/Cargo/shared mod/core/render change by this execution. phase.rs and context_control.rs stayed at the recovered hashes. No running shell session or background process was started.

## Final SHA-256 (code stopped before this document)

All code paths below are under `crates/vega_ui/src/conversation_stream/`:

| Path | SHA-256 |
|---|---|
| selection_menu.rs | `7fa6fca9ec7604125a3af5173cdc80e96c930bbf103bedd0d499793681d81d11` |
| selection_menu/body.rs | `0eda46624c90effb8689a5253ebf8e655439bf9a333609f9d9f89d5e2b196e25` |
| selection_menu/body_tests.rs | `81a7accd5ad775cc0fdd2ff06075a4bdda67fc9ea8d4cfc5d5f2f96ff51d13b7` |
| model.rs | `36c838210d65629f456b7afa944ddf55110756cd6bfc2a65b2c29040f8337ff9` |
| phase.rs (unchanged) | `89ee5313c43a805ce3eb4eeb575a1a8afc4e166dd94ae4c0a2794c0d2a9887ce` |
| context_control.rs (unchanged) | `25ff77d2807f4d266e29d361d6da89a1694e35985c19164a4038027ebda4cb9d` |
| render_rows.rs | `cf2e5b9e160b612007087eebce2b2bd29905497deded9d61570b2daf947b2298` |
| thinking.rs | `8c2a6d163b9e86f728721d4504d2e89c8e81bddb0e7d79d690879c01fd74bc17` |
| thinking_body.rs | `b28ebe66d232a5d79164dbd6c01f912bd242a3822a62a9f9b0be7143fa1286c4` |

Reading docs:

- `docs/907/nsh-reading-core.md`: `5eb34b05b5104f923c6128470854ba34483cf2dddf80bb64efe526b662c4e24b`
- `docs/907/nsh-reading-backend-contract.md`: `98a2bf1e36331b9c8aa91722c12de900fc43acd2b27ea0698e330cdfd2403d04`

This is a stopped recovery handoff, not a validated production-complete stage.
