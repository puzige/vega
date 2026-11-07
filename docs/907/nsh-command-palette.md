# Command palette — NSH 0.4 migration

## Acceptance (frozen before implementation)

Scope: `crates/vega_ui/src/command_palette.rs`, its new `command_palette/`
module directory, and this document only. Baseline palette SHA-256:
`f90dbee53bed0c5c3aa6c6c341688a9510cd0fdd0b946218d8b3193303dda340`.
Main owns Git synchronization, builds/tests, shared integration, and commits.

- Preserve `CommandPalette::new`, `apply`, `begin_activation`, `query`,
  `Focusable`, all three public events, `action_label`, and all existing actions.
  Application query workers and current-route/target validation remain unchanged.
- Render every visible part with pinned NSH 0.4: input, result labels/details,
  scope selection, backdrop, loading/empty/error/footer, and explicit UI recovery.
  Fast provides only paint-free layout, window/focus/events and mount lifetime.
- One authoritative shared nonsecret draft owns bounded committed text (at most
  256 Unicode scalar values), composition state, and revision. Input handlers
  update it synchronously before ordered commands. No intermediate stale query,
  old result, prior revision, unmounted control, or previous engine session may
  authorize activation against the current controller target.
- IME composition never activates or dismisses, including pointer and keyboard
  paths. Up/Down/Enter/Escape/Tab/Shift-Tab have no palette keymap registration
  during composition: they reach the IME, not a consumed no-op callback. Reactive
  registration cleanup removes old bindings before re-registration and on disposal.
  Host/current-draft guards remain independent defenses.
  Only committed text triggers application queries. Paste uses the native
  Input text path, never sends files or rich payloads to the application engine.
- Preserve action/task/file ordering, limits (30 tasks and 30 files), labels,
  aliases, all four scopes, loading, partial-file errors, activation errors, and
  retained retryable results. Retry is an intentional user activation, not replay.
- Preserve global shortcuts and local Down/Up/Enter/Escape/Tab; Shift-Tab cycles
  scopes backwards. Input keeps keyboard focus while navigating. Controls expose
  meaningful labels/roles and selected state; selected results stay in view.
- Engine/command failure closes action authorization. Recovery is an explicit
  NSH-only rebuild of the UI with the current draft, a new session, and no replay
  of queries or application actions. Recovery failure remains visible.
  Replacing a failed AppView synchronously transfers actual window focus to the
  replacement, so Enter rebuilds and Escape closes without pointer interaction.
  Rebuilding only the current mounted palette restores its ordinary input; no
  delayed host focus callback may reclaim focus after replacement/unmount.
  Explicit recovery backdrop dismissal is allowed before ordinary-operation
  rejection only for the current mount/session/draft/projection with no IME.
  Failure/recovery still rejects navigation, query commits, and activation.
- No credentials, filesystem/network IO, dependency upgrades/forks, Composer
  coupling, shared-file edits, app launch, Git mutations, or subordinate agents.

## Review follow-up acceptance (before corrective implementation)

The three reviewed defects are failures, not passed validation: unconditional
`When::Always` registrations consumed IME keys before the callback guard;
recovery replacement never moved actual window focus; the failed-operation gate
rejected recovery backdrop dismissal. Preserve all five original regression tests
and add bounded registration-lifetime and request-authorization regressions.
Main retains exclusive build/test execution; this writer claims no native proof.

## Pinned API evidence and design

Inspected installed `nsh-core`, `nsh-base`, and `nsh-platform-gpui` 0.4.0:
`element/host.rs` Input `value`, `max_length`, `auto_focus`, `on_change`,
`on_composition_change`, `on_submit`, `element/ambient.rs` `on_key_with`, and
platform `text_input.rs` composition reporting. Committed text is reported before
composition-end. The simple palette needs no range-decoration/Composer adapter.

Shared `Surface::new` takes a renderer function pointer. Non-comparable
projections use `signal_with(..., SignalEquals::Never)`; `ProjectionId` identity
uses `matches`, never equality. Commands use existing `OrderedCommands` rather
than lossy `try_send`. Keyboard navigation uses NSH's actual desktop keymap while
Input remains focused, not ordinary View focusability or a native overlay.

