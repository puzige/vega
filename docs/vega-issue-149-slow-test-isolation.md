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

## Disposition record

[Per-test audit](vega-issue-149-slow-test-audit.md) records the baseline observations, raw duration totals and remaining groups. These are measured costs, not claimed savings. The highest two matrices are now approved for a split under H1/H2, superseding the initial audit's recommendation to retain them whole. The audit records observation-time status; delivery results will explicitly identify which rows have migrated and which remain outstanding.
