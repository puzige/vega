# Fork titles, sidebar entry and pending footer validation

## Versions and preserved development evidence

- Task base: `d213dc4a618369615120da74f8e939fc9cfb96f1`.
- Initial development head: `95bb1e60a5ba00e83833962fe141fcbabc981493`; Actions [41471189271](https://github.com/puzige/vega/actions/runs/41471189271) completed successfully: 7 Rust tests executed/passed, 0 skipped; Clippy and required check succeeded.
- Corrected development head: `358843081db427c83e4da30f9aca46b9f05c528e`; Actions [41472741046](https://github.com/puzige/vega/actions/runs/41472741046) completed successfully: 9 Rust tests executed/passed, 0 skipped; Python suites 5/15/40 passed; Clippy and required check succeeded. Counts are from actual Nextest PASS records and Summary, not workflow source echoed in logs.
- Accessible immutable evidence tag: `evidence/851-fork-titles-35884308`, pointing at the corrected development head.
- `crates/vega_conversation/src/fork_title_tests.rs`: blob `2fc96f3b31d24f703bdca377c92ea06ac33ff7da`, SHA-256 `f35cae46c88fd2f06cb089529ebe7f5729dcdd5d26f81344338ed80f4caa3ef2`.
- `crates/vega_ui/src/sidebar/threads_block/fork_menu_tests.rs`: blob `a4db897bcec0d246548beb2113478843da6f0afb`, SHA-256 `e068673d04116e1f0dd05f59da906f93008ae3ad1367efc4cb722335cd42231e`.

## Reviewed and executed contracts

The seven conversation tests use migrated in-memory stores and the real conversation Fork transaction: first/repeated/nested ASCII title sequences and unchanged source; archived collisions/project isolation; meaningful numeric parentheses; literal punctuation/canonical suffix handling; latest trusted Final excluding trailing user/interrupted messages; missing Final/running-source rejection without destination; overflow rejection without source mutation.

Independent development review found one P2: sidebar lookup errors were misreported as absence of completed replies. The corrected production boundary distinguishes missing storage/query errors from `Ok(None)`; two additional regressions execute it with missing, unmigrated and migrated stores plus a valid Final. Independent incremental review of the corrected head reported no findings. No assertions were weakened and no failed regression was dropped.

## Final delta and outstanding checks

After preserving evidence, remove both development Rust test files and their two module declarations. No test-specific dependencies, features or manifests were introduced.

The user additionally requested that the pending message's entire footer remain visible after pointer exit. This small rendering-only delta derives `footer_visible` from message focus or that message's exact pending Fork identity and applies it to Copy, Fork and timestamp opacity. Spinner geometry, motion fallback, guards and completion/failure clearing remain unchanged. This change is not covered by the nine previously executed backend/menu regressions; exact-delta static independent review and final-head required CI are pending, and actual pointer/keyboard/animation rendering requires user manual acceptance.

Local checks: format and diff/static inspection only; no local Vega execution, compilation, build, Clippy or tests. Final Rust empty-set discovery/execution and final CI success must be verified separately before merge. Do not represent `no Rust tests` (0 discovered / 0 executed) as functional regression success.

## Manual acceptance after release

- New Fork title uses English half-width ` (2)`, then ` (3)`; repeat/nested Fork does not stack generated suffixes. Source and existing fork titles are unchanged.
- Sidebar project/pinned/recent/archived conversation menu exposes Branch in new chat for supported Native project conversations, including a noncurrent conversation. Correct latest completed reply is the cutoff. Keyboard navigation and adjacent organization actions remain correct.
- Busy/no-final menus explain their disabled condition, missing storage does not falsely claim no reply, duplicate submissions are blocked, and stale completion does not steal navigation or overwrite drafts.
- During message-footer Fork, move pointer away: Copy, same-size spinner and timestamp all remain visible on that message only. On completion/failure normal hover/focus visibility returns. Verify reduced-motion/static fallback, light/dark, keyboard focus and failure recovery on the actual app.

Issue #851 remains open/In review after merge until explicit user acceptance. No remote application/database changes or restarts were performed for this delivery.
