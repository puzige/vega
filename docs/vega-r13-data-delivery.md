# R13-D — Sidebar organization data delivery

## Freeze

- Contract: `docs/vega-r13-sidebar-organization.md`, data/service ownership only.
- Branch: `codex/r13-organization-data`.
- Verified at UTC: 2026-09-06 06:18:13–06:18:47; local: 2026-09-06 14:18:13–14:18:47 Asia/Shanghai.
- Existing source HEAD during final validation: `f1c7be687b0d42d6f20dc6c3e1d3bed510348f07` (second implementation commit).
- Final tracked source diff SHA256 (`git diff HEAD --binary -- crates`, including staged new tests): `055283e81d605957ff5ebb4ab8c7649d39d2f240c419d747a6947ee944264b96`.
- Store/service/calendar validation preceding the last authorized S7 schema-test-only adjustment used source diff SHA256 `993e68ac8a22ec1cc9dee6ae03da7d44bb5224c82c5a46889d9ad83f0d81912d`. S7 acceptance, final fmt and strict clippy verified the final source hash above.
- Platform: Darwin arm64; rustc 1.98.0 (88d9e12ae, 2026-08-18), cargo 1.98.0 (797e8a9bc, 2026-08-05), git 2.55.0.
- `git fetch` and `git rebase origin/master` completed before repository edits. No push or master merge performed.

## Delivered behavior

Migration 0004 adds four metadata tables while preserving every existing project, task and message column. Membership foreign keys naturally remove associations when tasks are deleted; archive/restore keeps the association; dissolving groups deletes only the group and its associations.

`vega_conversation::sidebar_organization::snapshot(&Store)` opens one read-only transaction, counts before loading, and returns all registered projects, active/archived task metadata, ordered groups/memberships, explicit project order, preferences, collapse targets and revision. It never loads message content or touches task timestamps/unread. Bounds are 10000 total tasks and 128 groups; exceeding either returns an explicit limit error without truncation.

`apply(&Store, expected_revision, action)` uses one immediate transaction for fresh reads, validation, mutation and revision advancement. Conflicts, missing targets, invalid names, self-moves and wrong-destination anchors are distinguishable failures with no partial write. `before_id: None` appends. Ungrouping requires `before_id: None`; ungrouped tasks retain timestamp projection order. Projects absent from explicit manual order append in stable recent-open order. Group names trim to 1–64 Unicode characters and colors use seven shared semantic values.

The shared `SidebarOrganizationOutcome { snapshot, created_thread }` returns the exact newly created task directly from the production create operation, avoiding identity guesses when another connection creates unrelated tasks. Create plus membership is atomic. All other actions return `created_thread: None`. Snapshot reads remain separate from navigation, metadata mutation fences and provider activity; callers must reopen file-backed Store and invoke service functions on their background executor.

`local_calendar_bucket(timestamp_ms, now_ms)` uses system-local civil dates, including DST, year/leap boundaries and future timestamps. Today includes future dates; Yesterday is one local date ago; Last7Days covers days 2–6; Last30Days days 7–29; Earlier starts at day 30.

## Results

Raw logs are retained as `/private/tmp/vega-r13-data-<name>.log` (equivalently `/tmp` on this host). Only bounded footers and SHA256 are recorded here.

