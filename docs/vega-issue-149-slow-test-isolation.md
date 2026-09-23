# Issue 149 — Continue isolating expensive test dependencies

## User goal and scope

The user explicitly asked to continue after the Issue140 pilot and to start with the most expensive tests. Source baseline da4cc48 / cloud run35723796223 has127 tests above2s (851.999s cumulative, not wall time), including70 trusted_git tests (543.863s). The previous9 scenarios are a completed pilot, not completion of this initiative. Retain the existing full suite and its security assertions.

Prioritize by avoidable external cost after examining assertions, not by duration alone. The28.817s empty-blob matrix and24.254s normalization/no-op matrix include actual Git normalization/semantics: audit their split boundary, preserve necessary adapter coverage, and do not fabricate equivalent real-Git evidence. The next implementation batch is the11 post-commit proof faults (roughly80s cumulative) plus the19.314s commit-message bounds matrix where its external execution can be substituted. Use the existing per-instance GitCommandBackend. No new dependencies, public interfaces, generic Git emulator, production policy/timeout changes, shard changes, ignored tests, retries or weakened assertions.

## Acceptance matrix, frozen before implementation

| ID | Scenario/action | Required observable result | Layer / evidence |
|---|---|---|---|
| B1 | Production refresh/checklist/prepare/commit with zero/wrong/two parents or tree drift after one mutation | Original ChangedDuringRead outcome, terminal workspace, exact single commit argv, duplicate stale rejection without a second mutation | In-process external-command substitution; all11 existing test identities retained |
| B2 | Same path with malformed/short/mixed parent, missing object, moved/deleted/renamed ref | Original MalformedOutput/GitFailed/ChangedDuringRead mapping as applicable; same capability consumption and single mutation assertions | Same boundary; every original assertion mapped |
| B3 | Existing message bounds scenarios, multibyte byte-length edges and exact stdin | Original invalid-message outcomes, zero mutation for invalid input, accepted boundary and exact bytes for valid input | In-process policy with real argument construction; retain representative real stdin integration |
| B4 | Actual born/unborn successful commit proof, immutable OID reads, process adapter | Existing actual Git parent/tree/argv/stdin and process lifecycle contracts pass | Real adapter integration retained |
| B5 | Disable a substantive proof or message validation temporarily | Corresponding migrated test fails; restore exact source before final validation | Negative control, preserve raw failure |
| B6 | Migrated policy tests run with external executable launches prohibited | All migrated cases pass; real Git control fails under the prohibition and passes normally | System-enforced no-exec verification; no .git repository creation |
| B7 | Same-machine before/after, targeted regressions and final complete cloud gate | Log command/source identity/counts/durations; full fmt/clippy/tests/docs pass without skips/retries added | Performance and regression |
| B8 | Remaining127 slow tests have traceable dispositions | Candidate groups, real-contract retention reasons, and remaining bottlenecks recorded; this batch does not close the wider initiative with unexamined items | Audit / follow-up tracking |

## Implementation plan

1. Dedicated implementation agent records baseline and assertion mapping, then narrowly extends the existing test-only command fixtures to describe pre/post mutation raw outputs and one captured command/input. Business policy and prepared capabilities are always produced by the real service.
2. Migrate11 proof-fault tests and the message matrix. Retain actual born/unborn proof and actual stdin checks. Unknown requests fail closed and are asserted, so injected failure cannot hide accidental mutations or incorrect reads.
3. Main agent reviews fixtures and outcome mapping; verify negative controls/no-exec contract and targeted real integrations. Preserve diagnostic failures rather than rerun-to-green.
4. Run final full cloud gate and measure observed throughput with queue/cache variance stated. Integrate only verified code. Record remaining work under Issue149 rather than imply all external dependencies are eliminated.

Scope is test architecture; no runtime data/config migrations or application UI installation. Rollback is the resulting PR commit. Persistent evidence stays outside removable worktrees.

## Highest-duration matrices — approved split before implementation

The code-level audit finds a useful split for the two largest cases; they are not exempt from optimization merely because some assertions need actual Git.

| ID | Existing matrix | Business portion to migrate | Real contract to retain |
|---|---|---|---|
| H1 | empty_blob_add_worktree_delete_and_staged_empty_delete_remain_distinct (28.817s, four scenarios) | Empty-add/deletion classification, staged-empty plus optional deletion, selection/nonselection policy and successful capability/commit path consume captured raw states through the actual service | A minimal actual Git adapter contract proves empty blob differs from missing entry and selection's add/delete changes actual index as expected. Verify recorded raw status/stages/tree for these inputs; no emulated Git algorithm |
| H2 | clean_and_normalized_noop_are_no_staged_changes_without_commit (24.254s, four branches) | Clean/ignored-mode NoStagedChanges, normalization no-op result handling and outside-selection drift rejection use actual service with finite input transitions; preserve no capability, exact zero/one mutation and authoritative terminal state assertions | Actual core.filemode/eol normalization/raw-output semantics remain tested in a focused real Git contract. Do not label the substituted service matrix as real E2E |

