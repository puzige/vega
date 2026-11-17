# NSH timestamp separators and ordinary-send controls

## Authority and baseline

This is author implementation of confirmed inventory gaps C1 and C2, **not independent review**. C3/message anchors belongs to the parent and is outside this lane. Worktree: `/Users/puzige/Workspace/worktrees/vega-907-nsh-full`; dirty HEAD `0b5ea426abfd8d4358489e916273d89474fb190f`. Parent reports fresh remote/task-head synchronization and mainline `0b3dcd3603a8e97e0ef74ae25a26f17fc3a094e5`; this lane performs no fetch, network or Git mutation.

Original source SHA-256:

- `conversation_stream/timestamps.rs`: `a78a4d05406b8c06edbe260ad38a852322c2e4004264d75279eb540ed3514753`.
- `conversation_stream/ordinary_submissions.rs`: `15f98bc6f0980004d039d269a8e95fa6ae6ccbbb76f12de0840d50ee2e258331`.

Source paths below are relative to `crates/vega_ui/src/`. Applicable references: root AGENTS, [execution guide](../vega-exec-guide.md), [design guidelines](../vega-design-guidelines.md), [full migration contract](nsh-full.md). No nested AGENTS applies under the two source leaves or `docs/907`.

## Acceptance fixed before code

### C1: timestamp separator

- Preserve timestamp collection, adjacency, invalidation, running-message suppression, scheduling, formatting and localized accessible timestamp exactly. Existing footer/focus behavior is not part of this renderer replacement.
- Preserve localized day followed by the existing separator suffix, day medium weight, tertiary color, body typography, centered row, `CONVERSATION_STAGE_TOP` vertical padding, noninteractive/non-tab-stop semantics and the existing initial list-height estimate.
- Actual visible text must be NSH text in a retained transparent Surface; no GPUI text child, shaped GPUI text paint, fallback old label or hard-coded replacement copy. Obtain actual height from the NSH natural content layout and update the retained host; stale measurements must not resize a replacement projection.

### C2: ordinary-send controls

- Preserve the actual original message/detail/action projection, including unknown outcomes, permission-invalidated detail, configuration-restart/no-action state and diagnostics-only states. No extra copy, hidden exception paths or business-rule changes.
- Preserve final user-text/user-images row matching, submission/thread/attempt identity, reference-failure and interrupted-run suppression, existing status mapping, retry eligibility, original action event authority and diagnostic clipboard behavior.
- NSH owns all visible status text, button labels and focus/hover/pressed chrome. GPUI may host transparent AX/event/focus wrappers only. Preserve action-before-diagnostics focus order, original per-row focus handles and `PendingMessageControl` keyboard activation.
- Commands carry the immutable displayed snapshot. Validate retained host identity, presentation identity, owner/route/current source, same submission and attempt, current eligible projection, live mount, visible row/viewport/foreground geometry, active window and Surface health before emitting the original `OrdinaryActionRequested` or reading diagnostics for the displayed attempt. Never substitute the latest attempt or rebuild a fresh authority snapshot in an activation callback.
- NSH natural layout reports height with matching presentation and width. Retain the Surface between unchanged renders; stop old presentation on semantic replacement or release, reject commands immediately when the mount lease disappears, and stop/remount on subsequent presentation reuse. Unreachable/dead mounts and stale queued commands must have no action.

## Mounted validation delta: acceptance and plan fixed before code

This author implementation continuation starts from the stopped seven-file identity `ed79f0098cd59c787ac47cd861ec2d2e497fa25b8d4ec8d838b938f770b9445d`. The parent reports those source bytes unchanged and the static source/snapshot/mount reading valid. Earlier GREEN counts and core-only tests do not satisfy this mounted acceptance. This continuation is not independent review.

Required mounted acceptance:

- Use real platform fonts, real GPUI windows/focus handles and the production `ConversationStream` with its original `ListState`; create ordinary submissions through an in-memory SQLite Store, admission, worker failure, seal and retry. Capture original row identities and `Source::capture`, never manufacture a parallel source/projection.
- Wait for real NSH layout, displayed acknowledgement and GPUI host-height/geometry convergence within the existing 10-second readiness budget. Scheduler drains retain the 2-second and 512-tick limits. No nominal Core/ApproxTextEngine test counts as mounted evidence.
- Hit actual NSH Retry and diagnostics buttons through the production full-inset transparent AX/focus wrappers. Assert one event with the exact thread/submission/displayed-attempt payload and exact displayed-attempt diagnostic clipboard. Exercise real focus order, keyboard and accessibility dispatch, without invoking activation directly as a positive-path substitute.
- Capture old displayed presentations and verify rejection after retry, same-attempt fact mutation, route/settings changes, geometry changes and actual unmount. Observation must not change authority or make a stale presentation current.
- Mount the production timestamp separator with long/narrow day plus suffix. Observe actual native text/layout and host natural height, original day/suffix weights/padding and passive pointer/wheel/focus behavior. Test both narrow and wide allocations; no GPUI text painting or replacement copy.

Plan: add small `nsh-reading-integration`-gated observations inside the two owned host leaves; add one mounted case module under ordinary controls and one under timestamps; add a fail-fast main-thread integration runner with exact case counts. Only fix a concrete production defect demonstrated by current code/API evidence. Parent wires the requested module re-export and Cargo test registration and executes all compilation/tests; this lane performs no Cargo/rustc/network/Git mutation or shared-module writes. Development-only fixtures/observations remain subject to archive-and-remove-before-master policy.

## Minimal implementation plan

1. Keep timestamp state/format logic untouched; replace only `timestamp_prefix` construction with a local RenderOnce helper that acquires the current window itself. Add a local retained passive host and NSH renderer with natural-height measurement.
2. Factor the existing ordinary projection/matching into a reusable local projection used both at render time and validation time, without changing its decisions. Replace only the visible builder with a local RenderOnce row, retained Surface host and NSH pixel renderer. Keep the public `ordinary_controls(index, cx)` signature if the helper can obtain its own window cleanly; otherwise report the exact parent-owned call-site change without editing `render.rs`.
3. Add focused development behavior tests that mount the production NSH renderers, inspect real text/geometry/control names and execute real retained handlers across replacement/disposal. Add direct production projection/validation checks where feasible; no source-string assertions or parallel decision mirrors. Existing tests and assertions remain unchanged. Local tests are development-only and must be archived/removed by the parent before master under repository policy.
4. Run only permitted static checks and record exact hashes. Parent owns all Cargo/rustc/build/test execution, shared registration, final-version regressions and later independent review. Preserve the historical inventory and add a clearly superseding current implementation section.

## Validation boundaries

