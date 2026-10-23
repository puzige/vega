# #542 test-first evidence

## Authorized checkpoint

- Stage: TEST-FIRST ONLY; tests and temporary exclusive observation/external-worker wiring. No product behavior implementation, independent review, Actions RED/GREEN, Git index mutation, commit, or push by this writer.
- Full base HEAD: `2ba904f7074115de5ee89cc18c376c083f803357`. Parent fetched/fast-forwarded before this continuation; its only upstream delta from `871f91f246e90753743a256cee1eec315e74effc` is unrelated docs/554, with identical production code (parent handoff).
- Frozen specification: v2.1, SHA-256 `a3edebd8d929a6a7b2a564b22c2c1b8c41b36ca86a850973c1fc5e0786f6197b`, rechecked unchanged after formatting. Spec, observation note, and pre-RED review are parent-owned and were not edited.
- Intake tracked `git diff --binary` SHA-256: `797b30cf0d771a4739a86b558af5418706ed2dd9d0d21a05f48bdc7b67f663dd`; intake untracked observation module SHA-256: `8b9be2e189a1cf9a040bad27d272a46df6dd437b996a9143a59df1df2fa7a4e9`. Both matched before writing. Previous incomplete work was preserved, not restarted.
- Read root AGENTS.md, execution guide (including §7.2), design guidelines, test-delivery lifecycle, frozen spec, observation note, and pre-RED review. A worktree search found only the root AGENTS.md.
- One code/test writer, no nested delegation. Existing controller test location and temporary cross-crate observation feature are explicitly authorized by spec §测试注入缝; the earlier statement that controller tests were unauthorized is obsolete.
- No local Vega execution, compilation/build/check/Clippy/test/nextest, Python tests, CUA, external applications, or worker execution. Static source inspection of the locked GPUI APIs is not runtime validation.

## Exact test inventory — STATIC ONLY

22 definitions: 12 UI GPUI tests, 6 controller GPUI tests, 4 conversation projection unit tests. These are source-definition counts, not discovered/executed/passed counts. Looped outcomes/themes/targets are not counted as separate tests. Actions discovery/execution/pass/fail/ignore/skip counts: **not obtained**.

### UI: `crates/vega_ui/src/artifact_card.rs`

| Test (line) | Frozen observable expectations |
|---|---|
| `production_text_observation_covers_card_content_actions_and_danger` (842) | Observation positive control in LIGHT/DARK: actual production workspace renderer paints filename, preview content containing both legacy source strings, real Preview/editor actions, and a known danger-styled error. Actual selectable assistant shaping/paint and container ancestry must also be observed. Does not require obsolete source metadata to survive the later implementation. |
| `artifact_cards_stay_out_of_the_stream_while_the_run_is_active` (919) | Active public capture acceptance yields no rendered file/group; terminal later exposes that file in its owning group. Controller registration is separately exercised below. |
| `finished_turn_groups_captured_artifacts_after_the_assistant_entry` (936) | Completed, Interrupted, and Error each yield one group after the final assistant and before the real Summary layout anchor; duplicate terminal/Summary delivery does not duplicate it. |
| `artifact_group_collapses_repeated_edits_of_one_file` (973) | Three distinct capture IDs for one filename yield one row with latest size and latest card/file preview capability (`2001`, generation 3). Three distinct files yield three rows in their own turn. |
| `artifact_group_offers_show_more_beyond_the_visible_cap` (1038) | Seven files yield six file rows plus Show 1 more; actual control click reveals all seven and removes overflow. |
| `artifact_group_omits_source_labels_and_editor_row` (1069) | Group-local actual text excludes source labels and default editor actions; known/unknown sizes, font/metadata/color/row geometry, hover/actions, LIGHT/DARK quads; workspace shared renderer retains its original row model/actions while omitting source metadata. Actual filename positive controls precede negative text assertions. |
| `artifact_group_keeps_failures_visible_after_the_turn_ends` (1192) | Error-only first capture has exactly one real danger text, no file/size/actions; same error in a prior group cannot satisfy the next turn. Seven files plus capture error has eight rows collapsed/expanded. Seventh-file preview/open errors are tested while still collapsed (nine rows with two errors); resolving the file error returns to eight. |
| `artifact_captured_after_the_turn_finishes_joins_that_turn_group` (1308) | First late capture creates old group before old Summary; later captures update it without duplicates. Same filename in new turn remains new-owned. Unknown call/error and history-reset stale results cannot adopt the active turn. |
| `artifact_menu_dispatches_allowlisted_targets_and_closes` (1427) | All six real menu choices emit matching typed events; trigger re-click, outside click, and Escape close the menu. |
| `artifact_preview_preserves_workspace_rows_and_request_fences` (1493) | Actual Preview control emits typed event; preview/error maintain `2 + preview_lines + error`; stale card/file cannot replace body; reading mode hides action row without changing row count. |
| `artifact_controls_do_not_request_unavailable_or_busy_files` (1571) | Missing current file/opening/reading states emit no disallowed request; disabled controls expose an actual painted real-tooltip reason after controlled test-clock advancement. |
| `artifact_actions_are_keyboard_reachable` (1646) | Tab/focus exposes controls without hover; Enter activates preview/menu; observable focus and typed events, not direct business-method substitution. |

