# #307 compact-guide implementation checkpoint

## 2026-10-23 01:28 UTC

The first implementation execution reached its active-stage budget and is interrupted, not READY. The parent confirmed its terminal state and inspected all preserved content before replacement. No partial file was reset, discarded or regenerated.

- Specification commit: `d8e2e9e410c5c459f9f72413160dfffc13af1f63`; based on master `d4a65a381d95b6bb8e4ff8d2858a11a1042ffa3a` (#570), which adds unrelated production compaction tests. Preserve these tests; the current delivery policy does not require an empty suite.
- After the writer stopped, parent fetched and integrated master `f3eab9827dcd1ddb459088410d6f1eedcbce8864` (#571) without conflicts at `20b4f59a5b7023fe566d0c2b867891ef5dd3f805`. Incoming changes affect Composer rendering and its documentation, not the onboarding source. Partial hashes remained identical.
- No implementation commit, CI run, completed implementation report or independent review exists for this correction yet. Local checks so far are read-only source/diff/hash inspection, not compilation, test execution or visual acceptance.

## Preserved partial content

| Path | State | SHA-256 |
| --- | --- | --- |
| `crates/vega_ui/src/cua_onboarding/controller.rs` | 16-line partial delta: reject overlapping recheck, shared privacy URLs, cancel pending inspection without closing UI | `c7346d2892d7802fab8e71b21cbd4cacfd1928d2e58b4eacedbfcd5fc0808b01` |
| `crates/vega_ui/src/cua_onboarding/mod.rs` | Partial PermissionPane enum/URL methods, crate-internal model module and callback identity | `885722b532e16210762e11cd383ee7e8e2e30c0763104930a709d3caf1dc60c8` |
| `crates/vega_ui/src/cua_onboarding/model.rs` | New untracked 295-line shared preparation model, not yet consumed by Settings/card | `a96e55c20fcca8d33db2825bff62819116f4b6507789f3edd7b0f6e2be60e24b` |

The new model holds one ProbeClient, pending exact ticket, service/adapter errors, identity-scoped UI owners and one cancellable polling task. `shared`, `acquire`, `release`, `recheck`, `snapshot` are implemented in source only. Release with close_guide cancels inspection; the last owner stops polling, retaining clearly marked past facts. Automatic acquisition is limited to first initialized clean state; explicit recheck uses Manual intent. This is an incomplete design requiring integration and review, not proof that ownership or retained/error semantics are correct.

## Remaining work and next verification

1. Replace old 480×420 diagnostic card/view with 400×140 branded drag helper per C1–C6/C10; remove no-longer-needed diagnostic scroll wiring. Reuse exact bundle and permanent on_drag freeze, native following and close ordering.
2. Have Settings consume the same shared preparation model; real independent facts, recheck/error recovery, exact per-permission routes and restart note live in Agents Native. No new second probe chain. Decide acquire/release integration explicitly, including Settings disposal/reopen and closing the guide while Settings remains.
3. Add targeted typed settings requests/host routing so first enable/privacy actions show the compact guide; retain original task/context/project fences and navigation-only behavior.
4. Update only scoped Layout tokens; embed the existing official Vega SVG without asset mutation or dependencies.
5. Review cancel/ticket/retained-state/current-owner paths and risk-proportionate regressions. Preserve underlying runtime/session/native safety mechanisms. New code must not hide persistent cleanup failures or infer live capture/Ready.
6. Obtain a stopped exact manifest with actual static checks; independent non-writer review and full existing Actions on that final version, not a temporary flow gate. Native/visual effect remains user manual acceptance.

No writes are permitted concurrently in the implementation scope. Parent owns Git/index, spec and checkpoint; replacement implementation writer owns the remaining source and its final implementation report. Current runtime, session adapter, native files, dependencies, workflows, release and user settings remain unchanged.

## 2026-10-23 01:52 UTC · UI wiring preserved, finalization pending

The second writer also ended with an explicit interrupted budget state, not READY. Parent confirmed it stopped and inspected its artifacts. This time substantive UI wiring exists: 12 tracked source files changed (393 insertions/538 deletions) plus the shared model. Guide is now 400×140 with an embedded official SVG, default instruction/drag region/close and optional task return; Settings presents independent permission facts using the shared model; typed PermissionPane requests reach the host, which reuses an open guide. Old card ProbeClient/scroll and Settings component-only worker have been removed. Native/runtime/session/dependency/asset/workflow paths remain unchanged. No report, commit or CI yet.

Current HEAD remains `20b4f59a5b7023fe566d0c2b867891ef5dd3f805`; fresh fetch found no further baseline delta. Exact inspected source SHA-256:

| Path | SHA-256 |
| --- | --- |
| cua_onboarding/controller.rs | `c7346d2892d7802fab8e71b21cbd4cacfd1928d2e58b4eacedbfcd5fc0808b01` |
| cua_onboarding/mod.rs | `885722b532e16210762e11cd383ee7e8e2e30c0763104930a709d3caf1dc60c8` |
| cua_onboarding/model.rs | `cfb3f66e903caa8239477152eb6a0271f4fc24ee51f7710e65b8fa5ce64a6837` |
| cua_onboarding/card.rs | `1ea5a985ac9a436071bcd8e049c7783bb491337b901f852c9dd52c025d0967db` |
| cua_onboarding/card/view.rs | `90cdbaa1161e2b7acfe23e185f4ba16f8ba1c6143c24c8aa7980da302bd659d7` |
| settings/state.rs | `a9732a94ad00f0029e2f35f2a3988bc533421a13d52f33b905532feaf056a1fd` |
| settings/agents.rs | `bdb805408b1b8d635b044d36a100a43a946dde076a1740682444bb83b817b1d4` |
| settings/mod.rs | `dcf9de93f8097a762d931d85354639dab75477c3a53133eb7a941bebb479c706` |
| settings/preferences.rs | `0115ffd3d51edaf73ff9d960bef2bf45c40ea8fe153475d294757823fc4d2705` |
| settings/render_impl.rs | `01e46dbca798b55d28b0abbd3d7d3e0d2a3ecb8659f55a70276450a371446d8f` |
| host window/cua_onboarding.rs | `e7cccf25bb4a4b01e5437bb57adb9a06914a24225917a8cfe8044ea8c503c206` |
| host window/render.rs | `f700802261ab90dc18f0e0f73eb0597dd3e8fc5b2ac167b60fb3335f1036782c` |
| theme/lib.rs | `b79a47bed0adb553d9b6ed53cb2b2c311ef66c633eaab3914da8fb3be37d75de` |

Parent checks actually found a trailing space in card/view.rs:280 and rustfmt differences; these checks have not passed and source has not been changed to hide that evidence. Needed next work is bounded finalization, not another full investigation: verify actual GPUI API/import/asset path types, format, examine model owner cancellation/retained error visibility, and produce stopped report. The default 140pt vertical fit must use the real 24pt close row (not the earlier approximate20pt arithmetic), padding/border, gap and72pt app target; avoid flex shrink/clipping. Settings opening unrelated sections must not unintentionally auto-acquire an exclusive probe. Shared probe/cleanup failure must remain discoverable from task-origin guide even though full diagnostics moved to Settings. Address these existing scope boundaries, then obtain exact final manifest for independent review and CI.

## Finalizer stopped · parent recovery handoff

The third implementation execution (`01M5K7F0H2RAV41ZWF3M0H3G88` / `01M5K7F0H2FTMR1MJD5Y2YMBD9`) is terminal/interrupted after its active-stage budget; it did not report READY. All three implementation executions are confirmed stopped. Parent inspected the preserved final artifacts and recovered the missing handoff under the interrupted-agent procedure, rather than restarting implementation or representing interruption as success.

The final source fixes the previously recorded static/lifecycle issues: ordinary Settings construction/open does not acquire ownership; passive Settings acquisition and Manual recheck are separate; a queued poll rechecks cancellation before updating; shared failures notify the guide with a Settings recovery route; icon geometry is nonshrinking and the real close row/borders are included in the 138pt calculation. Two production-controller regressions were added and retained. Persisted-enable acquisition remains fenced by visible Settings and successful saving.

Parent independently repeated focused rustfmt and `git diff --check` (both exit 0), verified the complete source manifest, inspected acquisition callers and protected-path diffs, and fetched master again. HEAD remains `20b4f59a5b7023fe566d0c2b867891ef5dd3f805`, master remains `f3eab9827dcd1ddb459088410d6f1eedcbce8864`, and there is no newer master delta. No local execution, build or tests occurred. The exact fourteen-file SHA-256 manifest, implemented scope, limitations and review questions are preserved in [the parent integration handoff](vega-issue-307-compact-guide-implementation.md).

Stage at the checkpoint above: recovered, stopped review/CI candidate; no writer READY, independent review, current Actions result or native acceptance was claimed then.

## Continuation: development GREEN and delivery-policy integration

Candidate `ccba58f8b9fa47ead8e3e516d234548bbc8ef608` was committed, pushed and opened as draft PR #574. Actions run `39889718957` at that exact head passed fmt/all-target Clippy, retained Python 5+15+40, and Rust 12 discovered/12 executed/12 passed/0 skipped (including both new production-controller regressions); required job `120207119205` succeeded. Parent retrieved the raw counts on continuation. The old review execution is not available through current agent tools and left no published result; no PASS is inferred.

Fresh fetch found `9f9fa092a3ca391a65f08419c9206f32979119e6` (#573), which corrects Rust-test retention independently of optional TDD. It was merged cleanly at `77bfcecd591699fb0e1cd7da8faaa15a308de095`, with no compact-guide source change. Upstream removed its compaction tests; do not restore them. The compact spec and integration handoff now record the policy change and reachable original test/source/patch/CI identities. Next is narrowly delegated removal of this task's already-passing controller test and exclusive wiring, independent read-only review of the stopped production/cleanup version and full latest-head CI (Rust 0 discovered/0 executed). Product acceptance is unchanged; the user still must accept the actual compact visual/drag effect in a new build.
