# #907 — NSH settings navigation and unsaved guard

## Authority and acceptance (recorded before implementation)

Authorized by the full migration contract in `nsh-full.md` and the current independent implementation scope. Baseline HEAD is `9b9541eb88274e1020ca024a2d45fbb80a72720e`; original `model_configuration.rs` SHA-256 is `18e77c0fd7c953f5bd7dbe750231555b4a105abe6c1911e2ad2b0d52169921bf`. Preserve the previously removed unused settings-help implementation. Automation is stopped and outside this scope.

- Replace all visible GPUI navigation guard, model header and route-button content with Rust NSH 0.4 controls through the existing `crate::nsh::Surface<Snapshot, Request>`.
- Preserve Back through `request_settings_section(0)` and Basic/Reasoning/Pricing through `request_settings_destination(ModelSection(..))`; current tabs and unavailable Basic are disabled. Preserve provider/model title, pricing library and reasoning chooser labels, narrow wrapping, native keyboard activation, visible focus, theme and localization.
- Extract the original Save/Discard/Stay handlers into controller methods, without reimplementing their business choices in NSH. Preserve pricing retry versus current shared-draft submission, capability/reasoning/provider saves, busy disabling, error-blocked resume and close dispatch only after real navigation completion.
- Every decision is bound to the displayed surface identity, route, pending destination/close intent, availability, current authority/base/operation and draft session/revision. Reject stale/hidden requests and duplicate submissions; provider key content is never in a snapshot.
- Pricing submission atomically claims the exact shared NSH draft, not the GPUI input carrier; composing/frozen/newer drafts cannot be saved. Confirmed discard atomically invalidates the observed pricing draft before detaching it. Failure preserves the original controller's draft and error.
- No new discard shortcuts or automatic success feedback. Stay cancels navigation only. Errors remain in the existing settings error presentation.

## Implementation plan and integration boundary

Only `settings/model_configuration.rs`, new files below `settings/model_configuration/`, and this document may be written here. Main owns state/render wiring, compilation, tests and CI. No git mutations, builds, subagents or application execution.

Use two optional presentations on SettingsView, one for the guard and one for model navigation. NSH and GPUI contexts remain isolated through snapshots/commands and the existing Surface. Actual pinned NSH button, layout, accessibility and focus APIs have been inspected in the local 0.4 registry source.

Pricing and provider editor authority includes private state in sibling modules. Supply narrowly scoped child bridge modules in this authorized new directory; main must mount them under their owning modules, preserving Rust privacy instead of exposing unrelated fields or secrets. The pricing bridge reuses the existing atomic shared-draft claim and original value-submission controller. The provider bridge returns only nonsecret authority/input revisions, including key input revision (never key text).

Focused development regressions cover stale route/intent/authority/draft rejection, disabled actions and busy behavior. They are unrun here by explicit user restriction; main must archive/remove development Rust tests and exclusive wiring before merge. Scoped rustfmt and whitespace/static inspection are the only local validation in this stage.

## Implemented behavior

- `model_configuration.rs` keeps the public destination shape (adding `PartialEq, Eq`), authoritative route methods, and extracted save/discard/stay handlers. The unused help implementation stays removed; existing Escape help compatibility remains unchanged.
- `nsh_navigation.rs` owns both measured-height native surfaces, theme/locale synchronization, wrapping buttons, selected accessibility states, focus border, guard-entry focus and Stay focus return. GPUI hosts contain only the NSH entity, not legacy visible content.
- Snapshots carry surface identity, route, pending destination/close intent, model availability, provider generation/operation/request/base/input entity+revision+composition, exact pricing authority and shared draft session/revision, reasoning authority/draft/operation, capability authority/models/draft/operation and current error. No provider key text is read by either bridge or included in a snapshot. ProviderConfig contains credential reference names only, never credentials.
- Decisions use original controllers. Save chooses pricing retry versus atomic shared-draft claim and original value-submission controller, then capability, reasoning or provider as before. No saving receipt is presented as persistence success. Busy/IME/stale requests reject; retained service failures stay under the original controller.
- Stay and new controller navigation intents invalidate the guard presentation identity. Hidden surfaces are dropped by synchronization, so close/reopen requests cannot reuse a prior surface identity. Current authority equality and duplicate-submission latches protect queued requests.
- Confirmed discard freezes the exact shared pricing draft under its mutex before detaching it. If the original pricing controller still has its asynchronous dirty plan, navigation remains pending until the existing resume path observes no draft and no error; Close is not dispatched while that plan remains dirty. Resume additionally checks all original save sources (including capability/provider saving). No new discard key binding is installed.

