# Shell recovery NSH migration

## Scope and authority

Implementation baseline: `b301b64e00690365087362ba21ec45a96f35400f`, with upstream merge supplied by the parent. Owned writes are `conversation_stream/shell_recovery.rs`, its new child directory, and this document only. No Git mutation, build, application execution, shared focus protocol changes, dependencies, or shell operation execution is authorized. The full migration contract is `nsh-full.md`.

The existing `ShellRecoveryAction`, `ShellRecoveryRequested`, `ShellRecoveryOutcome`, request/finish/refresh controller, sequence checks, source checks, operation identity, submitted decision and background refresh lifetime remain authoritative. NSH receives nonsecret presentation data and opaque observation identities, never `ShellRecoveryConfirmation`, its decision token, an Entity or an Rc lease. The host resolves acceptance against the exact live confirmation only after fresh validation.

## Mounted #916 closure acceptance (current follow-up)

Parent reports that the stopped compatibility source compiled and all 26 recovery policy tests passed, with overlay and four core reset hooks integrated and upstream service/window/locales imported byte-identically. Reported logs: `target/nsh-source-settings-recovery-compile.{stdout,stderr}` and `target/nsh-draft-recovery-policy.{stdout,stderr}`. That evidence does not establish mounted focus, geometry, close or Escape.

This follow-up owns only the existing recovery files/document plus `crates/vega_ui/tests/nsh_recovery_integration.rs`. Parent alone edits module exports and Cargo registration and runs Cargo/rustc. No network, Git mutation, dependencies, backend changes, or capabilities.

Two real-platform mounted development cases must load an actual `ConversationStream` and its production rendering/controller paths. A shared main-thread `TestAppContext` uses actual platform fonts, Kit initialization and `executor.allow_parking()`. Scheduler draining remains bounded by 2 seconds/512 ticks; readiness by 10 seconds. Test-only renderer observations may record actual layout, but must not replace visibility, routing, focus, selection/reveal or activation policy.

- Presentation: a normal unsaved draft has no recovery surfaces; blockers and errors render only the summary until explicit Review. Review is activated through the real keyboard/focus/reveal path, mounts separate details, and retains the composer summary. Assert actual host bounds and NSH layout/control observations, safe/review `FocusHandle` ownership, and host accessibility eligibility. Persisted threads retain inline details without a draft overlay.
- Pending lifecycle: actual UI close and root Escape while Prepare is pending hide details without emitting Resume or releasing the SQLite blocker. Late Prepare confirmations after dismissal or authority change are rejected by the real controller. Submitted Resume remains pending after dismissal, and only its explicit matching service completion may clear the blocker.
- All confirmations come from the real `ShellRecoveryService::prepare` against a unique temporary migrated SQLite database, using public store APIs to create auditable shell records. Never fabricate confirmation fields or decision/security tokens. Requests are captured at the production event boundary; no automatic external service execution or fake successful completion.
- Keep the 26 existing policy tests unchanged. Mounted evidence is development validation only, not native visual/assistive-technology acceptance; archive/remove test-exclusive support before master.

### Mounted blocking diagnosis acceptance (current follow-up)

Parent compiled the mounted source successfully (35.14s), then ran source and recovery sequentially under one 300-second outer timeout. The recovery stdout is empty (0 bytes); its stderr is 5,224 bytes, starts the first case, and ends at `[nsh-runtime] draw=22 surface=11`. There is no fixture-ready marker, PASS/FAIL, or runtime batch BEGIN/END. Parent reports no surviving Cargo/test process. This is an interrupted first case, not a passing run or a proven runtime-batch deadlock; the second case was not reached.

Pinned `gpui-fast 0.1.9` synchronously draws during `App::open_window`, and its test-mode `App::flush_effects` can draw dirty windows before an update returns. `VisualTestContext::update` does not itself call `run_until_parked`; scheduler `tick` executes a runnable synchronously. Consequently the existing 2-second/512-tick loop cannot bound a single callback/draw/update, and the missing fixture-ready marker places the current hang before the explicit fixture frame/drain. Production full-stream construction also creates permission/question listeners and a branch polling task; no evidence yet attributes the hang to these tasks or to SQLite.

Add development-only paired BEGIN/END stages around fixture globals/database/window creation, stream construction, route setup, root rendering, activation, update callbacks, individual scheduler ticks, draw/clear, service operations and case/subtest boundaries. Include elapsed/unwinding information and stop admitting new diagnostic stages after 1,024 pairs per process; already admitted stages still emit END. No production behavior, assertion, controller/security fence, timer or readiness/drain budget changes. No fake service completion or dependency/backend patch.

Parent should compile without running, then execute only this recovery binary once under an external watchdog, taking an OS stack sample if still running at 10 seconds and terminating its process group at 25 seconds. The watchdog bounds observation, not an increased test readiness budget. Preserve both diagnostic logs and the sample. Match the last unmatched stage with the main-thread stack to distinguish initial draw/effect flush, runnable/dispatcher locking, SQLite/service work or teardown before choosing a repair. Do not blindly repeat the combined 300-second command. This lane does not execute Cargo/rustc/tests or sampling; runtime/native acceptance remains pending.

Diagnostic implementation stopped with only `shell_recovery/mounted_regressions.rs` and this document changed in this follow-up. Actual static checks passed: installed 1.98.0 rustfmt format/check; scoped `git diff --check`; Python whitespace/newline/conflict/comment/lint-suppression checks; original 10s/2s/512-tick constants and two public cases retained; no confirmation struct literals; unchanged SHA-256 for `shell_recovery.rs`, `nsh_view.rs`, `tests.rs`, `reveal.rs` and the recovery runner. The watchdog script was parsed with Python `ast.parse`, not executed. Diagnostic Rust SHA-256: `c77184684377f91199483d1345f8404f84117efeabf5bc60e4939822a03c3c3b`. No Cargo/rustc/test/sample/network/Git mutation was performed by this lane. At that diagnostic handoff compilation and mounted execution were pending; the subsequent parent execution is recorded below. No concrete product repair or runtime/native acceptance is claimed.

#### Confirmed parent diagnostic result and recovery freeze

Parent reports successful diagnostic compilation in 34.13s, followed by exactly one recovery-only watchdog execution: sampling at 10 seconds, termination at 25 seconds, `watchdog_timeout=True child_exit=-9 elapsed=25.006s`, `sample_exit=0`, and no active process afterward. Inspected artifacts are `target/nsh-recovery-diagnostic.{stdout,stderr,sample.txt,sample-command.log}`; stdout is empty and stderr is 6,657 bytes. The sample-command log confirms successful sampling. The watchdog exit/timing and process cleanup are parent-reported, not a new execution by this lane.

- First case: `RUN draft_summary_opens_separate_overlay_with_real_focus`; no PASS/FAIL or completed runner counts. `new.open_window` BEGIN (`id=8`) has no END. Its `new.open_window.build_root` (`id=9`), `Root::render.build` (`id=12`) and `service.request.capture` (`id=13`) all have matching END records without unwinding. Capturing a request is not completing service work. The second case was not reached.
- Main-thread sample: 777 samples under `App::open_window`; `flush_effects` accounts for 557 and `apply_notify_effect` for 537. The sampled path includes `App::observe_internal<ConversationStream>` into `composer_selectors::Host`'s observer created by `Native::render` (210), then the observer closure, `Context<Host>::notify` (133) and `App::notify`. These are sampled stack counts, not elapsed times or completed notification counts.
- Interpretation: initial synchronous `open_window` effect flushing is occupied by a composer notification feedback loop. This identifies the responsible production module, rather than a blocked recovery drain, service call or dispatcher lock. Completed root construction/render and request capture do not establish completed child layout/paint or any recovery acceptance assertion. No further composer investigation or implementation is delegated to this recovery lane; parent assigns that repair to its separate composer author.