Follow-up inspected the actual pinned implementations:

- `nsh-platform-gpui/src/input.rs:994-1018` runs the keymap before text input;
  `keymap.rs:67-94` consumes a match even when the callback does nothing;
  `text_input.rs:2121` yields composing keys.
- `nsh-core/src/element/ambient.rs:1107-1129` registers a unique binding ID
  and owner cleanup removing that exact ID. `element/reactive.rs:866-900` owns
  effects/cleanup; `reactive.rs:841-849` cleans before rerunning the effect.
  `element/keys.rs:38-60` marks adds/removals dirty for key-table publication.
- Existing `Surface::focus` calls `window.focus` on the replacement AppView's
  real `FocusHandle`; pinned `app.rs:699-702` returns that handle. No shared
  hooks or delayed host focus callback were added. Ordinary Input retains its
  existing NSH auto-focus, whose native scheduling remains a verification limit.

## Verification plan and remaining gates

Main must run bounded behavioral regressions for committed edit → immediate
Enter, rapid edits and stale results, composition plus Enter/Escape/pointer,
query length/paste, scopes/order/selection boundaries, stale result click,
unmount/remount, failure/rebuild without replay, and original public events.
Development Rust tests, if added, must be archived and removed before merge.
This writer may run scoped rustfmt and static inspection only. Compilation,
Clippy, behavioral tests, required CI, independent review, and native keyboard,
AX, pointer, scrolling, theme and narrow-window checks are not implied by static
checks.

## Implementation handoff

The original action declarations, event payloads, public signatures,
`action_label` match, action/task/file entry construction and task/file caps are
retained. `new` still creates the original `Entity<TextInput>` and `sync_query`
updates it only as an unrendered carrier; there is no legacy input element in the
render tree. `Focusable` returns the NSH surface handle once mounted. No changes
were made to application query workers, generation/cancellation/route checks, or
activation target validation in `crates/vega/src/app_palette.rs`.

The shared draft is an `Arc<Mutex<Draft>>`. Edits and composition notifications
advance its revision synchronously. Controller query publication, result
application, and activation hold the same draft lock through their decision.
The controller records the published query revision, including same-text edit
cycles. Every request includes engine session, displayed projection, and draft
revision; pointer activation additionally compares index and full target with
current entries. A weak mount lease rejects requests after removal; remount and
recovery rotate session identities. Selection arrows clamp; Tab/Shift-Tab cycle
scopes forward/backward. Scope buttons and result rows use NSH semantics and
selected state; keyboard result changes announce labels/details. The list reveals
the selected row from its mounted layout callback.

Fast's only wrapper is an absolute paint-free event/layout host plus a mount
lease element that delegates layout/prepaint/paint unchanged. There is no Native
click-intercepting overlay. Fast captures pointer-down while composing so focus
loss cannot turn that gesture into an activation after composition-end. Recovery
has an explicit paint-free Enter/Escape keyboard route; ordinary operation uses
NSH Input keyboard routing, not `.focusable()`. No shared keyboard hooks are
required. Pending text with an older displayed projection is conservatively
non-activatable: Enter is not queued for replay against a later target.

### Corrective implementation delta

Only `presentation.rs`, its `presentation/tests.rs`, and this spec changed in
this follow-up; `command_palette.rs` remains byte-identical to the prior READY.

1. `register_keys` installs the six original key bindings inside one reactive
   effect, only while focused, not composing, and not a recovery surface.
   Owner cleanup retires registrations before re-registration/disposal. Callback
   and dispatch current-draft IME guards remain; recovery has no Input keymap.
2. Failed-to-recovery replacement synchronously calls `recovery.focus(window,cx)`.
   Explicit authorized rebuild mounts its ordinary replacement immediately,
   stops the old surfaces, calls `surface.focus(window,cx)`, and retains only the
   current mount lease/new session. It does not wait for another render to build
   a focus target and does not replay query commits or application actions.
   Enter/Escape use the fenced receive path; Tab remains contained in recovery.
3. `Request::authorized` preserves mount/session/draft/projection/IME validation,
   allows current explicit Dismiss/Rebuild when failed, and rejects all ordinary
   operations while failed/recovering. Dismiss is handled before ordinary query
   revision checks because closing has no business target. Dispatch permits that
   path even when retained recovery text differs from the last queried revision;
   target navigation/activation retains its stronger controller-query checks.

