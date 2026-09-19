# Issue 65 review ledger

Status: source review, full gates and native acceptance passed; local integration pending.
Date: 2026-09-19 (Asia/Shanghai).
Contract: [R1–R6](vega-issue-65-auto-title.md).

## Checks requiring final evidence

1. The app's primary per-run current-thread runtime ends after its main request.
   A title worker must survive that teardown; test a delayed title after a fast
   main reply through the production app worker.
2. AgentUpdate polling stops on Finished/Stale. Title completion notification
   must use an independent lifetime and cannot navigate to its originating task.
3. Route cancellation must not cancel the independent naming task. Do not modify
   #67's primary-run behavior as part of this card.
4. A stale sidebar organization snapshot must not restore old titles after the
   new-title refresh; cover the existing mutation-epoch boundary.
5. Header title reads must run off the UI event/render thread; asynchronous
   replies must be fenced against subsequent manual edits and route changes.
6. Both manual rename and generic update(title) preserve manual intent even
   for same-text and ABA writes; generated updates change only title metadata.
7. First-turn fallback and durable claim roll back with the user transaction;
   restart/later turns do not retry a claimed request.
8. Verify original composer text only, 8 KiB local output bound, no transport
   retry, no fake usage or fake visible message, and no recency bump.

These are review requirements, not claims of observed runtime failures or
completed test coverage. Final review must link actual tests and live evidence.

## Independent source review

The independent app/UI reviewer found the original-text capture, independent
worker/notification lifetime, title-only ID-fenced projection, same-text manual
rename, and sidebar mutation fence wired in the current implementation.
This is source inspection, not live acceptance or a complete test result.

Resolved R6 correction: distinguish a failed background title read from an absent
(deleted) thread. A final notification must not be consumed as success on a
temporary database-read error; use bounded retry with content-free diagnostics.
Final independent source review verified the Result distinction, three bounded
read attempts, content-free diagnostics and epoch-conflict reread after sender
disconnect. The production-facade recovery test makes the real table temporarily
unreadable, restores it, and observes the recovered title. Native acceptance is
still pending; source review is not a substitute for that evidence.

## Intermediate test evidence

Main independently read the first focused log: nine tests passed (app worker 1,
conversation 3, HTTP transport 1, store 4), zero failed. Evidence file:
`/tmp/vega-issue65-focused.9EaFno/automatic-title-tests.log`, SHA-256
`541817ee90a72b383418a88f33ec6bc798ce1add2fd40f22e84ba36040adec3c`.
This predates later GPUI/recovery edits; it is not final-tree gate evidence.
The HTTP test confirms auxiliary retries are disabled without changing the
primary provider's retry policy. Store tests cover manual same-text/ABA through
both writers, durable claim rollback/reopen, legacy migration and no resurrection.

## First full-workspace gate

`test --workspace -- --test-threads=1` stopped in the application test binary:
140 passed, 15 failed; later workspace crates were not executed by this command.
Raw log: `/private/tmp/vega-issue65-acceptance.ap67AY/workspace-tests.log`.
Fourteen failures use pre-existing single-stream provider fixtures, which let
the new auxiliary call consume/record primary rounds. The remaining r69 failure
expected an empty title after first acceptance, contrary to R1's fallback.
The task spec explicitly authorizes isolated test-provider fixture adaptation
and that one changed title expectation; primary behavior assertions stay exact.
Neither the original failures nor the required full rerun may be omitted.

## Fixture correction and final source review

The dedicated executor's complete application suite passed 156/156 after the
spec-authorized fixture adaptation. Main independently verified the source
manifest SHA-256: `d9edf3bab9badb34917e8798880f69c7ff7238adfadeb76a78ab542b25833677`.
The independent reviewer checked all 13 injection sites: only auxiliary scripts
are separated, while primary requests and their existing assertions remain
unchanged. The single fallback expectation is now exactly `materialize me`.
Dedicated Issue 65 success/HTTP tests remain unwrapped; production still starts
the naming worker. No blocking source finding remains in this review scope.

The first failed workspace log is retained. The complete workspace rerun and
native E2E remain acceptance gates, not inferred successes from this review.

## Final acceptance — 2026-09-19 01:27 UTC / 09:27 Asia/Shanghai

The following final gates ran against the frozen source manifest above. Every
command used `scripts/cargo-lock.sh`; all final commands exited 0.

| Command suffix | Result | Raw log SHA-256 |
| --- | --- | --- |
| `test --workspace -- --test-threads=1` | 1,321 passed, 0 failed, 9 ignored | `7b96043092713c8c3c95bf2f3e19254df2633254a475dc5da627dbb999463ce2` |
| `test --workspace -- --ignored --test-threads=1` | 9 passed, 0 failed | `bfa55c57292ad00b2562b9598d670267a7e395cb870b37741b197056c1c9144d` |
| `fmt --all -- --check` | PASS | `93b3a2ca7f623f21943d3007915b1e4ac5d511f99d9db1b53dbabd30a6a86769` |
| `clippy --workspace --all-targets -- -D warnings` | PASS | `7729cbbc1bbc65d75be949eb909b4fb18335a7880d5c06121c4f548c509979fb` |
| `build --workspace --all-targets` | PASS | `088e398486baad31307b4fd953117f420df854d5aa71e4f640ecd487c31b58aa` |
| `tree -p vega_runtime --edges normal` | No GPUI/UI dependency | `1406b13cc48710c4b5970e0d825f91551af89ab26832528b766aad8c311581c7` |
| `run -p xtask -- package` | Signed package verified | `8bcf1d1f1b4aac97baf2abd4742d076cd3dc36b7610b7c657b55745260f353b0` |

Installed executable SHA-256:
`932f83d12245d5a49733a3674858b1edfca219dbafaecd3c88fbcd532beed756`.
App and consistent SQLite backup retained before installation. No configuration,
credential or database edits were used to make the feature pass.

Native E2E: UI-created conversation, harmless sky-color question on the existing
deepseek-v4.1-flash selection, first-message fallback observed immediately, then
distinct generated title and real primary answer. Screenshot 01 captured the
completed generated title (not the earlier fallback). UI rename committed `E2E`;
leave/reopen, quit/restart and reopen retained it and the response. Unicode
`typeText` did not enter the intended Chinese suffix, so the observed committed
manual title is honestly recorded as `E2E`, not the intended longer label.

Five screenshots and all raw gate logs are retained outside the worktree under
the local evidence identifier `issue-65-2026-09-19`; the local manifest records
absolute paths and hashes. No private screenshot was uploaded to GitHub.
The previous failed workspace log is retained with SHA-256
`73a5268b3515098daefde36d328a847ff7ae327a5df1890386378c37e5322f5e`.

Limits: native E2E used one configured provider/model; timeout, failure, same-text
manual races, deletion and privacy boundaries are covered by production-chain
tests rather than artificial live-provider failures. At-most-once crash policy
and three-attempt UI read recovery remain the accepted R3–R6 contract.