Recovery code, runner, diagnostics, assertions and original 2s/512-tick/10s budgets remain frozen until the composer repair is tested. Parent then runs the original two recovery cases and returns concrete runtime failures if any; no additional diagnostic execution is requested here. Mounted summary/details geometry, focus, keyboard, close/Escape, authority/service lifecycle and native acceptance remain pending, not passed. This is an implementation handoff, not independent review.

The watchdog below only writes its named diagnostic artifacts, samples the child, terminates the child's process group and reaps it. It contains no filesystem removal or temporary-directory cleanup. A SIGKILL does not run fixture destructors; parent reports temporary SQLite directories were left behind. Preserve those directories and any unknown paths: do not infer ownership from a name/prefix, sweep temporary directories or add cleanup to the watchdog. No directory or SQLite file was removed by this lane.

#### Resumed recovery: real GPUI bounds selector repair (current handoff)

This follow-up supersedes the recovery freeze above only for the returned bounds failure. HEAD remains `0b5ea426abfd8d4358489e916273d89474fb190f`. Parent reports composer repair `04d769bd` with 6/6 policy tests green; that abbreviated revision/result is parent evidence, not a composer change or execution by this lane. Parent reports the single resumed recovery execution exited 1 in 3.195s. Inspected `target/nsh-resumed-recovery-cycle.stdout` contains exactly:

```text
recovery integration: 0 passed; 1 failed; 1 not executed; 0 ignored
```

Inspected stderr contains `new.open_window` END at 100.191042ms and `wait summary NSH layout` END at 65.009792ms. The subsequent failure is:

```text
thread 'main' (29184767) panicked at crates/vega_ui/src/conversation_stream/shell_recovery/mounted_regressions.rs:558:14:
called `Option::unwrap()` on a `None` value
FAIL draft_summary_opens_separate_overlay_with_real_focus
```

The real NSH summary panel was found with positive bounds before the failing `.debug_bounds("shell-recovery-summary-native-host").unwrap()`. This is not the old composer notification loop. Pinned local `gpui-fast 0.1.9` establishes the observation contract: `src/app/test_context.rs:1017-1022` reads only `rendered_frame.debug_bounds` by debug selector; `src/elements/div.rs:879-900` provides `InteractiveElement::debug_selector(FnOnce() -> String)`, and `:2557-2561` records actual element bounds during paint. Ordinary `.id(...)` does not register that selector. Recording requires GPUI tests or `debug_assertions` plus `test-support`; elsewhere the API is a no-op. Parent must retain a debug/test-support validation build, not substitute release bounds or fake geometry.

Minimal responsible-layer delta:

- `nsh_view.rs`: add `.debug_selector(|| "shell-recovery-summary-native-host".to_owned())` on the retained summary wrapper used while details are open; derive the existing leaf host string once, preserve `.id(host)`, and add `.debug_selector(move || host.to_owned())` on the actual summary/details host.
- `mounted_regressions.rs`: retain `.id("mounted-recovery-root")` and add `.debug_selector(|| "mounted-recovery-root".to_owned())` to the real full-stream fixture root.
- No assertion or expectation changes, surrogate mount, injected bounds, readiness retry, or behavioral/controller fix. Exact pre-edit reconstruction hashes prove every other Rust byte unchanged. All authority, close/Escape, pending Resume, reveal, AX eligibility and security assertions, real `ConversationStream`, temporary SQLite store/service preparation and request capture remain intact. Existing 26 policy tests (19 + 7) are byte-identical. Both mounted cases and 2s/512-tick/10s budgets remain unchanged. No source-anchor, parent-owned, dependency or backend changes; no temporary paths/database cleanup.

Actually executed static checks: installed `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)` from toolchain directory `1.98.0-aarch64-apple-darwin`, `--edition 2024 --config skip_children=true --check` on all six owned Rust files; scoped `git diff --check`; Python exact pre-edit reconstruction, 26-policy/two-mounted-case inventory, fixed-budget, trailing-whitespace/newline/conflict/comment/lint-suppression checks. All passed. The initial Python hygiene expression incorrectly treated Rust dereference lines (`*pending`, `*sequence`, `*self`) as comment starts; inspected those matches and corrected the checker without editing source or weakening assertions. Cargo/rustc/toolchain-version queries, compilation/tests, application/sample execution, network and Git mutation were not performed by this lane. Current mounted result remains 0 passed/1 failed/1 not executed from the pre-fix run, not a passing validation of this delta. This is author implementation, not independent review.

Exact SHA-256 (before → stopped source):

```text
ebcc6eb0ed52579adf4e86418b866f368e0685fd2c36b2900a191707c7cf76b8 -> b0d36bf87e0865819c5042277f382739e831c623fed90d77c1582a098594affb  shell_recovery/nsh_view.rs
c77184684377f91199483d1345f8404f84117efeabf5bc60e4939822a03c3c3b -> 751bc4307f70ca504161c066b07f06f3d572d84254ce7adfd679c6b7cecabadc  shell_recovery/mounted_regressions.rs
df104c45e6915dce6b2b48d92f3d5e43004ee024d4b3f95c8720a57bff687a1b  shell_recovery.rs (unchanged)
7cb931e71512a4d165a407a6522f65019d566a537396e1f59fe16ff26696b08c  shell_recovery/keyboard.rs (unchanged)
5761f0304ec55960a9c89653319e9918fcc417ddc7dac601197eb6ef15d46e87  shell_recovery/reveal.rs (unchanged)
cd54dbdd7385e7df588aab707b57dbaea8b0f40175a33d7663f25c94c2dfcf66  shell_recovery/tests.rs (unchanged)
f833de9a98f9f91b2602e4c3d7df46968953a7b55b423c8ca0c95d3c9d63f3f5  target/nsh-resumed-recovery-cycle.stdout (unchanged)
780c8f47b0686d9419f6831ec5e0d0e176b4ac939e2f89cb4865700ebcf2916c  target/nsh-resumed-recovery-cycle.stderr (unchanged)
```

Parent next step: compile the exact stopped source without running using the existing offline/locked `nsh_recovery_integration` no-run command below; then execute its newly emitted debug artifact once under the existing 10s-sample/25s-kill watchdog, using new named logs rather than overwriting the raw resumed failure. Return exact source/artifact hashes, exit/timing, both case counts and any concrete next failing assertion. Run/preserve all 26 recovery policy tests on this delta as parent-coordinated validation. No claim yet for mounted geometry, focus, pending lifecycle, native/OS AX acceptance, CI or merge; do not relax assertions or expand budgets if the next run exposes a separate failure.

#### Resumed recovery: confirmation keyboard traversal acceptance (current follow-up)

Parent reports the selector patch compiled successfully in 28.87s, then one bounded recovery execution exited 1 in 4.482s without timeout. `target/nsh-resumed-observation-build.json` reports `build-finished.success=true`; `target/nsh-resumed-recovery-selectors.stdout` records the first case PASS and `1 passed; 1 failed; 0 not executed; 0 ignored`. The second case fails at `mounted_regressions.rs:494:9`: `enabled control not reachable by Tab: Accept`. These results validate the previous selector repair, not this follow-up. Parent separately reports source 14/14 green; that lane is unchanged here.

Acceptance remains real full-production rendering, SQLite preparation/security tokens, host focus, selection/reveal and AX eligibility, keyboard reachability, close/Escape, late-Prepare/authority rejection and pending Resume fences, with unchanged 2s/512-tick/10s budgets and all existing 26 policy tests. Correct a fixture direction error only if the actual enabled-control order and native key API establish it; do not wrap production panel boundaries, select Accept directly, bypass reveal/AX, fabricate confirmation/bounds, change the safe initial selection or loosen assertions. Verify the final matching Prepare completion explicitly before keyboard activation. Preserve the first passing case and all debug selectors. Parent alone compiles and executes the resulting source.

Diagnosis and responsible layer:

- Read the entire second case, fixture helpers, real controller completion, `Snapshot::controls`/`initial_selection`/`next_selection`, host keyboard routing, reveal acknowledgment and AX eligibility. The raw second-case trace has **153 BEGIN/153 END records, no unmatched IDs**. `new.open_window` returns in 12.281875ms; `Fixture::new` returns in 310.035708ms; case END is 1.0226845s with unwinding. The shared 1,024-stage cap is exhausted at `fixture.update.callback` during initial `complete_read`; consequently the raw trace does **not** show the subsequent confirmation/Tab state. No missing END proves a hang, and no later snapshot state is claimed as directly logged.
- The matching `finish_shell_recovery` clears pending and installs the real service confirmation; rendering invalidates stale selection and intentionally initializes confirmation at **KeepPaused**, never Accept. Confirmed draft enabled order is **NotNow, Accept, KeepPaused**. `next_selection` is nonwrapping; forward from KeepPaused returns `None`. Existing boundary-policy tests require native focus-order exit, not a panel wrap. The old fixture only dispatched forward Tab twelve times, so it could not move backward from the safe default to Accept. This is a fixture routing defect; there is no evidence justifying a production focus-policy change or a sleep/budget increase.
- Actual pinned `gpui-fast 0.1.9` API: `src/platform/keystroke.rs:120–138` parses `shift-tab` as key `tab` plus `modifiers.shift`; `src/window.rs:5603–5625` sends a real non-held KeyDown through `dispatch_event`. Production `shell_key` forwards `modifiers.shift` to `next_selection`, performs `shell_select`, then requires fresh displayed/selection authority before activation. Native `focus_next`/`focus_prev` at `window.rs:2470–2487` use real rendered tab stops. No fake focus or direct business-action dispatch is needed.

Implemented delta (only `mounted_regressions.rs` plus this document):

- Retain the same activation loop, twelve-step cap, real `Window::dispatch_keystroke`, per-step reveal/AX wait and Enter activation. Read the **current** snapshot's enabled control order; assert both target and acknowledged selection are enabled; dispatch Shift-Tab if the target precedes the selected control, otherwise Tab. No direct selection/focus assignment to Accept, wrapping, disabled-control skipping beyond production eligibility or authority override. An at-most-twelve-line per-activation diagnostic records target, selected/enabled actions, confirmation/pending/submitted and actual focus, independently of the exhausted paired-stage counter. It logs no tokens or confirmation contents.
- Extend the existing second-case regression immediately after the final Prepare completion: require the exact sequence, no pending/error, a live confirmation and not-submitted state; then wait for the real acknowledged **KeepPaused** selection before navigating to Accept. This distinguishes rejected/too-early service completion from actual keyboard/reveal failure. All original assertions remain verbatim. All production Rust, the full first-case body and the 26 policy tests are byte-identical to the selector-patch baseline. The real full `ConversationStream`, unique temporary SQLite database/service and request-capture seam, security and late-completion fences remain unchanged. No parent-owned source anchors, Cargo, backend, composer, cleanup or unknown paths were changed.

Actually executed static checks: direct installed `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`, `--edition 2024 --config skip_children=true --check` on all six owned Rust files; scoped `git diff --check`; Python exact two-hunk pre-edit reconstruction, unchanged-five-Rust-file hashes, 26-policy/two-mounted-case inventory, unchanged 2s/512-tick/10s/twelve-step budgets, selector retention, trailing-whitespace/newline/conflict/comment/lint-suppression checks and four raw-evidence hash checks. All passed. No Cargo/rustc, build/test/application execution, network, Git mutation or filesystem cleanup was performed. The returned **1 PASS/1 FAIL** remains the last runtime result; this correction is not yet compiled or executed and is not independent review.

SHA-256 before → stopped Rust and retained raw evidence:

```text
751bc4307f70ca504161c066b07f06f3d572d84254ce7adfd679c6b7cecabadc -> a80aaf94e66c8956f970b556bbf48ab246bc6206cc9078ac255a5c9ccc81eff5  shell_recovery/mounted_regressions.rs
df104c45e6915dce6b2b48d92f3d5e43004ee024d4b3f95c8720a57bff687a1b  shell_recovery.rs (unchanged)
7cb931e71512a4d165a407a6522f65019d566a537396e1f59fe16ff26696b08c  shell_recovery/keyboard.rs (unchanged)
b0d36bf87e0865819c5042277f382739e831c623fed90d77c1582a098594affb  shell_recovery/nsh_view.rs (unchanged, selectors retained)
5761f0304ec55960a9c89653319e9918fcc417ddc7dac601197eb6ef15d46e87  shell_recovery/reveal.rs (unchanged)
cd54dbdd7385e7df588aab707b57dbaea8b0f40175a33d7663f25c94c2dfcf66  shell_recovery/tests.rs (unchanged, 19 + 7)
d9c63ada6601e69863b33c251017463f1485c39fd1d3d95438289fc92d44b286  docs/907/nsh-shell-recovery.md (before this follow-up)
66af43b7b4942e59d3f48463d698218e9e93faa8275c8025bbd82b46b0ae3aaf  target/nsh-resumed-observation-build.json
15f5d180e1a159c68d974b33292f69bdbf1865601b6984b0bc8771e1a94ff1cf  target/nsh-resumed-observation-build.stderr
d3f22a49ec7a48eede47eafe46491bd7e07db40fcb4b47a1820e295c2d918856  target/nsh-resumed-recovery-selectors.stdout
789b7189dec026b7ab48a1a4ceb78a6f1c9408b13e04c33f5824bce0575b34ee  target/nsh-resumed-recovery-selectors.stderr
```

Parent next test: compile this exact stopped source with the established offline/locked no-run command, writing new named build artifacts (for example `target/nsh-resumed-traversal-build.{json,stderr}`). Execute the newly emitted debug/test-support binary **once** under the unchanged 10s-sample/25s-kill watchdog with new logs such as `target/nsh-resumed-recovery-traversal.{stdout,stderr}`. Require first-case PASS preserved and second-case PASS, all 26 existing policy tests green, and the original budgets. The new traversal trace must show live-confirmation/no-pending/not-submitted/focused state and actual keyboard movement from KeepPaused to Accept; Enter must capture Resume and the existing Escape/pending/service-completion assertions must then pass. If any assertion fails, return its raw trace, artifact/source hashes, exit/timing and both executed/unrun counts without weakening expectations or retrying unchanged. No native OS AX/visual acceptance, CI or merge is claimed. Writes stop at this handoff.

#### Resumed recovery: actual green compiled snapshot (documentation-only follow-up)

This result supersedes the pending compile/run status of the confirmation traversal handoff above, not its historical failure evidence. Only this document is writable in this follow-up; all Rust is frozen. Source baseline remains parent-supplied HEAD `0b5ea426abfd8d4358489e916273d89474fb190f` plus the stopped content hashes above, including `mounted_regressions.rs` SHA-256 `a80aaf94e66c8956f970b556bbf48ab246bc6206cc9078ac255a5c9ccc81eff5`. The six owned Rust hashes were rechecked unchanged. No Cargo/rustc invocation, Git command/mutation, network, runtime rerun or cleanup occurred here.

Parent execution and inspected raw evidence:

