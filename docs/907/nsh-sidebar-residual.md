# Sidebar residual NSH migration — Agent3 next cycle

## Authority and scope

Assigned worktree `/Users/puzige/Workspace/worktrees/vega-907-nsh-full`, branch `feat/907-nsh-full`, HEAD `0b5ea426abfd8d4358489e916273d89474fb190f`. Parent reports fetch/update and ancestor check against master `34555cec9c0c4c4c1f7a528d739752f414e53edf`; this worker does not repeat Git/network operations. Read-only branch metadata agrees with assigned HEAD. Starting sidebar diff contains exactly three parent-reviewed `nsh.preview.reload` → `ui.reload_view` substitutions in `nsh_leaf.rs`; retain them.

Writes are limited to sidebar files (no text input) and this scope's specs. No shared root, dependency/patch, window, input behavior, Git, Cargo, network, application, installation, configuration or delegation operations. Parent coordinates four workers and all compilation/tests. Existing Rust development tests remain until final archival/cleanup.

## Continuation acceptance (frozen before code)

The parent integration's first `cargo check -p vega_ui --offline --locked` failed with E0616 at `nsh_leaf.rs:198–199`: GPUI `Pixels.0` is private. Use the actual `From<Pixels> for f32` API; retain finite positive engine dimensions and positive finite native width with one-logical-pixel agreement. Preserve the three reviewed recovery keys.

This continuation owns all sidebar non-input code, including `sidebar/mod.rs` (not shared crate roots). Migrate the existing root background, task/organization/root scroll fades, reorder insertion line and genuine running animation without changing navigation, scrolling, lazy growth, drop decisions, ordering persistence, input, dormant routing or dependencies. Waiting for an answer remains stationary; running/pending rotation is an actual NSH frame-driven visual, paused outside the active visible presentation and under Reduce Motion, disposed with its owner. Hidden/clipped rows must not keep a frame loop alive. Native layout/scroll/drag sampling supplies the existing geometry; native hosts do not draw these decorations. Decorative subtrees remain AX-hidden, not tab targets, and share exact-mount error/reload handling. Original native handlers/controllers and business fences stay authoritative.

Only direct scoped rustfmt and read-only static checks are authorized in this worker. Parent compiles/tests/reviews the integrated source; no Cargo/Git/network/application/dependency patch or cross-writer edit is authorized. Existing Rust tests stay for parent archival/cleanup. Three-platform native acceptance remains pending.

## Independent-review correction acceptance (before code)

This cycle addresses only sidebar findings P1 and P2 on the second handoff hashes. P1: the content leaf is not a decorator; restore its NSH AX descendants, including body/delete-confirmation text, unselected actions, menu titles and disabled reasons. Keep the paint-free native keyboard/focus proxy and the existing exact-projection/authority/controller/mount/layout-acknowledgement/Surface-error activation gate. Only pure decoration may hide its subtree. Explicit action labels must not swallow hint text: expose hints as AX descriptions as well as visible content. Full content exposure takes precedence over deduplicating the selected native focus proxy.

P2: finish the original container's prepaint before sampling the original ScrollHandle, including updated bounds/max offset and clamped offset. Then prepaint and paint chrome using those metrics. Pure decoration must not intercept mouse, wheel or drag: use the existing GPUI prepaint content-mask API to give its hitboxes an empty mask, not a dependency patch or a new scroll model. Restore the real mask for paint. Keep recovery controls outside the decorative pointer isolation. Native layout/interactivity/AX delegation, insertion geometry, active/inactive/hidden/Reduce Motion and original navigation/reorder/scroll behavior remain unchanged.

Parent acceptance must cover first layout, resize and content growth/shrink with offset clamping; pointer/scroll/drag through fades and insertion lines; recovery activation; AX reading and activation of unselected actions, delete confirmation, menu headings and disabled reasons, with stale requests still rejected. Native three-platform verification and parent compilation/tests remain required; this worker performs only scoped rustfmt and static inspection and does not run development tests.

## Initial batch inventory (historical; continuation supersedes the deferred rows)

