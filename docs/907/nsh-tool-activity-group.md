# ToolActivityGroup NSH migration

## Current timeline/scroll lane delta

Latest lane 1 replaces the expanded group's legacy GPUI scrollbar and old fade implementation with actual NSH thumb/edge paint driven by the same original ScrollHandle. The group remains a native layout/event host; child ToolCard entities/order/expansion/controllers are unchanged. Header publication now stabilizes when id/content/focus/used colors are equivalent. ToolCard's existing NSH detail scroll also uses the already-declared nsh-base built-in scrollbar with real target metrics/drag/AX and system Hover/Always preference. No dependencies or second vertical scroll state are added.

Current implementation, acceptance, source-level wheel blocker, validation commands and limits are in [tool/timeline scroll composition](nsh-tool-scroll-composition.md#current-full-lane-implementation-delta) and [timeline lane](nsh-run-timeline.md#full-timelinescroll-lane-acceptance-before-implementation). Prior header-only/incomplete-paint statements below describe earlier batches, not this delta. Cross-Surface wheel remainder remains blocked by the unauthorized backend event protocol; no native/full-composition pass is claimed. Scoped rustfmt/static only; all new/affected regressions await parent unified execution and post-four-lane independent review.

## Acceptance and risk recorded before implementation

Source baseline: `bd89db7d182e6d9a5c60b1388e79a4623e8cacc4`; original `tool_activity_group.rs` SHA256 `6278fac38d154bc19296bb56ec3088c97012968c77573af2a0ad97da03cd9cb7`. Parent has fetched/updated and integrated origin/master `34555cec9c0c4c4c1f7a528d739752f414e53edf`. No Git mutation is delegated. Writes are confined to `crates/vega_ui/src/tool_activity_group.rs`, its new `tool_activity_group/` subtree, and this document. No nested AGENTS.md exists under crates or docs at inspection time.

Full target: macOS/Windows/Linux production group summary, leading icon, disclosure, hover/keyboard focus, scrollbars and scroll edges rendered by NSH; Fast provides only native hosting/services. Preserve existing ToolCard entities and their production Surface rendering, business aggregation, latest-tool precedence, child order, public methods/events, maximum height/estimated-height/row-count semantics, scroll containment/edge handoff, and expansion persistence. Hiding/unmounting presentation never cancels tools.

Risk-based checks must cover stale mount/recycle, snapshot/child-list/expansion changes, ABA transitions, keyboard/AX activation, native focus/Tab entry and exit, overflow/scroll handoff, dynamic child sizing and theme/locale updates. No builds/tests/application execution/network/dependency/configuration changes are delegated; only scoped rustfmt/static inspection. Parent must coordinate compilation and execution; prior parent test counts do not validate this delta. Development Rust test sources, if authored, are UNRUN here and must be archived/removed by parent before final merge.

## Concrete full-composition blocker and bounded implementation

Pinned dependencies remain NSH `=0.4.0`, Kit `=0.7.3`, Fast `=0.1.9`; no upgrade, patch or fork. Palette/IME is excluded and has not been investigated.

Actual pinned APIs inspected: shared `Surface<S,C>`/`Renderer`/`ProjectionId.matches`/`OrderedCommands`, `nsh::keyboard`, NSH element-ref measurement/reveal, `AppView::register_component`, `NativeComponent`, and production `ToolCard::render_with_title`.

- `nsh-platform-gpui-0.4.0/src/native.rs`, `ComponentItem::prepaint`: every visible native child gets a `HitboxBehavior::BlockMouse` hitbox; pinned documentation explicitly states that the component receives pointer/wheel and NSH input does not. Thus placing the existing ToolCard Surface inside an NSH scroll view cannot preserve group wheel scrolling and boundary handoff using this API alone. `NativeComponent` exposes render/placed/focus_handle/set_prop/command, but no parent NSH scroll-dispatch hook.
- Native child boxes are laid out at exactly the NSH container size. Production ToolCard dynamically measures its NSH content into private `Presentation.height`; its existing public `render` returns an opaque GPUI element. `estimated_height()` is only an estimate and cannot substitute for measured height. Native `ComponentContext::set_size` exists, but the required actual child measurement is not available to this owned leaf.
- Ordinary NSH View focusability does not provide a native Tab stop. Existing explicit paint-free real FocusHandle/AX bridging is required for a usable header.

The implementable authorized fallback is **header-only migration, explicitly incomplete**: replace every old painted group-header control with production NSH, retain the original expanded-child scroller/scrollbar/edge implementation and original ToolCard::render calls verbatim, and do not call the entire group migrated. This is not a demo wrapper and has no visible legacy header fallback.

Needed parent integration hooks before full group completion: (1) a supported NSH/native-child wheel handoff path preserving child consumption and parent edge propagation without event replay; (2) a ToolCard measured-size/size-change contract usable by the parent NSH layout, keeping existing ToolCard controllers/Surface identity; (3) child native keyboard/Tab focus plus minimal reveal integration with the group scroll viewport. These are not implemented by guessing private APIs or changing dependencies in this leaf.

## Header acceptance

- NSH owns summary text, SVG leading icon, inline chevron, hover/pressed/keyboard focus pixels and accessible button state. Reuse existing theme/layout/typography/i18n and latest-tool aggregation.
- Paint-free host has a real Tab-eligible FocusHandle, explicit Enter/Space filtering through the shared keyboard parser, first/last exit, and AX activation. No native painted GPUI button/text/icon/focus fallback.
- All activation routes validate live mount generation, projection identity, original ordered child identities and expansion state, plus current summary/icon/tint; synchronous controller state remains authoritative. No asynchronous activation replay/retry. An unmounted/recycled or superseded header cannot toggle the controller.
- Header Surface is retained across ordinary projection updates; only native presentation is mounted/unmounted. No tool business cancellation or duplicated ToolCard controller.

## Verification status

Header implementation is present; **READY for parent integration/review, full group explicitly incomplete**. Production header uses a retained shared Surface, `OrderedCommands`, identity-matched projections and `SignalEquals::Never` for non-value signals. Ordered pointer/AX requests return to the original controller; keyboard/host AX activation is synchronous. A press retains its down-time projection rather than retargeting on watch updates. Controller child/expansion/tool-event mutations invalidate projection identity, including change-and-restore sequences. A frame-retained weak mount lease rejects unmounted headers; remount rotates identity. No new user-facing explanatory copy, tool controller, service ownership or cancellation is introduced.

Original-contract static comparison against the exact baseline confirmed identical public function inventory, description/contains/children/expanded accessors, len/row_count/estimated_height/aggregation/icon calculations, and the entire expanded child scroller subtree. Append/replace/extend/tool_event/toggle/set retain their original business decisions and notifications, adding only header invalidation. Header row height and layout token usage remain unchanged. No shared files, dependency files, Git/index, application, installation or configuration were changed by this leaf.

Actual static commands (project cwd):

```sh
rustfmt --version
rustfmt --edition 2024 --config skip_children=true crates/vega_ui/src/tool_activity_group.rs crates/vega_ui/src/tool_activity_group/nsh_header.rs crates/vega_ui/src/tool_activity_group/nsh_header/tests.rs
rustfmt --edition 2024 --config skip_children=true --check crates/vega_ui/src/tool_activity_group.rs crates/vega_ui/src/tool_activity_group/nsh_header.rs crates/vega_ui/src/tool_activity_group/nsh_header/tests.rs
git diff --check -- crates/vega_ui/src/tool_activity_group.rs crates/vega_ui/src/tool_activity_group docs/907/nsh-tool-activity-group.md
```

Observed rustfmt `1.9.0-stable (88d9e12ae1 2026-08-18)`; formatting and scoped whitespace checks passed. Read-only `git diff`, `git show` plus an inline Python baseline comparator confirmed the unchanged regions listed above and scanned the two production files for comments, unwrap/expect and try_send (none). This is source/static evidence only, not Rust type checking or behavior execution.

Four development `#[test]` sources are authored **UNRUN** in `tool_activity_group/nsh_header/tests.rs`: reordered/replaced/removed child rejection; expansion/headline/icon/tint rejection; recycled identity/restored-content rejection; production NSH Root narrow/wide header geometry, button label/expanded state and stable button identity after expansion update. Imports are explicit rather than importing the GPUI test macro. The renderer fixture uses pinned public `Root::of`, `Core::new(ApproxTextEngine, size(px(...), px(...)))`, `commit`, and `MountItem` APIs. Parent must archive original sources/hashes and remove Rust tests plus test-only wiring before final merge under repository policy.

Not run: cargo check/build/Clippy/test/nextest, native application/UI/AX, final CI. Parent's prior 124-test result predates this code and is not evidence for it. Parent should first compile this exact delta, then run the four development tests and relevant existing regressions; do not treat their source count as passed or executed. Independent non-implementer review is outstanding.

Remaining native acceptance: both Tab directions across neighboring groups/child AppViews, Enter/Space modifier/repeat/character-input guards, focused AX proxy versus NSH AX activation, pointer press across streaming projection changes, narrow text truncation/chevron, SVG assets, theme/locale, hide/remount while queued, and existing scroll-edge handoff on all three desktops. Chevron direction/visibility is NSH-driven; the old GPUI animated-chevron element is removed, and animation parity has not been verified. Full NSH group scrollbar/edge/child composition remains blocked by the hooks above; retaining that original subtree is an explicit incomplete scope, not an NSH fallback completion claim.