| Requirement | Evidence class | Exact command | Result / duration | Bounded footer / raw log SHA256 |
|---|---|---|---|---|
| store-final | INTEGRATION / existing store regressions | `cargo test -p vega_store` | PASS / 1.46s | `93 passed; 0 failed; 0 ignored`; `a991333ea3e721e6ed848edc6b8a00160843de81e09c41e741898ec0c2e3369c` |
| e2e-final | 7 E2E-REAL + 1 FAULT-INJECTION + 3 UNIT | `cargo test -p vega_conversation --lib sidebar_organization -- --nocapture` | PASS / 4.54s | `11 passed; 0 failed; 0 ignored; 277 filtered out`; `111220138cb31b6a9583b5471117c2feeb63cc424bbe92fd85d18c8e7018b67f` |
| calendar-new-york | UNIT / system calendar | `TZ=America/New_York cargo test -p vega_conversation --lib sidebar_organization::calendar -- --nocapture` | PASS / 0.22s | `3 passed; 0 failed; 0 ignored`; `ba16516017859d5fade6cbe260137d5c20dade65ba5207259c047f2dc73bdb9c` |
| calendar-utc | UNIT / system calendar | `TZ=UTC cargo test -p vega_conversation --lib sidebar_organization::calendar -- --nocapture` | PASS / 0.2s | `3 passed; 0 failed; 0 ignored`; `2267d59f72db15701986354e484e18ffd3584ae64104b2fd402a24b5908d25df` |
| s7-acceptance | E2E-REAL / synthetic provider boundary | `cargo test -p vega_conversation --test s7_acceptance_e2e` | PASS / 2.46s | `1 passed; 0 failed; 0 ignored`; `1d4f563083e66ef283099531aba707ab8fcc05e94634b31699dba41a40e430f3` |
| fmt-delivery | STATIC | `cargo fmt --all -- --check` | PASS / 0.97s | `exit 0; empty output`; `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| clippy-delivery | STATIC / strict all targets | `cargo clippy -p vega_store -p vega_conversation --all-targets -- -D warnings` | PASS / 0.38s | `Finished dev profile; exit 0`; `0027b28219e11368f3492a670d60c2b863dac564b13dcdfd537a5ebddc4f99ad` |

The owned file service journeys prove legacy schema migration/idempotence and preserved content; seven colors and Unicode bounds; cross-project membership and move-before ordering; project/group ordering; all projection collapse modes; restart persistence; group dissolution preserving tasks/messages; archive/reopen/restore; task-deletion cascade; exact creation identity after an unrelated external task creation; and two real SQLite connections competing with one expected revision (exactly one commit and one Conflict). Invalid actions compare the complete before/after snapshot. Bounds check both admission and over-limit read rejection without deleting persisted rows.

The one fault injection uses an owned SQLite abort trigger on membership insertion, while calling the real service. It proves a failed create-plus-membership transaction leaves no new task or revision/metadata change; removing the trigger permits the same production operation to succeed. This is rollback evidence, not a native UI or real provider acceptance claim.

Calendar tests ran in separate processes with `TZ=America/New_York` and `TZ=UTC`. New York explicitly asserted the spring day is 23 hours and autumn day is 25 hours while both remain Yesterday across local midnight. No process-global timezone mutation or dependency was introduced.

## First failures retained

| Exact command / attempt | First observed failure | Resolution | Raw log SHA256 |
|---|---|---|---|
| `cargo check -p vega_conversation` / contract first | 4 compile errors: rusqlite does not implement SQL conversion for usize/u64 | Store primitives use signed i64; service converts checked revision to u64 | `70dcc18b788f484a35a28de24e69ac5c61fb84e221a35063cb53286ae98f3c7b` (`contract-check`) |
| `cargo clippy -p vega_store -p vega_conversation --all-targets -- -D warnings` / first | 5 test-only cloned_ref_to_slice_refs diagnostics | Use `std::slice::from_ref`; same equality assertions retained | `287b3e5d79ceeb26ca083faf568f32cd95fc454e1170e4aa07a1ee9ae4be4d9e` (`clippy-first`) |
| Same strict clippy / second | Newly added DST helper needed parentheses around unsafe block before multiplication | Parenthesized the expression; no behavior/assertion change | `42b4cc5ece62d4a237c7b95492e2baa749b017b0a9c45d24fbdb505701153b94` (`clippy-second`) |
| `cargo test -p vega_store` / first | 92 passed, 1 failed: old permissions schema test expected version 3 while actual is 4 | Root authorized exact version/table-count update for migration 0004; all permission assertions remain | `a18abcabc05d3611a79e03e8277cb07026f1df7557b313fe4151e354a31b5af4` (`store`) |

Root additionally identified the exact old schema assertion in S7 acceptance before running it in this worktree. Only version and complete table-list expectations were updated to the approved migration; every pricing, rollback and content assertion remains. The affected E2E passed on its first run here. No failed assertion or timeout was weakened.

## Residuals

- NOT RUN here: full workspace tests/build, native Sidebar/Window interactions and combined application acceptance; root/UI executors own these gates.
- NOT RUN: real provider/network calls, Keychain/real credential access, Git runner changes, performance bench/soak. The pre-existing Git timing residual is outside this card and is not claimed fixed.
- LIMIT: service functions perform synchronous SQLite work and must be scheduled in the UI's background executor. This data card does not certify UI generation/fence behavior.
- LIMIT: organization revision tracks organization writes, not legacy task metadata/navigation writes. Snapshot reads are transactionally consistent and mutation results never write back task snapshots.
- ACCEPTED: manual moves preserve unaffected relative order; bounded metadata persistence rewrites position records inside the same transaction. Performance optimization remains deferred by the task contract.
- Spec deviation: none. Exact created-task identity and historical schema-test updates were root-authorized consistency corrections within R13.

## Root-authorized schema assertion follow-up

Root's first combined workspace run retained **962 passed / 2 failed / 0 ignored**. Both failures reached the final complete table-set assertion in `agent/tests/stream_persistence.rs` and `tests/todo_e2e.rs`: actual schema contained the ten migration-0004 tables, while those two historical expected sets still listed six. The raw first failure remains at `/private/tmp/vega-r13-final-workspace-tests.log`, SHA256 `43c94bbfdaa5c4985e736a673e52b46de47b756c6bfa87f7eaf4e9bae09f222d`; its hash was reverified in this follow-up. This failed full run is not relabeled as passed by the focused reruns below.

Root explicitly authorized one additional narrow test-migration commit beyond the original three implementation commits. The only code diff adds `sidebar_groups`, `sidebar_memberships`, `sidebar_organization`, and `sidebar_project_order` to each complete expected set. No production code, message/tool/cost/provider assertion, timeout, or set equality assertion changed. A repository-wide Rust scan of `sqlite_master`, `sqlite_schema`, `PRAGMA user_version`, table-count assertions, and historical six-table/version wording confirmed these were the only two remaining old complete sets; existing version-1 fixtures remain unchanged because they intentionally test migration.

Follow-up freeze: branch `codex/r13-organization-data`; HEAD before the follow-up commit `d813d5e95bb0bf85d0ad36830931b6672b64d496`; tracked source diff SHA256 (`git diff --binary -- crates`) `b4cc4a77fd7f07614b5bb4752785fff0fa729c3a547fa4ba5101130d94fbe8dc`; verified 2026-09-06 06:52:17–06:52:23 UTC / 14:52:17–14:52:23 Asia/Shanghai. Platform/toolchain unchanged from the original freeze. Fetch/rebase completed before this follow-up edit.

| Requirement | Evidence class | Exact command | Result / duration | Raw log SHA256 |
|---|---|---|---|---|
| stream | E2E-REAL / owned production chain with MockProvider | `cargo test -p vega_conversation --lib agent::tests::stream_persistence::persists_messages_tool_lifecycle_and_zero_cost_usage -- --exact` | `1 passed; 0 failed; 0 ignored` / 3.92s | `24066686afb9b844bcbd7370f8a57687a9ad2bd48f32f60187ccb70346e6bb1a` |
| todo | E2E-REAL / owned production chain with MockProvider | `cargo test -p vega_conversation --test todo_e2e finds_every_seeded_todo_with_real_tools_and_persists_the_run -- --exact` | `1 passed; 0 failed; 0 ignored` / 0.98s | `364085b75aae22dda9bd7d79baa9d16b693dde5106e9808454e0a00abd8d9cce` |
| fmt | STATIC | `cargo fmt --all -- --check` | `exit 0; empty output` / 1.0s | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |

Raw follow-up logs are `/private/tmp/vega-r13-schema-followup-{stream,todo,fmt}.log`. Each selected test ran once and passed; no broader tests or clippy were rerun by this executor. The normal pre-commit hook also checks formatting. Full workspace post-fix acceptance remains with root, and testing stops here as requested.
