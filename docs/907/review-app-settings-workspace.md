# #907 — Consolidated independent review: app / settings / workspace

## Decision

**One actionable P2 finding. Changes requested for that finding; no implementation performed.** No additional actionable finding was established in the bounded review below. This is a non-author, read-only source review of the stopped implementation, not full-product approval or three-desktop native acceptance.

## Exact reviewed version and integrity

- Worktree: `/Users/puzige/Workspace/worktrees/vega-907-nsh-full`
- Branch: `feat/907-nsh-full`
- HEAD: `0b5ea426abfd8d4358489e916273d89474fb190f`
- Migration comparison base: `34555cec9c0c4c4c1f7a528d739752f414e53edf`
- Declared integrated mainline: `2a29adf9b2ea78eaf59d591838531b45adc7373e` (#916 / #920 / #922 dirty-byte integration). This identifies the supplied integration context, not a claim that HEAD has that Git ancestor.
- Frozen content manifest: `target/nsh-implementation-review-content.json`, **2137 files**, excluding pycache.
- Manifest content identity: `46c7fbd372131b277956f6818af7def7ba8ce1b3912ce8fca377267fea78a3dc`.
- Manifest file SHA-256: `9b5d3ccf193668c12833165b216c842d65d8848bf056a84a7d02b6b32c96d71c` (distinct from its content identity).
- Frozen tracked binary patch: `target/nsh-implementation-review-tracked.patch`, SHA-256 `6833922e3f600349fe53838137f93ca893de87c2f80a707dfd75c9a0c97eb5ad`.

Before review and immediately before writing this report, every manifest file was independently hashed against the manifest: **2137 checked, zero missing/changed files**. Recomputing the manifest's documented sorted-path/hash JSON algorithm yielded the specified identity. The live `git diff --binary HEAD` also hashed to the frozen tracked-patch hash. Before this report there were no extra non-pycache tracked/untracked file paths outside the manifest. HEAD and branch matched the supplied version. Final read-back/integrity verification accompanies delivery; this new report is intentionally not part of the frozen implementation manifest.

HEAD alone does not identify these substantial dirty/untracked implementation bytes. Findings and line references apply to the frozen content, not merely that commit.

## P2 — Reject queued General-settings commands after Settings closes

**Primary location:** `crates/vega_ui/src/settings/nsh_input_preferences.rs:86-100`.

**Same defect in the sibling receiver:** `crates/vega_ui/src/settings/nsh_appearance.rs:78-88`.

### Concrete scenario and impact

While General settings is open, a user activates a prediction/Queue/Steer or appearance control and then closes settings or navigates back to a task before the NSH request is consumed on the main thread. When that old request is subsequently received, both receivers can still accept it and mutate preferences after the page is hidden. This is a source-established asynchronous ordering defect; no native reproduction was run here.

The receiver checks `this.section == 1`, the same retained presentation ID, and the relevant observed preference value. Closing the settings page does not change `section`, the presentation ID, or those values. Neither receiver checks the current global `SettingsOpen`, and neither snapshot contains a visibility flag:

- Input preferences snapshot/acceptance: `nsh_input_preferences.rs:17-23,42-58`; accepted requests invoke the original save handlers at `:101-108`.
- Appearance snapshot: `nsh_appearance.rs:44-54`; accepted requests call theme/language save or dispatch sidebar toggle at `:89-98`. Its pure `accepts` function checks enabled/current values, not visibility.
- Closing settings only cancels provider/OAuth/Skills/preparation activity in `settings/state.rs:174-184`; it does not revoke these two presentations.
- The app releases the settings entity only in the ordinary conversation-render branch, `crates/vega/src/window/render.rs:474-481`. In particular, `:471-472` enters conversation loading without taking `settings_view`, so entity destruction cannot be relied on as the authorization boundary. Even on an ordinary close, there is a pre-render interval with `SettingsOpen == false` and the old entity alive.
- The downstream `settings/preferences.rs:7-22,48-59` protects saving/conflicts, but does not reject a hidden settings owner. It can start the persisted preference change. This is not merely a stale local selected-state display.

The neighboring default-model implementation already checks current `SettingsOpen` in `settings/nsh_default_model.rs:61-74`; shell navigation likewise derives visibility in `settings/nsh_shell.rs:79-103`. The missing check in these two receivers therefore leaves a real gap in the migration's old-command/page-lifetime contract, rather than requiring a new business rule.

### Requested correction and verification

Recheck current settings visibility at the trusted receiver for **both** surfaces (or use an equivalent current-owner/visibility fence), before any business mutation or dispatched sidebar action. Preserve existing ID, observed-value, busy-state, conflict and failure recovery guards. Do not cancel or roll back a save already accepted while visible, and do not replay rejected commands.

Focused regression expectation for the implementation owner: retain a valid General-settings entity and captured current request, set `SettingsOpen(false)` without changing section/preferences (also cover a retained loading-route owner), and deliver the queued request. Expect a negative acknowledgement and **no preference save/global mutation/sidebar toggle**. The same request against a visible current owner should still work, and existing stale-value/busy guards should remain covered. This review did not author or run those regressions.

## Scope, references, and production-path checks

Read the applicable root `AGENTS.md`, execution guide, design guidelines and issue workflow, including the bounded independent-review ending rule. No nested `AGENTS.md` was found. The current explicit prohibition on network/Git mutation overrides the general fetch/update workflow for this review; no fetch or update was attempted.

Acceptance/reference inputs included relevant sections of `docs/907/nsh-full.md`, the original inventories `nsh-settings-production-closure.md` and `nsh-shell-workspace-production-closure.md`, `nsh-sidebar.md`, `nsh-single-line-native-policy.md`, `nsh-workspace-mounted-validation.md`, and the workspace/window-navigation/startup scope contracts. Historical author inventories were used to locate production chains, not treated as this independent review or as runtime evidence.

Reviewed the migration delta against the base and the frozen dirty implementation, then followed affected production roots and selected high-risk handlers. This was not an unrelated whole-code audit. Main checks:

| Area | Static observations and bounded result |
| --- | --- |
| Production settings | The real root is `impl Render for SettingsView` in `settings/render_impl.rs`, dispatching the actual sections at `:253-275`; the disconnected historical `nsh_settings.rs` renderer is not the product root. NSH page renderers construct visible controls; Fast parents arrange/scroll/host them. Provider/MCP draft claims and secret-only command handoffs, navigation/draft and permission-save guards, real default-model state, and Agents preparation/service actions were inspected at their affected boundaries. The finding above is the remaining concrete defect established here. |
| App shell / window navigation | Followed `window/render.rs` through shell chrome, permission-reset notice, shared navigation and command-palette presentation. Current observations, root/owner/mount/route checks and the original action handoffs remain present in inspected handlers. NSH render functions construct icons, labels, buttons and errors rather than embedding old painted product controls. |
| Sidebar / rename / DnD | Current organization entry uses the pinned/projects/recents production projection. Rename mounts `TextInput::render_nsh_single_line` through `threads_block/rename.rs:112-138`, not the old GPUI editor. The session subscription checks source/session/input/thread/row/project/navigation/route, focus, editor readiness and native shortcut identity at `:32-90` before original commit/cancel. Reorder retains native gesture/focus and clipped-bounds drop decisions, with NSH insertion/drag visuals; the measurement-only canvas has no painting callback. Native editing backend implementation remains assigned to the other review, not independently certified here. |
| Workspace / panels | Followed actual workspace snapshot/action/receiver and controller/separator paths. Pane/tab/route authority, live mount, measured visible keyboard/AX targets, presentation-only reload and original tab/terminal actions remain in inspected paths. Raw drag extents/display clamps and fullscreen/separator decisions remain in the extracted production controller. Branch/commit/diff panel admission and original action routes were sampled, not reimplemented. |
| Content preview / terminal | File preview is read-only NSH content; Markdown images render as alt text and links pass the existing trusted classification. Image actions retain presentation/revision/application identity and original open/save handoff. Terminal commands retain session epoch, input sequencing, paste bounds and original PTY service. This does not certify OS clipboard, file dialogs, device permissions or background PTY behavior at runtime. |
| Shared icons / tooltip / passive painting | Actual icon rendering enters `icons/nsh.rs`; tooltip Fast elements measure/host while NSH draws product content. The old painted icon-button helpers had no production callsites in the searched app/UI sources. Settings group/rule/text helpers use NSH leaves; transparent borders and inherited text styles are not evidence of Fast product painting. Workspace root/sidebar backgrounds and 1px separator pixels use passive NSH Surfaces, retaining the 5px native drag region and stop-on-release lifecycle. |
| Startup / recovery overlays | Startup mounts the real NSH view. Main-thread dispatch requires current root and rendered-frame/mount/state fences; UI-only recovery reload is separated from retrying startup work. Recovery/permission notice action routes inspected here retain current identity and existing service handoffs. Native compositor presentation is not established by engine acknowledgement. |

No further concrete live GPUI/Fast product painter was established in these inspected production closures. This is not a blanket proof that every state or pixel is migrated. Dormant organization/group projections, unused helpers and disconnected demo code were not misclassified as reachable product gaps. No unrelated finding was promoted into implementation or a new investigation.

## Validation and limitations

**Performed by this reviewer:** read-only source/reference/delta inspection, focused symbol/reachability searches, read-only Git identity/diff inspection, full manifest hashing, frozen/live tracked-patch hashing, and writing/reading this report only. No implementation history from another agent session was loaded; no child was delegated.

**Not performed:** Cargo/rustc/toolchain invocation, compilation, tests, formatter or Clippy execution, application launch, input injection, screenshots, network/CI queries, source edits, Git mutation, installation or machine configuration changes.

The user's supplied current evidence is eleven green targets: UI 358, host 36, geometry 24, reading/rename 15, scroll 16, icons 3, settings 3, recovery 3, anchors 6, palette 3, ordinary 4; strict workspace/all-target Clippy and fmt are also reported green. The retained `target/nsh-implementation-closure-final-clippy.stderr` ends in successful completion at 12.73s, consistent with that report. An earlier similarly named Clippy log contains a superseded failure and was not treated as a current product failure. These are supplied/retained results, **not checks executed by this reviewer**. No aggregate pass or current app/workspace additional-regression outcome is inferred; those were parent-run/pending in the request.

Static review does not establish actual macOS/Windows/Linux native painting, cross-Surface Tab order, IME candidate routing, focus/AX behavior, DnD, clipboard fidelity, file-picker/open-application completion, device-access permission behavior, clipping/scrolling or native pixels. Historical dependency/IME limitations are not silently marked solved or converted into newly discovered backend defects. Conversation/tool/native-backend correctness is assigned to other reviewers and excluded, except the directly affected public contracts needed to understand these app paths.

This report is the independent review result for this frozen implementation only. It is not final-head CI, Rust development-test archival/removal approval, merge/release approval or user handtest acceptance. Any subsequent implementation change needs affected-delta review and exact-version verification by the responsible owner; optional suggestions do not reopen unrelated investigations.

**STOP:** only this new report is written. Implementation remains stopped; the P2 correction and focused verification belong to the implementation owner under separate authorization.

---

## Incremental non-author review — queued General-settings close fix

### Decision

**The original P2 is closed for the exact source bytes below. No remaining actionable defect was established in this bounded fix and its affected contracts.** This supersedes only the original P2's changes-requested status, not the historical findings/evidence or the original review's wider limitations. No implementation was performed. Compilation and execution of the new regression remain the parent's responsibility; this is static review completion, not a runtime pass or merge approval.

### Exact reviewed delta and integrity

Worktree, branch and HEAD remain `/Users/puzige/Workspace/worktrees/vega-907-nsh-full`, `feat/907-nsh-full`, `0b5ea426abfd8d4358489e916273d89474fb190f`. Review is bound to these dirty bytes, not HEAD alone:

| Path under `crates/vega_ui/src/settings/` | Reviewed SHA-256 |
|---|---|
| `nsh_input_preferences.rs` | `6ea31883c09ccdc08610862d70c18f613851554494aba53e544064d1bd6227fe` |
| `nsh_appearance.rs` | `9e2259bcbe1885ddfc81bbd941377ad7ef622d7fe7ca6ef4460cae6ab430af72` |
| `state.rs` | `310cdab75503b9f5c703f555884631e33e8bb6b1162694038c68a175f48da726` |
| `nsh_close_regressions.rs` | `895b5dcc31ac0582d6b7369bd436da2f6412f32e79130d2089ed524dd490722e` |
| `nsh_appearance_close_support.rs` | `1a143e09dc8541d22fa8e68b676e9efb8d9e8b149e2b656887c3e850def3fc1e` |

The implementation note `docs/907/nsh-review-settings-close-fix.md` hashes to `84adff5768b7c05abd26339a37156b57f7d43d9749c4c69efa12d2081ec8070e`. The parent runner `crates/vega_ui/tests/nsh_settings_integration.rs`, inspected only for this regression's wiring, hashes to `3ec7c44f211b0173c8c0194554934e993bd13dff35547351d6cd3cebdf682b1f`; it retains the original three cases and appends the public SettingsView entry as case four. This proves source wiring only, not compilation, discovery or execution.

The original manifest remains SHA-256 `9b5d3ccf193668c12833165b216c842d65d8848bf056a84a7d02b6b32c96d71c`; independently recomputing its 2137 sorted path/hash pairs yields identity `46c7fbd372131b277956f6818af7def7ba8ce1b3912ce8fca377267fea78a3dc`. The retained original tracked patch remains SHA-256 `6833922e3f600349fe53838137f93ca893de87c2f80a707dfd75c9a0c97eb5ad`. These are retained baseline artifacts, not a claim that the live post-fix diff still equals the original patch.

Independently reversed the actual bounded additions **in memory only**, rather than relying on the author's assertion. Removing the two revoke methods, development module declarations/observation calls, consumption-time visibility checks, and the state observer's six added lines reconstructs the exact original three file hashes:

- Input preferences: `1a5fd80537cc1ed7098d29f52beefdf7235742a914813d1d4cd0c1bb35474346`.
- Appearance: `5edeb9a3c6cf651e03c0300a3427f6c1e08ef18c5b8014c8a3597cf691bd2525`.
- State: `c0cf16efcb0249bbce272eb78a93074ca95d85d94ba14587f98e5d737e495ab2`.

Thus the actual production delta is established against frozen raw bytes, including the unchanged handler/guard code, without applying a patch or writing source. Whole-manifest hashing before review and again before this append found zero missing files and nine differences: the three reviewed production paths; the parent's settings runner; and five out-of-scope paths (`sidebar/threads_block/rename_tests.rs`, `text_input/editing_core.rs`, `text_input/native_adapter.rs`, `text_input/nsh.rs`, `docs/907/nsh-reading-editing-integrated-validation.md`, all under their original crate/docs roots). Their hashes were stable across those observations. These concurrent-domain differences were recorded for identity only, not audited or treated as new findings. The five-file fix matches the supplied hashes.

### Why the original P2 is closed

1. **Current visibility is checked at consumption, before effects.** `nsh_input_preferences.rs:95-124` and `nsh_appearance.rs:87-114` now require an existing `SettingsOpen(true)` before section, presentation identity and existing value/availability checks can authorize a command. Closed or missing visibility fails closed and takes the original negative reply path; neither save nor sidebar dispatch is reached.
2. **Retained-entity reopen does not revive the old presentation.** `state.rs:174-191` revokes both presentations when the existing SettingsOpen observer sees closed state. Their revoke methods (`nsh_input_preferences.rs:16-20`, `nsh_appearance.rs:15-19`) replace the identity without dropping the entity or altering an accepted save. `nsh.rs:19-35` defines a fresh `Arc` identity with pointer equality; old queued snapshots retain their old allocation, so equal preference values after reopen do not restore authorization. Existing render/publish paths expose the replacement identity.
3. **The affected update ordering was checked, not assumed from the observer name.** Cached dependency source for the declared `gpui-fast = 0.1.9` shows global writes enqueue deduplicated notifications, effects flush at App-update completion, and observers read current state (`src/app.rs:1963-2007,2128-2133,2416-2421`; `src/app/context.rs:168-181`). The inspected production close action (`crates/vega/src/window/navigation.rs:103-121`) and shell reopen action (`shell_chrome/mod.rs:413-423`) are separate update callbacks, allowing close notification/revocation before a later reopen request. This is not a claim that an artificial false→true pair within one unflushed App update would expose the intermediate false value: GPUI coalesces it. No such production sequence was established in the affected paths, so that unsupported sequence is not promoted into a finding.
4. **Existing business authority remains unchanged.** The verified baseline reconstruction establishes no edits to section/value checks, saving/default-permission-IO availability, original preference-save/theme/language handlers, or ToggleSidebar dispatch. Sidebar remains independent of preference-saving availability. Provider/OAuth/Skills/preparation close cleanup remains after the new revocations; no cancellation or rollback of already accepted preference saves was added. No visible product text, painter, input/device-access route or permission grant was introduced.

### Regression review and limits

The new feature-gated regression is not a duplicate pure predicate: `nsh_close_regressions.rs:14-48,59-94,104-165` captures the actual rendered Surface command senders and retains a real `SettingsView` under a parent entity. `:288-345` enqueues prediction, Queue/Steer, theme, language and sidebar requests, closes before consumption, requires negative replies, verifies unchanged disk/config/globals/save-event/sidebar counts, and reopens the same entity with equal values before retrying old snapshots. Both ordinary-retained and preview-hidden parent states are authored. The appearance helper `nsh_appearance_close_support.rs:19-68` uses its original production queue and snapshot rather than replacing authorization.

The remainder (`nsh_close_regressions.rs:346-459`) retains busy/wrong-section/stale-value rejection coverage and authors fresh accepted prediction/running/theme/language/sidebar requests, including sidebar while preference saving is busy. The accepted running-message save is followed by close and checked on disk; it does **not** deterministically suspend the worker at the close boundary. The original production IO guard is unchanged; no new deterministic IO-pending scenario or full-app sidebar persistence coverage is claimed. The parent sidebar fixture counts the real dispatched action but is not the full application controller.

**Executed in this review:** focused read-only source/reference inspection, read-only Git HEAD/branch/status inspection, SHA-256 and manifest-identity calculations, in-memory reverse-delta reconstruction, and this report-only append/read-back. No child delegation or implementation-session history was used. No Cargo/rustc, test, formatter, Clippy, application launch, input injection, network/CI, Git mutation or source write was performed. The current user authorization overrides repository-default fetch/update requirements for this stopped read-only review.

**Unobtained by this reviewer:** compilation and execution of these new bytes and the parent's fourth settings case, fresh related regression results, final-head required CI, and actual macOS/Windows/Linux native acceptance. Earlier green settings 3/3 and the other baseline checks predate the fix and are not evidence that this delta runs. No new pass count is asserted. Development Rust fixtures, feature-only observers/module wiring and parent runner wiring still require archival/removal under the repository's pre-master lifecycle; that is not a reopening of this bounded correctness review.

**Incremental STOP:** original P2 closed by static independent review of the exact hashes above; no remaining actionable finding in this delta. Only this existing report was appended. Source implementation remains untouched and stopped; runtime validation and remaining delivery gates belong to the parent.