| Actual entry | Remaining native paint | This batch |
| --- | --- | --- |
| `ThreadsBlock::render_action_menu` | popup background/border/shadow, native scroll edge fade | migrate popup background/border/shadow |
| `ThreadsBlock::render_organization_menu` | same popup chrome | migrate popup background/border/shadow without enabling dormant routing |
| `render_delete_confirm_overlay` | dimming background | migrate dimming background |
| `Sidebar::render` | sidebar background, scroll-edge fade | leave for parent/next batch; shared module root excluded |
| `render_reorder_row` | insertion-line `paint_quad` | leave for next batch, preserve original geometry/gesture |
| organization projection helpers | drag-over and dormant group/timeline chrome | leave routes dormant |
| `render_row_actions` | static running/waiting status glyphs | explicitly not animation parity; follow-up requires active/inactive/hidden/Reduce Motion lifecycle |
| rename/group editor | existing native editor | Agent2 stable interface and parent integration only |

## Initial batch acceptance (historical)

1. NSH actually draws task/organization popup background, subtle border, menu radius and small shadow. Native popup wrapper retains layout, native scroll service, deferred window positioning, occlusion and original dismissal/focus/controller handlers, but does not paint those three decorations. Native scroll fades are recorded residuals, not represented as migrated.
2. A separate transparent NSH decoration Surface fills the actual viewport rather than scrolling with the menu contents. The existing native one-pixel border's layout reservation becomes equal padding, preserving content width/placement. No duplicate task/project projection or business command is added.
3. The delete dimmer is a transparent NSH decoration Surface under the unchanged confirmation contents. Outside dismissal, running-worker/delete controller and route guards remain unchanged.
4. Decoration trees have no focus targets, pointer commands or AX descendants. They are not a mirror of product controls. Existing native keyboard/AX proxies and NSH action contents are not replaced by a second editor or new menu.
5. Decoration mount leases retire stopped/unmounted Surfaces. Error observers are tied to the exact mount; failure exposes a reload action which only reconstructs decoration, never replays a product command. Geometry is supplied by the actual full-size native layout host; no inferred task/menu geometry or static window width is used.
6. Existing action leaf requests must be fenced by a live mount/controller/route/projection and a finite, positive engine layout acknowledgement consistent with native host width. A failed Surface must reject business requests before recovery is rendered. Native activation must not bypass those gates. Recovery creates a new projection and cannot reuse a prior acknowledgement.
7. Do not claim completed sidebar migration or animation parity. Preserve sorting/reorder persistence, drag gestures, confirmation, Tab/Shift-Tab/Escape/refocus, unique projection, empty/errors/disabled reasons, pending guards and AX behavior. Three native platforms remain pending acceptance.

## Verification and handoff

Only direct scoped rustfmt and read-only/static inspection are executed here. Parent must compile final source, execute existing sidebar development regressions and verify native popup geometry/shadow clipping, scrolling/fades, focus/AX, reload failure isolation and delete confirmation on macOS/Windows/Linux. Unexecuted checks are not passing checks. Final handoff records exact files/hashes, parent wiring and uncovered residuals.

## Implemented batch and integration notes

- Added `sidebar/nsh_leaf/chrome.rs`: actual NSH popup background/border/radius/shadow and delete dimming paint in transparent Surfaces. The popup's 8px transparent shadow margin is decoration-only, not product layout; original native border+4px padding is replaced by 5px padding to retain content placement. Native wrapper rounded clipping remains for original scroll behavior, but no background/border/shadow paint remains there.
- Both menus retain their original scroll handles, edge fades, deferred priority, anchor mode and dismissal/controller closures. Decoration is underneath content and outside the native scroll viewport. The delete confirmation public signature and controller path are unchanged.
- Decoration Surfaces have `accessibility_elements_hidden(true)`, `accessible(false)`, `PointerEvents::None` and native focus tab stop disabled. The pinned platform still registers a Surface hitbox; this is not a claim of native hitbox transparency. Content remains above the decoration. Native focus/AX/scroll/hit testing must be verified on all platforms.
- Decoration mount leases stop the exact old Surface on release using a foreground task. Error observer callbacks check mount identity and live lease, stop only their Surface, and expose a deferred priority-4 NSH reload leaf without business replay. The failed lease stays mounted until explicit reload/removal, avoiding automatic retry loops. Failure recovery is not a new product action.
- `nsh_leaf.rs` adds exact Surface mount fencing for both request and error tasks. Engine Layout requests carry width/height; positive finite layout acknowledgement, current projection/live lease/controller/route and nonfailed Surface are required. All pointer/key/native AX activation shares this gate and compares acknowledged width to actual native leaf width (1 logical-pixel tolerance). Projection replacement/recovery clears the acknowledgement. No Rust tests were added, modified or removed.
- Preserved the three parent-reviewed `ui.reload_view` changes. `sidebar/mod.rs`, all inputs, preferences, projection/reorder services, shared `nsh.rs`, dependencies, windows and global roots were not written.

