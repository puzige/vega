# #907 — General-settings queued-command close fix

## Scope and acceptance (written before implementation)

Implement the P2 in `review-app-settings-workspace.md`, not an independent review. Supplied worktree HEAD: `0b5ea426abfd8d4358489e916273d89474fb190f`; supplied integrated mainline: `2a29adf9b2ea78eaf59d591838531b45adc7373e`; supplied pre-fix frozen content identity: `46c7fbd372131b277956f6818af7def7ba8ce1b3912ce8fca377267fea78a3dc`. These identities are supplied context, not freshly queried Git state.

1. Both input-preference and appearance trusted receivers must reject requests when current `SettingsOpen` is false, before save, config/global/theme/language mutation or sidebar dispatch.
2. Closing settings must retire the two presentation identities even if the same SettingsView remains alive (including a parent retaining it while rendering loading/preview content). Reopening must not authorize pre-close requests with unchanged observed values (ABA).
3. Current, visible General-settings requests continue through their original business handlers. Retain section, identity, observed-value, saving and permission-IO guards; sidebar remains independent of preference-saving availability. No new product copy or painted controls.
4. Do not cancel or roll back saves accepted while visible; preserve provider/OAuth/Skills/preparation close handling.
5. Development regression must use an actual retained SettingsView and its actual asynchronous receiver queues, not just pure `accepts` tests. Enqueue current requests, close before consumption, require negative replies and unchanged disk/config/globals/sidebar; reopen the same entity, reject old requests and accept fresh requests. Preserve stale-value, wrong-section and busy coverage.

## Minimal implementation plan

- Add the current visibility fence to both existing receiver decision branches.
- Revoke only their presentation identities on the existing SettingsOpen close observer; do not alter in-flight business saves or other settings lifecycles.
- Add feature-gated exclusive development fixtures/queue access under the existing `nsh-reading-integration` feature. Publish a callable development entry on SettingsView if needed; propose external runner wiring to the parent rather than edit shared files.
- Perform permitted source/format/hash checks only. No Cargo, Git, network, dependencies, vendor, shared runner or other source writes. Parent owns central compilation/execution and non-author incremental review.

## Implementation status

Acceptance and plan were recorded before implementation. Implementation is written; compilation and runtime validation are **not run**. Existing supplied green runs predate this fix and do not validate these bytes.

### Production delta

- Both existing `Request::Decide` branches now require current `SettingsOpen(true)` before the original section/identity/observed-value/availability checks and business handler calls. The original negative acknowledgement path remains intact.
- Each presentation has a small `revoke` method that replaces its `ProjectionId`. The existing SettingsOpen close observer invokes both methods. Surface and entity retention are unchanged; a later render publishes the new identity through the original Surface. An old renderer snapshot therefore cannot become current again just because settings reopens with equal values.
- No preference-save worker, provider/OAuth/Skills/preparation cleanup, sidebar dispatch implementation, failure copy, disabled control, theme/language handler or permission-IO guard was changed. Already accepted saves are not newly cancelled or rolled back.
- No new visible text or GPUI-painted product controls. Development observation is feature-gated and does not change command dispatch or authorize commands.

### Development regression authored, not executed

`nsh_close_regressions.rs` adds `SettingsView::queued_general_commands_are_revoked_on_close(&mut gpui_kit::TestAppContext)` under the existing `nsh-reading-integration` feature. `nsh_appearance_close_support.rs` provides exclusive appearance queue observation. Both capture actual renderer snapshots and actual `mpsc::Sender<Request>` instances; requests reach the original asynchronous receiver loops. No pure acceptance-function substitute or synthetic affirmative reply is used.

The fixture retains one real SettingsView while testing both a still-mounted owner and a retained owner omitted from a loading/preview-like parent render. It enqueues prediction, running-message, theme, language and sidebar requests before closing, requires false replies, checks unchanged temporary config bytes and in-memory config, zero SettingsSaved events/sidebar actions, unchanged actual prediction/running/theme/language/sidebar globals, then reopens the same entity and requires old snapshots to remain rejected. Fresh commands exercise original saves and successful persisted/global results. It additionally covers saving-disabled preferences, wrong section, stale prediction/theme/language/sidebar snapshots, sidebar dispatch while preference saving is busy, and completion of an accepted running-message choice across a subsequent close. The existing pure stale-value/busy tests remain untouched. The accepted-save case does not claim deterministic worker suspension at the close boundary.

