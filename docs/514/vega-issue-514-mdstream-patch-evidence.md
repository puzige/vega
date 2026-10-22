# #514 mdstream 0.3 implementation READY evidence

## Stage and frozen identity

- Scope: v3 production dependency fix after valid cloud RED; no UI or Vega parser implementation changes, no frozen-test edits.
- HEAD: `ea64ee4dc3cb682d67c84bf2963cf83c79acd59d`; branch `feat/514-continuous-quotes`.
- Parent fetched at 07:46:33Z and confirmed master `e4f18c656aa69d192d095a4338a2108e6c9a3c2c` before this delegated implementation. This writer performs no Git/index mutations, commits, push or CI operations.
- Implementation patch SHA256: `8cc3af5dd3ef76d2a27b94f542d658697554b482dec49741b8b6afc60e9e0249`.
- Implementation file-manifest SHA256: `c4b8ab9a4469078119ac76997bcd2ebb00355c8c932cb7254b11b91b571075d9`.
- Both hashes exclude this evidence document and all parent-owned documents. The final handoff separately hashes this document and the entire writer artifact.
- Patch hash recipe: concatenate `git diff --no-ext-diff --binary HEAD -- Cargo.lock Cargo.toml` followed by `git diff --no-ext-diff --binary --no-index /dev/null <file>` for every regular file under `vendor/mdstream`, sorted by relative path. Hash the raw concatenation with SHA256. Each no-index diff exits 1 because the file is new.
- File-manifest recipe: the exact 45-line inventory below, each line `<sha256><two spaces><relative path><LF>`, in displayed order; hash those UTF-8 bytes with SHA256.

## RED provenance and unchanged tests

