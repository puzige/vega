# #514 v4/v5 boundary round: TEST-ONLY READY evidence

## Stage, authorization and exact content

- Stage: new boundary tests before independent review and new cloud RED. No production fix in this round, including no repair of the already failing v3 tilde pending case.
- HEAD: `27b88c2e9934a01aa912ed95fc26b6ec3fd3470a`; branch `feat/514-continuous-quotes`.
- Parent fetched at 08:32:56Z and confirmed master `e4f18c656aa69d192d095a4338a2108e6c9a3c2c` before this delegated task. Git/index, commit, push, CI and integration remain parent-owned.
- Initial v5 spec SHA256 at 08:36:44Z: `1d259ed160b99cc483e0550f9d7c5e8bdadebb841c33edbb6539c91120d0edc4`.
- Parent clarified v5 before test freeze with literal code-body protection, pending snapshots and source-preservation requirements; current spec SHA256 read at 09:01 and verified again at 09:05: `50fdca3643ed8c11c9655ee8a16680388d2f03589b3efafcda046d9426084aa8`. New tests include this clarification. This writer did not edit any parent-owned document.
- Test source diff SHA256: `b5df8f9f475cb82e5821806613e93423d3eaa92861a735ffb4a1b695dbc8c3cf`.
- Two-source file-manifest SHA256: `deb96bc88a1ca5a2b14c4f4b389ddfdc69277b53a653f89451fa7a46a15cb8dd`.
- Diff hash recipe, from the worktree root: SHA256 of the exact stdout bytes from `git diff --no-ext-diff --binary HEAD -- crates/vega_markdown/src/stream_boundary_tests.rs crates/vega_ui/src/conversation_stream/quote_behavior_tests.rs`. Parent documents and this evidence are excluded.
- File-manifest recipe: the two lines in the following code block, with two spaces between SHA256 and path and LF after each line. This evidence file and a combined writer file-manifest are hashed separately in the final handoff to avoid self-reference.

```text
41349d1a384a222053b8705d29b0dd6ae2a7cbd32d3c3bdc48b3f6011eb75826  crates/vega_markdown/src/stream_boundary_tests.rs
3b924f56957d96c3d849cc1406cae3ffcf8d40754e15fcf9ff64850afb3fe3a7  crates/vega_ui/src/conversation_stream/quote_behavior_tests.rs
```

## Complete writer file inventory

1. Append 133 lines to `crates/vega_markdown/src/stream_boundary_tests.rs`, in new nested module `list_fence_ownership_tests`.
2. Append 307 lines to `crates/vega_ui/src/conversation_stream/quote_behavior_tests.rs`, in new nested module `container_order_tests`.
3. Add this evidence document: `docs/514/vega-issue-514-boundary-round-test-evidence.md`.

No other writer-owned changes. Existing module gates already make both files test-only; no new wiring, production seam, visibility change, field, dependency, workflow or vendor file is needed. The current dirty spec and continuation evidence are parent-owned, not writer changes.

## Existing tests preserved byte-for-byte

The source bytes from HEAD are exact prefixes of the two appended files, verified by byte comparison, not just assertion counts:

| File | Frozen prefix bytes | Frozen prefix SHA256 |
| --- | ---: | --- |
| Markdown `stream_boundary_tests.rs` | 10801 | `d44043eca42a48840423c5c10fbe3d86768607bcdaec6ea238e4a5a38908b472` |
| UI `quote_behavior_tests.rs` | 35114 | `a87ff1e7183a4ab8b8f15d5039ef9113ddbd3550cf3fb0ee2e85e0337b44f3c6` |

All other retained tests and observation plumbing are unchanged. The original 24 definitions remain: 15 quote tests, 3 list-marker tests, 1 colocated Thinking lifecycle test and 5 parser tests. New source counts are 19 + 3 + 1 UI definitions and 8 Markdown definitions, totaling **31 source definitions**, not a claimed discovery/execution result.

The old `list_fence_snapshots_keep_unrelated_committed_nodes_frozen` still has its original tilde pending assertion. Its failure remains separate v3 product evidence. No original assertion was edited, deleted, ignored, retried, skipped, trimmed or weakened.

## Test inventory and intended discrimination

### v4: true quote/list ancestor order

