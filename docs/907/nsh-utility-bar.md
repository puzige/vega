# NSH utility bar — fourth production UI lane

## Scope and acceptance fixed before implementation

Parent-supplied baseline: `0b5ea426abfd8d4358489e916273d89474fb190f`, branch `feat/907-nsh-full`, dirty worktree retained. Parent completed fetch/update and ancestor verification against `34555cec9c0c4c4c1f7a528d739752f414e53edf`. This writer performs no Git/network/Cargo/native/install/configuration/dependency operations or delegation. Four lanes implement before the parent organizes concentrated independent review; no intermediate review request.

Owned implementation: `conversation_stream/utility_bar.rs`, new `utility_bar/**`, `file_selector.rs`/new `file_selector/**`, and this dedicated specification. Other allowed modules were inspected to avoid rewriting already migrated leaves: navigation and branch controls, CUA guidance and sidebar are already NSH. `file_selector.rs` is a headless model, but its actual `conversation_stream/render.rs::render_file_dropdown` still draws GPUI. Implement its NSH leaf using the same guarded presentation host; parent alone replaces that shared method with the precise one-line call recorded below. Preserve loading, failure/retry, no-match, exact candidate/highlight/index generation, current input token/revision/composition and original accept/retry/cancel methods. Do not claim production wiring until parent applies that replacement.

- Replace actual utility project/backend chips, menu surfaces, search, rows, states, selected markers and utility background with NSH content. Fast may retain layout, events, focus/AX proxies, window popup placement and original controller entities; no visible legacy control may be embedded in Surface.
- Preserve the existing visibility predicate, 37px bar, 19px inset, 28px capsules, 8px padding/gap, existing tokens/i18n, upward deferred window popup, viewport fitting, project search, original empty/no-match copy, disabled New Project explanation, No Project and backend identities. Reuse the already migrated branch selector unchanged.
- Retain the original project store loader and original project/backend request/controller paths. Search keeps the existing TextInput as an unrendered compatibility carrier. Backend mutation remains draft-only and reuses `request_task_backend`; no service, permission or persistence work moves into the engine.
- Commands carry exact page/presentation and parent epoch, actual thread/backend/project/draft identity, selected project, input identity/revision/composition, permission/busy state and current menu/candidates. Receiver rechecks actual route/settings/preview, visibility, active window, mounted clipped geometry/current viewport, nonfailed Surface and acknowledged displayed controls. A changed or retired presentation never replays delayed actions.
- Search uses NSH input with nonsecret shared draft and composition state. Pointer/key/AX project decisions cannot outrun a newer query or active composition. Keyboard uses real Fast focus/host capture: Tab/Shift-Tab traverse enabled current controls, Enter/Space only activate current displayed focused target, Escape dismisses and restores the surviving trigger/composer; native input text editing is not swallowed. Rows outside the measured viewport cannot activate through keyboard/AX.
- Failure recovery is explicit NSH presentation-only reload, never implicit business replay. Pure background decoration is AX-hidden, nontabbable and native-hitbox-isolated. Exact mount release stops its Surface; parent epoch retirement rejects retained business callbacks synchronously.

## Verification contract

Only scoped direct rustfmt and read-only/static inspection run in this lane. Parent compiles, runs meaningful developer regressions and final integrated CI after all four lanes finish, then concentrates independent review. No earlier test/CI result validates this delta. Native pointer/IME/Tab/Shift-Tab/Escape/AX, scrolling, resize/themes/narrow windows and macOS/Windows/Linux acceptance remain unrun. Any developer Rust tests must be archived/removed before master per repository policy.

## Resumed implementation contract — compile interfaces and per-row native AX

The parent production check (`target/nsh-primary-lib-check.stderr`) found private stopping access, missing NSH Element trait imports, Measurement/Bounds mismatches and a trigger-focus borrow conflict at the frozen hashes below. Fix their responsible interfaces without weakening currentness or acceptance. Per-row AX must use measured root-page coordinates plus the actual mounted surface origin, not composite bounds. Measured proxies retain exact row leases, scroll revisions, page/version/epoch and mounted geometry; stale focus/activation callbacks must fail closed. Include native Focus actions, selected/checked/expanded/disabled semantics and accessible disabled explanations. ACK-driven native redraw must not republish unchanged NSH rows and invalidate their leases indefinitely. Native AT remains unrun until actual platform verification.

At resume, stopping had no exact public query. Final read-back confirms the parent concurrently supplied this exact method inside `impl ComposerActions` in `conversation_stream/composer_actions.rs`; this lane calls it without editing that shared file:

```rust
pub(crate) fn is_stopping(&self) -> bool {
    self.stopping
}
```

## Resumed implementation handoff — READY for parent integration

This lane stops writing at the hashes below. READY means implementation handoff, not compilation success, independent review, final production wiring or native acceptance.

