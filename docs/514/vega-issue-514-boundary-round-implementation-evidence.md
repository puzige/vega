# #514 boundary-round implementation — replacement writer READY evidence

## Freeze

- Stage: **READY for independent implementation review and same-version cloud verification**, not reviewed/GREEN/merged/user-accepted.
- Branch: `feat/514-continuous-quotes`.
- HEAD: `e7afdfbcf40263f13831766148cc0bd58de9dc67`.
- Contract: [continuous quote boundaries, v5](vega-issue-514-continuous-quote-boundaries.md), SHA256 `50fdca3643ed8c11c9655ee8a16680388d2f03589b3efafcda046d9426084aa8`.
- Recovery input: [implementation checkpoint](vega-issue-514-implementation-recovery.md). The previous execution was confirmed interrupted before this replacement; the parent supplied the existing worktree and sole implementation-writer role. Parent owns Git/index/commit/push/CI and other dirty documentation.
- Full implementation diff SHA256: `08681f4d54c01be4e8ae7de3bfc6a7520f885d700a15549605b9b7dcd1ba1d81`. This is **identical to the recovery input**; no source or vendor README bytes were changed by this replacement.
- Inventory: 4 existing UI production files, 9 existing vendor Rust files, 1 existing vendor README, and 1 added private vendor Rust module. The retained diff contains 14 tracked paths (+224/-55), plus the new 219-line module: 15 implementation paths, +443/-55 overall.
- Implementation file-manifest SHA256: `ec42fcf156458682f089416fe48f9e973a53c6991030ebd77c7b1b950b104dfb`, over the 15 SHA256/path lines in the inventory below, in displayed order, each ending in LF.
- Final implementation/static verification: `2026-10-22T10:55:43Z` / `2026-10-22T18:55:43+0800`; report timing/footer finalized afterward. Environment: Darwin arm64; repository toolchain `1.98.0`; `rustfmt 1.9.0`; Cargo `1.98.0 (797e8a9bc 2026-08-05)`; Git `2.55.0`.

The implementation diff excludes this evidence document and parent-owned documentation. Reproduce its identity from the stated HEAD without changing Git state:

```sh
{
  git diff --no-ext-diff --binary HEAD -- \
    crates/vega_ui/src/conversation_stream/message_anchors.rs \
    crates/vega_ui/src/conversation_stream/model.rs \
    crates/vega_ui/src/conversation_stream/render_rows.rs \
    crates/vega_ui/src/conversation_stream/thinking_body.rs \
    vendor/mdstream
  git diff --no-ext-diff --binary --no-index -- \
    /dev/null vendor/mdstream/src/stream/fence_context.rs
} | shasum -a 256
```

The no-index diff exits 1 because the new file differs from `/dev/null`; that is expected, not a product or validation failure. The resulting SHA256 is the full implementation identity above.

## Inherited implementation and bounded static trace

### UI ownership and geometry

- `model.rs::QuoteOwner` retains `(block_id, node_identity)` and the optional list depth present when that quote container is created. `flatten_node` distinguishes no list ancestor from list depth zero and passes the same context through quote children. All actual `flatten_node` call sites were located and agree with the updated signature; no public parser API is added.
- `render_rows.rs::quote_blocks` applies only the list-inset difference relative to the parent quote owner. `markdown_blocks` subtracts the already-consumed quote-owner list inset from remaining body indentation. Both ordinary/selectable paths still share this wrapper composition, including `flex_1`/`min_w_0` remaining-width constraints.
- `thinking_body.rs::PreparedBody::new` uses the same owner list origin for rails and retains total list-plus-quote indentation for continuation text. Existing body-level rail collection/painting, source mapping, selection, y-offset and culling paths are not rewritten.
- `message_anchors.rs::estimate_markdown_height` consumes the shared total list-content inset and existing quote gutter. The quoted-list paragraph gap remains the existing 16px block-gap token; list-item spacing retains its existing branch.
- Static arithmetic agrees with the frozen v4 targets: List→Quote rail 24/text 38; nested List→Quote rail 48/text 62; Quote→List outer rail 0; Quote→List→Quote inner rail 14+24=38/text 52. This is a source trace, **not executed layout evidence**.
- No new UI copy, source rewriting, trimming/copy compensation, selection/link changes or #485 drag/autoscroll work is present in the retained diff.