All original distinctions must map to explicit assertions at the appropriate layer. Reuse the command boundary and finite fixture protocol from B1-B3; one owner controls shared command_stub changes. Prefer bounded capture/action transitions, no arbitrary nth-read counters. H1/H2 receive same before/after, no-exec, negative-control and full-gate acceptance as B1-B7. Record any irreducible real-process portion and its measured cost. Dedicated implementation owners may work in separate worktrees but shared backend changes integrate sequentially.

## Batch 5 — workspace lifecycle generation/race

Three `git_workspace::tests::lifecycle` race tests were migrated to the finite
in-process snapshot-read boundary (`lifecycle_stub.rs` + captured
`lifecycle-fixtures.json`). The real `GitWorkspaceService::refresh`,
`begin_owned_refresh` and `refresh_owned_after_mutation` still run their entire
read protocol; only the external `git` process is replaced by captured bytes and
an explicit completion channel. The shell `mkdir`/`sleep` gates and their Git
repositories are gone.

| Test | Original assertions preserved | Retained real contract |
|---|---|---|
| `git_workspace_latest_refresh_wins_without_stale_overwrite` | superseded refresh returns `StaleGeneration`; the latest snapshot stays diffable | `lifecycle_captured_states_match_real_git` |
| `git_workspace_owner_finalize_fences_pre_registered_poll_completion` | pre-registered poll returns `StaleGeneration`; `state.generation` equals the owner terminal generation | same |
| `git_workspace_obsolete_failure_does_not_invalidate_newer_snapshot` | obsolete failed capture returns `StaleGeneration`; the newer snapshot stays diffable | same |

Same-machine same-scope: three migrated tests **0.032s** wall (was 3.290s
cumulative); retained real adapter `lifecycle_captured_states_match_real_git`
**0.378s**. Full `git_workspace` suite 181/181 in 30.219s. 3 negative controls
exit 100 with byte-identical restore. No-exec: the 3 migrated tests pass under
`deny process-exec`; the real adapter is denied (exit 101) and passes normally.

Kept real: `git_workspace_read_timeout_is_typed_and_bounded` (10.36s real
process timeout + descendant reap), `git_workspace_early_parent_exit_…`,
`git_workspace_cancel_is_typed_and_reaps_fixture_group`,
`git_workspace_ctime_detects_equal_size_edit_with_restored_mtime` (real FS ctime)
and `git_workspace_metadata_remaining_cap_is_inclusive_and_plus_one_fails`.

## Disposition record

[Per-test audit](vega-issue-149-slow-test-audit.md) records the baseline observations, raw duration totals and remaining groups. These are measured costs, not claimed savings. The highest two matrices are now approved for a split under H1/H2, superseding the initial audit's recommendation to retain them whole. The audit records observation-time status; delivery results will explicitly identify which rows have migrated and which remain outstanding.

### Migrated rows (verified, on `feat/test-dependency-isolation-next`)

Batches 1–5, all with negative controls, no-exec verification and retained real
adapter contracts:

- commit proof faults + message boundaries (`0adbefc`)
- empty-blob and no-op normalization matrices (`b1a5d9c`)
- head-oid / failed-draft / disconnected-recovery policy (`762a99d`)
- service mutation-outcome error mapping and authoritative recovery, 21 tests (`87d5d75`)
- explicit-filter, `.gitattributes` and attrs-drift policy, 3 tests (`e8549b9`)
- workspace lifecycle generation/race, 3 tests (`1385a63`)
- branch lease/cleanup race policy, 2 tests (`956f7ea`)
- branch snapshot-id/generation policy, 4 tests (`668baf0`)
- branch state-guard policy, 3 tests (`962567a`)
- branch switch-race policy, 2 tests (`4b017d4`)
- trusted_git owner-refresh first-capture retry, 2 tests (`f9d7d89`)

## Batch 6 — `vega_runtime` pixel-budget header test

