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

## Current superseding checkpoint: authorized same-version native editing patch

This is author development closure, not independent review. It supersedes the
historical patch-authorization/blocker statements above, without deleting their
history. The user approved the minimal native editing patch at the same pinned
versions; Fast and dependency pins remain unchanged. The parent's dirty worktree
has HEAD `0b5ea426abfd8d4358489e916273d89474fb190f`; the user reports fetched
mainline `0b3dcd3603a8e97e0ef74ae25a26f17fc3a094e5` integrated in the dirty tree.
This lane does not fetch, update Git, compile, run tests, or change production.

Current source inspection finds `native_composing` before both key holding and
keymap dispatch in `input::key` and `input::key_now`. `composition_keys` drops
`Held::Key` while retaining `Held::Text`; `flush_pending` also checks composition.
`events.rs` has a separate native-composition early return: it stops propagation
without calling `prevent_default`. These facts supersede the old routing-order
blocker, but are not proof of OS candidate delivery. Reactive key-registration
cleanup remains required. During its publication interval an old table may still
be installed: the native routing guarantee is non-consumption, not instantaneous
removal of that stale table. This clarifies the original registration wording
without relaxing candidate-key, activation, dismissal or focus safety.

### Bounded regression and explicit evidence limits

Extend only the existing `marked_keys_never_reach_shortcuts_or_waiting_keys`
function. Keep all original keys/assertions, add repeat and Shift coverage, and
install a real NSH `MountItem::Keymap` with the six palette keystrokes and their
production repeat settings before entering composition via
`ime::replace_and_mark_text_in_range`. Leave that table installed without engine
updates. Exercise both `key` and `key_now`; require false (not a consumed no-op),
no `KeyCommand`, unchanged native focus, retained text and no waiting key replay.
After native unmark/flush, require the same bindings to emit their exact command
IDs through both entries, including both scope directions, and an unrelated
shortcut to work again. The table and matcher are real NSH APIs, not a replacement
routing algorithm.

The vendor host crate cannot call `vega_ui`'s private `register_keys` or mount the
actual palette/controller through this existing test. Constructing real platform
bindings therefore verifies stale-table native routing, not the actual palette
registration producer, operation callbacks, selection/scope values, activation,
or dismissal. Absence of `KeyCommand` closes this tested keyboard dispatch path;
it is not an end-to-end assertion of those application states. The separate
palette lifecycle tests still exercise the actual registration producer. Do not
claim the combined native-to-palette publication-interval contract is fully
exercised in this lane. Completing that exact integration requires a parent-owned
cross-layer test seam; no production API expansion is authorized here.

A further production limitation is visible: the current composition guards yield
**all** keys before shortcuts/keymap, not only candidate-navigation keys. Thus
unrelated shortcuts are only checked after composition here. If preserving their
execution during composition is required by the original unrelated-shortcut
contract, the current patch does not satisfy it; parent must resolve this rather
than weaken assertions. No production correction is authorized in this lane.

The existing parent log `target/nsh-current-nsh_native_host.stdout` reports
36 passed / 0 failed, including the old wrapper at
`crates/vega_ui/tests/nsh_native_host.rs:67`. Its SHA-256 is
`67215571388d7bef71d9b239af10a60d74b937a9181762bca7d9f506911824f9`.
This is pre-edit evidence only; the extended regression has **not been run**.
Synthetic host events and OS-native IME/focus acceptance remain separate.

### Measured source identity and parent next run

SHA-256 measured before this lane's edits (not inferred from HEAD):

