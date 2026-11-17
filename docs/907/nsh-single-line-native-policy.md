# Explicit native single-line editing policy

## Approved acceptance and minimum plan (before Rust edits)

This author lane now owns the application editing policy and task-title rename together. This supersedes the prior sidebar-only API handoff, not its historical evidence. Parent reports fresh remote fetch/update with task HEAD `0b5ea426abfd8d4358489e916273d89474fb190f` and master `0b3dcd3603a8e97e0ef74ae25a26f17fc3a094e5`; dirty bytes remain the actual implementation identity. No network, Git mutation, Cargo/rustc, vendor or Fast changes are authorized here.

Acceptance:

1. Add an explicit route policy, default Composer and opt-in SingleLine. Never infer policy from `multiline`, nor simulate an open popup. Carry policy through `ComposerRoute`, `NativeEditorRoute` and existing token validation. A crate-visible single-line renderer selects the matching route.
2. Only the explicit policy claims plain unshifted Return as Submit and plain unshifted Escape as Cancel. Default composer newline, primary Return, popup/prediction priority, history and Tab behavior remain unchanged. Existing native composition suppression runs before command policy; engine composition, pending edits/paste, repeats, serial, owner, focus, identity and single-claim fences remain authoritative. Commands rejected while pending are never replayed.
3. Production task rename mounts the real transactional NSH editor for `RenameSession.input`. Bind its `ComposerShortcut` subscription to the captured input entity, thread, row, project/navigation generation and route. Require current route, focus, matching shortcut and acknowledged editor readiness before submit/cancel. No raw sidebar key fallback, mirror editor or synthesized tokens.
4. Submit reads the latest accepted controller text and keeps original `resolve_rename`, trim/blank, `apply_update`, persistence, pending and async routing fences. Escape/outside click cancel without writing; stale callbacks cannot cancel a replacement session. Outside click may intentionally discard pending edits. Reopen/focus, existing layout/typography, native IME/paste/undo and budgets remain.
5. Group/dormant routing, separator/mounted ownership, unrelated business logic, conversation implementation and shared exports stay out of scope. Parent owns any minimal exhaustive caller initializer update, module exports, Cargo and central builds/tests.

Minimum plan: add the explicit policy and renderer, use the existing native adapter/token/dispatch delivery, replace sidebar old renderer/actions with a session-owned subscription and layout-only mount carrying Window, add bounded meaningful development regressions against actual bridge/adapter and mounted rename when feasible, then report exact file hashes and required parent test/registration commands. No Rust test or native behavior result is claimed until executed by parent. No independent review or fullnative acceptance is claimed.

## Implemented application policy

`NativeEditorPolicy::{Composer, SingleLine(EntityId)}` is default-safe and explicit; the entity belongs to the current rename binding and remains retained by that session. Policy/session identity travels from `ComposerRoute::single_line(session)` through `render_nsh_single_line` to `NativeEditorRoute` and existing native token equality. The policy is not inferred from multiline and does not reuse popup fields. Native command policy is checked only after the existing native/engine composition and identity/focus guards. Default composer key branches remain intact. Pending form commands are consumed, never deferred; a new key after acknowledgment is required.

