# #907 Message-position rail and preview NSH pixels

## Acceptance and scope before implementation

The author inventory confirmed reachable GPUI mark/background/border/shadow/text drawing in `conversation_stream/message_anchors.rs`. Replace only those visible leaves; retain the existing anchor projection, measured-list geometry/cache, sanitization, selected/current identity, rail window, keyboard focus, hover transition, click navigation, preview wheel routing and full accessible label. No change to message-location authority or persistence, no extra copy and no legacy visual fallback.

- A mark keeps its exact existing selected/rest width, track height, semantic color and centered hit region. Drawing is NSH and noninteractive; the outer Fast host retains the existing pointer/keyboard/AX contract.
- Preview keeps the existing available-width bound, metadata font, inherited weight/style/line-height, three-line ellipsis, full accessible label, padding, radius, border, background and small shadow. Natural height is computed from actual text layout, never a fixed one-line estimate. Measurement-only Fast text may request layout but must never prepaint/paint; NSH renders the actual text and chrome.
- Stable leaf projections retain a Surface. Changed content/theme/font/width updates the actual projection. Unmount stops the Surface. NSH children neither consume parent input nor add duplicate AX nodes or Tab targets.
- Preserve original clipping and redraw behavior. Development validation checks real mounting/layout and the existing source/scroll regressions where affected; native pixels/OS focus/three-platform acceptance remain distinct.

## Minimal plan

Add a private child module under `message_anchors/` owning retained passive NSH mark/preview leaves. Capture inherited font family, weight, italic style and resolved line height from `window.text_style()` inside the actual leaf's `request_layout`; the preview constructor does not take `window`. Keep all business callbacks and Fast structural/event hosts in the original file; remove only their visible styling/text children in favor of the real NSH child. A custom Element measures the preview with the existing three-line layout rule, then prepaints/paints only a passively embedded Surface with matching dimensions and inherited typography. No dependency/vendor/shared Surface changes.

## Author validation implementation handoff

Author lane only, on user-supplied `feat/907-nsh-full` HEAD `0b5ea426abfd8d4358489e916273d89474fb190f` plus the existing dirty migration. No Git command was used to independently re-read or modify HEAD. This handoff is not independent review, consolidated review, runtime success, or native acceptance.

Only these four authorized files were written:

- `crates/vega_ui/src/conversation_stream/message_anchors/pixels.rs`
- `crates/vega_ui/src/conversation_stream/message_anchors/mounted_regressions.rs`
- `crates/vega_ui/tests/nsh_message_anchors_integration.rs`
- `docs/907/nsh-message-anchor-pixels.md`

No Cargo, rustc, network, Git mutation, Cargo manifest edit, parent `message_anchors.rs` edit, module-root edit, vendor edit, shared Surface edit, or settings repair. Parent reported an earlier production `check -p vega_ui` exit 0 in 9.32 seconds (`target/nsh-anchor-pixels-check.*`); those logs were not inspected here, and that check does not validate these final bytes or the new tests.

### Production callchain and preserved authority

`conversation_stream/message_anchors.rs:436-474` computes the bounded preview width and deferred placement, calls `render_message_anchor_preview` at :447, and retains the existing rail/preview geometry. Its mark callback subtree at :583-614 keeps the original AX label, centered hit host, parent focus and message-location request; only :614 delegates drawing to `pixels::mark`. The preview wrapper at :627-663 keeps its complete AX label at :636, width, hover lifecycle and existing list-wheel routing at :653-660, delegating the visual leaf at :638 to `pixels::preview`.

`message_anchors/pixels.rs:98-145` captures actual inherited typography and requests real Fast text layout, including `line_clamp(3)`. The measurement tree is retained only as `Measurement::_element`; the leaf's prepaint/paint at :148-241 dispatches only its separate retained passive Surface child, not the measurement tree. No measurement, message-location, scrolling, business, security, service or persistence authority was changed.

