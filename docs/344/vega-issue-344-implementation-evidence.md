# #344 production implementation evidence

The first READY record below is retained as history for the patch based on `a367f21376e00ad8030589d73d3443945269b61a`. The review-repair record at the end supersedes its snapshot-to-height-cache import description and records the current delta and validation. Earlier checks and cloud results do not validate the repaired version.

## READY identity

- Worktree: `vega-344-message-anchor-bottom`（任务隔离工作树；省略本机绝对路径）。
- Branch: `feat/344-message-anchor-bottom`.
- Starting and handoff HEAD: `a367f21376e00ad8030589d73d3443945269b61a`; supplied master parent: `9f9fa092a3ca391a65f08419c9206f32979119e6`.
- The parent had already fetched/merged before this assignment. This implementation writer performed no Git mutations, fetch, push, PR/CI operations, local execution, compilation, build, or test execution.
- Exact v2 spec SHA-256: `3d2cbd53f810042e9fc5330a670a8ee7ef9678c7c5d1fd898152be06ff10fec4`.
- Frozen test source SHA-256: `c84c479c6f8f3b6d00290f7091abebd1537dcf7142cd4ab3f86e6ebdc2651565`; unchanged, 801 lines and 11 source test declarations. These are static source counts, not a new discovery/execution result.

Production patch identity, excluding this evidence file and parent-owned documentation:

```sh
git diff --binary --full-index HEAD -- crates/vega_ui/src/conversation_stream/core.rs crates/vega_ui/src/conversation_stream/message_anchors.rs
```

- Patch SHA-256: `4b83f3c0dbd1bacaecb8443ed5ff2bdd1c317acc3feff66566e8c232c5979ad2`.
- Patch size: 11,115 bytes; two files, 118 insertions and 46 deletions.
- `core.rs` SHA-256: `c62672089fc7fe42dee9d15ffaca8e8a5034197521fa41e31200273f33aaef61`.
- `message_anchors.rs` SHA-256: `a1133f7168c2341c2b31ed9569670ef62d4c7cc0d4aa60d5aea47475d6a97bde`.

## Changed scope

- `crates/vega_ui/src/conversation_stream/message_anchors.rs`: post-list layout snapshot, actual-tail current-anchor projection, invalid-layout visibility, keyboard-qualified focus fallback and rail-window following.
- `crates/vega_ui/src/conversation_stream/core.rs`: one internal snapshot field and its default initialization only.
- This new evidence document.
- No changes to `render.rs`, `content.rs`, frozen tests, observation wrappers, Cargo manifests/lockfile, module wiring, themes, body layout, persistence, location requests, pagination/fencing, or tail-follow control.
- The parent-owned `docs/344/vega-issue-344-delivery.md` became modified during this assignment. It was neither written nor reverted by this implementation writer and is excluded from the production patch identity above. Inherited unrelated changes remain intact.

## Implementation and review obligations

### B2: same production/harness observation path

The existing `render_message_anchor_rail` signature is unchanged. Its returned element carries a zero-sized absolute canvas: as a child of the unchanged visible rail, or as the returned observer itself when the rail is hidden. No layout wrapper, new public observation API, `display:none`, observer-less hidden return, changed positioning reference, or extra input handler was introduced.

Both existing callers already mount this returned element after their list. The canvas first callback therefore samples after preceding list prepaint. It captures the `ListState` and the exact render-time `foreground_height` also supplied to the list's `.pb(...)`; the snapshot is fully formed in that callback. Deferred work only compares/commits that snapshot and calls `cx.notify()` on the real stream entity if its value changed. It does not re-read geometry or combine old viewport bounds with a later foreground value.

The snapshot includes logical top/offset, viewport, captured foreground, actual item count, tail conclusion, and at most 256 positive measured-height samples. Including those height samples makes changed visibility/estimation inputs observable even when logical top and viewport do not change. Stable snapshots do not notify, so the observer does not create an unconditional refresh loop.

A missing/unlaid/zero-width viewport or nonpositive effective reading height produces the default invalid snapshot. Rendering has no window-height fallback. Snapshot foreground and item count must still match current render inputs before the rail can be shown. The observer remains mounted while hidden, allowing a subsequent valid layout to notify and recover without test-only refreshes.

### B3: actual final entry and measured continuity

Tail validation checks the complete range from observed logical top through `ListState::item_count() - 1`, not the last message anchor and not just cached final bounds. Every entry must have measured bounds with positive width, nonnegative height, and a bottom no lower than `viewport.bottom() - captured_foreground`. Measured height zero is legal evidence here. Short-circuiting rejects the first unknown/invalid bounds or bounds beyond the reading bottom; it does not mount or measure history and ignores unknown entries before logical top.

At the validated tail, the last valid message anchor becomes current. Otherwise the existing last-anchor-at-or-before-top rule applies. Neither `is_scrolled_to_end()` nor `is_following_tail()` is used as bottom evidence or modified.