### Parent wiring

No shared code wiring is needed for this batch: `nsh_leaf.rs` declares its private `chrome` child, and both menu entry points plus the dimmer call it directly. Parent must include these files in the integrated source and compile/typecheck them. Agent2 editor integration remains separate; no editor interface is invented here.

### Actual validation

- Direct rustfmt executable: `/Users/puzige/.rustup/toolchains/1.98.0-aarch64-apple-darwin/bin/rustfmt`; observed `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`.
- Scoped Rust 2024 formatting and final `--config skip_children=true --check` on `nsh_leaf.rs`, `nsh_leaf/chrome.rs`, `row_helpers.rs`, `threads_block.rs`, `threads_block/organization/menu.rs`: successful. Initial check reported only formatting differences, which were formatted; this is not a compilation result.
- Read-only index/worktree comparison (Python decoding metadata/loose objects, no Git command): verified initial parent diff and reviewed the final owned delta. Final scans: no trailing whitespace in changed files; new chrome source contains no comments, `try_send`, `.unwrap()` or `.expect(`. Original leaf retains exactly three `ui.reload_view` keys and no `nsh.preview.reload` keys.
- No Cargo/rustc/build/test, Git command, network, application/native UI, install/config/dependency operation or delegation executed. Rustc/Cargo version not queried. Existing four sidebar Rust development tests remain byte-identical, SHA256 `25c5b98d3477eea049839814cfaddd10cc46489a3e3d89a9a013d736c13c73c3`; pending parent execution and final archival cleanup.

### Initial batch parent verification / uncovered (historical)

The initial batch left root background/fades, insertion-line paint and running animation for this continuation. Its native platform acceptance was unrun. The following current result supersedes those residuals and the initial statement that mirrored leaf AX exposure was unchanged.

## Second handoff result (historical; review corrections below supersede AX and prepaint ordering)

### Completed production paths

- `Sidebar::render`: NSH paints the root background; native root retains layout/overflow/lazy-list/wheel/controller behavior. The original scroll handle remains authoritative.
- Root, task menu and organization menu fades now use NSH linear gradients with the original compact token, quarter-viewport length cap, epsilon, hidden/remaining-distance strength, direction and popup radius. Native scroll state supplies dimensions and distances; NSH does not own a second scroll model. Popup/dimmer chrome from the prior batch remains.
- Reorder canvas only records original full/visible bounds and paints nothing. NSH insertion line uses the same active-drag/valid-target/pointer-containment/upper-or-lower-half decision and original line token. No Surface mounts outside a valid drag target. Original source/target projection, drop dispatch, ordering persistence, focus restoration and navigation are unchanged.
- Running/pending task trailing chrome is actual NSH SVG rotation driven by `next_frame`, not a static running glyph. Waiting for an answer retains the stationary pause marker. `when` disposes the frame owner on inactive/background/Reduce Motion, and host content-mask visibility sampling stops clipped rows. Window activation/visibility subscriptions re-sample when presentation returns. Original hover/focus/menu action replacement remains.
- Leaf mounts now use a weak exact-Surface lease; releasing an unmounted leaf stops only that retired Surface via foreground task. Remount/recovery changes mount/projection and clears layout acknowledgement. Existing route/controller/projection/finite-positive engine size/Surface-error gates remain. Native width uses `f32::from(Pixels)`, requires finite positive width and retains the one-pixel consistency check; the reported E0616 source is removed.
- At the second handoff, the entire leaf NSH AX subtree was hidden and only the native paint-free focused-action proxy remained. Independent review identified this as P1, not acceptable AX parity; the correction below restores content exposure. Pure decorations remain AX-hidden and not Tab targets. All three reviewed `ui.reload_view` keys remain in `nsh_leaf.rs`.

### Native platform evidence and fence

Actual cached NSH 0.4 source provides `Styled::bg_linear/gradient_from/gradient_to/rotate`, reactive `element::lifecycle/reduce_motion`, `when`, owner-cleaned `spawn` and `next_frame`. GPUI cached geometry defines `Pixels(pub(crate) f32)` and `From<Pixels> for f32`. No dependency patch is needed or written.