| Source | SHA-256 |
| --- | --- |
| `vendor/nsh-platform-gpui/src/input.rs` | `895de6656a33012752b09d1c4ea5fd9fe075919366745b5fc861fbeb70850252` |
| `vendor/nsh-platform-gpui/src/text_input.rs` | `e27b80250ce6d8ec7ab62d60b8e0b84045c2b7e133e8dd4dc17488a2923601e0` |
| `vendor/nsh-platform-gpui/src/keymap.rs` | `f9ef92d322882e61665cc2f841c9f71eacba978a644c7e944132efedd12da0f8` |
| `vendor/nsh-platform-gpui/src/events.rs` | `793e13597fb1c1f7f722ee89b05ceea96acde1793900df5e95277290545d4233` |
| `crates/vega_ui/src/command_palette/presentation.rs` | `9968f6567b0f2f58772df3a90851b59fdf60da3eb16333d3aac3b362faa1c9a3` |
| `vendor/nsh-platform-gpui/src/text_input/host_tests.rs` (pre-edit) | `36617f0604204dce3519e0c7a917d795c2f70fe56cd6790e8d956f29099a9a8d` |

Final test/document hashes and exact scoped diff proof belong in the handoff,
not a self-referential document hash. The parent owns the next central build:
run the extended existing `nsh_native_host` regression with the existing host
feature wiring, then the relevant palette regressions and integration checks;
record exact commands, toolchain, final source hashes and fresh logs. Resolve
the cross-layer/unrelated-shortcut gaps above, then obtain independent review,
final-head required CI and user native IME/keyboard acceptance separately.
Parent updates the vendor manifest only after the source is stable. Development
Rust tests still require archival/removal before merge; no merge readiness,
independent approval, installation or native acceptance is claimed here.

## Current superseding contract: composition-safe global shortcuts

This author-development follow-up repairs the existing requirement in line 233;
protecting candidate keys and preserving unrelated shortcuts during composition
are both existing acceptance, not a new behavior or a relaxation. The preceding
broad-guard limitation and production-edit prohibition are historical: the user
now authorizes a minimal same-version vendor-only routing repair. Parent retains
manifest ownership; dependency versions, Fast, Core and Cargo remain unchanged.

Before engine updates, native marked text is authoritative for normal, direct /
replayed and GPUI-host key delivery. Candidate navigation (Up/Down/Left/Right,
Enter/Escape, Tab/Shift-Tab, Home/End/PageUp/PageDown), editing keys and ordinary
IME text must not reach the stale application keymap, native editing policy,
navigation, focus traversal or held-key queue. Repeats and modified candidate
keys have the same exclusion. The GPUI host stops propagation of these keys but
does not prevent OS default delivery, including unmapped native text keys.

The narrowly enabled global-shortcut class is a recognized key with the primary
shortcut modifier (Command on macOS, Control elsewhere), without Alt/Option,
whose actual `nsh_core::keymap::Keystroke::edits_text()` returns false, excluding
Space explicitly because NSH classifies it as a character but IMEs use it for
candidate confirmation. Thus
`mod+k` remains available; select-all/cut/copy/paste/undo/redo, candidate keys,
plain/Shift typing, macOS Control-only editing and AltGr/Option text stay native.
This intentionally does not enable every possible OS/IME-specific shortcut.
Only window shortcuts, the installed application keymap and menu shortcuts are
eligible: no normal text/navigation fallback or held-key admission. Unhandled
eligible shortcuts may bubble to the embedding app; handled ones are consumed.
No shortcut route itself commits, clears or replaces marked text. Key releases
must clear real keymap repeat tracking even while native composition is active.

Retain all existing candidate assertions and the installed-table regression.
Add positive `mod+k` execution during composition through normal and direct
entries, exact command IDs, release/repress and repeat behavior, with unchanged
marked display, focus identity and retained text-only queue; ensure no replay on
unmark. The private palette registration/controller coverage limitation and
separate synthetic-host / OS-native acceptance requirements above still apply.
Implementation and measured final-source evidence follow below after editing;
no new test execution is claimed by this contract entry.

### Author repair handoff: measured final source, not executed acceptance

`input::composition_key` now shares the native-state gate and restricted shortcut
route across `key`, `key_now`, component delivery and the GPUI host. It purges
only held keys through the existing `composition_keys`, releases actual keymap
repeat state on key-up, and never calls the normal editing/navigation dispatch
while composing. No queue implementation or normal dispatch chain was rewritten.
The host applies the returned native/shortcut disposition before native-component
routing: native down/up events stop propagation without `prevent_default`, and
only accepted global-shortcut downs prevent default. Unknown text keys remain
native. `text_input.rs` required no change.