No Cargo, rustc, build, app launch, network, Git mutation, vendor edits, shared exports, dependency changes or writes outside the authorized two leaves/new subdirectories and these two documents. Parent's pre-delta GREEN evidence is not validation of this delta. Mounted approximate-engine tests are not platform/native acceptance. Full native acceptance, final head CI and independent review are not claimed.

## Implementation and handoff

Both authorized visible leaves are implemented in the dirty worktree; compilation and behavior execution remain **unverified**. This is implementation handoff, not independent review or native acceptance.

### Production routes and preserved authority

- `conversation_stream/timestamps.rs:319` keeps the original formatting/suffix extraction and returns local `presentation::Separator`. `timestamps/presentation.rs:51` acquires the window through RenderOnce; `:105` retains a transparent Surface; `:187` observes width and suppresses Fast hitboxes; `:237` constructs actual NSH text with day medium weight, body line height, center alignment and original vertical padding. Natural NSH layout reports update the host height only for the matching projection and width. `timestamp_extra_height` remains the original bootstrap estimate; timestamp collection/scheduling and footer handling are unchanged.
- `ordinary_submissions.rs:611` contains the unchanged original final-row identity, suppression, controls and diagnostic availability projection. `:652` keeps `ordinary_controls(&self, index: usize, cx: &mut Context<Self>) -> Option<AnyElement>`, now returning `presentation::ControlsRow`. Existing `render.rs` calls need **no change**.
- `ordinary_submissions/presentation.rs:41` captures current attempt facts; `:109` checks route, stream suspension, original projection, current/latest attempt and unchanged live status mapping. The status mapping is factored once at `ordinary_submissions.rs:49` and remains used by the original refresh function. It does not change retry or permission decisions.
- `ordinary_submissions/presentation.rs:159` projects only original localized status/detail/button strings; `:264` creates/retains the keyed host with the original action and diagnostic focus handles. `:362` validates immutable presentation, owner, live mount, row/viewport/foreground geometry and Surface health; `:387` emits the original event payload or copies diagnostics for the **displayed** attempt, never the latest substitute. Transparent Fast wrappers at `:519–550` retain action-before-diagnostics focus order, original key context/action binding and AX activation. NSH owns visible text/chrome.
- `ordinary_submissions/presentation/pixels.rs:10` constructs the actual NSH status/button tree and natural-height listener. `ElementRef::measure`, presentation-owner liveness, layout revision and OrderedCommands establish Displayed receipts; press callbacks capture the old snapshot rather than reading latest signal authority. NSH draws the selected focus border and hover/pressed chrome. Narrow rows wrap instead of clipping out diagnostics; no explanatory copy is added.
- `ordinary_submissions/presentation.rs:598` is geometry-only prepaint forwarding; `:653` forwards child paint without drawing product pixels itself. Expired mount leases reject queued commands; presentation reuse/release stops the prior Surface. The shared `run_notice_epoch` is reused read-only as the existing stream-suspension identity; no shared field/export/API is added.

### Focused development behavior tests

Six `#[test]` functions are wired locally by `#[cfg(test)] mod tests`; **no shared registration is needed for these unit tests**:

- `timestamps/presentation/tests.rs`: actual production renderer text, localized Latin/CJK examples, day/suffix weights, center geometry, original padding, actual natural-height reports, passive AX semantics, stale projection/width/invalid measurement rejection.
- `ordinary_submissions/presentation/tests.rs`: original production controls/content projection mounted in the real NSH Core with ApproxTextEngine, combined retry/diagnostics, permission-invalidated detail, configuration-restart/no-action, message-only and diagnostics-only, named button ordering, narrow/wide geometry, actual action and diagnostic press handlers, displayed identity preservation across a real retry attempt replacement, disposed listeners, and production acceptance checks for changed route/entry/facts/presentation/eligibility.

These tests do not claim to execute live GPUI owner/window dispatch, clipboard, keyboard traversal, mount-lease/viewport rejection or OS-native rendering. Parent should run those mounted/integration regressions as well as the six new unit tests. The tests use the existing dependency graph only; the real submission fixture uses an in-memory Store and normal admission/worker/seal/retry APIs. Development tests must be archived and removed before master per repository policy.

### Executed static checks and unexecuted checks

Executed with exit 0: direct `/Users/puzige/.rustup/toolchains/1.98.0-aarch64-apple-darwin/bin/rustfmt --edition 2024 --config skip_children=true` on explicit authorized files, followed by `--check` on all seven Rust files. This invokes neither Cargo nor rustc and does not traverse shared modules. Toolchain compilation/version queries were not performed; parent owns those records.

Executed Python static checks: UTF-8, final newline, no trailing whitespace, no source comments/suppressions, no production unwrap/expect, and exact baseline comparisons proving unchanged timestamp collection, original controls decisions, original row matching/suppression/diagnostic availability and whitespace-normalized factored status mapping. These are static preservation checks, **not substitutes for behavioral tests**. Six tests are defined, **zero were executed by this lane**. No existing tests/assertions were changed or weakened.

Parent-provided pre-delta GREEN counts (UI347/source14/native36/geometry24/scroll16/icons3/settings3/recovery2 and strict workspace/all-target/features) remain historical, not results for this delta. Required next checks: compile/clippy the final dirty bytes; run the six new tests and existing conversation regressions; verify mounted focus Tab/Shift-Tab, Enter/Space/AX, diagnostic clipboard, original dispatch payload, same-attempt mutation/latest-attempt races, suspension/route/settings/window activity, resize/scroll/foreground/clipping/unmount/reopen; rerun the final integrated suites. No Cargo, rustc, build, application, network, Git mutation, dependency, vendor or shared-source write occurred here. The initial tool command used the default Loom cwd for read-only discovery; subsequent source work explicitly selected the assigned worktree. This lane read focused current vendor API definitions but never edited vendor.

### Final source identities

SHA-256; source paths are relative to `crates/vega_ui/src/`:

| File | SHA-256 |
| --- | --- |
| `conversation_stream/timestamps.rs` | `b4dd35a9b5a8df479a9c4df4cd8b42ec607a3b886d3722f71542b29fe748007d` |
| `conversation_stream/timestamps/presentation.rs` | `6f6157d676c0b3d156f660a40edcba888bc22ff09204bc5ffd5730c3d77c623c` |
| `conversation_stream/timestamps/presentation/tests.rs` | `c807b70365185535f5651548cd3211c2b5568939acd5d5a2776519282691d5a2` |
| `conversation_stream/ordinary_submissions.rs` | `292eba6cb83f87ca310a100cc29bb5f6f08175cfc5d387c31c46a07ff38703ed` |
| `conversation_stream/ordinary_submissions/presentation.rs` | `82da774bf40dab881d5b770502bec7ee71941deaf7f9593278f5e080323a7703` |
| `conversation_stream/ordinary_submissions/presentation/pixels.rs` | `0355b4a8a9a17d725920d5edaa0048d8c2521c7d2632215cd31cf88a0152114b` |
| `conversation_stream/ordinary_submissions/presentation/tests.rs` | `b9798a3b097ce65211fa2993be3fde8266150a494d8240e5b215b22d01a73355` |

