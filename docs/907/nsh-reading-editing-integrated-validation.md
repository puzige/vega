# Integrated reading and editing development validation

## Footer-notify followup — observer regression and UI green, ordinary mounted still fails convergence

This is **evidence-document implementation, not independent review**. This insertion supersedes only the current-status interpretation of the specific invocations below. All prior text remains verbatim, including the **39.48s lifecycle failures**, **34.87s callback-build / mounted RefCell failure**, **100.164s watchdog timeout without completed counts**, and separate sampled/SIGTERM diagnostic. Those historical failures and incomplete executions are not erased or reclassified. The supplied dirty-worktree base remains `0b5ea426abfd8d4358489e916273d89474fb190f`; it is not a frozen identity for the migration's uncommitted source. Removing just this insertion restores the preceding document SHA-256 `151c6d4120e57b0513b0c13b073bbf9bb99ab34c19e4a7a53fdbbab038000984`, not a self-hash of this revision.

The sole write in this lane is this document. This writer performs no Git, Cargo/rustc/rustfmt, test execution, network operation, sampling, app launch, implementation/other-document write or nested delegation. Parent command outcomes and author static results are explicitly attributed; missing tool-call identities, exact launcher details and execution timestamps are not invented. Parent reports that another author now owns ordinary presentation and mounted diagnostics; no future fix or passing result is certified here.

### Failed macro build, corrected import, successful build

- `target/nsh-closure-footer-notify-build.json:861` records **`recursion limit reached while expanding #[test]`** at `message_footer.rs:916:5`, on **`#[gpui_kit::test]`**. Line **863** records **`build-finished.success=false`**; the paired stderr retains the lib-test compile failure. This is the actual initial failed build, not a runtime regression or an omitted success. Its exit number/duration are not supplied here.
- Parent identifies the cause as local test macro resolution through the glob test import and corrected it with **`use core::prelude::v1::test;`** in the footer-local test module, **not a higher recursion limit**. The read-time source contains that import at **915**, followed by the regression at **918**. This is a harness/build-layer correction, not weakened production assertions.
- Parent reports the corrected import build **exit 0 / 20.49s**. `target/nsh-closure-footer-notify-import-build.stderr:2` confirms **20.49s**; build JSON **866** records **`build-finished.success=true`**. Lines **863** and **865** select the ordinary integration and UI test-profile executables, both `fresh=false`. The full shell argv for these two new build attempts is not captured in the retained streams or supplied verbatim in this followup; no earlier `--tests`/`--lib --test` command is silently relabeled as their exact invocation.
- Corrected-build JSON warnings at **862, 864** retain **`ld: __eh_frame section too large (max 16MB)`** and its possible exception-handling performance impact. Successful compilation is not a warning-free linking claim.

The actual toolchain remains the **parent-verified official Cargo `1.98.0 (797e8a9bc 2026-08-05)`**, **rustc `1.98.0 (88d9e12ae 2026-08-18)`**, and **rustfmt `1.9.0-stable`** (author record: `88d9e12ae1 2026-08-18`). This writer does not re-run version commands, infer a Cargo-fork version, or claim a newly frozen final source version.

### Three actual executions on the corrected-build artifacts

`target/nsh-closure-footer-notify-execution.json` binds each invocation to its exact executable path, SHA-256, argv, exit, elapsed time and stream paths. All three record **`watchdog_timeout=false`**. Elapsed times below are process-invocation durations, not test-harness footer durations. Stream prefixes are `target/nsh-closure-footer-notify-`.

| Invocation | Exact argv | Exit / elapsed | Observed result |
| --- | --- | --- | --- |
| `footer-owner` / `vega_ui` | `conversation_stream::message_footer::tests::owner_feedback_converges_without_consuming_receipts_and_real_changes_revoke_projections --exact --nocapture` | **0 / 1.768s** | `footer-owner.stdout`: **1 passed / 0 failed / 0 ignored / 357 filtered out**, harness duration **0.01s**. The exact new production-observer regression passes; it is not a mounted-controls or native-acceptance result. |
| `ordinary-mounted` / `nsh_ordinary_controls_integration` | `[]` | **1 / 13.922s** | `ordinary-mounted.stdout`: timestamp **PASS**, then actual footer **1 passed / 1 failed / 2 not executed / 0 ignored**. stderr **115** starts `production_buttons_pass_pointer_through_full_inset_wrappers_and_keep_focus_dispatch`; **7797–7798** panics at `ordinary_submissions/presentation/mounted_regressions.rs:202:13`, **`controls timeout: production NSH displayed/layout/host convergence`**; **7802** records that case's FAIL. |
| `ui` / `vega_ui` | `--test-threads=2` | **0 / 0.779s** | `ui.stdout`: **358 passed / 0 failed / 0 ignored / 0 filtered out**, harness duration **0.65s**. This full UI-library group includes the new observer regression. |

The ordinary failure is the fixture's **existing 10-second convergence assertion**, not a new watchdog, a process kill, or a blind retry converted to green. The observed fixture at **195–205** still checks `Duration::from_secs(10)` without a relaxed budget. Its genuine completed runner footer permits the **1/1/2/0** counts for this invocation only; those counts must **not** be backfilled into the earlier watchdog execution, which had no footer. Following the footer source-fingerprint repair, this invocation progresses beyond the former opening/effect-flush deadloop to a different displayed/layout/host-convergence failure. The exact observer regression and full UI pass support that repaired observer boundary, **not a blanket guarantee that mounted ordinary controls now work**. There is still **no final runtime pass for the three ordinary mounted cases**; the two later ordinary cases were not executed in this completed failing run. The responsible remaining layer is under ordinary presentation/mounted diagnosis; the logs do not yet establish its root cause.

| Corrected-build JSON line | Executable relative to worktree | Executed SHA-256 |
| --- | --- | --- |
| 865 | `target/official-nsh-validation/debug/deps/vega_ui-4268349f2e74c4f3` | `a6987e5a52a1e1a5c60487f47c610e831c206c37cc901789c97dd794c4337fad` |
| 863 | `target/official-nsh-validation/debug/deps/nsh_ordinary_controls_integration-880bb28b2b36eb9a` | `e29ebc3afc919ea8e79fbba1b47f8f140fa7dcb8eb48e4fefb2761bfdd7b8a1f` |

Execution JSON uses the absolute root `/Users/puzige/Workspace/worktrees/vega-907-nsh-full/` for these paths. Both actual binary files matched the recorded executed hashes during the read-only observation beginning **2026-11-09T10:20:38.632345+00:00 UTC**. Later builds may overwrite the same paths; recorded executed SHA, not future path contents, identifies these results.

### Strict workspace Clippy passed; final global formatting is not established

The previous `target/nsh-closure-list-lifecycle-clippy.stderr` remains a failed strict check: **one `clippy::collapsible_if`** at `ordinary_submissions/presentation/tests.rs:517`, promoted by `-D warnings`. Parent converted the nested condition to the equivalent let-chain, preserving the assertions. The observed source at **517–522** uses `if let Request::Layout { id, width, height } = request && id.matches(&shown.id) && width == shown.width` before pushing the height; the following height assertion remains present. This is a lint correction, not removal of a failed behavior check.

Parent reports the subsequent **strict workspace/all-targets Clippy**, both features **`vega_ui/nsh-reading-integration,vega/nsh-workspace-integration`**, same previously established offline/locked two-job CLI and target directory `target/official-nsh-validation`, with **`-D warnings`**, **exit 0 / 11.66s**. `target/nsh-closure-footer-notify-clippy.stderr:1–4` shows UI/app/xtask checks and successful `dev`-profile completion in **11.66s**; stdout is empty. The parent attributes the invocation to the same prior CLI; these streams do not independently contain a complete shell command, so no reconstructed exact launcher is asserted. This clears that observed lint blocker locally at its checked snapshot, not the mounted runtime failure or final-head CI gate.

The latest retained full formatting check **did not pass**: `target/nsh-closure-list-current-fmt.stdout` reports a wrapping diff in `message_footer.rs:214`; stderr is empty. Parent reports that the author subsequently applied scoped formatting, and `nsh-message-footer-list-borrow.md:77` records scoped `RUSTUP_TOOLCHAIN=1.98.0 rustfmt --check --edition 2024 --config skip_children=true crates/vega_ui/src/conversation_stream/message_footer.rs` **exit 0**. That author evidence predates the parent's macro-import correction: its recorded footer hash `0f33e53b…` differs from the read-time full-file hash below. It is **not a new final all-workspace formatting receipt**, and no later full-format pass is supplied. Empty streams, author intent, or Clippy success are not substitutes for it.

### Read-time source/document identities, not frozen build inputs

The following hashes identify bytes observed in the same read-only scan beginning **2026-11-09T10:20:38.632345+00:00 UTC**. The ordinary author may continue changing source, and the footer author record describes an earlier snapshot. There is no frozen dirty-input manifest tying these observations to every compiled input; diagnostic line references above belong to the retained logs. These hashes do not transfer execution success to later bytes or pretend that the earlier author hash is current.

| Observed file | SHA-256 |
| --- | --- |
| `crates/vega_ui/src/conversation_stream/message_footer.rs` | `c59d22e53b0a3123f9001c857659a2d63af6e1697232e9b2dba6ea72c5914424` |
| `crates/vega_ui/src/conversation_stream/ordinary_submissions/presentation/tests.rs` | `f901b47c496918f73eee03f0cf881dd7ac3d9ea2f2c217812306ef1ec6aefca0` |
| `crates/vega_ui/src/conversation_stream/ordinary_submissions/presentation/mounted_regressions.rs` | `3494fda898aae81d537c119c1089438992754c0dcec7cd96e182480f561db86c` |
| `docs/907/nsh-message-footer-list-borrow.md` | `6fa902ff0905427d272c31030a7507ec9c24ccc46cdd3bcd27136b76b5b52cab` |

At observation, `OwnerState::capture` reads source/presentation state; **236–243** compare source, reading binding, suspension, foreground, route, appearance and language. `Host::observe_owner` at **273–280** updates the fingerprint and stops/notifies only on a changed presentation. This actual guarded source plus the exact passing regression supersedes the earlier **unconditional footer observer** as the current observed implementation. It does not revive the already-fixed composer selector `same_source` defect, and no new composer change is claimed.

### Retained new evidence SHA-256

All paths below are relative to `target/`. Hashes are actual retained bytes, including empty streams; emptiness alone proves neither execution nor a pass.

| Evidence path | SHA-256 |
| --- | --- |
| `nsh-closure-footer-notify-build.json` | `7c3edbe029d877b406140b7cb6f890f2848c5a9f9ea6b9ceb954b90171ab15e0` |
| `nsh-closure-footer-notify-build.stderr` | `acc47fde98b0cd3bd674c89e191672eef63db528bdd10c70ed8615420d61850e` |
| `nsh-closure-footer-notify-import-build.json` | `9ff8e83df41adfa1486914c61cd8b327d5df8dec27cf891fa631e24da40481bf` |
| `nsh-closure-footer-notify-import-build.stderr` | `5a99e43ed4db35863e80d4ac5a5debb8ec9b915070fd54ea20aa896fd2c9304e` |
| `nsh-closure-footer-notify-execution.json` | `6b56b41e89bd9811f73e5605670113431810b2f19543b493759318f2e6fb7826` |
| `nsh-closure-footer-notify-footer-owner.stdout` | `cace3f99ce162a1849f7fda3f962cb8d0b6672c578765892a99a33cb485c3e56` |
| `nsh-closure-footer-notify-footer-owner.stderr` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `nsh-closure-footer-notify-ui.stdout` | `8977a4fcb4e619368f122a917f25957cecf6c8504eaa5767bc44f112575cbc68` |
| `nsh-closure-footer-notify-ui.stderr` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `nsh-closure-footer-notify-ordinary-mounted.stdout` | `c88728614a88a4b381f9f9527afcd48164b71e586220101e1beb1edc483d70a3` |
| `nsh-closure-footer-notify-ordinary-mounted.stderr` | `738c2750b5600a45aee7aaffdea6beb127ab21f9084f36c50075bf599d3b68b4` |
| `nsh-closure-list-lifecycle-clippy.stdout` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `nsh-closure-list-lifecycle-clippy.stderr` | `a6a752a714152984864d9f65a0e44841dd1a3ca6ef507dfa085e81200d714bb9` |
| `nsh-closure-footer-notify-clippy.stdout` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `nsh-closure-footer-notify-clippy.stderr` | `d0721696f39ba2570d8dbb7b50348af897b7f00b7a3b75dc8d1eb8c1d4ea3c02` |
| `nsh-closure-list-current-fmt.stdout` | `d102535c74c2d56a6a3a65dbe314f63426a77b7ec645ec31c49ce7aef7abc276` |
| `nsh-closure-list-current-fmt.stderr` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |

**Current limitations / READY evidence handoff:** corrected build, exact footer observer **1/1**, full UI **358/358, 0 ignored**, and strict workspace Clippy are observed positives at their stated snapshots. Ordinary mounted remains **exit 1**, with the timestamp passing and the first ordinary case failing its original convergence deadline; the three ordinary cases have no final runtime green. A new full-workspace formatting pass is not established. The ordinary author owns repair/diagnostics, and parent owns subsequent build/regression validation without weakened assertions or budgets. All earlier ten-target greens, diagnostic failures, Python/policy and other workspace evidence retain their original version boundaries; none is promoted into a fresh final all-target pass. No final independent review, exact-final-head required CI, development-test archival/removal or zero-test delivery gate, native OS IME/AX or macOS/Windows/Linux acceptance, merge, packaging, installation or delivery is claimed. Reporting retains the user's **fixed module weights**, actual per-module results/blockers and **no total estimate or invented overall percentage**. This evidence writer stops writes after readback/static integrity verification; READY refers solely to this evidence lane.

## List-lifecycle evidence delta — ten completed green targets, ordinary mounted watchdog timeout

This is **evidence-document implementation, not independent review**. This insertion supersedes current-status claims only for the invocations below. Every preceding entry is retained verbatim below it, including the **39.48s lifecycle failures** and **34.87s final-callback mounted RefCell failure**. Those results remain historical, not erased or relabeled. The supplied dirty-worktree base remains `0b5ea426abfd8d4358489e916273d89474fb190f`; it does not identify every uncommitted source byte. Parent reports that all implementation writers stopped for this build after footer `759bc…`, ordinary `e04f…` and the notice-lifecycle changes. A footer author subsequently resumed repair of the new hang; no future change or runtime outcome is certified here.

The sole write in this lane is this document. This writer runs no Git, Cargo/rustc/rustfmt, test executable, sample command, network operation, app launch, nested delegation, commit, push or implementation/other-document write. Prior parent remote/base observations remain historical; no new fetch or Git status is performed. Tool-call identities not supplied/recovered are not invented.

### Parent build, toolchain and static-check provenance

Parent supplied the exact build command, **exit 0 / 33.77s**:

```sh
CARGO_FORK_AUTO_UPDATE=0 rustup run 1.98.0 cargo test -p vega_ui --features nsh-reading-integration --tests --no-run --offline --locked -j 2 --target-dir target/official-nsh-validation --message-format=json
```

`target/nsh-closure-list-lifecycle-build.stderr:2` records **33.77s**. Build JSON line **882** records `build-finished.success=true`; lines **862, 864, 866, 868, 870, 872, 873, 875, 877, 879, 881** select the eleven executable `profile.test=true` artifacts, all `fresh=false`. This build-only command does not itself execute the tests. Compiler-message warnings at lines **861, 865, 867, 869, 871, 874, 876, 878, 880** retain the `__eh_frame section too large (max 16MB)` linker diagnostic and possible exception-handling performance impact.

Parent reports freshly verified **official Cargo `1.98.0 (797e8a9bc 2026-08-05)`**, **rustc `1.98.0 (88d9e12ae 2026-08-18)`**, and **rustfmt `1.9.0-stable`** (notice-author record includes `88d9e12ae1 2026-08-18`). These tools are not invoked by this writer; no Cargo-fork version is inferred. Parent reports that the initial current formatting check passed **before** the list fixes; `target/nsh-closure-current-fmt.{stdout,stderr}` are both empty retained streams, not standalone exit receipts. Parent separately reports that scoped ordinary formatting and scoped `git diff --check` passed **after format correction**. These are attributed parent static results, not a fresh post-repair/global format certificate, and are not substituted for runtime evidence. The notice-author document retains its own scoped formatting/diff-check evidence. Exact unprovided formatter command lines, process receipts and parent tool-call IDs are not invented.

### Eleven actual parent invocations

`target/nsh-closure-list-lifecycle-execution.json` records exact executable paths, SHA-256, argv, exits, elapsed seconds and stream paths. All targets have `args=[]` except `vega_ui`, whose argv is exactly `--test-threads=2`. The ten completed targets record **exit 0 / watchdog_timeout=false**. The ordinary target instead records **`exit=null`, `watchdog_timeout=true`, elapsed 100.164s**. Times below are invocation elapsed times, not harness footer times. Each stream pair is `target/nsh-closure-list-lifecycle-{target}.{stdout,stderr}`.

| Target | Exit / elapsed | Actual observed result and footer location |
| --- | --- | --- |
| `nsh_ordinary_controls_integration` | **null / 100.164s** | **100s watchdog timeout; no completed runner counts.** stdout:1 contains only `PASS timestamp_native_narrow_height_and_passive_input`; stderr:115 starts `RUN production_buttons_pass_pointer_through_full_inset_wrappers_and_keep_focus_dispatch`, then ends at line 132 without a result footer. No failure count, not-executed count or ignored count is available. |
| `vega_ui` | **0 / 2.925s** | **357 passed / 0 failed / 0 ignored**, 0 filtered; stdout:361. |
| `nsh_reading_integration` | **0 / 29.604s** | **15 passed / 0 failed / 0 not executed / 0 ignored**; stdout:16. Includes the mounted rename acknowledgement/IME/cancel/rebind/persistence PASS at line 15. |
| `nsh_scroll_integration` | **0 / 89.713s** | **16 passed / 0 failed / 0 not executed / 0 ignored**; stdout:17. |
| `nsh_message_anchors_integration` | **0 / 4.097s** | **6 passed / 0 failed / 0 not executed / 0 ignored**; stdout:7, including passive Tab forwarding and unmount/remount. |
| `nsh_native_host` | **0 / 1.057s** | **36 passed / 0 failed / 0 ignored**, 0 filtered; stdout:40. Process-internal native-host regression, not desktop/native acceptance. |
| `nsh_reading_geometry` | **0 / 6.645s** | **24 passed / 0 failed / 0 ignored**; stdout:25. |
| `nsh_palette_integration` | **0 / 4.179s** | **3 passed / 0 failed / 0 unrun / 0 ignored**; **stderr:200**, not stdout (stdout is empty). |
| `nsh_icons_integration` | **0 / 3.430s** | **3 passed / 0 failed / 0 not executed / 0 ignored**; stdout:4. |
| `nsh_settings_integration` | **0 / 4.406s** | **3 passed / 0 failed / 0 ignored**; stdout:4. No separate not-executed count is printed. |
| `nsh_recovery_integration` | **0 / 5.661s** | **2 passed / 0 failed / 0 not executed / 0 ignored**; stdout:3. |

**Timeout/observation correction:** parent reports that the watchdog killed and waited for the ordinary subprocess after its 100s limit. The execution JSON's `null` is the harness-recorded exit value, not a recovered OS exit status and not exit 0 or exit 1. The parent display branch `if code:` treated `None` as false and therefore omitted printing timeout stderr. That is an **observation/harness display defect**; the retained JSON and stderr preserve the timeout and second RUN. Missing console output must not be interpreted as product success, silence, or a completed failure. In particular, **do not transplant the previous run's “1 failed / 2 not executed” summary into this run**. One timestamp PASS is observed, but there is no completed ordinary runner summary and no final runtime green for the three ordinary mounted cases. A timeout alone does not establish a product root cause.

### Separate sampled diagnostic — evidence and current hypothesis, not retry-to-green

After the timeout, parent separately ran the **same ordinary binary** for a bounded hang diagnostic, sampled it at approximately **8 seconds** with **`/usr/bin/sample` for 2 seconds at 1ms intervals**, then terminated it with **SIGTERM after a further 5-second no-completion interval**. Parent supplied diagnostic process metadata **PID 3046 / exit -15**; this is distinct from the eleven-target JSON's `exit=null` watchdog receipt. No separate diagnostic JSON file is present in the inspected `target/nsh-closure-list-lifecycle-hang-diagnostic.*` set, so PID/exit/termination timing are attributed to the parent report, not invented as on-disk JSON. The sample itself independently identifies PID 3046 and executable basename `nsh_ordinary_controls_integration-880bb28b2b36eb9a`; the sample receipt confirms the 2-second/1ms operation completed.

Retained files are `target/nsh-closure-list-lifecycle-hang-diagnostic.{stdout,stderr,sample,sample-receipt}`. Diagnostic stdout again contains only the timestamp PASS; stderr again starts the production-buttons case and ends without a footer (132 lines). The sample header records **2026-11-09 17:59:02.360 +0800**, launch **17:58:54.295 +0800**, macOS **15.8.2 (24H232)**. Those are sample/process timestamps, not fabricated build/test start/end timestamps.

Sample lines **47–51** show `TestAppContext::open_window` → `App::open_window` → `flush_effects` → `apply_notify_effect`. Lines **593–597** include the footer Host owner-observer/render closure and `Context<message_footer::Host>::notify`; lines **1030–1033** include `Window::observe_keyed_state` for that Host and an App notify. The read-time footer source at **413, 438–443** creates keyed state and unconditionally changes projection/intent plus calls `cx.notify()` in its owner observer. Together these support the **current hypothesis of a footer owner-observer → Host notify → keyed-state/parent notification loop during window opening**. This is static call-chain/sampling evidence, **not a completed runtime root-cause proof, verified repair, or passing rerun**. Another author owns the footer repair; no assertion, test count, deadline or acceptance is weakened here.

The already-fixed composer selector source guard remains **present**: `composer_selectors.rs:367–389` has `sync_snapshot` comparing `same_source`, preserves identities for unchanged source, and notifies from the owner observer only when synchronization reports a change. The observed sample is not evidence that this known-fixed composer defect has been revived. Footer evidence and harness-display evidence must remain separate from that earlier defect.

### Executed binary identities and source-observation boundaries

Executable paths below are relative to the supplied worktree root. Each recorded SHA-256 was also compared with the actual file at this writer's read time and **matched**. This does not freeze source files or prevent later parent builds from replacing binaries at the same paths.

| Build JSON line | Executable | Recorded executed SHA-256 |
| --- | --- | --- |
| 872 | `target/official-nsh-validation/debug/deps/nsh_ordinary_controls_integration-880bb28b2b36eb9a` | `0f2531ef33bb91938782ac757ce36fb588894b6d57f176d713c02808ca2502cf` |
| 862 | `target/official-nsh-validation/debug/deps/vega_ui-4268349f2e74c4f3` | `3e747e0c8c66733bb42d99e0b36a132d3c51a56ae9fdd2682c182666eedd7b9e` |
| 868 | `target/official-nsh-validation/debug/deps/nsh_reading_integration-c754697d113fc18b` | `b936a6fd190a26e6198be3762d64769107f1e9bddf883716aeb1895b3132717f` |
| 879 | `target/official-nsh-validation/debug/deps/nsh_scroll_integration-1d1fd85936d176b3` | `a7115b53e4af307dd8517a7cbeca552dc3fa7b7400d070545d55b3fa3c3cab9d` |
| 870 | `target/official-nsh-validation/debug/deps/nsh_message_anchors_integration-b317b2d7635563d8` | `e827129908e9151a3afede084dfc51e64cd3c1397861e7ab262df73910412e1e` |
| 864 | `target/official-nsh-validation/debug/deps/nsh_native_host-c6fd6b079b892425` | `728aa0a8f01d3c2378cba6ac8abfa6b18706d8ee6060089cdcbb30a66e474fb8` |
| 873 | `target/official-nsh-validation/debug/deps/nsh_reading_geometry-da777a5f64c485cf` | `76d894883619356d6709bf504081e1da586ffe9a0d23293efa29d3dee58b01b6` |
| 875 | `target/official-nsh-validation/debug/deps/nsh_palette_integration-c2e61111db3ee3f6` | `dd06f67ab3800601b3f02884107f93c42cdf133dbd9bf65f101f963e725bbc56` |
| 866 | `target/official-nsh-validation/debug/deps/nsh_icons_integration-c7d68528d5563f59` | `8430c4c8acd6a8db20c5b05f5b64f4eada406b59b6e0bb65e062409599eac356` |
| 881 | `target/official-nsh-validation/debug/deps/nsh_settings_integration-c1b0a9b018bd18c8` | `0b61bc539c0a37f75a32be79538146caabbe1503798194e8f6ad29331d7261c0` |
| 877 | `target/official-nsh-validation/debug/deps/nsh_recovery_integration-9271aa7368d76ca1` | `71bbdbb1e67ee19b50fa48fca90ea6679da5f3134472b64ffe8459c2b3987ea0` |

The following are **read-time source/document observations at 2026-11-09T10:06:50.886733+00:00 UTC**, not a reconstructed full build-source manifest. Footer `759bc…` and ordinary presentation `e04f…` match the initial identities supplied by the parent and were still present at this snapshot; subsequent repairs may change them. Notice/CUA/removal bytes match the hashes in the notice author's handoff. Hashing that handoff records its claims, not independent runtime validation.

| Source or author evidence path | Read-time SHA-256 |
| --- | --- |
| `crates/vega_ui/src/conversation_stream/message_footer.rs` | `759bc76d7d4f388b3dc73b7ec4693931d3712140d80b2479e9be76a47cb86c9b` |
| `crates/vega_ui/src/conversation_stream/ordinary_submissions/presentation.rs` | `e04f6b636d10ac40e4bfbe60dddd23323f9e02c3e3aa4c1f99f080b126803bac` |
| `crates/vega_ui/src/conversation_stream/ordinary_submissions/presentation/mounted_regressions.rs` | `3494fda898aae81d537c119c1089438992754c0dcec7cd96e182480f561db86c` |
| `crates/vega_ui/tests/nsh_ordinary_controls_integration.rs` | `071bbc2c452b277e64f6b1e634bb62df44f9d7a1fd5210db1caf9712d0b012d6` |
| `crates/vega_ui/src/conversation_stream/composer_selectors.rs` | `04d769bd42d3b0e12f57d3cb7f43feee4545cc77604acfc72b9dc5af88ad844f` |
| `crates/vega_ui/src/conversation_stream/run_notice/presentation.rs` | `ff2e5b6b27c00b207ac63effb086b81e1bbb6c1d42b49f3489631f58b8efd2fd` |
| `crates/vega_ui/src/conversation_stream/computer_use/presentation/mounted.rs` | `9fa769e46a4885cabd70766dde8266398712fd9036b59a0389c83031064e47c6` |
| `crates/vega_ui/src/conversation_stream/attachments/presentation/removal.rs` | `171b98db67931c6dcc21db9dcbbe1c5b3e7e5c31c223ebf329142c34a34a0938` |
| `docs/907/nsh-notice-list-geometry-lifecycle.md` | `0e8b87e93b3dbc2bc570d825f5ba71d09d9c4f721222e17790005292fdd256b9` |
| `docs/907/nsh-ordinary-list-geometry-lifecycle.md` | `a64c92b009914bcc80e3d77425d20250502ea67173e816b7214d13232d5b2964` |

### Retained raw-evidence SHA-256

Paths in this table are relative to `target/`. No hash of this document is embedded in itself. Build/execution metadata, every invocation stream and all four diagnostic files were read before recording these hashes; large stream inspection was focused on RUN/PASS/failure/footer evidence rather than reproducing multi-megabyte tracing output.

| Evidence file | SHA-256 |
| --- | --- |
| `nsh-closure-list-lifecycle-build.json` | `e4f35b503f72ad72fd9484dff29ea008ece9245ebed520cef4170695afbd1d67` |
| `nsh-closure-list-lifecycle-build.stderr` | `6f1108bb16c9e7ab15b3cd5a2df3fea9ef1d2bad7b79eb9737625123c021fe23` |
| `nsh-closure-list-lifecycle-execution.json` | `e748f7ad5cf5f53e491e9aabcbe03995c6fbe4a9b8bd4bb67f8a59ab18e814ff` |
| `nsh-closure-list-lifecycle-nsh_ordinary_controls_integration.stdout` | `7b7647831029a3790e608f019eabff3c29d1c5a85373a41dea1973c37fa3ff55` |
| `nsh-closure-list-lifecycle-nsh_ordinary_controls_integration.stderr` | `94850fd42bd339203a79dabc392b45a34a6920e52d0955b8e536149678314802` |
| `nsh-closure-list-lifecycle-vega_ui.stdout` | `30ba8bfd6bd167d2127e89265bc2a1aa75f93d14c9e304188177283c28d7a2e5` |
| `nsh-closure-list-lifecycle-vega_ui.stderr` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `nsh-closure-list-lifecycle-nsh_reading_integration.stdout` | `900151850e4c5049cd2ba0de4c94fb8d0b46fe0f032059566bbb6484ef727e32` |
| `nsh-closure-list-lifecycle-nsh_reading_integration.stderr` | `c63e284f72533b0aba9349a590af8ea9b08108e2d6aba0bda53c673eafe119a0` |
| `nsh-closure-list-lifecycle-nsh_scroll_integration.stdout` | `5e3b12f7cb3cbf7b16577739653275717d76285f472a095dd12a58fa538390cf` |
| `nsh-closure-list-lifecycle-nsh_scroll_integration.stderr` | `3d3347c76925aa6d20e401aead7c382baddcb5219e45e7cfb6158fceddda7d97` |
| `nsh-closure-list-lifecycle-nsh_message_anchors_integration.stdout` | `8eb0583d25817b1a6e3edf3593144762f3cf1f289b2e029093bc07ca71ff2c22` |
| `nsh-closure-list-lifecycle-nsh_message_anchors_integration.stderr` | `a70e1688328eec765a3338b26fddf0e617bdce6c9990110f5d7c6413972e6d5e` |
| `nsh-closure-list-lifecycle-nsh_native_host.stdout` | `09e4944c70c34fb83106d0b3b9071d1cc726d65ead41f80e8ddb575166a89b57` |
| `nsh-closure-list-lifecycle-nsh_native_host.stderr` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `nsh-closure-list-lifecycle-nsh_reading_geometry.stdout` | `59600e972c5380677b5f8ff6afbf1bf23dc2b855f8e107108f266316f560297a` |
| `nsh-closure-list-lifecycle-nsh_reading_geometry.stderr` | `85e36a0cb4709c5a55240c40deb7b9a00aa461f37c35f16d7edff133fac783fa` |
| `nsh-closure-list-lifecycle-nsh_palette_integration.stdout` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `nsh-closure-list-lifecycle-nsh_palette_integration.stderr` | `7e9902817ad52c62cc1a6383c44a2b8b68cd6fda7981e669f5eba736bc07a44b` |
| `nsh-closure-list-lifecycle-nsh_icons_integration.stdout` | `9dd4a6c2ee6ecaa71af30d6404b9bb0929572d21e57d100382177679718322c8` |
| `nsh-closure-list-lifecycle-nsh_icons_integration.stderr` | `0a6ce274ce719fbdb1c0da2376771414046f9d25624ee6634bb812c8373c0178` |
| `nsh-closure-list-lifecycle-nsh_settings_integration.stdout` | `b57f873f5d8625dab01193161d46fe1f462a0cec4249c804c7adead0ebb36637` |
| `nsh-closure-list-lifecycle-nsh_settings_integration.stderr` | `6930b9343eda130feb92d9d34903f3c9854b6fb8b9ba151723c3a97e4fc2c0b7` |
| `nsh-closure-list-lifecycle-nsh_recovery_integration.stdout` | `d6a0c06e9d1de2519595bc6a48e6c466141237064ca96d4485408e221e6fb4a3` |
| `nsh-closure-list-lifecycle-nsh_recovery_integration.stderr` | `82989d85139485af0da326454496ff0ecd8f6057c0e39a1d7f10be55bcd45a17` |
| `nsh-closure-list-lifecycle-hang-diagnostic.sample` | `5d1286b8026a7c9d5a69e2a37992788496853f11217a2b46fac4e5ca0b67cd96` |
| `nsh-closure-list-lifecycle-hang-diagnostic.sample-receipt` | `986e4f23b005bb520c0154825fb59dbd0cea860f8bb0d6e2273e0d2abf5397f1` |
| `nsh-closure-list-lifecycle-hang-diagnostic.stderr` | `af290d5653a648db390379ca2f39390850c8d2a0013c2d13c6283ac59de5f20d` |
| `nsh-closure-list-lifecycle-hang-diagnostic.stdout` | `7b7647831029a3790e608f019eabff3c29d1c5a85373a41dea1973c37fa3ff55` |
| `nsh-closure-current-fmt.stdout` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `nsh-closure-current-fmt.stderr` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |

