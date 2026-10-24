# #514 v6 parser-prerequisite implementation — READY evidence

## Freeze and authorization

- Stage: **PARSER-ONLY implementation READY for independent review and same-version cloud validation.** Not reviewed/GREEN, and not authorization for UI repair.
- HEAD: `c780eb5ead8711f364bec54728d94d4244897669`; branch `feat/514-continuous-quotes`.
- Contract: [v6 spec](vega-issue-514-continuous-quote-boundaries.md), SHA256 `fb32cc443c3e9e5179a35e8974f38d8cb977ecd1b75e53247090816b0d1c33f0`, read before changes.
- Parent owns Git/index/commit/push/CI and previous documents. Parent supplied the completed fetch/sync; this writer did not fetch, rebase, reset or change Git state.
- Implementation diff SHA256: `b90d326d61fd410ce9d10e0edc1fd99d464c82372c325655f4efa33c2e579cf6`.
- Recipe: `git diff --no-ext-diff --binary HEAD -- vendor/mdstream | shasum -a 256`. Exactly the five changed paths below; parent-owned documents and this new report are excluded.
- Ordered five-file SHA256 manifest identity: `82863636d58696d7f61e8525f9b88c56244f79b42f0e618422b2cd1cafd79304` (hash of `shasum -a 256` output in the inventory order).
- Source snapshot verified at `2026-10-22T12:21:59Z` / `2026-10-22T20:21:59+0800`. First measured timestamp `2026-10-22T12:14:37Z`: 7m22s active-session elapsed through that snapshot, no external wait. Report finalization follows; final handoff UTC/elapsed is recorded in the response.
- Environment: Darwin arm64; locked toolchain 1.98.0; rustfmt 1.9.0; Cargo 1.98.0; Git 2.55.0. No compiler or Vega process was started.

## Valid RED and stage order

The following cloud evidence was supplied by the parent after reading the raw logs; it was not queried or rerun by this writer:

- Frozen test commit `c780eb5ead8711f364bec54728d94d4244897669`; previous stopped test diff `990101edac06cf79d337ca78e437ee2c9a4ec083638eb05a94245c32b534178c` committed unchanged after independent test review PASS at `11:58:23Z`.
- [Actions 39774947305](https://github.com/puzige/vega/actions/runs/39774947305), Nextest job `119811411225`: 38 discovered / 38 executed, 32 passed, 6 failed, 0 skipped, 1.412s. Clippy job `119811410237` succeeded; Python 60 passed.
- Original 31 regressions passed. The balanced prose/fence control passed, and the dedented independent-quote control passed before the first continuation target.
- Both prose/fence targets failed at their pending-tree assertion (`stream_boundary_tests.rs:478`, stage 0). Actual output lost the code structure and exposed synthetic closer/fence prose; expected one List item with temporarily repaired strikethrough prose and `CodeBlock(language=text, code="literal\n")`.
- The quote-first parser target failed at `stream_boundary_tests.rs:585`, depth-zero/whole: the first List→Quote paragraph was prematurely committed instead of retaining a pending List→Quote with two paragraphs.
- All three new UI tests failed at their public parse prerequisite (`quote_behavior_tests.rs:1549`). **These are not marker/geometry RED.** No UI change is included here.
- Raw evidence fixed at [PR #529 comment](https://github.com/puzige/vega/pull/529#issuecomment-6340450279).

Next gate: parent integrates this exact stopped version, independently reviews it and runs the existing PR gate in parallel on the same content. The parser targets and controls must execute successfully; unchanged UI tests must pass their real-tree prerequisites before any subsequent failure can establish UI's own geometry RED. Only a later precise follow-up after that UI RED can authorize UI repair. No new tests, assertion changes or test deletion are permitted to manufacture GREEN.

## Minimal implementation and static trace

### List-owned quote continuation

Root chain: `MdStream::process_line` / `process_incomplete_tail_boundary` → `is_new_block_boundary` → `BoundaryDetector::current_line_can_interrupt`. The generic quote-start condition previously saw `  >` after `- > first` as a new top-level block even though the maintained container path was List→Quote.

- `FenceContext::continues_list_quote` reads `self.containers` with a local `Cursor`. It uses existing `consume_owner`, so preceding lists must satisfy their actual marker-derived content indentation before an existing quote owner is accepted. No new parser, reparse, container allocation or mutable state is introduced by this query.
- `machine.rs` computes the new flag only in `BlockMode::List`. Its sole `BoundaryDetector::new` call supplies the updated argument.
- `BoundaryDetector` stores `owned_quote_continuation`; only its generic quote-interrupt condition uses the flag. Fence protection, heading/thematic/custom/footnote interrupts, after-blank decisions and other block modes are unchanged.
- Depth 0 statically consumes List(indent 2) then Quote. Depth 1 consumes both List(indent 2) owners then Quote. A zero-indent independent `>` cannot consume the outer list owner. In the frozen dedented control, the real blank line and existing `after_blank_line_decision` still select a new block.
- Complete lines still update `FenceContext` through existing `observe_line`; a completed line without the quote prefix drops that quote from the maintained path. The incomplete-tail boundary query is immutable, so partial prefixes are not consumed twice. Fence active/range handling is unchanged.
- Existing block transitions/reset clear context; compaction rebases protected byte ranges without changing container columns. No lifecycle mutation was added, and existing SingleBlock-footnote handling is untouched.

This is a bounded static trace, not an executed tree or geometry result. It does not claim a general Markdown parser redesign or new support beyond the accepted boundary.

### Prose-to-fence repair seam

Root chain: pending display → `PendingRepair::apply_with_fences` → `apply_prose`. The entire pre-fence prose span previously included its trailing newline separators; `balance_strikethrough` appended `~~` after those separators, joining that synthetic closer to the next protected fence line.

- Only prose spans immediately before a protected fence range change. Their trailing ASCII space/tab/CR/LF suffix is separated using reverse `char_indices`, retaining a UTF-8 boundary after the last content character.
- Existing `apply_prose` runs on the content prefix. The original separator slice is then copied verbatim before the existing protected fence slice. For the frozen source, this yields the repaired prose closer on the original paragraph line, followed by the original two newlines and indented fence.
- No separator or code byte is discarded. All-whitespace prefixes remain separators, and the reverse scan does not split Unicode code points. There is no global trim, disabled repair or source mutation.
- `apply_prose` itself, its `tail_window`, inline/style options, the final unprotected tail path, raw finalization, protected code ranges and pending transformers are unchanged. Closed prose remains a control; real code blank lines remain protected.
- No whole-buffer parser or extra standalone scanner is added to the streaming path. Existing maintained context/ranges and bounded terminator behavior are reused.

### Vendor notice

`vendor/mdstream/README.md` now documents list-owned quote continuation and separator-preserving pending repair. The inherited mdstream version remains 0.3.0; the prior 39 upstream Rust files plus Vega's private fence-context module, source provenance, licenses and package/dependency contract remain in place. No manifest, dependency, upstream test collection or license file changed.

## Exact implementation inventory

| Path | SHA256 |
| --- | --- |
| `vendor/mdstream/README.md` | `ef766049c3c7304737cd154c7035e6d7261e52c402982d598ba90a1be996928e` |
| `vendor/mdstream/src/pending/repair.rs` | `c66b81c6941e4c3d37137c6a92bf9d59a43e46b1ef5e4ae2040efe275c4cb54a` |
| `vendor/mdstream/src/stream/boundary_detector.rs` | `e37e27b9681ead455a844a3f22606ae650f0e79e3508f96bcd1bddf897362e2c` |
| `vendor/mdstream/src/stream/fence_context.rs` | `3818bef053d1f99ae16c8e79dca0bc1f007f22bdffcad44385d7b478a8ca6e1f` |
| `vendor/mdstream/src/stream/machine.rs` | `bd5fdc36a2dc03d6e8abf956f872e3801df5d2831e69ea98137452ada5d1b4e7` |

Diff: 5 files, 48 insertions / 4 deletions. Four Rust production files plus vendor README; this new report is the only other writer-owned artifact. No extra production file was needed.

## Frozen tests, UI and protected bytes

All 38 definitions and full test/support bytes match HEAD; counts are static, not execution results:

| Frozen path | Definitions | SHA256 |
| --- | ---: | --- |
| `crates/vega_markdown/src/stream_boundary_tests.rs` | 12 | `58af94904c5a86dde60992627dc4d0749adbacc13e70f6bf9fe5fcc15a83a837` |
| `crates/vega_ui/src/conversation_stream/quote_behavior_tests.rs` | 22 | `f314d6828b845abadf08e960f069f84982d46873bd89bb559f177441c320e726` |
| `crates/vega_ui/src/conversation_stream/list_marker_behavior_tests.rs` | 3 | `dc9938517dae1a9df334791939324c58ec7fead3850ac1dd1da9969395f73e72` |
| `crates/vega_ui/src/conversation_stream/thinking_body.rs` | 1 | `e6e85c9de040c23ba305f59a2ddce0790de33eed73f8e05dbb9622937f34a768` |

- Fresh raw-byte comparison against HEAD covered 509 protected tracked paths, excluding only the five authorized implementation paths. Selectors: `crates`, `vendor`, `scripts`, `xtask`, `.github`, `.config`, root Cargo/lock/toolchain, root AGENTS, delivery skill, recovery, previous implementation report and v6 test report.
- Sorted protected SHA256 manifest: `f757a28b11fb38500e9c1335cd1c687fa97fbd7b51cb355c17b8dcc22d096f37` (`sha256 + two spaces + relative path + newline`). Every path matched HEAD bytes using `git cat-file --batch` and binary file reads.
- This covers all UI production, observer/lifecycle hooks, frozen assertions, other vendor code, Cargo, lockfiles, workflows, Nextest configuration and Python scripts. No test was restored, edited, added, ignored or weakened.
- Parent spec remained at the contract hash above. Parent continuation evidence remained SHA256 `92f5d97b02099236c6bd32e4a356e398e4fd54744393db60ef4d838fe1f7aa92`; its dirty state is parent-owned and was left alone. All previous reports are unchanged.

## Local static verification only

| Exact command or procedure | Result |
| --- | --- |
| `git rev-parse HEAD`; `git branch --show-current`; `git status --short` | Exit 0; stated HEAD/branch and scoped inventory |
| `cargo fmt --all -- --check` | Exit 0, fresh repeated check on final source |
| `rustfmt --edition 2024 --check` on all four edited Rust files | Exit 0 |
| `rustfmt --edition 2024 --check $(git ls-files 'vendor/mdstream/src/*.rs' 'vendor/mdstream/src/**/*.rs')` | Exit 0; explicitly includes excluded vendor and its private module |
| `git diff --check HEAD --` | Exit 0, fresh repeated check on final source |
| `rg -n '//\|/\*\|\*/\|\.(unwrap\|expect)\s*\(\|\b(panic\|todo\|unimplemented)\s*!' vendor/mdstream/src` using the alternation expression shown below | No matches; rg exit 1 expected, checked wrapper exit 0 |
| Ruby/Open3 `git ls-files -z` + `git cat-file --batch` versus `File.binread` on 509 protected paths | Exit 0; all raw bytes equal HEAD |
| `rg -c '#\[(gpui_kit::)?test\]'` on the four frozen test-bearing files | Exit 0; 12 + 22 + 3 + 1 = 38 definitions |
| `shasum -a 256` on implementation diff/file inventory and frozen files | Exit 0; exact hashes above |
| `git diff --cached --quiet` | Exit 0; index untouched |

Exact hygiene scan (no regex escaping for a Markdown table):

```sh
rg -n '//|/\*|\*/|\.(unwrap|expect)\s*\(|\b(panic|todo|unimplemented)\s*!' vendor/mdstream/src
```

No command/check failure occurred in this implementation pass. Report whitespace/path/manifest consistency and final version identity are verified after this report write, with results in the handoff response.

## Limits and stopped handoff

- **NOT RUN:** Vega, cargo check, Clippy, Rust/Python tests, Nextest, harnesses, compilation/build/package or CI query/action. Prior RED Clippy/Python success does not validate this implementation.
- No new dependency/comment/panic construct, 0.4 migration, UI compensation, global trim, parser rewrite, post-implementation test or acceptance change. Spec deviations: **none**. No unresolved scope/API blocker found in this bounded pass; compiler and behavior validation remain pending.
- New work is only the parser ownership guard, prose/fence seam, vendor notice and this report. The original UI implementation and all 38 frozen tests were inherited unchanged.
- **READY means writes stopped, not GREEN.** At handoff this writer stops every authorized-scope write. No further writes without concrete follow-up. Parent may commit and run independent review and same-version cloud verification in parallel; UI remains strictly frozen until its own valid RED.