Seven-file identity: sort repository-relative paths; concatenate lowercase SHA-256, two spaces, path, LF; hash the concatenation: `ed79f0098cd59c787ac47cd861ec2d2e497fa25b8d4ec8d838b938f770b9445d`. Dirty HEAD alone is not a content identity.

Only these seven Rust files and the two authorized documents were written in the original implementation stage. No parent API/call-site/export request was required for that stage. C3/message anchors remains parent-owned and is not reassessed here. Independent review still waits for completion of all implementation; no final CI/native acceptance, merge or release is claimed. **Original-stage writes stopped at that handoff; the following authorized validation delta supersedes its next-check/export status.**

## Parent UI failure diagnosis delta: acceptance and plan

This section supersedes the earlier frozen-source/next-check status without erasing historical evidence. The parent built all eleven UI groups successfully in 33.06 seconds (`target/nsh-closure-ui-signal-build.json` and `.stderr`), then ran 356 UI tests: 354 passed, two failed, zero ignored (`target/nsh-closure-vega_ui.stdout:360-380`; stderr empty). Those results precede this delta, not validation of it. The parent applied the standard-test macro imports, `SignalEquals::Never`, public `nsh_core::TextLine` types and the timestamp Stateful wrapper ID; retain those changes.

Acceptance remains unchanged: retain the centering/padding/natural-height assertions, exact single displayed-attempt activation and stale-authority rejection assertions, all existing cases and all scheduler/readiness budgets. No product copy, dispatch authority, timestamp clamp, production rendering, shared module, vendor or Cargo changes are authorized without concrete failure evidence. The four mounted cases remain defined and unexecuted in this lane.

1. C2 is a confirmed fixture quiescence defect, not evidence of a production activation defect. `ordinary_submissions/presentation/tests.rs:118-123` polls tasks, commits, then exits on no mount/events/due `after_answer` callbacks. `pixels.rs:187-201` awaits `target.measure()` before sending Displayed/Activate. NSH queues that measurement in `vendor/nsh-core/src/core/components.rs:4973`; `core/mod.rs:2151-2153` delivers it inside commit; `element/task.rs:599-610` wakes its awaiting task. `has_due_answers()` only inspects the separate `after_answer` closure queue (`element/task.rs:533-550`), not this newly ready task. Thus a no-paint measurement commit can make the fixture return before the activation task resumes. Fix only fixture quiescence using the real public `Core::set_task_notify` callback and an atomic wake observation, keeping the existing 32-turn cap and every action assertion.
2. C1 is not yet classified as a product or fixture defect. The parent failure reports only the Boolean centering failure, not left/right/frame values. `Core::measure` provides parent-relative `frame` and root-relative `page` (`vendor/nsh-core/src/core/measure.rs:14-23,53-99`); no evidence authorizes swapping those coordinates or changing centering math. Add bounded, failure-only diagnostics to the existing centering/padding/height assertions: both actual measured text nodes, the native separator root and the root surface. Preserve the exact assertions and production clamp. The next parent execution must supply those numbers before any C1 layout repair; do not guess a production fix from the failure name.
3. Run scoped formatting/static checks only in this lane; central compilation and tests remain parent-owned. Handoff exact source hashes and a concrete parent rerun, distinguishing the C2 fixture repair from the C1 evidence-gathering delta. No rerun is represented as passed here.

## Mounted implementation delta handoff

### Added cases and evidence boundaries

Four mounted cases are defined by the new fail-fast runner `crates/vega_ui/tests/nsh_ordinary_controls_integration.rs`. **Four defined, zero executed here.** The runner uses the current platform text system, rejects absent fonts, allows scheduler parking, initializes the actual GPUI/UI bindings and reports passed/failed/not-executed counts, stopping at the first failure. The bounded frame loops retain 2-second/512-tick drains and 10-second readiness deadlines; no deadline extension, ignored case, suppression, new dependency or weakened existing assertion was added.

- `ordinary_submissions/presentation/mounted_regressions.rs:432`: mounts the production `ConversationStream` and original ListState. A real migrated in-memory SQLite Store supplies the database identity; ordinary admission/begin/worker failure/seal produces the user row. The fixture observes actual retained `Host` entities with `observe_new`, then validates original row identity through `Source::capture` (line 283), not a manufactured source. The readiness predicate (line 229) requires native status/button text layout reports, native root/button bounds, valid displayed acknowledgement, matching host natural height, matching viewport/row geometry and no Surface error. Actual GPUI MouseMove/MouseDown/MouseUp targets the NSH button bounds beneath the original full-inset AX/focus wrappers. One original Retry event must have the exact thread/submission/displayed-attempt payload. Real focus handles, `focus_next`/`focus_prev`, and original Enter/Space bindings are exercised. Diagnostics must copy exactly `preparation_diagnostics(displayed_attempt)` and emit no ordinary action. Narrow reflow must reconverge.
- Same module line 486: captures a real displayed production presentation, calls actual `try_retry`, rejects both old Retry and diagnostics before reprojection, completes/seals the next worker, and requires newly displayed diagnostics to match the next attempt. A later `invalidate_recovery` changes facts without changing the attempt; both old actions must again be rejected. Clipboard sentinel preservation accompanies zero-event assertions.
- Same module line 542: old displayed presentations must be rejected with Settings open, route absent, changed foreground geometry, actual width/layout change and actual root unmount. Unmount must release the original mount lease. Negative stale replay deliberately calls the production `Host::activate` with the old immutable snapshot; it never synthesizes a new authority snapshot or bypasses production validation. This is negative fence coverage, not positive input/AX dispatch evidence.
- `timestamps/presentation/mounted_regressions.rs:277`: mounts the production `Separator`/retained host at wide and narrow widths with a long day and suffix. Actual platform `TextLayout` and layout reports must converge with host dimensions; narrow text must increase natural height. Checks cover original day/suffix weights, centering, vertical padding, zero-area input mask, parent pointer/wheel pass-through, no additional tab stop and no Surface focus. The long input is fixture content supplied to the production leaf, not an alteration of production formatting or localized copy. Timestamp formatting/collection is unchanged and remains covered separately by the existing tests.

Gated instrumentation observes native text/layout, host geometry and passive input masks only. The ordinary production decision/action/clipboard code, original row focus handles and transparent full-inset wrappers remain unchanged. A current-API static correction imports `nsh_core::prelude::Interactive` at timestamp renderer scope for its existing `on_layout` call; no product rule or native behavior was changed. No runtime product defect is claimed without a run.

### Concrete remaining AX harness blocker