**Current limitations / READY handoff:** ten targets completed green at the recorded binary identities; ordinary mounted instead timed out without completed counts, and the separate SIGTERM diagnostic did not complete. No final runtime pass exists for its three ordinary cases. The footer author must finish the authorized repair, then the parent must bind a new build and relevant regression execution to those repaired bytes, retaining the same cases and assertions. Earlier backend/adapter/full-verifier, workspace, Clippy, Python and policy evidence remains historical at its own snapshots, not newly rerun or certified by this delta. No final complete-migration green, independent implementation review, final-head required CI, development-test cleanup/zero-test delivery gate, native OS IME/AX or macOS/Windows/Linux desktop acceptance, merge, packaging, installation or delivery is claimed. Reporting retains the user's **fixed module weights**, actual per-module results/blockers, and **no total estimate or invented overall completion percentage**. This evidence writer stops writes after document readback and static integrity verification; READY denotes this evidence-lane handoff only.

## Final-callback evidence delta — targeted and full-library greens, mounted footer failure

This is **evidence-document implementation, not independent review**. This insertion supersedes current-status claims for the scopes actually rerun below; all previous sections, including the **39.48s lifecycle build and its failures**, remain verbatim historical evidence. The sole write in this lane is this document. No Git, Cargo/rustc, test binary, network, implementation/other-document write, nested delegation, cleanup, commit, push or app launch is performed here. The supplied dirty-worktree base is **`0b5ea426abfd8d4358489e916273d89474fb190f`**, not a freshly queried HEAD or a frozen identity of the large uncommitted migration. Parent reports that it just fetched origin, task remote equals task HEAD, and master remains at the reported `0b3dcd..` prefix; this lane does not independently query or extend those Git observations.

### Actual parent build and four recorded invocations

Parent supplied this exact successful build command (**exit 0**, **34.87s**):

```sh
CARGO_FORK_AUTO_UPDATE=0 rustup run 1.98.0 cargo test -p vega_ui --features nsh-reading-integration --lib --test nsh_ordinary_controls_integration --no-run --offline --locked -j 2 --target-dir target/official-nsh-validation --message-format=json
```

Retained files are `target/nsh-closure-final-callback-build.json` and `.stderr`. Stderr line **2** records 34.87s; JSON line **866** records `build-finished.success=true`. JSON lines **862, 865** select exactly **two executable `profile.test=true` artifacts**, both `fresh=false`; this was a build-only command, not test execution. JSON warning records **861, 864** retain `__eh_frame section too large (max 16MB)` and possible exception-handling performance impact. The established parent toolchain remains Cargo **`1.98.0 (797e8a9bc 2026-08-05)`** / rustc **`1.98.0 (88d9e12ae 2026-08-18)`**, not queried again here; the supplied command explicitly selects Rust 1.98.0 and disables Cargo-fork auto-update. No new fork-version probe, wall-clock start/end timestamp or parent tool-call identity was supplied/recovered for this delta; none is invented.

`target/nsh-closure-final-callback-execution.json` records the following **four actual invocations**, including exact artifact paths/hashes, argv, exits, elapsed seconds and log paths. All record **`watchdog_timeout=false`**; no watchdog duration is inferred. Each stream pair below is `target/nsh-closure-final-callback-{invocation}.{stdout,stderr}`. Times are invocation elapsed times from the JSON, not the shorter harness footer times.

| Invocation / target | Exact argv after executable | Exit / elapsed | Actual result and boundary |
| --- | --- | --- | --- |
| `ordinary-press` / `vega_ui` | `conversation_stream::ordinary_submissions::presentation::tests::production_press_retains_displayed_attempt_and_replaced_handlers_cannot_launder_authority --exact --nocapture` | **0 / 2.274s** | **1 passed / 0 failed / 0 ignored / 356 filtered**. Retained original callbacks actually return after disposal, with zero stale activation. Numeric nodes **13 and 19 are reused**, not deleted identities; events addressed to those current nodes correctly carry replacement authority. See the callback/source boundary below. |
| `timestamp-center` / `vega_ui` | `conversation_stream::timestamps::presentation::tests::production_separator_preserves_localized_text_weight_center_padding_and_natural_height --exact --nocapture` | **0 / 0.134s** | **1 passed / 0 failed / 0 ignored / 356 filtered**. Corrected geometry assertion checks nearest-pixel centered origin and adjacent spans, not impossible exact equality of margins for an odd-width span. English remains **left 190 / right 291 / width 480**; Chinese is **211 / 269 / 480**. `surface_frame=None` remains diagnostic, not a failure. |
| `ui` / `vega_ui` | `--test-threads=2` | **0 / 0.785s** | **357 passed / 0 failed / 0 ignored / 0 filtered**. This is the full library result on this compiled artifact, not merely the sum of the two targeted greens or final-head acceptance. |
| `ordinary-mounted` / `nsh_ordinary_controls_integration` | `[]` | **1 / 3.791s** | **1 passed / 1 failed / 2 not executed / 0 ignored**. `timestamp_native_narrow_height_and_passive_input` passes. The next case, `production_buttons_pass_pointer_through_full_inset_wrappers_and_keep_focus_dispatch`, fails with a production ListState borrow reentry; neither later ordinary case executes. |

### Exact artifact identities and diagnostic failure

Build JSON and execution JSON agree on these executable paths, relative to the worktree root. This writer also hashed the actual files at read time: **both still matched** the recorded execution hashes. That equality identifies these binaries, not every concurrently changing source byte.

| Executable | SHA-256 |
| --- | --- |
| `target/official-nsh-validation/debug/deps/vega_ui-4268349f2e74c4f3` | `dc03ef55e14d14d8797b56857d9a6c292778c47a3c7875926701055252ecec53` |
| `target/official-nsh-validation/debug/deps/nsh_ordinary_controls_integration-880bb28b2b36eb9a` | `af3bc65f76755bfa2ac3dced9ce9e9ccdd162d47f02a756c030c5cdb90cc3f20` |

The first mounted failure is retained at `target/nsh-closure-final-callback-ordinary-mounted.stderr:128–131`: `RefCell already mutably borrowed`, GPUI Fast 0.1.9 `elements/list.rs:822:16`. Parent then ran the **exact same runner with `RUST_BACKTRACE=1`** as a diagnostic, not a blind retry seeking green. `target/nsh-closure-final-callback-mounted-backtrace.stdout` repeats the timestamp PASS and **1 passed / 1 failed / 2 not executed / 0 ignored**. Its stderr **128–146** identifies `ListState::viewport_bounds` → production `message_footer::Mounted::prepaint` at **`crates/vega_ui/src/conversation_stream/message_footer.rs:708:45`**; frames **89** and preceding `StateInner::prepaint_items` show enclosing List prepaint. The observed footer source reads `stream.list.viewport_bounds()` during child prepaint. This is a **production footer/List lifecycle defect**, not a font/geometry assertion failure or a passing diagnostic run. The four-entry execution JSON does not contain a separate diagnostic receipt; its exact elapsed/exit metadata was not supplied, so it is not invented here.

There is **no final runtime pass for the three ordinary mounted cases**: the production-buttons case failed, while `retry_and_same_attempt_fact_changes_reject_old_displayed_controls` and `route_settings_geometry_and_unmount_reject_captured_controls` were not executed. Another author owns the footer repair. This document neither implements nor predicts that repair; a later source change does not turn these recorded failures into passes. Parent must coordinate a fresh build and relevant mounted execution after that author's repair, without dropping these cases or weakening assertions.

### Read-time callback/geometry source observations

The corrected test seam and geometry source were already present; this writer changes neither. `ordinary_submissions/presentation/tests.rs:103–118,137–146` retains the actual production press closure and invokes saved callbacks before reporting owner liveness. Its test at **357–509** checks real displayed-attempt activation, newly owned replacement listeners, current authority when numeric IDs are reused, all original callbacks returning with owners disposed, and an empty stale-activation channel. Stderr explicitly records `original_nodes=[(13, "Check status"), (19, "Copy diagnostics")]` and the **same** replacement node IDs, with **2 original / 2 replacement callbacks**. `pixels.rs:169–199` retains command/presentation-owner liveness, revision and measurement checks around real command dispatch; the seam wraps the production closure only under `#[cfg(test)]`. This is not an assertion that reused numeric IDs must reject current input.

`timestamps/presentation/tests.rs:120–133` derives continuous centered origin, asserts `(continuous_left + 0.5).floor()` and the combined right edge, and preserves span adjacency. For the English width **101**, continuous left **189.5** rounds to **190**, leaving **190 / 189** margins; this explains the previous failing zero-difference assertion without claiming a raster or native acceptance result. These observations describe read-time source, not an independently frozen complete source manifest for the build.

| Read-time source path | SHA-256 |
| --- | --- |
| `crates/vega_ui/src/conversation_stream/ordinary_submissions/presentation/pixels.rs` | `3f1e1ef6ece0e0c0d39c00eccc0d50d7ccb1cc2d56cf7bf9fbd3ebf398b13e1d` |
| `crates/vega_ui/src/conversation_stream/ordinary_submissions/presentation/tests.rs` | `415720cd42d7adbf5124a21beca791ce34439212e316fe4d157268e4252d2670` |
| `crates/vega_ui/src/conversation_stream/timestamps/presentation/tests.rs` | `ab9bf3ff29a0ca440cb339d04d49b2ec5fdef6d872e3ead6580bb0b9fc231f65` |
| `crates/vega_ui/tests/nsh_ordinary_controls_integration.rs` | `071bbc2c452b277e64f6b1e634bb62df44f9d7a1fd5210db1caf9712d0b012d6` |
| `crates/vega_ui/src/conversation_stream/message_footer.rs` | `a4724b2a44191b714112f985da75d699289f59e47b332a75414f6e362c89fce1` |

Footer identity is an observation **before any later separately owned repair**, not a final stopped implementation identity. During this document's final static check, that file had changed: a focused second read observed SHA-256 **`759bc76d7d4f388b3dc73b7ec4693931d3712140d80b2479e9be76a47cb86c9b`**, with prepaint lines **704–717** now obtaining `selection_menu::mounted::list_bounds` and using the returned `list` for clipping/geometry. The historical failure-site hash in the table is deliberately preserved, not overwritten with later bytes. This is an observed parallel source change, **not a repair review or a fresh compile/runtime pass**; no validation of the later footer bytes was supplied in this handoff. The source hashes do not retroactively bind the full dirty tree to the compiled binaries.

### Retained final-callback evidence SHA-256

Paths below are relative to `target/`; hashes cover complete actual retained files, including empty stderr. Build JSON was parsed for artifact/completion/warning records; large diagnostic stderr was inspected at failure and relevant stack ranges rather than dumped wholesale.

| Retained file | SHA-256 |
| --- | --- |
| `nsh-closure-final-callback-build.json` | `cee06eb0868890c5fe0d8507744116db7686b2cdfe1b10fdcfaf008c7fcf86cc` |
| `nsh-closure-final-callback-build.stderr` | `c9bc1c84021192445c3a0de0855977e5466e496297c83e606eedbcc9c10b8c01` |
| `nsh-closure-final-callback-execution.json` | `37d1ae616bada63357b3d498d513ecd3d8a08bb421acd2e1087f7a4133724b12` |
| `nsh-closure-final-callback-ordinary-press.stdout` | `80c1a6cada0b06657eeb70fe205e17a733947a6b1b3317e114c91de5d6732913` |
| `nsh-closure-final-callback-ordinary-press.stderr` | `7aefb78e6594215789a8beb568752db210083fcf644a59babb23dc41a871725a` |
| `nsh-closure-final-callback-timestamp-center.stdout` | `b33f0cee0bab018a12c40fa1f540a2c5871058f7d75bae319a8b4cfe167aa5ad` |
| `nsh-closure-final-callback-timestamp-center.stderr` | `60a34885127a71431fb9ec23517bc6843b85a5be533ddc6276eb10788e599dfc` |
| `nsh-closure-final-callback-ui.stdout` | `21fadaab7efa6c27ce3f3bfcc1ad9a4456d58ea856bb86720becfb23d6ee697c` |
| `nsh-closure-final-callback-ui.stderr` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `nsh-closure-final-callback-ordinary-mounted.stdout` | `c88728614a88a4b381f9f9527afcd48164b71e586220101e1beb1edc483d70a3` |
| `nsh-closure-final-callback-ordinary-mounted.stderr` | `9c7e740504c998ce9267de1bc41fcae217460da998933201bb48f92a660742a1` |
| `nsh-closure-final-callback-mounted-backtrace.stdout` | `c88728614a88a4b381f9f9527afcd48164b71e586220101e1beb1edc483d70a3` |
| `nsh-closure-final-callback-mounted-backtrace.stderr` | `5fa6d5a7beb83a85114dc1ef453ba3867a638dd0922fe19a718d9199ba6dfe12` |

**Remaining boundary / READY handoff:** ordinary-press **1/1**, timestamp-center **1/1**, full UI **357/357, 0 ignored**, and the mounted timestamp PASS are positives at the recorded binary identities. The three ordinary mounted cases have no final runtime green. Rename **15/15**, anchors **6/6**, adapter **1/1**, backend CURRENT/full-verifier attribution, native/IME **36/36**, palette **3/3**, and earlier workspace/Clippy/Python/policy results retain their own historical snapshots; none was rerun or newly certified by this delta. Final implementation review by a non-implementer, exact-final-head required CI, development-test archival/removal and native OS IME/AX plus macOS/Windows/Linux desktop acceptance remain separate pending gates. No full migration completion, merge, packaging, installation or native acceptance is claimed. Reporting preserves the user's fixed module weights and uses actual results/blockers only, with **no total estimate or invented completion percentage**. This evidence writer stops writes after document readback/static checks and exact document-hash handoff; the parent and footer author continue their own authorized lanes.

## Closure lifecycle evidence delta — partial greens after `28225cb7`

This is **documentation implementation, not independent review**. Only this document is written, with all historical sections and failures preserved verbatim. Removing this inserted section restores SHA-256 **`28225cb73be8f461aa06fe18cf8b219efaf8b59cd17ed5657d8774d98c281c39`**. This lane runs no Cargo/rustc, test binaries, network, Git mutation (including fetch/update), source/other-document writes or cleanup. Base **`0b5ea426abfd8d4358489e916273d89474fb190f`** remains previously reported context, not a freshly queried HEAD or a frozen identity for the dirty inputs.

### Actual lifecycle build and six invocations

`target/nsh-closure-lifecycle-build.stderr` line **2** records **39.48s** test-profile completion; build JSON line **870** records `build-finished.success=true`, consistent with the parent's reported **exit 0**. The JSON contains **four** executable `profile.test=true` artifacts, all `fresh=false`. Its warning records at lines **861, 864, 866, 868** retain `__eh_frame section too large (max 16MB)` and possible exception-handling performance impact; these warnings were not suppressed. The established parent toolchain is Cargo **`1.98.0 (797e8a9bc 2026-08-05)`** / rustc **`1.98.0 (88d9e12ae 2026-08-18)`**, not queried again here. The exact build shell command/environment is **unavailable in this delta and retained build files**; earlier commands are not represented as this run.

`target/nsh-closure-lifecycle-execution.json` records **six** actual invocations, with exact executable, args, binary SHA-256, exit, elapsed time and stdout/stderr paths. The argv suffixes below are taken directly from that metadata, not reconstructed Cargo commands. All six record **`watchdog_timeout=false`**. The parent's previously reported 120s watchdog was not reached; this JSON does not itself record the timeout duration. Each log pair is `target/nsh-closure-lifecycle-{invocation}.{stdout,stderr}`.

| Invocation / target | Exact argv after executable | Exit / elapsed | Actual result and boundary |
| --- | --- | --- | --- |
| `ordinary-press` / `vega_ui` | `conversation_stream::ordinary_submissions::presentation::tests::production_press_retains_displayed_attempt_and_replaced_handlers_cannot_launder_authority --exact --nocapture` | **101 / 2.317s** | stdout **10**: **0 passed / 1 failed / 0 ignored / 356 filtered**. stderr **2–3**: `ordinary_submissions/presentation/tests.rs:368:5`, `disposed listeners cannot activate`. The previous disposed-Signal panic at `pixels.rs:188` is absent in this run, but the final listener-activation assertion still fails. No stale-callback/node-ID-reuse diagnosis is asserted as established fact. |
| `timestamp-center` / `vega_ui` | `conversation_stream::timestamps::presentation::tests::production_separator_preserves_localized_text_weight_center_padding_and_natural_height --exact --nocapture` | **101 / 0.145s** | stdout **10**: **0 passed / 1 failed / 0 ignored / 356 filtered**. stderr **1, 3–4** now reaches actual geometry and the assertion at `timestamps/presentation/tests.rs:120:13`: **left 190 / right 291 / separator width 480**, giving margins **190 / 189**, difference **1**. `surface_frame=None` remains diagnostic data but no longer causes the earlier diagnostic `expect` panic. This is **not a pass** or an accepted raster-centering contract change. |
| `singleline-adapter` / `vega_ui` | `text_input::native_adapter::tests::single_line_without_image_paste_delivers_only_current_acknowledged_commands --exact --nocapture` | **0 / 0.129s** | stdout **5**: **1 passed / 0 failed / 0 ignored / 356 filtered**; stderr empty. The library snapshot therefore accounts for **357 tests**, not 355, and only this one test passes in this invocation. Filtered tests are not executed and this is not a full-library green. |
| `reading-rename` / `nsh_reading_integration` | `[]` | **0 / 29.626s** | stdout **1–15**: **15 PASS**; footer **16**: **15 passed / 0 failed / 0 not executed / 0 ignored**. Original fourteen cases remain individually green, including mount ABA, manual scroll, selection, production-body and recovery-loop cases; **15** is `mounted_rename_native_ack_ime_cancel_rebind_and_persistence`. This entire rename behavior case now passes, superseding both the SQL-fixture and acknowledged-persistence failures only at this compiled identity. |
| `anchors` / `nsh_message_anchors_integration` | `[]` | **0 / 3.826s** | stdout **1–6**: **6 PASS**; footer **7**: **6 passed / 0 failed / 0 not executed / 0 ignored**. Both the former narrow-layout case and `clipped_passive_leaves_forward_input_once_and_skip_tab_focus` pass, followed by unmount/remount. The previous fifth-case Tab failure and cleanup abort remain historical, not current for this binary. |
| `ordinary-mounted` / `nsh_ordinary_controls_integration` | `[]` | **-6 / 3.936s** | stdout is **empty**. stderr **1** starts only `timestamp_native_narrow_height_and_passive_input`; **112–113** fails at `timestamps/presentation/mounted_regressions.rs:378:9`, `timestamp must not add a tab stop`. **116–117** then panics in GPUI test cleanup (`test_context.rs:871:14`, `window not found`); **219–221** records destructor-cleanup/non-unwinding panic and abort. **SIGABRT (-6), not ordinary exit 1**; no PASS/FAIL summary/footer or later-case RUN appears. One first-case assertion failure is observed, **0 PASS markers**, but footer totals are **unavailable**, not a fabricated `0/1/3` runner result. |

The ordinary-mounted first case progresses beyond the previous wrapped-natural-height assertion at line **285** before failing on Tab. A focused later source read places natural-height, input-mask, text/padding/centering and pointer/wheel checks before line 378; this supports the observed later stopping boundary, **not a first-case PASS**. Only that first case is recorded as started; the other three named cases have no actual execution evidence. Because cleanup aborts before the runner's catch/summary can complete, no footer count is invented. This ordinary timestamp fixture remains distinct from the now-green anchors fixture.

Per parent, rename and anchors authors have stopped and their production-path cases are green at the compiled identities above. C1/C2 work continues on node-ID reuse versus stale callbacks, the raster-centering contract and the ordinary timestamp Tab fixture; these are ongoing diagnoses, not conclusions or additional work performed by this writer. **No broad latest UI-library group has run after the shared native-adapter change.** The three targeted library invocations do not replace that missing regression. There is no global green aggregate, and no new policy, workspace, Python, native-host or palette run in this delta.

### Exact artifacts and contemporaneous source observations

Executable paths have prefix `/Users/puzige/Workspace/worktrees/vega-907-nsh-full/target/official-nsh-validation/debug/deps/`. Each path below matches both the build JSON artifact and execution metadata. The read-only scan ran **2026-11-09T09:12:47.144484+00:00–09:12:47.952164+00:00 UTC**. Every path matched its recorded execution SHA at observation time; later compilation may overwrite a path. The recorded executed SHA, not any later path contents, identifies the result.

| Build JSON line / executable basename | Executed SHA-256 | Path observation |
| --- | --- | --- |
| **862** / `vega_ui-4268349f2e74c4f3` | `95b595e95eb0e706a7616bcf12a5355908492bf91a09fd97bdba76e3255aabe5` | matches; shared by the three targeted library invocations |
| **865** / `nsh_reading_integration-c754697d113fc18b` | `e3afb854f1cf36d1647997efce1430ef810f9d851960e636b44b0f377592e039` | matches |
| **867** / `nsh_ordinary_controls_integration-880bb28b2b36eb9a` | `e78eeb05f760f0cb4e7a79229a9af34e1497817f565385f3092ecdf6e7c0887e` | matches |
| **869** / `nsh_message_anchors_integration-b317b2d7635563d8` | `31bce9befdca7afd331184896245f036a81ad6a7ccb2040042f4b35204eda1a1` | matches |

The following full-file hashes identify **later observed source bytes**, not proven compiled inputs. No frozen dirty-input manifest was recovered. Parent/author lanes may mutate after compilation or this scan; source observation does not transfer these failures or greens to newer bytes, and diagnostic line numbers can move.

| Later observed source | SHA-256 |
| --- | --- |
| `crates/vega_ui/src/conversation_stream/ordinary_submissions/presentation/pixels.rs` | `ae8b19137e8175b27882b7247c3c59307f312db90d31b5da4450b11ebb0c60a1` |
| `crates/vega_ui/src/conversation_stream/ordinary_submissions/presentation/tests.rs` | `036aa7b55b5c3767f06bf5c0d1d87329f7bbaf1d52b6622d253771d0b9d3642a` |
| `crates/vega_ui/src/conversation_stream/timestamps/presentation/tests.rs` | `4fde23c6d8d8923ff2b6f9f616431a714f104eeec4ab583eef8e388a3799da68` |
| `crates/vega_ui/src/conversation_stream/timestamps/presentation/mounted_regressions.rs` | `98234869f190f9e7a78b439f1d525878504bbf5f625dd1a66fdce7c98270a8b9` |
| `crates/vega_ui/src/conversation_stream/message_anchors/mounted_regressions.rs` | `eca2fa0802e5689483952f8b8ac46b403fe47043b95976922fffbe620905e17e` |
| `crates/vega_ui/src/sidebar/threads_block/rename_tests.rs` | `42ada6277b0519fa6d3a54de5a7fca1ac4f79572c6973580897cab1ceb283139` |
| `crates/vega_ui/src/text_input/native_adapter.rs` | `d9144ef7b2e6626b3062e7a28945c06924cde48b38342cf7ea4badb149593155` |
| `crates/vega_ui/tests/nsh_reading_integration.rs` | `f6435b362fa66975caccd6b15e0fbd1097fbe5fed3bf197d0f5476b0330a4d53` |
| `crates/vega_ui/tests/nsh_message_anchors_integration.rs` | `64c27a5b7f5d11dfd2c858e38fcd29db093bf716b467c7dab075251d9c24382a` |
| `crates/vega_ui/tests/nsh_ordinary_controls_integration.rs` | `071bbc2c452b277e64f6b1e634bb62df44f9d7a1fd5210db1caf9712d0b012d6` |

### Backend and remaining validation boundaries

Backend ledger remains **CURRENT at its refreshed backend inventory boundary**, per parent; observed `docs/907/nsh-backend-current-manifest.json` SHA-256 is unchanged: **`5083c6a877934019ad3405d5db560857495e305f43244ea41a6ac280c177b41c`**. The prior full-verifier pass remains parent-attributed, with no newly supplied exact verifier command/log and no rerun by this lane. Native/IME **36/36** and palette **3/3** remain green at their preceding compiled snapshot, not additional lifecycle runs, native OS IME/AX acceptance, or certification of every later UI source byte. Historical STALE sections remain historical. NSH **0.4.0**, Kit **0.7.3**, Fast / Fast platform **0.1.9** are unchanged.

### Retained lifecycle evidence SHA-256

Paths below are relative to `target/`; hashes cover complete retained files, including empty streams. The 48,816-line reading/rename stderr was not dumped wholesale; passing counts come from its actual stdout/footer and invocation metadata.

| Retained file | SHA-256 |
| --- | --- |
| `nsh-closure-lifecycle-build.json` | `7fc23ed0c4bb83ca2a45ad80be8d696ed398e54ebe8f7a7b24ee1c5b15f7c8e4` |
| `nsh-closure-lifecycle-build.stderr` | `2c8af58cabd209554dfd2c06c9b8bb4187c336995fc8401aa558534c923a9b04` |
| `nsh-closure-lifecycle-execution.json` | `96bced7d225daf6d89cdc1edab297195de935190ba47bd84081db809f72c9855` |
| `nsh-closure-lifecycle-ordinary-press.stdout` | `1e26794b23010bfea897de3a4bbd5438d453f43d18ef2b915f3d9607ae8767a7` |
| `nsh-closure-lifecycle-ordinary-press.stderr` | `115f549c87044c0fc8f2f8cb57d2bf2d6e439d98bf779662fde24e18d3168871` |
| `nsh-closure-lifecycle-timestamp-center.stdout` | `09b57d6ca2c31582637578e185003fa6afbab66d23bbad80d2d6be875422ac7d` |
| `nsh-closure-lifecycle-timestamp-center.stderr` | `6833fa668d70a706185a19daf0796533f26fe09c16828b3b591c4d12f940f3e2` |
| `nsh-closure-lifecycle-singleline-adapter.stdout` | `ba87216fc9f82738af94d328ecabe29c8ee1bdcd5a7e6fdcd033e6fce3ccfe05` |
| `nsh-closure-lifecycle-singleline-adapter.stderr` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `nsh-closure-lifecycle-reading-rename.stdout` | `900151850e4c5049cd2ba0de4c94fb8d0b46fe0f032059566bbb6484ef727e32` |
| `nsh-closure-lifecycle-reading-rename.stderr` | `940142d43d4c21930bb9bec15f34221ac3d33a02434d26760c6f30365c6294da` |
| `nsh-closure-lifecycle-anchors.stdout` | `8eb0583d25817b1a6e3edf3593144762f3cf1f289b2e029093bc07ca71ff2c22` |
| `nsh-closure-lifecycle-anchors.stderr` | `e7addd5b9d1dba8273fde2d3bd246acd1280e6461264fcabbb9e69bb10b8a8ba` |
| `nsh-closure-lifecycle-ordinary-mounted.stdout` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `nsh-closure-lifecycle-ordinary-mounted.stderr` | `b98dbf664df574387a60d751fecd3ed019d206b969e35a5343aa7823f046116f` |

**Stopped boundary:** rename **15/15**, anchors **6/6** and the targeted adapter **1/1** are positives at the recorded compiled identities; ordinary-press, timestamp centering and ordinary-mounted remain failed/incomplete as above. Fresh parent validation must cover subsequent repairs and the latest broad library snapshot. No assertions, budgets or acceptance contracts are weakened by this evidence update. Earlier policy/workspace/Clippy/Python greens retain their own snapshot boundaries. Independent final review, final-head required CI, native OS IME/AX and three-desktop native acceptance remain pending; no full migration or current-final-head success is claimed. This evidence writer stops writes after readback/static checks and exact document SHA handoff, without overlapping the parent or author implementation lanes.

## Closure repair evidence delta — failures after `8cfe47cc`

This is **documentation implementation, not independent review**. Only this document is written; all preceding sections and raw failure history are preserved verbatim. Removing this inserted section restores document SHA-256 **`8cfe47cc628247366a447d53afdef54393d9df667106c10aea3fcd67ce0b7edb`**. This lane runs no Cargo/rustc, tests, network, Git mutation, source/other-document writes or cleanup. The explicit continuation restriction excludes fetch/update. Base **`0b5ea426abfd8d4358489e916273d89474fb190f`** remains prior parent-reported context, not a freshly queried HEAD or frozen dirty-source identity.

### Actual repair build and five invocations

Parent reports fixture task-wake, migration and observation-order corrections, then **exit 0 / 37.82s** for `target/nsh-closure-repair-build.{json,stderr}`. Actual stderr line **2** records the 37.82s test-profile completion; build JSON line **870** records `build-finished.success=true`. This build contains **four** executable `profile.test=true` artifacts, all `fresh=false`, not another eleven-target build. Four retained linker warnings at JSON lines **861, 864, 866, 868** say `__eh_frame section too large (max 16MB)` and warn of possible exception-handling performance impact; they were not suppressed. The previously established parent toolchain remains Cargo **`1.98.0 (797e8a9bc 2026-08-05)`** / rustc **`1.98.0 (88d9e12ae 2026-08-18)`**, not queried by this writer. The exact build shell command/environment is **unavailable in this delta and retained build files**; no historical command is relabelled as this execution.

`target/nsh-closure-repair-execution.json` records exactly five actual invocations, including executable, argv, binary SHA-256, exit, elapsed time and log paths. All five have **`watchdog_timeout=false`**; none reached the parent's 120s watchdog. That watchdog bound is parent-supplied context, not a timeout field in this JSON. These are failures at this compiled snapshot, **not final-source PASS**, and filtered library tests are not unexecuted mounted cases.

| Invocation / exact target | Actual argv after executable | Exit / elapsed | Actual result and failure boundary |
| --- | --- | --- | --- |
| `ordinary-press` / `vega_ui` | `conversation_stream::ordinary_submissions::presentation::tests::production_press_retains_displayed_attempt_and_replaced_handlers_cannot_launder_authority --exact --nocapture` | **101 / 2.426s** | **0 passed / 1 failed / 0 ignored / 355 filtered**; stdout line **10**. The former request-count-zero failure is no longer the stopping point. stderr **2–3**: `ordinary_submissions/presentation/pixels.rs:188:48`, `a signal read or written after its owner was disposed`; stderr **6–7**: `presentation/tests.rs:368:5`, `disposed listeners cannot activate`. Parent is diagnosing the stale-handler defect; no root-cause fix or pass is inferred. |
| `timestamp-center` / `vega_ui` | `conversation_stream::timestamps::presentation::tests::production_separator_preserves_localized_text_weight_center_padding_and_natural_height --exact --nocapture` | **101 / 0.147s** | **0 passed / 1 failed / 0 ignored / 355 filtered**; stdout line **10**. stderr **2–3**: `timestamps/presentation/tests.rs:113:65`, `surface geometry`. The `core.measure(...).expect("surface geometry")` diagnostic panics **before the original coordinate assertion**. This attempt does not classify actual centering as correct or incorrect. |
| `reading-rename` / `nsh_reading_integration` | `[]` | **1 / 39.512s** | stdout **1–14**: **14 PASS**; footer **15**: **14 passed / 1 failed / 0 not executed / 0 ignored**. stderr **47633–47636**: `sidebar/threads_block/rename_tests.rs:67:13`, `rename stage: acknowledged rename persisted`; `FAIL mounted_rename_native_ack_ime_cancel_rebind_and_persistence`. The prior `no such table: projects` stopping point is resolved, **not all rename behavior**. The fixture stage deadline is **10s** (observed source `wait` assertion); **39.512s is the whole invocation**, not that stage's duration. The stage label is not evidence that persistence completed. |
| `anchors` / `nsh_message_anchors_integration` | `[]` | **-6 (SIGABRT) / 4.082s** | stdout **1–4**: **4 PASS**, including the previously failing narrow CJK/emoji preview. stderr **145–149**: fifth case `clipped_passive_leaves_forward_input_once_and_skip_tab_focus` starts, then `message_anchors/mounted_regressions.rs:773:9`, `Tab stopped at passive Surface`. stderr **152–153**: cleanup panic at GPUI Fast `app/test_context.rs:871:14`, `window not found`; **255–257**: destructor-cleanup panic and abort. **No completed runner footer/counts**; this is not ordinary exit 1. Case six `unmount_stops_both_surfaces_and_remount_creates_new_identity` is unexecuted (ordered six-case runner plus no sixth RUN/PASS). Four passes and a fifth-case assertion are observed, but final runner failure/ignored counters are unavailable. |
| `ordinary-mounted` / `nsh_ordinary_controls_integration` | `[]` | **1 / 3.810s** | stdout **1**: **0 passed / 1 failed / 3 not executed / 0 ignored**. stderr **112–113**: `timestamps/presentation/mounted_regressions.rs:285:5`, `long timestamp must use actual wrapped natural height`; **119**: `FAIL timestamp_native_narrow_height_and_passive_input`. This remains an actual mounted-height failure, distinct from the diagnostic panic in `timestamp-center`. |