- **Build:** `target/nsh-resumed-recovery-navigation-build.{json,stderr}` records `build-finished.success=true` and completion in **27.24s**; parent reports exit **0**. The JSON identifies the actual debug/test-support recovery executable and the same-build policy-library executable below. Toolchain/command context remains the established parent-owned 1.98.0 offline/locked validation build; no fresh Cargo/rustc version probe was performed here.
- **Mounted execution, once:** `target/nsh-resumed-recovery-navigation.stdout` records PASS for `draft_summary_opens_separate_overlay_with_real_focus` and `pending_draft_close_escape_and_authority_keep_recovery_fenced`: **2 passed / 0 failed / 0 not executed / 0 ignored**. Parent reports exit **0**, **4.549s**, `watchdog_timeout=false`; exit/timing/watchdog are parent-reported observer results, not fields in the case stdout.
- **Actual traversal:** stderr line 2242 records `target=Accept selected=Some(KeepPaused) enabled=[NotNow, Accept, KeepPaused] confirmation=true pending=None submitted=false focused=true`. Parent confirms KeepPaused → Accept via **Shift-Tab**. The passing second case completes the real Resume capture, Escape/pending and matching service-completion assertions; its final paired case END at line 2246 reports `unwinding=false`. The trace does not separately print a post-key Accept selection, so that result is established by the passing unchanged reachability/activation assertions, not an invented trace entry.
- **Same-build policy execution:** `target/nsh-resumed-recovery-navigation-policy.stdout` records **26 passed / 0 failed / 0 ignored / 0 measured**, **321 filtered out**, **0.01s**, preserving the existing **19 + 7** set. Policy stderr is empty. Filtered-out tests are not executed passes.

Both mounted cases retain the full production `ConversationStream`, unique temporary migrated SQLite stores, real Prepare/security tokens, original authority/late-Prepare/close/Escape/pending-Resume/reveal/AX assertions and unchanged **2s / 512 ticks / 10s** budgets. The selector repair and first-case PASS remain intact; no assertions, budgets, production policy or Rust were changed to record this result.

Exact SHA-256 of emitted executables and successful raw evidence:

```text
a02cc99634fe124588946b886f1913e70679528a75bcb2a66c59fe6406f01569  target/official-nsh-validation/debug/deps/nsh_recovery_integration-9271aa7368d76ca1
79d6f70f3986e3cd748cbe37582d0350db16953c149ac7e5eb6092d166b12e2c  target/official-nsh-validation/debug/deps/vega_ui-4268349f2e74c4f3
aecb28e7f9e012e9a516912e6ed1aa2ebf3b29da02f140ca1c2cc26be129c7be  target/nsh-resumed-recovery-navigation-build.json
c624865cf50993a02159888b2f083dcf74ef1abed274494bc903ef1a16b4af87  target/nsh-resumed-recovery-navigation-build.stderr
d6a0c06e9d1de2519595bc6a48e6c466141237064ca96d4485408e221e6fb4a3  target/nsh-resumed-recovery-navigation.stdout
644ba531d376dd7848cb84831178d2701661cab888a56c098b0d3c2bafe8b1a0  target/nsh-resumed-recovery-navigation.stderr
a2e9a11943ff70949034e04160e5a3bcf6410c5b061613da9e975b09815e7fa3  target/nsh-resumed-recovery-navigation-policy.stdout
e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  target/nsh-resumed-recovery-navigation-policy.stderr
b3fab2df0de8557eed287a811804b0cd89c8deaf4219700d32283a242893701c  docs/907/nsh-shell-recovery.md (before this documentation-only follow-up)
```

The earlier composer notify-loop watchdog, missing GPUI debug-selector bounds failure and forward-only Tab failure remain historical evidence, not erased or relabeled as passes. The four selector build/failure artifact hashes above were rechecked unchanged. The earlier `target/nsh-resumed-recovery-cycle.stdout` remains SHA-256 `f833de9a98f9f91b2602e4c3d7df46968953a7b55b423c8ca0c95d3c9d63f3f5`, and its stderr remains `780c8f47b0686d9419f6831ec5e0d0e176b4ac939e2f89cb4865700ebcf2916c`.

**Boundary and parent next step:** this is green development validation of the exact compiled snapshot, **not native visual/OS AX acceptance, independent review or final-head CI**. Parent reports current strict workspace Clippy has **nine unused errors in separately owned workspace development fixtures**; that workspace issue is not a recovery failure and no workspace-wide Clippy pass is claimed. Parent retains that separate ownership and the remaining review/native/final-head CI and development-test archival/removal gates. Do not rerun these unchanged green recovery cases merely for this documentation update or treat this record as authorization to change other lanes. Stop documentation writes after readback/static checks and report the final document hash; unknown paths and temporary databases remain untouched.

#### Historical single diagnostic execution (executed once by parent; do not rerun unchanged)

Compile separately without executing any cases; use the emitted artifact, not a guessed/possibly stale binary hash:

```sh
CARGO_FORK_AUTO_UPDATE=0 rustup run 1.98.0 cargo test -p vega_ui --features nsh-reading-integration --test nsh_recovery_integration --offline --locked -j 2 --target-dir target/official-nsh-validation --no-run --message-format=json > target/nsh-recovery-diagnostic-build.json 2> target/nsh-recovery-diagnostic-build.stderr
```

Only after successful compilation, run this once from the worktree root under an outer tool timeout of 40 seconds or longer. The child binary remains on its process main thread, without shell/Cargo intermediary; the external Python observer does not enter the GPUI context. Sampling uses the existing macOS `sample` command without elevation or capability changes. Sampling failure is evidence, not a reason to rerun; the watchdog still terminates/reaps the child.

```sh
python3 - <<'PY'
import json
import os
from pathlib import Path
import signal
import subprocess
import time

artifacts = []
for line in Path('target/nsh-recovery-diagnostic-build.json').read_text().splitlines():
    message = json.loads(line)
    if (message.get('reason') == 'compiler-artifact'
            and message['target']['name'] == 'nsh_recovery_integration'
            and message.get('executable')):
        artifacts.append(message['executable'])
assert len(artifacts) == 1, artifacts
prefix = Path('target/nsh-recovery-diagnostic')
with open(str(prefix) + '.stdout', 'wb') as stdout, open(str(prefix) + '.stderr', 'wb') as stderr:
    started = time.monotonic()
    process = subprocess.Popen([artifacts[0]], stdout=stdout, stderr=stderr, start_new_session=True)
    timed_out = False
    try:
        try:
            process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            with open(str(prefix) + '.sample-command.log', 'wb') as sample_log:
                try:
                    result = subprocess.run(
                        ['/usr/bin/sample', str(process.pid), '2', '1', '-file', str(prefix) + '.sample.txt'],
                        stdout=sample_log, stderr=subprocess.STDOUT, timeout=5,
                    )
                    sample_log.write(f'\nsample_exit={result.returncode}\n'.encode())
                except (OSError, subprocess.TimeoutExpired) as error:
                    sample_log.write(f'\nsample_error={error}\n'.encode())
            try:
                process.wait(timeout=max(0, 25 - (time.monotonic() - started)))
            except subprocess.TimeoutExpired:
                timed_out = True
    finally:
        if process.poll() is None:
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
        process.wait()
    print(f'watchdog_timeout={timed_out} child_exit={process.returncode} elapsed={time.monotonic() - started:.3f}s')
    if timed_out:
        print('INTERRUPTED: no synthetic PASS/FAIL counts; inspect paired stages and sample')
    raise SystemExit(124 if timed_out else process.returncode)
PY
```

Observation routing: `new.open_window.build_root` END with `new.open_window` unmatched and `Root::render.build` END puts the remaining work in synchronous child layout/paint/arena-clear or effect flush, not the explicit drain. A callback END with its outer update unmatched points after callback return to GPUI effects/draw. `drain.pending_task_counts` unmatched identifies queue-lock observation; `drain.tick` unmatched identifies one runnable/tick, and the OS stack distinguishes lock wait, service work or callback. `service.read/prepare/resume` unmatched isolates the real SQLite/service call. No runtime batch BEGIN means the runtime queue consumer has not been observed executing a batch; it does not prove the batch is stuck. Stage logging is capped; if the cap is reached use the sample and existing active outer stages rather than assuming the last completed stage is the blocker.

### Mounted implementation and parent wiring (prior stopped source)

