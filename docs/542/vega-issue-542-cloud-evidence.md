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