## Provider shared-draft integration amendment

The provider page migration introduces its own NSH shared draft. Before integrated acceptance, the owner bridge must include `DraftStamp` (session, exact nonsecret values/revision, composition/frozen/valid and secret-presence/revision metadata only) in navigation authority equality and composition gating. Save must bind its deferred local-secret submission intent to that displayed stamp. Discard must invalidate that exact observed stamp under the draft mutex, never retrieve a newer stamp after confirmation. A stale revision/session or IME/frozen transition rejects both operations and leaves navigation pending; no provider secret text enters navigation projections. Provider owner module wiring and root synchronization are integration prerequisites. Existing seven development regressions do not yet cover this new shared-draft path.

## Required main integration (not written by this stage)

### SettingsView fields and initialization (`state.rs`)

Add exactly these two fields and initialize both to `None`:

```rust
pub(super) settings_navigation_presentation:
    Option<super::model_configuration::nsh_navigation::Presentation>,
pub(super) model_navigation_presentation:
    Option<super::model_configuration::nsh_navigation::Presentation>,
```

### Private owner bridge wiring

At module top level in `settings/nsh_pricing.rs` add:

```rust
#[path = "model_configuration/pricing_bridge.rs"]
pub(super) mod navigation_bridge;
```

At module top level in `settings/provider_management.rs` add:

```rust
#[path = "model_configuration/provider_bridge.rs"]
pub(super) mod navigation_bridge;
```

These are intentionally children of their respective owners: they inspect existing private sessions/revisions/base/request data without making unrelated owner fields public. No new shared dependencies or Cargo/wiring changes are required beyond these declarations.

### Render/lifecycle integration (`render_impl.rs` and alternate shell callers)

- Update both production calls to `self.render_settings_navigation_guard(window, cx)` and `self.render_model_navigation(window, cx)`; both methods now take `&mut self` and `&mut Window`.
- In the Render synchronization pass add `self.sync_settings_navigation_presentations(cx)` **after** `resume_settings_navigation(window, cx)` and alongside existing presentation syncs, before any hidden-settings early return. It clears hidden presentations and republishes fresh authority even when only the underlying controller changed. Both render methods also synchronize before mounting.
- In `on_back`, call `self.invalidate_settings_navigation_intent()` immediately before setting `pending_settings_close`/`pending_section`. Do likewise in any other main-owned/alternate shell entrypoint that directly creates a pending intent instead of using `request_settings_section` or `guard_settings_close`; those two existing methods already invalidate internally. This fences identical repeated close requests as well as visibly different destinations.
- Ensure the existing `resume_settings_navigation(window, cx)` render call remains; asynchronous save/discard only completes routing through real controller state, never the NSH acknowledgement.
- The former `settings_route_button` has no external callers and is removed. No shared `nsh.rs`, `lib.rs`, automation, manifest or lock changes are needed. Existing catalog help removal is preserved; main may remove its remaining unused Escape fields separately if appropriate.

## Validation and remaining gates

Executed locally: scoped rustfmt with `--edition 2024 --config skip_children=true` using rustfmt `1.9.0-stable (88d9e12ae1 2026-08-18)`; tracked `git diff --check`; additional direct whitespace and production-source policy scans include all new files. Exact final hashes are in the READY report.

Seven development regressions: five in `nsh_navigation/tests.rs` cover route/pending/close mismatch, controller/secret-input-revision changes, busy/duplicate actions, hidden/reopened surfaces and selected/unavailable/library routes; two in `pricing_bridge.rs` exercise real shared-draft equality, exact-string claim, newer-session/revision rejection, IME and frozen/late-edit guards. These are **not executed**. Main must archive the original sources/hashes and remove the test file, its `#[cfg(test)] mod tests` declaration, the pricing bridge test module, and the provider bridge development-only fixture impl before merge.

Unrun: compilation, Clippy, all seven regressions, required CI, native focus/keyboard, narrow layout, light/dark/i18n, and real pricing/provider/reasoning/capability failure/recovery interaction. Main owns these checks and the integration edits above. The independent leaf is not compilable in isolation until the reserved fields/callers/owner-module declarations are integrated. No app execution, install, packaging, release, git/index mutations or nested agents occurred.