The five log pairs are exactly `target/nsh-closure-repair-{ordinary-press,timestamp-center,reading-rename,anchors,ordinary-mounted}.{stdout,stderr}`. Assertion locations describe the compiled run and can move as author lanes repair. There is no global green total and no new full-library, policy or other mounted-suite execution in this delta. Parent-author lanes continue C1/C2, anchor Tab and rename-persistence repairs; this evidence writer does not overlap those implementation scopes or infer their outcomes.

### Exact compiled artifact identities and later observations

All executable paths below have prefix `/Users/puzige/Workspace/worktrees/vega-907-nsh-full/target/official-nsh-validation/debug/deps/`. Build JSON selects the same exact paths recorded by the execution metadata; the two targeted library invocations share one binary. SHA-256 values are the **recorded executed binary identities**. A read-only scan beginning **2026-11-09T08:56:57.345852+00:00 UTC** found each path matching its recorded identity at observation time, not a promise that parallel builds cannot replace it later.

| Build JSON line / executable basename | Executed SHA-256 | Contemporaneous path observation |
| --- | --- | --- |
| **862** / `vega_ui-4268349f2e74c4f3` | `fd100c50a8b596fc8987109e13235ca16c1245ae6c3261be5ead53ac476efaf2` | matches; used by ordinary-press and timestamp-center |
| **867** / `nsh_reading_integration-c754697d113fc18b` | `ae4a7d32419b97e95802017a3a963b01993321e35302763041ed3f9be4aeff05` | matches |
| **865** / `nsh_message_anchors_integration-b317b2d7635563d8` | `5675b1b05753592d8310e19c95b22c8dac2e066411125ba994d8bb7b6fc0835d` | matches |
| **869** / `nsh_ordinary_controls_integration-880bb28b2b36eb9a` | `0051dbe27db0314f603ea46294d70aa625e1ced5201d7356d5e2c9a3693a67e4` | matches |

The following hashes identify **later observed source bytes only**, not proven compiled inputs. No frozen input manifest was recovered for this repair build, and author lanes may mutate these files after observation. The unchanged-looking hash of a single source file cannot certify the whole dirty build or transfer old failures/passes to current sources.

| Later observed source | SHA-256 |
| --- | --- |
| `crates/vega_ui/src/conversation_stream/ordinary_submissions/presentation/pixels.rs` | `cd900d7659948e63e5cf669b2fd50a8f001d4e3a20b5b2809a6a34a0a4db80ee` |
| `crates/vega_ui/src/conversation_stream/ordinary_submissions/presentation/tests.rs` | `036aa7b55b5c3767f06bf5c0d1d87329f7bbaf1d52b6622d253771d0b9d3642a` |
| `crates/vega_ui/src/conversation_stream/timestamps/presentation/tests.rs` | `db893b6b2f5df3f5aadaf989341b3cce86a7020e1b3b5222625d4c6e15d2c230` |
| `crates/vega_ui/src/conversation_stream/timestamps/presentation/mounted_regressions.rs` | `fb9925a37f7e5b930b3ce57f39c935d554a0b9ccd59e3ac1a018968478f304ea` |
| `crates/vega_ui/src/conversation_stream/message_anchors/mounted_regressions.rs` | `8589a44b6a74fbbbeba0e85a608005b13eec193171d6a7f09e3c793e79184341` |
| `crates/vega_ui/src/sidebar/threads_block/rename_tests.rs` | `c0768ec00bcb4044930cc5e369c549cedd17206f973150e08f383c4e4d80ec97` |
| `crates/vega_ui/tests/nsh_message_anchors_integration.rs` | `64c27a5b7f5d11dfd2c858e38fcd29db093bf716b467c7dab075251d9c24382a` |

### Backend ledger CURRENT at its refreshed inventory boundary

This supersedes **only the current narrative** of the preceding section's STALE warning; that warning is retained as historical evidence. Parent now reports the backend ledger **CURRENT**, refreshed after the VK_SPACE imports and removed-shortcut-closure drop, with a **full verifier pass before this request**. Observed `docs/907/nsh-backend-current-manifest.json` SHA-256 is **`5083c6a877934019ad3405d5db560857495e305f43244ea41a6ac280c177b41c`**; its timestamp is **`2026-11-09T08:40:58.370934+00:00`**. Its `snapshot_increment` explicitly describes this compilation-wiring increment and records the eleven-target **33.06s** build plus actual native/IME **36/36** and palette **3/3** at that compiled snapshot. These are retained green results, **not reruns in this repair delta**, not a claim that all UI/mounted groups pass, and not native OS IME/AX acceptance.

The manifest's combined inventory is **240 current files / 30 delta files**, tree SHA-256 **`981fb7e1f80f461007f04cbe5d3d33c42d4cb809267a41e47aa323771676edc4`**, delta SHA-256 **`07cc9147b814bbb0e815b61782b40a6cf965b930d1fda0d26c914bcf88333cf8`**. Its two increment-file hashes match this lane's later observations: `vendor/nsh-platform-gpui/src/input.rs` **`e5de4dba419888effc2bfe7af57401e2892e1ce5c5dd404ac7c089150f3aacef`** and `vendor/nsh-platform-gpui/src/text_input/host_tests.rs` **`98bb7764c80e023cb9a5bfa32a670a7525dd3b4d64f85eb54a67228d5ccacd63`**. This writer did not rerun the full verifier; its exact command/log identity was **not supplied or recovered**, so the full pass is attributed to the parent, not presented as this lane's independent verification. Manifest hash equality alone does not verify every vendor byte. NSH **0.4.0**, Kit **0.7.3**, Fast / Fast platform **0.1.9** remain fixed; this lane changes none of them.

### Retained repair evidence SHA-256

All following paths are relative to `target/`; hashes cover complete actual retained files. Large rename stderr was read only at focused failure ranges, not dumped wholesale.

| Retained file | SHA-256 |
| --- | --- |
| `nsh-closure-repair-build.json` | `809ca366eca58442e3d0c0b4bb01d7bcaf9e49cfaee3110f129f5eedc37b907a` |
| `nsh-closure-repair-build.stderr` | `0ff4fe5fce8c86fab8cb02d06491b5f23e1565007e095a3df9774fd15aef63a6` |
| `nsh-closure-repair-execution.json` | `6925a29d6853b20a0968eb82ae159cb67bf655fe9dba636e842f85275e0d8d5a` |
| `nsh-closure-repair-ordinary-press.stdout` | `d4e07c279228cb97954939a8d1a56ab36c0b2f41f96bc3594b417b76cbd907e4` |
| `nsh-closure-repair-ordinary-press.stderr` | `980abbdbbdc3d124e1ad794f884e7138e07880b4a9e18fb8acd53cb63360cae4` |
| `nsh-closure-repair-timestamp-center.stdout` | `a30ca4aa4ecaba4537a8869440990aa6936a973ba042ec0f4b19517fb4be4ed4` |
| `nsh-closure-repair-timestamp-center.stderr` | `686aff73efc90ce83fc20894bf1048ba8228113e745831912f0390784cf5cb0f` |
| `nsh-closure-repair-reading-rename.stdout` | `009f481d4e758deeea3d6d341d268a3bf14c056954a83acd2d44d77de46f588a` |
| `nsh-closure-repair-reading-rename.stderr` | `dd35b143606acae0bf31232360015df11b6443c840218360e762dffe4417c150` |
| `nsh-closure-repair-anchors.stdout` | `15bd53ddd56db7e2b065144de8446c06234e4fd0b49653ec26fbdc7cbdf2af13` |
| `nsh-closure-repair-anchors.stderr` | `f6ec05deaa87e8bb173a90598d13a567c3fda56685eb60dea838811652ae845f` |
| `nsh-closure-repair-ordinary-mounted.stdout` | `5a67a03fb5316d9c49d989a51831ff7d5913e2524c62be02d84b609151fa2c6c` |
| `nsh-closure-repair-ordinary-mounted.stderr` | `905ce059950319ec167ccc37a548b48d8673970eb6cb61096d0fb42688fbf993` |

**Stopped boundary:** failures above remain open at their compiled identities. Later author repairs require the parent's fresh validation; earlier strict Clippy/workspace/Python/policy greens remain their own historical snapshots. No assertion/deadline weakening, full migration completion, independent final review, final-head CI, native OS IME/AX or three-desktop acceptance is claimed. The evidence writer stops writes after readback/static evidence checks and final document hash handoff; implementation/validation remains with the parent and disjoint author lanes.

## Closure implementation followup — failures after `0cf7902c`

This is **documentation implementation, not independent review or final acceptance**. Only this integrated-validation document is written. Removing this inserted section exactly restores the preceding document, SHA-256 **`0cf7902c2ee0c3a72515ef9c5970361cd94610d92d18b887ed604d76acdc0054`**; all older failures, corrections and green snapshots remain history. No Cargo/rustc invocation, test execution, network, Git mutation, source/other-document write or cleanup is performed by this lane. Explicit continuation restrictions prohibit fetch/update here.

The dirty-worktree base remains the previously parent-reported **`0b5ea426abfd8d4358489e916273d89474fb190f`**, not a newly queried or certified HEAD. The new retained results supersede the earlier pre-rename/IME green boundary **only for the compiled artifacts identified below**, not every later source byte. There is **no global success**: the library and three mounted groups actually fail, and two groups leave cases unexecuted. Parent owns validation; the author lanes now repair C1/C2, the rename fixture and anchors. This lane records evidence only and does not diagnose or implement their repairs.

### Four compile failures retained, then an actual successful build

The previously recorded parent toolchain is **`cargo 1.98.0 (797e8a9bc 2026-08-05)`**, **`rustc 1.98.0 (88d9e12ae 2026-08-18)`**. It was not queried again by this writer. Exact shell argv/environment and durations for these five closure build attempts are **not recovered from their JSON/stderr files or supplied in this delta**. The earlier eight-target command is not relabelled as an executed eleven-target command. Parent reports the first four attempts **exit 101** and the final signal build **exit 0**; retained `build-finished` records independently establish failure/success, not the process exit code.

All following paths are relative to `target/`. The diagnostic locations are **compiled-snapshot locations**; later source edits can move them.

| Attempt (`nsh-closure-ui-…`) | Retained build footer | Actual compile evidence |
| --- | --- | --- |
| `build.{json,stderr}` | JSON line **863**, `success=false`; parent exit **101** | JSON **860–861**, **E0425**: `cannot find value VK_SPACE in this scope`, at `vendor/nsh-platform-gpui/src/input.rs:1422:24` and `vendor/nsh-platform-gpui/src/text_input/host_tests.rs:645:52` |
| `imports-build.{json,stderr}` | JSON **872**, `success=false`; parent exit **101** | JSON **861–865**, **E0603**: `module text is private`, at `ordinary_submissions/presentation/mounted_regressions.rs:19:77`, `message_anchors/mounted_regressions.rs:32:26` and `:88:64`, `timestamps/presentation/mounted_regressions.rs:17:51` and `:75:24` (all under `crates/vega_ui/src/conversation_stream/`); JSON **866**, **E0599**: `no method named role found for struct gpui_kit::Div in the current scope`, `timestamps/presentation.rs:143:18`; JSON **867**, **E0061**: `this method takes 2 arguments but 3 arguments were supplied`, `crates/vega_ui/src/sidebar/threads_block/rename_tests.rs:290:20` |
| `api-build.{json,stderr}` | JSON **864**, `success=false`; parent exit **101** | JSON **861**, `recursion limit reached while expanding #[test]`, at `crates/vega_ui/src/conversation_stream/ordinary_submissions/presentation/tests.rs:179:1` |
| `macros-build.{json,stderr}` | JSON **864**, `success=false`; parent exit **101** | JSON **861**, **E0277**: `can't compare timestamps::presentation::Snapshot with timestamps::presentation::Snapshot`, at `crates/vega_ui/src/conversation_stream/timestamps/presentation/tests.rs:17:23` |
| `signal-build.{json,stderr}` | JSON **882**, `success=true`; parent exit **0** | stderr **2**: successful `test` profile completion in **33.06s**; **11 executable artifacts**, each `profile.test=true`, `fresh=false`, selected below |

These are compiler/wiring failures, not failed runtime tests and not silently discarded attempts. Parent reports the compile corrections: missing `VK_SPACE` imports; private text-module `TextLine` access changed to the public root export; standard test-macro disambiguation; explicit `SignalEqualsNever` for the fixture; node ID enabling `Stateful` role use; the two-argument `simulate_click` API; removal of an unused fixture window; and dropping the removed-shortcut closure. The diagnostic stream also preserves the unused boxed `Fn` / `AppContext` warnings in `imports-build.json:860,868` and unused `window` warning in `api-build.json:862`. This is the parent's correction account, not a code review or a proof of final source equivalence. **No assertion or execution budget is reported weakened**, and this writer changes neither.

The successful build is **not warning-free**. `nsh-closure-ui-signal-build.json` retains **nine** linker warning records (**862, 864, 866, 870, 872, 874, 876, 878, 880**): `ld: __eh_frame section too large (max 16MB) to encode dwarf unwind offsets in compact unwind table, performance of exception handling might be affected`. They belong to anchors, recovery, icons, reading, settings, scroll, ordinary controls, palette and the UI library. These warnings were not suppressed or converted into compile failure; the earlier strict-Clippy green is a separate older observation, not a fresh lint result for this delta.

### Eleven actual artifact executions — group failures and unexecuted cases

Parent reports executing each newly JSON-selected `profile.test` executable **once**, the library plus the remaining **ten separately executed groups**. The two retained execution metadata files give exact executable paths, SHA-256, exits, elapsed times and stdout/stderr paths. The full launcher argv/environment/timeout limit is **not retained in those files or supplied in this delta**; no earlier launcher command is claimed as this invocation. All **11** entries explicitly record **`watchdog_timeout=false`**. The anchors' internal layout-stage timeout is a caught case failure, **not** a watchdog timeout. Test-profile artifacts do not constitute launching the desktop application.

The table uses `target/nsh-closure-<target>.{stdout,stderr}` for each target's raw logs. Process elapsed comes from execution metadata, not Rust's shorter internal suite timer. All groups report **0 ignored**. Successful compact geometry/settings footers omit a not-executed field; their **24 / 3 named PASS lines** and totals establish full execution, rather than an invented footer field.

| Target | Actual passed / failed / not executed | Exit | Process elapsed | Raw footer / failure evidence |
| --- | --- | --- | --- | --- |
| `vega_ui` | **354 / 2 / 0**; **356 run**, 0 filtered/measured | **101** | **3.034s** | stdout **380**: `FAILED. 354 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out`, internal **0.47s**; two assertion failures below; stderr empty |
| `nsh_native_host` | **36 / 0 / 0** | **0** | **1.073s** | stdout **40**: 36 passed, 0 failed/ignored/measured/filtered; internal **0.21s**; stderr empty |
| `nsh_reading_geometry` | **24 / 0 / 0** | **0** | **6.891s** | stdout **25**: `reading geometry and opacity: 24 passed; 0 failed; 0 ignored`, preceded by 24 named PASS lines |
| `nsh_reading_integration` | **14 / 1 / 0** | **1** | **29.126s** | stdout **15** explicitly reports 14 passed, 1 failed, 0 not executed, 0 ignored; stderr **47677–47680** records the failing rename fixture |
| `nsh_scroll_integration` | **16 / 0 / 0** | **0** | **87.231s** | stdout **17**: 16 passed, 0 failed/not executed/ignored |
| `nsh_icons_integration` | **3 / 0 / 0** | **0** | **3.377s** | stdout **4**: 3 passed, 0 failed/not executed/ignored |
| `nsh_settings_integration` | **3 / 0 / 0** | **0** | **4.245s** | stdout **4**: 3 passed, 0 failed/ignored, preceded by three named PASS lines |
| `nsh_recovery_integration` | **2 / 0 / 0** | **0** | **5.509s** | stdout **3**: 2 passed, 0 failed/not executed/ignored |
| `nsh_message_anchors_integration` | **1 / 1 / 4** | **1** | **13.382s** | stdout **2** explicitly reports 1 passed, 1 failed, 4 not executed, 0 ignored; stderr **155–158** records the narrow-layout failure |
| `nsh_palette_integration` | **3 / 0 / 0** | **0** | **4.306s** | stdout empty; stderr **186,197,201** are three named PASS lines; **202**: `palette integration: 3 passed; 0 failed; 0 unrun; 0 ignored` |
| `nsh_ordinary_controls_integration` | **0 / 1 / 3** | **1** | **4.066s** | stdout **1** explicitly reports 0 passed, 1 failed, 3 not executed, 0 ignored; stderr **112–119** records the wrapped-height failure |

**Actual failures, retained without reinterpretation as success:**

- Library timestamp centering: `conversation_stream::timestamps::presentation::tests::production_separator_preserves_localized_text_weight_center_padding_and_natural_height`, stdout **364–365**, `crates/vega_ui/src/conversation_stream/timestamps/presentation/tests.rs:103:13`: `assertion failed: (left - (480. - right)).abs() < 1.`
- Library ordinary Press dispatch: `conversation_stream::ordinary_submissions::presentation::tests::production_press_retains_displayed_attempt_and_replaced_handlers_cannot_launder_authority`, stdout **369–372**, `crates/vega_ui/src/conversation_stream/ordinary_submissions/presentation/tests.rs:310:5`: assertion `left == right`, **left 0, right 1** (request count). This is an actual failure, not passing authority coverage inferred from older policy tests.
- Source runner's first **14 source cases pass**, then `mounted_rename_native_ack_ime_cancel_rebind_and_persistence` fails at `crates/vega_ui/src/sidebar/threads_block/rename_tests.rs:192:6`: `project: Sql(SqliteFailure(Error { code: Unknown, extended_code: 1 }, Some("no such table: projects")))`. This identifies a fixture/database failure before completed rename behavior; it is neither a new source-selection failure nor proof that rename persistence works. **The 15-case group is failed**, despite its 14 passing source cases.
- Anchors: `short_preview_uses_actual_natural_height_and_original_shadow` passes; `narrow_long_cjk_and_emoji_preview_clamps_three_native_lines` fails at `crates/vega_ui/src/conversation_stream/message_anchors/mounted_regressions.rs:328:13`: `anchor stage timeout: actual native layout and text lines`. Its runner stops after the failure. The four **not executed** cases are `mark_states_keep_native_dimensions_and_semantic_colors`, `stable_frames_and_changed_font_theme_content_width_retain_surfaces`, `clipped_passive_leaves_forward_input_once_and_skip_tab_focus`, and `unmount_stops_both_surfaces_and_remount_creates_new_identity`.
- Ordinary controls: first case `timestamp_native_narrow_height_and_passive_input` fails at `crates/vega_ui/src/conversation_stream/timestamps/presentation/mounted_regressions.rs:285:5`: `long timestamp must use actual wrapped natural height`. Its runner stops; `production_buttons_pass_pointer_through_full_inset_wrappers_and_keep_focus_dispatch`, `retry_and_same_attempt_fact_changes_reject_old_displayed_controls`, and `route_settings_geometry_and_unmount_reject_captured_controls` are the three **not executed** cases. They are not ignored, failed assertions or passing observations.

The latest IME/native-host **36/36** and actually executed mounted palette **3/3** are **GREEN only at this compiled snapshot**. Palette's named passes cover marked keys leaving controller/focus unchanged, ordinary navigation/exact events, and retired requests not mutating the mounted controller. These are development in-process host/controller checks, **not OS-native IME/AX acceptance**. Recovery's two passes are current for this artifact; earlier summary-selector and Tab failures and the notification-loop interruption remain in historical sections. Full-policy development tests are unaffected per parent, but no new policy execution is supplied here; the older 26/26 ledger is not counted as a new run.

### Exact artifact identities and observation boundary

All basenames below are under `target/official-nsh-validation/debug/deps/` in this worktree. Each path is an exact match between the successful build JSON and its execution metadata, not a filename-glob guess. Execution SHA-256 remains the authoritative identity for the recorded result even if a later build replaces the path.

| Target | Build JSON line | Exact executable basename | Execution SHA-256 | Observation below |
| --- | --- | --- | --- | --- |
| `vega_ui` | **881** | `vega_ui-4268349f2e74c4f3` | `67bdb8d116ed143f6597a91f73203937adabe803774266a5897d2826f1a7d0dd` | matches |
| `nsh_native_host` | **868** | `nsh_native_host-c6fd6b079b892425` | `a1ff9807ac36beb712da9c5bddb696bec6bd2e1f31410de215d7a175510a15a6` | matches |
| `nsh_reading_geometry` | **869** | `nsh_reading_geometry-da777a5f64c485cf` | `83c9641a4ea09438b52875a48cddbe9e710fd6113e622540b92d6c360c4329a6` | matches |
| `nsh_reading_integration` | **871** | `nsh_reading_integration-c754697d113fc18b` | `0fcaa1ca935bdf877da346c6fc65ba2a355c85fe8d6c5dea1d70148b7e3e632a` | matches |
| `nsh_scroll_integration` | **875** | `nsh_scroll_integration-1d1fd85936d176b3` | `83ab2ed7b31be40e0042d4d8b58f4bcea6524a7186ad2b94a0127c60a6c5e950` | matches |
| `nsh_icons_integration` | **867** | `nsh_icons_integration-c7d68528d5563f59` | `a2b6be05cecefc80dc16656b8e89d76cf7c2bcc3960dc5e9c6d9cd03d7956630` | matches |
| `nsh_settings_integration` | **873** | `nsh_settings_integration-c1b0a9b018bd18c8` | `ea2367f7447b4438adcbe46093628c240d13c8ba961bdd15567a537ce1e017c8` | matches |
| `nsh_recovery_integration` | **865** | `nsh_recovery_integration-9271aa7368d76ca1` | `604d2e9fada3dd48ccb7e55ea7ed6557fc17409c59b75d3694484249f5460508` | matches |
| `nsh_message_anchors_integration` | **863** | `nsh_message_anchors_integration-b317b2d7635563d8` | `c0ec611dd507fe231b54fb9b6a4cbf75f293e5dbe6a372b522cd3434945bf313` | matches |
| `nsh_palette_integration` | **879** | `nsh_palette_integration-c2e61111db3ee3f6` | `49234b423fe61ca936d912f6a502a8f5390acef6c95b5024e77180413b70eb1a` | matches |
| `nsh_ordinary_controls_integration` | **877** | `nsh_ordinary_controls_integration-880bb28b2b36eb9a` | `8f3bca6bcb82d303d93a010e72630ea759fb91e921a61f51862c2cc52ded5dc9` | matches |

The read-only identity scan began at **2026-11-09T08:45:33.670178+00:00** (UTC). The table reports the bytes observed during that scan, not a promise that paths remain unchanged through handoff. A match cannot certify source equivalence or invalidate later author changes. No frozen dirty-source manifest mapping every compiled input to these artifacts was recovered.

The following full-file source SHA-256 values were observed **after compilation/execution** during this evidence update. They identify later bytes only: no assertion is made that these are exactly the compiled inputs, that failure line numbers still match, or that parallel repairs have passed. The backend ledger is separately parent-owned; parent reports it **STALE relative to the latest import changes** and will refresh it. Its observed hash below is not refreshed backend validation.

| Later observed source / ledger | SHA-256 (observation, not compiled-input identity) |
| --- | --- |
| `vendor/nsh-platform-gpui/src/input.rs` | `e5de4dba419888effc2bfe7af57401e2892e1ce5c5dd404ac7c089150f3aacef` |
| `vendor/nsh-platform-gpui/src/text_input/host_tests.rs` | `98bb7764c80e023cb9a5bfa32a670a7525dd3b4d64f85eb54a67228d5ccacd63` |
| `crates/vega_ui/src/conversation_stream/timestamps/presentation.rs` | `dfb45ccc2c1d9e797d215f8475da30804ff7f84f0437224b8940f4aeb09fff01` |
| `crates/vega_ui/src/conversation_stream/timestamps/presentation/tests.rs` | `db893b6b2f5df3f5aadaf989341b3cce86a7020e1b3b5222625d4c6e15d2c230` |
| `crates/vega_ui/src/conversation_stream/timestamps/presentation/mounted_regressions.rs` | `fb9925a37f7e5b930b3ce57f39c935d554a0b9ccd59e3ac1a018968478f304ea` |
| `crates/vega_ui/src/conversation_stream/ordinary_submissions/presentation/tests.rs` | `036aa7b55b5c3767f06bf5c0d1d87329f7bbaf1d52b6622d253771d0b9d3642a` |
| `crates/vega_ui/src/conversation_stream/ordinary_submissions/presentation/mounted_regressions.rs` | `3494fda898aae81d537c119c1089438992754c0dcec7cd96e182480f561db86c` |
| `crates/vega_ui/src/conversation_stream/message_anchors/mounted_regressions.rs` | `e5e75ce85321b55033bc01c6b1ff84d54f507f425e0c021e353cc2aa29914979` |
| `crates/vega_ui/src/sidebar/threads_block/rename_tests.rs` | `c0768ec00bcb4044930cc5e369c549cedd17206f973150e08f383c4e4d80ec97` |
| `crates/vega_ui/tests/nsh_reading_integration.rs` | `f6435b362fa66975caccd6b15e0fbd1097fbe5fed3bf197d0f5476b0330a4d53` |
| `crates/vega_ui/tests/nsh_message_anchors_integration.rs` | `64c27a5b7f5d11dfd2c858e38fcd29db093bf716b467c7dab075251d9c24382a` |
| `crates/vega_ui/tests/nsh_ordinary_controls_integration.rs` | `071bbc2c452b277e64f6b1e634bb62df44f9d7a1fd5210db1caf9712d0b012d6` |
| `crates/vega_ui/tests/nsh_palette_integration.rs` | `1b8b972019d97b0805124704e22af4cc5e167e45149bab30944271945783683c` |
| `docs/907/nsh-backend-current-manifest.json` | `5083c6a877934019ad3405d5db560857495e305f43244ea41a6ac280c177b41c` |

### Retained raw evidence identities

All paths below are relative to `target/`. Full-file SHA-256 values were calculated from actual retained bytes; the multi-megabyte stderr files were searched only at focused diagnostics/footer ranges, not dumped wholesale. The two execution manifests establish the timing/exit/artifact identities; the build JSON preserves exact compiler diagnostics, artifact records and build status. Empty logs are listed explicitly and do not independently prove success.

| Retained file | SHA-256 |
| --- | --- |
| `nsh-closure-ui-build.json` | `a9df8fb1f05fa4e83b51182bc82ca969271b7a7ccbf143874ef43c37e1101b24` |
| `nsh-closure-ui-build.stderr` | `ea6320aaa17fc3c903d52f6dc55ce771d72f3c26e5a538c00b33e6e93e3ebd34` |
| `nsh-closure-ui-imports-build.json` | `2cccdafe76cc8ffab6179e97c20ac2d6ecf1315c444a764529b06d755aa8da34` |
| `nsh-closure-ui-imports-build.stderr` | `1a0478d97e874122b64b5daf650f4ef4029c1cdd563a784127cc3d83351c1ab9` |
| `nsh-closure-ui-api-build.json` | `3cfead1396fe1e493b651c8fa5afca90240077947feb58531baf4744307df4f5` |
| `nsh-closure-ui-api-build.stderr` | `f195518bacdb4e4b2330a5b64394e0c076583fd83b59e05b77ed3db0693af8c7` |
| `nsh-closure-ui-macros-build.json` | `2ed3e4cd619a21405092cb9516f7ce8142643f2e6ff8781a9db3882ca90143b1` |
| `nsh-closure-ui-macros-build.stderr` | `acc47fde98b0cd3bd674c89e191672eef63db528bdd10c70ed8615420d61850e` |
| `nsh-closure-ui-signal-build.json` | `881a2b4f8b0f68da0cd3a7613df3eaed92600b570a0039f948cca8252b2046ac` |
| `nsh-closure-ui-signal-build.stderr` | `9602e3c33d285271ad77da2cc1debcbdd35a8853bd6692e08a2dda6285911898` |
| `nsh-closure-ui-execution.json` | `bf0ef2466fa528de0add990ba02531c7157a8be4fb880b2ac74478df4a6f5afd` |
| `nsh-closure-mounted-execution.json` | `a7680ae29c5381a3ebdf280421a1416e37141244c13df49bfe362c652a40746d` |
| `nsh-closure-vega_ui.stdout` | `93854b092e6d1d7ea3965237f91b7acac8633512e587dde96c02b3de38dc57e7` |
| `nsh-closure-nsh_native_host.stdout` | `b3dabe1bf16fdd12921c1db0477a27bdc67051b7480e60db8488823d7d9317e3` |
| `nsh-closure-nsh_reading_geometry.stdout` | `59600e972c5380677b5f8ff6afbf1bf23dc2b855f8e107108f266316f560297a` |
| `nsh-closure-nsh_reading_geometry.stderr` | `85e36a0cb4709c5a55240c40deb7b9a00aa461f37c35f16d7edff133fac783fa` |
| `nsh-closure-nsh_reading_integration.stdout` | `009f481d4e758deeea3d6d341d268a3bf14c056954a83acd2d44d77de46f588a` |
| `nsh-closure-nsh_reading_integration.stderr` | `bee0d78def3b13ed726cc17da1b6f39ea819d212d17f0a4910bb1726b6455398` |
| `nsh-closure-nsh_scroll_integration.stdout` | `5e3b12f7cb3cbf7b16577739653275717d76285f472a095dd12a58fa538390cf` |
| `nsh-closure-nsh_scroll_integration.stderr` | `92b86be37243f6d808a280b7fc375a60412036c2d2cca9e7b8cc9cad860a6c2d` |
| `nsh-closure-nsh_icons_integration.stdout` | `9dd4a6c2ee6ecaa71af30d6404b9bb0929572d21e57d100382177679718322c8` |
| `nsh-closure-nsh_icons_integration.stderr` | `44814cca4c68cebe596356b93e5c535c10559c0388349fbe045f9e0312fd77e1` |
| `nsh-closure-nsh_settings_integration.stdout` | `b57f873f5d8625dab01193161d46fe1f462a0cec4249c804c7adead0ebb36637` |
| `nsh-closure-nsh_settings_integration.stderr` | `fa89969462df29e45a1c126e23643d5d3ce18eedc84a9f458dad99cd9088e25b` |
| `nsh-closure-nsh_recovery_integration.stdout` | `d6a0c06e9d1de2519595bc6a48e6c466141237064ca96d4485408e221e6fb4a3` |
| `nsh-closure-nsh_recovery_integration.stderr` | `72d25adae6e5132c9b93ef2115712457ee72736d117c45684fb58e9ce3ef5124` |
| `nsh-closure-nsh_message_anchors_integration.stdout` | `99868509407947a923c68d89542f88a512dd3cf21566cab92499006a40f91ffe` |
| `nsh-closure-nsh_message_anchors_integration.stderr` | `d4126eff435f367a869110f292afd0b5b68e8e0c44458de3df92f07ece60b35e` |
| `nsh-closure-nsh_palette_integration.stderr` | `3e438b42c3f1ea62576fb92f5cb6f077b197e10ba3b09700ac5cf0dad25180fc` |
| `nsh-closure-nsh_ordinary_controls_integration.stdout` | `5a67a03fb5316d9c49d989a51831ff7d5913e2524c62be02d84b609151fa2c6c` |
| `nsh-closure-nsh_ordinary_controls_integration.stderr` | `ece73647e99a3a8160f8987043e653676ad813400b5dd8a4cbe63525c149d326` |

These **3** files are empty, each SHA-256 **`e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`**: `nsh-closure-vega_ui.stderr`, `nsh-closure-nsh_native_host.stderr`, `nsh-closure-nsh_palette_integration.stdout`.

### Current blockers, ownership and remaining gates

- Parent validates; author lanes repair **C1/C2, rename fixture and anchors**, without this document writer touching their source. Compiler/wiring correction does not turn subsequent assertion/layout/fixture failures into passes. No retry, budget enlargement, assertion relaxation or additional execution is performed by this evidence lane. New author bytes require new compilation and actual relevant execution before a green can be attached to them.
- The preceding 378/378 Nextest, app 31/31, workspace 6/6, strict Clippy, formatting, Python/i18n and #920 integration observations remain at their explicitly recorded **older snapshots**. This closure delta is not their rerun and makes **no actual #920 behavior-regression claim**. Local Nextest **0.9.151** versus CI-pinned **0.9.146** remains a provenance difference, not CI equivalence. The earlier rejected `wait_ms` / `workspace_sandbox_required` remains a **nonexecuted tool failure**, not a closure build or test failure.
- The backend manifest predates the latest import changes and is **STALE** pending the parent's refresh; no current backend-ledger recertification is claimed. Kit **0.7.3**, Fast / Fast platform **0.1.9** are unchanged by this lane.
- **Independent review has not occurred for this final implementation delta; final-head required CI and macOS/Windows/Linux native acceptance, including OS-native IME/AX, remain pending.** This documentation work is not independent review. Development-test archival/removal and final no-Rust-tests-head checks remain separate requirements. No final current-source green, complete production migration, merge, package, installation or desktop launch is claimed.
- Updates report only changed results/blockers using the user's fixed weights, without a total estimate or invented completion percentage. This lane stops writes after document-only static readback and hands off the exact document SHA-256.

