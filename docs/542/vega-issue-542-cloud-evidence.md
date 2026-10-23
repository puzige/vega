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

The prior reviewer execution is unavailable (the current child directory is empty). A replacement independent, read-only reviewer is reviewing the exact integrated commit and cumulative production delta against `5a1ec8ad26fa53fd6210a8e58b6ad85e739dd8bb`. Approval has not yet been obtained. Next: finish independent review; remove all 23 Rust tests and exclusive support in a separate commit while preserving production behavior; independently review that removal and statically inventory the final tree; run the existing complete required gate on the final head and verify Rust `0 discovered / 0 executed`, `no Rust tests`. Only then formalize/merge and keep the Issue open, In review for user manual testing. No local compilation, execution, Rust/Python tests, packaging or CUA has been performed.