### Controller: `crates/vega/src/window/artifact.rs`

| Test (line) | Frozen observable expectations |
|---|---|
| `captured_card_is_previewable_before_the_turn_finishes` (1536) | Preserved real registration/subscription/preview-fence regression, extended with mounted real stream: no active-run file/group, real Preview request and completion still work, then terminal group contains file and real preview error. |
| `rejected_proposal_before_stream_delivery_keeps_its_turn_error` (1592) | Oversized proposal enters actual batch ingress; real `validate_proposal` fails before stream proposal delivery and still cancels/fails route closed. Terminal yields independent ArtifactLimit error-only group for A, retained after B starts/finishes, never assigned to B. |
| `delayed_capture_worker_error_stays_with_the_finished_turn` (1654) | Actual capture dispatch for A, then A terminal/Summary and B start before original completion API receives `Err(GitUnavailable)`. Real rendered error belongs only to A. B's later SpawnFailed is asserted independently inside B's group. |
| `unknown_and_stale_capture_completions_do_not_adopt_the_current_turn` (1689) | Unknown route Err, stale sequence for a known A call, matching-sequence unknown call projection, and completion after real hard route close cannot create file/group in B; route cancellation tokens remain cancelled. |
| `cancelled_capture_completion_does_not_create_an_error_row` (1772) | A interruption plus delayed `Err(Cancelled)` produces no artifact error/group for A. B's subsequent real worker Err still yields its own error-only group; cancellation is not reclassified as a new UI failure. |
| `superseded_preview_completion_cannot_replace_the_current_result` (1830) | Preserved real request/completion fence regression: stale first completion is rejected; current completion controls preview rows/current capability. |

Fixture: passive `VegaWindow::for_artifact_tests`, inert store, no-I/O `GitWorkspaceService::for_test`. Only external terminal/preview worker execution is intercepted at existing cfg(test) inbox hooks (561–567, 776–782). Proposal/capture preparation, dispatch, ownership, registration, subscriptions, route closure, and completion/fence decisions remain the real implementation. Real stream host/helpers are at 1347–1508; `rendered` requires actual assistant paint before artifact assertions. Summary projection is supplied through the existing public stream boundary because the fixture deliberately has no database.

### Safe size projection: `crates/vega_conversation/src/artifact/mod.rs`

All four inherited tests retained unchanged:

1. `artifact_size_is_projected_from_retained_evidence` (638).
2. `terminal_byte_count_without_evidence_is_not_a_projected_size` (646).
3. `empty_file_evidence_projects_a_known_zero_size` (654).
4. `provenance_downgrade_removes_the_projected_size` (662).

## Original pre-RED findings: test-layer resolutions