---

## App, workspace Nextest and Python followup — after `d988cf05`

This is **documentation implementation, not independent review or final acceptance**. This section updates only the latest retained-evidence boundary. The entire preceding document, SHA-256 **`d988cf05c802b9075eab04f107a29b24d024f0248347e79703cee33e19f059fd`**, is preserved byte-for-byte when this inserted section is removed; historical failures and their later corrections remain intact. Only this integrated-validation document is written by this lane. No Cargo/rustc, product/test execution, network, Git mutation, cleanup, Rust or other-document write is performed here; the explicit continuation restrictions also preclude fetch/update.

The retained dirty-worktree base remains parent-reported **`0b5ea426abfd8d4358489e916273d89474fb190f`**, not a newly recertified HEAD. These successful executions **predate the subsequent rename/IME implementation/test delta**. Parent reports a genuine remaining old-GPUI rename visual child found through shell inventory, with its author implementing the repair; the native IME author separately owns the vendor host-test function and palette documentation. This writer does not inspect or implement those repairs. Their later bytes are **not validated by the results below**. Parent owns validation; the other authors retain their nonoverlapping scopes.

### Actual app build and two exact artifact executions

The actual official toolchain identities remain the parent's previously recorded **`cargo 1.98.0 (797e8a9bc 2026-08-05)`** and **`rustc 1.98.0 (88d9e12ae 2026-08-18)`**. This writer does not query either tool. Parent supplies this executed build command:

```sh
CARGO_FORK_AUTO_UPDATE=0 rustup run 1.98.0 cargo test -p vega --bin vega --test nsh_workspace_mounted --features nsh-workspace-integration --offline --locked -j2 --target-dir target/official-nsh-validation --no-run --message-format json
```

`target/nsh-current-app-build.stderr:1–3` records compilation of `vega_ui` and `vega`, followed by successful `test`-profile completion in **20.97s**. `target/nsh-current-app-build.json:874` records **`{"reason":"build-finished","success":true}`**; parent reports **exit 0**. Its two selected executable records have **`profile.test=true`**, `fresh=false` and feature `nsh-workspace-integration`: `nsh_workspace_mounted` at line **870**, kind `test`, and `vega` at line **873**, kind `bin`. Selecting a test-profile app artifact is not launching the desktop application.

Parent reports running each exact JSON-selected executable **once**, sequentially, with **`subprocess.run([executable], timeout=120)`**, no executable arguments, and captured stdout/stderr. The full Python launcher/environment is not retained here. `target/nsh-current-app-execution.json` supplies the exact paths, execution hashes, timings and outcomes; both entries have **exit 0**, **`watchdog_timeout=false`**.

| Executed target | Actual result | Process elapsed | Retained result evidence |
| --- | --- | --- | --- |
| `vega` test-profile app binary | **31/31 passed**, not merely the older shell subset of 4 | **0.679s** | `target/nsh-current-vega.stdout:35`: 31 passed, 0 failed/ignored/measured/filtered; 31 named `test … ok` lines; stderr empty |
| `nsh_workspace_mounted` | **6/6 passed** | **3.370s** | `target/nsh-current-nsh_workspace_mounted.stderr:13`: 6 passed, 0 failed/not executed/ignored; six named `PASS` lines; stdout empty |

All executable basenames below resolve under `target/official-nsh-validation/debug/deps/`. Execution hashes are from the retained metadata, not inferred from whatever bytes occupy the paths later.

| Target | Exact executable basename | Execution SHA-256 |
| --- | --- | --- |
| `vega` | `vega-39df89ae657a2ad7` | `8a578a53ef0a3c1ebb26a67255b66a51a87f658484e07757f42733e8edfa0cff` |
| `nsh_workspace_mounted` | `nsh_workspace_mounted-bc600955a0a1fea1` | `1adad0911afda1849e250da156878489867b83de728f48e335ed935393e8e21b` |

At **2026-11-09T07:02:04Z**, both paths still matched these execution hashes; the prior UI-lib path also matched its recorded **`8354638a73a64ba30514a3be44ff312f35a85a69e0d44d62765867747cf2a267`**. Each hash read had stable inode/size/mtime. **No mismatch was observed for these three paths at that time**; this is not a guarantee against subsequent replacement by Nextest or another build, nor a new execution of the old UI artifact. The Nextest log does not provide a full per-binary hash manifest, so these hashes are not silently assigned to its 16 binaries.

At **2026-11-09T07:01:25Z**, the six source paths in the preceding section's source table still matched its exact hashes, including source body **`8fdc0a1f37671a186f54fb008af4f9888011966e3370bf009efaeaf73ada935a`**, workspace `mounted_observations.rs` **`86deab2f362e736fb219f7db69cfc4362f8ec313ad6ea7dc69ecd929cd408f7c`** and `mounted_regressions.rs` **`76db729afb4cde3fad3693e2a4f2735d4b12b0840ac21b5aec78e4c9cf9f01f1`**. These focused later source observations are not an immutable or exhaustive compile-input manifest; they do not cover the in-progress rename/IME delta.

### Workspace Nextest: actual local execution, not CI equivalence

Parent supplies this subsequent executed command:

```sh
CARGO_FORK_AUTO_UPDATE=0 rustup run 1.98.0 cargo nextest run --workspace --lib --bins --features vega_ui/nsh-reading-integration,vega/nsh-workspace-integration --offline --locked --build-jobs 2 --target-dir target/official-nsh-validation
```

Parent reports **exit 0**. `target/nsh-current-workspace-nextest.stderr:19` records a **1m 10s** test-profile build; line **21** identifies run **`d1d1db54-14b3-48b1-8179-0268e63e323f`**, profile **`default`**. Line **22** starts **378 tests across 16 binaries**; there are **378 actual `PASS` lines**; line **402** reports **`Summary [   1.141s] 378 tests run: 378 passed, 0 skipped`**. **1.141s is test execution summary time, not total build-and-run wall time.** This command selects workspace lib/bin tests, not a fresh run of every mounted integration runner listed elsewhere in this document; their standalone results remain separately attributed. These are local development Rust tests, not the required final-head Rust-test-free delivery check.

The log is **not warning-free**: lines **6–10** retain **`ld: __eh_frame section too large (max 16MB) to encode dwarf unwind offsets in compact unwind table, performance of exception handling might be affected`**, emitted for `vega_ui` lib tests. Successful execution does not erase that linker warning. The earlier strict workspace Clippy **8.84s / exit 0** remains a separate check at its own snapshot, not proof that this later linking phase emitted no warnings or that subsequent rename/IME bytes passed Clippy.

Parent's actual local Nextest version query returned **0.9.151**. The retained run log does not itself print that version; this attribution is parent-supplied, not a new query by this lane. Required CI pins **`nextest@0.9.146`**, confirmed at `.github/workflows/pr-check.yml:54`. **The local and CI runner versions differ**: no upgrade/install is authorized or performed, and no CI-equivalence or final required-CI success is claimed.

### Python and i18n: retained current-round results

Parent identifies these actual executed groups. The table records supplied script/module names and arguments, **not an invented exact interpreter path, shell command or environment**; those launcher details are not retained in these output files. No Python product/regression command is rerun by this documentation lane.

| Parent-identified executed group | Actual retained result | Evidence |
| --- | --- | --- |
| `scripts/release_tests.py` | **5 tests, OK**, parent-reported exit 0 | `target/nsh-current-release-python.stderr:8–10`, 0.012s; stdout empty |
| `scripts/prepare_computer_use_tests.py` | **15 tests, OK**, parent-reported exit 0 | `target/nsh-current-cua-python.stderr:3–5`, 0.028s; stdout empty |
| `unittest discover -s scripts/tests -v` | **40 tests, OK**, parent-reported exit 0 | `target/nsh-current-unittest-python.stderr:43–45`, 0.321s; stdout retains `Publishing full-only delta manifest: Unsupported bundle path`, not a failing test footer |
| `scripts/check_i18n.py --json` | **0 errors**, parent-reported exit 0 | `target/nsh-current-i18n.json`: `errors=[]`, `inventory_scanned=true`, 313 inventoried literals; no `ui` or `unreviewed` inventory category |

The i18n inventory's actual classification counts are **benchmark 5, fixture 142, internal 68, model_prompt 4, protocol 88, stored_content 6** (313 total); both catalogs contain **2063 keys**. Zero errors and no UI/unreviewed entries do **not** mean zero inventoried literals or native-language/IME acceptance. Its retained limitations explicitly exclude proof of language switching, persistence, accessibility and A01–A07; dynamic/English UI coverage still requires human review. No actual **#920 behavior regression** is claimed from these checks or from the earlier mainline compile/lint result.

### Retained evidence hashes and remaining boundary

These are actual full-file SHA-256 values observed with stable inode/size/mtime at **2026-11-09T07:01:25Z**. Paths are relative to `target/`; no giant build JSON or stderr was dumped for inspection—artifact/footer/warning extraction was focused, while hashing read the complete bytes.

| File | SHA-256 |
| --- | --- |
| `nsh-current-app-build.json` | `43a151ba8439d7e0e50ad76f12b256adcf96002355893e8b0a14a58142330ade` |
| `nsh-current-app-build.stderr` | `5f92b70b0f9bc27619754b27c73a0f0594cc048d4243f0f4add1da24f1ae84c3` |
| `nsh-current-app-execution.json` | `6cd368e42730342343f06c9e3a399075bb4cb18ddd62896fe9b18106c08ba5e8` |
| `nsh-current-vega.stdout` | `9975f7b73c7aabcb6463ef64b279207a0a697a42f84abc4a904050611244992b` |
| `nsh-current-nsh_workspace_mounted.stderr` | `9b54eed47ade67bebfe1e2a566c5ff0806961fe289d762dc895a7d56b09fd3cb` |
| `nsh-current-workspace-nextest.stderr` | `0a4a9137c4b5a412d860e24fdef1276b652bab14a1239250be711417de7bcec6` |
| `nsh-current-release-python.stderr` | `e0af36579af04c70ba7c54140d903182cc7003afd1655496d8c941c1a00cb638` |
| `nsh-current-cua-python.stderr` | `4f1c31223fdd6e717fe53daeea28b7235d8f1e8109b99662437f258c5b9dc3d0` |
| `nsh-current-unittest-python.stdout` | `23a41c17f8e5a4ee94ea2832e4182877371a9d4b8faa25cc52993bdf5d2ba98c` |
| `nsh-current-unittest-python.stderr` | `7c6b21d48807f5d10c0e3f0fd9e1e6eb466bba6e593ff43857c8158938493d0b` |
| `nsh-current-i18n.json` | `f5eda511cc32c530bb141677bd02fba8f43beae24d48948e66c9e9d94d6cfc87` |

These **five** files are empty, each SHA-256 **`e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`**: `nsh-current-vega.stderr`, `nsh-current-nsh_workspace_mounted.stdout`, `nsh-current-workspace-nextest.stdout`, `nsh-current-release-python.stdout`, `nsh-current-cua-python.stdout`. Empty output is not an extra passed test.

The preceding UI **347**, native host **36**, geometry **24**, source **14**, scroll **16**, icons **3**, Settings **3** and recovery **2** successful standalone executions retain their precise compiled-snapshot attribution; no historical source/recovery failure is deleted or converted into a pass. The new app/workspace/Nextest/Python results do not establish final-full-production GREEN. **Rename/IME later-byte validation, implementation-independent review, development Rust-test archival/removal and final-head required CI, and macOS/Windows/Linux native acceptance remain pending.** No desktop app launch, native acceptance, merge, package or install is claimed. Kit **0.7.3**, Fast / Fast platform **0.1.9** remain unchanged. Updates report only changed results and blockers with the user's fixed weights—no total estimate or invented completion percentage. This evidence lane stops writes after static readback and hands off the exact document hash.

---

## Current UI snapshot and strict workspace followup — after `1b4ee9f7`

This is **documentation implementation, not independent review or final acceptance**. This section supersedes current-tense boundaries in the history below, including the former source-body compile gap and nine-diagnostic Clippy blocker; it does not erase any historical failure. The complete preceding document, SHA-256 **`1b4ee9f7d41efa2d42bcf4f4c2b922fc93ea611c0fe2dc3d114f8b19f1f25b7b`**, is preserved byte-for-byte after removing this inserted section. Only `docs/907/nsh-reading-editing-integrated-validation.md` is written by this lane. No Cargo/rustc, formatter, product/test execution, network, Git mutation, cleanup, Rust or other-document write is performed here. Explicit continuation restrictions override the normal fetch/update step; no remote currency is claimed.

The retained dirty-worktree base is **`0b5ea426abfd8d4358489e916273d89474fb190f`**; this lane does not run Git to recertify HEAD. The following results belong to the parent's **compiled snapshot**, not an immutable manifest of all current dirty sources or a final head. Parent is subsequently compiling the workspace app, so artifact paths may be replaced. Newer app-build files are outside this delta and are not silently included in these results.

### Actual toolchain, build and launcher provenance

Parent supplies the actual official toolchain identities **`cargo 1.98.0 (797e8a9bc 2026-08-05)`** and **`rustc 1.98.0 (88d9e12ae 2026-08-18)`**. These are fresh parent execution observations, not a version query by this writer. The exact parent-supplied executed UI build command is:

```sh
CARGO_FORK_AUTO_UPDATE=0 rustup run 1.98.0 cargo test -p vega_ui --features nsh-reading-integration --lib --test nsh_native_host --test nsh_reading_geometry --test nsh_reading_integration --test nsh_scroll_integration --test nsh_icons_integration --test nsh_settings_integration --test nsh_recovery_integration --offline --locked -j2 --target-dir target/official-nsh-validation --no-run --message-format json
```

`target/nsh-current-ui-build.stderr:1–2` records compilation of `vega_ui` and **20.73s** successful `test`-profile completion. `target/nsh-current-ui-build.json:876` is **`{"reason":"build-finished","success":true}`**; parent reports process **exit 0**. The eight executable records selected below each have **`profile.test=true`**, `fresh=false` and feature `nsh-reading-integration`. The UI lib is target kind `lib`; the other seven are kind `test`. A non-test library artifact is not an extra executed suite.

Parent reports each JSON-selected exact executable was run **once, sequentially**, using Python **`subprocess.run`**, **no binary arguments**, **`timeout=120` seconds per executable**, with stdout/stderr redirected to the paths in `target/nsh-current-ui-execution.json`. That retained JSON supplies exact paths, execution SHA-256, exit status, elapsed time and `watchdog_timeout`; it does not contain the full Python launcher source or inherited environment, which remain unavailable here. No historical/proposed launcher is relabelled as this invocation. All eight entries report **exit 0** and **`watchdog_timeout=false`**.

### Eight actual executions on this compiled snapshot

Paths in the evidence column are relative to `target/`; elapsed values are process-wall measurements from execution JSON, not the libtest footer's internal timing.

| Scope / exact target | Actual result | Elapsed | Retained result evidence |
| --- | --- | --- | --- |
| UI lib / `vega_ui` | **347/347 passed** | **2.161s** | `nsh-current-vega_ui.stdout:351`: 347 passed, 0 failed/ignored/measured/filtered; 347 named `test … ok` lines |
| Native host / `nsh_native_host` | **36/36 passed** | **0.911s** | `nsh-current-nsh_native_host.stdout:40`: 36 passed, 0 failed/ignored/measured/filtered; 36 named `test … ok` lines |
| Geometry / `nsh_reading_geometry` | **24/24 passed** | **6.190s** | `nsh-current-nsh_reading_geometry.stdout:1–25`: 24 named PASS lines, 0 failed/ignored |
| Source / `nsh_reading_integration` | **14/14 passed** | **27.709s** | `nsh-current-nsh_reading_integration.stdout:1–15`: 14 named PASS lines, 0 failed/not executed/ignored |
| Scroll / `nsh_scroll_integration` | **16/16 passed** | **88.382s** | `nsh-current-nsh_scroll_integration.stdout:1–17`: 16 named PASS lines, 0 failed/not executed/ignored |
| Icons / `nsh_icons_integration` | **3/3 passed** | **3.335s** | `nsh-current-nsh_icons_integration.stdout:1–4`: 3 named PASS lines, 0 failed/not executed/ignored |
| Settings / `nsh_settings_integration` | **3/3 passed** | **4.152s** | `nsh-current-nsh_settings_integration.stdout:1–4`: 3 named PASS lines, 0 failed/ignored |
| Recovery / `nsh_recovery_integration` | **2/2 passed** | **5.312s** | `nsh-current-nsh_recovery_integration.stdout:1–3`: 2 named PASS lines, 0 failed/not executed/ignored |

No failed, ignored, unexecuted or timed-out case is reported in these eight runs. Geometry and Settings do not print a separate not-executed counter; their full named PASS counts and parent execution report establish completion, rather than an invented footer field. Focused runtime-stderr searches found no panic, FAIL, assertion-failed or fired/expired-watchdog markers; large stderr files were hashed in full without giant reads. Source again includes **mount/owner ABA and manual-wheel cancellation**, and this newer build now covers the parent's collapsed-`if` body change. Recovery passes both `draft_summary_opens_separate_overlay_with_real_focus` and `pending_draft_close_escape_and_authority_keep_recovery_fenced`. The earlier summary-selector failure, later Tab→Accept failure, watchdog interruption, ABA/manual-wheel failures and their unexecuted cases remain historical failures, not retroactively passed attempts. Automated `nsh_native_host` GREEN is **not three-desktop native acceptance**.

### Exact executable identities and contemporaneous path observations

Each executable is under `target/official-nsh-validation/debug/deps/`. The full basename and execution SHA-256 come from retained execution JSON and match its unique test-profile artifact in build JSON. At **2026-11-09T06:50:10Z**, all eight paths still hashed to these execution identities; size/mtime were unchanged during each hash read. This is a **point-in-time match**, not a promise that a concurrent/later build leaves paths unchanged, nor retrospective proof of an immutable source tree.

| Target / build JSON line | Executable basename | Execution SHA-256 |
| --- | --- | --- |
| `vega_ui` / 862 | `vega_ui-4268349f2e74c4f3` | `8354638a73a64ba30514a3be44ff312f35a85a69e0d44d62765867747cf2a267` |
| `nsh_native_host` / 875 | `nsh_native_host-c6fd6b079b892425` | `b3cf1a6955c03ca1f9d52b2e757621d96f58f2aac341d69b14b0950ee536a97a` |
| `nsh_reading_geometry` / 864 | `nsh_reading_geometry-da777a5f64c485cf` | `71507267021657a0460be814720080bddcd9c40219e23f723b0bc8ca5b9f96fa` |
| `nsh_reading_integration` / 866 | `nsh_reading_integration-c754697d113fc18b` | `1b4b1d80123c1f3a8b2ec911fba7ee559592089372d38b929d63c8224f28720a` |
| `nsh_scroll_integration` / 870 | `nsh_scroll_integration-1d1fd85936d176b3` | `85a2060c20af6a5f27940b851d3f44fa8e6a9fd082349c1ff69512174d9fd2f9` |
| `nsh_icons_integration` / 872 | `nsh_icons_integration-c7d68528d5563f59` | `d6a4a3de5ec39c7259c5def3e1276846fbf13c15dcccc311120dfa26dced71f1` |
| `nsh_settings_integration` / 874 | `nsh_settings_integration-c1b0a9b018bd18c8` | `7486e631584e2f85377e41cae10b67774a4fb45b3b4e2f215d15f7c3f1be0fe9` |
| `nsh_recovery_integration` / 868 | `nsh_recovery_integration-9271aa7368d76ca1` | `4b59d961df004c0a32c24571969103cd66676a50ebf1c1d5a1877b3faeb97f88` |

At **2026-11-09T06:49:48Z**, read-only source hashing gave the identities below. Parent reports only rustfmt changes to the three formatted UI paths, and that the workspace harness author stopped after introducing `mounted_observations.rs`. These are parent-supplied change/ownership boundaries plus actual later source observations, **not a complete independently certified compile-input manifest**.

| Observed source path | SHA-256 |
| --- | --- |
| `crates/vega_ui/src/conversation_stream/selection_menu/body.rs` | `8fdc0a1f37671a186f54fb008af4f9888011966e3370bf009efaeaf73ada935a` |
| `crates/vega_ui/src/conversation_stream/mod.rs` | `bd514e597cd498febc356d2cd2501a48773f4dc1e4eb96dceee75431c30c2a7c` |
| `crates/vega_ui/src/conversation_stream/selection_menu/mounted.rs` | `2f1ac88cd5a677eb73568d84f24e80d7f094da0527d6879d8862a497a6764c6b` |
| `crates/vega_ui/src/disclosure/scroll_group/host_regressions.rs` | `314779da154af018d2195abcafb1dd7eedeef9372fe1a3eb3eb2959115247504` |
| `crates/vega/src/window/nsh_workspace/mounted_observations.rs` | `86deab2f362e736fb219f7db69cfc4362f8ec313ad6ea7dc69ecd929cd408f7c` |
| `crates/vega/src/window/nsh_workspace/mounted_regressions.rs` | `76db729afb4cde3fad3693e2a4f2735d4b12b0840ac21b5aec78e4c9cf9f01f1` |

The observed body identity exactly matches the parent-specified compiled body and the preceding document's post-collapse observation; the old “GREEN predates collapsed-if body” gap is superseded for this compiled snapshot. Its mtime is **06:27:40Z**; the three formatted files have mtimes **06:40:55Z**. These timestamps support chronology but are not alone compile-input proof. `body_integration_tests.rs`, recovery `mounted_regressions.rs`, the source/recovery integration entrypoints and backend `render.rs` still match their full SHA-256 values in the preceding section's source table, including backend **`b602deb11d26198704390ac10eb5969c66046ded6676c8e570a04d362c9e003d`**. The workspace mounted-regressions hash has changed from the prior section; this is explicitly a later harness identity, not silently the old source version. No claim is made that all current workspace bytes are covered by these UI executions.

### Formatting and strict workspace Clippy: previous blockers cleared locally

Parent reports the corrected full-workspace formatting check **exit 0**, after rustfmt-only corrections in `conversation_stream/mod.rs`, `selection_menu/mounted.rs` and `disclosure/scroll_group/host_regressions.rs`. Both retained `target/nsh-resumed-fmt-corrected.{stdout,stderr}` files are **empty**; they cannot independently establish invocation, exit status or duration. Exact formatter argv/duration are unavailable in those files; the successful result is attributed to the current parent report, not inferred from emptiness or borrowed from a workflow example.

Parent reports the subsequent strict **workspace/all-targets Clippy**, features **`vega_ui/nsh-reading-integration,vega/nsh-workspace-integration`**, **`--offline --locked -j2`**, target directory **`target/official-nsh-validation`**, with **`-D warnings`**, **exit 0**. `target/nsh-current-workspace-clippy.stderr:12` confirms successful `dev`-profile completion in **8.84s**, including the workspace app and UI checks; stdout is empty. These are the established command parameters, not a reconstructed exact full shell command: the retained files do not capture the complete launcher prefix/environment/argv. This successful local result **clears the previous nine dead-code diagnostics** in the duplicated app-bin test harness and supersedes the earlier `collapsible_if`/dead-code blockers, while both failed attempts remain below.

Parent also reports a `wait_ms` attempt rejected with **`workspace_sandbox_required`** before execution; the corrected **blocking** tool call actually ran. That rejection is a **nonexecuted tool/session-mode failure**, not a Cargo/build failure, extra executed check or failed product regression. No separate retained log/audit for the rejected attempt is claimed here.

These are still **implementation/development checks before final review**. Source/recovery authors own their source lanes; parent owns validation and workspace/app compilation; this lane owns only this evidence document. Mainline compatibility documentation remains separately owned. No independent-review result, exact-final-head CI result, desktop-native acceptance, app launch, merge, package or install is supplied by this delta. Development-test archival/removal and **macOS/Windows/Linux native acceptance remain pending**. Earlier shell/workspace, Python, composer/policy and #920/runtime-store evidence remains at its separately recorded snapshot; it is not a new run of those groups. Kit **0.7.3**, Fast / Fast platform **0.1.9** remain unchanged. Updates use the user's fixed weights and report changed results/blockers only, with no total estimate or invented completion percentage. This lane stops writes after static readback and hands off the exact document SHA-256.

### Fresh retained-log identities

All paths below are relative to `target/`. These are actual full-file SHA-256 values. Hashes identify the retained files read here; they do not make mutable artifact paths permanent.

| File | SHA-256 |
| --- | --- |
| `nsh-current-ui-build.json` | `9b23593ff64cc610327d5a717ebcbe5f059c2761605150a2be3c833f4b1ad0a9` |
| `nsh-current-ui-build.stderr` | `0caa9925e6b4a3322902c3f7d523f128f8dfd6d561828b2910f7619f0d8209b7` |
| `nsh-current-ui-execution.json` | `50cbf79bb43659911c364bb7a10d06313a19de0ff0f4909adeee1ddb46275a8d` |
| `nsh-current-vega_ui.stdout` | `2b54f2f2ba66db39c7d6473a15fc4ef750e8aa3df5bcbcfc6191f028f9e49ae5` |
| `nsh-current-nsh_native_host.stdout` | `67215571388d7bef71d9b239af10a60d74b937a9181762bca7d9f506911824f9` |
| `nsh-current-nsh_reading_geometry.stdout` | `59600e972c5380677b5f8ff6afbf1bf23dc2b855f8e107108f266316f560297a` |
| `nsh-current-nsh_reading_geometry.stderr` | `85e36a0cb4709c5a55240c40deb7b9a00aa461f37c35f16d7edff133fac783fa` |
| `nsh-current-nsh_reading_integration.stdout` | `6e07f4ec46c4096af3e8148da0248d8767804c755cfe992248e4895ed012e171` |
| `nsh-current-nsh_reading_integration.stderr` | `b2cef5d70008120786ed41f0cad158f2cae30e31fcc39ceee4d34b7ccd1e4a23` |
| `nsh-current-nsh_scroll_integration.stdout` | `5e3b12f7cb3cbf7b16577739653275717d76285f472a095dd12a58fa538390cf` |
| `nsh-current-nsh_scroll_integration.stderr` | `d66e8ebcd268b34cb33cf9bdd7ea01681f6c9636fb015fea4e208c3c15ac2872` |
| `nsh-current-nsh_icons_integration.stdout` | `9dd4a6c2ee6ecaa71af30d6404b9bb0929572d21e57d100382177679718322c8` |
| `nsh-current-nsh_icons_integration.stderr` | `696a0908c1f2b1017c345932aff69fcfa956690fa632ca4d97b0a34f57a8cd48` |
| `nsh-current-nsh_settings_integration.stdout` | `b57f873f5d8625dab01193161d46fe1f462a0cec4249c804c7adead0ebb36637` |
| `nsh-current-nsh_settings_integration.stderr` | `7449a12fc9443143ac21cbe2f163eae98f0f161f989c812156efc7afb2a5a961` |
| `nsh-current-nsh_recovery_integration.stdout` | `d6a0c06e9d1de2519595bc6a48e6c466141237064ca96d4485408e221e6fb4a3` |
| `nsh-current-nsh_recovery_integration.stderr` | `bb3d532d5c2a41b9abec2985a432a41f59231f06e3d20783f44fd2ba3476873c` |
| `nsh-current-workspace-clippy.stderr` | `ecfa441a94d5243d9b50ac11da83f97c0099015fe40a661331907ea2154927f5` |

These **five** files are empty, each SHA-256 **`e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`**: `nsh-current-vega_ui.stderr`, `nsh-current-nsh_native_host.stderr`, `nsh-current-workspace-clippy.stdout`, `nsh-resumed-fmt-corrected.stdout`, `nsh-resumed-fmt-corrected.stderr`. Empty files are not additional passed checks.

---

## Final-development followup — current-round evidence after `40d8a0b`

This is **documentation implementation, not independent review or final acceptance**. This section supersedes current-tense boundaries below; the complete preceding document, SHA-256 **`40d8a0b80a80e4fea339fc1e8b60155d311e800799091e376c955fd0edf099c6`**, is retained without deleting historical failures, commands, results or hashes. Only this document is written. This evidence writer performs no Cargo/rustc, product/test execution, network, Git mutation, cleanup or other-document/source write. The explicit continuation restrictions take precedence over the usual fetch/update step; no remote currency is inferred.

Worktree `/Users/puzige/Workspace/worktrees/vega-907-nsh-full` remains dirty at base HEAD **`0b5ea426abfd8d4358489e916273d89474fb190f`**. All results below are **compiled-snapshot evidence**, not exact-current dirty-source acceptance. Artifact paths can be overwritten by parallel builds; Cargo JSON records paths/profile, not binary SHA-256 or an immutable dirty-source manifest. Parent-reported process observations are distinguished from retained-file footers.

### Successful builds and exact test-profile selection

| Build evidence | Actual retained completion | JSON scope and boundary |
| --- | --- | --- |
| `target/nsh-resumed-observation-build.{json,stderr}` | stderr line 2: **28.87s**, `test` profile; JSON line **868**: `build-finished`, `success=true` | UI lib-test, non-test UI library, source and recovery integration artifacts, `nsh-reading-integration`; all four relevant artifact records `fresh=false`. Source run below belongs to this observation snapshot. |
| `target/nsh-resumed-app-build.{json,stderr}` | stderr line 6: **1m 06s**, `test` profile; JSON line **883**: `build-finished`, `success=true` | `nsh-workspace-integration`; app lib, workspace integration executable, both test and non-test app binaries, plus UI library. A successful build is not an app launch. |
| `target/nsh-resumed-recovery-navigation-build.{json,stderr}` | stderr line 2: **27.24s**, `test` profile; JSON line **866**: `build-finished`, `success=true` | UI lib-test, non-test UI library and recovery integration executable, `nsh-reading-integration`; all three relevant artifact records `fresh=false`. Recovery and policy below belong to this build. |

Parent reports the first app-binary selector asserted **two** matching binaries **before execution**. JSON confirms why: line **880** has target `vega`, kind `bin`, **`profile.test=true`**, executable `debug/deps/vega-39df89ae657a2ad7`; line **882** has the same target/kind but **`profile.test=false`**, executable `debug/vega`. The corrected selector required **`profile.test`**, selecting the test executable. **No actual app was opened** by the failed selector or these regression executions. The two-match assertion is a launcher/observation fault, not a failing application test; it is retained as parent-reported audit context, not invented as a separate file log.

Exact current launcher argv/environment, watchdog scripts and full build/Clippy/Python shell invocations are **not recovered in these retained files**. No unavailable historical command, workflow example or proposed command is relabelled as executed now. The prior official Cargo/rustc **1.98.0**, offline/locked `-j 2` record remains prior-round provenance, not a fresh tool-version query or independently established full argv for every new invocation. This writer does not run the toolchain to fill that gap.

### Actual current-round executions

| Scope | Actual result | Evidence and limit |
| --- | --- | --- |
| Source reading/editing | **14 passed, 0 failed, 0 not executed, 0 ignored** | `target/nsh-resumed-source-intent-cycle.stdout:1–15`, fourteen named PASS lines and explicit footer. Includes owner/mount ABA **and manual-wheel cancellation**, then all eight previously unexecuted cases. GREEN for the observation-build compiled snapshot only; later `body.rs` edits are not certified by this run. |
| UI library | **347 passed, 0 failed, 0 ignored, 0 measured, 0 filtered out**, **0.43s** | `target/nsh-resumed-observation-ui-lib.stdout:351`; actual full lib-test execution for the observation snapshot, not a current full-workspace gate or current replacement lib-test bytes. |
| Application shell chrome | **4 passed, 0 failed, 0 ignored, 0 measured, 27 filtered out**, **0.01s** | `target/nsh-resumed-vega-regression.stdout:3–8`; four permission/authority/remount/update-callback regressions on the corrected test-profile app selection. No real app/native desktop acceptance. |
| Workspace mounted + actual controller | **6 passed, 0 failed, 0 not executed, 0 ignored** | `target/nsh-resumed-nsh_workspace_mounted-regression.stderr:1–13`; stdout empty. Three mounted plus three controller cases; fresh app-build snapshot evidence, not just the earlier six-case run. |
| Mounted recovery navigation | **2 passed, 0 failed, 0 not executed, 0 ignored** | `target/nsh-resumed-recovery-navigation.stdout:1–3`; both original cases executed. Parent reports **4.549s**, **watchdog=false**. Retained stderr ends with second case `unwinding=false`; process timing/watchdog metadata is parent-reported, not a fabricated footer. |
| Recovery policy, same navigation build | **26 passed, 0 failed, 0 ignored, 0 measured, 321 filtered out**, **0.01s** | `target/nsh-resumed-recovery-navigation-policy.stdout:30`; separate actual selection, not mounted/native acceptance or all 347 cases rerun on this later build. |
| Python unittest | **40 tests, OK**, **0.350s** | `target/nsh-resumed-python.stderr:43–45`; actual retained unittest result. Stdout's `Publishing full-only delta manifest: Unsupported bundle path` is not a test-suite failure verdict; preserve it alongside the successful footer, without treating it as an i18n/release/computer summary. |