Four new GPUI test definitions share a small explicit fixture/geometry helper inside the existing test module. Each uses the public `MarkdownStream` output before geometry: explicit expected trees are checked after append and after finish. The rendered source is that same source through `open`, not a supplied `RenderNode` override. No structural fixture fallback is used.

| New test | Expected quote/list order | Expected rail / quoted text x relative to ordinary body | Baseline role |
| --- | --- | --- | --- |
| `list_owned_quote_rails_start_at_the_list_content_column` | List -> Quote | 24 / 38 | Target RED: current rail is outside the list inset. |
| `nested_list_owned_quote_rails_keep_both_list_insets` | List -> List -> Quote | 48 / 62 | Target RED: nested list ownership cannot be reduced to quote depth. |
| `outer_quote_rails_do_not_follow_descendant_list_insets` | Quote -> List | 0 / 14 for quote paragraphs; list item text remains at 38 | Positive control: moving every quote by descendant list inset is invalid. |
| `inner_list_owned_quote_rails_preserve_the_outer_quote_origin` | Quote -> List -> Quote | outer rail 0, inner rail 38 / inner text 52 | Target RED: inner rail follows its owner, outer rail stays at 0. |

The arithmetic is the frozen spec's: list indent 24, rail width 2, quote padding 12, gutter 14. Assertions use shared `Layout` tokens and observed ordinary-body origin, not the observed wrong rail as their origin. List item text columns and parent item columns are checked too, preventing double inset or marker movement.

Each test covers Ordinary, Selectable and Thinking via existing real rendering paths, at 240 -> 760 -> 240 logical pixel widths. It checks:

- Actual clipped 2px rail quads from existing `frame` observation (quad content-mask intersection and scale conversion unchanged).
- Exact rail/text columns, full coverage across the two paragraphs and the required 16px gap, and only the expected owner rail positions.
- Outer rail continuous through nested content, inner rail absent before/after its own range, and no quote rail over outside prose or list item text not owned by that quote.
- Unchanged outside prose origin, visible text contained within the available width, real reflow at wider width, and restored narrow geometry without stale rails.
- Reassembled exact paragraph text bound to real layouts, accommodating Thinking's physical wrapped fragments without dropping or trimming any characters.

Fixture feasibility was checked statically against current boundary handling. List-owned quote entries follow a blank line so `after_blank_line_decision` keeps the actual list container; adjacent quote entries that would interrupt List mode are deliberately not used. Public parse assertions still must execute in Actions before calling those preconditions confirmed. A parsing failure is not a v4 geometry RED and must be diagnosed without hiding it behind an override or changing expectations.

A static observation found that the current gap function selects its existing 8px list-paragraph gap for quoted list descendants before reaching quote handling. The new assertion requires the explicit v4 16px quote gap. That requirement is not weakened to current output. Current rail-origin assertions precede that gap assertion; cloud logs will establish the actual first failure.

### v5: list fence ownership and literal bodies

Three new parser test definitions use the existing public stream API and existing test helpers. All expectations are explicit `RenderNode` trees and literal code strings, not comparison between two parsed outputs.

| New test | Required public result | Baseline role |
| --- | --- | --- |
| `dedented_fences_after_wide_ordered_markers_keep_heading_syntax_as_code` | Ordered list start=10/item, then separate text-language CodeBlock containing exactly `# literal\n`, then Unicode prose. Two/three leading spaces before the top-level opener/close; backtick/tilde fences. | Target RED for the fixed >=2-space continuation heuristic; no Heading, extra code block or swallowed prose permitted. |
| `ordered_list_fence_continuations_preserve_code_blank_lines` | Ordered start=10 with four-column continuation and start=100 with five-column continuation retain their list-owned code and exact `first🙂\n\nsecond\n\n`, followed by prose. | Positive continuation/blank-line control; must not revert the v3 closing-fence fix. |
| `list_fence_bodies_keep_block_syntax_literal_in_pending_and_final_snapshots` | Unordered and ordered start=10 list-owned code retain `# literal`, a blank code line, `---` and following code; open, blank, thematic, body and close snapshots contain only the explicit list/code tree. Closing code content is exactly `# literal\n\n---\nsecond\n`, then independent Unicode prose. | Target RED for ordinary heading/thematic/blank interruptions and synthetic pending repair inside true code ownership. Added under the parent's pre-freeze v5 clarification. |