`images::tests::issue63_pixel_budget_rejects_valid_overbudget_header_before_decode`
no longer materializes and re-encodes 16M pixels. It consumes a pre-generated,
complete 4001x4000 PNG fixture (`crates/vega_runtime/src/overbudget_4001x4000.png`,
15 629 bytes, generated by the evidence script `runtime-images/gen_fixture.py`),
decodes it once to prove it is a real 4001x4000 image, then asserts the real
production pixel budget rejects it. Same test, same production entry point, same
assertion. Same-machine: **1.152s → 0.011s**. Negative control (relax the
16_000_000 budget to 16_004_000) makes the owning test fail with
`left: Ok(ImageAttachment { width: 4001, height: 4000 })`; source restored
byte-identically. Full `vega_runtime` suite 203/203 in 0.569s.

## Batch 7 — commit-draft request literals

`summary_draft::commit_draft_request_matches_frozen_literals_for_both_truncation_flags`
now derives the prepared capability from the in-process captured staged state
(`PolicyFixture::prepared`) instead of a real repository plus a mutation-recorder
script. `summary`/`summary_truncated` remain request inputs set directly, never
asserted results; every frozen literal assertion (model, empty tools, 256
max_tokens, both messages, full system/user text, exactly one provider request)
is unchanged. Same-machine: **3.372s → 0.024s**. Negative control (change the
production `USER_PREFIX` literal) fails the owning test; source restored
byte-identically. No-exec: the migrated test passes under `deny process-exec`;
the retained real summary-authority adapter is denied and passes normally.

## Batch 8 — branch lease/cleanup race policy

The two `git_workspace::branch::tests::lease_cleanup` race tests now run the real
`BranchWorkspaceService` over the finite in-process command boundary
(`branch/tests/branch_stub.rs` + captured `branch/tests/branch-fixtures.json`).
The service still performs its complete capture protocol (`rev-parse`,
operation-marker checks, `for-each-ref`, filter identity, `status`), the
target-tree authority diff and the trusted `switch`; only the external `git`
process is replaced by captured bytes plus explicit completion gates. The shell
`mkdir`/`sleep` wrappers (`blocking-switch.sh`, `read-wrapper.sh`) and their Git
repositories are gone. `BranchWorkspaceService` gains the same `#[cfg(test)]
command_backend` field the workspace service already has, within the
`git_workspace` module boundary; no public interface changed.

| Test | Original assertions preserved | Retained real contract |
|---|---|---|
| `rejected_execute_cannot_compete_with_owner_cleanup_refresh` | owner-exclusive refresh returns `StaleGeneration`; generation unchanged; rejected and third executes fail with no snapshot while `active_mutation` stays set; exactly one switch attempt; owner ends `Switched` on `topic-current` | `branch_captured_states_match_real_git` |
| `refresh_registered_before_owner_cannot_commit_after_lease_acquisition` | late refresh returns `StaleGeneration`; generation and `main`-current snapshot unchanged; owner ends `Switched` on `topic-current`; exactly one switch attempt | same |

Same-machine same-scope: two migrated tests **0.011s / 0.010s** (was 1.377s /
1.391s isolated); retained real adapter `branch_captured_states_match_real_git`
**0.157s**. Full `branch` suite 30/30 in 2.215s. 3 negative controls exit 100 with
byte-identical restore (`branch_sha256 e56e732a…`). No-exec: both migrated tests
pass under `deny process-exec`; the real adapter is denied (exit 101) and passes
normally.

## Batch 9 — branch snapshot-id/generation policy

The four `git_workspace::branch::tests::snapshot_ids` policy tests now run the real
`BranchWorkspaceService` over the same finite in-process command boundary. The
capture protocol, generation rotation, opaque-id sealing, permit consumption and
the raw-ref current selection are unchanged; only the external `git` process is
replaced by captured bytes from `branch-fixtures.json` (extended with
`main-another`, `main-temporary`, `main-topic-forced`, `topic-forced-current` and
the alias shapes). No shell gate, no repository.

| Test | Original assertions preserved |
|---|---|
| `unchanged_refresh_keeps_ids_and_branch_change_rotates` | identical refresh keeps generation/ids; a new ref rotates both |
| `opaque_ids_are_service_generation_slot_and_seal_bound` | current-branch rejection; forged slot/seal rejected; cross-service permit rejected; A->B->A rotates generation and kills the old id |
| `stale_permit_after_generation_rotation_does_not_leak_mutation_lease` | rotated permit fails `StaleGeneration` with no lease leaked; a fresh permit then switches |
| `shared_oid_refs_are_distinct_and_current_is_selected_by_raw_ref` | three distinct labels at one commit, exactly one current (`main`) |