The measured-height cache still accepts only positive heights for estimation, scans at most 256 entries, and retains the existing 512-entry bounded region and estimation rules. Its samples now come from the consistent post-list snapshot. Legal zero-height rows participate in tail continuity without changing those estimation semantics. No #545 performance work is included.

### Mouse and keyboard

Focused preview fallback and non-hovered rail-window following now require both rail focus and `window.last_input_was_keyboard()`. Hover preview precedence is unchanged. Mouse-down still focuses; mouse-up still retains the keyboard ID and requests the same stable message ID. No blur-on-leave was added. Up/down, Enter, Space, and Esc handlers are unchanged, including the existing Esc blur behavior.

### Acceptance mapping (static implementation review, not executed acceptance)

| Acceptance | Implemented path |
|---|---|
| A1 | A short final row can select the last anchor using actual post-layout tail geometry despite top backfill; neutral mouse state no longer receives focus-only preview fallback. |
| A2 | Tail continuity starts at logical top, so unknown rows above it do not prevent bottom recognition; tail-follow state is irrelevant. |
| A3 | Top rule is restored whenever continuity fails or actual final content remains below the reading bottom; non-anchor trailing entries are included. |
| A4 | Existing hover/leave state clears the preview without mouse focus recreating it. |
| A5 | Focus, keyboard IDs, key handlers, and stable-ID requests are retained; keyboard input mode enables their fallback/follow behavior. |
| A6 | Invalid layout has zero usable reading height with no window fallback; hidden returns retain the observer for normal-layout recovery. Existing anchor-count/estimated-overflow visibility conditions remain. |
| A7 | Post-list snapshots observe current top, viewport, foreground and bounded measured heights; changed-only notification supplies the next rail render with a consistent projection. |

No product text, dependencies, comments, or non-test `unwrap()` / `expect()` calls were added.

## Actual local static validation

Executed in the assigned worktree; all below completed successfully:

1. `cargo fmt --all -- --check` — exit 0; read-only formatting check only.
2. `git diff --check` — exit 0, no whitespace errors.
3. `git diff --exit-code -- crates/vega_ui/src/conversation_stream/message_anchor_tests.rs crates/vega_ui/src/conversation_stream/render.rs crates/vega_ui/src/conversation_stream/mod.rs Cargo.toml Cargo.lock` — exit 0, listed protected files unchanged.
4. SHA-256 comparison for the exact spec and frozen test source — matched supplied values.
5. Added-line static scan for comments and `.unwrap(`/`.expect(` in the two production files — no matches.
6. Read-only status, production diff inspection, SHA-256 and byte-size checks — identities recorded above; no unrelated production paths changed.

No `cargo metadata` was needed. No local cargo check, Clippy, test/nextest, app launch, build, or packaging was run. No new cloud result is claimed.

## Prior evidence and unresolved validation

Prior cloud RED is supplied evidence, not a run started or re-polled by this writer:

- Test-first commit: `9bda1ecb54cec85d4e217760408f2aafd03ad3a3`.
- Actions run: <https://github.com/puzige/vega/actions/runs/39828341844>.
- Nextest job `119997501291`: 11 executed, 4 passed, 7 failed, 0 skipped, after successful compilation; recorded failures are missing behavior rather than compile failures.
- Clippy job `119997501677`: passed for that earlier version only.

This final production patch has not been compiled or behavior-tested locally, as required. Parent-owned next steps: run the unchanged 11-test payload in the existing cloud PR checks, obtain independent implementation review bound to this exact patch, and diagnose any current-version failures without weakening the frozen assertions. No known scope/acceptance blocker remains from this writer's static review.

Rust development tests and their exclusive support must be removed in the later parent-owned delivery stage, not in this implementation stage. Final test-free HEAD still requires complete required CI and independent review; user manual testing remains necessary for real integration, themes, resizing/composer changes and keyboard behavior. READY does not mean CI-passed, merged, or user-accepted.

Implementation writing is stopped at READY; any further edits require a follow-up assignment.

## Review repair READY — current delta

### Baseline and concrete finding

- Starting and handoff HEAD: `606aa8a726e4ee2799079651931f3e417e0c3dc6`, initially clean on `feat/344-message-anchor-bottom`.
- Parent supplied the already-updated baseline and owns the next remote check, Git/index/commit, PR, CI and delivery-document integration. This writer performed no Git mutations, fetch, push, CI operations, build, compilation, application execution or tests.
- Independent review found a production cache defect: `capture_visible_entry_heights` replayed prior snapshot `(index, height)` samples against current identities. `apply_history_page` prepends both entries and identities, so old indices can assign heights to newly prepended identities and persist outside subsequent samples. Workspace-width/reading-preview invalidation clears the cache and remeasures the list, but replaying old snapshot heights repopulated invalid measurements before new layout.
- Parent reports Actions run <https://github.com/puzige/vega/actions/runs/39892038751> succeeded for the unrepaired `606aa8a726e4ee2799079651931f3e417e0c3dc6`. This writer did not fetch or poll that run; raw test discovery/execution counts are not obtained here. That earlier success neither resolves the finding nor validates this delta.

