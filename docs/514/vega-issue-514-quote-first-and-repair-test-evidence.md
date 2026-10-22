# #514 v6 quote-first and prose/fence tests — stopped review artifact

## Freeze and stage

- Stage: **TEST-ONLY READY for independent review, including the accepted public-parser prerequisite increment.** The parent resolved the earlier plan gap before freeze by updating v6; the product defect itself remains unfixed. This is not approval to repair production, and no v6 RED or UI geometry RED has been obtained.
- Branch: `feat/514-continuous-quotes`.
- HEAD: `87f02938fd2c723f2bff5d34c55e16c9ced2325e`, the parent-owned merge of master `6e50833202bb45f05c08f7bbc30a68be475609ff`.
- Current contract: [v6 spec](vega-issue-514-continuous-quote-boundaries.md), SHA256 `fb32cc443c3e9e5179a35e8974f38d8cb977ecd1b75e53247090816b0d1c33f0`, read before the one-test increment. The original six-test artifact used the earlier v6 SHA256 `3199c04ffac1b446b9edccead5eef75103488ffea0b251a4c16790bd6a6264c7`; its assertions remain unchanged.
- Current source test diff SHA256: `990101edac06cf79d337ca78e437ee2c9a4ec083638eb05a94245c32b534178c`. Previous stopped six-test diff: `4a127678f1701e221c48d40700c1e55a678bd15c200f99f4d8a32cdecc53257e`, independently confirmed by the parent at `11:39:53Z` and rechecked before this increment.
- Recipe: `git diff --no-ext-diff --binary HEAD -- crates/vega_ui/src/conversation_stream/quote_behavior_tests.rs crates/vega_markdown/src/stream_boundary_tests.rs | shasum -a 256`. Parent-owned docs and this report are excluded from that identity.
- Current ordered two-file SHA256 manifest identity: `61ceaf1555a615dfbdbf7c865afbc7b18be10583b5a03808e75383588af39d0a` (the `shasum -a 256` output for the two paths in the recipe's order, hashed again). Previous manifest: `8992e089c7b2dfd759982082d7f23ea5b750c81655cdada1e35ea72337ec7714`.
- Increment source snapshot: `2026-10-22T11:45:56Z` / `2026-10-22T19:45:56+0800`. First measured timestamp for this follow-up: `2026-10-22T11:42:17Z`, 3m39s active-session elapsed through that snapshot; report finalization follows and handoff UTC/elapsed is stated in the response. No CI/external wait. The earlier six-test writing interval was `11:25:13Z–11:38:37Z`, 13m24s; the handoff gap is not counted as active work.

## Accepted parser prerequisite — earlier plan gap resolved, not product GREEN

The specified quote-first source is valid Markdown, and the new tests retain its explicit desired public tree without injecting nodes. However, static tracing of the current streaming implementation contradicts the assumption that its multi-paragraph List→Quote prerequisite already passes:

1. For `- > first\n  >\n  > second`, `start_mode_for_line` selects `BlockMode::List` from the first line. `machine.rs::update_mode_with_line` leaves List/BlockQuote modes unchanged (`vendor/mdstream/src/stream/machine.rs:303`).
2. On the completed `  >` line, `FenceContext::protects_line` is false: no fence is active and its new-opener branch requires `fence_start` (`fence_context.rs:25–34`). Ordinary quote continuation is not protected.
3. `boundary_detector.rs:130–134` treats `is_blockquote_start(curr) && !is_blockquote_start(prev) && current_mode != BlockQuote` as an interruption. `is_blockquote_start` trims leading whitespace, so the current `  >` is true and previous `- > first` is false.
4. `machine.rs:151–158` then commits the first line and starts a separate BlockQuote block. The nested fixture has the analogous transition after `  - > first`.

**Consequence:** all three new UI tests can fail their public pending-tree assertion before reaching their real marker/rail/text assertions. This is static product-boundary evidence, not an executed failure, test-harness failure, or valid v6 geometry RED. The single-line quote-first marker defect and the independent prose/fence repair defect remain distinct.

**Parent resolution before freeze:** the parent independently confirmed this call chain and updated v6 to include a focused public-parser continuation target and dedented independent-quote control. The current increment implements only that accepted test addition. The plan gap is resolved; the parser defect and geometry defect are not claimed fixed or executed. No synthetic tree, bypass blank line in a continuation target, softened prerequisite, production edit or private/public API change was made. Parser failures and UI geometry failures remain separate evidence classes.

## New tests and observable expectations

### Public parser prerequisite increment

Exactly one definition is appended after all six retained v6 tests: `quote_first_list_continuations_preserve_owned_quote_paragraphs_across_chunks` (`stream_boundary_tests.rs:544`). It uses the existing public `MarkdownStream::append/snapshot/finish` APIs and whole/Unicode-character/syntax-boundary schedules, with explicit trees, not an algorithm mirroring the boundary detector:

| Case | Exact input | Expected pre-finish ownership |
| --- | --- | --- |
| Positive control, checked first | `- > first\n\n> outside\n` | Committed List→Quote(Paragraph("first")); separate pending Quote(Paragraph("outside")) |
| Depth 0 target | `- > first\n  >\n  > second\n` | No committed split; one pending List→Quote with Paragraph("first"), Paragraph("second") |
| Depth 1 target | `- parent\n  - > first\n    >\n    > second\n` | No committed split; one pending List item containing Paragraph("parent") and List→Quote with the two distinct paragraphs |

Each case checks explicit committed and pending node trees, the combined visible tree, final raw-derived structure and repeated-finish identity through the existing helper. The dedented control runs before the targets, so its result is observed before the expected target failure in this test. Static source review confirms its CommonMark distinction: the real blank line ends the inner quote/list continuation, and the following zero-indent `>` is outside the list marker's two-column content requirement. `after_blank_line_decision` returns StartNewBlock for that control; `parse_blocks` and `parse_list` retain the explicit separate quote/list structure. This is a static feasibility check, not a local execution result. No new harness, public API or test hook was introduced.

### Quote-first list items

Appended `quote_first_item_tests` contains three path-specific GPUI tests so each production renderer can report its own result:

- `ordinary_quote_first_items_keep_markers_outside_the_quote`.
- `selectable_quote_first_items_keep_marker_order_and_content_columns`.
- `thinking_quote_first_items_keep_marker_order_and_content_columns`.

Each covers depth 0 and depth 1, first checking actual `MarkdownStream::append/snapshot/finish` trees: List→Quote and List→List→Quote, with two distinct quote paragraphs. The existing `open` harness receives the real source; no `NODES` override is assigned. Real recorded text layouts and masked 2px painted rails are then checked at narrow→wide→narrow widths 240→760→240:

| Depth | Marker x | Rail x | Both paragraph x |
| --- | --- | --- | --- |
| 0 | 0 | 24 | 38 |
| 1 | 24 | 48 | 62 |

Coordinates are relative to outside-body text. Arithmetic uses existing tokens: list indent 24, rail width 2 plus code padding 12 = quote gutter 14. Assertions preserve 16px paragraph gaps, rail coverage across the gap, no rail outside its paragraphs, text containment, actual reflow and identical rail geometry after returning to the narrow width. Outside-before/after establish the column and selection endpoints; existing 31 tests retain paragraph-led list/quote controls.

### Copy expectation chosen before any v6 implementation

The existing `render_line_selectable` ListItem branch appends the two-space-per-depth literal prefix and `• ` before content; Thinking's `layout_list_marker` does the same in the projected copy document while excluding marker fragments only from source anchors. Both separator paths use one newline for consecutive ListItem rows and two between ListItem and continuation Paragraph. Therefore the explicit new logical copy expectations are:

- Depth 0: `outside-before\n\n• {FIRST}\n\n{SECOND}\n\noutside-after`.
- Depth 1: `outside-before\n\n• parent\n  • {FIRST}\n\n{SECOND}\n\noutside-after`.

`FIRST` and `SECOND` are the exact Unicode paragraph constants in the test module. Selectable and Thinking use actual drag selection and `MessageCopy::copy_selected_text`; expectations do not come from post-fix output. Copy is checked before geometry in those paths. It remains downstream of the public parse prerequisite; no passing copy result or UI geometry RED is claimed.

### Prose/fence repair

Appended `prose_fence_repair_tests` contains:

- Target: `pending_prose_repair_keeps_backtick_fences_on_their_original_line`.
- Target: `pending_prose_repair_keeps_tilde_fences_on_their_original_line`.
- Positive control: `closed_strikethrough_before_list_fences_preserves_code_and_real_blank_lines` (both fence characters).

The two targets begin with exact source `- before ~~unfinished\n\n  {fence}text\n  literal\n`. Each checks an open-fence snapshot, appends a genuine blank code line and Unicode code line (`literal\n\n尾🙂\n`), then closes the fence while the List remains pending. At each stage, the direct incremental snapshot and fresh whole/Unicode-character/syntax-boundary schedules must equal an explicit List with one item containing:

1. Paragraph with `Text("before ")` and `Strikethrough([Text("unfinished")])`.
2. `CodeBlock { language: Some("text"), code: exact_expected_code }`.

No extra paragraph, heading, inline-fence prose or synthetic delimiter is allowed by whole-tree equality. Finishing each stage must discard only the synthetic repair: the target's prose becomes literal `before ~~unfinished`, with code and real blank lines unchanged. Existing `finish_with_expected` also checks repeated-finish identity. The closed-prose control uses raw `before ~~unfinished~~`; its styled prose must persist in pending and final trees.

Only existing public `MarkdownStream` APIs are called. They expose nodes, not a raw source getter; exact source literals/deltas are retained and finish verifies raw-derived final prose/code, but no unavailable raw-byte observation or new API is claimed. UI source strings are also checked unchanged during resize. Production source-rewrite prohibitions are additionally protected by the static production-byte freeze.

## Preservation and inventory

Only these existing source files were edited, by append only:

| Path | Original byte prefix | Original prefix SHA256 | Appended bytes | Current SHA256 |
| --- | ---: | --- | ---: | --- |
| `crates/vega_ui/src/conversation_stream/quote_behavior_tests.rs` | 46593 | `3b924f56957d96c3d849cc1406cae3ffcf8d40754e15fcf9ff64850afb3fe3a7` | 8480 | `f314d6828b845abadf08e960f069f84982d46873bd89bb559f177441c320e726` |
| `crates/vega_markdown/src/stream_boundary_tests.rs` | 16454 | `41349d1a384a222053b8705d29b0dd6ae2a7cbd32d3c3bdc48b3f6011eb75826` | 5487 | `58af94904c5a86dde60992627dc4d0749adbacc13e70f6bf9fe5fcc15a83a837` |

- Current source diff: 220 UI lines + 158 parser lines, 378 insertions, zero deletions. The original 19 UI + 8 parser definitions and their complete source prefixes are identical. The other 3 marker tests and 1 Thinking lifecycle test are untouched: original 31 definitions plus 7 v6 definitions = **38 source definitions**, not a discovery/execution/pass count.
- This follow-up adds only 63 lines / 2242 bytes / one definition. The parser's first 19699 bytes still hash to `d4fbb0a179bc86108fcdb01fc2cc1d36e689f0b8f1618a09203556902609e297`; the entire UI file still hashes to `f314d6828b845abadf08e960f069f84982d46873bd89bb559f177441c320e726`. Thus all previous 37 definitions, including the six new v6 tests, remain byte-for-byte intact. Appended raw-byte SHA256: `4f6ae88eb45cf27c0b22841c4bb4abe3299adb0213b311b9dbdd1f727c9b6c3e`.
- 509 tracked protected paths were freshly compared byte-for-byte with HEAD using `git cat-file --batch` and `File.binread`: all paths in `crates`, `vendor`, `scripts`, `xtask`, `.github`, `.config`, root Cargo/lock/toolchain, recovery and previous implementation report, excluding only the two append targets. Manifest SHA256 `99f8416ded3ae866f212bf9f46e4cb7a5e8c634ddf3d3ae7cfb5343d2bcc5f08` (sorted `sha256 + two spaces + relative path + newline` entries).
- This includes all existing hooks, test-only support, production/vendor business logic, dependency manifests, workflows and Python scripts. No restored tests, new dependency, new comment, ignore, should-panic or lint-allow was added.
- Affected UI production and all `vendor/mdstream` bytes also match previous implementation commit `f07edec9b00ff17654da55eb9ae3ba74a5c2c5ee` at the merged HEAD.
- Parent-owned dirty documents were not edited: spec SHA256 above; continuation evidence SHA256 `59d3172992b8a5806fda522554f602ab7219970339aa940e3b364f0e11d2cadb`.
- This new evidence document is the sole additional file written. No module wiring, Cargo, hook or public API changes were necessary.

## Static verification commands/results

| Command/procedure | Result |
| --- | --- |
| `git rev-parse HEAD`; `git branch --show-current`; `git status --short` | Exit 0; matches the stated worktree branch/HEAD and authorized inventory |
| `rustfmt --edition 2024 crates/vega_markdown/src/stream_boundary_tests.rs` | Exit 0; formats only the appended test; both original and previous v6 prefixes remain identical |
| `cargo fmt --all -- --check` | Fresh exit 0 for this increment |
| `rustfmt --edition 2024 --check` on the two test files | Fresh exit 0 |
| `git diff --check HEAD --` | Fresh exit 0 |
| Ruby/Open3 `git show HEAD:<path>` binary prefix comparison plus fixed previous-v6 prefix SHA256 checks | Fresh exit 0; original 31 and previous 6 definitions/assertions unchanged |
| Ruby/Open3 `git cat-file --batch` versus `File.binread` for 509 protected paths | Exit 0; all equal |
| Increment scan for comments, ignore, should-panic and lint-allow; source-definition count | Exit 0; no prohibited additions; exactly 1 added definition, 7 v6 / 38 total |
| Previous writing pass: `git diff --exit-code f07edec9b00ff17654da55eb9ae3ba74a5c2c5ee HEAD -- vendor/mdstream` and four affected UI production files | Prior exit 0, not rerun unnecessarily; HEAD is unchanged and current protected-byte comparison above covers the affected scope |
| `git diff --cached --quiet` | Exit 0; index untouched |
| `shasum -a 256` test diff, two test files and parent docs | Exit 0; identities above |

No check failure or assertion was hidden, retried or softened. Final report whitespace/path/inventory checks are recorded in the handoff response after the report write.

## Cloud evidence, remaining gates and stopped writes

Inherited parent-supplied evidence only: `f07edec9b00ff17654da55eb9ae3ba74a5c2c5ee`, Actions `39767237956`, Nextest `119785628625`: 31 discovered/executed/passed, 0 failed/skipped, 0.886s; Clippy `119785629659` and Python 60 passed. Original source/evidence fixed at PR comment `6339385329`. Those results do not validate the new merged HEAD or new tests; no CI query was made here.

**NOT RUN:** compilation, cargo check, Clippy, Rust/Python tests, Nextest, Vega execution, harnesses, builds or packaging. No Git/index mutation, commit, push, fetch/rebase/reset or CI action/query was performed. Local work was source authoring plus quick format/diff/static inspection only.

Parent next, following the accepted revised v6 order:

1. Independently review this exact stopped test increment, then commit/push under parent ownership and use the existing PR gate on all original 31 plus 7 v6 definitions. Obtain this parser prerequisite's own valid RED and the prose/fence targets' own valid RED; a UI test failing its parse prerequisite is not geometry RED.
2. Only after those target RED results, authorize a precise parser-only follow-up. Keep the same UI tests and all assertions unchanged. Rerun the complete set after parser repair; public trees must pass before the UI tests can establish actual marker/geometry RED.
3. Only UI's own valid RED authorizes UI repair. Same frozen complete set GREEN, independent implementation review, evidence fixation, independent test deletion and final empty-set gate remain separate later stages.

No existing assertion was changed. The parent-owned acceptance/plan clarification was read before adding the one parser test; **spec deviations: none, prior plan blocker resolved, product verification still pending**. There is no new scope or API question in this bounded increment. No production repair is authorized by this report alone. All source and report writes are stopped after handoff; further writes require a concrete follow-up. Git/integration/CI and all previous documents remain parent-owned.