Production task rename subscribes to the actual current input and checks the binding entity, input, thread, row, project/navigation generations, current route, thread existence, native focus/readiness and shortcut route/stamp/token before calling the original commit/cancel path. There is no mirror, fallback, raw Return/Escape handler or vendor patch. The same-input rebind receives a new binding identity, so old token delivery cannot match the replacement session. Existing native edits, undo, paste and dispatch remain the only editing bridge. See [sidebar implementation details](nsh-sidebar.md#current-implementation-explicit-native-rename-policy-and-nsh-mount).

NSH single-line chrome uses the existing BODY typography and old rem-based input padding/radius/border/fill, with measured total height feedback. Default Composer receives no chrome. Native/GPUI pixel parity is not yet observed. Group/dormant routes and unrelated business changes remain excluded.

## Verification handoff

Executed: direct `/Users/puzige/.rustup/toolchains/1.98.0-aarch64-apple-darwin/bin/rustfmt --edition 2024 --config skip_children=true` on seven owned Rust files, followed by the same command with `--check`; formatting check returned success. This is parsing/formatting evidence only, not type checking. Source searches, SHA-256 and document static checks are allowed; no Cargo, rustc, build, test, network, Git mutation or independent review ran here. No Cargo/rustc version was queried.

Parent integration prerequisites:

- `crates/vega_ui/src/conversation_stream/nsh_composer.rs:21`: add `policy: Default::default(),` to its exhaustive `ComposerRoute` initializer, preserving all other fields.
- `crates/vega_ui/src/sidebar/mod.rs:757,761`: feature-gated re-export of `threads_block::mounted_rename_native_ack_ime_cancel_rebind_and_persistence`.
- `crates/vega_ui/tests/nsh_reading_integration.rs`: register that function from `vega_ui::sidebar` in the existing feature harness and adjust the array length. No new dependency or Cargo feature is needed. Owned `threads_block.rs` already gates/includes/re-exports its development case. Owned `nsh.rs` contains two feature-only real-host observation helpers; remove with test cleanup.

Next central commands (NOT RUN):

```sh
cargo test -p vega_ui text_input::native_adapter::tests::actual_key_translation_keeps_newline_and_single_claim_submit_separate -- --exact
cargo test -p vega_ui text_input::native_adapter::tests::explicit_single_line_keys_deliver_only_after_latest_receipt_and_reject_rebound_route -- --exact
cargo test -p vega_ui text_input::native_adapter::tests::single_line_composition_and_repeat_never_deliver_a_form_command -- --exact
cargo test -p vega_ui text_input::editing_core_tests::single_line_native_candidate_keys_precede_policy_and_old_focus_cannot_submit -- --exact
cargo test -p vega_ui text_input:: --lib
cargo test -p vega_ui --features nsh-reading-integration --test nsh_reading_integration
cargo clippy -p vega_ui --all-targets -- -D warnings
```

Actual adapter cases retain the existing default-composer newline/single-claim assertions and add explicit plain-key positive delivery, latest receipt text, pending no-replay, repeat/composition suppression and changed policy/session invalidation. The bridge case exercises native-composition-before-engine and old-focus rejection. The mounted case exercises actual app host/editor/controller/file-backed persistence, native paste/undo/redo, IME keys, pending Enter, trim/blank, Escape/outside cancellation, rebind/replacement and route change. Its exact registration name is above. Native OS candidate windows, keyboard/focus behavior, three desktop platforms and visual geometry still require central/user validation. Rust development tests and exclusive helpers/wiring must be archived/removed before master. Fullnative acceptance is not claimed.

## Exact author source identities

These dirty bytes, not HEAD alone, identify this delta. SHA-256:

| File under `crates/vega_ui/src/` | SHA-256 |
| --- | --- |
| `text_input/nsh.rs` | `77a0a20601948f032692e685a65c7998a287d8745aef066d391cbf29cc8ad716` |
| `text_input/editing_core.rs` | `34f4ec888643473847dc197a5477981023a5844e325cc454d2f70055ba001982` |
| `text_input/native_adapter.rs` | `dab702585357bdf8d15109b3f59eb1a4f6a08bf2b3fe9d0daa7d69b670bfebcc` |
| `text_input/editing_core_tests.rs` | `25ec9106d78faf8f4c1ae9c4a6c424e06d8056dc389c863d57687ea90886416a` |
| `sidebar/threads_block.rs` | `82d58901ca5d14d83b1bd99782e2dff44c3c227decff96d8661128f3d7cd1175` |
| `sidebar/threads_block/rename.rs` | `820de6499e38cc155a62fe36a5d3ec71d9fd1f2b3127a604451ad3a3c0285b94` |
| `sidebar/threads_block/rename_tests.rs` | `4368e9e3e0546e0ea0cc49c24f6570b81cde2c3bf668685c025bc4fb390ac3cc` |

Starting source hashes for the original four inspected inputs remain in the historical sidebar/inventory entries. No tests/verification from earlier snapshots are attributed to this delta. Stop writes at handoff; parent must re-observe any subsequent byte changes.

## Fixture migration repair after parent execution

This section supersedes the earlier pending integration status, while retaining that historical handoff and its hashes. Parent completed the default-policy initializer, sidebar export and fifteenth runner registration, and reported the 11-target compilation cycle exited 0 in 33.06 seconds. The recorded `target/nsh-closure-ui-signal-build.json` ends with `build-finished` / `success: true`; this evidence predates the repair below.

The original `target/nsh-closure-nsh_reading_integration.stdout` records 14 PASS results. Its stderr records `mounted_rename_native_ack_ime_cancel_rebind_and_persistence` failing at the then-current `rename_tests.rs:192` with SQLite `no such table: projects`. That failure occurs during fixture project creation, before editor mount; it provides no rename editing, IME, cancellation or persistence behavior result. The existing 14 PASS results are retained, not re-investigated or attributed to the repaired source.

Repair acceptance and minimum plan: initialize the existing isolated, real file-backed Store using its production migration API before creating project/thread fixtures; preserve every behavior assertion, scheduler budget, parent correction and scratch-directory lifetime. Do not fabricate schema SQL or change production behavior. `Store::open` configures the connection without migrating (`crates/vega_store/src/lib.rs:150-177`); `Store::migrate` is the production schema entry point (`:207`). The actual mounted recovery setup uses open → migrate → project creation (`crates/vega_ui/src/conversation_stream/shell_recovery/mounted_regressions.rs:125-127`).

The only Rust change is `store.migrate().expect("production schema migrations");` at `crates/vega_ui/src/sidebar/threads_block/rename_tests.rs:186`, immediately after opening the Store. The parent-removed unused import/Fixture window field remain absent, and the current two-argument `simulate_click` is preserved. All assertions, existing scoped Scratch Drop behavior and production files are unchanged. No artifacts or scratch directories were removed by this repair lane.

Post-repair SHA-256 for `crates/vega_ui/src/sidebar/threads_block/rename_tests.rs`: `c0768ec00bcb4044930cc5e369c549cedd17206f973150e08f383c4e4d80ec97`. The earlier hash table is historical, not the current identity of this fixture.

Validation in this repair lane: read original failure logs, inspect actual Store/mounted-recovery APIs, re-read the changed fixture and record its hash. No Cargo/rustc, build, test, network, Git mutation or independent review was run. Post-repair compilation and the mounted rename result remain pending central execution. No fullnative acceptance is claimed.

Next parent build/run command (NOT RUN here):

```sh
cargo test -p vega_ui --features nsh-reading-integration --test nsh_reading_integration
```

The existing custom runner has no case-filter handling and runs all 15 registrations; the rename case is last. Do not mistake a Rust test-name argument for selective execution or reuse the old executable as post-repair evidence. Parent should rebuild with its existing central toolchain/environment and record the repaired source identity and actual rename outcome. This fixture repair does not require another investigation of the preceding 14 passing cases. Stop writes at this handoff.

## Second mounted execution: command admission repair acceptance

Parent's migration-repair build exited 0 in 37.82 seconds. `target/nsh-closure-repair-reading-rename.stdout` retains the original 14 PASS results; stderr now fails at `rename_tests.rs:67`, stage `acknowledged rename persisted`, after the unchanged ten-second wait. `target/nsh-closure-repair-execution.json` identifies executable SHA-256 `ae4a7d32419b97e95802017a3a963b01993321e35302763041ed3f9be4aeff05`, exit 1, whole-run elapsed 39.512 seconds. The SQL blocker is resolved; no later rename assertion has passed merely because the fixture reached this stage.

Source inspection establishes a concrete production command-admission defect: `render_nsh_editor` creates `NativeAdapter` with `this.image_paste` as its composer-command flag (`text_input/nsh.rs:945-952`), while rename creates `TextInput::new(..., false)` without image-paste capability (`sidebar/threads_block.rs:507-510`). `NativeAdapter::route_key` rejects Return/Escape when that flag is false (`text_input/native_adapter.rs:453-456`), before the bridge can inspect explicit SingleLine policy. Existing adapter single-line tests construct the adapter with the flag true and therefore miss this production configuration. This is not evidence that the rename subscription's readiness or owner gates are incorrect.

Acceptance/minimum plan before Rust repair: let only the current explicit SingleLine policy bypass that legacy adapter admission flag; do not enable image paste, infer policy from multiline, synthesize commands, cache old policy across rebind, replay commands, or weaken bridge/readiness/subscription checks. Add an actual adapter regression with the flag false covering default rejection, acknowledged submit/cancel delivery, pending/composing suppression and policy replacement. Preserve all original mounted assertions and budgets; add bounded timeout state showing controller/native draft, readiness/focus/registration, current session/route, persistence, pending and error state so another failure is actionable. Parent alone rebuilds and executes. No vendor/Cargo/network/Git mutation, independent review or fullnative acceptance is authorized or claimed.

## Second-run repair implementation and handoff

Current effect order, verified from source rather than a new runtime trace:

1. Real host `input::key` checks native composition before `text_input::guarded_key` (`vendor/nsh-platform-gpui/src/input.rs:1363–1387`). `guarded_key` rejects inactive/hidden/secure/noneditable/composing inputs and calls the actual registered hook with current identity and key context (`text_input.rs:964–1001`). A consumed hook returns before engine key dispatch; a legitimate accepted command must not depend on a later engine acknowledgment of that key.
2. The hook enters `NativeAdapter::key` and then `route_key` (`native_adapter.rs:443–501`). The old image-paste-derived command flag rejected ordinary rename keys here. Current code queries `bridge.has_single_line_policy()` at dispatch, not at mount, so default Composer remains unchanged and policy rebind cannot retain a cached form bypass. The unchanged bridge validates native composition, active/attached/focused owner/epoch and settled state before issuing a single-claim token; pending commands are consumed, not replayed.
3. The native channel delivers the token to `dispatch_native_command` (`nsh.rs:1123–1124`, dispatch `:733–792`), which checks token claim/validity, actual focus and `composer_editor_ready`, emits the matching stamped shortcut, then calls `finish_command`. `composer_editor_ready` checks gate/native authority, not command-pending state. `finish_command` only clears command-pending; `validates` does not require that flag (`editing_core.rs:828–848`). Therefore completion itself does not invalidate deferred shortcut delivery. No reason was found to relax these checks, and none were changed. The new adapter regression explicitly preserves token validity after finish.
4. The unchanged rename subscription still verifies captured current input/session/row/thread, project/navigation generation, current visit route, focus/readiness and event route/stamp/token, then invokes original `commit_rename`/`cancel_rename`. Persistence remains `apply_update`; no business-path edits were needed. The original raw failure lacked bounded state, so it cannot establish a second subscription/persistence defect. That uncertainty remains for the rerun rather than being guessed away.

Changes:

- `text_input/editing_core.rs:565–568`: application-module-only current-policy query; no bridge state or validation semantics changed.
- `text_input/native_adapter.rs:453–460`: current explicit SingleLine policy may enter the existing command route even when image paste/composer flag is false. `:701–805` adds `single_line_without_image_paste_delivers_only_current_acknowledged_commands`, exercising the actual adapter and bridge with false flag: default plain/primary Return/Escape pass, pending commands do not deliver/replay, accepted latest text submits, cancel delivers, claim is single-use, finish preserves deferred validity, policy rebind invalidates old token, composing/pending composition-end suppress commands, default policy restoration rejects form commands. Existing `actual_key_translation_keeps_newline_and_single_claim_submit_separate` and single-line assertions remain unchanged.
- Development-only diagnostics under existing `nsh-reading-integration`: `nsh.rs:443–468` captures bounded controller/native text and real serial/selection/composition/policy/gate/readiness/focus/registration/host state; `rename.rs:17–31` captures current binding/route/generations; `rename_tests.rs:60–110` includes those plus real persisted title/pending/error state only when the original timeout fails. Text/title are bounded to 96 characters and error to 256. No new production logs, assertions, retries, clocks, deadlines or budgets were added/changed.

Before → after SHA-256 (paths relative to `crates/vega_ui/src/`):

| File | Before | After |
| --- | --- | --- |
| `text_input/editing_core.rs` | `34f4ec888643473847dc197a5477981023a5844e325cc454d2f70055ba001982` | `5ea00d864cb939b7f7868c4c85443a9dd603cf7804c892e8155a520c8969c1ac` |
| `text_input/native_adapter.rs` | `dab702585357bdf8d15109b3f59eb1a4f6a08bf2b3fe9d0daa7d69b670bfebcc` | `d9144ef7b2e6626b3062e7a28945c06924cde48b38342cf7ea4badb149593155` |
| `text_input/nsh.rs` | `77a0a20601948f032692e685a65c7998a287d8745aef066d391cbf29cc8ad716` | `8f71c554bc9643c50e03f8ad7ad4f4a78dc84ff789c3ffcd9ad88625d621dc59` |
| `sidebar/threads_block/rename.rs` | `820de6499e38cc155a62fe36a5d3ec71d9fd1f2b3127a604451ad3a3c0285b94` | `8b1c895d21ac007faf13925ce4fb755154039cf9f5a7c1d62dbb107ef54a2a06` |
| `sidebar/threads_block/rename_tests.rs` | `c0768ec00bcb4044930cc5e369c549cedd17206f973150e08f383c4e4d80ec97` | `42ada6277b0519fa6d3a54de5a7fca1ac4f79572c6973580897cab1ceb283139` |

Validation: direct rustfmt 1.98.0 toolchain `--edition 2024 --config skip_children=true --check` on all five changed Rust files exited 0 after formatting the new adapter test. No Cargo/rustc/build/test/network/Git mutation, vendor/Cargo/export/harness changes or artifact cleanup occurred here. Prior central 14-case results and earlier native36/palette3 results are historical, not final-version validation. The parent-reported HEAD remains `0b5ea426abfd8d4358489e916273d89474fb190f`; hashes, not HEAD alone, identify this dirty delta.

Next parent commands in the existing central environment (NOT RUN here):

```sh
cargo test -p vega_ui text_input::native_adapter::tests::single_line_without_image_paste_delivers_only_current_acknowledged_commands -- --exact --nocapture
cargo test -p vega_ui text_input::native_adapter::tests::actual_key_translation_keeps_newline_and_single_claim_submit_separate -- --exact --nocapture
cargo test -p vega_ui --features nsh-reading-integration --test nsh_reading_integration
```

Rebuild before using the registered 15-case runner; the runner has no case filter. Existing names/registrations require no changes, and the new adapter unit test is discovered normally. Parent chooses the necessary shared-native regressions after this common adapter change. A fresh rename result must verify persistence and every subsequent cancellation/rebind/route assertion; if it fails, preserve the new bounded stage state before another repair. Development tests/diagnostic helpers remain subject to pre-master removal and source archiving. Final runtime behavior, independent review and fullnative acceptance remain unclaimed. Stop writes at this handoff.
