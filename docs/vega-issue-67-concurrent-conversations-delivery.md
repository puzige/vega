# Issue 67 concurrent conversations · delivery evidence

## Freeze

- Task contract: [concurrent conversations](vega-issue-67-concurrent-conversations.md), committed as `63d68ee` before any code/test edits (rebased spec commit `6edbbe9`).
- Production baseline: `fcafcef`; second-stage implementation replaces the window-wide single-flight policy from PR #102.
- Evidence directory identifier: `issue-67-concurrent-2026-09-21`; raw logs and local manifest are outside the removable task worktree.
- Main agent owns specification, review, final validation and delivery; dedicated subagent owns implementation.

## Red evidence

Command: `scripts/cargo-lock.sh --wait test -p vega issue67_concurrent_production_new_thread_enters_before_origin_finishes -- --nocapture`

The mounted production root opens A, sends through its real composer, blocks
at the worker/provider boundary, creates a new task via the sidebar and submits
B through its composer. Only provider/config credentials are owned fixtures.
Before any production edits, the test failed with exit 101:

```text
assertion `left == right` failed: B must enter its worker while A is still active
  left: 1
 right: 2
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 184 filtered out; finished in 1.84s
```

Raw log: `red-admission-test.log`, SHA-256
`ba83c3915764c857371586f9ba0fdcd0834a79bbe7b345e870606774dadaa9a0`.
An earlier `red-admission.log` records an unavailable shared Cargo lock, which
is an execution delay rather than product RED evidence. The queued command
acquired the shared lock before compiling/running.

## Results

Implementation commit: `d3cc592`, rebased without conflicts onto `32de6a3`.
Coordinator gates and release packaging passed on that integrated source.
Merge/install identity will be written back to Issue #67 after integration.

## Ownership audit

| Boundary | Result |
|---|---|
| Admission, stream, generation, pending content, terminal, Stop | Active map keyed by thread; exact stream/generation checks on ingress, finish and rollback; duplicate submission preserves owner and draft |
| Preparation | Existing exclusive trusted lease retained; exact stream owner prevents Stop A from consuming B preparation |
| Context | Automatic ownership and operation mapping per run, frozen model fencing; navigation preserves owners, terminal removes only its own; returning active stream avoids stale reload |
| Artifact | Live/terminal capture routes retained by full identity; background capture never swaps current route; preview/open stay route-fenced; drained hidden routes retire |
| Plan review | Per-thread pending request retains exact authority through terminal, persistence and approved continuation; background continuation creates its own artifact owner |
| Models and accounting | Model gates use target thread; each worker owns frozen model/pricing snapshot; usage, titles and summaries remain thread-fenced |
| Branch and commit | Conservative any-active-run safety gate retained for shared repository mutation; ordinary peer sends are admitted |
| Manual context and configuration actions | Existing exclusive auxiliary lease retained; a different primary run alone does not block manual context |
| Settings and window teardown | Each retained stream independently fails hidden permission closed; Drop cancels every active token and retained artifact worker |

The coordinator checked the production diff for new unwrap/expect, abort and
unsafe calls (none), unchanged dependency/schema boundaries, route effects and
exact identity removal. Review fixes included preventing unrelated context
refresh on peer completion, preserving composer history during review, and
establishing artifact ownership for an off-route approved continuation.

## Evidence classification and intermediate failures

- Mounted A/B lifecycle cases use real window/composer, controllers, workers,
  owned filesystem/Git and SQLite, with MockProvider at the network boundary.
  Both live partial records, final text/tool joins, frozen models and separate
  usage values are checked. This is E2E-REAL as defined by exec-guide, not a
  real remote-provider claim.
- Concurrent context status/stale-owner assertions inject records at production
  AgentBatch ingress (ownership evidence). The plan test injects A's draining
  terminal handshake, then exercises real review persistence and approved
  worker beside B (FAULT-INJECTION for that handshake).
- `concurrent-expanded-first.log` retained the failed close test: the fixture
  held a root Entity outside the GPUI App update that flushes deferred releases.
  Moving that test-owned release into App update fixed the test; actual window
  removal and both interrupted durable records remain asserted.
- `concurrent-final-local.log` retained an incorrect fixture expectation that
  an artifact route stay active while Settings hides it. The corrected check
  observes Settings persistence and drained retained routes; it does not require
  the hidden route to occupy the visible artifact slot.
- Mechanical map call-site compiler failures remain in `check-initial.log`.
  All first failures and later passes are retained locally.

## Coordinator gates on integrated source

Verified at UTC `2026-09-21T09:05:58.108704+00:00` (Asia/Shanghai UTC+8). All 125 targeted tests passed; no tests were ignored in these selected suites.

