# Issue #117 delivery

Contract: [compaction stream specification](vega-issue-117-compaction-stream.md).

## Implementation

Compaction is a semantic conversation list item. The first accepted lifecycle
event closes the active Markdown segment and appends one row; subsequent accepted
events update that operation in place. Existing list splice/remeasure handling
preserves detached scrolling. The fixed Composer status band and six-transition
eviction are removed. Status rows contain no summary content and do not enter
the provider transcript. Existing generation/model/thread fences remain.

Reopen uses the existing latest durable status projection. Since legacy records
have no transcript anchor, the restored row appears after loaded history with an
explicit “已恢复 · ” prefix. This does not reconstruct all prior operations
or claim an exact original position. An active/new operation rejects late
restoration; the controller also retains its owner/load-sequence fence. No
schema, provider, budget, compaction algorithm or dependency change.

## Verification

Local evidence is retained outside the worktree in the private `issue-117`
evidence directory. Intermediate compilation failures are preserved along with
the original red test; they are not counted as passing runs.

| Requirement | Evidence | Result |
|---|---|---|
| C1 regression | `cargo test -p vega_ui issue117_context_status_is_a_conversation_item` on old production code | RED, exit 101: old fixed status band still exists |
| C1–C6 stream | `cargo test -p vega_ui conversation_stream::context_control::tests` | PASS, 7 tests, exit 0 (`green-context-ui-5.log`) |
| C2/C4/C5 controller | `cargo test -p vega context_compaction` | PASS, 15 tests, exit 0 (`controller-tests.log`) |
| Formatting | `cargo fmt --all -- --check` | PASS, empty output |
| Strict lint | `cargo clippy -p vega_ui -p vega --all-targets -- -D warnings` | PASS, exit 0 (`clippy.log`) |
| Broader stream regression | `cargo test -p vega_ui conversation_stream` | PASS, 260 tests, exit 0 (`stream-regression.log`); includes the 7 context tests above |
| Candidate package | `cargo xtask package`; strict deep code signature verification | PASS, exit 0 (`package.log`) |
| Native recovery and scrolling | Open existing conversation in installed candidate; scroll up and return to tail | PASS: one historical status row, row scrolls away with transcript, no fixed band |
| Live native compaction, C7 full appearance matrix | Owner manual acceptance | NOT RUN by agent; do not infer from recovery screenshot |
| Cloud PR check, integration, cleanup | Pending | NOT RUN |

Tests assert actual before-text / compaction / after-text / tool order, one row
per generation, terminal non-regression, more than six operations without
eviction, detached follow state, historical restore/idempotency and late/foreign
restoration rejection. The reopened production controller test checks the
restored row and absence of the fixed band. These automated results do not
substitute for native pixel or real-provider verification.

## Residuals

The owner elected to preserve the existing conversation and perform the live
compaction acceptance manually. No new prompt was sent to that conversation.
Live native acceptance, full appearance matrix, cloud check, integration and task cleanup remain outstanding.
Draft PR: <https://github.com/puzige/vega/pull/134>.
The Issue stays open until required acceptance and delivery are complete.
Rollback is a revert of this card's implementation commit; data formats are unchanged.

## Installed candidate and native evidence

Production source commit: `255a063`; subsequent changes only update this report.
Candidate and installed executable SHA-256 both equal
`9ae68d7dc780e4f4120c7404985723c01a102743a7462de23636ec51994a9321`.
The canonical Documents application was updated after checking that no message
was streaming and no tool was running. The previous application is backed up
as a zip with its executable hash outside the worktree.

Private evidence: `candidate-restored.png`, `candidate-scrolled.png`,
`baseline.png`, command logs and `manifest.json`. Screenshots were inspected:
the restored row aligns to the conversation column; scrolling moves it out of
the viewport and returning to the tail restores it. Existing conversation
content was neither edited nor submitted to a provider. These screenshots are
kept local because they include existing conversation content.

## Follow-up: compaction icon parity (C8)

Owner review of the installed build found the row still disagreed with the
reference: the old renderer swapped glyph per state (`Refresh` while running,
`Check` on success, `Warning` on failure, `Close` otherwise), and the reference
shows one outline glyph in both the running and completed states.

The reference glyph is Lucide's public `text-select` icon (ISC), which the
reference product registers as `text-select-light-16` and uses as its
`context-compaction` marker. It is a published library symbol, not a private
asset, so the path data was taken from the public Lucide source and committed
through Vega's existing inline-SVG convention; `gpui-kit-assets 0.6.0` does not
ship it. No third-party bundle was copied.