### Executed static checks

- `rustfmt --version`: `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`.
- Scoped `rustfmt --edition 2024` and subsequent `--check` on
  `command_palette.rs` and `command_palette/presentation.rs` (which discovers
  `presentation/tests.rs`): exit 0.
- Prior READY's read-only catalog audit found all 18 English/Chinese keys;
  this follow-up added/changed no localization keys (not rerun).
- Read-only Python production scan: zero code comments, `unwrap()`, `expect()`,
  or `try_send()` in either production Rust file.
- Follow-up read-only Python SHA verification: entire `command_palette.rs`
  unchanged; reconstructing the original five-test suffix gives exactly prior
  test hash `e93ff38971358e1c2742678324f36f6f8f1263d4a4026532ae7ef40a24e950f9`.
  Eight `#[test]` declarations found. Static checks are not behavioral passes.
- Final pinned-source inspection caught a harness expectation error before any
  execution: `Core::dispatch_event` returns true even for retired Rust key IDs.
  The regression now asserts live callback effects and absence of retired callback
  effects rather than interpreting that routing return value as activation.
- No Cargo/rustc build, Clippy, test execution, CI, Git operation, application
  launch, installation, configuration change, or subagent was run by this writer.
  Cargo/rustc runtime versions and behavior results are not obtained here.

### Development regressions and required main follow-up

Eight unrun behavioral tests are wired from
`command_palette/presentation/tests.rs`. The original five and every assertion
remain unchanged: committed composition ordering and old activation rejection;
full Unicode paste bound; same-text edit revision fence; rebuild draft
retention/old-session rejection; bidirectional scope cycling. Three additions:

- Real headless NSH Core publishes exactly six bindings normally, none during
  composition (including repeats), returns to six across three cycles, rejects
  retired binding callbacks (live bindings increment an observable counter),
  removes bindings on blur, and disposes them with the owner.
- Recovery dismissal requires current mount/session/draft/projection and no IME;
  stale revisions and replacement sessions cannot close the current palette.
- Failure rejects query commits/navigation/scopes/activation while allowing only
  current explicit close/rebuild; stale/unmounted rebuild remains unauthorized.

No native-focus automation was added: the synchronous focus calls and supported
API are statically inspected, not proven keyboard-only native operation.
Main can select them with
`cargo test -p vega_ui command_palette::presentation::tests` using its coordinated
toolchain. These tests exercise real NSH key tables, production authorization,
and shared draft transitions, not source strings or a mock controller. They do not establish native keyboard/AX/pointer behavior or
full application integration. Main still needs compile/Clippy, necessary
controller regressions and independent review; archive the exact test source and
results, then remove `presentation/tests.rs` and the final `#[cfg(test)] mod tests`
wiring before merge and run final-version gates. No test-only dependencies or
shared files were added.

### Native limits / explicit unverified items

Pinned 0.4 has no Dialog role, so the palette uses a labelled Group containing
Input, Tablist/Tabs and Listbox/Options. Ordinary Views have no native Tab/default
activation; Input keyboard handlers implement navigation explicitly. Input has
composition and full-paste support but no range decorations; this simple search
needs none. Global shortcuts, IME candidate key delivery (NSH keymaps precede
field editing), focus restoration, screen-reader announcements, scroll geometry,
backdrop/panel hit testing, themes and narrow windows require native verification.
The recovery UI is itself NSH: if its engine also stops, no paint-free host can
truthfully guarantee it remains painted; reopening the palette is the external
recovery path. A closed command receiver reports `Command failed` and cannot
replay application operations. No native-pixel or behavioral pass is claimed.

### Rust artifact SHA-256

| File | SHA-256 |
| --- | --- |
| `crates/vega_ui/src/command_palette.rs` | `9e344e439df73682f1aba13da0e1111bf5d55268cb2e23176bed7b18d65de08c` |
| `crates/vega_ui/src/command_palette/presentation.rs` | `c8fa4368999dc8495d20dbbcbfb8e72ef14793ae004ce0c6a5e6638d2e1b9d57` |
| `crates/vega_ui/src/command_palette/presentation/tests.rs` | `a9758a43783b7728b1baa8dd10e416b8937e4c0af4bd6f64e23d9c8e52783db8` |