- Utility production call remains `render_composer_utility_bar`; its unchanged caller now reaches NSH background, chips, project search/menu and backend menu through a paint-free Fast host. Existing migrated branch entity is retained unchanged.
- `utility_bar/host.rs` owns actual page/version/epoch, thread and permission/busy identity, selected project, draft/input identity/revision, menu/candidate state, file index generation/query/highlight, native mounted/clipped viewport geometry and nonfailed Surface gates. Search uses a stable NSH input, shared nonsecret draft and original hidden TextInput carrier. Ordered requests use the stable renderer owner; row callbacks carry their original engine-row lease and measurement scroll revision.
- Fast supplies layout, anchored/deferred popup placement, native input composition observation, keyboard/focus/AX bridge and empty-mask isolation for pure background hitboxes. NSH supplies product text, icons, colors, capsule state, menu/search and focus indications. Explicit reload recreates presentation only.
- Resumed fixes import NSH Element, convert Measurement using root `page` plus `frame.size` for reveal/visibility, clone the trigger FocusHandle before mutable focus and call the exact parent-supplied `ComposerActions::is_stopping()` accessor. Final read-back confirms the accessor is present with exact stopping semantics; integrated compilation of this delta remains unrun by this lane.
- Displayed ACKs now carry each real row rectangle. Mount geometry keeps the actual surface origin separately from its clipped bounds; native proxy placement adds that origin and rejects clipped/invalid rows. Each measured visible row has its own native AX node with exact bounds, label/disabled hint, radio checked/selected and trigger expanded state. Focus and Click callbacks recheck row lease, scroll revision, selection/focus state, page/version/epoch, business/input identity, composition and mounted geometry. NSH row AX subtrees are hidden to avoid duplicate unguarded nodes; the native NSH search input remains exposed. Unchanged publication deduplication prevents ACK redraws from repeatedly retiring measured row leases. The previous composite AX proxy is removed.
- File completion uses the same host and renderer with Files kind, preserving loading, no-match, failure/retry and exact bounded candidate identities. Original `on_selector_click`, retry action, cancellation and previous/next controller methods remain authoritative. No file model/controller, branch, navigation, sidebar, CUA, shared root or dependency was rewritten.

### Exact parent patch — shared file not written by this lane

In `crates/vega_ui/src/conversation_stream/render.rs`, replace the entire existing `render_file_dropdown` method (from its signature up to the next `render_model_controls` method) with:

```rust
    fn render_file_dropdown(&self, cx: &mut Context<Self>) -> AnyElement {
        self.render_nsh_file_dropdown(cx)
    }
```

No new shared field, root module declaration, dependency or lifecycle hook is required. Both new modules are declared privately by `utility_bar.rs`; parent `run_notice_epoch` retirement and existing suspended/route state are reused. Final read-back confirms the parent has applied this shared replacement. This lane did not write it. Production dispatch now reaches the new leaf in source; integrated compilation and native execution of the final delta remain unverified by this lane.

### Actual static verification

Tool: direct `/Users/puzige/.rustup/toolchains/1.98.0-aarch64-apple-darwin/bin/rustfmt`, reporting `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`. Actual command on all four Rust artifacts below: `rustfmt --edition 2024 --config skip_children=true --check`; exit 0 after scoped formatting. No Cargo/rustc version query or build/test command was run.

Read-only Python inventory of the same final Rust files: zero trailing-whitespace lines, zero code comments, zero `.unwrap()`/`.expect()` calls, zero `try_send`; six developer `#[test]` sources. No Fast `.bg`, `.border_color`, `.text_color`, shadow or `paint_quad` product drawing remains in the owned host/controller sources. The resumed static inventory also confirms no old composite proxy or direct private stopping access. Initial lane token resolution remains the earlier static evidence; no token was added in this resumed delta. These are static checks, not behavior or typechecking evidence.

Six authored developer tests call production decisions: query revision/composition acceptance, row lease/scroll measurement retirement, clipped/invalid visibility, clamped/settling oversized reveal, root-page versus parent-frame scroll coordinates, and native row origin translation/clipping/invalid rejection. The last two were added in this resumed delta. All six UNRUN. Parent must execute if retained during development, archive their exact source/hash and remove `host/tests.rs` plus the `#[cfg(test)] mod tests` declaration before master under repository policy.

### Final Rust SHA-256

Paths relative to `crates/vega_ui/src/conversation_stream/`:

| Artifact | SHA-256 |
| --- | --- |
| `utility_bar.rs` | `ef7c024b8a52d5459c559aa44c5f52c46d349b905c7b131864dbb0ca0df911f5` |
| `utility_bar/host.rs` | `6ed92d9075499ed90e82eed36f0c1b948b93c10f387afa1f6854e5932674e536` |
| `utility_bar/view.rs` | `f9b7926e5bf233f9ac79e7f87c2a997fcb9e8375ad8b194fb3647b1c4b207c5a` |
| `utility_bar/host/tests.rs` | `1c15e469462fe193ea47ebb68d48b40bf19cefb40469018bd6cd2bd2b5926f16` |

### Unrun gates, limitations and remaining inventory

- Parent shared accessor and file-dropdown replacement were both confirmed present by final read-back; no additional shared patch remains from this lane. Required parent work: integrated compile/strict Clippy, developer behavioral regressions, concentrated non-implementer review after all four lanes, final test archival/removal and final-head CI. No intermediate review was requested.
- Native verification remains UNRUN: project/backend open-close-reopen and sibling popup isolation; query typing/composition and retained-callback races; input/row Tab and Shift-Tab, Escape/close/refocus; pointer/AX activation and keyboard reveal under scrolling; hidden/remount/inactive/resize/failure-reload; themes/narrow window and macOS/Windows/Linux behavior. Measured per-row native AX bounds/focus are now implemented, but native AT bounds/focus fidelity is NOT verified. Partially clipped or offscreen rows deliberately expose no actionable proxy until displayed with current geometry; native scrolling/reveal and real Focus/Click delivery still require platform acceptance. No composite replacement is used.
- Existing Composer input/IME and broader same-version reading/editing backend work belong to the other lanes/shared integration. This lane does not claim they are fixed or globally keyboard-compatible.
- Existing sidebar/navigation/CUA/branch NSH implementations were retained, not reimplemented. Their documented native residuals remain acceptance work; this is not a claim of whole-product migration or three-platform completion.
- No Git/network/Cargo/app/native/install/configuration/dependency operation or nested delegation occurred. All pre-existing dirty files and other writers' outputs were left untouched. Only the four Rust artifacts above and this dedicated document were written by this lane.