### Concrete production defect corrected: shadow translation

The initial production leaf used NSH `.shadow_sm()`. The names do not define identical pixels:

- Installed `gpui-fast-macros-0.1.13/src/styles.rs:428-435`: Fast small shadow has two black 0.1-alpha layers, offsets `(0,1)` each, GPUI blur radii `3/2`, spreads `0/-1`.
- `vendor/nsh-core/src/tailwind/theme.rs:351`: NSH small shadow is one layer, `0 1px 2px rgb(0 0 0 / 0.15)`.
- `vendor/nsh-platform-gpui/src/decor.rs:438,461` converts CSS blur to sigma by dividing by two; :242 forwards sigma to GPUI `blur_radius`.

Minimal repair at `pixels.rs:244-260,327-341`: derive the existing Fast `div().shadow_sm()` style values without requesting its layout/prepaint/paint, convert each layer to NSH, multiply GPUI blur by two at the CSS boundary, and apply explicit NSH `.shadow(shadows)`. Thus projected CSS blur `6/4` reaches GPUI as `3/2`; the original layer count, offsets, spread, inset and source alpha are retained. NSH still packs colors into 8-bit channels. No shared style/token or vendor implementation was changed. Exact rendered appearance has not been observed.

### Observation seam and six-case inventory

The only production observation seam is gated by the existing `nsh-reading-integration` feature. It records the actual leaf Surface entity ID, renderer setup/projection, native root/text `on_layout`, native `TextLayout` events, host bounds, zero-area input mask, applied paint clip, errors and release after the actual `AppView::stop()` call. It does not select successful outcomes or supply geometry/authority. The same native elements and production constructors execute under observation; no mirror renderer or source-string assertions were added.

The integration runner uses `current_platform(true).text_system()`, asserts installed fonts, creates one `TestAppContext` on `main`, calls official `cx.executor().allow_parking()`, and executes six cases serially. Each scheduler drain remains **2 seconds / 512 ticks** and each readiness wait **10 seconds** (`mounted_regressions.rs:297-338`); no timeout/budget increase or ignored case.

| Case | Authored assertions; NOT RUN |
| --- | --- |
| `short_preview_uses_actual_natural_height_and_original_shadow` (:509) | Production preview Leaf, actual native one-line layout, 36px natural height with explicit inherited 18px line height and 18px total vertical chrome, 6px radius/8px padding, inherited normal metadata typography, two explicit shadow layers, full label metadata from the actual production wrapper. |
| `narrow_long_cjk_and_emoji_preview_clamps_three_native_lines` (:540) | At 132px width, Latin long text then CJK/emoji use three native lines and height 72px, line/text frame containment, truncated UTF-16 range with complete projected source, complete actual production AX label metadata, then short/wide natural-height restoration. |
| `mark_states_keep_native_dimensions_and_semantic_colors` (:603) | Rest/selected/rest widths 6/16/6, native and host height 2px, centered host, exact semantic colors, an actual GPUI painted quad matching the native mark's geometry/color, retained identity. |
| `stable_frames_and_changed_font_theme_content_width_retain_surfaces` (:632) | Eight stable frames do not republish or recreate either Surface. Changed content/light theme/installed platform font family/bold/italic/16px inherited line height/184px width reaches native projection/layout with retained Surface IDs. |
| `clipped_passive_leaves_forward_input_once_and_skip_tab_focus` (:690) | Translate 12px, clip preview to 100px without changing its 280px layout, click and wheel over each mounted leaf reach parent exactly once and preserve focused parent, forward/reverse Tab skip both passive Surfaces, expanding clip restores 280px paint extent. |
| `unmount_stops_both_surfaces_and_remount_creates_new_identity` (:765) | Remove both real leaves, observe weak Host release and actual stop callback once each, remount, verify two distinct new native Surface identities and exactly two renderer setups per leaf lifetime. |