Authored cases are `draft_summary_opens_separate_overlay_with_real_focus` and `pending_draft_close_escape_and_authority_keep_recovery_fenced`, both `pub fn(&mut gpui_kit::TestAppContext)` in the owned `shell_recovery::mounted_regressions` re-export. They mount the entire real `ConversationStream`, not a renderer mirror. Renderer hooks are feature-gated passive NSH panel/control layout observations only; controller/security/render decisions are unchanged. Review, Prepare, close and Accept are traversed using actual focus, dispatched Tab/Enter and production bounded reveal acknowledgment; Escape is dispatched to the full production root. Host AX eligibility is asserted using the actual `shell_accessibility` path, not an OS AX-tree claim. Overlay bounds are checked against the actual mounted root center (2px tolerance); persisted inline details must remain below its center. SQLite stores, workspaces and persisted audit rows are unique per case. Only test service requests are intercepted, and only explicit real read/prepare/resume methods produce successful results. The synthetic Unavailable result models an isolated read failure, not successful service work.

Parent must add only this development export in `conversation_stream/mod.rs` (not edited by this lane):

```rust
#[cfg(feature = "nsh-reading-integration")]
pub use shell_recovery::mounted_regressions as shell_recovery_mounted_regressions;
```

And register the new runner in `crates/vega_ui/Cargo.toml` (not edited by this lane):

```toml
[[test]]
name = "nsh_recovery_integration"
harness = false
required-features = ["nsh-reading-integration"]
```

Exact proposed parent execution after wiring:

```sh
CARGO_FORK_AUTO_UPDATE=0 rustup run 1.98.0 cargo test -p vega_ui --features nsh-reading-integration --test nsh_recovery_integration --offline --locked -j 2 --target-dir target/official-nsh-validation
```

The runner shares one actual-platform-font main-thread context, initializes Kit, permits parking, stops after the first failing case and reports passed/failed/not-executed/ignored counts. **Current mounted inventory: 2 authored, 0 executed, 2 not executed here; no pass/runtime claim.** Parent owns registration, compile/run and concrete failure feedback. Parent-reported earlier 26/26 policy results do not validate this new feature wiring or mounted code.

Current-cycle static checks actually executed: installed 1.98.0 `rustfmt --edition 2024 --config skip_children=true --check` on the four changed Rust files passed after formatting; scoped `git diff --check` passed; Python trailing-whitespace/conflict/comment/lint-suppression checks passed; existing `tests.rs` and `reveal.rs` SHA-256 remain identical to the stopped prior stage. Two public mounted cases and absence of confirmation struct literals were checked. Initial rustfmt check identified formatting only and was corrected; an initial atomic edit rejected a nonunique context without changing the file, then unique contexts were used. No Cargo/rustc/test/app/network/Git mutation, independent review, native or OS AX acceptance. Mounted compilation, real focus/keyboard/geometry and service lifecycle remain pending parent execution; native appearance, assistive technology and fault fallback remain separate acceptance gaps. This follow-up adds no dependencies, backend patches or capabilities and touches no Cargo/core/render/mod exports, icons, settings or other implementation lanes.

Stopped Rust SHA-256:

```text
df104c45e6915dce6b2b48d92f3d5e43004ee024d4b3f95c8720a57bff687a1b  shell_recovery.rs
ebcc6eb0ed52579adf4e86418b866f368e0685fd2c36b2900a191707c7cf76b8  shell_recovery/nsh_view.rs
a0538ffc6494261d53ddf0a6bbd18cef94c3d76907d128b5103c31e7192dc3d6  shell_recovery/mounted_regressions.rs
1c7bb60deb24323afc88bf19175e99961128c5c8c2b959c834ee7669eaf5337b  crates/vega_ui/tests/nsh_recovery_integration.rs
```

## #916 mainline compatibility acceptance (implementation follow-up)

Current worktree HEAD: `0b5ea426abfd8d4358489e916273d89474fb190f`; upstream contract: `f3688f5ae0b7becf1f46434c7b7d732e3a3c759a`, specifically its shell recovery controller diff and `docs/915/draft-shell-recovery.md`. This section supersedes historical statements below that the original controller is unchanged. This is production integration, not independent review. Writes remain restricted to this document, `shell_recovery.rs` and its children. Parent owns Git integration, core/render/window/service/locales, all Cargo/rustc execution and acceptance results. No dependency or capability changes.

| Scenario | Required compatibility behavior |
| --- | --- |
| Normal unsaved draft, including active-only projection | No recovery text or permanent recovery panel. Do not fabricate a successful read or create a persisted thread. |
| Draft with real blockers or read error | Composer area shows only the brief translated paused/status-unavailable label and discoverable Review; unavailable takes precedence. No workspace, command, pending detail or confirmation copy inline. |
| Explicit Review on draft | Open a separate overlay, focus its safe control, and call the existing refresh path. All visible overlay pixels and controls are NSH; GPUI provides invisible layout/focus/event/AX hosting only. |
| Close/Escape during Prepare or another unsubmitted request | Hide details, discard unsubmitted confirmation, increment sequence and clear pending even while Prepare is in flight; late response is rejected. Existing blockers/projection remain, not released. |
| Close after submitted Resume | Hide details without claiming cancellation or clearing the submitted decision/pending request; its matching completion remains eligible. |
| Request/reply route | Requests carry `draft`; completion requires identical thread/project, draft route, sequence and all upstream recovery authority fields: backend, mode, permission mode/revision, draft authority epoch, default permission and full-access grant. |
| Authority/route reset | Recreate recovery state with monotonically advanced sequence, fresh NSH identities and deferred refresh; parent calls this on upstream authority updates, draft route changes, backend changes and draft project rebind. |
| Queued NSH input | Preserve all existing surface/observation/projection/confirmation/selection/reveal fences and additionally reject changed route/authority before parent reset is rendered. Confirmation objects/tokens never cross the engine boundary. |
| Persisted thread | Existing NSH active/status/detail/check/retry/prepare/confirmation behavior remains inline; no draft overlay. |
| Keyboard, AX, faults | Retain ordered enabled controls, safe initial selection, actual host focus, bounded reveal acknowledgment and independent NSH fault/reload behavior; draft overlay has an enabled close path even during pending work. |

Development tests must exercise these production decisions and controller authority functions rather than mirrored policies. They are authored only, not executed by this lane. Parent must compile/run and perform native/AX/focus validation; development Rust tests and exclusive wiring still require archival/removal before master under repository policy.

### Implemented compatibility and parent integration

- `ShellRecoveryRequested` now carries `draft`. `matches_route`, `same_recovery_authority`, `recovery_presentation`, `RecoveryDecision::dismiss` and `reset_shell_recovery` match upstream function bodies after whitespace normalization. Request emission and reply acceptance use the new route. Existing observation/projection/confirmation fences, command-card updates and one-second refresh scheduling remain. Dismiss also rotates NSH observation/confirmation identities; reset recreates every NSH identity.
- Draft composer summary and details use separate NSH Surfaces with distinct IDs, sharing the authoritative recovery controller. Normal/active-only drafts mount neither. The summary renders only the upstream translated short status and Review (plus explicit UI Retry on NSH fault); no details, pending copy or confirmation tokens. While details are open, the existing brief summary remains beneath the overlay with Review disabled. Only the active presentation handles keyboard/AX/business input.
- Details are mounted only through the explicit overlay method. NSH draws its card, border, background, all copy and controls. GPUI's deferred priority-3 overlay provides invisible placement, occlusion and event isolation only, embedding the actual NSH Surface directly. Draft close is first in displayed/traversal order and remains enabled while pending; persisted-thread control order/availability is unchanged. Existing bounded NSH reveal/selection acknowledgment and native host focus remain. Hidden preview, permission-busy or non-Native states do not leave an invisible intercepting overlay.
- Full Thread authority is retained only in the host-side Presentation, never in the NSH Snapshot/Request. Snapshot eligibility compares that host authority against the live route before accepting input; route/stage fields add to, not replace, existing opaque fences. No confirmation object, grant or authority token enters the engine.
- `hide_shell_recovery_surface()` retires both display instances without canceling controller work. UI Retry retires only the active instance. Unsubmitted close invalidates pending work and late completion; submitted Resume is merely hidden and can still complete against strict current authority.