Actual accessibility dispatch is **not closed** by these four cases. The resolved local `gpui-fast-0.1.9/src/window.rs:7083` exposes listener registration, but `window.rs:7098` declares the dispatcher `pub(crate) fn handle_a11y_action`. `PlatformInput` (`interactive.rs:771`) has no AX action variant. `platform/test/window.rs` exposes pointer/key input but no AX callback injection; `app/test_context.rs:651` also keeps `test_window` crate-private. Production ordinary wrappers still register Click through `ordinary_submissions/presentation.rs:551`. Calling that handler/`Host::activate` directly as a positive AX test would not prove GPUI dispatch and was not added.

Needed next step: parent supplies an already-authorized real GPUI AX adapter/test entry point, or obtains authorization for the smallest GPUI test-support extension that activates the tree and sends an actual ActionRequest through the original dispatcher. This lane cannot edit Cargo/vendor/shared harness files and does not add an authority-bypass observation. The four-case suite is meaningful mounted input/layout regression implementation, **not complete mounted acceptance, full native acceptance or independent review**. Pointer pass-through and convergence are explicit required assertions but remain unrun; runtime failures must be fixed in the responsible production or harness layer, not converted to snapshot-only assertions or budget extensions.

### Parent wiring request (not applied here)

In `crates/vega_ui/src/conversation_stream/mod.rs` add:

```rust
#[cfg(feature = "nsh-reading-integration")]
pub use ordinary_submissions::mounted_regressions as ordinary_controls_mounted_regressions;
```

The owned modules already expose the controls cases and re-export the timestamp case. No `render.rs` call change is needed. In `crates/vega_ui/Cargo.toml` register:

```toml
[[test]]
name = "nsh_ordinary_controls_integration"
harness = false
required-features = ["nsh-reading-integration"]
```

Parent then compiles/clippies final bytes and runs `cargo test -p vega_ui --test nsh_ordinary_controls_integration --features nsh-reading-integration --locked --offline`, with its approved toolchain/environment and concurrency. Rerun the unchanged six focused unit tests and relevant integrated suites. Parent owns Cargo/rustc versions, actual command/log/result/time records, any runtime corrections and the remaining AX dispatch acceptance. Rust development tests and their exclusive observations/exports/registration must be archived and removed before master under repository policy.

### Executed checks and frozen identities

Executed successfully: explicit scoped rustfmt using `/Users/puzige/.rustup/toolchains/1.98.0-aarch64-apple-darwin/bin/rustfmt --edition 2024 --config skip_children=true`, then `--check` on the eight changed/new Rust files. Python static checks on all ten scoped Rust files confirmed UTF-8, final newlines, no trailing whitespace, no ordinary comments or allow/expect suppressions, and computed the identities below. Existing two `presentation/tests.rs` hashes are unchanged. No Cargo, rustc, build, mounted test, application, network, Git mutation, vendor/shared module/Cargo write or dependency change was performed. Earlier parent GREEN counts are not current-delta validation.

Paths below are relative to `crates/vega_ui/`; SHA-256:

| File | SHA-256 |
| --- | --- |
| `src/conversation_stream/ordinary_submissions/presentation/mounted_regressions.rs` (new) | `c85277d7f18bf65ffb6b2284142b01a864ba703b32408fb7b482a4778ead3149` |
| `src/conversation_stream/ordinary_submissions/presentation/pixels.rs` | `cd900d7659948e63e5cf669b2fd50a8f001d4e3a20b5b2809a6a34a0a4db80ee` |
| `src/conversation_stream/ordinary_submissions/presentation/tests.rs` (unchanged) | `b9798a3b097ce65211fa2993be3fde8266150a494d8240e5b215b22d01a73355` |
| `src/conversation_stream/ordinary_submissions/presentation.rs` | `7e18e375d8c6df57191df4809f560e28e6c581805394bddcbfc09e0e51553279` |
| `src/conversation_stream/ordinary_submissions.rs` | `2aa5e125dd4eee2faed302bafc04d904c0c425fcb9b441382d31ee781c41e130` |
| `src/conversation_stream/timestamps/presentation/mounted_regressions.rs` (new) | `10c30798398930e3a545b3a3c9f828cab18eb064d5c1330ebab02cadd58969ab` |
| `src/conversation_stream/timestamps/presentation/tests.rs` (unchanged) | `c807b70365185535f5651548cd3211c2b5568939acd5d5a2776519282691d5a2` |
| `src/conversation_stream/timestamps/presentation.rs` | `962f34693c6984e2dd85e3e00f69abce3fa79e3e4cf2f2f24b5ae30256f219fb` |
| `src/conversation_stream/timestamps.rs` | `adf79874b380a90680b7c0a486d80f1fc5a240fe6f1632cd9b76c244d954bb82` |
| `tests/nsh_ordinary_controls_integration.rs` (new) | `071bbc2c452b277e64f6b1e634bb62df44f9d7a1fd5210db1caf9712d0b012d6` |

Ten-file source identity using the original sorted repository-relative hash-manifest algorithm: `f85b56eb033e47a0d09d99831bb5bf883472baaea376dc40c71a70bcf1d09a0f`. Only eight Rust files above and this authorized document were written in this continuation. `nsh-conversation-production-closure.md`, shared exports/Cargo, C3/message anchors and other writers' leaves were not modified. **Writes stopped at that historical delta handoff; the failure-diagnosis handoff below supersedes its identities.**

## Failure-diagnosis handoff: writes stopped

This continuation wrote exactly three files: the two existing `presentation/tests.rs` leaves and this document. No production Rust bytes changed. Parent compilation corrections and the four mounted regression cases remain intact; no new exports or registration are requested. This remains author implementation/diagnosis, not independent review.

- C2: implemented the fixture-only wake-aware quiescence repair at `ordinary_submissions/presentation/tests.rs:117-157`. The existing 32-turn cap is unchanged. A real measurement-completion task wake keeps the fixture pumping; it neither manufactures a request nor bypasses `pixels::button`, immutable displayed snapshots, measurement, live-owner checks or ordered command dispatch. Single-action, diagnostics, replaced-handler and stale-attempt assertions remain intact. Added a bounded failure message at lines 322-329 only.
- C1: added failure-only measured text/separator/surface diagnostics at `timestamps/presentation/tests.rs:99-150`. The original centering formula/tolerance, vertical-padding check and initial/natural-height check remain unchanged. **The timestamp failure is still open; the original logs do not contain the numbers needed to distinguish a real layout defect from a fixture observation fault.** No unsupported coordinate substitution, renderer change or clamp change was made. Next repair must use the parent's diagnostic rerun, not treat these added diagnostics as a fix or a PASS.
- Validation actually executed here: the explicit `rustfmt --edition 2024 --config skip_children=true --check` command on both changed Rust files exited 0; Python static checks on all ten scoped Rust files verified UTF-8, final newlines, no trailing whitespace, no ordinary comments/suppressions and computed SHA-256 identities. No Cargo, rustc, build, unit/mounted test, network, Git mutation, shared module/Cargo/vendor write or dependency change ran in this lane. The original 354/356 result is not post-delta evidence. Four mounted cases remain unrun here; actual AX/native/full acceptance is not claimed.