| Finding | Final static resolution / anchors |
|---|---|
| 1 — Text and color observation | `artifact_card/observation.rs:59–109` delegates the real SharedString Element; layout stores actual `(TextLayout, TextStyle)`, paint delegates first, then records `layout.text()` and the layout-time style. Clip/viewport/window/frame isolation at 116–139,247–265. Known production danger/control/content positive control at `artifact_card.rs:842–916`. |
| 2 — Same capture ID hid dedup defect | Projection factory at `artifact_card.rs:608–616` uses distinct IDs by generation; test at 973–1033 checks distinct IDs, latest size, typed latest card ID 2001/file generation 3, and group-local distinct-file text. |
| 3 — Private list indices were not visual order/ownership | `render.rs:1572–1578` transparently wraps actual assistant/Summary/group elements, using public Summary/group getters, not `entry_identities` or indices as evidence. `selection.rs:348–349` records actual shaped text after paint. Observation module 194–244,272–348 uses painted-container ancestry and real geometry; `ordered` requires owner assistant bottom <= group top and group bottom <= Summary anchor. Completed Summary is intentionally zero-height at baseline; its real layout/paint anchor is used, not invented visible Summary text. |
| 4 — Seventh-file errors only after expansion | `artifact_card.rs:1252–1283` asserts real preview/open error danger text inside the correct group while file 7 is hidden. Show more is not clicked until 1296. |
| 5 — Disabled reason not observed | `artifact_card.rs:1571–1643` retains disabled/no-event assertions and actual tooltip paint assertions. `icons.rs:265–280` only wraps the real tooltip leaf behind the temporary gate; no expected reason is injected into observation. |
| 6 — Controller synthetic projection omitted size | Controller projections at `window/artifact.rs:1317–1324,1709–1716` now declare prospective `size_bytes: Some(6)`, consistent with UI/projection tests. No production field stub was added. |
| 7 — Controller ingress/late failure coverage absent | Actual ingress early-failure test at 1592, delayed original-API Err ownership at 1654, unknown/stale route/call/sequence at 1689, cancellation at 1772. Rendered group-local text/color/geometry, not only controller fields or mock counts. Both previous controller regressions remain at 1536 and 1830. |

These are static test/harness corrections for original acceptance, not product acceptance changes. They still require independent review of this stopped version and Actions observation controls before any failure can be accepted as target RED.

## Observation API review and boundaries

- Rechecked locked `gpui-kit/base 0.6.1`, `gpui-pre 0.3.4` source only. SharedString Element uses `TextLayout`/`()`, `Window::text_style()` returns owned TextStyle, public TextLayout::text exists; AnyElement request_layout/prepaint/paint delegation matches the transparent wrapper. GPUI list prepaints actual item geometry with absolute offsets, and paints real item children.
- `items` selects actually painted containers; text collection requires positive clipped width/height. Tokens are ancestry identifiers only, never visual-order indices. Geometry establishes visual order, and actual text ancestry establishes local content ownership.
- Evidence is text-layout/style plus real paint-call/clip observation, **not framebuffer pixels** or proof of absence of arbitrary occlusion/opacity. Styled assistant runs are used for text/geometry only; no inherited color is asserted for StyledText. Controlled test windows and disabled motion avoid offscreen/animation expectations but remain unexecuted.
- Legacy source strings used as positive preview body content are user data and must survive source-label removal; the old metadata and action leaves themselves are also instrumented in the unchanged production renderer. Cloud baseline logs must confirm these controls work; missing instrumentation, unrelated type errors, or offscreen content are harness failures, not product RED.
- Workspace UI tests reuse the production ArtifactCard row renderer, not the full app workspace chrome. Controller tests prove real registration/request/completion behavior but deliberately do not execute Git or external preview/open workers.

## Minimum prospective target interfaces — absent, not stubbed

1. `vega_conversation::types::ArtifactCard::size_bytes: Option<u64>` (alias `ArtifactProjection` in tests), derived only from retained ArtifactEvidence; four projection assertions reference it.
2. `ConversationStream::apply_artifact_capture_error(&mut self, call_id: &str, code: GitWorkspaceErrorCode, cx: &mut Context<Self>) -> bool`; unknown/stale call ownership must return false.
3. `StreamEntry::ArtifactGroup { group }`, whose entity exposes `message_id(&self) -> &str` and `row_count(&self) -> usize` and renders the frozen group row model. Observation calls those getters; it does not create a fake group renderer.

Controller tests deliberately retain the original `finish_artifact_terminal(&identity, Result<(u64, ArtifactTerminalResult), GitWorkspaceErrorCode>, cx)` API. Err currently has no call/sequence, and `terminal_in_flight` is only `Option<u64>`. Later implementation must preserve proven ownership in controller delivery/in-flight state without changing capture safety/error semantics. No new completion type/signature is required or stubbed by this patch.