Required parent call (window-aware, mutable, unlike the legacy upstream signature):

```rust
pub(in crate::conversation_stream) fn render_shell_recovery_overlay(
    &mut self,
    window: &mut Window,
    cx: &mut Context<Self>,
) -> gpui_kit::AnyElement
```

At the final `ConversationStream` root overlay assembly, alongside the existing pending menu and inside `!self.reading_preview`, add:

```rust
.child(self.render_shell_recovery_overlay(window, cx))
```

Keep the existing composer `.child(self.render_shell_recovery(window, cx))` call and existing root Escape dispatch to `dismiss_shell_recovery(window, cx)`. The new overlay method self-gates draft/opened/native/eligibility; do not embed legacy recovery elements or add GPUI-visible card styles. Parent must retain upstream `same_recovery_authority`/`reset_shell_recovery` calls in apply-thread authority updates, `set_draft_route`, `select_task_backend` and `rebind_draft_project`. These four core hook sites were observed in parent-owned source during handoff; the overlay call was not yet observed. Parent also owns service/window draft routing, authoritative resume revalidation and upstream locale integration (`ui.shell_recovery.status_unavailable`); none is implemented or validated by this lane.

### Current-cycle validation and stopped-source evidence

Actually executed: direct installed Rust 1.98.0 `rustfmt --edition 2024 --check` on the five owned Rust files, exit 0; scoped `git diff --check`, exit 0; Python static comparisons of the five upstream functions above, all 19 pre-existing test function bodies against HEAD, unchanged reveal SHA-256, and owned source comment/lint-suppression/production unwrap checks, all passed. These are static checks, not compiled API or runtime evidence. No Cargo/rustc, tests, network, Git mutation, application/native execution or independent review was performed. Icons/Cargo files were left stopped and untouched. HEAD remains `0b5ea426abfd8d4358489e916273d89474fb190f`.

Development inventory: **26 tests = 19 unchanged + 7 new; 0 executed by this lane**. New cases exercise production draft visibility/summary/detail controls, stale cross-presentation/route input, every upstream authority field, unsubmitted invalidation and submitted completion preservation. No budgets or existing assertions were weakened. Exact parent command proposed after render integration (parent coordinates execution/toolchain/logs):

```sh
CARGO_FORK_AUTO_UPDATE=0 rustup run 1.98.0 cargo test -p vega_ui --lib --features nsh-reading-integration conversation_stream::shell_recovery:: --offline --locked -j 2 --target-dir target/official-nsh-validation
```

Parent must verify discovered/executed counts rather than treating an empty filter as success. A coordinated production UI check/Clippy and broader regressions remain parent-selected requirements. Runtime/native draft overlay layout, scrolling, keyboard/focus/Escape, actual AX, fault fallback, real service/window authority and full upstream integration are **pending**, not accepted. Existing historical validation/review sections below do not validate this delta. Development tests/exclusive wiring must still be archived and removed before master.

Stopped Rust SHA-256:

```text
7ba8fc997bf6ea1e79b29747e03a5194b5db7e1abdaa5121bb1b86b5cb8c473a  shell_recovery.rs
d395bcbcf54bcb1d3cb23ef1260bcd02f737cd4346910af692040d97710e44ac  shell_recovery/nsh_view.rs
7cb931e71512a4d165a407a6522f65019d566a537396e1f59fe16ff26696b08c  shell_recovery/keyboard.rs
5761f0304ec55960a9c89653319e9918fcc417ddc7dac601197eb6ef15d46e87  shell_recovery/reveal.rs (unchanged)
cd54dbdd7385e7df588aab707b57dbaea8b0f40175a33d7663f25c94c2dfcf66  shell_recovery/tests.rs
```

## Acceptance matrix (before implementation)

| Scenario | Required observable behavior / rejection |
| --- | --- |
| Active commands | Preserve each original status label; Cancel enabled only for that displayed running active operation and while not pending. |
| Paused collapsed | Preserve danger/paused status and Review. Review opens the original controller and calls its original refresh path. |
| Open projection | Preserve workspace, every command or numbered empty-command fallback, exact exit/unknown result, translated deduplicated reason mapping, incomplete-output and unsettled warnings. |
| Check / retry / prepare | Dispatch the original action with the current controller projection; preserve original per-row availability; pending rejects all business actions. |
| Confirmation | Preserve title/body/output warning and accept/keep-paused; safe Keep Paused is the initial confirmation selection; no invisible default Resume; acceptance resolves only the exact live host-owned confirmation, once. |
| Dismiss / Escape | Original dismissal and decision-submitted behavior retained; focus returns to Review; original parent Escape route stays valid. |
| Empty / error | Original empty panel suppression, error mapping and Read-again behavior; failure remains visible and does not imply success. |
| Stale authority | Reject old surface/session, source thread/project/backend, sequence, projection identity, confirmation identity, opened/pending/error/submitted state, removed/disabled operation and stale keyboard selection. |
| Refresh lifetime | Hiding or rebuilding the Surface does not cancel controller refresh or automatically replay a business request. |
| Native keyboard | A real GPUI focus handle owns the leaf host; Tab/Shift-Tab traverse enabled displayed controls in order; held Enter/Space never submit; pointer selection, visible NSH ring and AX active descendant refer to the same observed action; Escape remains with parent. |
| Scroll reveal | Keyboard-selected controls are revealed with supported NSH measure/scroll APIs and current-selection fences, not on every snapshot; scrolling never moves native focus asynchronously. |
| UI failure / reload | Independent NSH recovery presentation retains explicit recovery/dismiss controls as applicable; reload replaces display identity only, never retries or resumes business work. |
| Styling / i18n | Existing theme and Typography/Layout tokens and all original translated recovery copy; NSH draws every product pixel, no legacy view inside native(). |

## Implementation plan

1. Preserve controller APIs and business branches, add opaque projection/confirmation observation identities and an optional presentation owned by `ShellRecoveryUi`.
2. Add a child NSH module with Send + Sync snapshots, ordered controls and a single production acceptance policy. All mouse, keyboard and AX activation uses that policy before the original controller methods.
3. Render existing content in a measured NSH scroll viewport with an invisible GPUI native focus/AX host only. Safe stage selection and observed scroll reveal must not authorize or replay requests.
4. Retain original refresh tasks independently of presentation. Describe the exact parent render/hide hooks without writing parent files.
5. Add focused development regressions for actual policy, stale authority and safe traversal. Run scoped rustfmt/static checks only; parent owns compilation, Clippy, tests, CI and native verification. Record tests for pre-merge archival/removal.

## Implemented controller and presentation contract