| Command | Result | Seconds (including lock wait) | Log / SHA-256 |
|---|---|---:|---|
| `scripts/cargo-lock.sh --wait fmt --all -- --check` | exit 0 | 72.19 | `final-fmt.log` / `5716242f3f267e640b1ac5f68c1a1980fb594e8b70ea703ba1a7025926ff30c3` |
| `scripts/cargo-lock.sh --wait clippy -p vega -p vega_ui --all-targets -- -D warnings` | exit 0 | 12.61 | `final-clippy.log` / `8cdfd53db36c815e265748b9b674e647f7fef866cb151f7d90a0ec663b5687f3` |
| `scripts/cargo-lock.sh --wait test -p vega tests::agent -- --nocapture` | 21 passed; exit 0 | 26.59 | `final-vega-agent.log` / `567f9c6a4573f9f08c46083588e46eb768efbea95dda570f058bfc7641670c34` |
| `scripts/cargo-lock.sh --wait test -p vega tests::composer_actions -- --nocapture` | 23 passed; exit 0 | 2.6 | `final-vega-composer.log` / `a3481b9b9fed9ef8208f7cd2a626e9c9e1fe0eccea7ac4a137b1537f285be884` |
| `scripts/cargo-lock.sh --wait test -p vega artifact -- --nocapture` | 5 passed; exit 0 | 1.96 | `final-vega-artifact.log` / `329e3d832a5284bba9be99d35a7079c4c81a46fc2a70ebf884ff887e5709b91a` |
| `scripts/cargo-lock.sh --wait test -p vega tests::plan -- --nocapture` | 6 passed; exit 0 | 0.42 | `final-vega-plan.log` / `9973cb03adbbf5f39f3f8239a4c9693f88bf54a0d06bdb77377a3abb5e58f908` |
| `scripts/cargo-lock.sh --wait test -p vega tests::model_selection -- --nocapture` | 4 passed; exit 0 | 0.51 | `final-vega-models.log` / `96f0ac76668787fa651d889fdfc05d532b1e9f7b6a1b0cdbe8beb6cbaa0ecfe4` |
| `scripts/cargo-lock.sh --wait test -p vega tests::branch -- --nocapture` | 8 passed; exit 0 | 1.11 | `final-vega-branch.log` / `922251b3760f663af318079f4a6f8c00401e6370b3e0f89e26ab6c74ae347e97` |
| `scripts/cargo-lock.sh --wait test -p vega tests::commit_ -- --nocapture` | 9 passed; exit 0 | 5.71 | `final-vega-commit.log` / `ad143cae8e4069454f8db8341664ecc0d47ae25696fda76c12c0636e8f6484e8` |
| `scripts/cargo-lock.sh --wait test -p vega window::navigation -- --nocapture` | 9 passed; exit 0 | 5.07 | `final-vega-navigation.log` / `b08458b87026773e168b897ab26d34caaa33a0b12cfe84b2102a82739090ebe3` |
| `scripts/cargo-lock.sh --wait test -p vega tests::automatic_titles -- --nocapture` | 5 passed; exit 0 | 0.6 | `final-vega-titles.log` / `10d411a2945be586ec2aa7f6527d7054e99327c9c0c0f6fec4a5adba26bf0173` |
| `scripts/cargo-lock.sh --wait test -p vega tests::reasoning -- --nocapture` | 11 passed; exit 0 | 0.82 | `final-vega-reasoning.log` / `d9d875bda2f146d0342333a0c8bb46c2fab204a7c773385cd4d61b4c24319f06` |
| `scripts/cargo-lock.sh --wait test -p vega_ui permissions_cards -- --nocapture` | 13 passed; exit 0 | 19.31 | `final-ui-permissions_cards.log` / `476c44a65b604c07f42f37b0646fadc49911ab2fd3b5dd8ca9db19b09aeae180` |
| `scripts/cargo-lock.sh --wait test -p vega_ui context_control -- --nocapture` | 5 passed; exit 0 | 0.73 | `final-ui-context_control.log` / `c128eebc1ff1dfa99e131362f2f38e789b2793dbe903315e511aa3513c982b61` |
| `scripts/cargo-lock.sh --wait test -p vega_ui composer_actions -- --nocapture` | 6 passed; exit 0 | 0.39 | `final-ui-composer_actions.log` / `e033f7e5581ebdd2df0d6ff432a4ea6924a3bc9e5e7cb1f9afab07250fefc3c2` |

## Acceptance matrix result

| IDs | Evidence / outcome |
|---|---|
| C67-01/02 | PASS: mounted A/B concurrent workers, distinct live partial records, real write/read tool persistence and artifact capture, usage 31/59 |
| C67-03 | PASS: exact stream remount; both real pending write prompts fail closed under Settings without run cancellation |
| C67-04 | PASS: background completion preserves B route/stream/draft/focus; Settings remains open through both completions |
| C67-05/06 | PASS: symmetric independent Stop and actual window removal + Drop with two durable interrupted records |
| C67-07 | PASS: duplicate thread submit preserves owner/token/draft and starts no extra worker |
| C67-08 | PASS: real background artifact capture and retained-route drain; context ingress ownership/status/model fences and affected worker regressions |
| C67-09 | PASS with handshake injection: queued A review persists and starts a real background continuation beside B; both durable outcomes and B route checked |
| C67-10 | PASS: B model selection while A frozen; branch/commit regression gates; manual context beside a different primary owner |
| C67-11 | PASS for real missing-reference pre-provider failure/retry and exact generation removal; OS thread-spawn failure not injected (same scoped finish path audited) |
| C67-12 | PENDING: user verification of installed merged application with real provider |

## Residuals

- NOT RUN: workspace-wide tests, explicitly excluded by the maintainer for this card. Run only affected functionality and crate gates.
- PENDING: real installed A/B manual verification by the user. Issue remains Open / In review after merging and installation; this is not automatic Done.

## Release package gate

Forced xtask rebuild with `scripts/cargo-lock.sh --wait clean -p xtask`, then
`scripts/cargo-lock.sh --wait run -p xtask -- package`: exit 0, 63.33 seconds.
The package ran from the issue worktree, built release Vega, signed the bundle,
verified its signature and validated Info.plist. Raw log `premerge-package.log`,
SHA-256 `70e3252fac043de7a67b2887700c6150cbcd7b0952d7136fd93a7e866710308b`.

The coordinator will repeat the forced xtask rebuild and package from merged
master before replacing the application; the final installation hash and smoke
result belong to the Issue writeback and local manifest.