Same-machine same-scope: four migrated tests **0.011–0.032s** (isolated baseline
0.269s / 0.857s / 1.309s / 0.435s). The retained real adapter
`branch_captured_states_match_real_git` now also validates the forced-target and
post-switch states. 3 negative controls exit 100 with byte-identical restore
(`parsing_sha256 1bc0b78b…`, `branch_sha256 e56e732a…`). No-exec: all four
migrated tests pass under `deny process-exec`; the real adapter is denied
(exit 101) and passes normally.

## Batch 10 — branch state-guard policy

The three raw-status `git_workspace::branch::tests::state_guards` policy tests now
run the real `BranchWorkspaceService` over the finite in-process command boundary
with captured dirty/detached/staged/untracked/unmerged status bytes. The
operation markers remain real filesystem facts written under the stub's plain
`metadata` directory, so the production marker traversal is still exercised. The
symlink and linked-worktree `--git-path` nofollow contract is retained as a real
Git/filesystem test.

| Test | Original assertions preserved |
|---|---|
| `dirty_detached_and_operation_state_fail_closed` | dirty tracked -> `BranchDirty`; detached -> `BranchDetached`; `MERGE_HEAD` -> `BranchOperationInProgress` |
| `staged_and_untracked_states_are_dirty_and_every_marker_is_rejected` | staged and untracked -> `BranchDirty`; every `OPERATION_MARKERS` entry rejected |
| `unmerged_index_is_dirty_and_never_enumerated_as_switchable` | unmerged index -> `BranchDirty` |

Same-machine same-scope: three migrated tests **0.011–0.033s** (isolated baseline
0.435s / 0.830s / 0.341s). 3 negative controls exit 100 with byte-identical
restore (`parsing_sha256 1bc0b78b…`). No-exec: all three migrated tests pass under
`deny process-exec`; the real adapter is denied (exit 101) and passes normally.

## Batch 11 — branch switch-race policy

The two zero-switch `git_workspace::branch::tests::switch_e2e` tests now run the
real `BranchWorkspaceService` over the finite in-process boundary. The permit
rotation, the pre-mutation byte-identity fence and the fail-closed dirty/operation
gates are unchanged; only the external `git` process is replaced by captured
bytes. `safe_temp_repo_switch_is_exact_and_authoritatively_refreshed`,
`ignored_collision_…`, `target_gitattributes_…` and `deleted_and_renamed_away_…`
remain real Git switch contracts.

| Test | Original assertions preserved |
|---|---|
| `newer_permit_invalidates_older_and_target_move_fails_before_switch` | older permit rejected `StaleGeneration`; newer permit `Switched` (one attempt); a target ref move after the permit fails `ChangedDuringRead` with a snapshot |
| `dirty_and_operation_races_are_zero_switch_with_owner_cleanup` | dirty worktree and a real operation marker both fail closed with zero switch attempts |

Same-machine same-scope: two migrated tests **0.038s / 0.039s** (was 1.620s /
1.100s). 3 negative controls exit 100 with byte-identical restore
(`branch_sha256 e56e732a…`, `parsing_sha256 1bc0b78b…`). No-exec: both migrated
tests pass under `deny process-exec`; the real adapter is denied (exit 101) and
passes normally.

## Batch 12 — trusted_git owner-refresh first-capture retry

The two `git_workspace::trusted_git::tests::selection_topology` owner-recovery
retry tests now run the real `TrustedGitService` `prepare`/`commit` protocol and
the real `GitWorkspaceService::refresh_owned_after_mutation` retry loop over the
finite in-process command boundary (`trusted_git/tests/command_stub.rs`). Only
the external `git` process is replaced by captured bytes; the real owner/generation
linearization, the `ChangedDuringRead`/`StaleGeneration` short-circuits and the
authoritative terminal publication are unchanged. The real `capture_head` proof
and the real `commit` mutation argv/stdin remain asserted.

The transient fault is modelled **structurally**, never by an nth-read counter:
the test arms one fault, serving the declared mutation converts it to
`status_fault_pending`, the capture-opening `ls-files -z --cached --deduplicate`
read of `build_snapshot` latches it, and that capture's first `status` read
reports `GitFailed` once. Because the ordinary `capture_head`/authority reads do
not read filter identity, the fault cannot be consumed by an earlier proof read.
Each test asserts `faults_served == 1`, so a silently unarmed fault cannot let the
retry assertion pass without exercising the retry.