The test fixture mounts the real production `mark`/`preview` leaves under structural/input hosts, not the whole production rail/controller. The AX metadata assertion additionally constructs `render_message_anchor_preview` itself and calls its actual `write_a11y_info`; it does not reproduce the label construction. Parent event delivery tests are leaf passivity tests, not revalidation of message-location business behavior.

### Parent-only wiring still needed

This lane intentionally did not edit either parent-owned module file or Cargo.toml. Add under the existing feature:

In `conversation_stream/message_anchors.rs`:

```rust
#[cfg(feature = "nsh-reading-integration")]
pub use pixels::mounted_regressions;
```

In `conversation_stream/mod.rs`:

```rust
#[cfg(feature = "nsh-reading-integration")]
pub use message_anchors::mounted_regressions as message_anchor_mounted_regressions;
```

In `crates/vega_ui/Cargo.toml`, register the new runner like the existing icons/settings targets:

```toml
[[test]]
name = "nsh_message_anchors_integration"
harness = false
required-features = ["nsh-reading-integration"]
```

These are development-only exports/registration, not a new product API; archive and remove the Rust development tests and their exclusive seams/wiring before master under repository policy.

### Actual validation and command requests

Executed successfully (exit 0):

```sh
rustfmt --edition 2024 --config skip_children=true crates/vega_ui/src/conversation_stream/message_anchors/pixels.rs crates/vega_ui/src/conversation_stream/message_anchors/mounted_regressions.rs crates/vega_ui/tests/nsh_message_anchors_integration.rs
rustfmt --check --edition 2024 --config skip_children=true crates/vega_ui/src/conversation_stream/message_anchors/pixels.rs crates/vega_ui/src/conversation_stream/message_anchors/mounted_regressions.rs crates/vega_ui/tests/nsh_message_anchors_integration.rs
```

Focused read/search checks inspected the live anchor callsites, exact measurement/prepaint/paint separation, native TextLayout payload semantics, installed Fast scaled-pixel/AX APIs, icon and Settings mounted harnesses, current native small-shadow preset and actual native shadow-to-GPUI conversion. Searches counted six public case entrypoints and unchanged scheduler limits. Formatting is a parse/style check, not compilation.