The same existing regression retains every prior assertion and case. Its added
matrix covers ordinary/Shift text, editing shortcuts, navigation/Space and
Option/AltGr; real installed `mod+k` emits its exact command ID through normal,
direct and component entries during composition, suppresses explicit/implicit
repeats and fires again after release. Additional existing-window-shortcut checks
exercise both direct entries. Marked display, focus identity and the text-only
pending queue remain asserted unchanged; an unbound global shortcut stays
unhandled without text mutation, and unmark/flush must not replay any command.
These are authored assertions, **not a claim that they have run**.

Measured SHA-256 after the Rust edits:

| Source | SHA-256 |
| --- | --- |
| `vendor/nsh-platform-gpui/src/input.rs` | `b61dd1f487fd5863af811d4bc0130cdf442cb583453ad8c968f7083b92b251a8` |
| `vendor/nsh-platform-gpui/src/events.rs` | `7be330b4044795c3dfdf71b957e5a01420dd7c95f154affd6532b399b6ae6148` |
| `vendor/nsh-platform-gpui/src/text_input/host_tests.rs` | `81a07509bac52becac0f60042afbe14d7c5f92ba985b3af1453e70494efb2d6c` |
| `vendor/nsh-platform-gpui/src/text_input.rs` (unchanged) | `e27b80250ce6d8ec7ab62d60b8e0b84045c2b7e133e8dd4dc17488a2923601e0` |
| `vendor/nsh-platform-gpui/src/keymap.rs` (unchanged) | `f9ef92d322882e61665cc2f841c9f71eacba978a644c7e944132efedd12da0f8` |

Static evidence: the installed toolchain's direct `rustfmt` executable reports
`rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`. With `--edition 2024 --emit stdout
--config skip_children=true`, final `input.rs`, `events.rs` and the existing test
function all parsed with exit 0 and byte-identical formatting. This is parsing /
formatting only, not type checking. No Cargo, rustc, native app, network or Git
mutation was run. Existing 36/36 evidence remains strictly pre-repair.

Exact scope proof reconstructs the measured pre-edit bytes after reversing only
the authorized deltas: input baseline `895de6656a33012752b09d1c4ea5fd9fe075919366745b5fc861fbeb70850252`,
events baseline `793e13597fb1c1f7f722ee89b05ceea96acde1793900df5e95277290545d4233`,
and test baseline `27bd7a4d8bf14d9ed79658cda0b67dfc65a8734f15409936ecff23114d426953`.
The test delta is insertion-only within the one authorized function (161 added,
0 removed lines). Combined Rust unified diff SHA-256 is
`66e13d40bd1db73fe2ecaf990a820334310b3820d6083cfbc1f5de74727059d1`
(Python `difflib.unified_diff`, three context lines, input/events/test order,
`a/` and `b/` repo-relative paths). Both documents preserve their complete prior
byte prefixes; final document hashes belong in the handoff outside themselves.

Parent next coordinated commands (not executed here), using existing dev-feature
wiring and the parent's approved toolchain / target directory:

```sh
cargo test -p vega_ui --test nsh_native_host marked_keys_never_reach_shortcuts_or_waiting_keys --locked --offline -j 2 -- --exact
cargo test -p vega_ui --test nsh_native_host --locked --offline -j 2
cargo test -p vega_ui --lib command_palette::presentation::tests --locked --offline -j 2
cargo check -p vega -p vega_ui --locked --offline -j 2
```

Record final input hashes, actual tools and fresh logs before drawing behavior
conclusions. The existing test does not invoke the GPUI `Window` event closure;
synthetic host propagation/default-delivery verification remains separate, as do
OS-native IME checks and the private real palette registration/controller seam.
Parent owns vendor-manifest stabilization, consolidated independent review,
final-head required CI, development-test archival/removal and user acceptance.
This author handoff is not independent review, merge readiness or native closure.