### Parser ownership, literal protection and lifecycle

- New crate-private `stream::fence_context::FenceContext` records ordered quote/list containers, list-marker content columns, active fence character/length/owners, and protected raw-byte ranges. `Cursor` advances over ASCII marker/whitespace bytes and accounts for tabs; the raw line bytes are not rewritten.
- `machine::is_new_block_boundary` passes `protects_line` to `BoundaryDetector` for List/BlockQuote modes. The detector checks an owned fence line before ordinary blank/heading/thematic/fence interruptions. Actual marker widths distinguish the frozen 2/3-space dedented fence after `10. ` from genuine 4/5-space list continuations. An owned close ends the internal fence, not the outer List block.
- `process_line` is newline-gated; `update_mode_with_line` advances fence state on completed lines. Its early return prevents a just-committed heading/thematic line from updating the next block's state. Incomplete-tail boundary detection consults context without advancing it.
- `MdStream::pending_fences` borrows maintained context when there is no nonempty incomplete tail; otherwise it clones context and previews exactly that tail with `complete=false`. A partial closing fence cannot permanently close active state. Borrowed context, input and options are disjoint from the mutable pending pipeline/transformer fields; `Cow<FenceContext>` dereference supplies the actual `&FenceContext` parameter. This is manual API/borrow tracing, not compiler verification.
- All three pending-display call sites pass the context and raw-start offset. `PendingRepair::apply_with_fences` intersects protected ranges with the current raw slice, copies literal bytes first, and applies existing prose repair only between/after them. Protection precedes trailing-space/setext/inline repair; retained closed-fence ranges remain protected, avoiding synthetic `~~` from tilde fences without UI compensation.
- Range endpoints come from UTF-8 input line boundaries; preview ends at the input tail, completed lines include their LF, and compaction subtracts the same removed-byte count from both input origin and range coordinates. No byte-indexing operation was added that advances through a non-ASCII character.
- `BlockMachine::new/reset/start_next_block_after` clear block-local fence state. Finalization clears it and pending display after the existing commit transition, without changing block IDs. `maybe_compact_buffer` rebases retained ranges and preserves the existing processed-line adjustment/cache clear.
- The SingleBlock-footnote transition resets once, then observes completed lines using the existing cursor; subsequent appends continue from that cursor. It does not reset/rebuild fence state on every append. Incomplete tails use the same preview path.
- The standalone public terminator reaches `PendingRepair::apply` and `FenceContext::for_text`; streaming append/snapshot call `apply_with_fences` with maintained context instead. No extra per-append whole-buffer pulldown parse was added. Existing top-level code-fence incremental fast path, transformers, adapters, committed cache identity/reference invalidation interfaces remain intact.
- No obvious missing integration or minimal source defect was found in this bounded pass. No implementation rewrite, new acceptance case, new test, dependency change or API expansion was necessary.

## Vendor provenance and semantic boundary