Parent's current Python/i18n tool-output evidence additionally reports **i18n: 0 errors / 0 findings**, **release: 5**, **computer-use: 15** successful cases. Those three results are **tool-output-only evidence supplied in this continuation**, not present in `target/nsh-resumed-python.*`; no separate retained log, exact invocation, timing or hash is invented for them. They do not establish final-current i18n/Python gates after further source mutations.

### Historical recovery failures remain failures

- First summary-selector/bounds run remains `target/nsh-resumed-recovery-cycle.*`: **0 PASS / 1 FAIL / 1 not executed**, **3.195s**, no timeout; `debug_bounds("shell-recovery-summary-native-host")` was `None` at compiled fixture line **558**. Initial `open_window` had returned; the notify loop was already resolved for that run. Earlier watchdog interruption remains separately recorded below, not recounted as this assertion failure.
- Intermediate selector run is now explicitly retained: `target/nsh-resumed-recovery-selectors.stdout:1–2` records **1 passed / 1 failed / 0 not executed / 0 ignored**. The summary case passed, but stderr **2238–2242** reports compiled `mounted_regressions.rs:494:9`, **`enabled control not reachable by Tab: Accept`**, then FAIL for `pending_draft_close_escape_and_authority_keep_recovery_fenced`. It is not silently converted to GREEN by the later navigation build.
- The **27.24s navigation build** and its two-PASS run supersede these as the latest completed mounted-recovery snapshot, without deleting either failure or claiming OS focus/AX acceptance. No assertion, budget or acceptance change is made by this documentation lane.

### Artifact identity, overwritten paths and source observations

The following SHA-256 values were read from actual files at **2026-11-09T06:32:40.723219+00:00**. Paths are relative to `target/official-nsh-validation/`; JSON selects paths, while this writer computes hashes. Parent associates source/recovery runs with the two supplied binary identities, and both match retained bytes. There is still no per-run launcher receipt sealing a full source manifest.

| Artifact / provenance | Observed SHA-256 |
| --- | --- |
| `debug/deps/nsh_reading_integration-c754697d113fc18b`; observation JSON line **865**, parent source-run identity | `867993ab6719a2c3be662034630717baca29070b3042a4cc0891805a7dbcdfac` |
| `debug/deps/nsh_recovery_integration-9271aa7368d76ca1`; navigation JSON line **865**, parent navigation-run identity | `a02cc99634fe124588946b886f1913e70679528a75bcb2a66c59fe6406f01569` |
| `debug/deps/vega_ui-4268349f2e74c4f3`; navigation JSON line **862**, later lib-test bytes | `79d6f70f3986e3cd748cbe37582d0350db16953c149ac7e5eb6092d166b12e2c` |
| `debug/deps/nsh_workspace_mounted-bc600955a0a1fea1`; app JSON line **879** | `1256db528c7a17a8bf75db7ecae2ca881bb7dd412b759f18fb827a12a5f0a36d` |
| `debug/deps/vega-39df89ae657a2ad7`; app JSON line **880**, test profile | `97d34d625f30c2d166ae6fbf001aaa206af08dacdb5a94b5e1dc37aa936874d2` |
| `debug/vega`; app JSON line **882**, non-test, **not launched** | `96c7b5fb0c5b0da6c67b0f95e88c07690b7de8c49daa93611db3ec2484585440` |

Observation-build JSON also names the same lib-test/recovery/library paths subsequently emitted by navigation-build JSON. **Do not attribute the later lib-test hash to the earlier 347-case execution, or navigation recovery bytes to the intermediate Tab failure.** Original earlier hashes and overwrite observations remain in the preceding-round history below. A build record naming a path does not prove the bytes currently there were those executed at that earlier time.

Parent reports the subsequent source-body edit only collapses the nested `if` without changing semantics. Current inspection confirms the combined `session.matches`, matching `area` and changed `scroll_node` condition in `Request::ScrollBinding`, but does not independently establish a complete before/after dirty diff or rerun source tests. `body.rs` mtime **2026-11-09T06:27:40Z** is later than observation JSON **06:13:31Z** and navigation JSON **06:25:26Z**. The following are **later source observations, not certified compile-input identities**:

| Source path | Observed SHA-256 |
| --- | --- |
| `crates/vega_ui/src/conversation_stream/selection_menu/body.rs` | `8fdc0a1f37671a186f54fb008af4f9888011966e3370bf009efaeaf73ada935a` |
| `crates/vega_ui/src/conversation_stream/selection_menu/body_integration_tests.rs` | `747426f974851330790b6a253967de9429b5f336292b09073020119bae2f2d84` |
| `crates/vega_ui/src/conversation_stream/shell_recovery/mounted_regressions.rs` | `a80aaf94e66c8956f970b556bbf48ab246bc6206cc9078ac255a5c9ccc81eff5` |
| `crates/vega_ui/tests/nsh_reading_integration.rs` | `87353d60ea543b0abf318004dee861ea0f7f278a1f891f3423da0d4692384df8` |
| `crates/vega_ui/tests/nsh_recovery_integration.rs` | `1c7bb60deb24323afc88bf19175e99961128c5c8c2b959c834ee7669eaf5337b` |
| `crates/vega/src/window/nsh_workspace/mounted_regressions.rs` | `9aaff4201242a9e4e66a1cd6a0600387a6406494c4535f16718ed5d5d3cec8d5` |
| `vendor/nsh-platform-gpui/src/render.rs` | `b602deb11d26198704390ac10eb5969c66046ded6676c8e570a04d362c9e003d` |

### Strict workspace Clippy is still blocked, not final review

1. `target/nsh-resumed-workspace-clippy.stderr` first failed on **`clippy::collapsible_if`** at compiled `selection_menu/body.rs:1793`; both UI lib and lib-test compilation failed. That real failure is retained. The parent's later body edit above addresses the nested condition; source-suite GREEN remains tied to the older compiled snapshot.
2. `target/nsh-resumed-workspace-clippy-fixed.stderr:3–92` next fails on **9 dead-code diagnostics** in `crates/vega/src/window/nsh_workspace/mounted_regressions.rs`, ending **`could not compile vega (bin "vega" test) due to 9 previous errors`**. These are `LINE`, `BASE`, `Events`, `Root`, `Fixture`, one grouped associated-items diagnostic and three mounted-case functions. Parent identifies the cause as the development workspace module being duplicated into the app test binary; the harness author owns that repair in parallel. No later successful strict-workspace result is available in this handoff, and no warning suppression, assertion weakening or Rust write is performed here.

Source author, recovery author, parent validation and this evidence lane remain separate; the harness repair belongs to its author, and mainline compatibility documentation remains separately owned. This is still **implementation/development validation before final review**. Consolidated independent review, exact-final-head required CI, development-test archival/removal and **macOS/Windows/Linux desktop-native acceptance remain pending**. Latest geometry **24/24**, icons **3/3**, Settings **3/3**, native host **36/36**, scroll **16/16**, composer **6/6** and #920/runtime-store checks remain the prior followup's evidence, not reruns silently assigned to these newer bytes. Kit **0.7.3**, Fast / Fast platform **0.1.9** remain unchanged; no new Kit/Fast or mainline claim is made.

Next parent-owned verification is the repaired harness's strict workspace check and appropriately scoped verification of stopped final source bytes. There is no current-final/full-production GREEN, merge, package, install or native acceptance claim. Updates report only changed results and blockers with the user's fixed weights; no total estimate or invented percentage. This documentation lane stops writes after static readback and supplies its exact document hash separately.

### Retained current-round evidence SHA-256

Paths below are relative to `target/`; hashes are actual retained files, not tool-output reconstructions. Large runtime stderr files were hashed in full but inspected with focused failure searches/tails, not giant reads.

| File | SHA-256 |
| --- | --- |
| `nsh-resumed-observation-build.json` | `66af43b7b4942e59d3f48463d698218e9e93faa8275c8025bbd82b46b0ae3aaf` |
| `nsh-resumed-observation-build.stderr` | `15f5d180e1a159c68d974b33292f69bdbf1865601b6984b0bc8771e1a94ff1cf` |
| `nsh-resumed-source-intent-cycle.stdout` | `6e07f4ec46c4096af3e8148da0248d8767804c755cfe992248e4895ed012e171` |
| `nsh-resumed-source-intent-cycle.stderr` | `0c8995516dfbec92ea1fac85c2a187dbe92b53fee7eada34d33417e3296fe5c6` |
| `nsh-resumed-observation-ui-lib.stdout` | `6a9087b10db4bbe608994a5b9d705733a619f051ee444ad5a43072fbb8b5a7a1` |
| `nsh-resumed-app-build.json` | `9696c591a0e5d51e1f3f92f6e18bce5c22bff98a2dba671fe2184ef7abddf0e0` |
| `nsh-resumed-app-build.stderr` | `04dc12a0389c614ddb88c206561833604669a1d9d5ed5dafcdcee11321f93aaa` |
| `nsh-resumed-vega-regression.stdout` | `8cc756c37c34b1d8798b2beb5acc62833cf1d13c8a48d471193d7a2e3a06201d` |
| `nsh-resumed-nsh_workspace_mounted-regression.stderr` | `9b54eed47ade67bebfe1e2a566c5ff0806961fe289d762dc895a7d56b09fd3cb` |
| `nsh-resumed-python.stdout` | `23a41c17f8e5a4ee94ea2832e4182877371a9d4b8faa25cc52993bdf5d2ba98c` |
| `nsh-resumed-python.stderr` | `1e95abbd031aff496ffa811028332314e0160e2642065d2f9270628ea1130c5d` |
| `nsh-resumed-recovery-selectors.stdout` | `d3f22a49ec7a48eede47eafe46491bd7e07db40fcb4b47a1820e295c2d918856` |
| `nsh-resumed-recovery-selectors.stderr` | `789b7189dec026b7ab48a1a4ceb78a6f1c9408b13e04c33f5824bce0575b34ee` |
| `nsh-resumed-recovery-navigation-build.json` | `aecb28e7f9e012e9a516912e6ed1aa2ebf3b29da02f140ca1c2cc26be129c7be` |
| `nsh-resumed-recovery-navigation-build.stderr` | `c624865cf50993a02159888b2f083dcf74ef1abed274494bc903ef1a16b4af87` |
| `nsh-resumed-recovery-navigation.stdout` | `d6a0c06e9d1de2519595bc6a48e6c466141237064ca96d4485408e221e6fb4a3` |
| `nsh-resumed-recovery-navigation.stderr` | `644ba531d376dd7848cb84831178d2701661cab888a56c098b0d3c2bafe8b1a0` |
| `nsh-resumed-recovery-navigation-policy.stdout` | `a2e9a11943ff70949034e04160e5a3bcf6410c5b061613da9e975b09815e7fa3` |
| `nsh-resumed-workspace-clippy.stderr` | `24809a15af209cf915923f5db77216dd9de1be411cd187df20ec14709763d3b3` |
| `nsh-resumed-workspace-clippy-fixed.stderr` | `b65d2b2c47f3934975657a836922d91ef84ba2bc08ad3585cd0a59c6c90c354d` |

These six retained files are empty, each SHA-256 **`e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`**: `nsh-resumed-observation-ui-lib.stderr`, `nsh-resumed-vega-regression.stderr`, `nsh-resumed-nsh_workspace_mounted-regression.stdout`, `nsh-resumed-recovery-navigation-policy.stderr`, `nsh-resumed-workspace-clippy.stdout`, `nsh-resumed-workspace-clippy-fixed.stdout`. An empty file is not an additional passed test.

---

## Resumed followup — compiled-snapshot evidence after `d6a6987`

This is **documentation implementation, not independent review**. This section supersedes current-tense boundaries in the retained history below, without deleting any earlier failure, command or hash. The complete preceding document had SHA-256 `d6a6987abf7ffce8d2a0e8f98f3a867871762085c7686da888263905a521d685`. Only this document is written here; no Cargo/rustc, product/test execution, formatter, network, Git mutation or cleanup is performed by this evidence writer.

The dirty worktree remains `/Users/puzige/Workspace/worktrees/vega-907-nsh-full`, base HEAD **`0b5ea426abfd8d4358489e916273d89474fb190f`**. These are **compiled-snapshot results, not final/current-source acceptance**: source and recovery authors may change files after compilation. The JSON identifies artifacts, not a complete immutable dirty-source archive. Later file hashes below are explicitly observations, not retrospectively assigned build inputs.

### Build, backend and exact upstream integration

- Parent's actual `target/nsh-resumed-integrated-build.json` ends at line **876** with `{"reason":"build-finished","success":true}`; stderr line **5** records **1m 03s**, `test` profile. Parent reports **exit 0**, official Cargo/rustc **1.98.0**, offline/locked, **`-j 2`**, target directory `target/official-nsh-validation`. Previously recorded detailed tool versions are Cargo `1.98.0 (797e8a9bc 2026-08-05)` and rustc `1.98.0 (88d9e12ae 2026-08-18)`; they were not re-queried here. JSON confirms `nsh-reading-integration`, debug/test profiles and the actual artifacts below.
- Count the actual JSON, not a shorthand target estimate: **8 executable artifacts = 7 integration executables + 1 UI lib-test executable**, plus **1 non-test UI library artifact** (`.rlib`/`.rmeta`). All nine UI artifact records are `fresh=false`. This is the evidence behind the parent's “8 targets + lib” handoff; it is not 8 integration executables plus an extra lib-test. No workspace executable is listed in this build.
- Backend default check: `target/nsh-resumed-backend-default-check.stderr:1–2` records `nsh-platform-gpui v0.4.0`, successful dev-profile completion in **3.50s**; parent reports **exit 0**. The original feature-check attempt remains `target/nsh-resumed-backend-check.stderr:1`: `error: cannot specify features for packages outside of workspace`. That invocation failed **before compilation** and is not a successful check. The presence patch `vendor/nsh-platform-gpui/src/render.rs`, SHA-256 **`b602deb11d26198704390ac10eb5969c66046ded6676c8e570a04d362c9e003d`**, now has compilation and snapshot runtime evidence; the historical “uncompiled/unrun” boundary remains below as history only.
- Parent manually integrated the exact **five-path #916 → #920 delta**, without committing/rebasing the dirty branch: upstream #920 **`0b3dcd3603a8e97e0ef74ae25a26f17fc3a094e5`**, parent #916 **`f3688f5ae0b7becf1f46434c7b7d732e3a3c759a`**. This writer independently byte-compared each listed file to the local upstream object using read-only `git show`; all five match. This establishes byte integration, not merged ancestry, historical #920 test execution, or fresh remote currency. Mainline compatibility documentation remains owned by its separate author.

| Five-path integrated content | SHA-256, matching upstream #920 bytes |
| --- | --- |
| `crates/vega_conversation/src/agent/pipeline.rs` | `de7a261fa5a6038d5aee0f6da45ff9252bcfeb72ff7c218b7da8064735db0ea5` |
| `crates/vega_store/src/lib.rs` | `ec23a8663ccd428e75f9ae8bec0131331fd2f72dd52d2f04c4a03cc84cbdd4f3` |
| `crates/vega_store/src/write_coordination.rs` | `c49370c7fecccdf9556941e7b9ba1a6e69f47e28daa20773606f5929fdbc8107` |
| `docs/918/preparation-concurrency.md` | `6c61f850cc98ee72b13c692b96611563f72a1c1be7d10a7c6c2ce0890fb5e976` |
| `docs/918/validation.md` | `0b0bc2a238bccc6b32a240952f13d17ab1ef6387919129d30f805fb0ef7869fe` |

`target/nsh-resumed-mainline-check.stderr:1–3` records successful completion in **2.92s**; `target/nsh-resumed-mainline-clippy.stderr:1–9` records **8.54s**, both **exit 0** per the actual parent execution record in `nsh-mainline-compatibility.md` (its “Parent integration result” section). That record establishes these commands and common options: official Cargo/rustc 1.98.0, `CARGO_FORK_AUTO_UPDATE=0`, `--offline --locked -j 2 --target-dir target/official-nsh-validation`:

```sh
cargo check -p vega_conversation -p vega_store
cargo clippy -p vega_conversation -p vega_store --all-targets -- -D warnings
```

These are the recorded executed command scopes with the recorded common options above, not the later, broader **proposed** UI/application Clippy command in that document. The backend/build/runtime log files do not themselves retain full launcher argv/environment or watchdog scripts. Their exact shell invocation is **not recovered in this continuation**; JSON target/artifact identities, actual footers and the parent's supplied toolchain/flags/exit/timing records are recorded instead. Do not reuse a historical unavailable command, a proposed command, or an artifact filename as proof of a current exact invocation. No validation command is rerun here.

### Actual executions on that compiled snapshot

| Scope | Actual result | Retained evidence and limit |
| --- | --- | --- |
| Composer selector policy | **6 passed, 0 failed, 0 ignored; 341 filtered out** | `target/nsh-resumed-composer-policy.stdout:2–10`, **0.03s**. Six real policy/model/notification regressions, not all 347 discovered library cases or mounted recovery acceptance. |
| Source reading/editing | **5 passed, 1 failed, 8 not executed, 0 ignored** | `target/nsh-resumed-source-presence-cycle.stdout:1–6`; explicit runner footer, not derived totals. First five source-caret cases, **including owner/mount ABA**, passed. Sixth `source_caret_manual_scroll_rejects_pending_and_delayed_follow` failed; stderr **32486–32490** records compiled fixture `body_integration_tests.rs:1412:13`, `assertion failed: state.source_anchor.is_none()`, after the later real manual wheel. Later eight cases did not execute. No source-suite GREEN. |
| Mounted recovery | **0 passed, 1 failed, 1 not executed, 0 ignored** | `target/nsh-resumed-recovery-cycle.stdout:1`; parent reports **exit 1 in 3.195s, no timeout**. First `draft_summary_opens_separate_overlay_with_real_focus` failed; second original case did not execute. This is a completed failing runner, not the earlier 25s watchdog interruption. |
| Real-font geometry/opacity | **24 passed, 0 failed, 0 ignored** | `target/nsh-resumed-nsh_reading_geometry.stdout:25`; new emitted-binary run, not just the older 24-case snapshot. No extra not-executed count is invented. |
| Shared icons/tooltip | **3 passed, 0 failed, 0 not executed, 0 ignored** | `target/nsh-resumed-nsh_icons_integration.stdout:4`; independent emitted-binary execution, not raster pixels or real assistive-technology acceptance. |
| Settings natural height | **3 passed, 0 failed, 0 ignored** | `target/nsh-resumed-nsh_settings_integration.stdout:4`; independent emitted-binary execution, not complete native Settings acceptance. |
| Native host | **36 passed, 0 failed, 0 ignored, 0 measured, 0 filtered out** | `target/nsh-resumed-nsh_native_host.stdout:40`, **0.21s**; independent emitted-binary execution, not macOS/Windows/Linux OS acceptance. |
| Nested scrolling + intermediate tool group | **16 passed, 0 failed, 0 not executed, 0 ignored** | `target/nsh-resumed-scroll-cycle.stdout:1–17`; actual body → group → original List, gesture/remainder regressions executed on this snapshot. |

The mount-ABA failure is no longer the executed source-restore blocker on this compiled snapshot; manual-wheel cancellation is now the boundary. This does **not** declare restore or subsequent author edits correct. The failure location is the line captured by the compiled run, not a promise that line 1412 still contains that assertion after author edits.

Recovery stderr **103** has paired `END id=8 new.open_window elapsed=100.191042ms unwinding=false`: initial window opening returned, so the earlier composer notify-loop interruption is resolved for this run. It then reached `assert_summary unavailable=true` (**480**) and panicked at compiled `shell_recovery/mounted_regressions.rs:558:14` (**649–650**), `Option::unwrap()` on `None`, finally `FAIL draft_summary_opens_separate_overlay_with_real_focus` (**657**). Source inspection identifies the failing lookup as `debug_bounds("shell-recovery-summary-native-host")`. The summary-host measurement failure is not a timeout or evidence of completed overlay/focus/service behavior. The logged case scope lasted **523.269209ms** (**656**); that is distinct from the parent's **3.195s process elapsed**. Assertions and original 2s / 512-tick / 10s budgets have not been relaxed here.

Workspace **6/6** and shell-recovery policy **26/26** remain retained earlier evidence, not fresh runs of those exact selections in this followup. No historical failed/interrupted run is converted into PASS. The full UI-lib/Python outputs are not used here to enlarge the requested subset claims or claim current full-workspace gates.

### JSON-selected artifact SHA-256

Paths below are relative to `target/official-nsh-validation/debug/deps/`. The JSON's `executable`/`filenames` fields select these paths; SHA-256 was computed from the actual retained files by this writer, **not supplied by Cargo JSON or inferred from filename suffixes**. Artifact hashes identify the files at inspection; the parent's handoff associates these emitted binaries with the execution logs. There is no per-run launcher receipt sealing an executable hash into each stdout footer, and no immutable full dirty-source manifest for this build is inferred.

| JSON line / artifact | Initially observed SHA-256 (see later overwrite observation) |
| --- | --- |
| 862 `vega_ui-4268349f2e74c4f3` (lib-test; composer policy) | `692fe8168ad1781302edf192b6392d2a0debe48437a951ceacfac9c192448808` |
| 863 `libvega_ui-92efa6e635a88bec.rlib` (non-test library) | `88ca0431ce92b7ccbbfd9f7ef69d4edc21da372e180c94ee874ead5242ff0071` |
| 863 `libvega_ui-92efa6e635a88bec.rmeta` | `b98c56d790faff541c462924a4470db17f29a652ac8368e249cb1c68de0ffc7b` |
| 864 `nsh_native_host-c6fd6b079b892425` | `1a2f8c657b23aa11daa20d686aea9ea4bb0b05c023e002e9e2a5b1b80b71313f` |
| 866 `nsh_reading_integration-c754697d113fc18b` | `689c0c2d7469e88c35402095a31a78b3768726b47ae9a1335bfed91a7cc7cc1d` |
| 868 `nsh_settings_integration-c1b0a9b018bd18c8` | `db28e868dcd2e415b01f458eab18cf6577b37845d07211f991a5ac8b1e08f2d8` |
| 870 `nsh_icons_integration-c7d68528d5563f59` | `c88517b36a83a9fe9c9c98e88f63790c6bdc9a6277c31ab3c5c89d9d635bf703` |
| 872 `nsh_recovery_integration-9271aa7368d76ca1` | `5e12e3c12802b72f065c0e8f8d711acfb3cfeb9c614de6f722e158c64a912a44` |
| 873 `nsh_reading_geometry-da777a5f64c485cf` | `43fab2cbe49d719d7646d7e37ed8e8c4e2832822466a9ba73d6fc46d764e97e9` |
| 875 `nsh_scroll_integration-1d1fd85936d176b3` | `19961bc3719fe6c26d5bf26db531b1da410f7de459e8bc45566a8fc2d6a864a2` |

### Final readback observation: artifact paths changed, retained logs did not

The first document static check successfully reconstructed the prior document hash and checked added-section whitespace/fences, then **failed** its current-artifact equality assertion: `vega_ui-4268349f2e74c4f3` now hashed to `30f01fc2e56fa72d050c5e1e3a5c812e84e2d948c3b8f3eede29a3360f77e4d1`, not the initially observed `692fe8168ad1781302edf192b6392d2a0debe48437a951ceacfac9c192448808`. This was an observation/provenance race, not a newly executed product test failure. Inspection at **2026-11-09T06:14:42.688171+00:00** confirmed all **16** retained evidence-file hashes below (including the build JSON) remained identical, while **5 of the 10** artifact files had changed at the same paths. The earlier table is retained as the initial observation, not a claim that every file still has those bytes. No old result is assigned to these replacement bytes, and the failed current-byte equality check is not relabeled PASS.

| Replaced artifact path (same deps directory) | SHA-256 at final readback observation |
| --- | --- |
| `vega_ui-4268349f2e74c4f3` | `30f01fc2e56fa72d050c5e1e3a5c812e84e2d948c3b8f3eede29a3360f77e4d1` |
| `libvega_ui-92efa6e635a88bec.rlib` | `3dd58180a074a2bf4289bdb70a2fc544163e335ea44e55d0d00675678c7a4e5c` |
| `libvega_ui-92efa6e635a88bec.rmeta` | `222b08c5440a4f0d97c23d8a8797bb4656a0178d037ee0465e6e2764c4bb0c19` |
| `nsh_reading_integration-c754697d113fc18b` | `867993ab6719a2c3be662034630717baca29070b3042a4cc0891805a7dbcdfac` |
| `nsh_recovery_integration-9271aa7368d76ca1` | `bf81228e70699c14d8d1ca26319c21e76bfc912a778e71fcf67a748b7d8a082d` |

The other five artifact files (native host, Settings, icons, geometry and scroll) still matched their initial SHA-256 at that observation. Changed-file mtimes were `06:13:10–06:13:31 UTC`; they are observations, not a recovered compiler/run receipt. Newly compiled source/recovery bytes need their own parent-owned logs and provenance. The appropriate next document verification checks retained-log identities, original footers and the explicitly historical versus later artifact observations separately; no rebuild or product rerun is performed here.

### Evidence and later source observations

All paths in the following table are relative to `target/`; these are the actual inspected retained files, not recreated output.

| Evidence file | SHA-256 |
| --- | --- |
| `nsh-resumed-integrated-build.json` | `2a17de09fd1428f534c1cd181c47be0ea7840b064376fd25b828049d63e81505` |
| `nsh-resumed-integrated-build.stderr` | `d3ec7d5ac24322f00ea989316e18cdaa61787d0b719bc01699985f6a831542fe` |
| `nsh-resumed-backend-check.stderr` (rejected invocation) | `e8ba25ea0890bf6630709e9f811ccb71944e489015181c521f81c609f9040c2a` |
| `nsh-resumed-backend-default-check.stderr` | `03d85a5802f15354d4be19221f7df64a61342703b2a06b6bf41b977b94ad4a9a` |
| `nsh-resumed-mainline-check.stderr` | `55d25b61c1aa416a9205ea1636bd84fa78b01f689f2ae52e67bd87c4b181c920` |
| `nsh-resumed-mainline-clippy.stderr` | `af4fb36f6c651d7405e28a592e50e85b4fe60cf9a4b45c4a6f3b6bf27cbf92dd` |
| `nsh-resumed-composer-policy.stdout` | `a715c725608ffb0e33c9f2667c4bd05de2fce202cc5e2037d3056235cbf81680` |
| `nsh-resumed-source-presence-cycle.stdout` | `98330616a5ec6408c4bf2a4ce444b82488fed1b741b953881c3a26eb8449b636` |
| `nsh-resumed-source-presence-cycle.stderr` | `8187bfa4b1d71ce1e7c4cfbb2fc1abe9960682fe71e359345dc4f5c04038522d` |
| `nsh-resumed-recovery-cycle.stdout` | `f833de9a98f9f91b2602e4c3d7df46968953a7b55b423c8ca0c95d3c9d63f3f5` |
| `nsh-resumed-recovery-cycle.stderr` | `780c8f47b0686d9419f6831ec5e0d0e176b4ac939e2f89cb4865700ebcf2916c` |
| `nsh-resumed-nsh_reading_geometry.stdout` | `59600e972c5380677b5f8ff6afbf1bf23dc2b855f8e107108f266316f560297a` |
| `nsh-resumed-nsh_icons_integration.stdout` | `9dd4a6c2ee6ecaa71af30d6404b9bb0929572d21e57d100382177679718322c8` |
| `nsh-resumed-nsh_settings_integration.stdout` | `b57f873f5d8625dab01193161d46fe1f462a0cec4249c804c7adead0ebb36637` |
| `nsh-resumed-nsh_native_host.stdout` | `e58cd453f1dbfdce85a27e0f87f46d3b4d021e5788884ff70072f7f7b07c800a` |
| `nsh-resumed-scroll-cycle.stdout` | `5e3b12f7cb3cbf7b16577739653275717d76285f472a095dd12a58fa538390cf` |

Later source observations at **2026-11-09T06:11:43.241805+00:00**, not certified build-input identities:

| Source path | Observed SHA-256 |
| --- | --- |
| `crates/vega_ui/src/conversation_stream/selection_menu/body.rs` | `24353258cef87cd2a9076263250d997ff87deb19b6865b0be75f75b269301f7a` |
| `crates/vega_ui/src/conversation_stream/selection_menu/body_integration_tests.rs` | `747426f974851330790b6a253967de9429b5f336292b09073020119bae2f2d84` |
| `crates/vega_ui/src/conversation_stream/shell_recovery/mounted_regressions.rs` | `751bc4307f70ca504161c066b07f06f3d572d84254ce7adfd679c6b7cecabadc` |
| `crates/vega_ui/src/conversation_stream/composer_selectors.rs` | `04d769bd42d3b0e12f57d3cb7f43feee4545cc77604acfc72b9dc5af88ad844f` |
| `crates/vega_ui/src/conversation_stream/composer_selectors/tests.rs` | `223e50ae628e072d97c715f871529790df9b7b9f01097ad309027dd6ca62e1e9` |
| `crates/vega_ui/tests/nsh_reading_integration.rs` | `87353d60ea543b0abf318004dee861ea0f7f278a1f891f3423da0d4692384df8` |
| `crates/vega_ui/tests/nsh_recovery_integration.rs` | `1c7bb60deb24323afc88bf19175e99961128c5c8c2b959c834ee7669eaf5337b` |

The backend `render.rs` observation still matches `b602deb11d26198704390ac10eb5969c66046ded6676c8e570a04d362c9e003d`. NSH packages remain **0.4.0**, recorded upstream **`e626a8dd8f8d8b277b9507cfd5ac74d0e67020f3`**. Kit **0.7.3**, Fast / Fast platform **0.1.9** remain unchanged; no Kit/Fast patch/upgrade or OS HTML clipboard implementation is claimed.

### Four lanes, blockers and remaining gates

- **Source author** owns source/manual-wheel correction and its scoped tests/runner; **recovery author** owns summary-host mounted failure; **parent** owns coordinated validation/runtime integration; **this lane** owns only this evidence document. No source/recovery/mainline-document overlap or independent review is performed here.
- Parent must coordinate a new compiled snapshot after author changes and rerun the relevant unchanged source/recovery assertions. Current blockers are manual-wheel anchor cancellation and absent mounted summary-host bounds, not the historical mount-ABA failure or composer `open_window` notification loop. No wider fix, budget increase, assertion relaxation or cleanup is authorized by this document.
- **Consolidated final independent review, exact-final-head required CI and macOS/Windows/Linux desktop-native acceptance remain pending**, as do development-test archival/removal under the separate delivery rules. No full-production completion, final source/recovery GREEN, fresh full-workspace/strict-Clippy/fmt/Python/i18n gate, merge, packaging or installation is asserted. The successful Clippy scope above is only runtime/store.
- Progress reports retain the user's fixed weights and report changes/results/blockers only: no total estimate or invented completion percentage. This documentation handoff ends writes after static readback and an exact document SHA-256; it does not end the parent task.

---


## Resumed full-production migration — latest retained-evidence boundary

This is **documentation implementation, not independent review**. This checkpoint takes precedence over current-tense status statements in the historical sections below; their failures, commands, hashes and earlier successes are retained, not erased. Only this document is written by this continuation. No Cargo/rustc, test binary, formatter, network, Git mutation or cleanup is executed here; inspection and document-only static validation do not constitute fresh product validation.

Observed worktree: `/Users/puzige/Workspace/worktrees/vega-907-nsh-full`, dirty HEAD `0b5ea426abfd8d4358489e916273d89474fb190f`. The dirty implementation is preserved. HEAD is a base identity, not a frozen identity for uncommitted source or the binaries behind these logs. No new fetch/update is performed under this continuation's explicit no-network/no-Git-mutation restriction. Earlier remote observations below are historical, not a fresh mainline compatibility conclusion.

### Actual retained results, not final-version acceptance