- `shell_recovery.rs` retains all three public event/action types and original dismissal, refresh, request, finish and translated reason functions. The only additions inside those controller functions rotate opaque UI observation identities. A static function-body comparison against `b301b64e00690365087362ba21ec45a96f35400f` confirms equality after removing those identity assignments and whitespace. Original thread/project/sequence guards, command-card updates, error decisions and one-second refresh scheduling are unchanged.
- `nsh_view.rs` contains the Surface, nonsecret projection, displayed ordered action policy, original action dispatch and NSH product renderer. Controller confirmation and decision/authority tokens never enter snapshots or commands. The host clones the exact current confirmation only after the current observation/confirmation identity, source, stage, availability and submitted checks pass.
- `keyboard.rs` binds the original `review_focus` / `safe_focus` to the invisible native host. Focus subscriptions publish actual ownership. No mount, reload, measurement or error callback dispatches a business request. Stage focus transfers synchronously only if the other recovery handle already owns focus; it never takes focus from another product region. Pointer activation selects that same action. Tab/Shift-Tab skip disabled actions in displayed order and return to native focus traversal at boundaries; no held Enter/Space activation. Confirmation defaults to Keep Paused, not Accept. AX active descendant uses the selected name and the same guarded action, with an additional current-selection check.
- `reveal.rs` uses pinned NSH 0.4 `watch_scroll`, `ElementRef::measure` and `scroll_to`. Measurement.page is viewport-relative page geometry; scroll.offset is the positive content offset. The destination adds only the missing top/bottom distance and clamps to content minus viewport size. A visible control produces no scroll. It runs once for a keyboard selection identity, revalidates through the host, and checks unchanged scroll observation before issuing the command. After a selected target is mounted/measured (and any requested scroll applied), it acknowledges that exact observed selection. Native Enter/Space and AX activation require that acknowledgment, not merely a newly published host snapshot. No asynchronous native focus calls exist.
- Surface error switches to a new independent NSH recovery Surface, invalidating the old Surface identity. It retains original active statuses and eligible cancellation, pending/error/paused feedback, safe dismiss or review/read as applicable, and an explicit UI Retry. UI Retry destroys only presentation; it does not retry Check/Prepare/Resume or cancel the controller refresh task. Recovery never offers Accept/Prepare. All original detailed content returns on explicit UI reload.
- Every status/warning/command/result/control pixel is NSH. GPUI only sizes/embeds the Surface and owns native focus, keyboard capture and an invisible AX active descendant. Content height is NSH-measured, capped by `Layout::TOOL_GROUP_MAX_HEIGHT` with a local NSH viewport. Theme, language and typography use existing APIs/tokens. No new IO, dependencies or arbitrary URI handling.

## Exact parent integration (required before compilation)

No edits to parent-owned files were made. Parent must apply:

1. In `conversation_stream/render.rs`, change `render_conversation_foreground(&self, ...)` (baseline line 128) and `render_composer(&self, ...)` (line 275) to `&mut self`. Both callers already run in mutable `Render::render`; no public event API change is needed.
2. Change baseline line 456 from `.child(self.render_shell_recovery(cx))` to `.child(self.render_shell_recovery(window, cx))`. The new method is `pub(in crate::conversation_stream)` with `&mut self, &mut Window, &mut Context<Self>`.
3. At the start of `ConversationStream::render`, before constructing the tree, call `self.hide_shell_recovery_surface()` when `self.reading_preview` is true. This method only drops the presentation, not projection/decision/pending/refresh. Action reception independently rejects reading-preview state even before this hook runs.
4. Call `hide_shell_recovery_surface()` when the containing cached stream is hidden/unmounted, and before source route rebind/backend changes in `core.rs` (`select_task_backend`, `rebind_draft_project`, and `apply_thread` when backend differs). This invalidates Surface sessions even for leave-and-return routes; direct thread/project/backend comparison already rejects cross-source callbacks while changed. Do not reset or cancel `shell_recovery.refresh` as a UI hide operation.
5. Keep the existing Escape dispatch at baseline `render.rs:2000` calling `dismiss_shell_recovery`; no new Escape handler or keybinding is required. No new `core.rs` fields or initialization are needed; `ShellRecoveryUi::new` initializes its own presentation and opaque identities.

The source-rebind hook names describe the current baseline. Existing source projection behavior remains parent-owned; this migration does not change how the controller refreshes a new source.

## Validation evidence and limits

Executed on the final owned Rust source:

```text
rustfmt --edition 2024 --check <shell_recovery.rs and four child Rust files>
exit 0

git diff --check -- <owned paths>
exit 0

static controller function comparison against b301b64e00690365087362ba21ec45a96f35400f
  dismiss_shell_recovery: unchanged except opaque UI identity rotations
  refresh_shell_recovery: unchanged except opaque UI identity rotations
  request_shell_recovery: unchanged except opaque UI identity rotations
  finish_shell_recovery: unchanged except opaque UI identity rotations
  recovery_reason: unchanged except opaque UI identity rotations
controller preservation comparison: PASS
```

Pinned registry API inspection confirms ordinary View is not a keyboard stop (`nsh-platform-gpui-0.4.0/src/input.rs:1114`, only Input/Native); View focus resolves through input state (`text_input.rs:1757`). The bridge therefore uses actual native host FocusHandle ownership rather than `view().focusable(true)` as a substitute. Core `element_ref.rs:183/289` supplies scroll/measurement; `ambient.rs:1143` documents initial and changed scroll observations. These are API/static facts, not native runtime evidence.

Twelve focused development tests in `shell_recovery/tests.rs` exercise the production action/selection/reveal policy: review/check/retry/prepare/confirmation eligibility; pending/submitted single-submit; stale session/source/sequence/projection/confirmation; row availability; exact running cancel identity; safe ordered disabled-skip traversal; UI recovery without implicit resume; empty/error behavior; current-selection reveal; scroll geometry; measured-selection rejection; native traversal boundaries. **All are unrun here.** Before merge, archive their exact source/hash and actual parent validation results, then remove `tests.rs` and the exclusive `#[cfg(test)] #[path = "tests.rs"] mod tests;` declaration in `nsh_view.rs`. All other modules are production code.

No build, compile, Clippy, Rust tests, CI polling, app execution, installation or native handtest was performed. Parent-supplied baseline passes are not evidence for these changes. Required next checks are parent integration, coordinated compilation/strict Clippy and the 12 new regressions, independent review, then native keyboard/AX/scroll/focus and recovery handtests on supported platforms. The unintegrated parent call currently prevents compilation; do not claim this leaf has compiled. A second independent recovery-engine failure is not native-tested; engine recovery cannot guarantee rendering under a persistent backend failure.

## Pinned signal API compile correction

Acceptance: replace only the three non-PartialEq signal initializers in `nsh_view.rs` with pinned `signal_with(value, SignalEquals::Never)`. Do not add equality or hashing to Snapshot/ProjectionId, change action assertions, authority comparisons, or controller behavior. Pinned `nsh-core-0.4.0/src/element/reactive.rs:501/506` requires PartialEq only for `signal`, not `signal_with`; Never notifies on every write. Existing explicit content-authority/appearance/language filtering remains intact. No builds/tests are authorized for this correction; parent owns compile validation. No Hash use was found in the owned ShellRecovery Rust files.

Correction evidence: scoped rustfmt check passed after the three-line change. The parent compile report identified these three E0277 failures; no compile or tests were rerun here. Cached outgoing stream entries are `crates/vega/src/window/render.rs` in `VegaWindow::render`, both `self.stream_view.take()` branches (observed lines 513 and 584); parent has now inserted `previous.update(... suspend_shell_recovery_presentation())` in both. This is the concrete outgoing cached-stream retention boundary, not an additional leaf change.

## Parent integrated validation (supersedes leaf-only unrun status)

Parent integrated mutable foreground/composer rendering and window-aware recovery mount; reading-preview, project/backend/source rebind invalidation; public presentation-only suspend hook; app settings/opening-thread hiding and both cached outgoing-stream transitions. No controller timer or business operation is canceled by the UI hide hook. Existing Escape dismissal is unchanged.

Stopped integrated source on baseline b301b64e: UI `cargo check -p vega_ui --locked --offline -j 2` exit0/6.03s; workspace all-target `cargo clippy --workspace --all-targets --locked --offline -j 2 -- -D warnings` exit0/7.86s; workspace fmt and diff checks exit0. `cargo nextest run --workspace --locked --offline -j 2`:87 discovered/executed/passed,0 skipped; all twelve Shell regressions executed, run c40ad357-f27d-45c3-89af-65b36f562866, build15.13s/execution0.403s. Toolchain rustc1.98.0, Cargo1.101.0/fork0.102.0-9, Nextest0.9.151. Prior leaf unrun statements describe its isolated handoff, not this integrated result.