The sidebar fixture counts the actual dispatched ToggleSidebar action and updates its isolated app global at the parent action handler; it is not the full application persistence controller. The retained-owner parent is a bounded fixture, not an actual app loading-route/native-acceptance claim. Real pointer/AX acceptance is not claimed: this regression targets the actual queued-command consumption boundary.

### Required parent wiring / API packet

Shared runner is deliberately unchanged. In `crates/vega_ui/tests/nsh_settings_integration.rs`, import `vega_ui::settings::SettingsView`, change `[MountedCase; 3]` to `[MountedCase; 4]`, and append:

```rust
(
    "queued_general_commands_are_revoked_on_close",
    SettingsView::queued_general_commands_are_revoked_on_close,
),
```

No `settings/mod.rs` re-export, dependency or feature addition is needed: SettingsView is already public and the associated method is compiled only under the existing integration feature. Keep all three existing cases, assertions, deadlines and failure reporting. Parent should centrally compile/run the four-case settings target, affected UI/app regressions and required formatting/Clippy checks, then obtain non-author incremental review. This implementation owner did not modify or run that runner.

### Performed verification

- Scoped `rustfmt --edition 2024 --config skip_children=true` and subsequent `rustfmt --check` on the three authorized existing files and two exclusive new fixture files: successful. Formatter: `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`.
- Read-back of changed production paths and new fixtures; actual Surface/SettingsView/config/global APIs inspected.
- Read-only Python SHA-256/diff check: reversing exactly the declared additions in each of the three existing production files reproduces its pre-fix frozen-manifest hash. This proves their bounded delta relative to the supplied frozen bytes, not integrity of other agents' concurrent files. New fixture files were checked for comment-line additions; none found.
- No Cargo, Git, network, vendor/dependency, shared-runner or user-configuration operations. Cargo/rustc versions were not queried. Runtime assertions, compilation, Clippy, full-workspace fmt, CI, independent review and native user acceptance remain unrun/unobtained by this author.

### Exact scoped SHA-256

| Path under `crates/vega_ui/src/settings/` | Frozen pre-fix | Implemented |
|---|---|---|
| `nsh_input_preferences.rs` | `1a5fd80537cc1ed7098d29f52beefdf7235742a914813d1d4cd0c1bb35474346` | `6ea31883c09ccdc08610862d70c18f613851554494aba53e544064d1bd6227fe` |
| `nsh_appearance.rs` | `5edeb9a3c6cf651e03c0300a3427f6c1e08ef18c5b8014c8a3597cf691bd2525` | `9e2259bcbe1885ddfc81bbd941377ad7ef622d7fe7ca6ef4460cae6ab430af72` |
| `state.rs` | `c0cf16efcb0249bbce272eb78a93074ca95d85d94ba14587f98e5d737e495ab2` | `310cdab75503b9f5c703f555884631e33e8bb6b1162694038c68a175f48da726` |
| `nsh_close_regressions.rs` | New exclusive development fixture | `895b5dcc31ac0582d6b7369bd436da2f6412f32e79130d2089ed524dd490722e` |
| `nsh_appearance_close_support.rs` | New exclusive development fixture | `1a143e09dc8541d22fa8e68b676e9efb8d9e8b149e2b656887c3e850def3fc1e` |

The new report's own hash is supplied in the final handoff to avoid a self-referential hash. Rust development fixtures/observers and runner wiring must be archived and removed before master delivery under the repository lifecycle policy; production close fences must remain.

**STOP READY:** only the three authorized production paths, two new exclusive development fixtures and this new report were written. Parent central build/execution and non-author incremental review are pending; no integrated/tested/native-accepted claim.
