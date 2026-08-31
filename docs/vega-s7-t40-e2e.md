# Vega S7-T40 E2E evidence

- Card: T40 Per-task cost summary card (A10-06)
- Branch: `feat/s7-t40-task-cost-summary`
- Base commit at execution: `b96fcefa5909e604f0093f093fd202ee3cd75e06` (S7-T38 squash, = `origin/master` when this card started)
- Evidence time: 2026-08-31 (implementation continued from an interrupted 29-minute prior session whose uncommitted work was inventoried and kept, not rewritten)
- Implementation commit: `8ca43cf840c71864bdfa8e1fae91e051545d814c` (single commit; rebased once onto `origin/master` `429cb2d7860742226d7e1f38429ded97c5f1e526` after S7-T39 #43 merged — two conservative conflict resolutions kept both T39 and T40 semantics; full gate suite re-run green post-rebase: **747 passed / 0 failed / 1 ignored**)
- Implementation diff SHA-256 vs the T38 base commit `b96fcef` (excluding this evidence file, measured pre-rebase): `4b5775af55c81b7b129e31da8fd27d058c2647974c052793d9aff46ad95606d0`; post-rebase vs the same base (T40 content + conservative conflict merges): `84cc33b62025b91366a4dcdb1e6bf18e0d94e077c94baec14741c8b3906a1504`
- Raw command log: `/private/tmp/vega-s7-t40-gates.log` (ephemeral; this file is the durable summary)

Recompute the implementation hash:

```sh
git diff --binary b96fcefa5909e604f0093f093fd202ee3cd75e06 -- . ':!docs/vega-s7-t40-e2e.md' | shasum -a 256
```

## E2E-REAL: owned headless per-task summary lifecycle

Command:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
cargo test -p vega_conversation --test task_cost_summary_e2e
```

Result: **PASS, 7/7 tests** (finished in ~0.04s). Every test runs against an owned temporary data root (tempdir + temp SQLite store) with `MockProvider` scripts; no real Vega data root, API key, provider, network, or external application was touched.

Measured numbers (frozen pricing: input $1/1M, output $2/1M, cache-read $0.1/1M; C2 numerator charges uncached input at the input rate):

| Scenario | Persisted facts asserted |
|---|---|
| 1 provider call, 0 tools | usage `Some(80_000 in / 8_000 out / 0 / 0)`, cost `Priced(96_000 µ¢)`, `tool_count 0`, cache hit `Some(0%)`, live duration `Some(900)` |
| 2 provider calls, 2 tools | usage `Some(150_000 / 15_000 / 50_000 / 0)`, cost `Priced(135_000 µ¢)` = uncached 60_000×1+10_000×2+40_000×0.1 (84_000) + uncached 40_000×1+5_000×2+10_000×0.1 (51_000), cache hit `Some(33%)` (50k/150k half-up), tool count `2` |
| provider error (0 usage-bearing calls) | outcome `Failed`, usage `None`, cost `Unavailable`, cache `None`, tool count `0` — typed unavailable, never `$0` |
| provider cancellation | outcome `Interrupted` (durable terminal, never a running state), usage `None`, cost `Unavailable` |
| restart (store closed + reopened) | token four-item / cost / tool count / cache ratio recover byte-exact; live duration `Some(1_700)` vs restarted duration `None` (renders `—`) |
| thread deletion | `token_usage::aggregate_by_message` still returns the audit rows (no thread FK, C5 deletion-safe); full projection fails closed with `NotFound` because the message row was deleted with the thread |
| unpriced run (no catalog) | tokens stay visible, cost `SummaryCost::Unavailable` (legacy/unpriced never masquerades as free), cache hit `Some(0%)` |

## UI narrow tests (GPUI, T37 conventions)

Command:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
cargo test -p vega_ui --lib summary_card
cargo test -p vega_ui --lib task_summary_card
```

Result: **PASS, 4 + 1 tests**:

- `unavailable_fields_render_em_dash_not_zero`: no-usage summary renders exactly 7 `—` and never `$0`; k/M token compaction (`999`, `1.0k`, `12.4k`, `1.0M`); `US$0.000000` for a priced zero vs `—` for unavailable; sub-second `999ms` vs `1.5s` duration.
- `priced_summary_renders_every_persisted_fact`: header `任务摘要 · 完成`, four-token line, `成本 US$0.150000`, `耗时 1.5s`, `工具 2 · 缓存命中 38%` (CJK labels render from the typed projection).
- `renders_exact_window_under_light_and_dark_without_layout_panic`: exact 960×600 window (ui-spec §6 minimum) renders the five card rows without panic under Light, then re-renders under Dark (theme tokens only; `rg '#[0-9a-fA-F]{6}' crates/vega_ui` = 0 matches).
- `read_only_card_never_traps_keyboard_navigation`: repeated Tab + Enter over the card window neither panics nor traps (the card registers no key context and no bindings).
- `task_summary_card_appends_once_and_ignores_duplicates` (conversation_stream): `apply_task_summary` appends one five-row `StreamEntry::Summary`, first-wins on duplicate/stale projections, and the visible text carries `成本 US$0.135000` / `耗时 12.4s` / `工具 2 · 缓存命中 33%`.

## Projection layer unit tests (predecessor session, verified unchanged)

Command:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
cargo test -p vega_conversation --lib summary
```

Result: **PASS, 6/6 tests** (`completed_summary_projects_exact_fields` incl. 50/130 → 38% half-up, zero-input cache hit defined as `0%`, missing usage typed unavailable, unpriced rows keep tokens with unavailable cost, non-terminal/foreign messages fail closed, usage audit survives thread deletion at aggregate scope).

## Gates

Final rerun commands (raw log in `/private/tmp/vega-s7-t40-gates.log`):

```sh
export PATH="$HOME/.cargo/bin:$PATH" && cargo fmt --all -- --check
export PATH="$HOME/.cargo/bin:$PATH" && cargo clippy --all-targets -- -D warnings
export PATH="$HOME/.cargo/bin:$PATH" && cargo test --workspace
export PATH="$HOME/.cargo/bin:$PATH" && cargo build --workspace
git diff --check
rg -n '#[0-9a-fA-F]{6}' crates/vega_ui
```

Result: **ALL PASS.** `cargo test --workspace`: **747 passed / 0 failed / 1 ignored** (the explicitly ignored real-Keychain test; final post-rebase run including S7-T39's merged tests — the pre-rebase run was 731 passed / 0 failed / 1 ignored). `cargo clippy --all-targets -- -D warnings`, formatting, workspace build, and `git diff --check` clean. Hard-coded-hex scan over `crates/vega_ui`: 0 matches. Non-test `unwrap`/`expect` added by this card: none.

## Red-line review

- No new dependency; no DDL; the only SQL added is read-only (`SELECT COUNT(*)` in `vega_store::tool_calls::count_by_message`, `SELECT ... LIMIT 1` in `vega_store::messages::last_terminal_assistant`).
- No new `ConversationEvent` variant: types.rs remains the sole UI event stream; the card is fed by existing terminal events + the `vega_conversation::summary` query projection (documented deviation below).
- No SQLite access, pricing file, or cost formula in `vega_ui`; the card renders a bounded typed `TaskCostSummary` only.
- Zero real key / real provider request / real fee; all tests use `MockProvider` + temp store.

## Deviations

1. **Summary card via app-layer query projection, not a new event.** The task card allows "既有事件 + 查询投影" when no card/contract basis exists for a new event variant. `VegaWindow::apply_agent_batch_ingress` records the run's terminal assistant message id (`AppAgentController::observe_terminal_message`) and, at run finish, projects `vega_conversation::summary::task_cost_summary` from the main store and applies it via the new `ConversationStream::apply_task_summary` (first-wins, keyed by message id). A failed projection degrades to no card — the card can never present a running task as finished (the projection fails closed on non-terminal statuses).
2. **Duration measured in the app layer.** C4 keeps wall-clock duration in live-run memory only; `ActiveAgentRun.started: Instant` is captured at run begin and elapsed is taken when the run finishes. It includes the event-poll delay (poll interval is bounded), which is an acceptable in-memory wall-clock measurement; restart recovery passes `None` → `—`.
3. **Restart recovery restores the latest terminal task summary on thread open.** Because the conversation stream is memory-only (S3: content clears on restart), the recovery path re-projects `latest_task_summary(store, thread, None)` when a thread view is constructed, so the last task's token/cost/cache/tool-count card survives restart with duration `—`. This is the C4 recovery sentence ("token/cost/cache/tool count 可按现有 message_id 持久化归属恢复") applied to the only durable card element.

## HUMAN PENDING (not automatable)

- Light/Dark side-by-side visual walkthrough of the card against ui-spec §4.2 (8px radius, 1px border-subtle, no shadow) and the §6 与 Codex/ZCode 并排截图 comparison.
- Real-keyboard walkthrough that the card does not disturb the §6 全键盘可达 flow (建会话→发消息→批准权限→看 diff→提交); automation covers no-trap behavior only.
- P1/P2/P7/P8 bench re-measurement is T41 acceptance work; this card adds no per-delta work and the card is built once per task finish.
- Real provider billing accuracy / dogfood remains S7-T41 human-only acceptance.

## Residuals and boundaries

- Duration for a live run is observed at batch-finish time (includes the bounded event-poll delay), not a precise kernel timestamp; C4 only requires it to be live-run memory, real wall-clock, and `—` after restart.
- The card shows the task of the run that just finished (one card per run). Cross-task/thread aggregates and dashboards are A10-07+ (out of scope).
- T39 (composer counter) is being implemented in a sibling worktree; this branch contains no Composer counter work and no shared-seam writes beyond the documented summary-card seams.
