# #514 TEST-ONLY writer READY evidence

## Stage and content identity

- Stage: TEST-ONLY + harness observation correction; production fix not started. READY is for independent review, not GREEN or integration approval.
- Branch: `feat/514-continuous-quotes`.
- Unchanged HEAD: `7ec15f2c44269d7e068e853ed797f651668dc019`.
- Parent-owned v3 spec SHA256: `cf733965f647607b95fec9f95177e20334a01de1894b6e59ac39a90fca50e159`; verified before writes and after source completion; not edited by this writer.
- Source patch SHA256: `f06d0ec343fd407b0595b5d7f03a64e4eb9628c366123b9731969849da2cafbf`.
- Patch hash recipe, from the worktree root: concatenate `git diff --no-ext-diff --binary HEAD --` for the six tracked source paths below, in the displayed order excluding the new test file, then `git diff --no-ext-diff --binary --no-index /dev/null crates/vega_markdown/src/stream_boundary_tests.rs`; hash the concatenated bytes with `shasum -a 256`. The no-index diff exits 1 because it describes a new file, not because validation failed. Parent-owned documentation is excluded.
- All source changes remain unstaged; Git/index, integration, commits, push and CI belong to the parent.

| Writer source path | SHA256 |
| --- | --- |
| `crates/vega_markdown/src/lib.rs` | `a07bc52bc9927491251a0ee6381c011b481725b3d24f3b0d050f7d124b6fe5b9` |
| `crates/vega_markdown/src/stream_boundary_tests.rs` | `d44043eca42a48840423c5c10fbe3d86768607bcdaec6ea238e4a5a38908b472` |
| `crates/vega_ui/src/conversation_stream/inline_code.rs` | `79c1ee767cc41f30d4c12dea63acce076e181a9216d4b2036e344d3370bd5a62` |
| `crates/vega_ui/src/conversation_stream/list_marker_behavior_tests.rs` | `dc9938517dae1a9df334791939324c58ec7fead3850ac1dd1da9969395f73e72` |
| `crates/vega_ui/src/conversation_stream/quote_behavior_tests.rs` | `a87ff1e7183a4ab8b8f15d5039ef9113ddbd3550cf3fb0ee2e85e0337b44f3c6` |
| `crates/vega_ui/src/conversation_stream/render_rows.rs` | `2ea921c2883eb22ca27bab74cbacc5b282965f840bdebf0eb1dd2ef507ba1bc5` |
| `crates/vega_ui/src/conversation_stream/selection.rs` | `243d067beb12db799a0b0656d8c6a481818252f95b1d83a916cca1395977844a` |

This evidence file is the only writer-owned documentation addition. The parent-owned spec and continuation evidence are outside this source patch.

## Test inventory and expected discrimination

Existing 19 definitions retained: 15 quote behavior tests, the unchanged colocated Thinking lifecycle test, and 3 list marker tests. No test removed, ignored, skipped or retried; the original UI copy string `before\n\n• code-first\nsecond-line\n\nafter` is unchanged. H1 marker geometry, H1 size and `Typography::HEADING_PAGE_WEIGHT` remain required in Ordinary, Selectable and Thinking, both quoted and unquoted.

Five new definitions in `stream_boundary_tests.rs`, authored from v3 acceptance against the current public `MarkdownStream::new / append / snapshot / finish` API, with no parser production changes:

| Test | Observable acceptance and intended baseline role |
| --- | --- |
| `list_fences_keep_following_prose_outside_code_across_chunk_schedules` | Explicit paragraph/list-item-CodeBlock/following-prose tree and exact code string. Backtick + tilde; quoted + unquoted; whole-input, Unicode-character and syntax-boundary schedules. Unquoted no-blank closing fence is an expected RED target; quoted variant is a control. |
| `list_fences_preserve_internal_and_terminal_blank_code_lines_verbatim` | Explicit complete tree and code string `first中\n\nsecond🙂\n\n`; both fence types and quote variants under all three schedules. Guards true internal/terminal blanks against trim or newline deletion. Expected preservation control. |
| `top_level_fences_after_lists_remain_separate_blocks` | Genuine unindented fence directly after a list, without a blank line, remains a separate top-level CodeBlock and committed block; both fence types and all schedules. Detects a blanket suppression of list-mode fence interruptions. Expected control. |
| `list_fence_snapshots_keep_unrelated_committed_nodes_frozen` | Explicit open-code, appended-code, closed-fence and following-prose snapshots for both fences/quote variants. The same nonempty committed stable paragraph retains content, ID, version and borrowed-node allocation across append and finish. Unquoted close snapshot is an expected RED target; quoted and cache behavior are controls. |
| `late_reference_definitions_update_only_the_affected_committed_block` | Literal unresolved reference becomes the explicit expected link URL/label after a later definition. Affected committed ID stays stable and version advances once; unrelated nonempty committed paragraph retains ID/version/allocation/content. Expected selective-invalidation control, not full reparse. |

All five tests require explicit final trees, no pending tail after finish, and idempotent second finish with unchanged committed identity/content. Chunk schedules use valid UTF-8 character boundaries and cut at list markers, quote markers, each fence character and newlines. Expectations are explicit node construction, never comparison of two parser outputs.