| Scope | Actual retained result | Evidence and boundary |
| --- | --- | --- |
| Nested scrolling + real intermediate tool group | **16 passed, 0 failed, 0 not executed, 0 ignored** | `target/nsh-scroll-complete-cycle.stdout:1–17`; stderr records build 0.74s and the real scroll integration target. Original List/body/group and gesture/remainder cases completed. This does not validate the later backend presence patch. |
| Real-font geometry/opacity/conditional actuator | **24 passed, 0 failed, 0 ignored** | `target/nsh-geometry-24.stdout:1–25`; build 0.87s in stderr. **Older snapshot**, not current presence-fence or final-byte validation; no extra not-executed count is invented for this footer. |
| Native host | **36 passed, 0 failed, 0 ignored, 0 filtered out** | `target/nsh-scroll-native-host.stdout:40`; build 3.06s in stderr. **Older snapshot**, not OS native acceptance or current presence-fence validation. |
| Shared icons/tooltip | **3 passed, 0 failed, 0 not executed, 0 ignored** | `target/nsh-icons-mounted-cycle.stdout:1–4`; stderr build 3.76s. Retained lifecycle/input/layout/accessibility state, not raster pixels or real assistive technology. |
| Settings natural height | **3 passed, 0 failed, 0 ignored** | `target/nsh-settings-natural-height-cycle.stdout:1–4`; stderr build 0.28s. All three original cases executed. This later log supersedes the Settings author document's stopped-at-UNRUN handoff, not the historical first-assertion, double-panic, readiness-only or current-measurement failures. Full Settings/native acceptance remains separate. |
| Workspace mounted + actual controller | **6 passed, 0 failed, 0 not executed, 0 ignored** | `target/nsh-workspace-controller-cycle.stderr:9–23`; build 55.83s; stdout empty. Three mounted plus three production-controller cases. This does not validate complete VegaWindow wiring, actual native pointer dragging/tab focus or platform compositing. |
| Shell recovery policy | **26 passed, 0 failed, 0 ignored, 320 filtered out** | `target/nsh-draft-recovery-policy.stdout:2–30`; stderr build 0.26s. Policy tests are not mounted summary/details, service completion or native acceptance. |
| Source reading/editing integration | **4 passed, 1 failed, 9 not executed, 0 ignored** | `target/nsh-source-selection-init-cycle.stdout:1–4`, stderr `28095–28100`, plus the fourteen-case ordered runner/source handoff. The old runner reports failure and remaining cases unexecuted without numeric totals; 9 is derived from 14 − 4 − 1, not a fabricated footer. No source-suite GREEN. |
| Mounted recovery | **First case interrupted; no PASS or completed runner totals; second case not executed** | Earlier `target/nsh-recovery-mounted-cycle.{stdout,stderr}` remains. Recovery-only diagnostic evidence is `target/nsh-recovery-diagnostic.{stdout,stderr,sample.txt,sample-command.log}`. Do not convert interruption into a passing or assertion-failure count. |

Settings' natural-height repair moves the same keyed Host into `request_layout`; actual 61/149px label/description measurements had been trapped inside a 73px row. The subsequent three-pass log is runtime evidence for that scoped repair. Earlier observation-order fixes did not by themselves fix production layout and remain recorded below and in `nsh-settings-mounted-validation.md`.

The four source PASS lines are reflow with affinity/signed inset, retained trim, removed trim and replacement. The fifth case, `source_caret_owner_and_mount_aba_reject_old_geometry`, reaches the same-numeric-layout remount with a fresh owner session, then fails at `body_integration_tests.rs:1358`: `assertion failed: !view.read(cx).reading_geometry_is_current(&old.geometry)`. The following conditional-scroll rejection assertion has not executed. The old native geometry lease remained current across outer GPUI host unmount/remount with the retained AppView/Surface. This later failure supersedes the earlier first-case width/actuator failures as the latest executed boundary; those failures remain historical evidence. The remaining nine cases, including later editing/recovery/preview cases, are not inferred to pass from earlier snapshots.

Recovery-only diagnosis compiled in **34.13s** (`target/nsh-recovery-diagnostic-build.stderr`; JSON `build-finished.success=true`). The retained stderr has `new.open_window` BEGIN id=8 without END; root construction, root rendering and request capture have paired END records. Both recovery stdout files are empty. The sample locates synchronous `App::open_window` / `flush_effects` / `apply_notify_effect` and the `composer_selectors::Host` observer created by `Native::render` feeding `Context<Host>::notify`. These are sampled stacks, not notification execution counts. The separate recovery handoff records the parent's watchdog result `watchdog_timeout=True child_exit=-9 elapsed=25.006s`, sampling after 10s and termination at 25s; the retained sample-command log independently records `sample_exit=0`. Watchdog timing/exit and no-surviving-process statements are parent-reported, not newly executed or inferred from stderr. This is composer notification feedback during initial window opening, not evidence of completed recovery layout, service work or acceptance. Original 2s / 512-tick / 10s assertions and budgets remain; no retry or larger budget is claimed.

### Later backend bytes: uncompiled and unrun

`vendor/nsh-platform-gpui/src/render.rs` now has SHA-256 **`b602deb11d26198704390ac10eb5969c66046ded6676c8e570a04d362c9e003d`**. This is the parent-owned host-mount presence patch, after the source mount-ABA failure: retained presence is reused for the same Surface, and fresh presence/drop invalidates geometry through the existing backend-private fence. The preceding ledger hash was `fd837ce64f1b094ec46691dfba58fa4649488155157d2a66dcdb6681793e40c9`. **The new presence-patch bytes are uncompiled and unrun at this checkpoint.** No ABA closure, backend regression pass or review approval is claimed. During this document's final static validation, the parent-owned `target/nsh-resumed-backend-check.stderr` changed from empty to `error: cannot specify features for packages outside of workspace` (65 bytes); stdout remains empty. This is a pre-compilation invocation rejection, not a compiler result or successful check; the exact rejected command was not recovered. The document check initially rejected its now-stale empty-stderr assertion, then inspected the changed artifact and corrected this observation rather than rerunning the parent's command.

The current backend provenance manifest records upstream NSH commit `e626a8dd8f8d8b277b9507cfd5ac74d0e67020f3`, both local NSH packages at **0.4.0**, and complete vendor tree identity `26bc85dccf7707e3115ca058b1c4d97227e8bc3ae9bfd20c7b50b49656c65da5`. This is an inventoried source snapshot, not behavior validation. Kit **0.7.3**, Fast / Fast platform **0.1.9** remain unchanged; no Kit/Fast patch or upgrade is part of this work. Approved same-version conditional source-caret and complete nested-wheel capabilities supersede earlier pending-authorization text. OS HTML clipboard acquisition remains absent and outside these approved patches.

### Commands, toolchain and source-version limits

Earlier recorded execution used official Cargo **`1.98.0 (797e8a9bc 2026-08-05)`** and rustc **`1.98.0 (88d9e12ae 2026-08-18)`**, `CARGO_FORK_AUTO_UPDATE=0`, offline/locked, `-j 2`, target directory `target/official-nsh-validation`. These are recovered execution records, not versions newly queried by this writer. Historical exact commands below remain tied to their original runs.

For the latest result logs, Cargo stderr establishes target and build duration, but does **not** recover a full shell invocation/environment. The retained command records establish these target scopes: `vega_ui` targets `nsh_scroll_integration`, `nsh_reading_geometry`, `nsh_native_host`, `nsh_icons_integration`, `nsh_settings_integration`; `vega` target `nsh_workspace_mounted`; UI library shell-recovery policy filter. **Exact per-cycle shell commands are unavailable where no execution receipt is retained.** The Settings/workspace author documents contain proposed commands; proposal plus matching target output is not proof of an exact executed command or a separate production check/Clippy pass. No exact invocation is reconstructed merely from a filename or common flags.

`nsh-shell-recovery.md` preserves the historical diagnostic compile command and full Python watchdog script. The compile line documented there names only `--test nsh_recovery_integration`; the actual retained JSON contains **both** `nsh_recovery_integration-9271aa7368d76ca1` and `nsh_reading_integration-c754697d113fc18b` executable artifacts and successful build completion. Consequently the full exact compile invocation for that JSON is **unavailable**, rather than asserted identical to the single-target documentation. The watchdog script selects the emitted recovery executable, starts it directly on its main thread, samples with `/usr/bin/sample <pid> 2 1 -file <sample-path>` and bounds observation at 25s. Its exact script is retained there, not rerun here. The source handoff identifies a bounded direct-binary execution using that JSON's reading artifact; its full exact launch/watchdog shell command is **unavailable**. Executable filename suffixes are not content hashes, and no historical executable SHA-256 was recovered.

| Source identity | SHA-256 and meaning |
| --- | --- |
| `crates/vega_ui/src/conversation_stream/selection_menu/body.rs` | `24353258cef87cd2a9076263250d997ff87deb19b6865b0be75f75b269301f7a`; source handoff identity for the four-pass/fifth-fail cycle; observed bytes match. |
| `crates/vega_ui/src/conversation_stream/selection_menu/body_integration_tests.rs` | `54b685e364a6edf3c587a9c0ebf75a760226b38a8b1f82bb3eaf733e98535421`; same source-cycle fixture identity; observed bytes match. |
| `crates/vega_ui/tests/nsh_reading_integration.rs` | `87353d60ea543b0abf318004dee861ea0f7f278a1f891f3423da0d4692384df8`; observed later runner with explicit numeric accounting, **not** the old failure footer's tested-runner identity. Runner author owns this increment; no fresh execution inferred. |
| `crates/vega_ui/src/conversation_stream/shell_recovery/mounted_regressions.rs` | `c77184684377f91199483d1345f8404f84117efeabf5bc60e4939822a03c3c3b`; diagnostic handoff identity; observed bytes match. Not proof of completed mounting. |
| `docs/907/nsh-backend-current-manifest.json` | `db31f0c27eb65e0aed75c6f176cc72ae313db7971792e75135dca4b85d6bf60b`; observed provenance document, separate from the vendor tree aggregate. |

No complete frozen dirty-source manifest for each individual passing result was recovered. Older hashes below identify their stated snapshots only; observed current hashes do not retroactively establish full source/binary equivalence for a retained run. Local logs/source remain development artifacts, not a durable published original-test archive.

### Retained result evidence identities

SHA-256 values were calculated read-only from the named retained files. Large stderr/sample files were searched at focused failure/stage ranges, not reproduced wholesale.

| Retained file | SHA-256 |
| --- | --- |
| `target/nsh-scroll-complete-cycle.stdout` | `5e3b12f7cb3cbf7b16577739653275717d76285f472a095dd12a58fa538390cf` |
| `target/nsh-geometry-24.stdout` | `59600e972c5380677b5f8ff6afbf1bf23dc2b855f8e107108f266316f560297a` |
| `target/nsh-scroll-native-host.stdout` | `456ae80799c020086f0779274109a2f400aae52231e48b19b397a9042ba154ac` |
| `target/nsh-icons-mounted-cycle.stdout` | `9dd4a6c2ee6ecaa71af30d6404b9bb0929572d21e57d100382177679718322c8` |
| `target/nsh-settings-natural-height-cycle.stdout` | `b57f873f5d8625dab01193161d46fe1f462a0cec4249c804c7adead0ebb36637` |
| `target/nsh-workspace-controller-cycle.stderr` | `652e215c9916b53f0ad833f1f90b3a6cbff346e1837308f1c6e5355bf29145b8` |
| `target/nsh-draft-recovery-policy.stdout` | `f1995c12fafb8d228afc608967ea068fdf15e5ae8351dc14ac0da6ed0b1788c0` |
| `target/nsh-source-selection-init-cycle.stdout` | `63c73131cda4699eb5da0bf38bf6b082e11b244464473b928d4de9afed93da88` |
| `target/nsh-source-selection-init-cycle.stderr` | `1f9cd28a9604ee9d573d08e33e3b6f65c79d7a3fb4c10363a387a1167db652d5` |
| `target/nsh-recovery-mounted-cycle.stderr` | `5602673e92c5054d7e3ca06ee4ec1e328f5c853c654f82892f377ee63702d895` |
| `target/nsh-recovery-diagnostic-build.json` | `e06efdef9aba71bdfb312ec5851a78d976f019a78567667ac91ce998399bde40` |
| `target/nsh-recovery-diagnostic-build.stderr` | `c1f3ca48b8ec38e37c1d19510909bae85c52d8127389d2f7d9cccf8891c97ebf` |
| `target/nsh-recovery-diagnostic.stderr` | `299b80f47fa7378e09a4638b57908883576d4cb289e8f064717d32e62cf382ae` |
| `target/nsh-recovery-diagnostic.sample.txt` | `94b21383536bc40c9337639a121433a35b3393db4687975b2b23300f605511b8` |
| `target/nsh-recovery-diagnostic.sample-command.log` | `b8e9a0142e2c56c702278b6d5eb8d44aa1c91cbd2328ab4855af335ca0232cb2` |

### Current ownership and remaining gates

- Parent owns source/runtime validation and backend presence compilation/regressions. Composer author separately repairs the production notification feedback. Source runner author owns runner/accounting changes. Mainline agent owns compatibility documentation. This lane does not overlap those writes or launch their validation.
- Recovery diagnostics/runner/assertions stay frozen pending composer repair and actual execution of both original recovery cases. No recovery-only repeated diagnostic run, budget enlargement or cleanup is performed here.
- Consolidated final independent review, final-head required CI and **all three desktop native acceptances remain pending**. Historical focused source reviews and local suite passes do not satisfy those gates. Final recoverable development-test archival/authorized cleanup remains separate; no cleanup is performed here.
- No current full-workspace/strict-Clippy/fmt/Python/i18n gate, complete production migration, merge, packaging, installation or final source/recovery GREEN is claimed. Historical warning counts are not current counts. Progress reports use only changes/results/blockers with the user's fixed weights unchanged; no total estimate or invented completion percentage is added.

## Historical scrolling/source-anchor integration checkpoint

The following checkpoint and all subsequent sections retain the original chronology. Current-tense statements within them describe those older snapshots; use the resumed checkpoint above for latest boundaries.

### Latest mounted scrolling result

The current development run completed all 16 real-body/original-List/tool-group cases: **16 passed, 0 failed, 0 not executed, 0 ignored**. Raw evidence: `target/nsh-scroll-complete-cycle.{stdout,stderr}`. This supersedes the historical scroll failures below, but is not frozen-final-version or three-desktop native acceptance. It covers actual partial and reverse remainder transfer, mixed axes, gesture ownership and terminal/cancel behavior, real group ScrollHandle offsets, scope replacement/detachment, and real body → group → original List composition.

The original fixture List reentrancy was corrected by using the production ReadingList boundary with a shared frame token for its owners. The real-engine runners now use the pinned Fast test executor's official `allow_parking()` mode; the earlier initialization deadline failure did not require a production command-bridge patch or larger case budgets. Event callbacks no longer query draw-only `window.current_view()`; refreshing the window preserves retained invalidation. Group fixture state is captured during the actual render phase instead of querying keyed state from an update callback. The intermediate outer-command 120-second timeout is preserved in `target/nsh-scroll-mounted-state-fixed.{stdout,stderr}`; it was not a case assertion failure. The completed run used a longer outer execution limit, without enlarging 2-second drain, 512-tick or 10-second case limits.

The source Presence fixture repair now passes initialization and the original six-condition current-source-projection wait. The next run compiled in 33.75 seconds, then failed the first reflow case at `same source caret at retained signed inset` after 10.047 seconds: 0 passed, 1 failed, 13 not executed, 0 ignored. Raw evidence: `target/nsh-source-presence-cycle.{stdout,stderr}`. The fixture had replaced the real ThinkingBlock Presence/keyed-state boundary with a bare body; it now injects projection at the original body call instead, without weakening owner/source validity. No source-caret mounted pass is claimed; the author is diagnosing this later caret-restoration failure.

The dedicated shared-icon/tooltip runner compiled in 3.76 seconds and completed **3 passed, 0 failed, 0 not executed, 0 ignored**. Raw evidence: `target/nsh-icons-mounted-cycle.{stdout,stderr}`. This validates retained Surface identity, size/theme updates, clip/opacity parent-input behavior and tooltip shortcut/layout/accessibility state; it is not pixel readback, platform screen-reader or three-desktop acceptance. Both new commands used the common toolchain/offline/locked/-j2/target-directory arguments below. macOS linker compact-unwind warnings remain. An attempted session-mode source command was rejected with `workspace_sandbox_required`; it did not execute and was replaced by the supported blocking protocol, not another automation route.

Subsequent development results, on the dirty implementation rather than a frozen final head:

- **Settings 3/3 passed** after fixing a production natural-height defect. Real measured label/description heights (61/149px) had remained inside a 73px row because a prepaint-created `container_query` child could not size its parent. The same keyed Host now participates in `request_layout`. Original assertions and budgets remain; fresh evidence `target/nsh-settings-natural-height-cycle.{stdout,stderr}`. Earlier first-assertion failure, destructor double panic, readiness-only attempted repair, and decisive current-measurement failure remain in their separate logs; no discarded regression.
- **Workspace 6/6 passed, 0 failed/not executed/ignored**, including the original three native mounted separator/background cases and three actual production controller cases for clamps/defaults/raw extents, release/cancel/window-resize and visibility/fullscreen. Both production 5px drag regions use the same tested host. Evidence `target/nsh-workspace-controller-cycle.{stdout,stderr}`; build 55.83 seconds. Native pointer dragging, actual tab scroll/focus and compositor/platform acceptance remain pending.
- **Recovery policy 26/26 passed** after #916 compatibility integration; evidence `target/nsh-draft-recovery-policy.{stdout,stderr}`. The authoritative window/service files match upstream commit bytes, core invalidation hooks and separate NSH overlay are wired. `cargo check -p vega_conversation` passed (31.62 seconds), and i18n checks reported 0 catalog/reference errors. Mounted overlay interaction is still being implemented.
- **Source caret remains failing**: width acknowledgements now preserve the saved anchor until actual engine reflow width matches the host. The 218px transition progressed, but 302px restoration attempted the one-shot synchronous actuator with delta -184 and received false. Full bounded diagnostic is in `target/nsh-source-width-ack-cycle.stderr`: 0 passed, first case failed, 13 not executed. Caller and backend rejection conditions are under investigation; no unconditional retry, safety bypass, tolerance increase or mounted-source GREEN is claimed.

Four disjoint implementation lanes cover source restoration, Settings integration, recovery mounted interaction and workspace controller closure; consolidated independent review has not begun. Backend original archive/manifests were reconciled (180/180 core, 45/45 platform) and current source inventory is in `nsh-backend-current-manifest.json`; development-only support is retained pending genuine final archival/cleanup.