Until these interfaces exist, Actions may report only minimum missing-target-interface RED; **behavior assertions have not executed**. Do not classify unrelated harness/type/dependency failures as valid RED.

## Static checks actually executed

| Command / inspection | Exit / result |
|---|---|
| Intake `git rev-parse HEAD`, status, tracked diff hash, spec/module SHA-256; scoped AGENTS search | 0; matched handoff |
| Source/API inventory using focused read/rg | Static inspection only; a few exploratory paths did not exist (rg exit 2), then located actual `app_agent.rs` / `sidebar/threads_block.rs`; not product evidence |
| `cargo fmt --all` | 0; format only. First subsequent diff inspection also found the unplanned Cargo.lock delta below |
| `cargo fmt --all --offline` | 2; cargo-fmt rejects that option. No validation success claimed for this invocation |
| `CARGO_NET_OFFLINE=true cargo fmt --all` | 0; corrected offline environment syntax, format only |
| `git diff --check` after final Rust formatting | 0 |
| Static inventory | 22 definitions (12 UI + 6 controller + 4 projection); no runtime counts |

Local Vega execution/build/check/Clippy/test/nextest, Python tests, CUA, external applications, independent review and all Actions checks: **not run / not obtained**. Existing Actions, after parent review/integration, remain the only runtime validation route.

## Lockfile delta — parent-reconciled test-support wiring

Cargo.lock was clean at writer intake. The writer first observed the 118-line delta after formatting and correctly left its origin for parent reconciliation. The parent had separately run dependency-only commands while the writer owned Rust sources: `cargo metadata --offline --locked --format-version 1` first failed because the lock needed updating; `cargo generate-lockfile --offline` then failed on the already locked, yanked `zip = 8.6.0` without changing the lock; incremental `cargo metadata --offline --format-version 1` preserved that existing lock record and succeeded. Final `cargo metadata --offline --locked --format-version 1` succeeded with 924 resolved packages. None of these commands compiled or ran Vega/tests.

The parent independently compared package identities and resolved dependency edges: ten new transitive packages are required by the explicitly approved temporary GPUI `test-support` feature; no existing package identity, version, source or checksum changed, and no edges were removed. Existing-package edge additions are only `gpui-pre` → `backtrace` and `proptest`; other printed dependency differences merely disambiguate same-name versions. This is temporary test wiring, not a production dependency upgrade or valid RED, and its exclusive records must be removed with the features after GREEN.

- Current Cargo.lock SHA-256: `d7c701e424bae4802ca725b937899e3875d0d0293b7d3be5558ff707849ab888`.
- Base Cargo.lock SHA-256: `71388bbb23bdeddcf884a02731f1190f22f32c889521ce431f933a834d12ce9d`.
- Lock-only `git diff --binary -- Cargo.lock` SHA-256: `902a7f987cecd82377d05e2fc35f89fcec5e3e2506574ce0a372a1c06f86ce0e`.

## Content checkpoint and next gate

- Tracked full `git diff --binary` SHA-256 (including reconciled lock delta): `f76826b3c03989f7563c8ba4172310a94c7e97b8f7f94096c7d17cb854d99252`.
- Tracked authorized diff `git diff --binary -- . ':!Cargo.lock'` SHA-256: `8a1a3fa1703f825960fdc345e9dfbb7debb54bf2f21e1a297bf8905d3ddc93df`.
- New observation module SHA-256: `99b8426f633c16bdea6022ddeb20e56555447d9d92fa42957ec047400ede1c47`.
- Evidence file and other untracked parent docs hashes are reported separately in the final READY report; tracked diff hashes do not include untracked files. This document does not self-hash.
- All inherited/new tests and exclusive fixtures/features/dev dependencies remain for independent pre-RED review and Actions; no product implementation or test deletion is authorized before valid RED/GREEN stages respectively. No old suite was restored, no assertions weakened/ignored, and no competitor research repeated.
- Next: independent review of the stopped precise content version, then existing draft-PR Actions. The parent reconciled the lock-only delta as above. Minimum-interface errors and observer controls must be distinguished from behavior RED. No product ambiguity was resolved by redesigning the frozen spec.
- Writer cessation is declared in the READY report after final static inventory/hashes. Parent owns Git/index/commit/push, external documentation, independent review, and Actions.
