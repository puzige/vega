# #307 Stage B recovery and integration checkpoint

## Recovered version, 2026-10-22 17:17 UTC

The active agent directory is empty after continuation; no predecessor execution remains available to resume. The parent inspected the preserved artifacts before assigning a replacement. No existing artifact was reset, removed or regenerated.

- Committed UI-A: `212e50d5a0503fe462c2c2b5443c10c5656c4274`, independently reviewed PASS.
- Updated baseline: `48cef07bce9fdfb8187b2fae08873725b2ac9abf`, conflict-free merge of master `2ba904f7074115de5ee89cc18c376c083f803357`; incoming delta is documentation only.
- UI-A Actions run [39817506348](https://github.com/puzige/vega/actions/runs/39817506348): Nextest job `119959751923`, Clippy `119959752614`, required aggregate `119965500294` all success. Raw Nextest output: 28 tests across 15 binaries; 28 run, 28 passed, 0 skipped. These are 13 runtime + 15 UI tests. This does not validate the untracked and unwired Stage B artifacts.

Preserved untracked SHA-256:

| File under `crates/vega_ui/src/cua_onboarding/` | SHA-256 |
| --- | --- |
| `card.rs` | `75746b1d94c641d0d751bb7159b74b94f18ffae7e1407bbdb35ca30439a36600` |
| `card/view.rs` | `fbcdef5646d6c4b48ff43a5602fcf59293e01abff6afd3ad6557539ea7dd299c` |
| `session.rs` | `e860cdb86ebdc58f703f119a28e092096a9b0643f50fe83230a40132c3dd1b30` |
| `native.rs` | `122a7e9126862ce1beac7bb5c0fecd1a3430b643a5d77097cf1efdabc7d31fb4` |
| `native/macos.rs` | `e5c65124751445bb5edcec0db7bc7fd27133aef8cb19fcc7f994598b0faf03f0` |

The native files and their delivery note have a completed independent static PASS at these hashes. The card/session note still says implementation evidence pending; there is no READY result or independent review for those files. The next writer must finish the handoff rather than treat them as verified.

Frozen tests remain unchanged: UI `26971f63dbaa01dc31e68e76d6237b2ffcc1942234bdc4ade96055069e6e48f2`; runtime `48076fccdbe6546543d4246f45128adf5177b19172015f6b0d3c5ddd94328d2a`. The original UI missing-interface REDs and runtime RED/GREEN remain recorded in PR #537 and the existing test plans. No tests are added, removed or weakened in this integration stage.

## Unchanged acceptance and observation route

The main spec's latest guidance/status-only scope and the nine integration anchors in the frozen UI test plan control this work. Every preparation request remains false/false, component/AX/screen facts remain separate, actual capture remains unverified, and there is no automatic completion, dismissal, resume, old grant or old observation replay. Real TCC, dragging, focus, following, layering and Spaces require later user manual acceptance.

The recovered card already consumes CardController and contains synchronous begin_drag, exact bundle payload, bounded initial Settings acquisition, polling and cancellation hooks. The session adapter serializes/coalesces original tickets on an app-lifetime current-thread Tokio service and uses explicit PreparationProbe open/check/close/cleanup_pending APIs. These are inspected source facts, not compilation or behavioral validation.

Finish and review these invariants:

1. All renderer values and enabled actions use CardView. Mouse and keyboard share CardAction. Close/destruction/return invalidate before ownership drops. URL/Finder dispatch is not presented as verified launch success.
2. One successful preparation session is reused; errors preserve their identity and unresolved cleanup custody. Recheck is the explicit recovery action; do not automatically reopen or stop an unrelated ComputerUseControl. Cleanup status must not falsely report release or hide unresolved resource ownership.
3. Native adapter stays unchanged unless concrete evidence requires a scoped correction and renewed review. Preserve unique marker, nonactivating Floating focus:false options, points and display-ID pairing. Beginning drag freezes following for this lifetime, regardless of drop acceptance.
4. Opening from a retained entry validates both original TaskContext and thread ID against the emitting stream before consuming the event. Return selects only the original thread through existing navigation and never submits or resumes work.
5. Opening from Settings is available manually and after a persisted desktop-enable transition. A save failure must not start guidance or claim enablement. Keep browser behavior outside this change. Open failures remain visible with a retry path.
6. Host ownership prevents duplicate surviving cards; replacing or destroying an owner closes and invalidates its card. Old callbacks cannot close or update a newly opened card. Settings-origin cards need no fabricated task or return destination.
7. New geometry enters typed shared Layout tokens before use: visible body 480 by 420 logical points; transparent shadow allowance 24 per side (528 by 468 outer bounds). Existing placement policy remains unchanged. Existing Typography and ThemeColors remain the styling sources. Required explanatory/error text stays concise, actionable and truthful.

## Implementation sequence and write boundary

One replacement writer owns the recovered card/session files, their delivery note, production module wiring, Settings event/success integration, host subscription/window ownership/navigation, and narrowly related shared Layout tokens. Existing runtime/controller/lifecycle/tests/native files are read-only. Any need to alter those is reported first with concrete evidence. The parent alone owns Git/index/commits and this checkpoint.

Read applicable AGENTS, exec/design guides, the main spec and frozen test plan before edits. Finish the recovered work rather than redo it; inspect actual GPUI signatures from the pinned installed sources. No local Vega execution, builds, tests, Clippy, cargo check, dependency/workflow changes or new acceptance. Allowed validation is focused formatting, whitespace/diffs, static API/call-chain inspection and hashes. Return READY with exact changed-file hashes, commands/results, unresolved items, manual/CI gaps and explicit cessation of writes. A non-writer reviews that immutable version; existing Actions verify the unchanged 28 tests and complete gate after module wiring. Tests remain until the complete implementation is independently reviewed and GREEN, followed by a separate removal commit and latest-head empty-set full gate.