### Minimal repair and invariant

Only `capture_visible_entry_heights` changed in production. It again reads the current `ListState::logical_scroll_top()` and `bounds_for_item(index)` during render, pairing each accepted live height with `entry_identities[index]` in that same render. The function is byte-for-byte identical to the preexisting `a367f21376e00ad8030589d73d3443945269b61a` function after removing its old comments; no comments were copied or added.

The importer no longer accesses `message_anchor_layout` or its indexed samples. Snapshot `measured_heights` remains only an input to snapshot equality/change detection and refresh. The unchanged post-list observer can notify another render after new layout, and that render captures current bounds rather than replaying old samples. Existing history prepend/splice identity alignment and cache-clear/remeasure invalidation remain authoritative. The 256-entry scan, positive-width/positive-height filtering, stop-on-unknown behavior, 512-entry retention region and estimation fallback retain baseline semantics.

No generation/identity API, observer, true-tail continuity, zero-height tail handling, input-mode gate, location request, pagination/fencing or tail-follow change was needed. `core.rs`, `render.rs`, `content.rs`, tests, fixtures, observation wrappers, expected results, module wiring and Cargo files remain unchanged against the repair baseline. This evidence file is this writer's only other changed path. The parent-owned `docs/344/vega-issue-344-delivery.md` became modified concurrently during final verification; its diff was inspected and left untouched, and it is excluded from this repair's patch identities. No spec or acceptance change, new dependency, product text, comment, or non-test `unwrap()`/`expect()` was introduced.

### Exact production identity against the repair baseline

```sh
git diff --binary --full-index 606aa8a726e4ee2799079651931f3e417e0c3dc6 -- crates/vega_ui/src/conversation_stream/message_anchors.rs
```

- Production repair patch SHA-256: `b1b5868d44347fe591f0b4d81323ea99a1747f2b73860b981005c7b1cfe18245`.
- Patch size: 1,493 bytes; one file, 15 insertions and 2 deletions.
- Repaired `message_anchors.rs` SHA-256: `b8a248633df501ca357b51e024cf56d41ed86a5c02d4c38199f4198edb23dd48`.
- Unchanged `core.rs` SHA-256: `c62672089fc7fe42dee9d15ffaca8e8a5034197521fa41e31200273f33aaef61`.
- Frozen tests remain SHA-256 `c84c479c6f8f3b6d00290f7091abebd1537dcf7142cd4ab3f86e6ebdc2651565`; exact v2 spec remains SHA-256 `3d2cbd53f810042e9fc5330a670a8ee7ef9678c7c5d1fd898152be06ff10fec4`.

### Current-cycle static checks actually executed

Executed in the assigned worktree; final assigned-scope checks passed. The concurrent parent-document observation is retained below rather than reported as an all-tree pass:

1. `cargo fmt --all -- --check` — exit 0; formatting check only.
2. `git diff --check` — exit 0; no whitespace errors.
3. `git diff --exit-code 606aa8a726e4ee2799079651931f3e417e0c3dc6 -- . ':(exclude)crates/vega_ui/src/conversation_stream/message_anchors.rs' ':(exclude)docs/344/vega-issue-344-implementation-evidence.md'` — initially exit 0, later exit 1 when the concurrent parent-owned delivery-document edit appeared. The returned diff showed only that document; no production/protected-file defect. After inspecting the evidence and current status, the same check with an additional `':(exclude)docs/344/vega-issue-344-delivery.md'` completed with exit 0; all remaining paths unchanged.
4. Inline `python3` source-only comparison/hash check using read-only `git show` — exit 0: importer equals the baseline with comment lines removed; no importer snapshot reference; production content outside the importer equals `606aa8a`; supplied spec/test hashes match. No Vega code was executed.
5. Added-line scan of `git diff --unified=0 606aa8a726e4ee2799079651931f3e417e0c3dc6 -- crates/vega_ui/src/conversation_stream/message_anchors.rs` for comments and `unwrap`/`expect` calls — no matches.
6. `shasum -a 256`, `git diff --binary --full-index`, `wc -c`, `git diff --numstat`, read-only status and HEAD checks — exact identities and scope recorded above.

### Remaining verification and stop boundary

No repaired-version compilation, behavior execution, cloud result or independent delta-review result is claimed. Parent next steps are independent review of this stopped delta and the same unchanged suite through existing cloud PR checks on the final repaired head. Do not weaken assertions or treat the prior green run as repaired-head evidence. No static scope/acceptance blocker is known. Rust development tests and their exclusive support remain untouched for the later parent-owned cleanup stage; final test-free HEAD still requires complete required CI, and real integration remains subject to user manual testing.

Repair implementation writing is stopped at READY. The first-patch history above is retained; this repair requires final-version revalidation.