Remote refresh found `origin/master` at `f3688f5ae0b7becf1f46434c7b7d732e3a3c759a` (#916 recovery fix). Its window/service/locales and required UI behavior have now been applied to the dirty migration implementation; Git ancestry has not yet been advanced and no merge/commit/push is claimed. The UI retains NSH rendering rather than restoring upstream's visible GPUI details. Existing earlier development results are not blanket evidence for this newer integrated version.

The implementation remains dirty at HEAD `0b5ea426abfd8d4358489e916273d89474fb190f`. The synchronous conditional source-caret actuator and complete nested-wheel protocol extensions are now authorized; historical statements below describing them as unapproved are superseded, not erased. Kit/Fast versions are unchanged. OS HTML clipboard acquisition remains absent and outside the approved capability patches.

Current-cycle validation uses official Cargo `1.98.0 (797e8a9bc 2026-08-05)` and rustc `1.98.0 (88d9e12ae 2026-08-18)`, `CARGO_FORK_AUTO_UPDATE=0`, `--offline --locked -j 2`, target directory `target/official-nsh-validation`. These are development snapshot results, not final frozen-byte validation.

| Actual command scope | Actual result | Raw evidence |
| --- | --- | --- |
| `cargo test -p vega_ui --features nsh-reading-integration --lib --test nsh_reading_integration --test nsh_reading_geometry --test nsh_scroll_integration --no-run` | Compiled all four targets, 53.24 s; unused `Fragment.row` and macOS linker compact-unwind warnings remain | `target/nsh-source-scroll-compile-4.{stdout,stderr}` |
| Same common arguments, `--test nsh_reading_geometry` | 24 passed, 0 failed/ignored, including six composed-gesture and six conditional-actuator cases | `target/nsh-geometry-24.{stdout,stderr}` |
| Same common arguments, `--lib` | 339 passed, 0 failed/ignored/filtered | `target/nsh-scroll-ui-lib-current.{stdout,stderr}` |
| Same common arguments, `--test nsh_native_host` | 36 passed, 0 failed/ignored/filtered | `target/nsh-scroll-native-host.{stdout,stderr}` |
| Same common arguments, `--test nsh_reading_integration` | First source-caret reflow case failed during `Fixture::new`, before source projection: scheduler drain exceeded existing 2 s at tick 13; remaining 13 cases not executed | `target/nsh-source-anchor-mounted.{stdout,stderr}` |
| Same common arguments, `--test nsh_scroll_integration` | 0 passed, 1 failed, 10 not executed, 0 ignored: first no-target case hit original List RefCell reentrancy at Fast `list.rs:822` | `target/nsh-nested-scroll-11.{stdout,stderr}` |

Original failure evidence is retained. Initial compilation found NSH/GPUI Point confusion, a missing exact snapshot matcher and unreachable public observation export; parent fixed those integration layers. Shared icon/tooltip authors used nonexistent `ns::bind`; parent corrected both to the pinned version's existing `ns::dynamic`. A direct dependency feature-check command was rejected before compilation because the package is outside the workspace; that invocation is not counted as a check pass.

Source-anchor and real-body fixture authors are diagnosing their respective mounted failures without expanding 2 s / 512-tick / 10 s budgets or weakening assertions. Icon lifecycle diagnostics and the actual intermediate tool-group ScrollHandle remainder integration continue in disjoint implementation lanes. Independent consolidated review has not begun. No new commit, push, merge, packaging, installation or native three-desktop acceptance is implied.

## Previous author-cleanup validation checkpoint

The dirty implementation remains based at `0b5ea426abfd8d4358489e916273d89474fb190f`; no new commit/push/merge is implied. Official toolchain: Cargo `1.98.0 (797e8a9bc 2026-08-05)`, rustc `1.98.0 (88d9e12ae 2026-08-18)`. The sorted implementation-content manifest (389 existing changed/untracked Rust/manifests/lock/vendor files; POSIX path/SHA-256 pairs) hashes to `46316929a85a498cf97d2089476c8d09c0fe4dce3ac5ce248c7947893beb443b`. Deleted legacy `selection.rs` is separately recorded by the Git diff. This is a local content identity, not a remotely recoverable source archive.

Four author lanes were dispatched. The preview/dead-entry lane completed and released its scope; the other three failed/released without useful handoff or reported unknown effects. Parent inspected current bytes, preserved the dirty implementation and completed their cleanup directly without restarting empty workers or conducting intermittent independent reviews. Proven callerless GPUI selection drawing and old control entry points were removed; test-used fade/published-retention helpers and all existing meaningful assertions remain. Native key context was aggregated without dropping fields; request snapshots were boxed; owner/route/list-frame types were named; branch simplifications retain full authority/focus/geometry conditions. Uncalled source-anchor geometry prototypes were removed, while eight semantic tests remain. Temporary UI publication/header diagnostics were removed; feature-gated backend diagnostics still require genuine final cleanup.

Actual current-cycle commands/results (`CARGO_FORK_AUTO_UPDATE=0`, offline/locked, `-j 2` or Nextest `--build-jobs 2`, target dir `target/official-nsh-validation`):

| Command / scope | Actual result | Raw local evidence |
| --- | --- | --- |
| `cargo clippy -p vega_ui --all-targets --features nsh-reading-integration -- -D warnings` | exit 0 | `target/nsh-lint-cycle-clippy-4.{stdout,stderr}` |
| `cargo clippy --workspace --all-targets -- -D warnings` | exit 0; 16.09 s | `target/nsh-lint-cycle-workspace-clippy.{stdout,stderr}` |
| `cargo test -p vega_ui --features nsh-reading-integration --lib --test nsh_native_host --test nsh_reading_geometry --test nsh_reading_integration` | 339 library + 36 host + 12 real-font + 5 mounted passed; no failures/ignored; build 41.50 s | `target/nsh-lint-cycle-integrated.{stdout,stderr}` |
| `cargo test -p vega shell_chrome` | 4 executed/passed; 27 filtered; separate empty target not counted | `target/nsh-lint-cycle-shell.{stdout,stderr}` |
| `cargo check -p vega` | exit 0; 7.46 s | `target/nsh-lint-cycle-app.{stdout,stderr}` |
| `cargo fmt --all -- --check`, `git diff --check` | exit 0 | `target/nsh-lint-cycle-fmt.{stdout,stderr}` / command receipt |
| `python3 scripts/check_i18n.py` | catalog/reference errors 0; UI/unreviewed 0 | `target/nsh-lint-cycle-i18n.{stdout,stderr}` |
| release / computer-use preparation / `scripts/tests` Python suites | 5 / 15 / 40 executed/passed | `target/nsh-lint-cycle-python-{release,prepare,scripts}.{stdout,stderr}` |
| `cargo nextest run --workspace --lib --bins` | 370 executed/passed; 0 skipped; 1.011 s test runtime; default concurrency/retries unchanged | `target/nsh-lint-cycle-nextest.{stdout,stderr}` |

The platform dependency still emits three missing-public-documentation warnings for opacity APIs; dependency warnings are not evidence of a warning-free vendor tree. One default-feature UI test link emitted the macOS compact-unwind size warning. Neither warning was suppressed.

Nextest discovery failures are retained: initial `-j` was rejected before execution (actual syntax is `--build-jobs`); first correct workspace discovery timed out during compilation with no remaining process; extended discovery built successfully but failed parsing the harness=false geometry runner's `PASS` output because it does not implement libtest `--list`. This is development test-runner compatibility, not product failure. The explicit ordinary `--lib --bins` run above is not misrepresented as all-target Nextest; host/geometry/mounted cases were separately executed by the successful dedicated Cargo command. Final full required gates remain mandatory after development tests/support are archived and removed; no required regression was dropped to obtain a passing claim.

Implementation remains incomplete at the two unapproved dependency boundaries: nested wheel consumption/remainder/gesture ownership, and synchronous current-geometry source-caret anchor scrolling. Existing block/pixel anchoring remains; body wheel-target correction does not solve cross-Surface propagation. OS HTML clipboard acquisition is also not present and is not claimed. Consolidated independent review, exact-final CI and macOS/Windows/Linux native acceptance are still outstanding; no release/package/install is authorized by these local results.

## Previous integrated development checkpoint (before lint cleanup)

The dirty implementation at base HEAD `0b5ea426abfd8d4358489e916273d89474fb190f` executed **339 library**, **36 native-host**, **12 real-font geometry/opacity** and **5 main-thread mounted reading/editing** regressions successfully. Raw output: `target/nsh-current-integrated-regressions.stdout` and `.stderr` (39.80 s build). Shell validation separately executed **4 tests successfully**; another target discovered/executed zero and is not counted as a passing test suite (`target/nsh-current-shell-tests.stdout` and `.stderr`). Application check completed in 7.07 s with 36 UI warnings (`target/nsh-current-app-check.stderr`). Static i18n reported zero catalog/reference errors and zero UI/unreviewed findings (`target/nsh-current-i18n.stdout`). These results precede the current four-way lint cleanup and do not validate its final bytes.

The original integration assertions and time budgets remain intact. The body geometry lease now comes from the actual body element's prepaint bounds, not a sibling canvas positioned after the body. This corrected the wheel target without changing dependency wheel propagation. Stationary hover now retires an invalid captured arming task and rearms from a currently valid measured target with a fresh delay; stale geometry/source/scroll fences remain enforced. Mounted link-focus assertions run in the existing main-thread harness because NSH's runtime queue cannot safely follow ordinary parallel GPUI test-thread lifetime; library concurrency is unchanged.

Native editing registers hooks again through the existing idempotent render opportunity after the backend node attaches. The integration fixture activates its actual test window instead of weakening production focus checks. Backend host-focus restoration now notifies the current registered hook once; blur still retires it and stale epochs cannot reinstall it. Recovery testing waits for the real attached, enabled/pressable Reload AX node, then performs the original AX Press assertion within the original budget. Object creation alone is not evidence of mounted operability. Temporary diagnosis output was removed before the full passing integrated run.

Strict Clippy subsequently **failed** (`target/nsh-current-strict-clippy.stderr`: UI library 55 errors, library-test target 59 errors). The active work is minimal author-side lint cleanup and removal of proven dead migrated paths, not an independent per-module review. Required next verification is coordinated compilation, strict lint and the same integrated regressions on the resulting bytes. No fresh-head CI, consolidated independent review, native three-desktop acceptance, merge, packaging or installation is established.

## Earlier implementation-first checkpoint (superseded results, retained evidence)

The earlier dirty application snapshot compiled with official Cargo/rustc 1.98.0. Application regressions executed: **326 passed / 0 failed / 0 ignored / 0 filtered**, after correcting two test fixtures without relaxing behavior: native frame origins require parent-chain accumulation; CommonMark emphasis after `A` needs a separating space. Original failing result was 324 passed / 2 failed in `target/nsh-current-production-lib.stdout`; the succeeding rerun is `target/nsh-current-production-lib-repaired.stdout` and `.stderr`.

Native targets then executed **35 host passed**, **12 real-font geometry/opacity passed**. Reading integration failed its first mount at the original 2 s scheduler drain deadline (3.309 s, tick 17); the other three integration cases did not execute. Raw evidence: `target/nsh-current-native-targets.stdout` and `.stderr`. This is not complete migration, CI, or native desktop acceptance.

The continued feedback is now traced to `shimmer::native::Snapshot` publication on every acknowledgement-driven prepaint: wall-clock sampling changes colors even without a real animation frame. The scoped correction must sample phase on actual host animation callbacks only, deduplicate pending frame requests, retain natural presentation changes/reduced-motion/visibility gates, and keep the original integration time/assertion budgets. No runtime yield, global Surface deduplication, or dependency change is authorized by this finding.

## Content and scope

Development branch `feat/907-nsh-full`, base HEAD `0b5ea426abfd8d4358489e916273d89474fb190f`, with uncommitted migration changes. Origin fetched before this continuation: remote task branch identical to HEAD; master remained `34555cec9c0c4c4c1f7a528d739752f414e53edf`. These results validate the dirty source identities below, not a new committed/pushed head, CI, native acceptance, or complete production migration.

The fixed same-version reading patch now supports single-text directed endpoints and current-snapshot-guarded installation as well as cross-text installation. All single-text range/direction changes go through one update point; equal normalized ranges with changed direction invalidate prior observations. Empty selections, other-text selection, and content replacement clear directed state. A mounted production regression covers reversed/forward ranges, surrogate interior rejection, overrun/index/cross-Surface rejection, collapsed/cleared/reselected state, content object replacement, detach, reused node ID, and stop.

The shared main conversation Copy action now delegates through a current NSH ReadingBinding. Deferred copy captures its originating projection session and native snapshot and revalidates both against the current exact window/root/content/geometry reading authority before clipboard write. There is no stale display-text fallback in this action.

## Actual toolchain and command

- Official Cargo `1.98.0 (797e8a9bc 2026-08-05)` via `rustup run 1.98.0 cargo`.
- rustc `1.98.0 (88d9e12ae 2026-08-18)`.
- Isolated official target directory: `target/official-nsh-validation`.

```sh
rustup run 1.98.0 cargo test -p vega_ui --lib --test nsh_native_host --offline --locked -j 2 --target-dir target/official-nsh-validation
```

Initial integrated result: exit 0; build 9.43 s; **252 application tests passed / 0 failed / 0 ignored / 0 filtered**, 0.09 s; **30 native host tests passed / 0 failed / 0 ignored / 0 filtered**, 0.01 s. Includes four actual-menu-action Undo regressions and single-text endpoint installation, prior cross-text/listener/clipboard/paste/mutation regressions, application mention/history/recovery, reading mapping, sidebar and tool regressions. These are process-local development tests, not real OS popup/IME/AX/clipboard or three-platform manual acceptance.

Raw current local output: `target/nsh-ui-tests-build.stdout` and `.stderr`. Local target paths are diagnostic artifacts, not durable published test archives. Rust tests and exclusive support still require recoverable publication and final cleanup before master.

## Failures and responsible-layer correction

1. Application compilation initially recursed in ordinary `#[test]`: parent `gpui_kit::*` glob imported GPUI's test macro into two pure reading/menu test modules. Explicit `core::prelude::v1::test` disambiguation corrected the harness without raising recursion limit or changing assertions.
2. Four actual TextInput behavior tests required existing fixed Kit `test-support`; their test-only feature was missing. Development-only `gpui_kit` support was added, with no Kit/Fast version change. A PasteEvent test also incorrectly constructed an external non-exhaustive struct; using Default then public field assignment fixed that API error.
3. Enabling test support resolved 14 cached test-support packages. A TOML identity comparison confirmed no existing package removed and no existing checksum changed. This is test-only development wiring, not authorization for production dependency expansion; remove it and its exclusive lockfile support at final cleanup.
4. Independent backend review found native menu Undo bypassed authoritative key policy. Implementer fixed actual menu action to check captured focus/mount/registration/window, exclude composition, call guarded policy once, and recheck before native fallback. Non-implementer independently closed this P1 at production `text_input.rs` hash `c06e7f65bf156a00c687d94b3fb7f4744ee6d49f11606bba1943c197167dac15`. No new source finding; real native acceptance remains open.
5. First integrated execution: application 252 passed, host 29 passed/1 failed. The failing menu rebind regression incorrectly expected native serial 1 after intentional re-registration. Existing `set_hooks` publishes a new snapshot and increments serial. The next harness correction initially reused `expect_edit`, whose precondition equates observer count to global serial; the new observer had one event at global serial 2. Final correction directly asserts one event at serial 2, unchanged text/directed selection, no mutation, same focus identity, one key-policy call, unchanged history/count, and retained hooks. Blur separately asserts no new event and serial 1. No production code or acceptance assertion was weakened to obtain the final 30-pass result.

## Source identities at successful validation

| File | SHA-256 |
| --- | --- |
| `vendor/nsh-platform-gpui/src/reading.rs` | `d91e1eca7473ac92c5bc0cef100ea003f3e10a9bd0326f05a17c1d3235455c7b` |
| `vendor/nsh-platform-gpui/src/text.rs` | `b64e7d203c838bee191a6a0a74bf489cac720ec90cadadd591543c18e69676ae` |
| `vendor/nsh-platform-gpui/src/selectable_view.rs` | `52c454e75734a8f0d6eaac8803df86c91a707b062aeae276aced021e8e4e1e95` |
| `vendor/nsh-platform-gpui/src/app.rs` | `6f83c389df1a51bf35836c5e9f6adce6d87b5442611575cf14617cc7f9c6c768` |
| `vendor/nsh-platform-gpui/src/surface.rs` | `be06cab31b340a22f0754ba024de22298ff0f58016139d85e47ab90c70d77a5d` |
| `vendor/nsh-platform-gpui/src/reading/host_tests.rs` | `3d8ccac1d12ba357da2ad27011396f14204a2f652b48966dc6385113cec83670` |
| `vendor/nsh-platform-gpui/src/selectable_view/host_tests.rs` | `2b303fcc1d2ec2a4f0e5897621011f52be15f47ee4195b8d11f5670470d554d9` |
| `vendor/nsh-platform-gpui/src/text_input/host_tests.rs` | `e0ac6302fc4d0df87ec465faf7f50d34b1f55cf01e7edbd6fa63dc5c8f8b1469` |
| `vendor/nsh-core/src/selection.rs` | `c2f11f09b79220e066dbfbce4cf4786f89d8043487fb8bc159e15f6a356e5db3` |
| `crates/vega_ui/src/conversation_stream/selection_menu/body.rs` | `9d7ac279e7cae2eab08804ffb00bb259023f3a89d696397a56014fabda0fcf8b` |

## Reading geometry and preview continuation

The next stopped version added opaque current-snapshot `ReadingGeometry` with exact layout/content/ancestor state, per-invalidation identity, root/origin/scale checks; the parent connected Surface invalidation, resize/scale-before-observers, and changed prepaint origin revocation. Source queries use actual GpuiLayout selection rectangles/carets/hit tests; unsupported transforms/clips reject instead of approximating. A non-implementer independently reviewed all seven frozen backend files with no findings, beginning/ending hashes identical. This is separate from native acceptance.

AppView geometry was connected to actual NSH body prepaint. Trusted links map target URLs and fragment bindings to current UTF-16 native tag ranges and clipped measured rectangles. The existing preview coordinator now accepts measured targets and revocable authority predicates. It checks currentness at hover/arm/show, load initiation/response, keyboard/AX, card Open and retry; captured stale callbacks cannot activate a new preview. The original application preview review found two P2 issues: stationary previews lost their geometry lease during scrollbar fade, and native focus proxies lacked fragment-level visible focus. Both repairs were subsequently independently closed at their frozen identities below; that static closure is not mounted-body or native acceptance.

Latest executed command (258/30/6 repair version):

```sh
rustup run 1.98.0 cargo test -p vega_ui --lib --test nsh_native_host --test nsh_reading_geometry --offline --locked -j 2 --target-dir target/official-nsh-validation
```

**Exit 0; 258 application tests passed (0 failed/ignored/filtered), 30 native host tests passed (0 failed/ignored/filtered), six geometry cases passed (0 failed/ignored).** Latest build 4.32 s; application 0.09 s; host 0.02 s. Raw local output is `target/nsh-preview-geometry-test.stdout` / `.stderr`; the parent reread the actual result lines on continuation. The six geometry cases run on the main thread with `gpui_kit::platform::current_platform(true)` and its real font-backed `TextSystem`; available font names must be nonempty. This custom harness executes six cases, not six standard Rust test-harness discoveries. The default GPUI test platform's no-font system was explicitly not used for shaping evidence. No Vega window/native app was launched.

New four preview tests use actual GPUI prepaint registration, wrapped-fragment pointer hit tests, revoked keyboard/hover and active-load cancellation, multiple-fragment lease combination, and nonfinite/empty/revoked registration. The body mapping test uses actual structured RenderNode links with strong/CJK-surrogate content, duplicate target occurrences with distinct binding identities, and disallowed-scheme exclusion. Its original Markdown fixture did not generate nested style nodes (the parser returned literal asterisks); replacing the false fixture premise with the supported structured input retained all expected labels/ranges and binding assertions. Parser behavior was not expanded to make the fixture pass. Preview harness fixes used explicit built-in `test` macro and actual platform URL-open observation rather than assuming a return from the unit-returning keyboard dispatcher.

Source hashes at the 258/30/6 validation and focused repair review:

- `link_preview.rs`: `55b0bd606a35ce6912e14e2b6b52549c909a79b33deaf6961e2e941763499936`
- `link_preview_tests.rs`: `cf24fd1ea11906473db0191053424fac0df812a2bde1f996008d989d856268aa`
- `conversation_stream/selection_menu/body.rs`: `58efd2597fcc83ce8bef3b3e6270f3e015c55612e5f9e0512d82379befb0968d`
- `conversation_stream/selection_menu/body_tests.rs`: `6bb98e3e8b086ee39740dd162867e28f9af125f5e910f7c6c51d533d5959d1d0`
- `vendor/nsh-platform-gpui/src/surface.rs`: `eda7db67ae67647d1c4e06856a2f73905eaa483040c924b99c70b60977ee43bb`
- `vendor/nsh-platform-gpui/src/scroll.rs`: `b3e6a7383a77a1e0e5bcafe9df803cf86b942a50f9050a4941c25727ca666658`
- `vendor/nsh-platform-gpui/src/reading/geometry_host_tests.rs`: `514973578d8c4ff6aa297cadc2c418c26ce288bda3f20f9af43991cf3881a2c6`

The six-file focused non-implementer review (`01M6WH3Y1CNE7P2KQA6F7TWQ88`) matched its supplied identities at beginning and end and closed F1/F2 with no new actionable findings. The reviewer executed no tests. Production movement/relayout still invalidates geometry; only scroll decoration redraw uses paint-only `repaint()`. No numerical equal-rectangle lease reacquisition exception remains. The sixth real-font case drives the shared scroll state machine through 700–1000 ms hold/fade, confirms alpha reaches zero while stationary geometry stays current, and rejects displacement/return ABA.

The added actual GPUI root-frame focus fixture draws production fragment focus decorations and exercises forward/reverse traversal, Escape dismissal, wrapped-gap exclusion, and revoked-target exclusion. It observes selected paint rectangles, not raster pixels. A test initially failed after a plain `Cell` validity mutation because no repaint was requested; requesting refresh and clearing the draw arena corrected the harness lifecycle without changing its assertion or production currentness checks. A mounted actual BodyState/Thinking stationary-hover → fade → continued card Open/retry regression is still needed; the backend and coordinator seam tests do not establish that chain.

The earlier endpoint/listener/core/shared-Copy review also finished with no findings at the original identities above. Those closure conclusions cover their frozen bytes; the geometry/backend and app preview deltas have separate review/validation status.

## Unified opacity/original-paste/integration attempt

After the 258/30/6 repair version, the parent added the independently source-reviewed Omitted Thinking NSH notice, exact-text-bound StreamFade ranges, eight source-anchor semantic regressions (production anchor remains blocked), original ClipboardItem fidelity, six paint-only opacity cases, and development-only mounted-body/request-isolation support.

Actual official Cargo/rustc remains 1.98.0. Executed command:

```sh
CARGO_FORK_AUTO_UPDATE=0 rustup run 1.98.0 cargo test -p vega_ui --features nsh-reading-integration --lib --test nsh_native_host --test nsh_reading_geometry --test nsh_reading_integration --offline --locked -j 2 --target-dir target/official-nsh-validation
```

**This command failed overall.** Build completed in 54.92 s. Application **279 passed / 0 failed / 0 ignored / 0 filtered**, 0.09 s; backend host **32 passed / 0 failed / 0 ignored / 0 filtered**, 0.01 s; custom main-thread real-font geometry + opacity harness **12 executed / 12 passed / 0 failed / 0 ignored**. These include five StreamFade regressions and eight semantic source-anchor regressions; passing the latter does not activate production anchors. Raw output: `target/nsh-integrated-opacity-test.stdout` / `.stderr`. Warnings remain: platform 3, application lib 31, application lib-test 27 (22 duplicates), body-integration target 1. These results are not strict Clippy or full workspace gates.

The mounted actual Thinking integration runner panicked during its first case; the second case did not execute. Running the exact newly built integration binary once with `RUST_BACKTRACE=1` produced `target/nsh-body-borrow-diagnostic.stdout` / `.stderr`, confirming a **production lifecycle defect**, not an assertion or font failure: GPUI `List::prepaint` holds its ListState mutable borrow across child prepaint, while the production body's canvas calls `stream.list.viewport_bounds()` at body.rs:1515. LinkedBody's prepaint `valid → owner.current` and the menu canvas have equivalent direct list reads. The next correction must repair all relevant production prepaint/list reads, preserve action/completion currentness, and rerun the same mounted-body assertions. Do not replace the real list fixture, suppress the panic, bypass owner/currentness, or drop the required integration cases.

Original-paste fidelity received a no-finding static review at its initial Rc original/normalized identities, but parent acceptance review found an actual regression: new duplicate image buffers reduce the previous multi-image acceptance under the existing 16 MiB budget. That version is **not accepted as no-degradation** despite its tests passing. A followup implementation must use an explicitly Native/Synthetic borrowed-file carrier, preserve the existing count/per-image/total/nonimage limits, remove newly introduced retained image copies, and rerun safety and full-limit acceptance regressions. No increased limit or reduced feature range is authorized. System HTML acquisition and real AX Paste remain distinct unresolved items.

The opacity backend's real-font cases passed at its stopped six-file version; its independent source review subsequently found no findings. Application per-part integration remains pending. Native paint/emoji pixel behavior is not established by the custom geometry harness.

## No-copy paste and list lifecycle repair attempts

The same unified command was rerun after the real list-prepaint lifecycle repair and explicit Native/Synthetic borrowed-file carrier. The first run compiled in 52.68 s and stopped at **283 application passes / 1 failure**: the new full-budget authority test created TextInput with default `image_paste=false`, so the production attachment gate correctly rejected its images. Parent compared the existing attachment test's explicit opt-in and added only `input.image_paste = true` to the new fixture before its snapshot/bridge setup. No production gate or assertion changed. Host/geometry/body targets did not execute in that failed run. Raw output: `target/nsh-integrated-no-copy-test.*`.

The corrected rerun compiled in 9.92 s and completed **284 application passes, 33 backend host passes, 12 custom real-font geometry/opacity passes**, all zero failed/ignored/filtered where harness counts apply. It includes actual Native carrier → hooks → adapter → authority/ImagePaste delivery of two 8 MiB and four 4 MiB images with independent metadata and over-capacity normalization. The complete command **timed out at 120 s** while the mounted-body integration target produced no result. A process check confirmed no Cargo/rustc/integration process remained. No overall success or two-body-case execution is claimed. Raw outputs: `target/nsh-integrated-fixture-test.stdout` / `.stderr`.

The list lifecycle source repair separately received independent review: no functional finding, one repository-rule finding for two newly added ordinary comment blocks. Parent removed both comments without changing code; explanation remains in the spec. The reviewer correctly noted that synchronous push/prepaint/pop is not an unwind-safe Drop guard, contrary to an earlier handoff wording. Actual mounted-body progress now needs bounded phase diagnostics to identify the nonreturning stage; assertions, real list/font fixture and currentness guards remain required.

No-copy paste's second independent review (`01M6WNWC967MX4B37WYWKRF2TZ`) subsequently completed with no findings at the corrected five-file identities, including `nsh.rs` `9fa7f5bdf4cbdfe33d9fc2acc2bbb25948bad4fff9dfa1e4fefd78c6c0b11cd0`. It confirmed original-only image storage, exact Rc seal, full prior limits and release paths; it ran no tests. Published CI still validates old committed HEAD only; these dirty bytes have not been pushed or merged.

## Bounded mounted-body scheduler diagnosis

The test writer added stage begin/end and bounded scheduler ticking without changing the behavioral assertions, real list or currentness. The focused body command failed its drain deadline rather than silently timing out: `target/nsh-body-stage-diagnostic.stderr` records tick 7 taking 45.750945875 s before returning. A separate existing sampled run recorded 13.090759959 s in its retained stderr; do not substitute one duration for the other. Process sampling shows NSH runtime job/update batches entering Fast test-mode dirty-window draw, not network or a blocked font read. These samples alone did not prove an infinite loop.

Parent then added development-feature-only, bounded runtime batch/draw/repaint counters, Surface publication type identities and actual Thinking height-change logs. No dispatch, yield, invalidation, currentness or publication semantics changed. The focused run compiled and failed the same two-second drain assertion in roughly 2.8 s; its tick contained around 2700 job batches, 10800 Surface draws and 8200 publications, almost entirely three `run_activity::native::Snapshot` entities (16, 22, 23). Thinking height changed only once from 0 to 28. Raw output: `target/nsh-body-flow-diagnostic.stdout` / `.stderr`. This discriminates a stable-title publication feedback defect from height oscillation or external waiting. The next scoped correction is the actual run-header mount/publication identity path; a general Surface dedup or runtime yield must not mask it. Both original body cases and all stale-target assertions remain required.

## Header publication correction and current validation

The first scoped correction separates stable presentation identity from action authorization: None/None owners can compare equal for display, while acceptance and acknowledgement still require an owner. Header and divider use the local change gate, and live mount leases are reused before prepaint and while clipped. Independent read-only review `01M6WRCH7FZXNK6218RVQBVG2P` found no finding at production module SHA-256 `0b337f6e0e1232b47cd9b31015c9592e43042ca5d63a5a052ec0bd2dd4b750db`; it did not establish convergence or runtime acceptance.

The unchanged unified command with official Cargo/rustc 1.98.0 compiled in 32.78 s, then reported **287 application passed / 1 failed / 0 ignored / 0 filtered**, 1.70 s. The three new pure-state regressions passed. The fourth actual-header test reached the real NSH engine but the ordinary `#[gpui_kit::test]` scheduler's `end_test` rejected its cross-thread wake as nondeterministic (`target/nsh-header-convergence-test.stdout:364–365`). This is a test-host mismatch, not an asserted header behavior failure. Backend host, geometry and body targets did not execute in that command. Warnings: platform counts not re-extracted; app lib 31, lib-test 28 (22 duplicates), integration 1, plus linker unwind-size warning. A requested shell-session invocation was rejected with `workspace_sandbox_required`; the documented blocking execution above did run.

Parent separately executed the newly built original two-case body binary `target/official-nsh-validation/debug/deps/nsh_reading_integration-c754697d113fc18b`. **The production correction is insufficient:** the first case still failed before mounting completed; the second did not execute. `target/nsh-header-body-direct.stderr` records approximately 34,700 publications, 7,200 batches and 21,900 draws; tick 12 took 5.724740792 s and the drain lasted 7.438383333 s before the unchanged two-second assertion failed. These are not the previous run's counts. Next verification adds only finite development-feature field-delta diagnostics and moves the real-header regression to the existing main-thread `harness=false` integration host, retaining its assertions and both original body cases. No runtime yield, global dedup, budget increase or authority relaxation is permitted.

## Still open

The current build emits 30 application warnings, including obsolete legacy selection paths and unfinished retained-reading consumers; this is not strict Clippy green. No latest full workspace/Clippy/fmt/Python/i18n/CI gate is claimed. Application mention and draft-recovery source fixes were independently closed before the harness-only PasteEvent adjustment; actual recovery button/remount/send and three-platform native semantics remain unvalidated. Reading endpoint/listener/core/shared-Copy and geometry-backend reviews are complete; the newer preview fade/focus repairs also have completed focused independent source review. Subsequent source-anchor and integration-test work is a separate unvalidated increment. Real window/native link preview and wrapped-link keyboard/AX acceptance, intra-block trim/reflow anchors, streaming fade/content identity, metadata/AX paste fidelity, remaining visible UI migrations, and final head gates remain incomplete. Nested wheel consumption/remainder protocol is outside the two approved dependency packages and remains unpatched pending separate approval.

## Current-text / #922 followup — ordinary 4/4 green, UI 358 green, recovery 2 passed / 1 failed

**Latest appended evidence; supersedes only the current-status interpretation of the specific invocations below, including the earlier ordinary-not-green status.** This is evidence-document implementation, **not independent review**. All preceding bytes are preserved, including the lifecycle 39.48s failures, callback RefCell failure, list-lifecycle watchdog/SIGTERM diagnostics, footer macro failure and subsequent convergence failures. They remain historical failures or incomplete executions, not retroactive passes. The preceding document was 215,126 bytes with SHA-256 `fc9e483e25667516d44458493fbc6290b7a40cc5e7b02d224091cf34967c2010`; this is a prior-version identity, not this revision's self-hash.

This lane writes only `docs/907/nsh-reading-editing-integrated-validation.md`. It runs no Git, Cargo/rustc/rustfmt, test executable, network, app launch or nested delegation, and changes no implementation or other document. Root AGENTS and the relevant exec-guide rules were read; no nested `docs/**/AGENTS.md` was found. Parent supplies execution exit receipts and command facts; logs and execution JSON were inspected read-only. Unprovided tool-call identities, launcher details, timestamps and complete CLI strings are not invented. The dirty-worktree base remains parent-supplied `0b5ea426abfd8d4358489e916273d89474fb190f`, not a frozen identity for all uncommitted bytes.

### Preserved predicates failure and responsible observation-layer correction

- Parent reports `target/nsh-closure-ordinary-predicates-build.{json,stderr}` **exit 0 / 27.77s**. Stderr confirms 27.77s; JSON line 864 records `build-finished.success=true`, with the ordinary test executable selected at 863, `fresh=false`. This is compilation, not a runtime pass. The complete build argv was not supplied verbatim in this followup.
- `target/nsh-closure-ordinary-predicates-execution.json` records **exit 1 / 13.406s / `watchdog_timeout=false`**, argv not recorded. The exact binary was `target/official-nsh-validation/debug/deps/nsh_ordinary_controls_integration-880bb28b2b36eb9a`, execution-time SHA-256 **`892b8b2b8701df09cf1eca9f27259b3ee98239fc4a0a40580462f88607d5fb4d`**. Stdout reports **1 passed / 1 failed / 2 not executed / 0 ignored**, with only `timestamp_native_narrow_height_and_passive_input` PASS. Stderr 7718–7719 retains the panic at ordinary `mounted_regressions.rs:202:13`, `controls timeout: production NSH displayed/layout/host convergence`, followed by failure of `production_buttons_pass_pointer_through_full_inset_wrappers_and_keep_focus_dispatch`.
- The diagnostic tail has **displayed/valid/enabled/source-current/submission/active true**, matching snapshot/live geometry and root dimensions, healthy Surface, current layouts 3, but **current text events 0 versus historical total text events 3**, with all three `text_matches=false`. Parent identifies the responsible observation fault: event-only text emission is not guaranteed for every Projection. These bytes do not establish missing rendered product text. This does not erase the failed fixture result or justify dropping text assertions.
- The subsequently added, feature-gated **read-only current-text API** is `AppView::integration_text_observation`, implemented in `vendor/nsh-platform-gpui/src/app/text_integration_support.rs`, wired through `app/integration_support.rs`, and exported as `IntegrationTextObservation` under `vega-host-regressions` in `src/lib.rs`. At read time it returns owned current nonselectable native text, actual backend line records and frame/content bounds; checks running state, borrow availability, attachment/parent/node identity, finite geometry/layout and unchanged generation; prunes hidden/concealed/nonpositive-opacity subtrees. Its contract explicitly is **not painted-pixel, AX or cross-snapshot identity evidence**, and callers must reject app errors and reacquire per decision. This is a source observation, not an independent correctness review.
- At read time the ordinary fixture queries that API directly and compares exact expected text multiplicity, nonempty valid line metrics and finite positive bounds; historical text events remain diagnostics rather than the sole readiness oracle. Parent reports correction at the observation layer, not relaxed behavior assertions. No new deadline or acceptance change is claimed by this evidence writer.

### Provenance increment and #922 integration boundary

The inspected `docs/907/nsh-backend-current-manifest.json` has SHA-256 **`73318b7f0de0621ea9537f6704fdbec7a89fc75940c5073ed1ed3b9681cc64fc`**, snapshot UTC `2026-11-09T10:57:13.997769+00:00`. It records **241 current files / 31 delta files**, combined tree SHA-256 `5b5662c032ef28ea8ef968ffc8958fa788fad95a7639ac37796af11c89dd0f61` and delta SHA-256 `de99c2770034c8aa72a04a03a53cbc835ccf693a2ef158efb9eabd94721263ab`. Package counts are core 180 current (1 changed original, no additions/missing) and platform 61 current (14 changed originals, 16 additions, none missing). These are the provenance agent's fixed manifest values, not a newly recomputed whole-tree inventory or behavior/review certificate. Its `observed_origin_master` still records the earlier `0b3dcd3603a8e97e0ef74ae25a26f17fc3a094e5`; do not treat that field as a fresh post-#922 remote observation.

Parent now reports fetched #922 mainline **`2a29adf9b2ea78eaf59d591838531b45adc7373e`**. The inspected [backend import evidence](nsh-mainline-922-backend-import.md) pins that NEW commit/tree `a54ccd7591fd6886817dad7343634044119a28ab`, OLD `0b3dcd3603a8e97e0ef74ae25a26f17fc3a094e5`, and author-verified **12 exact-upstream paths** (including upstream docs), with static import/registration checks only. The inspected [UI compatibility evidence](nsh-mainline-922-compatibility.md) records the separate mixed UI/locale/NSH command-control adaptation and its authored mounted regression; that author's handoff stopped **before compilation/runtime validation**. Those are author/import records, not new independent review. Neither the old upstream implementation document's CI nor the compatibility document's unrun coverage is promoted into #907 runtime evidence. Parent's later build and executions below are separate evidence.

### Actual checks/builds and fixture compile correction

The toolchain is the previously parent-verified **official Cargo `1.98.0 (797e8a9bc 2026-08-05)`**, **rustc `1.98.0 (88d9e12ae 2026-08-18)`**, and **rustfmt `1.9.0-stable`**. This writer runs no toolchain query and infers no Cargo-fork version.

1. `target/nsh-current-text-check.stderr` retains **`error: cannot specify features for packages outside of workspace`** for the attempted direct package check with the feature. This is Cargo workspace/package selection rejection, **not a Rust compile or runtime failure**. Empty stdout is not a pass. Full original argv, exit number and elapsed time were not supplied.
2. Parent then used the existing `cargo rustc` route for `nsh-platform-gpui`, reporting **exit 0 / 13.76s**; `target/nsh-current-text-rustc.stderr` confirms successful dev compilation in 13.76s. Supplied command core (environment/toolchain prefix not repeated as an exact receipt):

   ```sh
   cargo rustc -p nsh-platform-gpui --lib --offline --locked -j 2 --target-dir target/official-nsh-validation -- --cfg 'feature="vega-host-regressions"'
   ```

   This validates the development API's compilation, not native text behavior by itself.
3. Parent reports `cargo check -p vega_conversation` **exit 0 / 4.43s** after the #922 backend import. `target/nsh-mainline-922-backend-check.stderr` confirms its successful dev check. The remaining command flags were not supplied verbatim; they are not reconstructed from other checks. This is not an executed database migration, runtime cleanup test or full-workspace pass.
4. `target/nsh-current-text-mainline-build.{json,stderr}` retains the initial **failed UI build**: JSON 860–864 records five fixture/support errors, and 868 `build-finished.success=false`. Errors: missing `ToolCallStatus` twice at `shell_recovery/command_control_regressions.rs:67,70`; missing `ShellOperationRecord.concurrent` at recovery `mounted_regressions.rs:192`; extra `cx` argument to zero-argument `AppView::stop` at tool-card `mounted_regressions.rs:120`; non-static translation key escaping at that file's line 58. The stderr explicitly says compilation failed due to five errors. No numeric exit/duration was supplied for this attempt.
5. Parent explicitly corrected the responsible fixture/API layer, **without changing assertions**. Read-time source has the `ToolCallStatus` import, `concurrent: false` in the existing recovery record, `view.stop()` and `key: &'static str`. This source read is not an assertion-diff review or a claim that future writer bytes are identical.
6. Parent reports repaired build **exit 0 / 51.98s**, with supplied Cargo CLI:

   ```sh
   cargo test -p vega_ui --features nsh-reading-integration --lib --test nsh_ordinary_controls_integration --test nsh_recovery_integration --no-run --offline --locked -j 2 --target-dir target/official-nsh-validation --message-format=json
   ```

   The environment/toolchain launcher prefix was not supplied verbatim for this invocation. `target/nsh-current-text-mainline-repaired-build.stderr:2` confirms 51.98s; JSON 868 records `build-finished.success=true`, with test-profile UI/ recovery/ordinary artifacts at 862/866/867, all `fresh=false`. Linker warnings at 861,864,865 retain `__eh_frame section too large (max 16MB)` and potential exception-handling performance impact. The predicates build also retained this warning. Build success is not a warning-free claim, and `--no-run` is not test execution.

### Exact repaired-artifact executions — partial green, not final acceptance

`target/nsh-current-text-mainline-execution.json` records the following exact artifact hashes/argv, stream paths, exit statuses and elapsed times. **All three record `watchdog_timeout=false`.** Paths below are relative to `target/official-nsh-validation/debug/deps/`; all processes used the absolute worktree paths recorded in that JSON. Elapsed times are process durations, not the UI test footer's 0.68s harness duration.

| Target / artifact | Exact argv | SHA-256 at execution | Exit / elapsed | Actual completed result |
| --- | --- | --- | --- | --- |
| `nsh_ordinary_controls_integration-880bb28b2b36eb9a` | `[]` | `636a86aa2b716b6baa3856b8534e28ade99aa7173591eade9e5ebdd6b8525b0f` | **0 / 4.704s** | **4 passed / 0 failed / 0 not executed / 0 ignored** |
| `nsh_recovery_integration-9271aa7368d76ca1` | `[]` | `701299d501227b390439f5427948eba040aab5144a58ee420de04a2f5cf8882f` | **1 / 14.943s** | **2 passed / 1 failed / 0 not executed / 0 ignored** |
| `vega_ui-4268349f2e74c4f3` | `["--test-threads=2"]` | `abd24f68921eb3e9b636fe1e058872d9904028a457ec0e512fcf60e5303de8dc` | **0 / 2.903s** | **358 passed / 0 failed / 0 ignored / 0 measured / 0 filtered out** |

Ordinary stdout explicitly reports PASS for all four original runner entries: `timestamp_native_narrow_height_and_passive_input`; `production_buttons_pass_pointer_through_full_inset_wrappers_and_keep_focus_dispatch`; `retry_and_same_attempt_fact_changes_reject_old_displayed_controls`; `route_settings_geometry_and_unmount_reject_captured_controls`. This is a new successful execution after the observation/fixture corrections, **not a blind retry or relabeling of the 13.406s failed run**. The numeric artifact pathname is reused, but the execution hashes differ; current filesystem bytes must not be substituted for the earlier binary identity.

Recovery stdout explicitly reports PASS for `draft_summary_opens_separate_overlay_with_real_focus` and `pending_draft_close_escape_and_authority_keep_recovery_fenced`. Stderr 2265 onward retains **`mounted command control deadline`** at `shell_recovery/command_control_regressions.rs:145:13`, followed by FAIL `mounted_cancel_supersedes_read_and_retry_keeps_operation_authority`. The read-time fixture retains its **original 10-second deadline**. The new card case actually executed and failed; it is **not unrun, ignored, a watchdog timeout, or covered by the two existing recovery passes**. Repeated draw diagnostics do not by themselves establish the responsible product/harness layer. Parent reports that another UI author now diagnoses it; no final cause, fix, retry or passing result is certified here.

### Retained log identities (SHA-256)

All following paths are relative to `target/`. These hashes identify the inspected raw streams/JSON, including empty streams; they are not exit receipts or hashes of this document.

| Log | SHA-256 |
| --- | --- |
| `nsh-closure-ordinary-predicates-build.json` | `f333f93b1112762481e8106ade3ef66a04b450d4993a916b2c7119a1c006062d` |
| `nsh-closure-ordinary-predicates-build.stderr` | `0bf54f21141a6e04379436883836a02df2d861018d3091cffa2476d88bae1f14` |
| `nsh-closure-ordinary-predicates-execution.json` | `6bfe00459e18b1fb4111c014441e10baa64f0750b2d2d7aa2dbd366cf457664e` |
| `nsh-closure-ordinary-predicates.stdout` | `c88728614a88a4b381f9f9527afcd48164b71e586220101e1beb1edc483d70a3` |
| `nsh-closure-ordinary-predicates.stderr` | `375b96f2b06f24016ed08972733031d762d17e6a9ed89af5bcb6afa65a89707a` |
| `nsh-current-text-check.stdout` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `nsh-current-text-check.stderr` | `e8ba25ea0890bf6630709e9f811ccb71944e489015181c521f81c609f9040c2a` |
| `nsh-current-text-rustc.stdout` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `nsh-current-text-rustc.stderr` | `b7793c9d19809f9336c47e01b75ea348b647e7c232b6fc42b93e3c7fa9ebb6fe` |
| `nsh-mainline-922-backend-check.stdout` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `nsh-mainline-922-backend-check.stderr` | `45b07179d63c9ccdc3ce171ba06c4564772f6d7165c4bd6ecc32c5612fb19bc5` |
| `nsh-current-text-mainline-build.json` | `b4e80cd92fd7bc8a31ef39ca5a47f23e388f902af4fa80c2365295d44d6ba079` |
| `nsh-current-text-mainline-build.stderr` | `aa67a9116305b31a40c3f87d7b0246624e78a420afdb33530d8676f1a8d1142d` |
| `nsh-current-text-mainline-repaired-build.json` | `0f4e4b59383922aed6471fa1fdb659610f38eb1d70106aff6b0df283947e704e` |
| `nsh-current-text-mainline-repaired-build.stderr` | `f622e227a9b9ed8dfd6ac0b5920b3f2f6c1c55c7c2ad42ec29612463af0d787b` |
| `nsh-current-text-mainline-execution.json` | `3bb796e1b132aa1c54e768609f95127ed909a900584196fff6365a1d659b7d61` |
| `nsh-current-text-mainline-nsh_ordinary_controls_integration.stdout` | `53a888d7d23e0be2e972fa743d35b95d53fcfef561273308390ee7bdac5db684` |
| `nsh-current-text-mainline-nsh_ordinary_controls_integration.stderr` | `49b81ec5ac4bb1a0012e8811429f85bc8b17479fc50b79382910df4b495dfa30` |
| `nsh-current-text-mainline-nsh_recovery_integration.stdout` | `64abc5b97ccc446a13c41dbb9f520f7df62a7dda3b17e2949a1e668a3cd27c83` |
| `nsh-current-text-mainline-nsh_recovery_integration.stderr` | `ee89da5855823736d03e6303325829425717a23f6ec5ca80c6d857ea3c24e15d` |
| `nsh-current-text-mainline-vega_ui.stdout` | `ed53540b6f5384615a838b8e3f554d545e56943590719cf11a321dd9d32ac61c` |
| `nsh-current-text-mainline-vega_ui.stderr` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |

### Read-time source and referenced-document identities, not future/build bindings

The following SHA-256 values were computed from actual bytes in this evidence-writing session. Implementation authors may be changing source concurrently. These values are **at-read observations only**, not proof of bytes compiled into either execution, a stopped-writer fence, a complete migration snapshot, or an identity for future repairs. Fixed execution hashes above remain the run identities. Referenced author documents retain their own historical scope; hashing them does not endorse their conclusions or refresh their checks.

| Path | SHA-256 at read |
| --- | --- |
| `vendor/nsh-platform-gpui/src/app.rs` | `7d73eda709a7ab2cafa23384c35fedb33935374ad9972b3b8ba25be1e9eead6f` |
| `vendor/nsh-platform-gpui/src/app/text_integration_support.rs` | `d9a90d8d5a2a9558113288d37afecb320b9e353560d4bffeecdf5c1688f3ec4c` |
| `vendor/nsh-platform-gpui/src/app/integration_support.rs` | `ad96f234d393ec4e283bc44bab60335b018033ac1efbf492efb18dd3e54cc4e7` |
| `vendor/nsh-platform-gpui/src/lib.rs` | `a509bd210b55681f08df36848e5f16f2396d9298f46b11571bb778298d0a58fc` |
| `crates/vega_ui/src/conversation_stream/ordinary_submissions/presentation/mounted_regressions.rs` | `c197eaaaccbd3127377de40e4b7079519b043b2e381e13e47eb6639a7691ca7b` |
| `crates/vega_ui/src/conversation_stream/shell_recovery/command_control_regressions.rs` | `c47ef0fb10f3777e9ec1c0f1b070cc2700e950027e551ffc2e3d03d2215fed0d` |
| `crates/vega_ui/src/conversation_stream/shell_recovery/mounted_regressions.rs` | `35933af419fb42b91ffe00577687d9de251c08f10b7270e326b8f251af17bb9a` |
| `crates/vega_ui/src/tool_card/nsh_tool/mounted_regressions.rs` | `046e92fd2e16746766001f6a8a27c69d19d33453218dbf330b6ca7c9da051994` |
| `crates/vega_ui/tests/nsh_ordinary_controls_integration.rs` | `071bbc2c452b277e64f6b1e634bb62df44f9d7a1fd5210db1caf9712d0b012d6` |
| `crates/vega_ui/tests/nsh_recovery_integration.rs` | `bf1d749fc80be338e96a66365ed87f5889867da1e501b56aee59b33e3b56f189` |
| `docs/907/nsh-backend-current-manifest.json` | `73318b7f0de0621ea9537f6704fdbec7a89fc75940c5073ed1ed3b9681cc64fc` |
| `docs/907/nsh-mainline-922-compatibility.md` | `f18df35aa2ef2d7036ffa0a0e0eca317e3f4d04ad3fabe423ff6ad50a20e8e44` |
| `docs/907/nsh-mainline-922-backend-import.md` | `6e6e0a803bb2ab0b502028bad5463ddd617e1ad27c6a69dbdde4c22961822759` |

### Current limitations and evidence-lane handoff

**Ordinary mounted now has a completed 4/4 green run; full UI has 358 green; recovery is still not green because the new command-control case failed its original deadline.** No final product-versus-observation diagnosis or future repair is inferred for recovery. Parent/UI author owns that diagnosis and subsequent coherent-version validation. This followup supplies no new final all-workspace fmt or strict Clippy result; earlier format failures/corrections and Clippy passes remain version-specific historical evidence, not refreshed by these builds. The prior ten-target/lifecycle/native/scroll/etc. runs likewise are not a new post-#922 all-target pass.

No latest-head independent review, required CI, native/manual OS IME/AX or macOS/Windows/Linux acceptance, development-test archival/removal or zero-test final delivery gate, commit, push, merge, packaging, installation or delivery is performed or claimed. The user-facing progress convention remains **fixed module weights, actual per-module results/blockers, no total estimate or invented overall percentage**. No self-referential document hash is inserted. **READY means only this evidence lane is ready after readback; stop writing and return ownership to parent.**


## Stopped-implementation closure followup — eleven UI targets green, app/workspace green, review pending

**Latest appended local-development evidence; supersedes the preceding current recovery-not-green status only for the completed invocations below.** Every preceding byte is retained, including the lifecycle 39.48s failures, callback borrow failure, watchdog and diagnostic terminations, macro/fixture compile failures, ordinary convergence failures, and prior recovery deadline failure. A subsequent green invocation does not retroactively pass any of them. This is the **evidence writer lane, not independent review**. Parent reports all implementation writers stopped and three new non-author independent reviewers running; this records dispatch/state reported by parent, **not reviewer conclusions, approval, or verified reviewer completion**. This lane performs no source mutation or review delegation.

### Frozen review content, scope and identity boundary

- Parent-supplied base HEAD remains `0b5ea426abfd8d4358489e916273d89474fb190f`; integrated #922 mainline is `2a29adf9b2ea78eaf59d591838531b45adc7373e`. No Git operation was performed by this lane. These commit IDs alone do not identify the dirty migration.
- `target/nsh-implementation-review-content.json` records UTC `2026-11-09T11:48:08.327075+00:00`, **2,137 files**, content identity **`46c7fbd372131b277956f6818af7def7ba8ce1b3912ce8fca377267fea78a3dc`**. Its algorithm is SHA-256 of `json.dumps(sorted((relative_POSIX_path, SHA256(raw_bytes)) pairs)).encode()`. This lane independently recomputed that identity and read/hashed every listed path before this append: **2,137 matched, zero missing, zero changed**. Parent describes the freeze as excluding Python cache files; the manifest's exact path list, rather than an inferred whole-directory scope, defines this identity.
- The manifest file's own SHA-256 is **`9b5d3ccf193668c12833165b216c842d65d8848bf056a84a7d02b6b32c96d71c`**, distinct from its content-set identity. `target/nsh-implementation-review-tracked.patch` is 1,100,239 bytes, SHA-256 **`6833922e3f600349fe53838137f93ca893de87c2f80a707dfd75c9a0c97eb5ad`**; this is an existing patch read, not a newly generated Git diff or an archive-completion claim.
- The frozen path list includes **this validation document's pre-append bytes**. Consequently the all-listed-path identity is **historical after this authorized append**; do not relabel it as a new whole-worktree identity. Reviewers must name their source/path scope and check its per-file hashes. This lane changes only this document; source-content continuity is verified against the frozen per-file identities, not by pretending the old all-content identity covers new documentation. Unlisted files and future changes are outside that comparison.
- Applicable root AGENTS and execution-guide rules were read; `docs/AGENTS.md` and `docs/907/AGENTS.md` were absent. This lane runs no Cargo, Git, rustc/rustfmt, test binary, network, app launch, CI query, or nested delegation. Python is used only for read-only inspection/hashing and this sole authorized document append. Tool-call identities not supplied by parent are not invented.

### Recovery predicates failure, corrected current-measure observation, and separate successful executions

1. Parent's `target/nsh-command-control-predicates-build.{json,stderr}` build reports **exit 0 / 26.55s**. JSON ends with `build-finished.success=true`; selected recovery integration artifact is test-profile, `fresh=false`. Its execution JSON retains **exit 1 / 14.902s / `watchdog_timeout=false`**, binary `nsh_recovery_integration-9271aa7368d76ca1`, execution SHA-256 **`482a768f6ae94b22f93f13b00a113a1287e7cf08ad5ab7ab5a70e074b2e3f282`**. That JSON does not record argv; none is invented. Stdout reports **2 passed / 1 failed / 0 not executed / 0 ignored**.
2. The failure is the original mounted command-control deadline at `shell_recovery/command_control_regressions.rs:168:13`, **phase `pending Stopping`**. The diagnostic reports current identity/content/layout true and actual native text **`Stopping command`**, while the stored control measurement has stale identity/content (`false, false, true`). Parent classifies this as an observation-layer old-measurement fault, not a missing product Stopping state. The failed test remains failed; this is not a watchdog or a blind retry to green.
3. Parent reports the development observer was corrected to measure on **tracked Snapshot changes plus layout**, using **actual page coordinates**, without a business/product behavior change. Read-time source supports the stated mechanism: `tool_card/nsh_tool.rs:815–818` has feature-gated `effect(... state.get())`; lines 843–846 schedule measurement on layout. `nsh_tool/mounted_regressions.rs:15–38` compares identity/content/layout, rejects a stale awaited measurement, and builds bounds from `measurement.page` plus `measurement.frame.size`. Lines 65–97 still require current matching control measurement, measured card size, healthy Surface and current native control/notice text. This is a focused source observation, **not an independent review or assertion-equivalence audit**.
4. `target/nsh-command-control-current-measure-build.{json,stderr}` reports **exit 0 / 27.67s**, JSON `build-finished.success=true`, three selected test-profile artifacts all `fresh=false`. Its execution JSON and raw streams record the following completed results; **all exit 0, all `watchdog_timeout=false`**. Artifact names are relative to `target/official-nsh-validation/debug/deps/`; exact absolute paths are retained in JSON. Times are process elapsed values, not harness-only durations.

| Target / artifact | Recorded argv | Execution SHA-256 | Elapsed | Completed result |
| --- | --- | --- | --- | --- |
| `nsh_recovery_integration` / `nsh_recovery_integration-9271aa7368d76ca1` | `[]` | `c8e37f0834a100ef56f0e38a156fe35d3c1457aa71b296c1b333962eaeb65578` | 5.186s | **3 passed**, 0 failed, 0 ignored |
| `nsh_ordinary_controls_integration` / `nsh_ordinary_controls_integration-880bb28b2b36eb9a` | `[]` | `27fee997986040ec78814a9becd3345b2b188df14bd41692e56cb30be38bf853` | 4.440s | **4 passed**, 0 failed, 0 ignored |
| `vega_ui` / `vega_ui-4268349f2e74c4f3` | `["--test-threads=2"]` | `d07ce151e58cb48cc35a05ae75f3451974ddc115e6605b9be324b16c76278f80` | 2.915s | **358 passed**, 0 failed, 0 ignored |

### Format and Clippy: preserve failed invocation and separate corrected check

- `target/nsh-implementation-closure-fmt.{stdout,stderr}`: parent reports **exit 0** for the full format check. Both streams are empty; empty output is not independently an exit receipt. Parent also reports a later final full Cargo fmt check **exit 0 on console, no retained logfile**, after the equivalent function-pointer correction. The later console-only result is attributed to parent, not falsely linked to the earlier empty streams or presented as a newly hashed log. No fmt command was run by this lane.
- `target/nsh-implementation-closure-clippy.{stdout,stderr}` preserves the **failed** strict workspace all-target invocation: one `clippy::redundant_closure` at `shell_recovery/command_control_regressions.rs:307`, recommending `controls::stop` in place of `|card, cx| controls::stop(card, cx)`. It failed compilation of both lib and lib-test due to that diagnostic; a numeric exit/time was not supplied for that attempt. Parent made the equivalent function-pointer correction; current line 307 is `card.update(cx, controls::stop);`. This does not weaken assertions or reclassify the original failure.
- `target/nsh-implementation-closure-final-clippy.{stdout,stderr}`: parent reports **exit 0 / 12.73s**, strict workspace **all targets / both development features**, `-D warnings`. Stderr confirms a completed dev-profile check in 12.73s. Both features refer to the established reading/workspace integration feature scopes. The exact full launcher/argv was not supplied anew in this followup and is not reconstructed from an older invocation. This is a local check, not CI success.

### Final eleven-target UI execution — all completed green, no watchdogs

`target/nsh-implementation-closure-build.{json,stderr}` reports **exit 0 / 25.15s**, with JSON `build-finished.success=true` and all eleven selected executable artifacts test-profile, `fresh=false`. This compilation is separate from execution. `target/nsh-implementation-closure-execution.json` records the exact paths, binary hashes, arguments, stream files, exits and process times below. **Every listed execution has exit 0 and `watchdog_timeout=false`.** Full UI and host libtest output reports zero failed/ignored; mounted/custom harness footers report zero failed/ignored and zero not-executed/unrun where that field exists. Geometry/settings footers do not define a not-executed field; none is invented.

| Target / artifact | Recorded argv | Execution SHA-256 | Elapsed | Completed result |
| --- | --- | --- | --- | --- |
| `vega_ui` / `vega_ui-4268349f2e74c4f3` | `["--test-threads=2"]` | `af6603febc37e740a244d2294bbdbb6d512ef0539060ec48786f4bf028b570a0` | 2.936s | **358 passed**, 0 failed, 0 ignored |
| `nsh_native_host` / `nsh_native_host-c6fd6b079b892425` | `[]` | `2f8672a000adade01c9d2b2e2a850a3f7269737fa6e828b1e30bae14a3e168e4` | 1.063s | **36 passed**, 0 failed, 0 ignored |
| `nsh_reading_geometry` / `nsh_reading_geometry-da777a5f64c485cf` | `[]` | `cc9b9361803883230429d0c061d775bbb6e7b0d8427faaa88a7d894988463699` | 6.614s | **24 passed**, 0 failed, 0 ignored |
| `nsh_reading_integration` / `nsh_reading_integration-c754697d113fc18b` | `[]` | `bff8cea4d8759cfc432fafecbdee7fe9cbb5cce618c1221fc8cf399b5398ab2a` | 29.683s | **15 passed**, 0 failed, 0 ignored |
| `nsh_scroll_integration` / `nsh_scroll_integration-1d1fd85936d176b3` | `[]` | `5968245ccbbed093b2af83ae3614342d27d8ace3e2167182c52913441f2bbd29` | 91.782s | **16 passed**, 0 failed, 0 ignored |
| `nsh_icons_integration` / `nsh_icons_integration-c7d68528d5563f59` | `[]` | `873acd0356a486f25fa6514d8ccccd82a3f69e321d60867d08db0b1ba18aa50f` | 3.433s | **3 passed**, 0 failed, 0 ignored |
| `nsh_settings_integration` / `nsh_settings_integration-c1b0a9b018bd18c8` | `[]` | `f2ec1e9eafd461e6448dd1266869ecc0fb677bc36e399ad7343adb19601e4782` | 4.199s | **3 passed**, 0 failed, 0 ignored |
| `nsh_recovery_integration` / `nsh_recovery_integration-9271aa7368d76ca1` | `[]` | `e85146e27ce13538e56370abf7211e5b97b2381df708043d577edea5073af499` | 5.595s | **3 passed**, 0 failed, 0 ignored |
| `nsh_message_anchors_integration` / `nsh_message_anchors_integration-b317b2d7635563d8` | `[]` | `a69a7efa7caef6c0b9d95650af22d01c3539fdb0608812ada1b14f178302eb83` | 3.980s | **6 passed**, 0 failed, 0 ignored |
| `nsh_palette_integration` / `nsh_palette_integration-c2e61111db3ee3f6` | `[]` | `62e33da897bbe0d40eaaba11d424838acad15a3f5d6c3cca7bc213749775fdd9` | 4.294s | **3 passed**, 0 failed, 0 ignored |
| `nsh_ordinary_controls_integration` / `nsh_ordinary_controls_integration-880bb28b2b36eb9a` | `[]` | `900a23c5f6c2f7ab1c274c31710f1b49c5e168b1602b107d668221b56a87406f` | 5.083s | **4 passed**, 0 failed, 0 ignored |

Palette's **3 passed / 0 failed / 0 unrun / 0 ignored** footer is on **stderr**, not its empty stdout. Reading/scrolling/geometry and other completed targets are confirmed from their actual raw footers, not inferred from exit status alone. These results supersede earlier current-status blockers for these particular checks while retaining their failed/incomplete history. `nsh_native_host` here is a development host test target, **not native OS AX/IME/manual acceptance**.

### App/workspace build and execution — no application launch or release packaging

`target/nsh-implementation-closure-app-build.{json,stderr}` reports **exit 0 / 1m24s**. JSON ends `build-finished.success=true`, selects test-profile `nsh_workspace_mounted-84e12a4f84938dce` and test-profile `vega-5abc20f909d24de7` (`fresh=false`), and also emits the non-test `debug/vega` artifact. The execution JSON selects the **test executables below**, not that ordinary application artifact. Build success and emitted binary do not mean the application was launched, installed, packaged, or released.

| Target / artifact | Recorded argv | Execution SHA-256 | Elapsed | Completed result |
| --- | --- | --- | --- | --- |
| `nsh_workspace_mounted` / `nsh_workspace_mounted-84e12a4f84938dce` | `[]` | `f81344b9eeeda6058113d941bc06d0181f5c22889c8d7cac9da6521d3a3f3601` | 3.401s | **6 passed**, 0 failed, 0 ignored |
| `vega` / `vega-5abc20f909d24de7` | `["--test-threads=2"]` | `608f1563a7eef69672747381499c56485bfae4b9f6f6ba3f92c292f567cb77bb` | 0.779s | **31 passed**, 0 failed, 0 ignored |

Workspace's **6 passed / 0 failed / 0 not executed / 0 ignored** footer is on **stderr**. The app libtest footer has **31 passed / 0 failed / 0 ignored / 0 measured / 0 filtered out**. Neither is user acceptance.

### Python checks and Nextest — real completed local counts, runner mismatch retained

| Parent-executed check | Logs under `target/` | Actual result |
| --- | --- | --- |
| `python3 scripts/release_tests.py` | `nsh-implementation-closure-release-python.{stdout,stderr}` | Parent exit **0**; stderr **5 tests / OK / 0.010s** |
| `python3 scripts/prepare_computer_use_tests.py` | `nsh-implementation-closure-cua-python.{stdout,stderr}` | Parent exit **0**; stderr **15 tests / OK / 0.027s** |
| `python3 -m unittest discover -s scripts/tests -v` | `nsh-implementation-closure-scripts-python.{stdout,stderr}` | Parent exit **0**; stderr **40 tests / OK / 0.319s** |

These Python entry points are the established check identities; no additional launcher/environment argv is inferred. Harness footer times are not newly measured whole-process elapsed times.

- Parent reports local **Nextest 0.9.151**, **not CI's pinned 0.9.146**. The existing verified toolchain record carried forward is official **cargo 1.98.0 (`797e8a9bc 2026-08-05`)**, **rustc 1.98.0 (`88d9e12ae 2026-08-18`)**, **rustfmt 1.9.0-stable**. This lane did not rerun version commands; local runner/toolchain evidence is not a claim of CI-version parity.
- `target/nsh-implementation-closure-nextest-list.{json,stderr}` preserves the **initial rejected list invocation**: `error: unexpected argument '-j' found`. JSON is empty; this is CLI/harness argument rejection, **not product failure, test discovery, or test execution**. Parent supplied no exact exit integer. The recommended `-- -j` tip is not proof of a command actually run and was not replayed by this lane.
- Parent corrected build concurrency via **`CARGO_BUILD_JOBS=2`**, rather than the rejected Nextest `-j`, and reports fixed list **exit 0 / 28.16s**. `target/nsh-implementation-closure-nextest-list-fixed.json` parses with **`test-count: 389`** and **16 rust-suites**; only app 31 and UI 358 contain tests. The 14 empty suites are not additional passed tests. Fixed-list stderr confirms successful test-profile build in 28.16s. Full original Nextest argv/filter/launcher is not available in this followup; exact scope is evidenced by the list JSON and run output, not guessed from filenames or a historical CLI.
- `target/nsh-implementation-closure-nextest-run.{stdout,stderr}`: parent reports **exit 0**. Stderr records default profile, run ID **`69c954a6-9d33-486e-8153-3be7ffa6a46c`**, **389 tests across 16 binaries**, and **`389 tests run: 389 passed, 0 skipped` in 1.076s**. Parent reports default test parallelism; `CARGO_BUILD_JOBS=2` constrains build jobs, not test-thread parallelism. The 0.44s test-profile finish is separate from 1.076s runtime. This is a nonempty local run, not the final required zero-Rust-test CI gate, and does not replace the separately executed integration harnesses above.
- **Warning preserved:** fixed-list and run stderr contain `ld: __eh_frame section too large (max 16MB) to encode dwarf unwind offsets in compact unwind table, performance of exception handling might be affected`; `vega_ui` lib-test generated one linker warning. Successful exit is not a warning-free claim. This warning is not silently dropped or misreported as the strict Clippy failure.

### Exact command and provenance limits

Parent supplied invocation identities, scope, exit receipts and timings; the on-disk build JSON/streams corroborate outcomes, target kinds, test profiles and selected artifacts, but **do not retain complete original shell commands**. This followup therefore records the known operation/scope instead of manufacturing exact build/fmt/Clippy/Nextest command strings, argument order, environment prefixes, timeout settings or parent tool-call IDs. Historical full CLIs elsewhere in this document remain historical and are not falsely presented as exact receipts for these new invocations. The execution JSONs above do preserve exact executable paths and `args` (except the predicates JSON, which has no `args` field), so those runtime arguments are recorded exactly. A parent audit receipt is needed if reproduction requires the missing full build/check argv. This limitation does not change the observed completed results.

### Read-time source identities within the frozen path scope

The following raw-byte identities were read here and match their manifest entries before append. They describe focused referenced source bytes, not a new independent code review, a reproduction of the build environment, or permission for further implementation changes. Earlier execution hashes remain execution-time records even if later builds overwrite the same binary pathname.

| Referenced source path | SHA-256 at read / frozen manifest match |
| --- | --- |
| `crates/vega_ui/src/conversation_stream/shell_recovery/command_control_regressions.rs` | `7c07882d2d36de89d5f50c56a97d6228ea97e4ac38bbc48d6b3c8eb667da904a` |
| `crates/vega_ui/src/tool_card/nsh_tool.rs` | `19274fe66c25eb181662c7df0089582808191847f3c807b7ade38205ebf398ae` |
| `crates/vega_ui/src/tool_card/nsh_tool/mounted_regressions.rs` | `f3634c60abfafbe2b01a255f7a84136d65157785b580e8898a1140cc94a2f7f6` |
| `crates/vega_ui/src/conversation_stream/ordinary_submissions/presentation/mounted_regressions.rs` | `c197eaaaccbd3127377de40e4b7079519b043b2e381e13e47eb6639a7691ca7b` |
| `vendor/nsh-platform-gpui/src/app/text_integration_support.rs` | `d9a90d8d5a2a9558113288d37afecb320b9e353560d4bffeecdf5c1688f3ec4c` |

### Retained log byte identities for this followup

Every file below was read before writing this appendix; SHA-256 is over actual bytes. Paths are relative to `target/`. Empty streams have the standard empty-byte hash; their emptiness is not treated as an exit receipt. These are **log/manifest/patch hashes, not a self-referential hash of this document**. Execution-time binary hashes remain separately recorded above and are not substituted with a later rebuilt file's hash.

| Evidence file | SHA-256 |
| --- | --- |
| `nsh-command-control-current-measure-build.json` | `93e19aa2d379a4ab39b2936e183e9b52c7e2cf7bf3331af5ef8dbd07b7c29979` |
| `nsh-command-control-current-measure-build.stderr` | `70f98bbb6b62fd2200b3d4e79cb2cbee18bf9293f14461b01a53b91ef13cab16` |
| `nsh-command-control-current-measure-execution.json` | `f65341bbf279e5dbadd59aff554bd793a2c2a83975d070ed5797239e2c1266c7` |
| `nsh-command-control-current-measure-nsh_ordinary_controls_integration.stderr` | `4d0c8b1cbf9459a6bd164c12dad705c83ca257d16c84e29a177a4a6bdaa34ea3` |
| `nsh-command-control-current-measure-nsh_ordinary_controls_integration.stdout` | `53a888d7d23e0be2e972fa743d35b95d53fcfef561273308390ee7bdac5db684` |
| `nsh-command-control-current-measure-nsh_recovery_integration.stderr` | `c44ed90c20a7be17c878788403d335ddeca580fa9699103df861532dd1e122be` |
| `nsh-command-control-current-measure-nsh_recovery_integration.stdout` | `2a92be9fa55f6c82a399a0f4ff8f195970f9c17d99911cfa26a933bea45d334f` |
| `nsh-command-control-current-measure-vega_ui.stderr` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `nsh-command-control-current-measure-vega_ui.stdout` | `5d41576b2cacf5c6d6344fd480e079d06ae63301861d0db91dbe925a97654175` |
| `nsh-command-control-predicates-build.json` | `29dcea46c4b302d630491339c3a8aba201f53ccbdf981f0438363baa1f216b90` |
| `nsh-command-control-predicates-build.stderr` | `34ff0dc664d9588606f584b77c202cc24baae49c73268bb1cf2fe75fc095eb5b` |
| `nsh-command-control-predicates-execution.json` | `c028d2ef77d5f5c678cdf1c7444ac8955b38283fae95f1a6999d781216ac61c5` |
| `nsh-command-control-predicates.stderr` | `1ec2c81454b1c2d113e3c9d8b9fdd3d72a9cd7812dea7278d21329f96127fabe` |
| `nsh-command-control-predicates.stdout` | `64abc5b97ccc446a13c41dbb9f520f7df62a7dda3b17e2949a1e668a3cd27c83` |
| `nsh-implementation-closure-app-build.json` | `0e1485c91ad9619d04e096c6b531407e6faf98c1e588c339f170a844ecc8cd4d` |
| `nsh-implementation-closure-app-build.stderr` | `24482fc46f6403968431c12ed91494561e21046c253d92ac18123b0193b93709` |
| `nsh-implementation-closure-app-execution.json` | `a821b6816ffc2fe7394de05a2932b57deb29a6c07cabc90ab4b119e6be5cacf3` |
| `nsh-implementation-closure-build.json` | `306f7424d89e3923bc204b9676213983228ea97dc3e924fcfd66e3b82e30bf02` |
| `nsh-implementation-closure-build.stderr` | `af862fcbc512860d3c0ccab073d02012327266ece9d5487606e0a74290b32ebb` |
| `nsh-implementation-closure-clippy.stderr` | `60952d5a01cc902d266d4b480501a8c5ac2f75bca25dfeb3d3607dba08e075c2` |
| `nsh-implementation-closure-clippy.stdout` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `nsh-implementation-closure-cua-python.stderr` | `a2dc3c137a58e07b1fd7b3c8f9e51920dcf77b1e278bedb9dc1292f27cdb7160` |
| `nsh-implementation-closure-cua-python.stdout` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `nsh-implementation-closure-execution.json` | `ae24c5fe14ab5c8472b2eedb95c10253823bb7ade2353ae1ad72daa80f07de70` |
| `nsh-implementation-closure-final-clippy.stderr` | `94637b073a16289db091cacc2b1d6247c5a8f14fdc6e64ef1ee22ca9c37d1c1e` |
| `nsh-implementation-closure-final-clippy.stdout` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `nsh-implementation-closure-fmt.stderr` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `nsh-implementation-closure-fmt.stdout` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `nsh-implementation-closure-nextest-list-fixed.json` | `222fb6b75278737ba3bd8d58761dcf2b3ea613eabc617282513eefa84ca90641` |
| `nsh-implementation-closure-nextest-list-fixed.stderr` | `a6879b2fca8f47bfa4dc891356888562297240aa0987d42a3ac0028ab3120685` |
| `nsh-implementation-closure-nextest-list.json` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `nsh-implementation-closure-nextest-list.stderr` | `d52bec90370fea2c3b7ce495e4b43acdb9b56b4c6b26568400405fafac766b4e` |
| `nsh-implementation-closure-nextest-run.stderr` | `4765c667996b1e4e2dc21684d2720010895d97bc2843509ce789cd55250380ca` |
| `nsh-implementation-closure-nextest-run.stdout` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `nsh-implementation-closure-nsh_icons_integration.stderr` | `e7f78d02dd0d03a9c32d05aa6505d71c2fbb82aff8ccfde0326e7013d58f541a` |
| `nsh-implementation-closure-nsh_icons_integration.stdout` | `9dd4a6c2ee6ecaa71af30d6404b9bb0929572d21e57d100382177679718322c8` |
| `nsh-implementation-closure-nsh_message_anchors_integration.stderr` | `6aee9b97bdd91a757b6611628330d5e257b2cf5cf6b3c4574daa489a843617f8` |
| `nsh-implementation-closure-nsh_message_anchors_integration.stdout` | `8eb0583d25817b1a6e3edf3593144762f3cf1f289b2e029093bc07ca71ff2c22` |
| `nsh-implementation-closure-nsh_native_host.stderr` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `nsh-implementation-closure-nsh_native_host.stdout` | `8be046e1160d2721d7a80a5ee5cdcddf7d1d3ab046006e34b5a5a1c097ed87f3` |
| `nsh-implementation-closure-nsh_ordinary_controls_integration.stderr` | `d9ede7c0b8e46fa507e11bc2293bc2ef9543d97c39606bc050dda2a82eb281df` |
| `nsh-implementation-closure-nsh_ordinary_controls_integration.stdout` | `53a888d7d23e0be2e972fa743d35b95d53fcfef561273308390ee7bdac5db684` |
| `nsh-implementation-closure-nsh_palette_integration.stderr` | `f42e8236278cb3f467ed9dd696f7a5c0ae212cecc8a4706508e14bac2a86caab` |
| `nsh-implementation-closure-nsh_palette_integration.stdout` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `nsh-implementation-closure-nsh_reading_geometry.stderr` | `85e36a0cb4709c5a55240c40deb7b9a00aa461f37c35f16d7edff133fac783fa` |
| `nsh-implementation-closure-nsh_reading_geometry.stdout` | `59600e972c5380677b5f8ff6afbf1bf23dc2b855f8e107108f266316f560297a` |
| `nsh-implementation-closure-nsh_reading_integration.stderr` | `7090bf23908f4856c1fbea68a42d10e397a5f9c9c09f656b1065dae062185732` |
| `nsh-implementation-closure-nsh_reading_integration.stdout` | `900151850e4c5049cd2ba0de4c94fb8d0b46fe0f032059566bbb6484ef727e32` |
| `nsh-implementation-closure-nsh_recovery_integration.stderr` | `d572bc2867dc6619b69984c58199d122527bed341448448d17cbc4f7ce53aa46` |
| `nsh-implementation-closure-nsh_recovery_integration.stdout` | `2a92be9fa55f6c82a399a0f4ff8f195970f9c17d99911cfa26a933bea45d334f` |
| `nsh-implementation-closure-nsh_scroll_integration.stderr` | `3278ac68bdcf31252288c916a0330137e2f6a37529823d89fc21ca3714f57c4f` |
| `nsh-implementation-closure-nsh_scroll_integration.stdout` | `5e3b12f7cb3cbf7b16577739653275717d76285f472a095dd12a58fa538390cf` |
| `nsh-implementation-closure-nsh_settings_integration.stderr` | `17c4333220f6b0c3b30f50d447eb3bb758d5f493a19ba5b9fdfdcc589975b143` |
| `nsh-implementation-closure-nsh_settings_integration.stdout` | `b57f873f5d8625dab01193161d46fe1f462a0cec4249c804c7adead0ebb36637` |
| `nsh-implementation-closure-nsh_workspace_mounted.stderr` | `2a232dd40ed9188a5c6eb029fb95740dd95e5c6dbafc3bf6ccc705f2f1e87b9a` |
| `nsh-implementation-closure-nsh_workspace_mounted.stdout` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `nsh-implementation-closure-release-python.stderr` | `46b3cfb9caf48c577dcea7ddca0262418deef7deb1540218f389d25858d99fcc` |
| `nsh-implementation-closure-release-python.stdout` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `nsh-implementation-closure-scripts-python.stderr` | `f099b9ab135d3b1c1da59fca59008bad154271f15d90b0290b1745294cc7ebdf` |
| `nsh-implementation-closure-scripts-python.stdout` | `23a41c17f8e5a4ee94ea2832e4182877371a9d4b8faa25cc52993bdf5d2ba98c` |
| `nsh-implementation-closure-vega.stderr` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `nsh-implementation-closure-vega.stdout` | `0b583124b503ee768e8f0e89ffec19e83261c5d195c1a1f4ee91105c7dbf48e6` |
| `nsh-implementation-closure-vega_ui.stderr` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `nsh-implementation-closure-vega_ui.stdout` | `7922cb469bd88a575fec96032570e60275432ae398cc0371919d93fd05fb189f` |
| `nsh-implementation-review-content.json` | `9b5d3ccf193668c12833165b216c842d65d8848bf056a84a7d02b6b32c96d71c` |
| `nsh-implementation-review-tracked.patch` | `6833922e3f600349fe53838137f93ca893de87c2f80a707dfd75c9a0c97eb5ad` |

### Current handoff and uncompleted gates

**This stopped-source local-development cycle has eleven completed green UI targets, app 31 green, mounted workspace 6 green, Python 5/15/40 green, final strict Clippy green and a completed 389-test Nextest run.** Initial predicates, Clippy and Nextest-list failures remain distinct historical attempts; subsequent corrected checks supersede current blockers only within their recorded content/scope. Parent reports final fmt green without a retained final-console logfile; the earlier logged fmt green remains separately identifiable.

Independent reviews are **reported running, conclusions pending**, not approved by this evidence writer. No latest-head required CI, native AX/IME or manual macOS/Windows/Linux acceptance, development-test archival/removal or final zero-test gate, formal release/package, application launch, commit, push, merge, installation or delivery is performed or claimed. The development results do not waive those gates. Reporting continues with **fixed module weights and actual per-module results/limitations, no total estimate or invented overall percentage**.

Only this document is appended. Post-append readback must retain all preceding bytes, validate the added log hashes and compare the frozen listed paths: the authorized document is expected to be the sole changed listed path, while source paths remain unchanged. The frozen whole-content ID is not rewritten to hide the documentation delta. **READY means this evidence appendix is ready after those checks; stop writes and return ownership to parent, not delivery approval.**