Integrated parent SHA-256: core.rs37d9e0cd4da974e497629a3c05848b0725c04cbacfff14a06c2cb0e8a0cb817a; UI render.rsd73bb870a3636d5b77eaf14d2c44a70b7d9eee49523b8137d23bd50576dff89c; app window/render.rsfebd1a8946a63fa1cf54044927baa24add6bf2546fa7df14b100443ab37b2364. Independent Shell/lifecycle review remains pending; exact new committed-head CI and native keyboard/AT/scroll/recovery acceptance remain unrun. No app execution/packaging/install/merge/release.

## Independent review corrections: acceptance before implementation

Scope for this correction is only `shell_recovery/nsh_view.rs`, `reveal.rs`, `tests.rs` and this document. Parent owns the composer early-return hide hook. No build, Git mutation, app execution or delegation is authorized.

| Finding / scenario | Required correction and regression |
| --- | --- |
| Permission UI suppresses recovery before the next render | Fresh action authority rejects `automation_permission.busy()` even if the old Surface still exists. Snapshot presentation eligibility includes reading-preview and permission suppression; queued Accept and every other action fail while suppressed. |
| Leave / return to a new presentation | Hiding drops presentation only, retaining projection, decision, pending and refresh. Old callbacks remain invalid after return because the new Surface identity differs; new displayed confirmation retains safe Keep Paused selection. No confirmation token enters the engine. |
| Missing control / viewport measurement | An attempt is not a Displayed acknowledgement. A bounded fresh asynchronous measurement may succeed without changing the selected control; no business retry or native focus call occurs. |
| Scroll / layout changes during measurement or host validation | Reject the stale observation, then measure fresh geometry within a finite budget. Acknowledgement requires successfully measured current selection and exact current scroll observation; a post-scroll measurement must succeed too. |
| Selection or Surface changes while an attempt is pending | Old completion cannot acknowledge or consume the new selection's measurement. At most one measurement attempt is in flight for the current selection. |
| Repeated invalidation, duplicate effect notifications, host rejection | Distinct in-flight/completed/failure state prevents duplicate measurement and reactive self-trigger spin; finite per-observation and per-selection budgets bound automatic measurement. Host rejection terminates that selection's measurement, never automatically retries an action. |
| Regression preservation | Keep all twelve existing tests/assertions, add focused tests against the production eligibility/attempt policy for hidden queued activation, new-presentation replay, transient measurement and scroll recovery, stale completion, duplicate scheduling and safe caps. Development tests remain pending parent execution and must be archived/removed before merge. |

Implementation: add a nonsecret presentation-eligibility projection plus a direct fresh busy fence; replace the consumed-at-start marker with a production bounded attempt state machine and exact observation checks around asynchronous measure/host validation/scroll. Preserve existing controller bodies, geometry math, native focus bridge and action dispatch.

## Review correction implementation and evidence

P1: `Snapshot.presentation_eligible` is derived from `!reading_preview && !automation_permission.busy()`, participates in visibility and exact authority, and therefore also fences selection/Displayed/Reveal acknowledgement. `accepts_shell` independently checks fresh permission busy state before original action dispatch. Parent's `render_composer` early-return hide call is present on readback (line 277 at inspection). No additional parent hooks are required for this correction. The hide body remains presentation-only; `shell_recovery.rs` and `keyboard.rs` retain their supplied hashes. No controller token or new business operation enters the engine.

P2: production `Attempts` distinguishes in-flight, measured-completed and terminal host-rejected outcomes. It permits two attempts for an unchanged observation and six total for a Surface/selection identity. A changed scroll/layout observation resets the two-attempt allowance, never the six-attempt total. Scroll changes increment a monotonic observation revision (including away-and-back changes); control and viewport layout bounds changes increment that same revision. Each effect reads actual state/scroll/layout plus an asynchronous completion wake, but scheduling mutations use nonreactive `Stored`, so scheduling cannot synchronously retrigger itself. At most one attempt is active for the current selection; old completions cannot release a newer attempt. Tasks belong to the Surface renderer owner rather than a rerunning control effect, preventing an effect rerun from canceling a task and stranding in-flight state.

Measurement checks current Surface/selection/authority and exact scroll+revision after awaits and after host reveal approval. A scroll command is never a Displayed ack: it waits for a layout round, then a fresh bounded measurement must establish visible, finite positive control/viewport geometry with unchanged observation. Only that current measured completion sends Displayed; the host retains its original fresh focus/selection/authority validation. Host rejection is terminal even if focus changes before its response arrives. No delayed native focus, automatic business request, reload/retry/resume or controller timer change is introduced. Completed selections do not unconditionally remeasure/scroll on every snapshot. Exhausted budgets fail closed (no Enter/Space/AX activation); a new user selection/presentation gets a new budget. Persistent measurement failure and actual native scroll/AX behavior still require runtime validation.

Static validation on the final correction: scoped `rustfmt --edition 2024 --check` passed for the three owned Rust files; owned-file trailing-whitespace/conflict-marker/newline checks passed. Original twelve test functions/assertions are byte-for-byte preserved: removing only the new fixture eligibility field and appended tests reproduces original tests SHA-256 `6b5b96676f02225d6a3050d915a717d619d90510f2f7a60a71f1ba8212a041ab`. The first preservation-check script accidentally appended an extra newline; inspecting the prefix identified that observation-script error, and the corrected hash check passed without source/assertion changes. Production files have no unwrap/expect/comments; reveal has no native focus call. No Git command was run for this correction; therefore no current `git diff --check` result is claimed. No build/Clippy/test/app/CI execution was performed. The prior integrated 87-test result does not validate this delta.

Development inventory is now **19 tests: 12 retained + 7 new**, all unrun on this correction. New tests call production `Snapshot` eligibility and `Attempts` transitions, not a duplicate retry algorithm: hidden queued Accept and new-Surface replay; missing measure then fresh success; changed-scroll rejection then fresh success; old selection/Surface completion rejection; duplicate scheduling/unchanged cap plus changed-layout restart; finite total cap; terminal host rejection and hidden completion. Parent must compile/run these regressions, review the asynchronous platform wiring, and archive/remove `tests.rs` plus the exclusive test module declaration before final merge as already required above.

## Final correction validation and independent review

Main executed on the corrected stopped source: workspace fmt/diff checks exit0; `cargo clippy --workspace --all-targets --locked --offline -j 2 -- -D warnings` exit0,8.21s; `cargo nextest run --workspace --locked --offline -j 2` executed/passed94/94,skipped0,execution0.411s. This includes all nineteen Shell policy regressions and all three shared keyboard regressions. The earlier87-test run is historical, not the final correction result. Final UI render.rs SHA-256 is `60ec454302052d001117f126cfe9f6b50a154a1ed15c3ac46bd142d9f2f45645`; unchanged parent core/app hashes remain above.

Independent incremental source review closed P1 and P2 on the final leaf hashes below: the permission early-return retires presentation and fresh suppression gates block callbacks; bounded observation/selection budgets, explicit effect wake, renderer-owned tasks, exact scroll/layout rechecks and fresh post-scroll measurement preserve acknowledgment safety. No unresolved concrete finding in the corrections. Persistent measurement failure or budget exhaustion intentionally fails closed; native interaction and assistive-technology behavior remain unverified. No app/packaging/install/merge/release. Exact new-head cloud CI is still required after committing this development stage; tests still must be archived/removed before final merge.

## Owned Rust content hashes (SHA-256)

```text
d51aa2c69048f60995a0f7a865dd9fd0a507a71f504c017fdf755d3b659df901  shell_recovery.rs
d2a22d85c38421d61f0ec4fe6b1030b1297bd31644c1b39343f36f161f373f3b  shell_recovery/nsh_view.rs
c48f7811bc55c1d017cd4ce05d1fadb181ef7a6f14edde3b3e801661b6fcebaa  shell_recovery/keyboard.rs
5761f0304ec55960a9c89653319e9918fcc417ddc7dac601197eb6ef15d46e87  shell_recovery/reveal.rs
c9c74dcb753d2399dbc6029a274303cf2a892c9c10f81866878f81fd3ec50a04  shell_recovery/tests.rs
```
