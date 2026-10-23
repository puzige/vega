# #542 cloud evidence

## Frozen source and first interface RED

- Production baseline: `2ba904f7074115de5ee89cc18c376c083f803357`.
- Original test commit: `27026d56c06fdec83706e53e52b94d005e08ee69`.
- Independently reviewed test head: `8a6e16a8eee5d2d530248a66ce300dff5c3db64c`.
- Frozen v2.1 specification SHA-256: `a3edebd8d929a6a7b2a564b22c2c1b8c41b36ca86a850973c1fc5e0786f6197b`.
- Draft PR: <https://github.com/puzige/vega/pull/564>.
- First Actions run: <https://github.com/puzige/vega/actions/runs/39829738672>, associated PR-head `8a6e16a8eee5d2d530248a66ce300dff5c3db64c`, created `2026-10-22T18:15:26Z`, completed with failure.
- Actual checkout in both jobs: synthetic PR merge `2f476bd777aedf485cd8491788ffd87a5536cf90`, merging that reviewed head into master `6615748f91273bfb8ee424011b3b5a09b3777d2b`. The run did not test the PR-head tree alone. This accounts for the renderer diagnostic's shifted line number.
- Clippy job: <https://github.com/puzige/vega/actions/runs/39829738672/job/120002383659>.
- Nextest job: <https://github.com/puzige/vega/actions/runs/39829738672/job/120002383966>.
- Required aggregator: <https://github.com/puzige/vega/actions/runs/39829738672/job/120004011728>, failure because both execution jobs failed.

The parent retrieved and inspected all failed-step diagnostics. Format succeeded and the existing Python release-script check step succeeded in Actions. Clippy reported five reads of the deliberately missing `size_bytes` projection field and the deliberately missing `StreamEntry::ArtifactGroup` variant. Nextest stopped while compiling the same missing variant, before completing test discovery. No unrelated compiler, dependency, harness, network, or runner failure appears in the retrieved diagnostics.

### Original diagnostic excerpts

The following excerpts preserve the original failure text; ANSI styling, job prefixes, timestamps, and repeated source-context lines are omitted.

```text
error[E0609]: no field `size_bytes` on type `types::artifact::ArtifactCard`
   --> crates/vega_conversation/src/artifact/mod.rs:642:29
642 |         assert_eq!(cards[0].size_bytes, Some(12_345));
    |                             ^^^^^^^^^^ unknown field
    = note: available fields are: `id`, `label`, `source`, `current_file_id`, `preview_available`
```

The same E0609 occurs at lines 650, 658, 666, and 674 for the frozen unknown-size, known-zero-size, and provenance-downgrade assertions.

```text
error[E0599]: no variant named `ArtifactGroup` found for enum `conversation_stream::model::StreamEntry`
    --> crates/vega_ui/src/conversation_stream/render.rs:1595:71
1595 | ... Some(StreamEntry::ArtifactGroup { group }) => ...
     |                   ^^^^^^^^^^^^^ variant not found in `conversation_stream::model::StreamEntry`
    ::: crates/vega_ui/src/conversation_stream/model.rs:1088:1
1088 | pub(crate) enum StreamEntry {
     | --------------------------- variant `ArtifactGroup` not found here
```

Nextest's actual failing command was:

```text
cargo nextest list --workspace --message-format json
error: command `/Users/runner/.rustup/toolchains/1.98.0-aarch64-apple-darwin/bin/cargo '--color=always' test --no-run --message-format json-render-diagnostics --workspace` exited with code 101
```

The workflow contains shell source strings for both empty and nonempty suites in its logged step preamble. Those strings are **not executed test summaries** for this failed run.

### Classification and limits

This is **minimum-interface-missing RED**, not a behavior-assertion failure or acceptance result. The observed missing interfaces were specified before production implementation. The third prospective interface, capture-error delivery, was not reached by this compilation and has no separate diagnostic in this run.

- Static source inventory: 22 definitions (12 UI, 6 controller, 4 evidence projection).
- Runtime discovered count: not obtained; discovery failed to complete.
- Runtime executed/passed/failed/ignored/skipped counts: not obtained; no behavior assertion executed.
- GREEN: not obtained.
- No production implementation or test removal at this checkpoint.
- Independent classification review: the existing nonwriter reviewer confirmed the raw failures, both actual checkout identities, minimum-interface RED classification, and count limitations. No behavior, observer-positive-control, or later compile-stage success is claimed.

Original sources and patches remain recoverable independently of the final branch:

- <https://github.com/puzige/vega/commit/27026d56c06fdec83706e53e52b94d005e08ee69.patch>
- <https://github.com/puzige/vega/commit/8a6e16a8eee5d2d530248a66ce300dff5c3db64c.patch>
- [UI source](https://github.com/puzige/vega/blob/8a6e16a8eee5d2d530248a66ce300dff5c3db64c/crates/vega_ui/src/artifact_card.rs)
- [Controller source](https://github.com/puzige/vega/blob/8a6e16a8eee5d2d530248a66ce300dff5c3db64c/crates/vega/src/window/artifact.rs)
- [Projection source](https://github.com/puzige/vega/blob/8a6e16a8eee5d2d530248a66ce300dff5c3db64c/crates/vega_conversation/src/artifact/mod.rs)

## Current-baseline synchronization before implementation

A fetch found two new upstream changes after the first run: context/image budgeting (#557) and Composer controls (#558). The clean task branch merged current master `6615748f91273bfb8ee424011b3b5a09b3777d2b` without conflicts, preserving the exact original test commits and RED SHA. The synchronized production-unimplemented head is `d6210619593f925fdd4f7910c4b119bbbc6b039e`.

The upstream renderer delta only changes Composer permission/model triggers; the artifact observation arm is unchanged. The UI test file, observation module, controller test file, projection test file, and Cargo.lock are byte-identical to the reviewed test head. Local `cargo fmt --all -- --check` and full baseline-to-head `git diff --check` succeeded after synchronization. These are static checks, not runtime validation.

The synchronized head was pushed to the existing draft PR for another baseline RED check. Production writes were withheld while that result was pending.

After independent review identified the actual synthetic checkout, the parent verified its Git tree through the GitHub Git API: `202469c24c14443895960555013d760d5231519f`. Local `d6210619593f925fdd4f7910c4b119bbbc6b039e^{tree}` is exactly the same tree. Thus the first run already exercised the synchronized file content; this is content-identity evidence, not a new run or any behavior execution.

No local compilation, build, package, tests, Python checks, Vega execution, or CUA was performed.

## Synchronized interface RED and implementation release

- Run: <https://github.com/puzige/vega/actions/runs/39831306039>, associated head `d6210619593f925fdd4f7910c4b119bbbc6b039e`, created `2026-10-22T18:25:58Z`, completed with failure.
- Actual checkout in both jobs: `4466a28f605ac8f75b78d05f78aa2e75d3523995`, merging that synchronized head into master `6615748f91273bfb8ee424011b3b5a09b3777d2b`.
- GitHub API tree identity: `202469c24c14443895960555013d760d5231519f`, identical to the synchronized local tree and first run's synthetic tree.
- Clippy job: <https://github.com/puzige/vega/actions/runs/39831306039/job/120007857359>.
- Nextest job: <https://github.com/puzige/vega/actions/runs/39831306039/job/120007857006>.
- Required aggregator: <https://github.com/puzige/vega/actions/runs/39831306039/job/120009694187>.

The parent inspected the full failed-step diagnostics and checkout logs. The same five E0609 `size_bytes` reads and E0599 `ArtifactGroup` variant failure were the only compiler diagnostics; Nextest again failed its build with exit 101 before discovery. Format and the existing Python release-script step succeeded. There was no new unrelated harness/dependency/runner failure in these logs. This second run validates the synchronized preimplementation baseline, not a behavior retry-to-green.

The existing nonwriter reviewer independently checked this delta and **approved releasing production implementation from `d6210619593f925fdd4f7910c4b119bbbc6b039e`**. Classification remains minimum-interface RED only. Observation controls and behavior assertions did not execute; discovered/executed/passed/failed/ignored/skipped counts remain not obtained. Early target-interface compilation failures do not establish that later stages are clean.

The read-only preparation child failed without a report. The parent confirmed its terminal failed state and that there were no tracked changes or new code files; only this parent-owned evidence document was untracked. A replacement sole implementation writer received the same baseline, frozen tests, call-chain findings, safety constraints, and verified RED evidence. No test work was restarted. The replacement is authorized for minimal production implementation, not test/fixture/observer changes. Parent retains documentation and Git integration ownership.

Frozen Rust test-module suffix SHA-256 values (from the first top-level `#[cfg(test)]` through EOF at reviewed `8a6e16a8eee5d2d530248a66ce300dff5c3db64c`; these identify source content, not execution):

| File | SHA-256 |
|---|---|
| `crates/vega_ui/src/artifact_card.rs` | `2e4fdceeb7eaa86db585de8aa7afcbf08bb365a295be84de341d64afcda9e6f8` |
| `crates/vega/src/window/artifact.rs` | `e2e8c63c1d5fae3ec96deb274049da27ae8cf25b079a9602fc03efa34eab6019` |
| `crates/vega_conversation/src/artifact/mod.rs` | `0e470cd28563afa2aee80d8ec3f278032deb1216858448c7117fc31a893da077` |

The unchanged observation-module SHA-256 is `99b8426f633c16bdea6022ddeb20e56555447d9d92fa42957ec047400ede1c47`. The parent will compare these exact scopes after the writer stops; any necessary harness correction must be separately classified and reviewed, never treated as a product fix or silent expectation change.

## First stopped implementation

The dedicated writer returned READY and stopped all writes. Patch base is `d6210619593f925fdd4f7910c4b119bbbc6b039e`; `git diff HEAD --binary` SHA-256 before adding this parent-owned document is `842e54eaf1ad4f82e57a8fdd1379d6b335fe11f6ab8787d877dc514a46c91ee4`. There are no new implementation files. The parent independently reproduced this exact hash and ran `cargo fmt --all -- --check` and `git diff --check`, both successfully. No runtime validation was performed locally.

The implementation changes 14 production files: evidence projection, controller in-flight ownership, compact artifact UI, and conversation stream buffering/lifecycle/rendering. Full escaped relative labels are used only as presentation deduplication keys within a turn; latest sealed entities remain preview/open authority. No path or external capability is reconstructed from labels. Capture work retains original message/history ownership; final flush occurs after assistant finalization and late insertion honors the matching Summary boundary. Obsolete inline-artifact activity paths were removed.

### Frozen tests at stopped implementation

The initial whole-suffix selector above is sensitive to a newly necessary test-only `ArtifactSource` import near the top of `artifact_card.rs`. The parent therefore additionally compared the actual nonempty `mod tests {` / `pub(crate) mod tests {` blocks through EOF with original reviewed `8a6e16a8eee5d2d530248a66ce300dff5c3db64c`. All three complete test modules are byte-identical:

| Module | SHA-256 |
|---|---|
| UI | `16f7c4b8b76ceb3d5e6869ac238cfdd7fb5d4b5a17321a35562f6da22a9863b8` |
| Controller | `cae39fa2624f0ea40ad33c6e307557af7a58616df96ac18e9d116c404bd31553` |
| Evidence projection | `850076e3b56d7abd71ae77d3a1df61cda7434335aca6f387125f5aa6109f70f8` |

Controller/projection whole-suffix hashes also remain unchanged. The observer module, Cargo manifests/lock, temporary fixtures/support, and upstream Composer/selection observation wiring have no delta. No test initializer exception was needed; no tests, data, assertions, or expectations were changed.

## First implementation validation — compile failure, not GREEN

- Associated implementation head: `7acb4bc225eb83184a3f7436b2afca3b7ddd2a97`.
- Run: <https://github.com/puzige/vega/actions/runs/39839237766>, created `2026-10-22T19:20:39Z`, completed with failure.
- Actual Clippy checkout: synthetic PR merge `503b9249df39528782a50c6bace3592bf8ba4628`.
- Clippy: <https://github.com/puzige/vega/actions/runs/39839237766/job/120035567118>.
- Nextest: <https://github.com/puzige/vega/actions/runs/39839237766/job/120035567720>.
- Required aggregator: <https://github.com/puzige/vega/actions/runs/39839237766/job/120037591389>.

The parent inspected the full failed-step diagnostics before choosing a repair. Format and Python release-script steps succeeded. Both Rust jobs stopped during compilation; no behavior assertion executed and runtime discovery/execution/pass/fail/ignore/skip counts were not obtained.

### Responsible layers

1. **Production API defect** at `artifact_card.rs:342`: `Context::on_focus_out` requires a four-argument callback, including `FocusOutEvent`, while the implementation supplied three arguments.
2. **Previously masked harness import defect** at `artifact_card.rs:1003,1031,1042`: the frozen pointer-event helpers call `to_platform_input`, but the `gpui_kit::InputEvent` trait is not in scope. The implementation did not change these helpers. The earlier minimum-interface RED stopped before this compilation stage and was not proof of a clean harness.

Original diagnostic excerpts:

```text
error[E0593]: closure is expected to take 4 arguments, but it takes 3 arguments
   --> crates/vega_ui/src/artifact_card.rs:342:26
342 | .push(cx.on_focus_out(focus, window, |this, window, cx| {
    |          ^^^^^^^^^^^^                ------------------ takes 3 arguments
    |          expected closure that takes 4 arguments

error[E0599]: no method named `to_platform_input` found for struct `gpui_kit::MouseMoveEvent` in the current scope
    --> crates/vega_ui/src/artifact_card.rs:1003:14
help: trait `InputEvent` which provides `to_platform_input` is implemented but not in scope; perhaps you want to import it
```

MouseDownEvent and MouseUpEvent report the same missing trait import. The locked GPUI `on_focus_out` signature is `FnMut(&mut T, FocusOutEvent, &mut Window, &mut Context<T>)`; its `InputEvent` trait defines `to_platform_input` for these event types.

A separate nonwriter reviewer approved the ignored focus-event callback parameter and missing `InputEvent` import before editing. The original implementation writer then applied exactly these two corrections and stopped writes. The callback body, event simulation, fixtures, durations, expectations, assertions, and acceptance are unchanged. Parent reproduced the scoped binary-diff SHA-256 `96ff1050cdb489cc93a29bfe825b6fd4dcb2d4f15c9e7f1e6eac2255dc1f828e` and confirmed whole-workspace fmt and diff checks. The import-only harness correction is explicitly distinguished from product behavior; it is not a new behavior test or a valid behavior RED.

## Independent implementation finding — new boundary cycle required

The nonwriter implementation review of `7acb4bc225eb83184a3f7436b2afca3b7ddd2a97` returned changes required: the callback compile defect above and a separate reconciliation/deduplication defect. `ArtifactGroup.files` caches the initial escaped label, but reconciliation can update the actual projection label. Capturing `a.txt`, reconciling its rename to `b.txt`, then capturing another edit of `b.txt` in the same turn can append a second row because the first remains keyed by `a.txt`, although it displays `b.txt`.

The parent confirmed the responsible paths: real reconciliation assigns `next_label` in the artifact service; controller metadata delivery applies it; the group observer only synchronizes errors while capture matches the stale cached key. This violates same-file/latest-capability aggregation, rather than representing a runner or observation failure.

This rename/reconcile boundary is absent from the frozen 22 tests. The original cycle is therefore paused under execution-guide §7.2: preserve all original assertions and source evidence, record an additive boundary-cycle specification against the existing implementation, author and independently review a focused observable regression while the deduplication implementation remains unchanged, obtain valid Actions RED, then make the minimal deduplication correction and require all original tests plus the new boundary test GREEN. No deduplication fix or retrospective proving test is authorized before that new RED. No product choice or safety contract is relaxed.

The API/import repair is an independently reviewed prerequisite that lets the test environment compile far enough to evaluate the new boundary; it does not implement the deduplication fix. No GREEN, test deletion, ready PR, merge, installation, or manual acceptance is claimed.

## Boundary test submission and macro-shadowing harness failure

The additive test and cycle specification were independently reviewed and committed as `5bee216d8dbec305f98bbd4020e7551e88803465`, after prerequisite repair `f52a1e7f0b599dfc6f80611a43be7223df7ca929`. The test-only code-diff SHA-256 is `729407ff84576961bec29e2cd426a253c27f1650e9a1b7437091fdb5531f04e0`; removing just that addition restores the previous UI file byte-for-byte. Original 22 tests are retained; inventory is 23 static definitions, not runtime counts. The committed specification was read back remotely with matching Git blob `ee007b5ade96ed6c826af8f8ad7be68234cebfd2`.

- Run: <https://github.com/puzige/vega/actions/runs/39842063120>, associated head `5bee216d8dbec305f98bbd4020e7551e88803465`, created `2026-10-22T19:39:54Z`, failed.
- Actual Clippy checkout: `004fb2cef80c75cfd86c1c055d09fd7d20a83c88`, synthetic merge into master `6615748f91273bfb8ee424011b3b5a09b3777d2b`.
- Nextest job: <https://github.com/puzige/vega/actions/runs/39842063120/job/120045425323>.
- Clippy job: <https://github.com/puzige/vega/actions/runs/39842063120/job/120045425464>.
- Required aggregator: <https://github.com/puzige/vega/actions/runs/39842063120/job/120047489162>.

Both jobs stopped with the same harness compilation error:

```text
error: recursion limit reached while expanding `#[test]`
    --> crates/vega/src/window/artifact.rs:1625:5
1625 |     #[gpui_kit::test]
     |     ^^^^^^^^^^^^^^^^^
```

The parent inspected both logs and actual locked macro source. The suggested compiler recursion-limit increase is not the appropriate correction: `window` imports `gpui_kit::*`, `artifact` imports its parent wildcard, and the controller test module imports that wildcard again. This brings the GPUI `test` macro into scope. Its generated unqualified `#[test]` resolves recursively to that same macro instead of Rust's built-in attribute. GPUI Kit 0.6.1 explicitly documents this hazard and advises explicit test imports. The UI test module uses a prelude/named imports that exclude the macro.

Classification: **harness macro-name shadowing**, neither target boundary RED nor GREEN. No behavior assertion ran and runtime discovery/execution/pass/fail/ignore/skip counts were not obtained. Format and the existing Python step succeeded. An initially inconsistent Clippy job status delayed retrieval of that job's log; the already available Nextest failure was used for diagnosis without treating missing logs as success.

A nonwriter reviewed and approved an exact controller test import-only replacement. The original writer applied it and stopped writes, preserving all six qualified GPUI attributes, test bodies, assertions, fixtures, production code, and runtime settings. Scoped binary-diff SHA-256: `f7514fb9a564379551caa0b10dd9295a526b9d994a0dd8029da3adb0e3c9c6b1`. Whole-file content outside the import block is unchanged; no recursion limit or test ignore was introduced.

A required fetch found newer master `5fb9a3d0491c620a901e67a0e3f348ba05bd76c6` (bounded native delegation/context, #561). Parent inspected the nonoverlapping paths and merged it without conflicts as `2d638b37f93743837fc3f44a4403f400f12b90cb`, preserving the stopped import patch and all prior commits. Artifact implementation and test files were unchanged by that integration. The next existing Actions run must evaluate this current integrated baseline; no prior result substitutes for it. Deduplication repair remains withheld until valid boundary RED.

## Explicit-import correction — incomplete inventory failure

- Import-hygiene commit: `b68bda5644c76962ddc375585e65bf54ecd817e1`.
- Run: <https://github.com/puzige/vega/actions/runs/39844416136>, created `2026-10-22T19:56:03Z`, failure.
- Actual Nextest checkout: `80c388351d6aad2f1de1e0867b8ce25e9091cefa`, synthetic merge of that head into master `5fb9a3d0491c620a901e67a0e3f348ba05bd76c6`.
- Nextest: <https://github.com/puzige/vega/actions/runs/39844416136/job/120053787072>.
- Clippy: <https://github.com/puzige/vega/actions/runs/39844416136/job/120053786616>.
- Required aggregator: <https://github.com/puzige/vega/actions/runs/39844416136/job/120060129508>.

Macro recursion is no longer reported. Both jobs instead fail on five controller harness import errors introduced by the incomplete explicit-import list: three E0599 `update_global` calls (lines 1413, 1559, 1616) require `gpui_kit::BorrowAppContext`, and two E0433 `WorkspaceFileId` uses (1424, 1816) require the existing conversation type. The parent inspected both original job logs and the locked trait definition. No behavior assertions ran; discovery/execution/pass/fail/ignore/skip counts remain unavailable, and this is not boundary RED or GREEN.

The prior import-list reviewer independently diagnosed the omissions: its inventory missed the extension trait and omitted a known identifier when assembling the final list. The reviewer acknowledged the responsible harness layer and approved exactly two missing named imports, preserving the existing methods, bodies, assertions, fixtures, and execution settings. Do not replace `update_global`, restore wildcards, raise recursion limits, or suppress these errors. The same writer received only this two-import repair scope; no deduplication repair is authorized.

The whole-run status initially remained queued even though Nextest had finished; later its completed job log was retrievable through the existing GitHub job-log API while Clippy was running. CLI refusal to supply logs was treated as an observation limitation, never success or target RED. Clippy's eventual raw log confirmed the same five missing-import errors.

## First executed development suite — four failures

- Head: `091191bf26634981414a4b4f9b9618107ab58735`.
- Run: <https://github.com/puzige/vega/actions/runs/39846822695>.
- Actual checkout in both jobs: synthetic merge `6d4498846e6ddb4e4c39164e5e9f81d40c0e316d`, merging that head into `5fb9a3d0491c620a901e67a0e3f348ba05bd76c6`.
- Clippy: <https://github.com/puzige/vega/actions/runs/39846822695/job/120062185157>, success, including fmt and all-target warnings-denied compilation.
- Nextest: <https://github.com/puzige/vega/actions/runs/39846822695/job/120062185310>, failure during actual test execution, not compilation or discovery.
- Required aggregator: <https://github.com/puzige/vega/actions/runs/39846822695/job/120064659541>, consequential failure.

The parent re-read the original failed-step and full-run logs. Nextest reports `Starting 23 tests across 15 binaries` and `23 tests run: 19 passed, 4 failed, 0 skipped`. All six controller tests and all four evidence-projection tests passed. Nine UI tests passed and four failed. These are actual execution results, not the earlier static inventory; there is no combined GREEN.

| Failed test | Original failure | Classification / next verification |
|---|---|---|
| `artifact_group_merges_capture_after_reconciled_rename` | `observation.rs:294`, group rows `Some(2)` instead of `Some(1)` | The previously reviewed cached-label deduplication defect is now observed behavior RED. Repair current presentation identity while keeping latest sealed capability authority. |
| `artifact_actions_are_keyboard_reachable` | `artifact_card.rs:2188`, `Tab must reach preview without pointer hover` | Actual focus/reveal path needs repair; retain the existing keyboard assertion and typed event checks. |
| `artifact_group_keeps_failures_visible_after_the_turn_ends` | `observation.rs:294`, group rows `Some(8)` instead of `Some(9)` | Determine notification scheduling versus product error synchronization before editing the responsible layer. Preserve hidden-file error visibility and exact arithmetic. |
| `artifact_group_omits_source_labels_and_editor_row` | `artifact_card.rs:1665`, combined background/border/radius quad predicate | Check the locked GPUI drawing contract; do not change product styling merely to satisfy an invalid observation assumption, or weaken required styling checks. |

## Continued repair baseline and current policy

On continuation, the old child directory contained no active executions and the task tree was clean. The parent fetched and merged latest master `9f9fa092a3ca391a65f08419c9206f32979119e6` without conflicts as `894bb5e7db5ce9629f7c40e843ad4a7dedef7d7a`. The artifact UI and observation files were unchanged by this integration. Upstream changes in shared stream/controller paths were retained; the final current version still requires fresh Actions validation.

The integrated repository policy removes mandatory TDD sequencing, while retaining independent implementation review, no local execution/build/tests, truthful risk-based verification, Rust test/support removal before master, and the final complete zero-test required gate. Earlier records remain historical evidence, not fabricated current gates. For this already exercised change, all 23 development tests remain required regression checks; no failing test is deleted or weakened. Product acceptance and the explicit no-CUA boundary are unchanged.

One code writer owned production repairs in `artifact_card.rs`; a separate read-only diagnosis checked the remaining observation/scheduling failures. The parent owns evidence and Git integration. No local Vega execution, compilation, tests, Python tests, or CUA was performed.

### Source-backed responsibility classification

Independent read-only diagnosis and parent inspection of locked GPUI source confirmed that the hidden-error test observes within the same outer `TestAppContext::update_window` as its mutations. GPUI `App::finish_update` flushes notifications only after the outer update completes; the test's `frame()` calls refresh/draw and clears the element arena, not notification effects. The existing card → group → stream notification route is present. Correct sequencing returns from setup, mutates through an outer test context, then observes in a new window update. Keep the exact `8 → 9 → 9 → 8` counts, both themes, hidden seventh-file error text, danger color, and independent error persistence; no direct observer invocation or sleeps.

The style test's single-quad conjunction contradicts `gpui-pre 0.3.4/src/style.rs:713–755`: background and border paint as separate quads, respectively with a transparent border and a transparent background. Require both same-container quads independently, retaining exact theme tokens, all positive border widths, and the exact 12-logical-pixel scaled radius. Do not change the predicate to an OR, drop a required style property, or alter production drawing just for the test.

The production writer reached its execution budget after producing a partial patch and stopped; no READY result was claimed. Parent preserved its scoped binary diff SHA-256 `f22c985b49c62756a17f863749241db4e2300c5c214635496def6b9b1b36abc7` and confirmed tests unchanged. The patch makes compact cards render as their own GPUI entities and coalesces live labels while owning superseded subscriptions. A replacement sole writer received that exact patch plus the diagnosed harness corrections; completed test authoring and prior evidence were not restarted.

Keyboard diagnosis is qualified: GPUI Kit Root supplies generic Tab routing, but Vega's actual `main.rs` creates `VegaWindow` directly, not that Root. Vega focuses the stream using its public `focus_composer` API. The test should start from that actual focus route and let each key update complete before observing focus-driven repaint. Do not add a test-only global focus fallback or directly focus artifact controls. The card's real local Tab path and notification-driven repaint remain product obligations.

A post-merge static audit also found that upstream added `VegaWindow.onboarding` while the temporary `for_artifact_tests` constructor lacks its default initializer. This is a test-fixture integration omission, not a request to execute CUA or change onboarding behavior. It must be reconciled with the same default used by the production constructor before the next cloud build.

### Stopped repair content

The closure writer completed and stopped all writes. Its scoped `git diff HEAD --binary -- crates/vega_ui/src/artifact_card.rs` against `894bb5e7db5ce9629f7c40e843ad4a7dedef7d7a` has SHA-256 `10ac08978ebbc1ba6b29c7292dbdaca978c5499ae2cdc190dca48058db50e0e2`, reproduced by the parent. Live-label coalescing is linear per synchronization, retains first distinct-file position and latest capture authority, and owns subscriptions so superseded card errors do not reappear. Compact rows render as card entities with a real local Tab/Shift-Tab route. No capture, Git, provenance, permission, route/history fence, cancellation or allowlist code changed.

Only the three diagnosed UI harness sites changed: split outer updates for error and focus notifications; initialize keyboard focus through public `focus_composer`; require separate same-bounds background and border quads with all four exact radii and positive border widths. All 23 test definitions remain. No tests were added, removed or ignored; no expected text, row count, capability, timing limit or product requirement was lowered. The stopped writer reported SHA-256 `6b9f5ea0e1e4861c07193e0145ebe8e53c0216c8e6d727d52ba437026335a777` as a UI test-module identity. Parent readback below establishes that it identifies the complete `artifact_card.rs` file, not a module-only extraction; the writer's module/byte-count description is not retained as verified evidence.

The separate fixture writer added exactly `onboarding: cua_onboarding::OnboardingHost::default()` inside `for_artifact_tests` and stopped. Scoped diff SHA-256 is `3e976dfd64cf48aa0107f26d59db537547c8005a26ea8a0e6b3bbd7a421480e2`; no production initializer or behavior changed.

Parent verification: `cargo fmt --all -- --check` and whole-worktree `git diff --check` exited 0. These are static checks, not compilation, execution, test success, or GREEN. No local Vega/test/Python/CUA execution occurred. Next at that checkpoint: independent review and existing Actions on the same committed repair version, preserving all 23 development regressions.

## Successful development validation and recoverable sources

- Tested task head: `2e64e16d151267481e601644f8469a65d5623ed8`, `fix(ui): reconcile artifact rows and restore keyboard repaint`.
- Run: <https://github.com/puzige/vega/actions/runs/39894505989>, created `2026-10-23T03:20:42Z`, completed successfully. Both checkout logs report the complete synthetic merge `6cf94875f41cae6c946edddb6ea9af3f68931054`, integrating the task head with master `9f9fa092a3ca391a65f08419c9206f32979119e6`.
- [Clippy job 120221683211](https://github.com/puzige/vega/actions/runs/39894505989/job/120221683211): success, including `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets -- -D warnings`.
- [Nextest job 120221683020](https://github.com/puzige/vega/actions/runs/39894505989/job/120221683020): success, including the existing three Python checks, `cargo nextest list --workspace --message-format json`, and nonempty `cargo nextest run --workspace`.
- Actual runtime footer at `2026-10-23T03:24:07Z`: `Summary [   1.693s] 23 tests run: 23 passed, 0 skipped`. Discovery reports 23. Execution/pass count is 23, failure count 0, skipped count 0; no separate ignored count was emitted in the retained footer. All 13 UI, six controller and four projection regressions passed, including the previously failing rename, keyboard, hidden-error and surface checks.
- [Required aggregator 120222704303](https://github.com/puzige/vega/actions/runs/39894505989/job/120222704303): success, completed `2026-10-23T03:24:18Z`.

These are development behavioral results, not the final empty-suite gate, real external integration or user manual acceptance. No assertions were lowered and no tests were removed to obtain this result.

Before removing tests/support, the parent fetched each following source via GitHub's contents API at the full tested SHA and confirmed its returned Git blob ID equals the local committed blob. These fixed source links and the original/additive commits above preserve the complete development sources independently of the working tree and final squash.

| Recoverable source at tested head | Git blob | Complete-file SHA-256 |
|---|---|---|
| [UI suite and production file](https://github.com/puzige/vega/blob/2e64e16d151267481e601644f8469a65d5623ed8/crates/vega_ui/src/artifact_card.rs) | `de646fd9b977c44865463c0e080cd489b05ac2ad` | `6b9f5ea0e1e4861c07193e0145ebe8e53c0216c8e6d727d52ba437026335a777` |
| [Actual render observation support](https://github.com/puzige/vega/blob/2e64e16d151267481e601644f8469a65d5623ed8/crates/vega_ui/src/artifact_card/observation.rs) | `85070d182d6862c27d5b847876c45941db2c3212` | `99b8426f633c16bdea6022ddeb20e56555447d9d92fa42957ec047400ede1c47` |
| [Controller suite and production file](https://github.com/puzige/vega/blob/2e64e16d151267481e601644f8469a65d5623ed8/crates/vega/src/window/artifact.rs) | `b9828eec8e3e60b16c5ccaa6d4cc03ecda37711d` | `5aa87d353e24f05993524d04161ee8c8d4d7242080405da88985adfedcaadc0d` |
| [Evidence projection suite and production file](https://github.com/puzige/vega/blob/2e64e16d151267481e601644f8469a65d5623ed8/crates/vega_conversation/src/artifact/mod.rs) | `dba3448f4ddb718dd2d9ec56f182be5e145d1457` | `d2b94969d5a6a45e64d46733ec301f2fd0703f4d11871d80232a37fbbe69e4d1` |

The [repair commit patch](https://github.com/puzige/vega/commit/2e64e16d151267481e601644f8469a65d5623ed8.patch) was also retrieved successfully: 27,617 bytes, SHA-256 `c1800cf7bd5fd7fdd1e8cee379ec096bf42674035218f2eaeb063953476762e3`. It is the repair delta, not a claim to contain the entire suite; recover the complete sources from the fixed links above.

### Baseline integration and remaining stage

The parent fetched and cleanly merged new master `5a1ec8ad26fa53fd6210a8e58b6ad85e739dd8bb` as `37a0f4610eb6dec7eb80045b1c5182523f92fe27`, preserving all task and development-test commits. The upstream change concerns stage handoff/recovery; its only shared task file is `conversation_stream/content.rs`, in deferred-permission presentation rather than artifact grouping. The successful development run above predates this merge and is not presented as validation of a later head.

At that checkpoint the prior reviewer execution was unavailable (the child directory was empty). A replacement independent, read-only reviewer was reviewing the exact integrated commit and cumulative production delta against `5a1ec8ad26fa53fd6210a8e58b6ad85e739dd8bb`; approval, test/support removal and the final empty-suite gate were pending. The subsequent finding and current work are recorded below. No local compilation, execution, Rust/Python tests, packaging or CUA has been performed.

### Integrated development validation

Run [39896566304](https://github.com/puzige/vega/actions/runs/39896566304), head `e323a00314723f6116ab8557b81144e516785b12`, completed successfully at `2026-10-23T03:48:20Z`. Both job checkout logs report synthetic merge `a9b3302af4240f84532340a927d046decba1be07`. The [Nextest job 120228283160](https://github.com/puzige/vega/actions/runs/39896566304/job/120228283160) reports `Summary [   1.960s] 23 tests run: 23 passed, 0 skipped` and 23 discovered. Thus 23 executed, 23 passed, zero failed/skipped; no separate ignored count was emitted. The [Clippy/fmt job 120228283422](https://github.com/puzige/vega/actions/runs/39896566304/job/120228283422) and [required aggregator 120229465590](https://github.com/puzige/vega/actions/runs/39896566304/job/120229465590) also succeeded. This validates the integrated development version against `5a1ec8ad26fa53fd6210a8e58b6ad85e739dd8bb`, not the subsequent baseline or keyboard correction, and it does not cover the newly identified overflow focus path.

## Independent review: overflow keyboard traversal

The nonwriter reviewed stopped implementation `37a0f4610eb6dec7eb80045b1c5182523f92fe27` against `5a1ec8ad26fa53fd6210a8e58b6ad85e739dd8bb` and requested one P2 correction: the focused `Show N more` control handles Enter/Space but not Tab/Shift-Tab. Individual compact-card handlers are siblings. Composer bindings require the `Composer` context, which belongs to another branch rather than this transcript control's ancestors. The common `ConversationStream` root does not provide that traversal. The reviewer traced the focused-node parent/context dispatch in locked `gpui-pre 0.3.4` and upheld the finding; absence of GPUI Kit Root alone was not the complete rationale. This is a static product finding, not a locally reproduced failure. No additional blocking finding was reported in ownership/history fences, terminal placement, latest sealed capability selection, deduplication/subscriptions, error retention, existing preview structure, capture/security or the six-target allowlist.

The cleanup execution was cancelled and confirmed interrupted before any changes; the worktree remained clean. All 23 development regressions and their support are retained. Master `6e490aefc18fd0e8a263cbcd6b67570bcbf9348e` was integrated without conflicts as `2922a688ed434a0d74e67f173336b59767015050`. That upstream delta changes onboarding/settings, not the artifact production or test files. A fresh fetch confirms this baseline before the keyboard correction.

### Acceptance and next verification

This clarifies the approved keyboard and overflow requirements, not a new product design. In a finished seven-file turn, with pointer outside and focus initialized through public `ConversationStream::focus_composer`:

1. Tab reaches `Show 1 more` through real mounted controls without directly focusing the overflow button or adding a harness-only traversal fallback.
2. Shift-Tab returns to the preceding visible file action; Tab returns to overflow, and a further Tab moves away rather than leaving focus trapped.
3. Enter and Space each expand a fresh collapsed group to all seven actual file rows, remove `Show 1 more`, and leave focus on a real available file action so subsequent traversal works.
4. Existing 23 regressions remain unchanged; folding/error arithmetic, capture ownership, sealed capabilities, disabled/busy/reading behavior, theme tokens and all safety boundaries remain intact.

Use one focused GPUI regression over the existing real stream renderer, control snapshots/painted text and typed events; every key mutation completes its outer update before observing settled focus. No direct private-state algorithm assertions, sleeps, local execution or CUA. Make the minimum production change at the overflow handler and its keyboard expansion focus handoff. Existing Actions will validate the corrected development suite, then sources/results must be pinned before separate test/support cleanup and the final zero-test required gate.

### Stopped overflow correction

On continuation, the prior writer was absent from the child directory and no code patch existed; only this parent-owned evidence document was dirty. A fresh fetch confirmed master `6e490aefc18fd0e8a263cbcd6b67570bcbf9348e` was already integrated at `2922a688ed434a0d74e67f173336b59767015050`. A replacement sole writer completed the scoped correction in `crates/vega_ui/src/artifact_card.rs` and stopped writes.

The production delta is confined to the overflow key handler: Tab/Shift-Tab traverse through the actual window focus APIs; Enter/Space expand and hand focus to the first available newly exposed file action, falling back to an available visible file action or normal traversal. Reading, busy and unavailable cards are excluded; preview-unavailable cards use their Open action. Pointer expansion, visible text, style, capture and security logic are unchanged.

One additive GPUI regression exercises fresh Enter and Space cases through public composer focus and actual mounted controls. It checks backward/forward traversal, seven actual painted filenames, overflow removal, seventh-file action focus, the exact typed Preview capability and subsequent Tab. Each input completes its outer update before settled observation. The parent verified all original 13 UI tests/helpers remain a byte-identical prefix and the six controller/four projection test files remain byte-identical. Static definitions are now 24 (14 UI, six controller, four projection); this is not a discovery/execution result.

Stopped scoped binary-diff SHA-256 against `2922a688ed434a0d74e67f173336b59767015050`: `c5dc6b178a58d1d4232e9a7adf0cc745422627ef0e49a88c3f02bade28fab94b` (10,500 bytes). Complete UI file SHA-256: `11612fd577e006cf25c28f510e76e9c59cf712738414e4b6dc9a3a18901123ef`. Parent reproduced both identities, `cargo fmt --all -- --check` and `git diff --check` (exit 0). Independent review and existing Actions on this stopped committed version are next; no runtime success, cleanup or final gate is claimed. No local Vega execution, compilation, Rust/Python tests or CUA occurred.

The final precommit fetch found master `3277ebb5fd710d7d33080f91b6c5235e1558e0d3`, correcting message-anchor geometry and preview dismissal. The parent inspected its two shared stream-file deltas and merged it without conflicts as `a2e6f8e4ba1f1e7a6a9daf5c6159849ff87b3812`, preserving the stopped dirty correction. The task-specific delta in those files remains only artifact state plus the `ArtifactGroup` match arm; no upstream anchor change was discarded. The overflow binary diff is unchanged. Fresh review and cloud regression cover this integrated baseline; earlier successful runs do not validate it.

### Overflow regression execution and responsible layer

Stopped correction `0190ecf85e77cd58f04a39d61b72f7746799ccc0` was independently reviewed without static blocking findings, but [Actions 39914716332](https://github.com/puzige/vega/actions/runs/39914716332) failed. Both jobs checked out synthetic merge `90c5136a84955fbaef8132e003067634032e161d`. [Clippy/fmt 120285559310](https://github.com/puzige/vega/actions/runs/39914716332/job/120285559310) succeeded. [Nextest 120285559754](https://github.com/puzige/vega/actions/runs/39914716332/job/120285559754) discovered and executed 24 tests: `Summary [2.605s] 24 tests run: 23 passed, 1 failed, 0 skipped`. All original 23 passed. The additive keyboard test failed at `artifact_card.rs:2400`, `Tab must reach the sixth file action`, before reaching the overflow control. [Required aggregator 120287485058](https://github.com/puzige/vega/actions/runs/39914716332/job/120287485058) consequently failed. This is not a successful corrected-suite result.

The parent inspected the original failure and locked focus traversal/snapshot APIs. The independent read-only diagnosis reached its stage budget and stopped without a completed review report; its source findings were then checked by the parent. Seven real Write events create an aggregate run-stage header, unlike the original single-file keyboard fixture. That header is a tab stop but handles only Enter/Space (`run_activity.rs` stage header); the sibling Composer handlers and Markdown reading wrapper do not handle its Tab. Thus normal traversal can stop before the artifact rows. This is a production-path gap, not grounds to remove the Write events or bypass the header in the fixture. The minimal repair adds ordinary Tab/Shift-Tab traversal to the two existing run-activity disclosure header handlers, retaining activation semantics.

A second source-backed boundary must be corrected before the same rerun: leaving file six hides its actions, so only its Preview entry remains mounted. Generic reverse traversal from overflow cannot restore the previous Open action. Overflow Shift-Tab must explicitly prefer the last available visible file action (excluding reading/busy/unavailable cards), falling back to normal traversal when no such action exists. This preserves the already recorded exact backward-navigation acceptance rather than relaxing it to any focus. All 24 test definitions, fixtures, limits and assertions remain unchanged. Next verification is the same complete cloud suite and an independent review of this narrow repair; no cleanup or local execution is authorized by the failure.

The sole writer completed those two production repairs and stopped writes. Scoped two-file binary diff against `0190ecf85e77cd58f04a39d61b72f7746799ccc0`: 3,827 bytes, SHA-256 `50f745e765013b577c2d77bcbba77c45b44c5a40e2591e3b9ba7ab341ec771f1`. Complete-file SHA-256: UI card `128121cd36c2f2febf0c2c9df0c78b4f21a0a5446a36bd55af6398dcbd5adbaa`; run activity `5d6cdc666daea51227104a4e99fbf667a778e24e0e95e9ea0333078463cff61c`. The parent reproduced the diff identity, unchanged UI test-module SHA-256 `2267e9eeb52867d2a4e0743006d833a60b038d1652d3013229b1f02851bb9325` (61,414 bytes), whole-workspace fmt and diff checks (exit 0). No new state, dependencies, comments or production unchecked unwraps were added. At that checkpoint independent delta review and fresh complete Actions were pending; the 24-test failure above remains preserved, not erased by static review.

## Complete corrected development validation

The nonwriter independently approved the narrow focus delta at stopped head `dec5d200c03fbdbcb09902135a7a7e2d7256d365`, reusing unchanged cumulative conclusions and checking focus eligibility/lifecycle, disclosure activation, real traversal and preservation of all 24 tests. No blocking finding remained. Review was static, not runtime acceptance.

[Actions 39919980627](https://github.com/puzige/vega/actions/runs/39919980627), at that exact head, completed successfully. Both jobs checked out synthetic merge `f7528f3fb70ed024529384cfc5f12a01ca97782e`, merging master `3277ebb5fd710d7d33080f91b6c5235e1558e0d3`; its tree `66ed7223a83db5d50cc892d70846d99dc20bd914` equals the stopped task-head tree. [Nextest 120302588969](https://github.com/puzige/vega/actions/runs/39919980627/job/120302588969) reports 24 discovered and `Summary [5.033s] 24 tests run: 24 passed, 0 skipped` at `2026-10-23T07:53:06Z`. Thus 24 executed/passed, zero failed/skipped; no separate ignored count was emitted. The added `artifact_overflow_keyboard_traverses_and_expands_to_an_available_file_action` passes, covering both Enter and Space cases, exact reverse/forward navigation, seven real file rows and typed capability. [Clippy/fmt 120302589142](https://github.com/puzige/vega/actions/runs/39919980627/job/120302589142), existing Python checks and [required aggregator 120305213962](https://github.com/puzige/vega/actions/runs/39919980627/job/120305213962) also succeeded. All 24 expectations stayed unchanged after the preceding execution failure.

### Corrected source retention before cleanup

The parent fetched these complete sources through GitHub contents API at full head `dec5d200c03fbdbcb09902135a7a7e2d7256d365` and matched each Git blob to the local committed source. They remain recoverable independently of the final tree and squash.

| Source | Git blob | Complete-file SHA-256 |
|---|---|---|
| [UI tests and product](https://github.com/puzige/vega/blob/dec5d200c03fbdbcb09902135a7a7e2d7256d365/crates/vega_ui/src/artifact_card.rs) | `28c77969d40febaf05bdfc152373600a3b102719` | `128121cd36c2f2febf0c2c9df0c78b4f21a0a5446a36bd55af6398dcbd5adbaa` |
| [Controller tests and product](https://github.com/puzige/vega/blob/dec5d200c03fbdbcb09902135a7a7e2d7256d365/crates/vega/src/window/artifact.rs) | `b9828eec8e3e60b16c5ccaa6d4cc03ecda37711d` | `5aa87d353e24f05993524d04161ee8c8d4d7242080405da88985adfedcaadc0d` |
| [Projection tests and product](https://github.com/puzige/vega/blob/dec5d200c03fbdbcb09902135a7a7e2d7256d365/crates/vega_conversation/src/artifact/mod.rs) | `dba3448f4ddb718dd2d9ec56f182be5e145d1457` | `d2b94969d5a6a45e64d46733ec301f2fd0703f4d11871d80232a37fbbe69e4d1` |
| [Observation support](https://github.com/puzige/vega/blob/dec5d200c03fbdbcb09902135a7a7e2d7256d365/crates/vega_ui/src/artifact_card/observation.rs) | `85070d182d6862c27d5b847876c45941db2c3212` | `99b8426f633c16bdea6022ddeb20e56555447d9d92fa42957ec047400ede1c47` |

The [latest repair patch](https://github.com/puzige/vega/commit/dec5d200c03fbdbcb09902135a7a7e2d7256d365.patch) was retrieved: 9,325 bytes, SHA-256 `08a8ec1e5e06ffb0b7162ed56147abc783d1d89343c20ce1ded18f61f8003fa7`. It is a repair delta, not the complete suite; the source links above preserve all 24 definitions and temporary support. Original and additive test commits remain fixed earlier in this record.

### Baseline and cleanup stage

Before further changes the parent fetched master `b032d0d2ab9ee057137f3e5ad2c0e517011ea1ae` and merged it without conflicts as `b25bd346383d00ca6d76b8a6f4e6d6b7645d6b38`. This upstream context-usage correction adds primary UsageUpdated delivery and live-context reset/clear operations in shared ingress/stream files; artifact ownership, grouping and focus code are unchanged. The development success above predates that merge; final integrated-head review and complete CI remain required. Next is a separate removal of all 24 Rust tests and exclusive support, with protected production/Python/CI paths preserved, followed by exact cleanup review and final `no Rust tests` (0 discovered/0 executed) gate. No local execution, compilation, tests, packaging or CUA occurred.

## Development test/support removal checkpoint

The sole cleanup writer reached its stage budget after making the authorized removal and stopped; no completed READY was fabricated. The parent inspected the actual dirty artifacts against immutable cleanup base `8ccb1895b3cc9c9f3941ff6673b1e234b2c4ddac`, confirmed the writer had stopped, and retained the patch rather than repeating completed work. Exactly the 15 authorized paths changed; no untracked files or index changes were introduced. Scoped code/manifest/lock binary-diff SHA-256 is `235fc28a3c07dc60cdcfcd27e0eb00c8e1b8aa487525418707679f1ec0b6ff26`.

All 24 development definitions (14 UI, six controller, four projection), the 348-line observer, synthetic sealed-ID/preview constructors, error-event/no-I/O/window fixtures, two worker-interception branches, observer render/text/tooltip hooks, three control observation wrappers, test features/dev dependencies and exclusive lock graph are removed. Real IDs, AX roles/names, focus/tab handlers, typed events, content/layout, card/group observers, worker paths and safety checks remain. `compact_button` now returns its actual `Interaction`; text children receive their unchanged values directly.

Parent verification on this final removal content: `cargo fmt --all -- --check`, `git diff --check`, and `cargo metadata --no-deps --offline --locked --format-version 1` exited 0. Repository-wide Rust/manifest scan found no test attributes, cfg(test), task support features, mock inbox/fixtures, observation module wiring or ObservedElement/test_support wrappers. The only Rust doc fence found is preexisting `text`, not executable doctest. Projection production is byte-identical after removing its test module; controller production is byte-identical after removing exactly its test module and two mock intercepts. Ten manifest/fixture/observer-hook files plus Cargo.lock equal immutable integrated master `b032d0d2ab9ee057137f3e5ad2c0e517011ea1ae`. Lock packages decrease 924 to 914, only the previously listed ten test-exclusive packages; retained versions/sources/checksums/metadata are unchanged, with no newly introduced identities. Lock SHA-256 is `71388bbb23bdeddcf884a02731f1190f22f32c889521ce431f933a834d12ce9d`. No local Vega/Rust/Python execution, compilation or CUA occurred. Independent cleanup review and cloud final gate remain pending.

During the static audit, origin/master advanced to `988e1e37271c666f05ff8eae46d662324e91afd6` (queued steering run continuation). It was not integrated at that checkpoint. That upstream lifecycle delta adds a nonterminal `RunContinued` call to `finish_agent_message` and references the removed inline Artifact child in a new header predicate. Cleanup success does not waive that new product boundary or final-head validation.

## Run-continuation baseline compatibility

Independent read-only cleanup review approved exact removal commit `8201db8df4f49d344c323d0016aee89c5471d61f` with no blockers, reproducing its scope/hash and retained production invariants. The separate lifecycle diagnosis did not complete, but the parent checked its original source findings against immutable upstream: `SteeringAccepted` continues the same runtime loop and emits `RunContinued → PendingMessageDelivered → MessageStarted → RunStarted`, without a run terminal. Upstream explicitly states that insertion is not task completion. Current artifact finishing is called by the old message's closure, so an unqualified merge would display its cards during an active continued run. The new run header also contains a stale `RunActivityChild::Artifact` match, a variant removed by this task. These are integration defects, not changes to capture/security acceptance.

### Compatibility acceptance before repair

- A continuation closes the predecessor message for original-owner and late-capture validation but does not publish that message's artifact group while any explicit successor remains unfinished. A gap before successor MessageStarted must not publish it either.
- A terminal Completed/Interrupted/Failed successor releases each finished, loaded predecessor's own group at that original message anchor and the successor's own group at its own anchor; no card/error/capability is moved into a different message group. Multiple continuations and late capture success/errors follow the same explicit chain.
- Unrelated completed runs retain their normal display even if another run is active. Unknown/stale owners, history replacement, cancelled capture errors and six-target/fence/safety semantics remain unchanged. Duplicate terminal events for continued predecessors remain ignored by upstream.
- No continuation relation is inferred from adjacency, time or missing duration. Invalid/cyclic relations must not produce unbounded traversal or premature display.
- Preserve upstream continuation state/clock/terminal filtering. Adapt the new run-header child predicate to the remaining process-child enum, rather than restoring the removed Artifact variant or losing real children.

Minimum plan: retain the upstream run-continuation field while resolving the core merge conflict; gate artifact flushing on the explicit successor chain being finished, and revisit its predecessor groups when the final message finishes. Keep durable owner registration/finished-message tracking separate from display eligibility. Use bounded chain traversal and original per-message insertion. Do not restore the cleaned test framework or introduce dependencies. Validation for this narrow post-cleanup integration is static call-chain/state-transition review by a nonwriter plus the existing final complete required CI; continuation-specific runtime/manual acceptance remains for the user and is not included in the earlier 24-test success. Final no-Rust-test CI is not behavioral regression success.

### Stopped compatibility implementation and parent static verification

The dedicated writer reported READY and stopped all writes on the merge of `8201db8df4f49d344c323d0016aee89c5471d61f` with upstream `988e1e37271c666f05ff8eae46d662324e91afd6`. The parent reproduced the stopped source identities before staging. The three-file binary diff against the first parent is 6,280 bytes, SHA-256 `706785630bae260b887a23ba6819344a27c010b58d423b8d4c166e2c1fb1c637`. The additional composer/header repair, excluding the already staged upstream changes and core resolution, is 2,558 bytes, SHA-256 `f5f0286efbecc4842f14222c539c4a1208129b370434ed97db54b8ff33d679c8`.

- `core.rs` retains upstream continuation state beside all artifact ownership/group fields without restoring the obsolete inline-card comment. Complete-file SHA-256: `27102e72f354b4e79a67d195193af504b12b06dabc7ce6374ff63f59cee6ad34`.
- `composer.rs` keeps physical closure separate from display eligibility, traverses only explicit successor relations with a map-length-plus-one bound, and deterministically rechecks closed messages when a tail finishes. Existing original-message anchors, Summary bounds, ownership/generation checks and late-capture paths remain. Complete-file SHA-256: `9acd52a678039e9d8ad49a26e918240415b3c0aae30ae49cfd18bb551d2579be`.
- `run_activity.rs` counts the remaining Thinking/Tool/ToolGroup children as process details; it does not restore inline artifacts or change continuation clocks/terminal behavior. Complete-file SHA-256: `389c0f3ed522a25ac3ceb585aa9273333e096d7778c58dabcfb6114a367f60e7`.

Parent checks on this stopped content: `cargo fmt --all -- --check`, `git diff --check HEAD`, and `cargo metadata --no-deps --offline --locked --format-version 1` all exited 0. Full Rust/manifest scans found no test attributes, exclusive test wiring/support, stale Artifact child references or working-file conflict markers. The only documentation fence is the existing nonexecutable `text` fence; other backticks are ordinary string literals. Cargo.lock remains SHA-256 `71388bbb23bdeddcf884a02731f1190f22f32c889521ce431f933a834d12ce9d`. The upstream runtime/conversation/store/migration changes are retained; the phase delta only removes old inline-artifact handling. A fresh fetch still identifies the same upstream merge baseline.

These are static checks and source state walks, not behavioral execution. Independent review of the complete integrated commit and final-head complete required CI remain pending at this checkpoint. No local Vega execution, compilation, Clippy, Rust/Python tests, packaging or CUA occurred.