| Test | Original assertions preserved | Retained real contract |
|---|---|---|
| `owner_refresh_prepare_first_capture_failure_retries_exact_owner` | one transient capture failure; `error.is_none()`; authoritative terminal workspace; terminal generation advanced past the checklist generation; a real prepared capability; exactly one `add` mutation with exact argv/stdin | real E2E `e2e_owned_repo_checklist_prepare_mock_draft_commit` |
| `owner_refresh_commit_first_capture_failure_recovers_new_head_once` | one transient post-commit capture failure; `Committed` with a terminal workspace; captured head changed; exactly one `commit` mutation with exact argv/stdin | same |

Same-machine same-scope: two migrated tests **0.041s / 0.040s** (was 2.394s /
3.065s first run and 2.709s / 3.020s second run). 3 negative controls exit 100
with byte-identical restore (`service_sha256 60604e02…`, `workspace_sha256
d49140a0…`). No-exec: both migrated tests pass under `deny process-exec`; the
retained real E2E is denied (exit 101) and passes normally. The now-dead shell
`fail_first_status_after_trigger` read-fault helper was removed.

## Batch 13 — trusted_git owner/generation completion ordering

The three remaining `trusted_git` owner/generation completion-ordering tests now
run the real `TrustedGitService` `prepare` protocol and the real
`GitWorkspaceService::refresh`/`refresh_owned_after_mutation` linearization over
the finite in-process command boundary (`trusted_git/tests/command_stub.rs`).
Only the external `git` process is replaced by captured bytes.

The shell `sleep` gates (`blocking_mutation`, `blocking_summary_reader` plus
`wait_for_path`/`release` markers) and their real repositories are gone. They are
replaced by one deterministic in-process completion barrier, `arm_gate`:
`GateTarget::Mutation` holds the declared mutation *after* its captured
post-state is applied, and `GateTarget::Summary` holds the commit-summary
`diff --cached --patch` read after its captured bytes are resolved. The gated
command signals `entered` and then waits for an explicit `release`, so the test
orders a concurrent ordinary poll (or an authoritative index drift) at exactly
the intended protocol point with no filesystem polling or timing assumption. The
barrier is consumed once; a later operation is never reordered.

| Test | Original assertions preserved | Retained real contract |
|---|---|---|
| `owned_prepare_accepts_exact_b_published_by_ordinary_poll` | ordinary poll publishes the exact B while the owner is held; `prepared.is_some()`; the completion workspace generation equals the concurrently observed B; a later poll keeps that generation; exactly one `add` with exact argv/stdin | real E2E `e2e_owned_repo_checklist_prepare_mock_draft_commit` |
| `owned_prepare_rejects_a_to_b_to_a_without_capability` | content driven A -> B -> A while the owner is held; `prepared.is_none()`; `error == ChangedDuringRead`; an authoritative workspace is still carried | same |
| `summary_authority_change_after_capture_fails_before_provider` | authoritative index drift (a *different* captured state, so the workspace generation is provably untouched) lands during the summary read; `error == ChangedDuringRead`; `prepared.is_none()`; `workspace.is_some()`; zero provider requests | real summary authority `commit_status_drift_real_git_consumes_prepared_and_spawns_zero_commit` |

Same-machine same-scope (nextest, `--test-threads=1`, original sources restored
for the baseline): `owned_prepare_accepts_exact_b_published_by_ordinary_poll`
**2.523s → 0.030s**, `owned_prepare_rejects_a_to_b_to_a_without_capability`
**2.109s → 0.031s**, `summary_authority_change_after_capture_fails_before_provider`
**1.919s → 0.032s**.

Negative controls exit 100 with byte-identical restore (`mod.rs d49140a0…`,
`service.rs 60604e02…`, `parsing.rs b0417f1d…`): tightening the exact-B bound to
`revision_delta == 0` fails the acceptance test; removing the layered ABA guards
fails the A→B→A test; dropping the post-summary `require_exact_authority` fails
the summary-drift test. No-exec: all three migrated tests pass under
`deny process-exec`; the retained real adapter contracts are denied (exit 101)
and pass normally. The now-dead shell gate helpers (`blocking_mutation`,
`blocking_summary_reader`, `wait_for_path`) were removed.

### Outstanding rows

The remaining 127-item optimization is **not** complete. Still outstanding:
`filter_gitlink::real_gitlink_…`, `commit_proof` new-OID/root-inode contracts,
`codec_topology::sha256_…`, the artifact `preview_open` group, the remaining
`provider_settings::production_cancel_and_total_deadline`, UI controllers/layout
and agent concurrency, `vega_markdown` ten-thousand-line document, and
`s6_acceptance::agent_diff_artifact_dirty_reject_and_two_stage_commit`.