The document's own final hash is reported in the handoff rather than embedded in
itself. READY means ready for main's build/test/review, not acceptance or merge.

## Integration checkpoint: unresolved native IME P1

Base HEAD `bd89db7d182e6d9a5c60b1388e79a4623e8cacc4`; the parent corrected the missing `nsh_core::element::Element` import after the first coordinated compile. The artifact hashes above identify the earlier writer handoff, not this newer delta.

Independent static assessment confirms that reactive registration cleanup does not close the native publication interval: the native field can enter composition while the old six-binding table remains installed. A no-op callback still consumes the key. This is a product/platform defect, not a permission failure; native reproduction has not been run.

Pinned `nsh-platform-gpui` 0.4.0 `input.rs:982–1018` can hold keys before `key_now`, which runs the keymap before `text_input::key`. The latter's synchronous composition check at `text_input.rs:2121–2123` is therefore too late. `InputState`, focused-field access and `AppView::surface` are private. Public `on_submit` covers Enter only; `on_key_press` cannot cancel routing and lacks Up/Down/Tab. `When::Auto`/`NotEditing` suppress required ordinary-input shortcuts. A NativeComponent cannot observe another Input's native model. No supported fixed-pin palette-only solution was found.

The blocked step is command-palette IME acceptance. Do not merge or call this leaf complete. The user explicitly chose to maintain the original dependency constraints after being offered a narrow platform patch; do not upgrade or fork, and continue independent migration scopes. No dependency modification has been made. A responsible patch must check native composition before both key holding and direct/replayed routing, leave the affected candidate keys unhandled through downstream navigation/Tab, and preserve unrelated shortcuts. Retain reactive cleanup, but reconcile the spec's instantaneous-registration wording with the actual guarantee of native non-consumption even with a stale table; do not weaken the user-visible IME requirement.

Required regression: deliberately keep the old table installed, begin native composition without processing engine updates, and route Up/Down/Enter/Escape/Tab/Shift-Tab plus repeats through normal and direct/replayed entries. Assert no command, queued replay, activation, dismissal or focus movement, then verify ordinary post-composition navigation/scopes and unrelated shortcuts. Native candidate delivery remains a separate user verification. Existing headless key-table lifecycle tests do not cover this interval.

### Parent validation of the stopped integration delta

Normal command execution, compilation and tests are available; no permission reset or machine change is needed. Actual tools: Rust 1.98.0, Cargo 1.101.0 / huacnlee fork 0.102.0-9, Nextest 0.9.151 (cloud pin unchanged). Parent corrected only the missing Element trait, an equivalent Clippy conditional collapse, and test harness imports/public Root/size constructors; no assertion was removed or weakened.

- `cargo check -p vega -p vega_ui --locked --offline -j 2`: exit 0 (7.08 seconds before the equivalent conditional cleanup; final Rust delta rechecked successfully in 3.21 seconds).
- `cargo fmt --all -- --check`: exit 0 on the latest Rust delta.
- `cargo clippy --workspace --all-targets --locked --offline -j 2 -- -D warnings`: exit 0 on the latest Rust delta.
- `cargo nextest run --workspace --locked --offline --build-jobs 2`: exit 0; 124 executed / 124 passed / 0 skipped, including palette 8 and branch-selector 5 new development regressions. These do not prove native IME delivery or resolve P1.
- Python release protection 5, computer preparation 15 and scripts 40: all passed. `git diff --check`: exit 0.
- Latest hashes: `presentation.rs` SHA-256 `9968f6567b0f2f58772df3a90851b59fdf60da3eb16333d3aac3b362faa1c9a3`; `presentation/tests.rs` `55598e94e86fc674ff137ed15b8912201ac58c0387589b41a5ed7c17d4e89283`. `command_palette.rs` unchanged from writer handoff.
- Branch-selector correction independent review completed with no actionable findings; all four previous findings statically resolved. Palette native IME P1 remains open. No new commit/push, exact-delta cloud CI, merge, release, installation or app execution is claimed. Rust development tests remain pending archive/removal before final delivery.
