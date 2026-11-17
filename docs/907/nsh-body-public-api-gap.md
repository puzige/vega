# Pinned NSH body-selection bridge investigation

## Scope and outcome

Read-only investigation of public NSH 0.4.0 / GPUI Kit 0.7.3 / Fast 0.1.9 APIs for #907, development HEAD `0b5ea426abfd8d4358489e916273d89474fb190f` plus the current integration batch. Dependencies remain unchanged by explicit user decision. No private patch, fork, new dependency, app execution or alternative renderer was implemented. This is a technical missing contract, not a request for renewed full-migration authorization.

The existing AppView does not expose the geometry actually used to paint its text, nor a revision-bound built-in document selection/copy override. A paint-free Native input proxy alone cannot reconstruct that geometry. This blocks a faithful direct migration of Thinking/main Markdown selection using this host's current public extension seams. It does not prove all possible renderer implementations impossible; building an entire alternate text/platform renderer would not be a small evidenced integration and is not silently adopted.

## Verified public/private boundary

All paths below are within the cached published crates, not repository patches.

- `nsh-core-0.4.0/src/text.rs:699–773`: public TextEngine supports layout/hit_test/caret_rect/range_rects. Default queries estimate characters evenly over line width and are not acceptable for proportional fonts, ligatures or bidi. TextLayout is opaque with a public downcast, not a public geometry schema.
- `nsh-platform-gpui-0.4.0/src/text.rs:350,363`: GpuiTextEngine and constructor are crate-private. Its precise queries at 1497–1534 consume GpuiLayout; that type and fields are private at 1959–1965. Drawing at 2300–2356 downcasts to the same private GpuiLayout and returns without drawing if the downcast fails. An external TextEngine cannot simply feed its own layout to that painter.
- `nsh-core-0.4.0/src/ops.rs:781–782` and `text.rs:834–860`: onTextLayout exposes TextLine text, UTF-16 range, x/y/width/height and font vertical metrics. It does not expose clusters, visual bidi runs, caret edges, affinity-aware queries or a mounted-layout query handle. Core reports lines and mounts the separate opaque layout at `core/mod.rs:6523–6550`.
- Ambient measure_text/text_metrics return aggregate measurements. `canvas.rs:582–592` has width/ascent/descent/height only. Measuring prefixes separately does not establish identity with contextual shaping and the displayed wrapped/bidi layout.
- `nsh-platform-gpui-0.4.0/src/native.rs:114–170,191–221`: public NativeComponent render/placed/props/commands/focus and ComponentContext emit/set_size/redraw expose that component's own container, not sibling/descendant NSH text layout or selection. Its host inserts BlockMouse at 832–842; it is not a passive pointer observation tap. Links, wheel handoff and selection would need explicit coordination.
- `nsh-engine-0.4.0/src/lib.rs:1174–1178,554–556`: Engine::start accepts a custom TextEngine and MountSink. Public MountItem::Text contains attributed text, opaque layout and content bounds (`nsh-core/src/mount.rs:725–735`). However AppView's surface is private (`platform-gpui/src/app.rs:256–269`); Surface::start hard-wires its private sink and GpuiTextEngine (`surface.rs:335–367`); Mounter is private (`mount.rs:42–64`). No public external engine-factory or mount-observer connection was found.
- `nsh-base-0.4.0/src/text_view/view.rs:128–143`: TextViewEvent contains LinkPress, ImagePress and CodeBlockAction, no selection-range event. Built-in code copy occurs before CodeBlockAction. `platform-gpui/src/selectable_view.rs:55–59,591–620` keeps document selection private and writes clipboard directly. Input on_selection_change/set_selection concern input text, not this structured selectable document.

## Required faithful bridge

Either expose a revision-bound built-in cross-text selection snapshot and a pre-copy override/cancellation hook, or expose enough same-rendered-layout geometry to an application selection owner:

1. Stable text-node/content identity, revision and mounted layout generation.
2. UTF-16+affinity hit testing, caret geometry and range rectangles tied to that exact painted layout.
3. Current origin/transforms/scroll/clipping conversion and synchronous removal/replacement invalidation.
4. Custom highlight presentation and control over built-in copy while preserving NSH text/link rendering.
5. Trusted mapping through retained rendered-to-source spans into MessageCopy, with current owner/source/window/selection checks.

Preserve read-only preview selection, permitted copy and links separately from mutation authorization. A selected string/native event is not itself clipboard or mutation authority. Do not use equal-width estimation, clone raw Markdown as an assumed range map, embed visible legacy GPUI text, or call layout acknowledgement native paint acceptance.

## Evidence provenance and remaining work

An independent scoped reader traced these APIs and returned the concrete paths above; parent additionally inspected public TextEngine/TextMetrics/TextViewEvent declarations and the private platform text types. No behavior tests or native acceptance were performed for this investigation. Existing GPUI body/scrollbars remain explicitly unmigrated. The IME/editor public-API limitations also remain unchanged. Independent timeline, menu and leaf work continues; this gap does not invalidate their distinct contracts or justify an unfinished merge.
