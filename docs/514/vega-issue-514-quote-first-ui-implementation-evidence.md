# #514 v6 quote-first UI implementation — READY evidence

## Version, scope and ownership

- Stage: **UI-only implementation READY for independent review and same-version cloud validation.** This is not an independent review, GREEN, merge or user acceptance.
- HEAD: `e96f96cb53ef61d6ac6a9453d042887b5e8d8ce0`; branch `feat/514-continuous-quotes`.
- Contract: [v6 spec](vega-issue-514-continuous-quote-boundaries.md), SHA256 `fb32cc443c3e9e5179a35e8974f38d8cb977ecd1b75e53247090816b0d1c33f0`.
- Parent fetched at `2026-10-22T12:41:37Z` and merged master `48d958743cb66295ef3de9db31449f213263d417` at `12:42:15Z`. The six upstream paths do not overlap this repair. Git/index, integration, commit/push and CI remain parent-owned; this writer used only read-only Git commands.
- Implementation diff SHA256: `10e4baf15fdc1fd5a0870c48c6b6994129e1fb099f012d27d89b1fc5d713b696`.
- Ordered three-file manifest SHA256: `693aa51d6a6ca3c60a847287f6a77e51d474ab5ed33949a6d470c6a3af7c83c3`.
- Exactly three production files changed, 113 insertions / 36 deletions. This new evidence report is the only additional writer-owned artifact. Parent-owned `docs/514/vega-issue-514-continuation-evidence.md` was already dirty and was not edited or restored.
- First measured timestamp: `2026-10-22T12:43:34Z`. Source/static-verification cutoff: `2026-10-22T13:12:27Z`, **28m53s active-session elapsed**, including context recovery and static review; no external/CI wait. Report finalization follows this cutoff; final handoff time is stated in the response.

Diff identity recipe, run from the repository root:

```sh
git diff --no-ext-diff --binary HEAD -- \
  crates/vega_ui/src/conversation_stream/model.rs \
  crates/vega_ui/src/conversation_stream/render_rows.rs \
  crates/vega_ui/src/conversation_stream/thinking_body.rs | shasum -a 256
```

The new report and parent-owned documents are excluded from the implementation identity.

## Inherited valid UI RED and completed prerequisites

These are parent-supplied cloud results, corroborated by the parent-owned continuation record; this writer did not query or rerun CI:

- Parser-only implementation `bd4312baf59967153a9fad4234bb2278a8dd87f1` received independent implementation review PASS at `12:30:57Z`.
- [Actions 39778474005](https://github.com/puzige/vega/actions/runs/39778474005), Nextest job `119823361007`: **38 executed, 35 passed, 3 failed, 0 skipped, 1.255s**. Clippy job `119823360609` succeeded; Python 60 passed.
- All parser targets and controls passed, including the quote-continuation prerequisite and prose/fence repair. The original 31 regressions passed.
- All three quote-first UI tests passed their real public pending and final parse-tree prerequisites, then failed the actual marker geometry assertion at `quote_behavior_tests.rs:1502`: depth-zero marker required x=0; Ordinary and Selectable painted x=38, Thinking x=14.
- This is the UI's own valid marker/geometry RED, fixed in [PR #529 evidence](https://github.com/puzige/vega/pull/529#issuecomment-6341001529). The earlier run `39774947305` failed UI parse-tree prerequisites and is expressly **not** geometry RED.
- The later parent merge requires the complete unchanged 38-test set to run again on the new baseline. Prior passes are not current-version GREEN.

Parser repair, test authoring and their reports are completed inherited work, not part of this UI patch. The vendor/parser implementation is frozen here.

## Bounded composition repair

### Model ownership helpers

`StreamLine::list_marker_quote_depth` identifies only a marker-bearing `LineKind::ListItem` whose quote ancestry contains an owner created at that list depth. This distinguishes List→Quote from an unrelated ancestor Quote→List. `list_marker_inset` subtracts only the parent quote owner's already-consumed list inset, using checked optional lookup.

No flattening, source spans, line kinds, committed IDs, quote identities, cache policy or parser data were changed. These are crate-private layout helpers, not a new public API.

### Ordinary and Selectable shared wrappers

- The existing independent marker route now also handles the quote-first ListItem. It renders a real ListItem marker with empty spans; the paragraph content uses a temporary render-only clone with no marker/checkbox, avoiding a second marker column. The original model line remains unchanged and still determines separators, gaps and previous-line state.
- Selectable registers the marker before the content using its existing real selectable text path. The two-space-per-depth literal copy prefix remains in that path. No separator is inserted between marker and first paragraph.
- Each block carries an optional marker plus its owning quote-ancestry depth. Recursive `quote_blocks` consumes it only at that owner, before the rail. It occupies the same owner-relative list-inset space that wrapper padding previously consumed; the indent is not charged again inside the quoted paragraph.
- Quote grouping, owner identity checks, first-child gap handling, rail width/gutter and the flex content column remain in place. The rail still belongs to the complete continuous quote group, including the original internal paragraph gap. Content remains `flex_1` / `min_w_0` inside the remaining width.
- Existing nonparagraph-first heading/code markers retain their former independent marker behavior. Their temporary marker clones clear quote metadata so the new owner-relative marker helper cannot reinterpret their inherited padding; the original content ancestry still controls wrappers. Code grouping, scrolling, header and physical-line rendering were not changed.

Static token arithmetic for the frozen quote-first fixtures:

| Depth | Marker x | Rail x | Both paragraph text x | Internal paragraph gap |
| ---: | ---: | ---: | ---: | ---: |
| 0 | 0 | 24 | 38 | 16 |
| 1 | 24 | 48 | 62 | 16 |

These are the intended layout calculations, not newly executed geometry results. List indent is 24; quote gutter is 2 + 12 = 14. Ancestor quote gutters stay inside their own wrapper; marker ownership is not hoisted outside an unrelated outer quote. No blanket negative inset was introduced.

### Thinking preparation

- The same owner-depth predicate selects the new separate marker preparation. The marker origin uses only ancestor quote gutters before its owner, while the existing marker layouter retains the absolute list-depth marker column and literal copy prefix.
- The quoted body receives list content inset plus every owned quote gutter, so first and continuation paragraphs use the same remaining width. A separately prepared quote-first marker does not add its width a second time.
- Separator registration occurs once before the marker; the content branch skips that separator and marker registration only for this case. `previous_text` still refers to the original line.
- `layout_list_marker` is unchanged: marker/checkbox text remains logical copy text while its synthetic source anchors are removed. The body still contributes its original source spans in order.
- The existing code-first marker path remains separate and still adds its width normally. Rail accumulation, y offsets, culling, content masks, independent rail painting and horizontal code ranges are unchanged.

`message_anchors.rs` needed no edit: its existing text-height estimate already subtracts list content inset and all quote gutters, matching the required text columns. Its SHA256 remains `5e4f7cbff49c0e3008dd33fa2488a5969e61500c014ee03a1c18592d26a0830e`.

No UI copy, dependencies, source rewriting, global trim, parser work, #485 drag/autoscroll behavior or unrelated redesign was added. No specification deviation was needed.

## Exact changed-file manifest

Manifest order is the order below; hash the normal `shasum -a 256` output, including relative paths and newlines.

| Path | SHA256 |
| --- | --- |
| `crates/vega_ui/src/conversation_stream/model.rs` | `4a9bf9df7bf69e5503d4e172b13d3984cc657fc31c9596bf880c848d7aabe247` |
| `crates/vega_ui/src/conversation_stream/render_rows.rs` | `eb1ac7f33231dd54fd149ef613eea45f9db71ed7f4d338c1c7adc9dd7ad395f6` |
| `crates/vega_ui/src/conversation_stream/thinking_body.rs` | `985ae28b1ac4b51a97a2ccac9d30e0bfbed5a4e06b0bf884b1f9eb5d2c663e0c` |

## Frozen tests and protected bytes

The definition counts below are static inventory, not execution counts:

| Frozen path | Definitions | SHA256 |
| --- | ---: | --- |
| `crates/vega_markdown/src/stream_boundary_tests.rs` | 12 | `58af94904c5a86dde60992627dc4d0749adbacc13e70f6bf9fe5fcc15a83a837` |
| `crates/vega_ui/src/conversation_stream/quote_behavior_tests.rs` | 22 | `f314d6828b845abadf08e960f069f84982d46873bd89bb559f177441c320e726` |
| `crates/vega_ui/src/conversation_stream/list_marker_behavior_tests.rs` | 3 | `dc9938517dae1a9df334791939324c58ec7fead3850ac1dd1da9969395f73e72` |
| Thinking test-only suffix | 1 | `0f6bd1dd0f567a19cc09b43a0b00222274c08cb278a9a33c9be07f472c734215` |

- Ruby/Open3 performed a fresh raw-byte comparison against `HEAD` using `git cat-file --batch` and binary file reads: **518 protected tracked paths matched**. Sorted manifest SHA256: `44f72b284b06c37764155aaa74f343a6fcea1403c1aa9bd9bee6115022a186ce` (`sha256`, two spaces, relative path, newline).
- Selectors were `crates`, `vendor`, `scripts`, `xtask`, `.github`, `.config`, root `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `rustfmt.toml`, `AGENTS.md`, the delivery skill and `docs/514`. Exclusions were only the three implementation files, the new report and the parent-owned continuation record.
- This covers frozen full-file tests, all vendor/parser files, Cargo inputs, workflows, Nextest configuration, scripts, existing reports, untouched anchors and unrelated upstream code. Nothing was restored from an older baseline.
- Both renderer `cfg(test)` observation blocks and their `cfg(not(test))` partners match HEAD byte-for-byte; concatenated-block SHA256 `7e9b1079eae9305d66a728fc6bcf9e9c00ad84d46b5e3a8e1eec060f4e4cdc01`.
- Thinking's model-input observer matches HEAD byte-for-byte; SHA256 `610b805f0f6ed561fd2135e3694f949f4ae17f86cd909a2c34d7364f61fc3472`.
- The entire Thinking suffix from `cfg(test)` / `quote_test_body` through the lifecycle test matches HEAD byte-for-byte; its hash is in the table. Production edits only shift its line number.
- Total frozen definitions remain **38**. No test, expectation, fixture, observation hook, skip/ignore setting or assertion was added, removed or weakened.
- Prior parser report remains SHA256 `c96b2d8df51fde04d140d61cbbe7735398851c53da5c2910590e20c6df2cfed4`. Parent continuation record was observed at SHA256 `76da092f291127c41f414cf88cd0e2f93cb52b654bf9482abb01d29ed6912314` and left alone.

## Local static verification only

Environment: Cargo 1.98.0, rustfmt 1.9.0, Git 2.55.0; workspace Rust edition 2024.

| Command or procedure | Actual result |
| --- | --- |
| `cargo fmt --all -- --check` | Exit 0 on final source; initial check reported formatting only, subsequently applied to the three authorized files |
| `rustfmt --edition 2024 --check` on the three changed Rust files | Exit 0 on final source |
| `git diff --check HEAD --` | Exit 0 |
| `git diff --cached --quiet` | Exit 0; index unchanged |
| Ruby/Open3 protected raw-byte and embedded-hook comparisons | Exit 0; 518 paths plus the stated embedded chunks equal HEAD |
| Static test attribute counts | Exit 0; 12 + 22 + 3 + 1 = 38 |
| Added production line scan for comments, `.unwrap(`, `.expect(`, panic/todo/unimplemented/assert macros | Exit 0; no matches in added production lines; inherited unrelated comments untouched |
| Source diff, ordered manifest and frozen SHA256 calculations | Exit 0; identities above |
| Scope inventory against the allowed source files, parent document and new report | Exit 0; no other changed or untracked artifact |

The earlier ambiguous `render_rows.rs` batch edit was rejected atomically (`edit_multiple_matches`); inspection confirmed no partial renderer change. It was reapplied using unique branch context. This was an edit-tool correction, not a product or test RED. The first formatting failure was likewise a formatting check result, not behavioral evidence.

No Vega/product/harness execution, compilation, build, packaging, cargo check, Clippy, Rust/Python tests or CI query was performed locally. Formatting and static byte checks do not establish type correctness or painted geometry.

## Handoff and remaining gates

Source writes stop at this exact version. After report finalization, all writes in the delegated scope stop; further changes require a concrete follow-up. Parent may commit this unchanged patch and run independent implementation review and existing Actions in parallel on that same stopped content.

Still required: current-baseline cloud execution of all 38 frozen tests, including marker/copy/wrapping/resize/rail assertions and existing heading/code, ancestry, selection and lifecycle regressions; independent implementation review; evidence fixation. Neither GREEN nor review PASS has been obtained for this UI patch. No known additional scope change is requested.

Only after the same frozen tests are GREEN and evidence is fixed may a separate authorized Rust-test deletion and final empty-set full gate proceed. Empty-set success cannot replace the pending 38-test GREEN. Final merge and user manual acceptance remain later stages.