The parent supplied and recorded the original cloud evidence at
[PR #529 comment](https://github.com/puzige/vega/pull/529#issuecomment-6335569822).
[Actions 39741381663](https://github.com/puzige/vega/actions/runs/39741381663)
on the exact HEAD above started and executed 24 tests in 15 binaries:
21 passed, 3 failed, 0 skipped, execution summary 1.128s. Nextest job
`119699148836` failed only the two public parser boundary assertions and
the unchanged original UI copy assertion. Clippy/fmt job `119699149068`
succeeded; Python executed 5+15+40=60 passing tests. These are inherited
cloud RED results, not results of executing this implementation.

The three product failures were the extra `CodeBlock(None, "\nafter\n")`
after the list, an extra empty CodeBlock at the intermediate closing fence,
and an extra copy newline. Real heading weight observation now passed,
as did the original 16 tests and blank-code, top-level-fence and late-reference
controls. All 24 tests and observers remain byte-for-byte equal to HEAD.
No test was removed, ignored, skipped, retried or weakened. This patch
has not been compiled or tested locally or in the cloud by this writer.

## Minimal semantic change

Only `vendor/mdstream/src/stream/boundary_detector.rs` changes production
logic relative to the exact 0.3.0 archive after the hygiene steps below:

```diff
-        if fence_start(curr).is_some() {
+        if fence_start(curr).is_some()
+            && !((matches!(self.current_mode, BlockMode::List)
+                || matches!(self.block_start_mode, BlockMode::List))
+                && is_list_continuation(curr))
+        {
             return true;
         }
```

`start_mode_for_line` recognizes the opening `- ```text` as List, and
`update_mode_with_line` keeps List/BlockQuote under boundary detection.
The new guard uses the same current/start list context and existing
continuation predicate already used by `after_blank_line_decision`.
The indented closing fence therefore stays in the list instead of opening
a second code block. An unindented fence is not a list continuation and
still interrupts; no blanket list-fence suppression is used. Existing
blank-line decisions, non-list modes, source text, adapters, pending
completion, committed allocation/version logic and reference invalidation
are unchanged. No trim, full-document reparse, 0.4 migration or #485 code
selection/autoscroll change is included.

## Source, licenses and hygiene

- Exact archive: `mdstream-0.3.0.crate`, SHA256 `f3327364edd79c4f66b36db667c0a28e078389351b0855e5f95ed4f22d778821`, matching the original registry lockfile entry.
- The archive's `.cargo_vcs_info.json` fixes upstream commit `950d0926f54667f04aed87fad6cc9921aa74ef5c`, subdirectory `mdstream`.
- All 39 cached Rust source files were byte-compared with archive members before copying; all matched. Vendoring reads archive members directly, not a mutable upstream checkout; registry/cache data is never edited.
- Of the archive's 107 files, omit all 56 files under `tests/` (including fixtures), the one benchmark, six examples, standalone lockfile, registry/original manifests, VCS metadata and upstream README. Replace the README with the provenance/modification notice and write a minimal production manifest.
- Before copying, omit only two test-only tail modules: `src/stream.rs` from `#[cfg(test)] mod html_state_tests` to EOF (16 lines / 621 bytes), and `src/adapters/pulldown.rs` from `#[cfg(all(test, feature = "sync"))] mod tests` to EOF (36 lines / 1147 bytes). Each contains one upstream test; the second includes its test-exclusive helper. These are not frozen Vega tests.
- A bounded static Python lexical pass walks source from left to right, recognizing whitespace, line comments, nested block comments, ordinary/byte strings and escaped quotes, raw-string delimiters, character/byte-character literals, identifiers, lifetimes and punctuation. It copies every non-comment span verbatim; each comment character becomes a space except preserved CR/LF. It is not regex deletion over arbitrary source. Prefix reconstruction and non-comment token equality were asserted before writing each new file.
- This archive contains 291 lexical comments, no block comments or raw-string literals; two comments trail fields in `stream/input.rs`. No multiline literal occurs. No literal is stripped, normalized or trimmed.
- Rust 1.98.0 rustfmt then applies formatting and explicit Unix line endings. A second archive-derived comparison independently repeats the omissions/comment transformation and formats via stdin with the same settings. All 39 final files match exactly after applying only the displayed guard to the one expected file.
- The token comparison finds no other production token differences at all, including trailing commas. All 367 retained production string/character/byte literals have identical spellings and order. Thirty-eight files are token-equivalent to the hygiene-only source; the remaining file differs only by the guard.
- No Rust comments, executable doctests, test/support code, unwrap/expect, unsafe code, or new dependency exists in the vendored copy. `doctest = false` is explicit. No upstream development dependencies or tests are imported.

The release declares `MIT OR Apache-2.0` and author Mingzhen Zhuang. The
archive omits licenses, so the writer retrieved both original files from
the fixed upstream commit with read-only `gh api` contents requests,
decoded their base64 payloads directly, and verified response SHA, byte
length, Git blob hash and SHA256 before writing. No license text is
synthesized. Exact upstream copyright remains in the MIT license.

| File | Bytes | Git blob | SHA256 |
| --- | ---: | --- | --- |
| `LICENSE-MIT` | 1072 | `f7c57f575b1b78ac893b339ffafd9effe4bab286` | `4670b75aa0fca969a99cb3ea2bd5175055ac05eefaf8edffefa8c1cc586ede9a` |
| `LICENSE-APACHE` | 9157 | `cd9f8ad8d319325bf9237d5195735eebbfcdcfdc` | `5bedb04d972252dd9fd5bbe03eb9998c66fab819a3951467b612ffe32fed5e11` |

## Manifest and lock invariants

- Root manifest changes only workspace exclusion of `vendor/mdstream` and the same-name `[patch.crates-io]` path. Existing `vendor/block` is byte-unchanged.
- The production manifest retains name/version `mdstream 0.3.0`, edition 2024, Rust 1.85 minimum, original author/license/repository, `default = []`, `pulldown = ["dep:pulldown-cmark"]`, `sync = []`, and release dependency requirement `pulldown-cmark 0.13.4` optional. No dev dependency is present.
- Parsed lock comparison: exactly remove `source` and `checksum` from the mdstream entry; everything else is identical. All 924 packages, every version, every dependency edge and every other package's source/checksum remain unchanged. No transitive dependency is added, upgraded or removed.
- Offline locked no-deps metadata succeeds with 13 workspace members and no vendor member. No-deps metadata does not independently prove the fully resolved active patch graph; cloud compilation/logs must confirm local mdstream selection. No full dependency resolution or build was run locally.

## Local static validation

All final checks below exit 0:

1. Archive SHA256 and all 39 archive/cache source comparisons; fixed-commit license response/blob/SHA256 checks.
2. `rustup run 1.98.0 rustfmt --edition 2024 --check --config skip_children=true,newline_style=Unix` over all 39 vendored Rust files.
3. `rustup run 1.98.0 cargo fmt --all -- --check`.
4. `rustup run 1.98.0 cargo metadata --no-deps --offline --locked --format-version 1`.
5. `git diff --check`, plus new-file LF/final-newline/trailing-whitespace checks.
6. Archive-derived final-source/token/literal comparison described above; one guard delta only.
7. Parsed manifests/lock comparison, 43-file vendor inventory, no symlinks, and source policy scans (no tests/comments/unwrap/expect/unsafe).
8. Byte comparison of frozen/protected tracked code, workflows, existing vendor and writer evidence against HEAD. Initial extended comparison included 438 paths and passed; parent-owned task documents are excluded from final code-freeze comparison because the parent is updating them.

No Vega execution, compilation, cargo check, Clippy, Rust/Python tests,
Nextest, build/package, Harness search/execution, Git/index mutations,
commit/push, CI dispatch or CI query was performed. The only network
reads were the two fixed upstream license contents.

### Diagnostic corrections, not product or acceptance changes

- One read-only shell batch used the zsh special variable `path` as a loop variable, temporarily replacing PATH inside that subprocess; its final date command exited 127. A later batch used `instruction_file` and read the actual UTC successfully. No repository mutation was involved.
- The first metadata command used `cargo +1.98.0`, but this machine's direct cargo binary interpreted the selector as a subcommand and exited 101. The responsible command layer was corrected to `rustup run 1.98.0 cargo`; offline locked no-deps metadata then exited 0.
- The first archive reconstruction comparison failed at `src/adapters/mod.rs` because file rustfmt retained the archive's CRLF while stdin rustfmt emitted LF. Inspected raw bytes isolated the observation mismatch; both formatter modes were made explicit `newline_style=Unix`, the vendored source was formatted again, and the full archive/token/literal comparison passed. No logic or assertion changed.
- A broad protected-path diff exited 1 because it included the parent's continuation evidence update. Code-only comparison exited 0; parent-owned documentation was read and left untouched.

## Scope update and remaining boundary

The authorized initial spec was v3 SHA256
`cf733965f647607b95fec9f95177e20334a01de1894b6e59ac39a90fca50e159`.
During final static checks the parent revised its spec to v4, SHA256
`b4db40ec42db9ad0508d1defe0e9f56a52ba9da60b4da7a73d4755e29ec8d4fe`
at the 08:06 read. It records a separate quote-inside-list rail indentation
boundary and explicitly keeps v3 tests frozen. This writer reported the
observed update and does not modify that spec, add v4 tests, or implement
v4/UI changes. The mdstream patch remains limited to the independently
reviewed v3 acceptance; no v3 spec deviation. Overall issue delivery remains
blocked on the parent's separate v4 cycle as well as normal review/GREEN.

Parent next: independently review this stopped artifact, integrate using
parent-owned Git/index, run existing cloud checks with all unchanged 24
tests, and verify both active path resolution and GREEN. A failed/new
boundary requires diagnosis before any test change. No cloud GREEN,
independent implementation approval, merge or native user validation is
claimed here. Existing tests remain until the complete required suite is
GREEN under the repository lifecycle.

Writer activity began `2026-10-22T07:47:50Z`; the final handoff records the
actual ending UTC and elapsed active duration. No external-result wait
was inserted; license reads, static investigation, writing and diagnostic
rework are included in the active interval. At final READY this writer
stops all scope writes before parent review, with no subsequent fixes
without a concrete followup.

## Complete implementation inventory and SHA256

43 new vendored files plus the two existing Cargo files. This document is
the only additional writer-owned new file; its hash is reported separately
in the final handoff to avoid a self-referential hash.

```text
d7c701e424bae4802ca725b937899e3875d0d0293b7d3be5558ff707849ab888  Cargo.lock
b2745ea61e0b9434b97562a71653c99a88a4f32436be778b911ee4edf914156f  Cargo.toml
3e5aa7e9443f8789bcd946cfb687f87d5f0ffe5bca1807391598c78d8b430cf4  vendor/mdstream/Cargo.toml
5bedb04d972252dd9fd5bbe03eb9998c66fab819a3951467b612ffe32fed5e11  vendor/mdstream/LICENSE-APACHE
4670b75aa0fca969a99cb3ea2bd5175055ac05eefaf8edffefa8c1cc586ede9a  vendor/mdstream/LICENSE-MIT
ad4df6edd9b9692a0b1a81c38e6e319fd2b34aafe725623c771643d96dffb755  vendor/mdstream/README.md
01178f99c23c18968b0e42930bcfc3ea2293ed0b9e0984315997feb0cd677ea9  vendor/mdstream/src/adapters/mod.rs
dada39f8da314f42a67f0f591d9ca4eb568d6013d1c9387ffc6c58e569ff99a2  vendor/mdstream/src/adapters/pulldown.rs
71e3c0d5df2e1249e4e806fe41e691d2e29fbc8b99ef599cf35587f2e9cf4beb  vendor/mdstream/src/analyze.rs
dbc59476640ea0b15751ef8197d56d9ee5b7caa9a015b855c4228b4efa560f85  vendor/mdstream/src/boundary.rs
0e94ca0849151be9aac5fe095d459a2c419ae404442e86ca91ced8c5fa988f4c  vendor/mdstream/src/extensions/boundary_registry.rs
77c7b2a679ad276a3e92295256f91b213e144aad5c0414544b89148b2842e9b6  vendor/mdstream/src/extensions/mod.rs
434e2b0893a22d31d46a398d8fba696b73608001b4a459004394d02e73f08b32  vendor/mdstream/src/extensions/pending_transformers.rs
19c10f3bf985ce6fe20964230ec692860474508ed28609c7af756ce3234ce3e2  vendor/mdstream/src/lib.rs
d02199b73b5a871996133db8df7e6d9f2bc0b4e77b4b8ec0ea4706cca4072e6a  vendor/mdstream/src/options.rs
48ffc17f38f3909fbf1577728fc9c15ff78fdfeb4f3d8ca9a693258020dfb6b1  vendor/mdstream/src/pending/mod.rs
9893be6a10f24c7e044f8bf2894bdc7a5782b0e6cf0de928be7bfc62aca9b002  vendor/mdstream/src/pending/pipeline.rs
10cacbda06204b8684282847ca9bfaa1736f764c6f2e84657acd15ce71776cf8  vendor/mdstream/src/pending/repair.rs
bba80c1d3188d5c7e86d8fb0e38a5340d2c0fe04154f3571c63709f542e4f406  vendor/mdstream/src/pending/repair/context.rs
b1cba4acf8c0e829376d54f4184caae0589ecaa6cd2b6a277c46b4941914a593  vendor/mdstream/src/pending/repair/emphasis.rs
0fd2b02f3fe4df64bb32a1f68be063a24cadd26916160b98c41f5698cf8b773f  vendor/mdstream/src/pending/repair/inline.rs
0c431153dc705d5d570224becd379973ddc5b29399bb2b1aa16f30a41311bf6b  vendor/mdstream/src/pending/repair/links.rs
c705035bddd503985eb31d2873249145ebb08af08c2de4c431e25f89f9c0f641  vendor/mdstream/src/pending/repair/setext.rs
d03bd3dd8b3c744be0b2026c6525bf7b4bd68f963942280c7d6c988c33035c36  vendor/mdstream/src/pending/terminator.rs
42678ddf5386a4b5d317b80613022a7e6a3c8293f5824961e343d9416aa5d82b  vendor/mdstream/src/reference.rs
8d5944a777537c27fb6edbcd218f760e31c36959569d697e651e92cdee441689  vendor/mdstream/src/semantics/footnotes.rs
8887a84c5541f9bf2137ebcb01b5874cf6423bc88cae8782290267062e0b860a  vendor/mdstream/src/semantics/mod.rs
84c574a00a3f2e7d540538d587ed2fe979eeccd2da5b4880d07bb748a84d9535  vendor/mdstream/src/semantics/references.rs
e2076635b2c32f573d2716efdd0cf5979e4f9081f8efc47e5aa93472bb66bcfe  vendor/mdstream/src/state.rs
8b3065dacff32593fc718ca7cebadb7a912bf4178875ceb0996efeafebd518ec  vendor/mdstream/src/stream.rs
0cf0537205e37e34bc8d240d600abde3bd6ca37b0f9699effd3f668cb073aabe  vendor/mdstream/src/stream/block_machine.rs
d4099e8812dde7574e751701ea4cd3ea39c9b5f691ca5d40cf9be93b2de40c29  vendor/mdstream/src/stream/boundary_detector.rs
9d1104d10b7926c7189d24adf5d47b3ef7dc58fd7f63cc1766425e00cd765dfb  vendor/mdstream/src/stream/builder.rs
74f70289bf3f822b733d6377c9c52b4e2567738a19e67e83a2acd88c05c6e67e  vendor/mdstream/src/stream/compaction.rs
50e70fae1f614be28769f0369532402e1c655223625f4338cf59c86e99f0a78e  vendor/mdstream/src/stream/engine.rs
a6a6bb5ab7f67ce9c465fdcf41d22eab324fbbf67af81c9878640244773de820  vendor/mdstream/src/stream/footnotes.rs
dd2f478e0f29aa0ef784988b6625afbb34d49480c42606d7ec01d44e7bf4f1b9  vendor/mdstream/src/stream/html.rs
a90f818b31814970ff4bd1f1ff13427b9017ac39227d31848cbf499e299bc941  vendor/mdstream/src/stream/input.rs
b7244684dfbe52e9ed2ca239a5c151d0ddb04225006b135359687bded1323c51  vendor/mdstream/src/stream/machine.rs
d4911dfcb0fe4a9627ea517b5f8938fceb76ed78e0e4b8c81d61a6423dcb6b39  vendor/mdstream/src/stream/mode.rs
c8f264f8d62118d2d8d563c85a74a0c3042876a64eb293480a11f93946679bb1  vendor/mdstream/src/syntax.rs
088523437f35d05c619e191aac4f6182979995174d519723e72a3d4e1d910503  vendor/mdstream/src/syntax/containers.rs
5118e686f948220991f3b282bd0dfd0cb9e99fe38506c4b3cd7e02e52b84712d  vendor/mdstream/src/syntax/facts.rs
9cd93f83ea244797e333b1ee982f5a3ca60ee237c3fbee9d2b2bb5ee1e2c957b  vendor/mdstream/src/transform.rs
49de6e324cdf832132da12be7fb46e37c5470a2e42d794ec6816ca53b0c4e493  vendor/mdstream/src/types.rs
```