### Parent's concrete next verification

Using the parent's already-approved toolchain/environment, rebuild the changed library test target and rerun the exact two failing cases with `--exact --nocapture` (full names below); do not reuse the old binary as current-source evidence:

1. `conversation_stream::timestamps::presentation::tests::production_separator_preserves_localized_text_weight_center_padding_and_natural_height` — preserve its full failure message with left/right, both page/frame measurements, separator/root bounds and actual/initial height if reached. Those are the concrete missing C1 evidence. If the assertion still fails, return those numbers for the smallest responsible-layer repair.
2. `conversation_stream::ordinary_submissions::presentation::tests::production_press_retains_displayed_attempt_and_replaced_handlers_cannot_launder_authority` — must complete its actual Press → measurement reply → ordered command path, emit exactly one displayed-attempt CheckStatus and one matching diagnostics request, and reject disposed/replaced listeners under the unchanged 32-turn cap.
3. Rerun all six focused C1/C2 unit cases and the full UI group, then the retained four-case `nsh_ordinary_controls_integration` runner. Keep the existing 2-second/512-tick drain and 10-second readiness budgets, full-inset actual pointer path, displayed/layout convergence and stale-authority assertions. Record actual command/toolchain, counts/time and remaining AX boundary; do not call nominal Core fixtures mounted acceptance.

### Current frozen identities

SHA-256; paths relative to `crates/vega_ui/`. These include the parent's intervening compilation fixes, not only this lane's edits:

| File | SHA-256 |
| --- | --- |
| `src/conversation_stream/ordinary_submissions/presentation/mounted_regressions.rs` | `3494fda898aae81d537c119c1089438992754c0dcec7cd96e182480f561db86c` |
| `src/conversation_stream/ordinary_submissions/presentation/pixels.rs` | `cd900d7659948e63e5cf669b2fd50a8f001d4e3a20b5b2809a6a34a0a4db80ee` |
| `src/conversation_stream/ordinary_submissions/presentation/tests.rs` | `036aa7b55b5c3767f06bf5c0d1d87329f7bbaf1d52b6622d253771d0b9d3642a` |
| `src/conversation_stream/ordinary_submissions/presentation.rs` | `7e18e375d8c6df57191df4809f560e28e6c581805394bddcbfc09e0e51553279` |
| `src/conversation_stream/ordinary_submissions.rs` | `2aa5e125dd4eee2faed302bafc04d904c0c425fcb9b441382d31ee781c41e130` |
| `src/conversation_stream/timestamps/presentation/mounted_regressions.rs` | `fb9925a37f7e5b930b3ce57f39c935d554a0b9ccd59e3ac1a018968478f304ea` |
| `src/conversation_stream/timestamps/presentation/tests.rs` | `db893b6b2f5df3f5aadaf989341b3cce86a7020e1b3b5222625d4c6e15d2c230` |
| `src/conversation_stream/timestamps/presentation.rs` | `dfb45ccc2c1d9e797d215f8475da30804ff7f84f0437224b8940f4aeb09fff01` |
| `src/conversation_stream/timestamps.rs` | `adf79874b380a90680b7c0a486d80f1fc5a240fe6f1632cd9b76c244d954bb82` |
| `tests/nsh_ordinary_controls_integration.rs` | `071bbc2c452b277e64f6b1e634bb62df44f9d7a1fd5210db1caf9712d0b012d6` |

Ten-file identity (same sorted repository-relative hash-manifest algorithm): `592aa472649fd09bf5356c0924b6a8b9a80de1a0608fbd4d5aa7fc26edc306a3`.

Original evidence SHA-256:

| File | SHA-256 |
| --- | --- |
| `target/nsh-closure-ui-signal-build.json` | `881a2b4f8b0f68da0cd3a7613df3eaed92600b570a0039f948cca8252b2046ac` |
| `target/nsh-closure-ui-signal-build.stderr` | `9602e3c33d285271ad77da2cc1debcbdd35a8853bd6692e08a2dda6285911898` |
| `target/nsh-closure-vega_ui.stdout` | `93854b092e6d1d7ea3965237f91b7acac8633512e587dde96c02b3de38dc57e7` |
| `target/nsh-closure-vega_ui.stderr` (empty) | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |

**Writes stopped at that historical handoff. The second-run repair continuation below supersedes its current-state conclusion and identities. No assertion or budget was weakened to obtain green.**

## Second-run repair: acceptance and plan fixed before edits

Parent rebuilt successfully (`target/nsh-closure-repair-build.json` / `.stderr`, reported exit 0, 37.82 seconds). Exact executions are recorded in `target/nsh-closure-repair-execution.json`. This remains author implementation, not independent review. Current scope is only the C1/C2 source leaves/children and this document; no Cargo/rustc, network, Git mutation or shared-file writes by this lane.

Evidence classification:

- **C2 production stale-listener defect:** `target/nsh-closure-repair-ordinary-press.stderr` records `pixels.rs:188` reading a disposed `revision` signal, followed by the unchanged `tests.rs:368` disposed-listener assertion. The original zero-request fixture failure is no longer the failure. Actual `Owner::is_live()` checks handle generation and graph disposal (`vendor/nsh-core/src/element/reactive.rs:993–1001`); `Owner::run()` executes even for a disposed owner, under the disposed graph identity. Therefore running under the command owner is not itself a presentation-liveness check. Repair all C2 pixel event handlers before presentation-owned signal access, and guard both owners before spawning and again inside async measurement leases, including after the await. Retain immutable displayed snapshots, revision equality and host/controller authority checks; no catch-panic, latest-snapshot substitution or action bypass.
- **C1 observation defect:** `target/nsh-closure-repair-timestamp-center.stderr` records the added `tests.rs:113` `expect("surface geometry")` panic. `Core::measure()` may return `None` for a root without a created native view (`vendor/nsh-core/src/core/measure.rs:50–61`). Make only this auxiliary surface diagnostic optional. Keep required day/suffix/separator measurements and every assertion; emit bounded real measurements before the first geometric assertion so neither centering nor height hides the evidence.
- **C1 production natural-wrap defect:** the actual mounted runner reaches `timestamps/presentation/mounted_regressions.rs:285` and fails the wide-versus-narrow natural-height assertion; its summary is 0 passed, 1 failed, 3 not executed. Static source establishes native `Style::default()` is already flex display but defaults to `flex_shrink: 0` (`vendor/nsh-core/src/style.rs:1434–1450`), while the current timestamp text children do not opt into shrinking. Native measurement uses known width/available width to constrain text (`vendor/nsh-core/src/core/mod.rs:7704–7730`). The smallest repair is `flex_shrink(1.)` on the two timestamp text children, preserving intrinsic width when space suffices, the centered row, natural height, existing body line-height and vertical padding. Adding `.flex()` is not a justified fix: `flex_row()` only sets direction, but default display already is flex. Centering's separate original failure remains unmeasured; do not claim the shrink correction closes it until the exact run produces coordinates.