After wiring, parent should run centrally (NOT RUN by this lane; use the parent's authorized pinned Cargo setup):

```sh
cargo check -p vega_ui
cargo test -p vega_ui --features nsh-reading-integration --test nsh_message_anchors_integration --no-run
cargo test -p vega_ui --features nsh-reading-integration --test nsh_message_anchors_integration
cargo test -p vega_ui --features nsh-reading-integration --test nsh_reading_integration --test nsh_scroll_integration --test nsh_reading_geometry
```

Parent must record actual compiler/Cargo versions, final dirty-source hashes, command output/exit/duration and actual `6 passed / 0 failed / 0 not executed / 0 ignored` only if observed. This lane did not query Cargo/rustc versions, compile, discover or run any case. Runtime result: **0 executed; six authored, all unrun**. No passing result is claimed for typography, clamp, shadows, lifecycle or input.

### Explicit evidence limits / remaining acceptance

- Native `TextLayout` events provide real line count, typographic geometry and original-source UTF-16 ranges. They do **not** expose the ellipsis glyph itself: NSH fills `TextLine.text` from source ranges (`vendor/nsh-core/src/core/mod.rs:7547-7559`). The case detects three-line truncation and full source preservation but does not prove tail-ellipsis pixels versus clipping. Actual three-line ellipsis appearance remains pending parent/native observation; do not call it accepted from these assertions.
- GPUI TestPlatform's window does not implement AX adapter activation; `Window::debug_a11y_tree_json` requires a captured activated tree. No shared platform change or pretend tree was added. The full production wrapper label metadata is checked directly, and passive root hiding remains at `pixels.rs:268-271`, but **no duplicate live AX nodes** is not dynamically established by this runner. Parent/native AX inspection remains required.
- Actual Fast parent input/focus/Tab behavior is exercised by authored cases, but OS focus, screenshot appearance, border/radius/shadow rasterization, visual CJK/emoji fallback, three-platform acceptance and full rail navigation/hover integration are not established. Neither shadows passed into the real renderer nor native text metadata should be reported as screenshot acceptance.
- If strict frame-containment, line-height or lifecycle assertions fail, capture the first concrete native bounds/content/font/Surface evidence and fix the responsible leaf or test-harness layer. Do not infer typography correctness from the parent's earlier production-only check; do not extend 2s/512/10s budgets or weaken assertions to obtain a pass.

### Exact source-content hashes at handoff

SHA-256; paths relative to the worktree unless absolute:

| Source | SHA-256 |
| --- | --- |
| Parent `crates/vega_ui/src/conversation_stream/message_anchors.rs` (read only) | `fb2db89928a0c3f980ce14ecbadaeb0f3c7a3b99004b54cb6b333110f7969fda` |
| `crates/vega_ui/src/conversation_stream/message_anchors/pixels.rs` | `9dc4ee4985fec046c72c8226285a11ab80e5c398c45145610ed6f408d46288d3` |
| `crates/vega_ui/src/conversation_stream/message_anchors/mounted_regressions.rs` | `892ee1c0bc63fa668f955f70140c1cee3e7ea41c42587766681014dfe801dc2c` |
| `crates/vega_ui/tests/nsh_message_anchors_integration.rs` | `64c27a5b7f5d11dfd2c858e38fcd29db093bf716b467c7dab075251d9c24382a` |
| `vendor/nsh-core/src/tailwind/theme.rs` (read only) | `825c1046fa9a292c0adb9dc1205cc58673ce05b33c81f9e3df610de1394d1494` |
| `vendor/nsh-platform-gpui/src/decor.rs` (read only) | `ff99994c61c1887046c52bf9243f065695d322a9433581148a0cefd0afae181b` |
| `/Users/puzige/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/gpui-fast-macros-0.1.13/src/styles.rs` (read only) | `5fbd5dac9b7cdc954860a018f6b6925e49c82b4dcfe29424ecea828d47a3ff40` |

The document's own final hash is reported outside this file to avoid self-reference. Writes stopped at that handoff; the following follow-up supersedes its pending-wiring and all-unrun status for the original bytes only.

## Actual mounted failure follow-up: fixture invalidation ordering

Author implementation/diagnosis lane, not independent review or native acceptance. Parent now owns and has registered the module exports and Cargo target. Parent's only intervening edit to this lane was `nsh_core::text::TextLine` → public `nsh_core::TextLine`. Pre-follow-up mounted source SHA-256: `e5e75ce85321b55033bc01c6b1ff84d54f507f425e0c021e353cc2aa29914979`.

### Original evidence inspected

- `target/nsh-closure-ui-signal-build.json` is Cargo JSON-lines, not one JSON document. Its final record is `build-finished: success=true`; it contains the registered anchor executable artifact. Parent reports the centralized 11-target build exited 0 in 33.06s. This successful build predates the follow-up edit.
- `target/nsh-closure-mounted-execution.json` records the anchor executable `target/official-nsh-validation/debug/deps/nsh_message_anchors_integration-b317b2d7635563d8`, executable SHA-256 `c0ec611dd507fe231b54fb9b6a4cbf75f293e5dbe6a372b522cd3434945bf313`, exit 1, elapsed 13.382s, no watchdog timeout.
- Original stdout: short natural-height/shadow case PASS; **1 passed / 1 failed / 4 not executed / 0 ignored**. Original stderr: case 2 panics at old `mounted_regressions.rs:328`, stage `actual native layout and text lines`. It contains runtime job/draw counters but no host/native bounds, label, font, or line count. It does not identify which iteration of case 2 timed out or which readiness conjunct failed.

| Original evidence file | SHA-256 |
| --- | --- |
| `target/nsh-closure-nsh_message_anchors_integration.stdout` | `99868509407947a923c68d89542f88a512dd3cf21566cab92499006a40f91ffe` |
| `target/nsh-closure-nsh_message_anchors_integration.stderr` | `d4126eff435f367a869110f292afd0b5b68e8e0c44458de3df92f07ece60b35e` |
| `target/nsh-closure-mounted-execution.json` | `a7680ae29c5381a3ebdf280421a1416e37141244c13df49bfe362c652a40746d` |

### Static diagnosis and minimal responsible-layer repair

A concrete observation-ordering hazard exists in the fixture: old `Fixture::change` notified/mutated the root **before** clearing observed text lines. Installed `gpui-fast-0.1.9/src/app/test_context.rs:57-64` delegates entity update to `App::update_entity`; `src/app.rs:3196-3210` wraps it in `App::update`. `src/app.rs:1985-2042` flushes effects and draws dirty test windows before update returns. Therefore fixture mutation is not a publication-free boundary. Clearing after that boundary can discard an observation produced during the mutation. Native text events are not heartbeats: `vendor/nsh-core/src/core/mod.rs:6520-6536` skips unchanged content bounds and emits `TextLayout` when a layout is produced. Repeated frames alone do not promise to restore a discarded event.

Minimal repair at current `mounted_regressions.rs:395-410`: invalidate line observations **before** root mutation/notification and release the observation mutex before entering GPUI. Do not invent events, preserve stale lines, manufacture native bounds, republish a Surface, or force production changes to satisfy the fixture.

**Attribution limit:** the original failure lacks the metrics needed to establish that this ordering hazard caused that particular timeout; native publication is asynchronous as well. This is a concrete fixture-ordering repair, not a claim that production geometry/font/clamp is correct or that case 2 is fixed at runtime. The next run must confirm or reject this hypothesis. No production typography, geometry, line clamp, shadow, input, authority or shared Surface code changed in this follow-up.

Current `mounted_regressions.rs:321-350,359-362,372-391` adds timeout-only diagnostics and includes the exact awaited synthetic label in the stage. One timeout captures two leaf snapshots, with at most the first three native line records per leaf: Surface identities, setup/projection/stop/paint counts, actual projected content and inherited font/line-height, radius/padding, host bounds, native root bounds, native text bounds, line-label/count/ranges/metrics, passive mask, clip and error. No per-frame logging or new dependency/API/seam was added. These fixed synthetic fixture labels bound the captured content.

### Preserved gates and exact next parent verification

Readiness still requires the requested label, nonempty actual native lines and both leaf native widths/heights within **0.5px** of the actual Fast hosts. Three native lines, natural host height ≤72px (72px at the narrow long fixture), line/frame/padding containment, truncated source range, full production AX-label metadata, identity retention, input and lifecycle assertions remain unchanged. Budgets remain **2s / 512 ticks / 10s**. Six cases remain registered, at current mounted-source lines **546 / 577 / 640 / 669 / 727 / 802**, in the same order and with the same names recorded above.

Parent should centrally rebuild and run this target once with the existing pinned setup/environment, saving complete stdout/stderr and exit/duration, not rerun the old executable:

```sh
cargo test -p vega_ui --features nsh-reading-integration --test nsh_message_anchors_integration --no-run
cargo test -p vega_ui --features nsh-reading-integration --test nsh_message_anchors_integration
```

Both commands are **UNRUN by this lane**. On any timeout, inspect the captured requested label, projected label/font, `lines_label`/count, host/native size deltas and text frame first. A current projection and populated lines with divergent sizes indicates geometry/layout rather than lost line publication; missing/stale line publication with matching current geometry instead isolates the observation/event path. Keep all strict assertions and budgets. If the run succeeds, record the actual six-case summary; success is not presumed. Ellipsis glyph pixels and live duplicate-AX absence remain pending native acceptance, unchanged from the earlier limits.

This follow-up ran only scoped `rustfmt --edition 2024 --config skip_children=true` on the mounted file and `rustfmt --check` on the three authorized Rust files: **exit 0**. No Cargo/rustc execution, network, Git mutation, Cargo/module-root edit, vendor/shared write or unrelated repair. Only `mounted_regressions.rs` and this document were written; `pixels.rs` and the runner remain byte-identical.

### Follow-up final source hashes

| Source | SHA-256 |
| --- | --- |
| `crates/vega_ui/src/conversation_stream/message_anchors/pixels.rs` (unchanged) | `9dc4ee4985fec046c72c8226285a11ab80e5c398c45145610ed6f408d46288d3` |
| `crates/vega_ui/src/conversation_stream/message_anchors/mounted_regressions.rs` | `8589a44b6a74fbbbeba0e85a608005b13eec193171d6a7f09e3c793e79184341` |
| `crates/vega_ui/tests/nsh_message_anchors_integration.rs` (unchanged) | `64c27a5b7f5d11dfd2c858e38fcd29db093bf716b467c7dab075251d9c24382a` |
| Parent `crates/vega_ui/src/conversation_stream/message_anchors.rs` (read only) | `d5f7b78f6d47d6048072ad007189a4332803552fd935d47345ee62b38c5c5800` |

The document's final hash is reported outside this file. Stop writes after the final read/hash check; parent owns centralized rebuild/execution and any resulting production-versus-fixture diagnosis.

## Actual rerun follow-up: tracked fixture Tab stops and fallible cleanup

Author implementation lane only, not independent review or native acceptance. Parent supplied the same HEAD plus dirty migration and stopped writes to the four authorized files. The incoming mounted-source hash matched `8589a44b6a74fbbbeba0e85a608005b13eec193171d6a7f09e3c793e79184341`. This follow-up writes only `mounted_regressions.rs` and this document; production `pixels.rs`, the runner, all parent-owned modules, Cargo files, vendor and shared code remain unchanged.

### Observed original evidence

`target/nsh-closure-repair-build.json` is Cargo JSON-lines, not a single JSON document; its `build-finished` record reports `success: true`. Parent reports build exit 0 in 37.82s. An initial read-only metadata extraction treated that file as one JSON value and failed with `Extra data`; reading its individual records resolved that observation error without repeating any build or execution.

`target/nsh-closure-repair-execution.json` identifies `nsh_message_anchors_integration-b317b2d7635563d8`, executable SHA-256 `5675b1b05753592d8310e19c95b22c8dac2e066411125ba994d8bb7b6fc0835d`, exit **-6**, elapsed **4.082s**, watchdog false. Stdout records exactly the first four cases passing, including the formerly timing-out narrow long/CJK/emoji case, mark dimensions/colors, and retained font/theme/content/width updates. Stderr :145 starts case 5; :148-150 reports the forward-Tab assertion at the incoming mounted source :773. It does not record the actual focused handle. At :152-153, fixture teardown hits `VisualTestContext::update`'s `window not found` unwrap; :255-257 reports destructor double-panic/abort. There is no final runner summary; observed inventory is **4 PASS / fifth assertion failed / sixth unrun**, not a fabricated six-case summary.

| Original evidence | SHA-256 |
| --- | --- |
| `target/nsh-closure-repair-build.json` | `809ca366eca58442e3d0c0b4bb01d7bcaf9e49cfaee3110f129f5eedc37b907a` |
| `target/nsh-closure-repair-build.stderr` | `0ff4fe5fce8c86fab8cb02d06491b5f23e1565007e095a3df9774fd15aef63a6` |
| `target/nsh-closure-repair-execution.json` | `6925a29d6853b20a0968eb82ae159cb67bf655fe9dba636e842f85275e0d8d5a` |
| `target/nsh-closure-repair-anchors.stdout` | `15bd53ddd56db7e2b065144de8446c06234e4fd0b49653ec26fbdc7cbdf2af13` |
| `target/nsh-closure-repair-anchors.stderr` | `f6ec05deaa87e8bb173a90598d13a567c3fda56685eb60dea838811652ae845f` |

### Responsible layer and minimal repair

The fixture's incoming `Root.focus` at :266 constructed two plain `cx.focus_handle()` values. Installed `gpui-fast-0.1.9/src/window.rs:553-564` defaults both to `tab_stop=false`. The fixture divs set `.tab_stop(true).track_focus(...)` at mounted source :188-189 and :219-220, but `src/elements/div.rs:2282-2297` applies the element's Tab flag only when there is **no explicit tracked handle**. Painting registers the tracked handle at :2590-2591. Consequently those two fixture handles were not traversable Tab stops despite the div flags. `src/tab_stop.rs:78-89,111-145` filters disabled entries, and `src/window.rs:2470-2477` does nothing if no eligible next handle exists. With this fixture's registered handles all disabled, the traced expected outcome is to remain at `focus[0]`, not stop at a passive Surface. The original log cannot prove which handle was actually focused; the old assertion's message misattributed every wrong destination to the Surface.

The production passive chain does not exhibit that registration defect: `pixels.rs:158-160` creates the Surface and sets its focus handle's Tab flag false; shared `nsh.rs:208-209` delegates to `AppView`; `vendor/nsh-platform-gpui/src/app.rs:799-802` clones its owning focus handle and :937-945 tracks it on the native host. Installed `window.rs:595-600` writes the opt-out into the shared focus map, and :650-652 implements clone by reloading from that map via :569-579. Thus later native host tracking sees the opt-out, even though AppView initially creates a Tab-enabled owner at vendor :308. No production focus change is justified by this failure.

Minimal repair at current mounted source :266 constructs the two actual tracked fixture handles with `.tab_stop(true)`. Forward traversal must still reach `focus[1]`, and reverse traversal must still return to `focus[0]` (:801-806); no assertion is skipped, weakened, delayed, or replaced with a different expected target. The new strict helper at :442-463 reports actual `window.focused(cx)`, both fixture identities/Tab flags, and at most two live Surface identities/handles/Tab flags/focused states **only on assertion failure**. It adds no public production API or new observation seam. Existing click/wheel exactly-once and no-focus-steal checks remain unchanged.

Fixture teardown at :557-575 now uses fallible `TestAppContext::update_window` instead of `VisualTestContext::update`'s unconditional unwrap (`gpui-fast-0.1.9/src/app/test_context.rs:868-871`). It still stops live Surfaces and removes the window. A cleanup error panics normally, preserving a genuine standalone cleanup failure; during an existing unwind it is reported to stderr without creating a second panic. This follows the current settings fixture's exact pattern at `settings/nsh_tail/mounted_validation.rs:415-438`. The inspected icon/recovery destructors still use the infallible wrapper, so they are historical teardown precedents, not evidence that the wrapper is safe during this failure. No edits were made there.

The parent-owned production rail remains read-only: `message_anchors.rs:478-485` tracks `message_anchor_focus`, :600-614 retains the pointer-focus/navigation authority, :616 delegates only mark pixels, and :629-640 retains the complete preview label and delegates preview pixels. This fixture fix makes no claim about full production rail Tab/navigation correctness and does not expand into auditing that separate controller.

### Preservation, current validation and next parent run

The previous pre-mutation line invalidation at mounted source :395-410 is retained. Readiness still requires the awaited label, nonempty actual native lines, and both native/host dimensions within **0.5px**. Natural height ≤72px, three-line truncation, padding/frame containment, full AX-label metadata, dimensions/colors, projection retention and lifecycle assertions are unchanged. Budgets remain **2s / 512 ticks / 10s**. No comments, lint suppressions, dependencies, Cargo/module edits, production geometry/input/authority changes or shared Surface changes were added.

Scoped `rustfmt --check --edition 2024 --config skip_children=true` on the three authorized Rust files returned **exit 0** after these edits. That is a formatting/parse check, not compilation. No Cargo/rustc, network, Git mutation or mounted execution ran in this lane. Focused searches/read-only inspection covered the exact panic and runner records, fixture creation/render/traversal/teardown, production Leaf/Surface/AppView registration chain, installed GPUI handle/clone/Tab traversal semantics, and existing settings/icons/recovery patterns. Unrelated failure records in the shared execution metadata were excluded from repair scope.

All six runner entries remain, in their existing order:

| Case | Current mounted-source entry | Original rerun evidence |
| --- | --- | --- |
| `short_preview_uses_actual_natural_height_and_original_shadow` | :577 | PASS |
| `narrow_long_cjk_and_emoji_preview_clamps_three_native_lines` | :608 | PASS |
| `mark_states_keep_native_dimensions_and_semantic_colors` | :671 | PASS |
| `stable_frames_and_changed_font_theme_content_width_retain_surfaces` | :700 | PASS |
| `clipped_passive_leaves_forward_input_once_and_skip_tab_focus` | :758 | Forward Tab assertion failed before this repair |
| `unmount_stops_both_surfaces_and_remount_creates_new_identity` | :823 | Unrun |

Parent must centrally rebuild the changed fixture and run the resulting exact target using the existing pinned setup/environment; do not replay the old binary:

```sh
cargo test -p vega_ui --features nsh-reading-integration --test nsh_message_anchors_integration --no-run
cargo test -p vega_ui --features nsh-reading-integration --test nsh_message_anchors_integration
```

Both commands are **UNRUN by this lane**. Record final source and binary hashes, complete stdout/stderr, exit, duration and actual case counts. Next verification must preserve all first-four regressions, demonstrate Before → After → Before while both passive Surface handles remain non-Tab, and execute the sixth lifecycle case. A failure now reports actual handle identities rather than assuming the Surface was focused. The final-version runtime result remains unknown. Actual ellipsis glyph pixels, native live AX duplicate absence, OS focus and full native acceptance remain pending under the unchanged limits above.

### Final source identities for this repair

| Authorized source | SHA-256 |
| --- | --- |
| `crates/vega_ui/src/conversation_stream/message_anchors/pixels.rs` (unchanged) | `9dc4ee4985fec046c72c8226285a11ab80e5c398c45145610ed6f408d46288d3` |
| `crates/vega_ui/src/conversation_stream/message_anchors/mounted_regressions.rs` | `eca2fa0802e5689483952f8b8ac46b403fe47043b95976922fffbe620905e17e` |
| `crates/vega_ui/tests/nsh_message_anchors_integration.rs` (unchanged) | `64c27a5b7f5d11dfd2c858e38fcd29db093bf716b467c7dab075251d9c24382a` |

Read-only focus-chain evidence identities: `nsh.rs` = `e35474eab2519497b89216193213967c785faf44b8d653f99d8835b61441270c`; `vendor/nsh-platform-gpui/src/app.rs` = `7d73eda709a7ab2cafa23384c35fedb33935374ad9972b3b8ba25be1e9eead6f`; installed `gpui-fast-0.1.9/src/elements/div.rs` = `fa34b334cb250630fde1f0ba8db3d7c63b87c8f188a0e5a656d2de79fc730fff`; `src/window.rs` = `651325f0c518596012292bbb5e635445a367705edfab27f001bb8d41eee2d893`; `src/tab_stop.rs` = `7f742d6d6b7e5cd6fc634f2764fc3937e1cdebcf086d4b7c166379724edd1881`. Read-only settings cleanup source = `7124adf338ab4104add23d9ed01455b683947d290561a2fb6d8c0bb9b5f5fad3`.

The document's final hash is reported outside this file. Stop writes after final read/hash verification; parent owns all compilation and runtime follow-up.