`vendor/nsh-platform-gpui/src/render.rs:772` unconditionally inserts `HitboxBehavior::BlockMouse`, even when engine pointer events are None. The second handoff prepainted decoration before the original base, then painted chrome after it. Independent review identified stale ScrollHandle sampling as P2; this ordering is superseded below. Native focus, wheel propagation, drag and clipping remain acceptance items.

### Second handoff files / SHA256 (historical)

Paths relative to `crates/vega_ui/src/sidebar/`:

| File | SHA256 |
| --- | --- |
| `mod.rs` | `1fbccc77f44fbabba1dc918415a9d81f4d097d4d001163654ef5f5f0825532ae` |
| `nsh_leaf.rs` | `fc80212ddbc59c87a91ab52d9c8282a2417ac210089e653c1ad20f78255994c7` |
| `nsh_leaf/chrome.rs` | `fb017926e129ea377ab42882ee84ef51921541ee8f27f9fbc4b06dff438c9c5a` |
| `nsh_leaf/geometry.rs` (new) | `2829fce2700b51c73050b2cfb6b794f2695aefe64ec415bfe5fdb17eeccafe96` |
| `threads_block.rs` | `3d73bf3986074a74171e1e4a75296b0c377c5cbb73368b0309b2a6ec1fc19569` |
| `threads_block/organization/menu.rs` | `1a9ba5695f46920cf896fd44879d4bc459536d7ac74195c98a71e9045e2cea03` |
| `threads_block/organization/reorder.rs` | `c066a637bfaec155f88bbf3605d8a4ab254d11de6337a9a6d0d4234b222593d5` |

Prior delivery unchanged: `row_helpers.rs` = `dc6413f15b219ec72d1aa41f1dc8080244fd4f1192faf9bd154e2ca7a1902ba4`. `docs/907/nsh-sidebar.md` was not rewritten this continuation. This document's final hash is emitted separately.

### Second handoff verification (historical)

Direct `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)` from toolchain directory `1.98.0-aarch64-apple-darwin`; scoped Rust 2024 formatting, then final `--config skip_children=true --check` on the seven Rust files above completed with exit 0. Initial check showed formatting-only differences; this is not a compile result. Final Python static scan: zero trailing whitespace; new chrome/geometry contain no comments, unwrap/expect or try_send; exactly three reviewed reload keys and zero old keys. Focused production root/threads/projects/row/menu/reorder/organization-render scans find no `paint_quad`, `.bg(` or `tracked_scroll_edge_fade` remaining in those entry files. Dormant `organization/projections.rs` is intentionally unchanged and not enabled.

No Cargo/rustc/build/test/Git/network/application/install/config/dependency operations or delegation executed. Rustc/Cargo version not queried. Existing four sidebar Rust development tests remain unchanged (`25c5b98d3477eea049839814cfaddd10cc46489a3e3d89a9a013d736c13c73c3`), pending parent execution and final archival cleanup. No new Rust tests were added.

### Parent wiring / remaining acceptance

No shared-root/window/dependency wiring required: `nsh_leaf` declares private `geometry`; every call is wired inside sidebar. Parent includes all seven Rust files, re-runs `cargo check -p vega_ui --offline --locked`, relevant tests and independent review. Agent2 native input/rename/group editor integration remains parent-owned, untouched here. Dormant group/timeline routing is unchanged, not deferred production work and not activated.

Required native acceptance remains unrun on macOS/Windows/Linux: themes/narrow/resize/first layout; root and popup fade no-overflow/top/middle/bottom and wheel/lazy-growth parity; same-kind reorder upper/lower insertion, drop persistence, cancel and refocus; running/pending → waiting → terminal, hover/menu replacement, clipped rows, hidden/minimized/inactive/Reactivate/Reduce Motion, close/reopen/reload/unmount; deletion confirmation, errors and disabled reasons; Tab/Shift-Tab/Escape/reopen and complete native AX content/action exposure. The selected native focus proxy is retained; deduplicating it must not hide content. Static completion is not full production acceptance, animation runtime proof, passing compilation or three-platform validation.

## Independent-review corrections — READY for parent validation, stop writing

### P1 disposition: content is not decoration