Validation plan: retain the 32-turn Core settle cap, timestamp 24-turn cap, mounted 2-second/512-tick drains and 10-second convergence deadlines without extension. Add only failure-context geometry/line summaries to mounted assertions; no new copy, production diagnostics, weakened assertions or fixture mirror. Parent centrally rebuilds and executes the same exact two unit failures, all six focused cases and all four actual mounted cases after this lane stops writing. A fresh mounted failure must include widths, heights, native text boxes and line counts; no nominal Core result counts as mounted acceptance.

### Second-run implementation handoff: writes stopped

Exactly four Rust files and this document were written in this continuation:

- `ordinary_submissions/presentation/pixels.rs:21–44,58–69,183–230`: guard command and presentation owners before the display effect's signal access, layout revision updates, hover/pressed signal writes, press spawning, and async measurement reads/sends. The pre-await and post-await guards preserve the original revision lease and immutable snapshot payload. Actual disposed event callbacks return before reading signals or spawning work. Existing ordinary production tests and mounted fixture bytes are unchanged.
- `timestamps/presentation.rs:252–259`: add only `.flex_shrink(1.)` to native text construction. The root clamp, center justification, initial estimate, NSH natural-height reports, noninteractive mask, localized content, weights and padding are unchanged. This corrects a statically established production constraint; runtime natural-height acceptance is not yet verified.
- `timestamps/presentation/tests.rs:99–154`: remove only the extra root-measurement `expect`; actual day, suffix and separator geometry remains required. A bounded geometry line prints before the first geometric assertion (four locale/theme combinations maximum), including page/frame values and optional root. Original center, padding, accessible-content, no-button and natural-height assertions remain intact.
- `timestamps/presentation/mounted_regressions.rs:285–348`: only enrich three assertion messages with actual native/host/text bounds, line counts and padding. The long/narrow increase, padding and centering predicates are unchanged, as are input/focus assertions and all drain/readiness budgets.

Executed checks: scoped `/Users/puzige/.rustup/toolchains/1.98.0-aarch64-apple-darwin/bin/rustfmt --edition 2024 --config skip_children=true` and its `--check` form on those four Rust leaves (exit 0); Python UTF-8/final-newline/no-trailing-whitespace/no-ordinary-comment/no-allow-or-expect-suppression checks on all ten scoped Rust files (passed). Files were read after edits. No Cargo, rustc, build, test, network, Git mutation, shared/Cargo/vendor write, dependency, re-export or registration change. Compiler versions and new behavioral results must come from the parent's actual next central execution; none were obtained here. Earlier parent native/palette/source GREEN reports do not validate these new bytes.

Parent next verification, on freshly rebuilt binaries:

1. Exact `conversation_stream::ordinary_submissions::presentation::tests::production_press_retains_displayed_attempt_and_replaced_handlers_cannot_launder_authority --exact --nocapture`: require both original activations and the unchanged disposed-listener rejection without a disposed-signal panic. Preserve failure logs; no catch-panic or filtering of rejected events.
2. Exact `conversation_stream::timestamps::presentation::tests::production_separator_preserves_localized_text_weight_center_padding_and_natural_height --exact --nocapture`: collect the reliable printed coordinates even if the original center assertion fails. If still red, use those values to classify/fix the responsible layer; do not alter tolerances or shift expected coordinates to fit output.
3. All six existing C1/C2 unit cases, full UI group, and `nsh_ordinary_controls_integration` with `nsh-reading-integration`. Its previous actual execution was 0 pass / 1 fail / 3 unexecuted; require all four to execute, including actual pointer-through-full-inset wrappers, keyboard/clipboard/current-attempt dispatch, stale authority and unmount. Existing AX acceptance limits remain; do not claim native OS acceptance or independent review.

SHA-256; paths relative to `crates/vega_ui/`. Full ten-file identity includes unchanged parent fixes and fixtures:

| File | SHA-256 |
| --- | --- |
| `src/conversation_stream/ordinary_submissions/presentation/mounted_regressions.rs` (unchanged) | `3494fda898aae81d537c119c1089438992754c0dcec7cd96e182480f561db86c` |
| `src/conversation_stream/ordinary_submissions/presentation/pixels.rs` | `ae8b19137e8175b27882b7247c3c59307f312db90d31b5da4450b11ebb0c60a1` |
| `src/conversation_stream/ordinary_submissions/presentation/tests.rs` (unchanged) | `036aa7b55b5c3767f06bf5c0d1d87329f7bbaf1d52b6622d253771d0b9d3642a` |
| `src/conversation_stream/ordinary_submissions/presentation.rs` (unchanged) | `7e18e375d8c6df57191df4809f560e28e6c581805394bddcbfc09e0e51553279` |
| `src/conversation_stream/ordinary_submissions.rs` (unchanged) | `2aa5e125dd4eee2faed302bafc04d904c0c425fcb9b441382d31ee781c41e130` |
| `src/conversation_stream/timestamps/presentation/mounted_regressions.rs` | `98234869f190f9e7a78b439f1d525878504bbf5f625dd1a66fdce7c98270a8b9` |
| `src/conversation_stream/timestamps/presentation/tests.rs` | `4fde23c6d8d8923ff2b6f9f616431a714f104eeec4ab583eef8e388a3799da68` |
| `src/conversation_stream/timestamps/presentation.rs` | `696d560a4907bfa4de83c01c577cf15571db6c1f72b90115734382f0898b1964` |
| `src/conversation_stream/timestamps.rs` (unchanged) | `adf79874b380a90680b7c0a486d80f1fc5a240fe6f1632cd9b76c244d954bb82` |
| `tests/nsh_ordinary_controls_integration.rs` (unchanged, read/hash only) | `071bbc2c452b277e64f6b1e634bb62df44f9d7a1fd5210db1caf9712d0b012d6` |

Ten-file source identity: `f67302660135bed4d0ce81740e0b39eae45b6e9e852a61f4e6c92489406a670c` (`SHA256` of the sorted `sha256 + two spaces + repository-relative path + newline` manifest).

Second-run evidence SHA-256:

| File | SHA-256 |
| --- | --- |
| `target/nsh-closure-repair-build.json` | `809ca366eca58442e3d0c0b4bb01d7bcaf9e49cfaee3110f129f5eedc37b907a` |
| `target/nsh-closure-repair-build.stderr` | `0ff4fe5fce8c86fab8cb02d06491b5f23e1565007e095a3df9774fd15aef63a6` |
| `target/nsh-closure-repair-execution.json` | `6925a29d6853b20a0968eb82ae159cb67bf655fe9dba636e842f85275e0d8d5a` |
| `target/nsh-closure-repair-ordinary-press.stderr` | `980abbdbbdc3d124e1ad794f884e7138e07880b4a9e18fb8acd53cb63360cae4` |
| `target/nsh-closure-repair-ordinary-press.stdout` | `d4e07c279228cb97954939a8d1a56ab36c0b2f41f96bc3594b417b76cbd907e4` |
| `target/nsh-closure-repair-timestamp-center.stderr` | `686aff73efc90ce83fc20894bf1048ba8228113e745831912f0390784cf5cb0f` |
| `target/nsh-closure-repair-timestamp-center.stdout` | `a30ca4aa4ecaba4537a8869440990aa6936a973ba042ec0f4b19517fb4be4ed4` |
| `target/nsh-closure-repair-ordinary-mounted.stderr` | `905ce059950319ec167ccc37a548b48d8673970eb6cb61096d0fb42688fbf993` |
| `target/nsh-closure-repair-ordinary-mounted.stdout` | `5a67a03fb5316d9c49d989a51831ff7d5913e2524c62be02d84b609151fa2c6c` |

**Historical stop:** C2 stale-owner repair and C1 text-shrink repair were implemented but unexecuted. The original C1 center failure still needed measured-coordinate evidence from the exact fresh run. No post-delta PASS, completed mounted acceptance, native acceptance or independent review was claimed.

## Lifecycle execution continuation: observation acceptance fixed before edits

This section supersedes the preceding pending diagnosis, not the original product requirements. Parent lifecycle build succeeded (reported exit 0, 39.48 seconds); `target/nsh-closure-lifecycle-execution.json` records the two exact unit failures and mounted abort. Their unit executable SHA-256 is `95b595e95eb0e706a7616bcf12a5355908492bf91a09fd97bdba76e3255aabe5`; mounted executable SHA-256 is `e78eeb05f760f0cb4e7a79229a9af34e1497817f565385f3092ecdf6e7c0887e`. Existing historical manifests remain unchanged.

### Evidence and responsible layers

- C2 now reaches the final disposed-listener assertion without the disposed-signal panic. The test saves only numeric button IDs and later calls `Core::component_event` with them. Current lookup is explicit in `vendor/nsh-core/src/core/components.rs:4602,4646,4691,4715`: dispatch uses the current owner/handlers installed for that ID, not a queued callback identity. `Listener` retains its own owner (`vendor/nsh-core/src/element/builder.rs:20-63`), which a numeric ID does not preserve. Before declaring another production defect, the corrected fixture must retain the actual original press closures plus their presentation owners, prove those owners are disposed, and invoke those same closures under the live fixture runtime. A current-node press must separately demonstrate which displayed snapshot it actually targets; stale callbacks must still produce zero activations. Positive actions continue through real Core events, never direct invocation.
- C1 exact metrics: `Today` frame x=190,width=36; suffix x=226,width=65; separator width=480. The 101-point combined label has continuous left `(480-101)/2 = 189.5`. Core uses device-pixel edge rounding, with half ties toward the positive edge (`vendor/nsh-core/src/core/mod.rs:7425-7490`); at ratio 1 the exact expected left is 190, right 291. The old margin-difference `<1` assertion excludes this valid tie. Correct the observation to require exact pixel-snapped expected edges and adjacency, not a larger arbitrary tolerance; retain exact continuous centering when representable. No production offset, padding change, rounding API change or backend expansion is authorized or needed.
- C1 Core natural-height observation must match the actual text engine, not the initial GPUI estimate: the measured separator is 64, text height is 16, padding is 24 per side, while `initial_height()` is 68.15. `ApproxTextEngine::measure` uses `font * 1.2` (`vendor/nsh-core/src/text.rs:1038-1054`), not the requested body line-height multiplier. Preserve the initial estimate unchanged and assert the emitted natural height equals both actual separator height and actual maximum text height plus original vertical padding. This strengthens the actual measurement contract rather than accepting an arbitrary new height. The mounted real-font long/narrow height assertions already progressed past wrapping in this run and remain unchanged.
- Mounted C1 first fails the passive Tab assertion (`timestamps/presentation/mounted_regressions.rs:378`), then cleanup panics because `VisualTestContext::update` unwraps a removed-window error. Fixture handles at line 157 are not Tab stops. Mark only the fixture's before/after handles `.tab_stop(true)`, retaining the production passive Surface and both original focus assertions. Adopt the existing anchor fixture's fallible `update_window` cleanup: preserve normal cleanup errors as failures, but report rather than double-panic if the original assertion is already unwinding. Scheduler/readiness budgets remain 2 seconds/512 ticks/10 seconds.

### Minimal plan and required verification

Add only a development-test capture of the exact production press closure and immutable presentation owner; no alternate action authority or new public API. Correct the unit event observation, raster center invariant and natural-height observation as above. Correct timestamp fixture Tab eligibility and cleanup only; keep production behavior and all other mounted cases intact. Then static-check and freeze exact hashes for the parent to rebuild and execute both exact failures, all six unit cases, the UI group and all four mounted cases. Compilation/tests remain parent-only; AX unavailability is a separate boundary, not success evidence. No independent review or full native acceptance is claimed.

### Implemented lifecycle-observation delta and frozen handoff

Only four owned Rust files and this document changed in this continuation:

- `ordinary_submissions/presentation/pixels.rs:151,169,198`: name the unchanged production press closure and, only under `cfg(test)`, retain the same closure through an Rc wrapper with its immutable presentation ID/owner/action. Production owner guards before signal access/spawn and after measurement are preserved. Feature-gated layout observation remains before the snapshot move. No current-state authority reconstruction, second control or new visible copy.
- `ordinary_submissions/presentation/tests.rs:82-154,370-497`: capture the original closures before retry, replay real MountItem props/deletes to observe current button IDs, assert new presentation owners, and separately press old numeric IDs through current lookup. Require exactly one current-snapshot activation for each reused current ID and reject laundering of the old snapshot. Replay every saved original closure under the fixture's live Core task runtime; require an acknowledgement after every callback returns, require all original owners already disposed, and retain the exact `disposed listeners cannot activate` zero-activation assertion. The acknowledgement makes a task panic or unexecuted callback fail rather than masquerade as rejection. No additional public export or registration.
- `timestamps/presentation/tests.rs:120-133,159-173`: exact positive-half pixel-snapped center, right edge and day/suffix adjacency; exact natural report/separator height and actual text-height-plus-original-padding equality. No arbitrary tolerance enlargement. Localized text, weights, color, padding and passive assertions remain. Production timestamp files are byte-identical to the preceding stop.
- `timestamps/presentation/mounted_regressions.rs:157,264-283`: fixture-only Tab stops and fallible window cleanup. Original natural-height, centering, passive pointer/wheel and both focus assertions remain unchanged. Normal cleanup failure still fails; an already-unwinding assertion is no longer replaced by a cleanup abort. Drain/readiness budgets are unchanged.