Implementation: new `Icon::TextSelect` in `crates/vega_ui/src/icons.rs`
(inline `TEXT_SELECT_SVG`, 24px viewBox, stroke-2, round cap/join, consistent
with the file's other missing-symbol icons), plus one shared
`context_compaction_visual` helper in
`crates/vega_ui/src/conversation_stream/render_rows.rs` that returns the same
glyph for every state and recolors only failure. `Status` no longer selects a
shape; the existing label still carries the transition, and the row geometry,
`Typography::METADATA` size and `text_secondary` neutral color are unchanged.

Verification:

| Check | Command | Result |
|---|---|---|
| Old behavior is detectable | `cargo test -p vega_ui --lib issue117_compaction_row_uses_one_glyph_for_every_state` with the previous per-state mapping restored | RED, exit 101: "Compacting must share the single text-select glyph" |
| C8 regression | same test on the fix | PASS, 1 test, exit 0 |
| All #117 stream tests | `cargo test -p vega_ui --lib issue117` | PASS, 4 tests, exit 0 |
| Broader stream regression | `cargo test -p vega_ui --lib conversation_stream` | PASS, 270 tests, exit 0 |
| Controller regression | `cargo test -p vega context_compaction` | PASS, 15 tests, exit 0 |
| Formatting | `cargo fmt --all -- --check` | PASS, empty output |
| Strict lint | `cargo clippy --workspace --all-targets -- -D warnings` | PASS, exit 0 |
| Asset parity | normalized element comparison of the committed `TEXT_SELECT_SVG` against Lucide's published `text-select` SVG | identical: 15/15 elements, same viewBox/stroke attributes |

Native pixel acceptance of C8 on a real compaction, plus the C7 Light/Dark and
narrow-window matrix, remain owner-run; this report does not claim them.

## Follow-up: compaction row copy (C9)

Owner review of the installed build after the icon fix found the wording still
disagreed with the reference. The rendered row was `上次上下文压缩完成`: the
`上次` prefix comes from `restored`, which is set only when a conversation is
reopened and its latest durable status is projected back into a fresh stream —
so a compaction that had just run in the same conversation reappeared as a
"last time" leftover. The reference product's own zh-CN bundle
(`zh-CN-523d79e20e0e.js`) uses `正在压缩上下文` / `上下文已压缩`
(`contextManuallyCompacting` / `contextManuallyCompacted`).

Owner ruling: change only (1) the restored prefix and (2) the live verb forms.
The reference's manual/automatic split (`上下文已自动压缩` /
`正在自动压缩上下文`, and the Work-mode `已优化对话` / `正在优化对话`) was
**not** adopted: `ContextCompactionStatusRecord` carries no manual/automatic
source, so that would be new plumbing and is left to a separate card.

Spec was updated first (`docs/vega-issue-117-compaction-stream.md` §2, §5, new
§7, new C9 row and change log). Implementation:

- `context_control::status_label_for(status, failure)` is the single copy
  source; the previous record-taking `status_label` wrapper was removed once it
  had no non-test caller (it would otherwise be dead code under `-D warnings`).
- `render_rows::context_compaction_label(status, failure, restored)` is the
  render-side entry point: `已恢复 · ` for a recovered row, otherwise the bare
  label. The retired `上次` prefix and `上下文压缩完成` no longer appear in any
  row.

Verification:

| Check | Command | Result |
|---|---|---|
| Old copy is detectable | `cargo test -p vega_ui --lib issue117_compaction_copy_matches_reference_and_marks_restored_rows` with the `上次` prefix restored | RED, exit 101: left `"上次上下文已压缩"` ≠ right `"已恢复 · 上下文已压缩"` |
| C9 regression | same test on the fix | PASS, exit 0 |
| All #117 stream tests | `cargo test -p vega_ui --lib issue117` | PASS, 5 tests, exit 0 |
| Broader stream regression | `cargo test -p vega_ui --lib conversation_stream` | PASS, 272 tests, exit 0 |
| Controller regression | `cargo test -p vega context_compaction` | PASS, 15 tests, exit 0 |
| Formatting | `cargo fmt --all -- --check` | PASS, empty output |
| Strict lint | `cargo clippy --workspace --all-targets -- -D warnings` | PASS, exit 0 |

The C9 assertion also fails if `上下文压缩完成` or `上次` returns in any
rendered row (live or restored, all four states), so the retired wording cannot
silently come back.

## Follow-up: comment red line

Master gained `ed49e74` (#162, "forbid code comments repo-wide") before this
card merged, so the card's own diff must contain no new comments. The C9 card
had added 20 comment lines (3 in `context_control.rs`, 5 in `render_rows.rs`,
12 in the test file); a follow-up commit removes exactly those and touches no
pre-existing comment. Behaviour, strings and tests are unchanged, and the
verification table above was re-run on the cleaned tree.

Native pixel acceptance of C9, plus the C7 Light/Dark and narrow-window matrix,
remain owner-run; this report does not claim them.

## Follow-up: row vertical alignment (C10)

The user hand-tested the merged copy and reported that the row's left icon and
right label did not look vertically centered. Measured cause: the row used
`items_start`, so the 16px icon slot and the 19.5px metadata label shared a top
edge while their optical centers differed by 1.75px.

Spec was updated first (new §8, change log, new C10 row). Implementation:
`render_rows.rs` switches the compaction row container from `items_start` to
`items_center`; the icon now sits in an explicit `context-compaction-icon-slot`
wrapper so the geometry is selectable by tests. The reference product's
equivalent `{icon, summary}` divider rows and Vega's existing icon+label rows
(thinking toggle, Composer utility chip) all use center alignment, so the
compaction row no longer diverges.

Measured geometry (TestAppContext, 12px metadata):

| Alignment | icon center | label center | offset |
|---|---|---|---|
| `items_start` (before) | 12.00 | 13.75 | 1.75px |
| `items_center` (after) | 13.50 | 13.75 | 0.25px |

Verification:

| Check | Command | Result |
|---|---|---|
| Old alignment is detectable | `cargo test -p vega_ui --lib issue117_compaction_row_icon_and_label_are_vertically_centered` with `items_start` restored | RED, exit 101: `icon center 12 and label center 13.75 must share one optical center` |
| C10 regression | same test on the fix | PASS, exit 0 |
| All #117 stream tests | `cargo test -p vega_ui --lib issue117` | PASS, 6 tests, exit 0 |
| Broader stream regression | `cargo test -p vega_ui --lib conversation_stream` | PASS, 280 tests, exit 0 |
| Formatting | `cargo fmt --all -- --check` | PASS, empty output |
| Strict lint | `cargo clippy -p vega_ui --all-targets -- -D warnings` | PASS, exit 0 |

Native pixel acceptance of C10 remains owner-run; this report does not claim it.