Both fence types use whole-input, per-Unicode-character and syntax-boundary schedules. The ordered syntax schedule cuts at marker digits/dot, whitespace, each fence character, heading markers and newlines; every cut is at a valid UTF-8 boundary. Final append snapshots and finish results are checked; the inherited finish helper also requires no pending tail and idempotent second finish with unchanged committed identity/content. The literal-body test additionally checks meaningful intermediate line-level snapshots before the complete schedule cases.

No test runs mdstream directly or relies on new production APIs. No upstream test set was restored. Existing v3 tilde pending assertions remain untouched, not replaced by these tests.

## Original evidence, order and current limitations

Inherited raw results supplied by the parent:

- [Actions 39746319972](https://github.com/puzige/vega/actions/runs/39746319972), exact current HEAD: Nextest job `119715543602` started/executed 24, 23 passed, 1 failed, 0 skipped, summary 1.179s; Clippy job `119715543343` SUCCESS; Python 60 passed. Compilation selected vendor/mdstream 0.3.
- Full parser input and original UI copy now pass; remaining v3 failure is the original tilde/unquoted pending snapshot with an extra `Paragraph("~~")`, not an observation fault. Parent recorded raw evidence at [PR #529 comment](https://github.com/puzige/vega/pull/529#issuecomment-6336398417).
- v4 and v5 were independently discovered boundaries, specified before these tests. That prior v3 failure is not RED evidence for v4/v5.

This round authors new tests against the unchanged production baseline. No further mdstream, pending repair, UI, model or rendering implementation was made. The parent clarified new v5 literal protection before freeze and before any new production fix; this clarification is recorded, not inferred from executed output. There was no failed test execution and no retry-to-green in this writer session.

All new discovery, compilation, Clippy and behavior results are **NOT RUN / pending cloud Actions**. A locally passing formatter is not a type/borrow check. The supported-source parse preconditions, wrapped-text reconstruction and repeated-width geometry remain runtime preconditions to verify in the first cloud round; no native UI, clipboard, network or OS integration is claimed. No known acceptance gap; deviations from the current parent spec: none.

## Static verification

Executed locally without compiling or running Vega or tests:

1. `rustup run 1.98.0 rustfmt --edition 2024 --config skip_children=true` on the two appended test files: exit 0.
2. `rustup run 1.98.0 cargo fmt --all -- --check`: exit 0 after final source additions.
3. `git diff --check`: exit 0.
4. Python standard-library byte comparison of both frozen prefixes and all other tracked files except the two parent-owned documents: exit 0; 1085 other tracked regular files byte-equal to HEAD at the 09:05 check.
5. All 47 vendor files (mdstream and block) byte-equal to HEAD. Their sorted `<sha256><two spaces><path><LF>` manifest SHA256: `40f3e5b4b460dfa957d69a453a91276b32dc0c1c71442bd5a4840215898fb9ba`.
6. Root/crate manifests, lockfile, workflows, parser production, UI production, old test files/observers and prior writer evidence unchanged; focused `git diff --exit-code HEAD --` protected paths: exit 0.
7. Appended-source static scan: no added comments, ignore/should_panic, trim calls or dependency edits; 3 new Markdown and 4 new UI test declarations. Source diff contains only insertions. Index diff empty: exit 0.

No local Vega execution, compile/build/package, cargo check/Clippy/test/nextest, Rust/Python test, Harness search/execution, network/CI action or Git/index mutation was performed. Python was used only for low-CPU source/hash/static comparisons. Parent-owned docs were only read.

## READY boundary and timing

Writer start: `2026-10-22T08:36:44Z`. The final handoff records actual ending UTC, elapsed active interval, final evidence hash and combined writer manifest hash. Work included rules/API review, append-only tests, the parent's pre-freeze spec clarification and static validation; no external-result wait was inserted.

At final READY this writer stops all scope writes. Parent next: independently review the exact stopped patch, integrate through parent-owned Git/index and existing Draft PR #529, execute the complete frozen 24 plus 7 new tests in existing Actions, and confirm separate valid v4/v5 target REDs before any concrete production followup. Existing v3 failure remains separately classified. Keep the complete combined suite until GREEN; no fixes after READY without a precise followup.