Classification: the new lifecycle failures are observation/fixture defects based on the exact current routes and logged geometry; no additional production behavior repair is supported by this evidence. The prior C2 owner guards and C1 shrink repair remain. Numeric-ID reuse itself is statically supported by current-tree dispatch but its exact old/new ID values are not yet observed in this lane: the next parent run prints both lists and validates replacement snapshot identity. It must also execute the saved-closure rejection path; neither result is claimed in advance.

Executed in this continuation: explicit Rust 1.98 toolchain `rustfmt --edition 2024 --config skip_children=true` on the four changed Rust files, then `--check` over the ten-file source set (exit 0); Python static checks over all ten for UTF-8/newline/whitespace, no comments/suppressions, six unit cases, unchanged 2-second/512-tick/10-second mounted budgets, and retained production owner/measurement/revision guards (exit 0). Source was read after edits. No Cargo/rustc/version query, build, test, network, Git mutation, shared/Cargo/vendor write or dependency change. Parent's prior source15/anchors6 GREEN reports remain historical and do not validate this new identity.

Frozen SHA-256, paths relative to `crates/vega_ui/`:

| File | SHA-256 |
| --- | --- |
| `src/conversation_stream/ordinary_submissions/presentation/mounted_regressions.rs` (unchanged) | `3494fda898aae81d537c119c1089438992754c0dcec7cd96e182480f561db86c` |
| `src/conversation_stream/ordinary_submissions/presentation/pixels.rs` | `3f1e1ef6ece0e0c0d39c00eccc0d50d7ccb1cc2d56cf7bf9fbd3ebf398b13e1d` |
| `src/conversation_stream/ordinary_submissions/presentation/tests.rs` | `415720cd42d7adbf5124a21beca791ce34439212e316fe4d157268e4252d2670` |
| `src/conversation_stream/ordinary_submissions/presentation.rs` (unchanged) | `7e18e375d8c6df57191df4809f560e28e6c581805394bddcbfc09e0e51553279` |
| `src/conversation_stream/ordinary_submissions.rs` (unchanged) | `2aa5e125dd4eee2faed302bafc04d904c0c425fcb9b441382d31ee781c41e130` |
| `src/conversation_stream/timestamps/presentation/mounted_regressions.rs` | `7834cb392c864aeba8f865b97d008a0dfb8cad43872cc771593edf50cb83e666` |
| `src/conversation_stream/timestamps/presentation/tests.rs` | `ab9bf3ff29a0ca440cb339d04d49b2ec5fdef6d872e3ead6580bb0b9fc231f65` |
| `src/conversation_stream/timestamps/presentation.rs` (unchanged) | `696d560a4907bfa4de83c01c577cf15571db6c1f72b90115734382f0898b1964` |
| `src/conversation_stream/timestamps.rs` (unchanged) | `adf79874b380a90680b7c0a486d80f1fc5a240fe6f1632cd9b76c244d954bb82` |
| `tests/nsh_ordinary_controls_integration.rs` (unchanged; hash/read only) | `071bbc2c452b277e64f6b1e634bb62df44f9d7a1fd5210db1caf9712d0b012d6` |

Ten-file source identity: `5e739d442555a495fa6c104350d2bafc7e6b3b13667644723feb5b6b69c70a10` (same sorted repository-relative manifest algorithm as prior sections).

Lifecycle evidence SHA-256:

| File | SHA-256 |
| --- | --- |
| `target/nsh-closure-lifecycle-build.json` | `7fc23ed0c4bb83ca2a45ad80be8d696ed398e54ebe8f7a7b24ee1c5b15f7c8e4` |
| `target/nsh-closure-lifecycle-build.stderr` | `2c8af58cabd209554dfd2c06c9b8bb4187c336995fc8401aa558534c923a9b04` |
| `target/nsh-closure-lifecycle-execution.json` | `96bced7d225daf6d89cdc1edab297195de935190ba47bd84081db809f72c9855` |
| `target/nsh-closure-lifecycle-ordinary-press.stderr` | `115f549c87044c0fc8f2f8cb57d2bf2d6e439d98bf779662fde24e18d3168871` |
| `target/nsh-closure-lifecycle-ordinary-press.stdout` | `1e26794b23010bfea897de3a4bbd5438d453f43d18ef2b915f3d9607ae8767a7` |
| `target/nsh-closure-lifecycle-timestamp-center.stderr` | `6833fa668d70a706185a19daf0796533f26fe09c16828b3b591c4d12f940f3e2` |
| `target/nsh-closure-lifecycle-timestamp-center.stdout` | `09b57d6ca2c31582637578e185003fa6afbab66d23bbad80d2d6be875422ac7d` |
| `target/nsh-closure-lifecycle-ordinary-mounted.stderr` | `b98dbf664df574387a60d751fecd3ed019d206b969e35a5343aa7823f046116f` |
| `target/nsh-closure-lifecycle-ordinary-mounted.stdout` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |

Parent next checks, in order:

1. Rebuild final bytes with current central toolchain; no registration/export request in this delta.
2. Run exact `conversation_stream::ordinary_submissions::presentation::tests::production_press_retains_displayed_attempt_and_replaced_handlers_cannot_launder_authority --exact --nocapture`. Inspect printed old/new IDs and require all current-snapshot dispatch assertions, callback acknowledgements, disposed-owner observations and zero stale activation.
3. Run exact `conversation_stream::timestamps::presentation::tests::production_separator_preserves_localized_text_weight_center_padding_and_natural_height --exact --nocapture`. Require both locales/themes, exact snapped geometry and native-height equality; retain raw diagnostics.
4. Run all six C1/C2 unit cases and full UI group, then all four `nsh_ordinary_controls_integration` cases. Require timestamp passive Tab to pass without cleanup abort and all three previously unreached ordinary cases to execute, especially real pointer pass-through/full-inset overlays, displayed/layout convergence, keyboard/current-attempt clipboard and stale-authority rejection. Existing AX observation unavailability stays separately unresolved; no simulated positive path substitutes for it.

**Writes stopped. This continuation is implemented and statically checked, not behaviorally executed. No post-delta PASS, completed mounted/AX/native acceptance or independent review claimed.**