Source definition count is 24 (19 retained + 5 new); discovery/execution/pass/fail/skip counts for this patch are NOT RUN / pending Actions. The existing UI copy regression should still fail on the unchanged dependency. No precise future execution counts or results are claimed.

## Font observation correction

- Verified pinned `gpui-pre 0.3.4`: `NoopTextSystem::font_id` returns ID 1; `layout_line` ignores its input font runs and emits shaped ID 0. Reverse lookup of that dummy shaped ID is not a valid font-weight observation.
- Ordinary text: `inline_code::styled_text` passes the same real run vector used by `StyledText::with_runs` to the cfg(test) `ObservedText` wrapper. Inline-code/link fragments observe their existing actual `self.runs`.
- Selectable text: cfg(test) storage clones the actual vector used by `StyledText::with_runs`, including flow-fragment paint runs, before it is consumed. Prepaint records those runs alongside the same visible content and real `TextLayout`.
- Thinking: existing `PreparedFragment.runs` flow through `append_positioned`; the observer captures that same vector before `with_runs`. No Thinking preparation, shaping, culling or lifecycle production code changed.
- For genuinely unstyled list markers/checkboxes only, the observer captures `window.text_style().to_run(content.len())` at `request_layout`, matching GPUI's unstyled fallback. It does not substitute defaults for styled body/heading runs.
- `PaintedText` still filters by the real layout/content-mask intersection, and now binds runs to that record. No helper-only or expected-token-derived observation.
- The existing heading test retains the exact H1 weight expectation, requires complete run-length coverage and consistent heading weight, and distinguishes real normal-weight body text at the conversation size in the same rendered frame. Observation does not claim native font rasterization or real system integration.
- New mixed-file fields, parameters, clones and calls are cfg(test); the non-test token stream is unchanged apart from formatting after removing cfg(test) additions. `render_rows` edits are confined to existing cfg(test) wrappers. New `expect` calls occur only in test-gated code. No new comments or dependencies.

## Static validation and preserved scope

Executed locally, without compiling or running Vega/Rust/Python tests:

1. `rustup run 1.98.0 rustfmt --edition 2024 --check --config skip_children=true` over the seven writer source files: exit 0 after formatting. Pinned formatter reports `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`.
2. `git diff --check`: exit 0.
3. `git diff --exit-code HEAD --` for Markdown `stream.rs`/`nodes.rs`, conversation `model.rs`/`thinking_body.rs`/`message_anchors.rs`, root/crate manifests, `Cargo.lock`, workflows: exit 0, unchanged.
4. Focused source inventory: 15 + 3 + 1 retained UI definitions, 5 new parser definitions; cfg(test) module wiring inspected. Added-line comment/ignore scans found none. Original copy assertion inspected unchanged.
5. Read-only HEAD, branch, status, file hashes and spec hash verified. No nested AGENTS files on scoped paths. Only writer source/documentation and parent-owned documentation are dirty.

Retained diagnostic history: the first formatter invocation used `rustfmt +1.98.0`, but this machine's rustfmt is a direct binary and interpreted the toolchain selector as a filename; exit 1 also printed genuine formatting differences. The responsible command layer was corrected to `rustup run 1.98.0 rustfmt`; formatting was then applied with exit 0 and checked with exit 0. A guessed registry `stream/semantics.rs` path was absent; the actual read-only source is `semantics/mod.rs`, `semantics/references.rs` and `reference.rs`. Neither observation failure is a test RED.

No Vega execution, compilation, cargo check/Clippy/test/nextest, Python tests, build/package, Harness search/execution, network/Git mutations, commits, push, CI dispatch or CI queries were performed. Registry sources were read-only. Existing production and all tests remain present.

## Provenance, order and next boundary

- Original retained test provenance and prior cycle results are recorded in spec v3: quote freeze `bffce7b6c9eac5e83b8efb0241d690dba74b2ed8`, list marker tests `b21f635eecb256fdb13c4177df0680eccbea254d`, existing implementation through `ae1888267fd8b05db1c388c01ed80bfc40643a1b` merged into the unchanged baseline HEAD.
- Prior Actions `39578674519` / Nextest `119150532907` had 19 executed, 17 passed, 2 failed, 0 skipped; Clippy `119150532737` succeeded. That evidence is inherited, not current-patch validation.
- New parser tests are this writer's v3 test-first source, not restored upstream or historical tests, and do not retroactively prove the earlier quote/list implementation. No mdstream patch, vendoring, migration or production repair was made.
- Parent next: independent review of this stopped version, integrate the same source patch into existing Draft PR #529, obtain valid cloud RED, freeze tests, then authorize a concrete minimal mdstream 0.3 followup. No production fix before that boundary.
- Open limitations: local type/borrow/lint execution and new RED/GREEN are unavailable by policy; static review/formatting does not replace them. Native selection/clipboard/links/scroll experience remains user manual validation. No known acceptance gap or spec deviation.
- Writer began `2026-10-22T06:54:52Z`; final READY UTC and real elapsed active interval are reported with the final file hashes in the handoff. There was no intentional external-result wait; this bounded session was active rule/source review, test/observer writing and static validation.
- On final READY, this writer stops all scope writes before independent review. This is not a commit, push, CI request, approval to fix production, or claim of GREEN.