Removed `accessibility_elements_hidden(true)` from the content leaf. Its root remains `accessible(false)`, which the actual adapter treats as a flat container rather than a hidden subtree (`access.rs::part` / `role_for`). Body text, delete-confirmation explanation, unselected buttons, menu headings and disabled labels now follow the existing NSH AX adapter. Item hints additionally use `Accessible::hint`, which the adapter maps to a description; explicit button labels otherwise suppress folded child text. Disabled state and non-actionability remain unchanged. The native paint-free selected-action proxy and keyboard/focus behavior are retained. Selected-action duplication may remain; this cycle does not suppress necessary content to deduplicate it.

AX activation still travels through the original NSH press handler → ordered `Request::Activate` → `Leaf::activate`: exact projection, mount lease, authority/controller, drag, finite positive layout acknowledgement/native width agreement and Surface-error checks were not edited. Recovery keys remain exactly three `ui.reload_view` uses. Pure chrome alone retains its AX-hidden subtree.

### P2 disposition: current-frame metrics without pointer occlusion

`Overlay::prepaint` now completes and retains the original base's prepaint state before laying out/prepainting chrome. The original GPUI `clamp_scroll_position` updates clamped offset, `ScrollHandle.max_offset` and bounds before the decoration sampler reads them. Sampling precedes the decoration Surface prepaint/tick, so the geometry publication is available to that prepaint. No second scroll state or deferred stale-frame sample is introduced.

`sample_decoration` samples under the real mask, then applies `Window::with_content_mask` with an empty `ContentMask` only while prepainting its pure decorative child. Actual GPUI `insert_hitbox` stores this mask, and frame hit testing intersects bounds with it before considering `BlockMouse`; `Bounds::contains` excludes zero-area bounds. The mask is popped before normal paint, so NSH chrome paints under the real ancestor clip after the base. Native input/content is neither masked nor reordered under an unmasked chrome hitbox. This also isolates insertion/background/popup/dimmer decoration. The failed-decoration recovery branch returns before `sample_decoration` and retains ordinary pointer/AX activation. No vendor or dependency changes.

### Final code SHA256 (this correction)

| File, relative to `crates/vega_ui/src/sidebar/` | SHA256 |
| --- | --- |
| `nsh_leaf.rs` | `df5c976f22c23a1b6236d74291ad6f701de6558ae3a9e15687f69ac5eff3b77a` |
| `nsh_leaf/geometry.rs` | `e932c02f13aa763b996670fc22ef5b2fee80e02d84524a95f8dded95c56a8996` |
| `nsh_leaf/chrome.rs` | `4730d94358fd1d6cf8a7fff966e13fe0c0f1e6e878fd10dc509f5b13727420d0` |

Only these three Rust files and this spec were written this cycle. This document's final hash is reported separately. All other sidebar files, four existing development tests, tool files, input/dormant routes and other writers' outputs were untouched.

### Verification performed / not performed

- Verified initial hashes of the three Rust files and this spec exactly match the second handoff; existing tests still hash to `25c5b98d3477eea049839814cfaddd10cc46489a3e3d89a9a013d736c13c73c3`.
- Read the actual locked `gpui-pre 0.3.9`, NSH 0.4 builder and vendor adapter contracts, including scroll clamping, content-mask intersection/restore, hit testing, Surface prepaint/paint, AX flattening, description and press dispatch. Dependency reads only; no vendor writes.
- Ran direct `/Users/puzige/.rustup/toolchains/1.98.0-aarch64-apple-darwin/bin/rustfmt --edition 2024 --config skip_children=true --check` for the three changed Rust files: exit 0. Actual `rustfmt --version`: `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`.
- Read-only Python source inventory: zero trailing whitespace/comments/unwrap/expect/try_send in these three Rust files; three reviewed reload keys, zero old keys. These are static observations, not behavioral tests.
- No Cargo/rustc/build/test/Git/network/App/install/configuration or delegation executed. No new development tests added; existing four remain for parent execution and final archival cleanup. Rustc/Cargo versions were not queried.

### Parent acceptance still required

No shared wiring is needed. Parent compiles and independently reviews these exact final bytes. Native macOS/Windows/Linux acceptance remains unrun: AX body/delete-confirmation reading, menu headings, unselected enabled/disabled actions and reasons, normal AX activation and stale-projection rejection; Tab/Shift-Tab/Escape/reopen/refocus; first-layout/resize/content-growth/content-shrink fades including offset clamp; pointer/wheel/drag through fade and insertion chrome; deletion overlay outside-click and recovery-button activation. Preserve the previous navigation/reorder persistence and real animation lifecycle acceptance. This worker reports static fixes, not passing runtime acceptance or compilation.
