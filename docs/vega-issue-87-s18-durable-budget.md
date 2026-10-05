# Issue #87 / S18 — Skills, configured budget and durable compaction

Status: **reviewed and frozen by the main agent** (2026-10-06). The four-function test-only joint evidence slice is approved. The main agent separately released the single test-module registration after S14's writer finished; focused execution is now authorized on the current baseline, followed by a new run after S14 actually integrates. Original-run budget/schema persistence and native acceptance are excluded from this approval. Whole S18 and Issue #87 remain **PARTIAL**.

Tracking: [Issue #87](https://github.com/puzige/vega/issues/87). Contracts: [Skills S3/S6](vega-issue-74-skills.md), [S18 delivery row](vega-issue-74-skills-delivery.md), [model-owned budget correction](vega-issue-76-model-context-correction.md), [context persistence](vega-issue-76-context-compaction.md) and the later [Claude compaction adaptation](vega-claude-compaction-parity.md). The latter supersedes the old fixed-byte staged-summary and rolling-summary strategy, including its output limit. Current #140/#149 instructions govern external execution and test scope.

## 1. Baseline, authority and observed gap

The independent worktree was created after `git fetch origin` at `3f696af368975204e3ff3558c2ad0fa59f62c0ea`, then fetched and rebased onto `a53662e46f85cfd6155dc1f556b4fb4e578613e7` after PR #264 merged. The newer baseline includes the Plan/text provenance-query fix and the Plan-only activation anchor. No master files or other task worktrees were edited.

The existing S18 runtime test exercises `ContextBudget::new(428_000, 128_000, true)` and a tail-preserving mock hook. It establishes the loop's captured projection and active envelope, not a conversation-owned checkpoint or a closed/reopened Store. Source review found the production budget resolver, actual conversation hook and checkpoint installation already implemented. Missing joint evidence is **not an established production defect**.

| Existing path | Source fact | Remaining evidence needed here |
| --- | --- | --- |
| `agent/pipeline.rs`, `prepare_run_with_images_and_reasoning` | Exact frozen provider/model selects `ModelContextPolicy`; checked `B + O` constructs runtime budget. Missing policy uses the assumed default; legacy thread rows are not the runtime fallback. | Combined Skills run must obtain its budget through this path, rather than injecting an `AgentRequest` budget. |
| `agent/entry.rs`, `run_thread_task_with_images_reasoning_and_mcp` | Attaches the prepared Skill run and MCP handles, starts the persistence actor, and supplies `ConversationCompactionHook` whenever a budget exists. | Exercise this existing entry, including persistence acknowledgements, without a replacement compaction hook. |
| Runtime `agent/loop_.rs` | Rebuilds catalog and active Skill system envelope each round; freezes native/MCP/Skill schemas once; evaluates actual message/schema estimate before primary requests. | Capture real `ChatRequest` values from the MockProvider and compare accounting events with those exact values. |
| Conversation `agent/compaction.rs` | Reads a bounded SQLite snapshot, retains newest user/live suffix, validates source/predecessor, estimates the full prospective projection and appends one checkpoint through a guarded transaction. | Observe append, immutable old rows, source coverage and resumed primary request with an active Skill. |
| `pipeline::primary_history_from_context_source_with_checkpoint` | Shared reload projection labels summary and old tool records as untrusted history, and removes old executable tool protocol. | True close/reopen must recover one summary and intact tail with zero automatic provider/tool replay. |
| `agent/events.rs`, `agent/persistence.rs` | Skill activation/snapshot and context lifecycle/usage are acknowledged through durable persistence. Summary Usage rows have no assistant message ID. | Check separately stored binding/digest, persisted usage and terminal status before read-only recovery. |
| `agent/skills.rs`, `recover_skill_run`; `history.rs`; Store `skills.rs` | Recovery validates Store snapshot/binding/digest and returns provenance, not execution. Current baseline projects both text and Plan owners. | Recovery after owned source removal must validate frozen provenance and leave counters/rows unchanged. |

### Known limits of stored metadata

The current private Skill snapshot stores catalog, binding, bodies/references and their integrity data. It does **not** store the original runtime `ContextBudget` or the exact frozen native/MCP/Skill schema vector. `context_checkpoints` stores thread/model, source metadata, estimator and summary, not provider identity or a whole run configuration. Live context accounting/Usage anchors are not persisted as historical provider usage; a new run starts estimated again.

Consequently this slice can prove the persisted model policy reopens, the old checkpoint/provenance restores, one running request keeps its frozen policy, and a later direct user submission uses freshly resolved policy/catalog. It cannot claim recovery of an old run's budget/schema vector or resume that run. The older Skills integrator handoff requirement to persist those exact run metadata remains explicitly open. Adding such storage would need a separate main-reviewed additive contract; this slice proposes no migration or implicit resume.

## 2. Proposed scope and invariants

Implement only focused tests using existing private services and owned fixtures unless an actual business failure justifies a separately reviewed production correction.

1. Use an owned project and file-backed SQLite Store, approved project Skill and model policy saved through existing Store fixture APIs. These fixtures are not native/UI consent evidence. The primary test uses Execute/Confirm solely so an approved mock MCP schema is available; no operational tool is proposed or granted authority.
2. Freeze exact model policy `B=300,000`, `O=128,000`, `auto=true` through the production reasoning/provider selection. Arithmetic: `L=B+O=428,000`; trigger `ceil(300,000*4/5)=240,000`; target `floor(300,000*3/5)=180,000`. Assumed capacity is not discovered provider capability.
3. Size deterministic completed historical text and a bounded Skill body so actual initial primary input is below 240,000, model `load_skill` succeeds, and the following projection reaches 240,000 while remaining at or below 300,000. Assert these values from captured requests/events. A fixed approximate string length is not acceptance evidence; fixture-calibration failures are recorded as harness failures.
4. Invoke the existing conversation entry. The actual `ConversationCompactionHook` must call the same owned MockProvider for a summary, append one new checkpoint, then let the primary loop continue. Do not manually install the expected new checkpoint or substitute `TailPreservingCompactionHook`.
5. Catalog and active envelope each occur once in the primary system message. Exact body bytes/hash stay frozen after changing the owned external `SKILL.md` within the accepted run. The summary provider receives historical data, not current active Skill instructions; no Skill body or consent/audit is promoted from a summary. Newly returned reference text is lower-trust tool data and included in the following request estimate, not another system-body copy.
6. Every primary request carries the same exact frozen schema vector. Each schema name is unique. Catalog/body/reference removal and individual MCP-schema removal must produce a strictly smaller estimate on private clones of captured requests; this proves nonzero contribution while preserving the production estimate's one conservative factor. Do not assume independently rounded deltas add linearly.
7. Summary requests use empty tools and the current `min(20,000, O)` cap from the Claude adaptation; default primary `max_tokens` stays absent. O is not an instruction to generate 128,000 tokens. No summary request exceeds B and no post-summary primary projection exceeds either the 180,000 target or B. Account actual MockProvider Usage separately from deterministic estimates.
8. Compaction may append a checkpoint. **Original messages/tool/image rows and all pre-existing checkpoints remain byte-for-byte unchanged.** Freeze exact sorted tuples, including IDs, chronology, kinds/status, tool input/output/approval, image bytes, and checkpoint summary/source/predecessor fields. Exclude only the new accepted turn and its new rows from old-row comparison. Never compare the whole evolving source fingerprint as if it should remain constant after new rows.
9. Seed an optional small valid predecessor through existing fixture APIs before the accepted run, retain its full tuple, and require the new checkpoint's expected predecessor to refer to it. Only the checkpoint created by the production hook counts as compaction evidence. Covered source stops before newest user; live `load_skill`/resource calls remain outside the covered prefix and occur once.
10. Runtime failure/cancellation preserves the original user task and raw history. Existing soft-threshold summary fallback is retained: a recoverable summary failure can continue only when the untouched primary request still fits B. It must not fabricate a successful checkpoint. Cancellation/source/authority failures remain terminal.

### MCP schema without a network or tool call

Use an owned `vega_mcp::mock::Endpoint` with bounded synthetic `tools/list` content and existing `HttpClient::connect` / `McpReadyServer::connect_http`. Conversation dev-dependencies already enable `vega_mcp/test-support`. In `vega_mcp::transport`, that feature directs `send_mcp` to the in-process handler registry; it never calls `client.execute`, binds a socket or launches a server. Missing registration fails with a transport error, without network fallback.

The handler records bounded JSON-RPC method names only; the current owned fixture responds to `server/discover` and `tools/list` and rejects every other method, including `tools/call`. Freeze the advertised mock schema and revision in captured primary requests. Assert zero `tools/call`, zero permission prompts and zero native/Bash dispatch counters before/after recovery. Do not introduce a ready-server constructor or expose private registry APIs for testing. This is process-local schema/authority evidence, not real MCP consent/transport acceptance.

## 3. True Store lifecycle and read-only recovery

1. Complete the production conversation function so its runtime, event processor, persistence actor and Skill watcher have all joined/closed. For cancellation, likewise await full termination; dropping the future alone is not accepted.
2. Capture old-row tuples, final assistant status, checkpoint tuple, snapshot digest/binding, activation audit, tool rows and actual usage through owned read APIs. Capture provider call count, endpoint method counts, prompt count and in-process tool dispatch counts.
3. Drop the first `Store` and every fixture-held SQLite connection/read guard. Retain only the owned TempDirs/database path and scalar/private copied expectations. No second live Store is called a restart.
4. Remove or change only the owned activated Skill/reference source files. Do not revoke consent just to simulate a missing source; that would test a different authority state.
5. Open the same database path with `Store::open`; use migration idempotently if needed and the normal `restart_history_page`, `recover_skill_run`, Store checkpoint and snapshot reads. Recovery must perform no source read, provider request, MCP/tool replay, new usage/audit insertion or checkpoint rewrite.
6. Validate old body/reference bytes through `SkillRun::restore_snapshot` using separately Store-fetched binding/digest; compare its exact private frozen envelope/hash with the pre-close expectation. Public/history projections expose only verified content-free provenance. Do not publish snapshot bytes or private text.
7. Compare the restored source projection through the shared checkpoint helper: one labelled untrusted summary, intact latest user/tail, old tool activity as untrusted text without executable tool-call protocol. Browseable original messages/tools remain exact in the Store.
8. Any subsequent user submission is a **new run**. Validate fresh policy/candidate selection separately; a stale explicit pin fails before a provider/operational tool request. The old run is never automatically resumed to prove continuity. Changing policy must not retroactively rewrite its checkpoint or provenance.

## 4. Usage and failure evidence

For success, feed one summary `ProviderEvent::Usage` with fixed nonzero input/output/cache values and no pricing catalog. Assert one thread-level `token_usage` row (`message_id=NULL`) with those exact values, NULL pricing version/profile/start-time, and `known_unpriced` terminal status. The existing integer cost placeholder must not be reported as known zero cost. Known tokens, unavailable price and incomplete token usage are different states.

A paired branch omits summary Usage: no fabricated zero-token row; the persisted operation is `unknown`, `has_unknown_usage_for_thread` remains true after reopen, and subsequent primary usage cannot make the earlier summary accounting complete. Deterministic `predicted_input` is not substituted for actual Usage. No pricing lookup or real credential reader is installed.

For a primary mock 503 after successful summary, retain its valid checkpoint, successful context status, frozen Skill snapshot and failed assistant state. This broadens owned joint-path coverage without claiming the root cause of Issue #168's historical native failure. For cancellation during the summary, use cancellation-aware event/stream gates, not a real delay as synchronization: no new checkpoint, no follow-on primary request, an interrupted assistant and durable activated provenance; any genuinely emitted Usage remains accounted once.

## 5. Acceptance matrix and first-run classification

Every row below is **NOT RUN** at specification time. Existing unrelated results are not reused as this slice's results.

| ID | Requirement/risk and owned operation | Observable criterion | Evidence class / planned test |
| --- | --- | --- | --- |
| DB01 | Production configured B/O, large auto Skill, actual hook | Initial primary <240,000; post-load decision >=240,000 and <=300,000; one summary and appended checkpoint; continuing primary <=180,000 | Conversation + MockProvider; `issue87_s18_durable_configured_budget_compacts_and_recovers` |
| DB02 | Exact catalog/body/MCP schema accounting | Single catalog/body; identical unique schema vector each primary round; component removal reduces captured wire estimate; post-summary accounting equals exact next request estimate | Same DB01, private request comparisons |
| DB03 | On-demand reference after compaction | Frozen active Skill remains available; first reference result reaches next request once as lower-trust data; no body/path in activation audit/provenance | Same DB01 |
| DB04 | Raw history and predecessor | Old sorted message/tool/image/checkpoint tuples unchanged; new checkpoint has correct source/coverage/predecessor; live calls not re-executed | Same DB01 |
| DB05 | Source edit then complete run and true close/reopen | Old binding/digest/body/reference validate after source removal; verified history and old summary/tail; zero automatic provider/tool/MCP calls or new rows | Same DB01 |
| DB06 | Known unpriced and absent summary Usage | Exact one thread-level known-token row vs no fabricated row; known_unpriced/unknown restored, price still unknown | DB01 with two owned fixture variants |
| DB07 | Frozen policy while running; fresh policy after reopen | Old run keeps B/O across rounds despite an owned policy save; later prepare gets new exact policy; no legacy/provider/model leakage or old-run replay | `issue87_s18_durable_policy_freeze_and_new_run_resolution` |
| DB08 | Successful checkpoint then primary 503 | Ordered success then typed primary failure; failed assistant; checkpoint/snapshot/raw rows recover unchanged; no automatic retry | `issue87_s18_durable_primary_failure_keeps_checkpoint_and_provenance` |
| DB09 | Stop during actual summary | Summary cancelled; zero follow-on primary; no new checkpoint; interrupted assistant; activated snapshot and any observed usage preserved | `issue87_s18_durable_summary_cancel_preserves_history_and_provenance` |
| DB10 | Stale explicit selection on a later user run | Current source hash/availability rechecked; selected unavailable error; zero provider/prompt/operational tool calls; original submitted text retained | DB07 plus existing stale-pin regression |
| DB11 | Input/body cannot fit, source/CAS and malformed recovery safety | Existing focused guards remain intact; no larger ceiling, weakened assertion, retry-to-green or schema change | Named related tests below; this does not claim a new large-B irreducible-Skill integration case |
| DB12 | Native model/UI, real permission/MCP transport, summary quality | Installed exact-version UI/real-provider evidence with accepted source and current capacity | NOT RUN / separate native acceptance |

### First-red observation standard

No failing business result has been observed during this audit. After main approval, register the new tests and run them once on the frozen unmodified production baseline. A failure of exact envelope retention, budget enforcement, source/coverage/old-row preservation, snapshot integrity, cancellation or replay assertions is a candidate first-red business regression; preserve raw output before changing production. Establish its actual cause before calling it a bug.

A compile error, missing/invalid fixture binding, mistaken private-test registration, or initial fixture not meeting the intended threshold is a **harness/precondition error**, not product evidence. Preserve it and repair only that precondition. If the first business run is green, report first-run green and test-only coverage, with zero production fix. Do not force a synthetic red, retry until green or claim the runtime's earlier mock hook was a durable test.

## 6. Ownership, review points and implementation sequence

Owned proposed files:

- This independent specification.
- New child test module `crates/vega_conversation/src/agent/tests/skills/s18_durable_budget.rs`, with private owned fixture/capture/lifecycle helpers. It can reuse parent Skills approval helpers and the existing production entry without adding APIs.
- Exactly one `mod s18_durable_budget;` registration in `crates/vega_conversation/src/agent/tests/skills.rs` **only after the main agent serializes ownership with S14**. Until that grant, this shared file stays untouched.

S14 currently owns Runtime/catalog/private snapshot compatibility, pipeline/types/tool-card files and Skills test registration; S16's Plan/history changes are already in the refreshed baseline. This stage edits none of those files or the common delivery matrix. Implementation must rebase after S14 integration and recheck module/snapshot compatibility, with new focused results on that integrated tree. The main agent owns serialized delivery-row writeback, review, PR and integration.

Review decisions before implementation:

1. Approve the bounded joint-path **test-only** slice and the four named test functions/fixture variants. No new public API, dependency, migration, production provider wrapper or protocol change. A private cancellation-aware test provider may delegate to the owned MockProvider without replacing the production compaction hook.
2. Confirm the matrix's raw/old checkpoint immutability wording permits append-only new checkpoints, as existing production compaction requires.
3. Keep original-run budget/schema persistence explicitly open. Do not call DB07 a restored original request or make whole S18 PASS.
4. If a business first-red requires production work, return its raw output/root cause and exact minimal proposed file change for main review before editing shared production files. Keep mode/permission/revocation/path fences and #76 behavior unchanged.

Sequence after approval: refresh base -> serialize registration -> add owned tests -> preserve first compile/business outcomes -> minimal reviewed repair only if justified -> run the final focused selection and related safety tests -> freeze tested tree/source/log hashes -> main review/cloud CI/integration. No push, PR, package, install, user DB/config mutation or external write is delegated here.

## 7. Planned focused verification and truthful delivery

These commands are a **future plan**, not execution evidence. The new selection matches only the four proposed functions; fixture variants are branches within those tests. Do not invent counts or run IDs before Nextest prints them.

```sh
cargo nextest run -p vega_conversation -E 'test(/issue87_s18_durable_/)'
cargo nextest run -p vega_conversation -E 'test(/model_owned_budget_is_shared_across_threads_and_frozen_before_tool_rounds/) | test(/same_model_id_different_provider_uses_only_exact_model_policy/) | test(/missing_model_policy_uses_default_budget_not_legacy_thread_settings/) | test(/issue76_auto_source_fence_rejects_unrelated_history_before_summary_request/) | test(/issue76_real_hook_compacts_after_persisted_tool_result_without_reexecution/) | test(/issue168_auto_compaction_then_primary_failure_keeps_checkpoint_and_fails_run/) | test(/issue88_v1_checkpoint_remains_readable_under_v2_with_shared_wrapper/) | test(/issue74_project_auto_load_persists_audit_snapshot_and_content_free_receipt/) | test(/issue74_explicit_pin_is_loaded_before_round_one_and_stale_pin_pauses/) | test(/issue74_reference_result_persists_lower_trust_bytes_without_path_in_audit/)'
cargo nextest run -p vega_runtime -E 'test(/issue74_s18_model_load_compacts_near_configured_context_budget/) | test(/issue74_skill_still_over_budget_after_compaction_is_rejected/)'
cargo nextest run -p vega_store -E 'test(/model_policy_is_shared_across_threads_but_isolated_by_provider_and_model/) | test(/model_policy_rejects_partial_or_inconsistent_numeric_values/) | test(/stale_same_seq_checkpoint_is_rejected_and_raw_rows_remain/) | test(/checkpoint_install_reloads_and_preserves_previous_on_insert_failure/)'
```

Use the worktree's isolated default target. Retain exact commands, exit codes, first/final raw footers, actual run IDs, selected counts, source/tested-tree hashes and duration outside the worktree. No new ignore/retry, wall-clock tightening, local workspace-wide test or real shell/Git/network/MCP/model execution. Formatting/whitespace review follows task instructions; full Clippy/Nextest required gates belong to cloud CI.

This audit has run no Cargo command or test and created no native artifact. Successful future cases may close only this owned MockProvider/Store evidence gap. Native acceptance, real summary quality/limit claims, real MCP use, original-run budget/schema persistence and whole Skills delivery remain separately tracked; S18/#87 must not be marked Done on these results alone.

## 8. Approved implementation and baseline evidence

The main agent approved the four-function test-only slice and separately released the one-line child registration after the S14 writer finished. The implemented child uses the normal conversation entry, configured provider/model policy, actual conversation hook and file-backed owned Store. There are no production changes, new public APIs, dependencies, migrations, test bypasses, ignores or retry changes. The acceptance contract in sections 2–5 is unchanged.

Each fixture makes **one separate owned calibration request** on `thread-s18-calibration` to capture the production system/catalog and schema vector. Binary search uses the production estimator and shared historical projection to select initial text near 225,000 input tokens, below the 240,000 trigger. The actual main request/events must independently satisfy all budget assertions. Five fixture instances exercise the four test functions because the successful recovery function has known-unpriced and omitted-Usage variants; those five calibration requests are not additional business test results.

The service return awaits Runtime/event processing, Skill watcher, diagnostic draining and persistence actor close. The fixture then takes and drops its only `Store`, with all local statements/SQLite guards already out of scope, before reopening the same path. SQL-derived private snapshots include every column, row identity and blob bytes of original messages/tools/images/checkpoints; comparisons never print private bodies. The finished run's context statuses, Usage, Skill snapshot and activation audit also compare exactly after recovery. Source removal changes only owned Skill/reference files, without consent revocation.

### First-run classification retained

| Attempt | Actual result | Classification and narrow correction |
| --- | --- | --- |
| First compile, frozen tree `2262f73714e8d6c12b6968c7f8661b51bed1b7ec` | Exit 101; no Nextest run or business execution | Harness E0277: `ApprovalAudit` has its existing strict `to_json()` codec rather than `Serialize`; only the private fixture call was corrected. |
| `5a962d57-2dae-48f6-a604-123e0adc6c16`, tree `b7adff82c6ca4dd6dedb0375852f2dcc4dd48ae8` | Exit 100; 4 selected, 0 passed, 4 failed, 553 skipped; 0.081s | All four stopped in independent calibration. Synthetic MCP description exceeded the existing 2,048-byte boundary; repeat count changed from 128 to 48, leaving the production limit and nonzero schema-contribution assertion intact. No main run began. |
| `776ae8f4-b657-4db5-94d4-68a29ea3731a`, tree `2a309e481309730cbb1bf3a61a4bf9c66e91ef43` | Exit 100; 4 selected, 3 passed, 1 failed, 553 skipped; 1.350s | Success/503/cancellation business functions were first-run green. Policy test passed old frozen wire/reopen/new budget, then wrongly unwrapped a fresh optional Skill state. Existing `prepare_skill_run_with_config_dir` returns `None` without current candidates or pins; the corrected exact assertion requires that absence after source removal. No product regression was observed. |

These outcomes are retained as harness/precondition failures, not fabricated product first-red evidence. The subsequent four-function green run is **coverage-only**. Production fixes: **0**.

### Baseline freeze and actual focused results

- Baseline HEAD: `a53662e46f85cfd6155dc1f556b4fb4e578613e7`; tested tree: `b6e64a7a569ea2b66fbe081fd76dbb56146691a7`.
- Verified after all baseline selections at `2026-10-05T19:28:02.662197Z` / `2026-10-06 03:28:02 Asia/Shanghai`.
- Child source SHA-256: `353b4c44ddd18cd4af1ad021fd2c519b1545b26af1ab639d4717f403c5f353f2`. Source/registration/config/lock/toolchain hashes and tested tree were unchanged across the successful selections.
- `default` profile, `retries=0`, `fail-fast=false`; isolated worktree target; Rust 1.98.0 and Nextest 0.9.146. No workspace-wide local test, real operational Shell/Git subprocess, network transport, user DB/config, package or installation was used.

Commands are the four exact focused selections in section 7. Their actual results are:

| Selection | Nextest run ID | Raw footer | Exit | Raw log SHA-256 |
| --- | --- | --- | --- | --- |
| Four new conversation functions | `e888665e-6179-4ee2-801f-2b7733920658` | `Summary [   1.361s] 4 tests run: 4 passed, 553 skipped` | 0 | `8de302ad4c86592da2363d9a7e80ac59d506d6e430c09cbb103ca310ba977e4c` |
| Named conversation regressions | `ad1c4b15-5c44-4a8d-814a-570bb9ddd264` | `Summary [   0.102s] 10 tests run: 10 passed, 547 skipped` | 0 | `007052879de3dc00164a730bbe305cb66812a903d2e06c4412519ecf74656f4d` |
| Named Runtime regressions | `49b590e1-ceb9-4aac-8efa-6895a83ea9f4` | `Summary [   0.059s] 2 tests run: 2 passed, 251 skipped` | 0 | `f44798b9047b1a4eba56fc1a1774d2455f183ce1de53bd03052066d309d31e49` |
| Named Store regressions | `c1676e1b-aef8-4372-9bd1-2e530f5fe228` | `Summary [   0.068s] 4 tests run: 4 passed, 148 skipped` | 0 | `3a4448a7ca2516e2948729fc78d32dba010c281ceff513a05b7510f17d5da4e1` |

DB01–DB10 now have owned process-local joint evidence on this baseline; DB11's named related guards are green. These results do not prove original-run budget/schema recovery or real model/UI/MCP acceptance. S14 is not yet integrated into this tested tree, so its final integrated-base run is pending and must be recorded separately. Whole S18 and Issue #87 remain **PARTIAL**; the shared delivery matrix is unchanged.

Source self-review then strengthened the same zero-operational-replay criterion with the exact persisted live-call set: only one `load_skill`, plus the optional one `read_skill_resource`, all terminal success. This applies before and after recovery, including cancellation's load-only branch. It adds no test function or production change and relaxes no assertion. On the same baseline, frozen tree `e14bcbf0deae8460fda09b2e6e1fc0bdc5be7581` and child SHA `0d1f28db58de50ef529e8d16a15437525207fd27ce02021f7c10224cad6d32f2`, run `cb397c85-26fe-48d7-89c3-5a415d5b08a1` exited 0: `Summary [   1.385s] 4 tests run: 4 passed, 553 skipped`. Raw log SHA: `0676e90a6bf16d6f8bf8da4142553ab1339c71ee89f7ad0e7bdd4efbe03c0b6b`. Source/config/tree remained unchanged throughout this run; the final S14 integrated selection remains pending.

The main agent completed full source/spec review and approved two further direct-wire assertions: the latest user `TASK` occurs exactly once in every primary request, including the primary before summary cancellation; each resumed primary contains exactly one labelled untrusted historical summary, with User role and the bounded returned summary text. This only strengthens already-frozen DB01/DB04/DB05. The pre-change source/commit and all raw logs remain retained. After fresh fetch/rebase confirmed the same base, these assertions were added without another old-baseline test run, as the main agent directed. Their result is **NOT RUN** until the final selection on the actual S14 integrated base; earlier green results do not cover this later source revision.
