# Vendored `mdstream 0.3.0`

Production-only modified copy of Mingzhen Zhuang's streaming Markdown
middleware. Vega's root `[patch.crates-io]` selects this same-name,
same-version package. It is excluded from workspace membership and retains
the existing committed/pending, block identity and reference invalidation
model; it is not the incompatible mdstream 0.4 model.

## Source and integrity

- Published package: <https://crates.io/crates/mdstream/0.3.0>.
- Source archive: <https://static.crates.io/crates/mdstream/mdstream-0.3.0.crate>.
- Archive SHA256: `f3327364edd79c4f66b36db667c0a28e078389351b0855e5f95ed4f22d778821`, matching the original registry lockfile entry.
- Upstream repository: <https://github.com/Latias94/mdstream>.
- Release VCS commit: `950d0926f54667f04aed87fad6cc9921aa74ef5c`, subdirectory `mdstream`, from the archive's `.cargo_vcs_info.json`.
- The original 39 Rust files under `src/` originate from that exact archive, verified byte-for-byte against the cached release before transformation. Vega additionally supplies the private `src/stream/fence_context.rs` module described below.

## Modification notice

All Rust source files are modified distributions: upstream documentation
and ordinary comments are removed to follow Vega's code policy, followed by
Rust 1.98.0 formatting with Unix line endings. Two test-only tail modules
are omitted before copying:
`src/stream.rs::html_state_tests` and
`src/adapters/pulldown.rs::tests` (the latter gated by `all(test, feature = "sync")`).
Production semantic changes are confined to fence ownership, list-owned
quote continuation and pending repair, with lifecycle wiring in
`src/stream.rs`, `src/stream/{block_machine,
boundary_detector,machine,engine,compaction}.rs`, and
`src/pending/{pipeline,repair,repair/context}.rs`.

The private `src/stream/fence_context.rs` module tracks actual list-marker
content columns, ordered quote/list prefixes, and the active fence character
and length. Owned fence lines are checked before ordinary block interrupts;
an owned close ends only that fence, while a dedented fence can begin a
top-level code block. This replaces Vega's earlier fixed-two-space guard.
The existing complete-line cursor advances the context, incomplete tails
preview without consuming it, and block transitions/reset/compaction keep
its lifecycle and byte offsets aligned.

The same maintained container path identifies an existing quote inside a
list when a continuation line satisfies the owning list's actual content
indent. In List mode this suppresses only the generic new-quote interrupt;
it does not protect prose as code or attach a dedented independent quote
to the list. Complete lines still update ownership, and incomplete-tail
boundary checks only read it.

Pending display shares protected fence ranges with that context, copying
code bytes before trailing-space, setext or inline repair while continuing
to repair eligible prose. Closed ranges remain protected in the pending
container. At a prose-to-fence seam, trailing whitespace and line endings
are saved and copied verbatim after prose repair, placing any synthetic
inline closer before the separators rather than on the fence's line.
The standalone terminator uses the same private scanner; the
streaming path does not scan the whole buffer per append. The existing
top-level code-fence incremental fast path and pending transformers remain.
No source text, blank code lines, adapters, cache identity or reference
invalidation contracts are changed. Finalization clears block-local context
without changing the existing block-ID transition.

## Packaging and source hygiene

The minimal manifest preserves version `0.3.0`, edition 2024, Rust 1.85
minimum, `default = []`, the optional `pulldown-cmark` dependency at the
release's `0.13.4` requirement, and `pulldown`/`sync` features. The production
library target has doctests disabled. It omits upstream development
dependencies (`criterion`, `proptest`), benchmark/example/integration-test
targets and nonessential registry/docs metadata. The workspace lockfile
continues to resolve the existing pulldown-cmark version and dependency graph.

No upstream `tests/`, fixture, test support, `benches/`, `examples/`,
standalone upstream lockfile, generated registry manifest, or executable
Rust documentation example is included. The upstream README is replaced
by this provenance and modification notice.

Comment removal uses a lexical pass that distinguishes quoted/escaped
strings, byte and character literals, lifetimes, raw-string delimiters and
nested block comments. Each comment is replaced by whitespace with its
line endings retained, not by a regular expression over source text.
Production token sequences and literal spellings are compared before and
after comment removal, then formatting is checked separately against the
same transformed archive. This release has 291 comments, no lexical raw
strings or block comments, and two trailing inline comments outside literals.
No string content is trimmed or rewritten.

## Licensing and attribution

The release manifest declares `MIT OR Apache-2.0` and names
**Mingzhen Zhuang** as author. The archive omits standalone license files;
this copy includes the exact license bytes retrieved from the fixed
upstream release commit, not synthesized license text:

| License | Fixed upstream Git blob | SHA256 |
| --- | --- | --- |
| `LICENSE-MIT` | `f7c57f575b1b78ac893b339ffafd9effe4bab286` | `4670b75aa0fca969a99cb3ea2bd5175055ac05eefaf8edffefa8c1cc586ede9a` |
| `LICENSE-APACHE` | `cd9f8ad8d319325bf9237d5195735eebbfcdcfdc` | `5bedb04d972252dd9fd5bbe03eb9998c66fab819a3951467b612ffe32fed5e11` |

The MIT notice retains `Copyright (c) 2025 Mingzhen Zhuang`. Both licenses
are reproduced unchanged; no author, date, permission or warranty terms
have been invented. See the fixed-commit sources at
[LICENSE-MIT](https://github.com/Latias94/mdstream/blob/950d0926f54667f04aed87fad6cc9921aa74ef5c/LICENSE-MIT)
and
[LICENSE-APACHE](https://github.com/Latias94/mdstream/blob/950d0926f54667f04aed87fad6cc9921aa74ef5c/LICENSE-APACHE).

## Validation boundary

Local validation is limited to quick static provenance/token comparisons,
formatting, diffs and offline locked metadata. No Vega code, compilation,
Clippy or Rust/Python tests are executed locally. Existing frozen Vega
regressions exercise this patch through the normal cloud PR gate; upstream
tests are deliberately not imported. Source equality and static checks do
not claim cloud GREEN or native UI validation.