Inherited provenance is documented in [vendor README](../../vendor/mdstream/README.md) and the parent-owned [continuation evidence](vega-issue-514-continuation-evidence.md#mdstream-03-来源核对只读静态证据):

- Published archive: <https://static.crates.io/crates/mdstream/mdstream-0.3.0.crate>, SHA256 `f3327364edd79c4f66b36db667c0a28e078389351b0855e5f95ed4f22d778821`.
- Release VCS commit: `950d0926f54667f04aed87fad6cc9921aa74ef5c`, subdirectory `mdstream`. This replacement rechecked the cached archive checksum and extracted `.cargo_vcs_info.json`; both match. No fetch/network query was used.
- All 39 inherited Rust files came from that exact 0.3.0 release, with the previously documented lexical comment removal, formatting, and omission of the two test-only tail modules. This replacement did not repeat or claim a new full upstream token-equivalence audit.
- Of those 39 inherited Rust files, 30 are now byte-identical to HEAD and 9 carry the authorized semantic/lifecycle diff. SHA256 of the sorted unchanged-source SHA256/path manifest: `ced519a19f9cbd0dafd953fe004a9cd4ebc54ea4eaa068152ee53c90bce075c9`. The private `src/stream/fence_context.rs` is Vega-authored, not upstream release source; the current tree has 40 vendor Rust files.
- README notice now separates the original 39 files from the private module and names ownership, pending repair and lifecycle changes. No incompatible 0.4 migration, dependency/lockfile edit, upstream tests, fixture or dev dependency was introduced by this recovery.
- Rechecked unchanged licenses: MIT SHA256 `4670b75aa0fca969a99cb3ea2bd5175055ac05eefaf8edffefa8c1cc586ede9a`; Apache-2.0 SHA256 `5bedb04d972252dd9fd5bbe03eb9998c66fab819a3951467b612ffe32fed5e11`. Attribution and dual-license terms remain inherited and unchanged.

## Frozen test and business invariants

The frozen source still defines **31 tests**: 19 quote behavior, 8 parser boundary, 3 list-marker, and 1 embedded Thinking lifecycle. This count is static source inventory, not discovery/execution or GREEN.

| Frozen file | SHA256; raw bytes equal HEAD |
| --- | --- |
| `crates/vega_ui/src/conversation_stream/quote_behavior_tests.rs` | `3b924f56957d96c3d849cc1406cae3ffcf8d40754e15fcf9ff64850afb3fe3a7` |
| `crates/vega_markdown/src/stream_boundary_tests.rs` | `41349d1a384a222053b8705d29b0dd6ae2a7cbd32d3c3bdc48b3f6011eb75826` |
| `crates/vega_ui/src/conversation_stream/list_marker_behavior_tests.rs` | `dc9938517dae1a9df334791939324c58ec7fead3850ac1dd1da9969395f73e72` |

Additional byte comparisons against HEAD:

- Thinking model-input hook: 89 bytes, SHA256 `9261dcebee5d9bdc6f241937a44a4bfd378d222f6a2040ffd9f30801441f3309`.
- Complete Thinking test-helper/lifecycle suffix beginning at `quote_test_body`'s attributes: 7,321 bytes, SHA256 `0f6bd1dd0f567a19cc09b43a0b00222274c08cb278a9a33c9be07f472c734215`.
- Two existing render-row observer sections: 448/499 bytes, SHA256 `a5e81177efb020791834181963e1dcd64b81a19ba306b973f76132edb39c531d` / `4f2c564364cfd381910e6f56e3b1235098b3c70b88ce8dba04f4113da6d9b70d`.
- The protected 42-path manifest, compared with `git show HEAD:<path>` and `File.binread` without Git filters, has SHA256 `b201639960a1229dbfc80dc04cb16eb8a8f071dceed56454bdcd6ea061dbedfc`. It covers every tracked `**/Cargo.toml`, `Cargo.lock`, `.github/workflows/**`, `.config/**`, `rust-toolchain.toml`, `**/*tests.rs`, `**/*test_support*`, `**/*test_hooks*`, `scripts/**`, Markdown `lib.rs`/`stream.rs`, and conversation-stream `mod.rs`. All present matching paths are unchanged.
- Frozen assertions/data/skips/hooks were not changed or restored; no post-implementation tests were added. Only the stated production implementation is under review. Persistence, runtime configuration/data, business/security assertions, cache/reference contracts, Python scripts, Cargo and workflows were not changed by this replacement.
- Parent-owned recovery document SHA256 `bddc6000fa4313b21383f19a25a2cbcdf38f907b693fa073ca83d7a358f15ca7` and continuation evidence SHA256 `18fa1ed3d216ce8e5ce0c3f2022d4c681c7551a17e883e3bec44debc72bd5392` were observed, not edited.

## Results — local static checks only

All commands were run from the selected worktree; only relative repository paths are retained here.

| Check / exact command or procedure | Result |
| --- | --- |
| `git rev-parse HEAD`; `git branch --show-current`; `git status --short` | Exit 0; stated HEAD/branch; only retained scope, parent docs and this new report |
| `cargo fmt --all -- --check` | Exit 0, fresh at `10:49:15Z`; formatting only |
| `rustfmt --edition 2024 --check $(git ls-files 'vendor/mdstream/src/*.rs' 'vendor/mdstream/src/**/*.rs') vendor/mdstream/src/stream/fence_context.rs` | Exit 0; explicitly includes excluded vendor package and new private module |
| `git diff --check HEAD --` | Exit 0, including fresh pre-report check at `10:52:24Z` |
| `cargo metadata --no-deps --offline --locked --format-version 1` piped to JSON field extraction | Exit 0; 13 workspace/default-member packages; mdstream requirement `=0.3.0`; resolve graph omitted as requested by `--no-deps`, not falsely claimed verified |
| Vendor-source hygiene `rg` command shown below | No matches; rg exit 1 is the expected absence result, enclosing hygiene check exit 0 |
| `git diff --no-ext-diff --unified=0 HEAD --` the four UI files, added lines scanned for the same comment/panic constructs | No matches; enclosing hygiene check exit 0; inherited untouched comments are not new additions |
| Protected-file comparison: `git ls-files -z --` the 42-path selectors above, `git show HEAD:<path>` versus `File.binread(path)` | Exit 0; complete byte equality; manifest identity above |
| Frozen production-file hook/lifecycle comparison against HEAD | Corrected byte-oriented check exit 0; all four sections match with lengths/hashes above |
| Vendor scope comparison: `git ls-tree -r --name-only HEAD -- vendor/mdstream/src`, HEAD bytes versus `File.binread`, authorizing only the 9 retained Rust paths | Exit 0; 39 inherited, 30 identical, 9 authorized differences, 1 private added module |
| `shasum -a 256` cached `mdstream-0.3.0.crate`; `tar -xOf` its `mdstream-0.3.0/.cargo_vcs_info.json` | Exit 0; archive checksum/release commit match documented provenance |
| `shasum -a 256 vendor/mdstream/LICENSE-*` | Exit 0; both unchanged values above |
| Recovery diff recipe and `shasum -a 256` implementation paths | Exit 0 for hash computation; full diff matches inherited recovery input; file manifest records that exact retained version |
| Final repeated workspace/vendor format checks | Exit 0 for both; unchanged implementation version |
| `git diff --no-index --check -- /dev/null docs/514/vega-issue-514-boundary-round-implementation-evidence.md` | Raw exit 1 with 0 diagnostic bytes: expected new-file difference, no whitespace errors; checked wrapper exit 0 |
| Report inventory/path hygiene static check | Exit 0; all 15 file hashes and manifest identity match, no sensitive absolute paths |
| `git diff --cached --quiet` | Exit 0; no index changes; no Git mutation performed |

Exact vendor hygiene command:

```sh
rg -n '//|/\*|\*/|\.(unwrap|expect)\s*\(|\b(panic|todo|unimplemented)\s*!' vendor/mdstream/src
```

### Diagnostic correction retained, not hidden

The first ad-hoc Ruby comparison of the embedded hooks used a text regexp against Git output labelled US-ASCII. It failed with `ArgumentError: invalid byte sequence in US-ASCII` after matching the first hook; that is an observation-script encoding fault, not a product/test failure. A surrounding shell footer masked its exit status, so that initial aggregate command is **not counted as passed**. The original error was inspected, then the comparison was changed to binary strings and `/n` regexps and rerun with `set -e`/`pipefail`; all four required hook sections matched. The first final aggregate check also stopped at the new-report no-index diff's expected exit 1; inspection confirmed no diagnostic output, and the corrected wrapper accepted only exit 0/1 with no diagnostics before completing the remaining checks. No repository code, tests, assertions or acceptance were changed for either observation-command correction.

## Original cloud evidence and next gate

Inherited, not rerun or queried by this writer:

- Test-before-production commit `e7afdfbcf40263f13831766148cc0bd58de9dc67`, based on `27b88c2e9934a01aa912ed95fc26b6ec3fd3470a`; independent test review preceded the `09:18:29Z` push.
- [Actions 39754408243](https://github.com/puzige/vega/actions/runs/39754408243), Nextest `119742673340`: 31 executed, 25 passed, 6 failed, 0 skipped, 0.790s. Clippy/fmt job `119742672896` succeeded; Python 60 passed. Raw evidence is fixed in [PR #529 comment](https://github.com/puzige/vega/pull/529#issuecomment-6337487253).
- v4 actual public parse prerequisites passed, followed by target rail failures 24→0, 48→0 and inner38→14; the outer-rail control passed. v5 dedented fence/code-literal targets failed while real ordered continuation/blank controls passed. Original v3 pending tilde extra `Paragraph("~~")` remains valid RED for the frozen snapshot assertion.
- **No compilation or cloud GREEN exists for this retained implementation yet.** Earlier RED Clippy/Python success does not validate this patch.

Parent next: independently review this exact stopped implementation; commit/integrate under parent Git ownership, then run the existing PR gate on that same version. Independent implementation review and same-version cloud verification may proceed in parallel. Require the complete same frozen 31-test set GREEN before any separate Rust-test deletion. Preserve raw RED/GREEN source/commit/job evidence; subsequent deletion and final empty-set gate remain separate parent-owned lifecycle stages.

## Inventory — exact current implementation file bytes

```text
5e4f7cbff49c0e3008dd33fa2488a5969e61500c014ee03a1c18592d26a0830e  crates/vega_ui/src/conversation_stream/message_anchors.rs
5b622ad0cbddfb3fc0343ae9bfa5f2070b788c88431d50ad171c816d014ee923  crates/vega_ui/src/conversation_stream/model.rs
33dda8a820a129848b76951bd4d8eae110d0179522c2100137e6624a50d0b068  crates/vega_ui/src/conversation_stream/render_rows.rs
e6e85c9de040c23ba305f59a2ddce0790de33eed73f8e05dbb9622937f34a768  crates/vega_ui/src/conversation_stream/thinking_body.rs
93cb2712c8adf5dada511672cbf795575bbf53778fea48001f82a6fc74be4e72  vendor/mdstream/README.md
856b257143fca3bfdcf7facdf761a80e83a21ac4e90c3bb789702a03ef4e6b9e  vendor/mdstream/src/stream.rs
03d6c15b64755c9ae67448159536c80baa1e205aef18c4cf2268897a914acb02  vendor/mdstream/src/stream/block_machine.rs
016a41c3ad1fbf7f2cd16f9a77e69f8bd4b8f5761e913481bc0b6f99b83e0f3c  vendor/mdstream/src/stream/boundary_detector.rs
41171615195a43ed0b458d3d300827cf08e33726910accd58d4d6f886cd92ac1  vendor/mdstream/src/stream/compaction.rs
fbede4eed8c78413fbf17e91b15dc5a4a8f5f1cac8430a5a04cb4efdfc730b97  vendor/mdstream/src/stream/engine.rs
c3ade51e479bc44240525e8a73c1f55a6423f81f74eacd62e2c5dff4d00bb012  vendor/mdstream/src/stream/fence_context.rs
fc60aa55d0d94d2237350d54a1293068f58009bc2b4ad18b21b60158ffa9659f  vendor/mdstream/src/stream/machine.rs
9166b91484a9043fd9e71a565fa5c4d0213bf9c5f84fec65aa75869cfc43dc0c  vendor/mdstream/src/pending/pipeline.rs
b5e4932dace140d1a7b8a75d547e101d3f2a64a291f5c43b27650fd719ad436a  vendor/mdstream/src/pending/repair.rs
d3bcaf1483657977c09756db94215c5eab5ac5cc29751097f00ded202d9a7ba6  vendor/mdstream/src/pending/repair/context.rs
```

## Residuals, timing and handoff

- Spec deviations: **none**. No unresolved acceptance decision or concrete source integration blocker found in this bounded pass. Static API/borrow traces do not substitute for cloud compilation, Clippy, behavior tests, native layout or user system integration.
- NOT RUN: Vega execution, cargo check/Clippy/test/nextest, Rust/Python tests, harnesses, compilation/build/package, network fetch/rebase/reset, CI queries, Git mutation. No new dependency, source trim, UI copy compensation, skipped/softened assertion or post-implementation test.
- Inherited: all 15 implementation paths and their completed logic, vendor transformation/provenance notice and original RED evidence. New in this replacement: bounded call-site/type/visibility/lifecycle trace, fresh permitted checks, byte-freeze/manifests and this evidence report only. No source edit was justified; preserving the completed implementation avoids restarting/reworking it.
- Measured replacement static-work interval: **2026-10-22T10:46:20Z–2026-10-22T10:55:43Z, 9m23s elapsed active-session time**, with no CI/external wait; timing/report finalization follows that verification snapshot. The start is the first timestamp command after the mandatory checkpoint read, not a claim about earlier unread wall time or sampled CPU time. The interrupted predecessor's approximately 09:31–10:43 execution interval is inherited context, not this writer's active time and not claimed as precisely measured active implementation time.
- Git/index/commit/push/CI and parent-owned dirty documents remain with the parent. This report is the only file written by the replacement. **READY: implementation and report writes are stopped.** No further writes without a concrete follow-up; parent may proceed with independent review and same-version cloud verification.
