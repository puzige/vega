use super::*;
use crate::nsh::{OrderedCommands, ProjectionId, Surface};
use ns::{
    Accessible as _, Element as _, Interactive as _, IntoElement as _, ParentElement as _,
    Scrollable as _, Styled as _,
};
use nsh_core::prelude as ns;
use tokio::sync::mpsc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Action {
    Open,
    Retry,
    Address,
}

impl Action {
    fn index(self) -> usize {
        match self {
            Self::Open => 0,
            Self::Retry => 1,
            Self::Address => 2,
        }
    }
}

#[derive(Clone, PartialEq)]
struct Content {
    key: String,
    target: String,
    generation: u64,
    data: Option<Result<LinkPreviewData, LinkPreviewFailure>>,
    expanded: bool,
    width: f32,
    height: f32,
    viewport: [f32; 2],
    scale: f32,
    geometry: Vec<[f32; 4]>,
    selected: [bool; 3],
    palette: [Rgba; 7],
    language: &'static str,
}

#[derive(Clone)]
struct Snapshot {
    mount: ProjectionId,
    revision: u64,
    content: Content,
    host_bounds: Option<[f32; 4]>,
}

impl Snapshot {
    fn accepts(&self, observed: &Self, action: Action) -> bool {
        self.mount.matches(&observed.mount)
            && self.revision == observed.revision
            && self.content == observed.content
            && self.host_bounds == observed.host_bounds
            && match action {
                Action::Open => true,
                Action::Retry => retryable(&self.content.data),
                Action::Address => self.content.target.len() > 120,
            }
    }
}

pub(super) fn retryable(data: &Option<Result<LinkPreviewData, LinkPreviewFailure>>) -> bool {
    matches!(
        data,
        Some(Err(LinkPreviewFailure::Network
            | LinkPreviewFailure::Timeout
            | LinkPreviewFailure::Malformed
            | LinkPreviewFailure::TooLarge
            | LinkPreviewFailure::Busy))
    )
}

enum Request {
    Action(Snapshot, u64, Action),
    Layout(Snapshot, u64, f32, Vec<(Action, [f32; 4])>),
    Scroll(Snapshot, u64),
}

pub(super) struct Presentation {
    surface: Surface<Snapshot, Request>,
    snapshot: Snapshot,
    measured: Option<f32>,
    controls: Vec<(Action, Bounds<Pixels>)>,
    bounds: Option<Bounds<Pixels>>,
    scroll: u64,
}

impl Presentation {
    pub(super) fn stop(self, cx: &App) {
        self.surface.entity().read(cx).stop();
    }

    pub(super) fn focused(&self, window: &Window, cx: &App) -> bool {
        self.surface.focus_handle(cx).contains_focused(window, cx)
    }
}

fn rect(bounds: Bounds<Pixels>) -> [f32; 4] {
    [
        bounds.origin.x.into(),
        bounds.origin.y.into(),
        bounds.size.width.into(),
        bounds.size.height.into(),
    ]
}

fn content(state: &Coordinator, window: &Window, cx: &App) -> Option<Content> {
    if !state.active_current(window, cx) {
        return None;
    }
    let active = state.active.as_ref()?;
    let link = state.visible.iter().find(|link| link.key == active.key)?;
    let colors = theme(cx).colors;
    Some(Content {
        key: active.key.clone(),
        target: active.target.clone(),
        generation: active.generation,
        data: active.data.clone(),
        expanded: active.expanded,
        width: Layout::LINK_PREVIEW_WIDTH.min(
            (f32::from(window.viewport_size().width) - 2. * Layout::LINK_PREVIEW_MARGIN).max(0.),
        ),
        height: Layout::LINK_PREVIEW_MAX_HEIGHT.min(
            (f32::from(window.viewport_size().height) - 2. * Layout::LINK_PREVIEW_MARGIN).max(0.),
        ),
        viewport: [
            window.viewport_size().width.into(),
            window.viewport_size().height.into(),
        ],
        scale: window.scale_factor(),
        geometry: link
            .text_bounds
            .iter()
            .chain(&link.icon_bounds)
            .copied()
            .map(rect)
            .collect(),
        selected: [
            active.open.is_focused(window),
            active.retry.is_focused(window),
            active.address.is_focused(window),
        ],
        palette: [
            colors.bg_elevated,
            colors.border_subtle,
            colors.text_primary,
            colors.text_secondary,
            colors.text_tertiary,
            colors.accent,
            colors.bg_hover,
        ],
        language: vega_i18n::language(),
    })
}

fn current(
    state: &Coordinator,
    observed: &Snapshot,
    action: Action,
    window: &Window,
    cx: &App,
) -> bool {
    window.is_window_active()
        && window.is_visible()
        && state.presentation.as_ref().is_some_and(|page| {
            page.surface.errors().borrow().is_none()
                && page.snapshot.accepts(observed, action)
                && page.bounds == state.card_bounds
                && page.bounds.map(rect) == observed.host_bounds
                && page.bounds.is_some()
                && page.controls.iter().any(|(control, _)| *control == action)
                && content(state, window, cx).as_ref() == Some(&observed.content)
        })
}

fn receive(entity: &Entity<Coordinator>, request: Request, window: &mut Window, cx: &mut App) {
    match request {
        Request::Action(observed, scroll, action) => {
            let accepted = entity.update(cx, |state, cx| {
                if !current(state, &observed, action, window, cx)
                    || state
                        .presentation
                        .as_ref()
                        .is_none_or(|page| page.scroll != scroll)
                {
                    return false;
                }
                let Some(active) = state.active.as_mut() else {
                    return false;
                };
                let focus = match action {
                    Action::Open => &active.open,
                    Action::Retry => &active.retry,
                    Action::Address => &active.address,
                };
                window.focus(focus, cx);
                match action {
                    Action::Open => open_target(&active.target, cx),
                    Action::Retry => {}
                    Action::Address => active.expanded = !active.expanded,
                }
                window.refresh();
                true
            });
            if accepted && action == Action::Retry {
                load(entity, true, window, cx);
            }
        }
        Request::Scroll(observed, scroll) => {
            entity.update(cx, |state, _| {
                if let Some(page) = &mut state.presentation
                    && page.snapshot.mount.matches(&observed.mount)
                    && page.snapshot.content.generation == observed.content.generation
                {
                    page.scroll = scroll;
                    page.controls.clear();
                    window.refresh();
                }
            });
        }
        Request::Layout(observed, scroll, height, controls) => {
            entity.update(cx, |state, cx| {
                if content(state, window, cx).as_ref() != Some(&observed.content) {
                    return;
                }
                let Some(page) = &mut state.presentation else {
                    return;
                };
                if !page.snapshot.mount.matches(&observed.mount)
                    || page.snapshot.revision != observed.revision
                    || page.snapshot.host_bounds != observed.host_bounds
                    || page.scroll != scroll
                    || !height.is_finite()
                    || height <= 0.
                    || height > observed.content.height + 1.
                {
                    return;
                }
                let controls = controls
                    .into_iter()
                    .filter_map(|(action, bounds)| {
                        measured_control(bounds, observed.content.width, height)
                            .map(|bounds| (action, bounds))
                    })
                    .collect::<Vec<_>>();
                if page.measured != Some(height) || page.controls != controls {
                    page.measured = Some(height);
                    page.controls = controls;
                    window.refresh();
                }
            });
        }
    }
}

pub(super) fn measured_control(
    bounds: [f32; 4],
    width: f32,
    height: f32,
) -> Option<Bounds<Pixels>> {
    if bounds.iter().all(|value| value.is_finite())
        && bounds[0] >= 0.
        && bounds[1] >= 0.
        && bounds[2] > 0.
        && bounds[3] > 0.
        && bounds[0] + bounds[2] <= width + 1.
        && bounds[1] + bounds[3] <= height + 1.
    {
        Some(Bounds::new(
            point(px(bounds[0]), px(bounds[1])),
            size(px(bounds[2]), px(bounds[3])),
        ))
    } else {
        None
    }
}

pub(super) fn card(
    entity: &Entity<Coordinator>,
    window: &mut Window,
    cx: &mut App,
) -> Option<(Bounds<Pixels>, AnyElement)> {
    let next = content(entity.read(cx), window, cx)?;
    let state = entity.read(cx);
    let active = state.active.as_ref()?;
    let link = state.visible.iter().find(|link| link.key == active.key)?;
    let anchor = link
        .text_bounds
        .iter()
        .find(|bounds| bounds.contains(&window.mouse_position()))
        .or_else(|| link.text_bounds.first())
        .copied()?;
    entity.update(cx, |state, cx| {
        if state.presentation.is_none() {
            let snapshot = Snapshot {
                mount: ProjectionId::new(),
                revision: 0,
                content: next.clone(),
                host_bounds: None,
            };
            let (surface, mut requests) =
                Surface::new_transparent(snapshot.clone(), pixels, window, cx);
            surface.focus_handle(cx).tab_stop(false);
            let weak = cx.entity().downgrade();
            let handle = window.window_handle();
            cx.spawn(async move |_, cx| {
                while let Some(request) = requests.recv().await {
                    if cx
                        .update_window(handle, |_, window, cx| {
                            if let Some(entity) = weak.upgrade() {
                                receive(&entity, request, window, cx);
                            }
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .detach();
            state.presentation = Some(Presentation {
                surface,
                snapshot,
                measured: None,
                controls: Vec::new(),
                bounds: None,
                scroll: 0,
            });
        }
        let page = state.presentation.as_mut()?;
        page.surface.sync_environment(cx);
        if page.snapshot.content != next {
            let size_changed = page.snapshot.content.width != next.width
                || page.snapshot.content.data != next.data
                || page.snapshot.content.expanded != next.expanded
                || page.snapshot.content.height != next.height
                || page.snapshot.content.language != next.language;
            page.snapshot.revision = page.snapshot.revision.wrapping_add(1);
            page.snapshot.content = next;
            if size_changed {
                page.measured = None;
                page.controls.clear();
            }
            page.surface.publish(page.snapshot.clone());
        }
        let mut host = div()
            .w(px(page.snapshot.content.width))
            .h(px(page.measured.unwrap_or(page.snapshot.content.height)))
            .occlude();
        if let Some(active) = &state.active {
            for (action, bounds) in &page.controls {
                let focus = match action {
                    Action::Open => &active.open,
                    Action::Retry => &active.retry,
                    Action::Address => &active.address,
                };
                host = host.child(
                    div()
                        .id(ElementId::Name(
                            format!("nsh-preview-focus-{}", action.index()).into(),
                        ))
                        .absolute()
                        .left(bounds.origin.x)
                        .top(bounds.origin.y)
                        .w(bounds.size.width)
                        .h(bounds.size.height)
                        .track_focus(focus)
                        .tab_index(0),
                );
            }
        }
        Some((
            anchor,
            host.child(page.surface.entity())
                .relative()
                .into_any_element(),
        ))
    })
}

pub(super) fn positioned(
    state: &mut Coordinator,
    bounds: Option<Bounds<Pixels>>,
    window: &mut Window,
) {
    state.card_bounds = bounds;
    if let Some(page) = &mut state.presentation
        && page.bounds != bounds
    {
        page.bounds = bounds;
        page.snapshot.host_bounds = bounds.map(rect);
        page.snapshot.revision = page.snapshot.revision.wrapping_add(1);
        page.surface.publish(page.snapshot.clone());
        window.refresh();
    }
}

fn encoded_image(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut source = String::with_capacity(bytes.len() * 3 + 16);
    source.push_str("data:image/png,");
    for byte in bytes {
        source.push('%');
        source.push(HEX[usize::from(byte >> 4)] as char);
        source.push(HEX[usize::from(byte & 15)] as char);
    }
    source
}

pub(super) fn icon_source(site: LinkSiteKind) -> String {
    use gpui_kit::AssetSource;
    let name = match site {
        LinkSiteKind::Github => gpui_kit::assets::IconName::Github,
        LinkSiteKind::Mail => gpui_kit::assets::IconName::Mail,
        LinkSiteKind::Web => {
            return crate::icons::nsh_source(crate::icons::Icon::Globe).unwrap_or_default();
        }
    };
    gpui_kit::assets::Assets::new("")
        .load(&name.path())
        .ok()
        .flatten()
        .and_then(|data| String::from_utf8(data.into_owned()).ok())
        .unwrap_or_default()
}

fn control(
    incoming: ns::Signal<Snapshot>,
    scrolling: ns::Signal<u64>,
    snapshot: Snapshot,
    action: Action,
    label: &'static str,
    target: ns::ElementRef,
    commands: OrderedCommands<Request>,
) -> ns::AnyElement {
    let colors = snapshot.content.palette;
    let observed = snapshot.clone();
    let alive = Rc::new(std::cell::Cell::new(true));
    let lifetime = alive.clone();
    ns::cleanup(move || lifetime.set(false));
    let owner = nsh_core::element::Owner::current();
    let focused = ns::signal(false);
    target.on_focus_in(move || focused.set(true));
    target.on_focus_out(move || focused.set(false));
    ns::view()
        .element_ref(&target)
        .role(nsh_core::AccessibilityRole::Button)
        .focusable(true)
        .label(label)
        .h(ns::px(Layout::LINK_PREVIEW_ACTION_HEIGHT))
        .flex_shrink_0()
        .px_2()
        .justify_center()
        .select_none()
        .text_color(crate::nsh::color(colors[5]))
        .border(ns::px(1.))
        .border_color(move || {
            crate::nsh::color(
                if focused.get() || incoming.get().content.selected[action.index()] {
                    colors[5]
                } else {
                    colors[0].opacity(0.)
                },
            )
        })
        .hover(move |style| style.bg(crate::nsh::color(colors[6])))
        .on_press(move || {
            let latest = incoming.get_untracked();
            let mut shown = observed.content.clone();
            let mut current = latest.content.clone();
            shown.selected = [false; 3];
            current.selected = [false; 3];
            shown.geometry.clear();
            current.geometry.clear();
            if alive.get() && observed.mount.matches(&latest.mount) && shown == current {
                owner.run(|| {
                    commands.send(Request::Action(latest, scrolling.get_untracked(), action))
                });
            }
        })
        .child(ns::text(label))
        .into_element()
}

fn pixels(incoming: ns::Signal<Snapshot>, sender: mpsc::Sender<Request>) -> ns::AnyElement {
    let commands = OrderedCommands::new(sender);
    let owner = nsh_core::element::Owner::current();
    let displayed = ns::computed(move || {
        let mut content = incoming.get().content;
        content.selected = [false; 3];
        content.geometry.clear();
        content
    });
    let scrolling = ns::signal(0u64);
    ns::dynamic(move || {
        let _ = displayed.get();
        let snapshot = incoming.get_untracked();
        let colors = snapshot.content.palette;
        let data = snapshot
            .content
            .data
            .as_ref()
            .and_then(|data| data.as_ref().ok());
        let site = data.map(|data| data.site).unwrap_or_else(|| {
            classify_target(&snapshot.content.target)
                .map_or(LinkSiteKind::Web, |target| target.site)
        });
        let host = data
            .map(|data| data.host.clone())
            .unwrap_or_else(|| display_host(&snapshot.content.target));
        let root = ns::element_ref();
        let open = ns::element_ref();
        let retry = ns::element_ref();
        let address = ns::element_ref();
        let viewport = ns::element_ref();
        let reveal_revision = ns::signal([false; 3]);
        let scroll_offset = ns::signal(0.0f32);
        ns::effect(move || {
            let selected = incoming.get().content.selected;
            if selected == reveal_revision.get_untracked() {
                return;
            }
            reveal_revision.set(selected);
            let target = if selected[1] {
                Some(retry)
            } else if selected[2] {
                Some(address)
            } else {
                None
            };
            if let Some(target) = target {
                ns::spawn(async move {
                    let Some(control) = target.measure().await else {
                        return;
                    };
                    let Some(scroll) = viewport.measure().await else {
                        return;
                    };
                    if incoming.get_untracked().content.selected != selected {
                        return;
                    }
                    let top = control.page.y.0 - scroll.page.y.0;
                    let bottom = top + control.frame.size.height.0;
                    let offset = scroll_offset.get_untracked();
                    let next = if top < 0. {
                        offset + top
                    } else if bottom > scroll.frame.size.height.0 {
                        offset + bottom - scroll.frame.size.height.0
                    } else {
                        offset
                    };
                    if (next - offset).abs() > 0.5 {
                        viewport.scroll_to(ns::point(ns::px(0.), ns::px(next.max(0.))), false);
                    }
                });
            }
        });
        let measurement_revision = ns::signal(0u64);
        let measure_commands = commands.clone();
        let measure = move || {
            let observed = incoming.get_untracked();
            let stamp = scrolling.get_untracked();
            let measurement = measurement_revision.get_untracked().wrapping_add(1);
            measurement_revision.set(measurement);
            let commands = measure_commands.clone();
            owner.run(|| {
                ns::spawn(async move {
                    let Some(card) = root.measure().await else {
                        return;
                    };
                    let Some(scroll_bounds) = viewport.measure().await else {
                        return;
                    };
                    let mut controls = Vec::new();
                    for (action, target) in [
                        (Action::Open, open),
                        (Action::Retry, retry),
                        (Action::Address, address),
                    ] {
                        if let Some(bounds) = target.measure().await {
                            let mut left = bounds.page.x.0;
                            let mut top = bounds.page.y.0;
                            let mut right = left + bounds.frame.size.width.0;
                            let mut bottom = top + bounds.frame.size.height.0;
                            if action != Action::Open {
                                left = left.max(scroll_bounds.page.x.0);
                                top = top.max(scroll_bounds.page.y.0);
                                right = right
                                    .min(scroll_bounds.page.x.0 + scroll_bounds.frame.size.width.0);
                                bottom = bottom.min(
                                    scroll_bounds.page.y.0 + scroll_bounds.frame.size.height.0,
                                );
                            }
                            controls.push((
                                action,
                                [
                                    left - card.page.x.0,
                                    top - card.page.y.0,
                                    right - left,
                                    bottom - top,
                                ],
                            ));
                        }
                    }
                    if measurement_revision.get_untracked() == measurement {
                        commands.send(Request::Layout(
                            observed,
                            stamp,
                            card.frame.size.height.0,
                            controls,
                        ));
                    }
                });
            });
        };
        let projection_measure = measure.clone();
        ns::effect(move || {
            let _ = incoming.get().revision;
            projection_measure();
        });
        let layout_measure = measure.clone();
        let scroll_measure = measure.clone();
        let scroll_commands = commands.clone();
        let mut body = ns::view().w_full().flex_col().gap_2().selectable(true);
        if let Some(image) = data.and_then(|data| data.image.as_ref()) {
            let width = (snapshot.content.width - 2. * Layout::LINK_PREVIEW_PADDING)
                .max(0.)
                .min(image.width as f32);
            let height = width * image.height as f32 / image.width.max(1) as f32;
            body = body.child(
                ns::image(encoded_image(&image.png))
                    .w(ns::px(width))
                    .h(ns::px(height))
                    .flex_shrink_0(),
            );
        }
        let line_height =
            (Typography::SIDEBAR * Typography::BODY_LINE_HEIGHT * snapshot.content.scale).round()
                / snapshot.content.scale;
        match &snapshot.content.data {
            None => body = body.child(ns::text(vega_i18n::t("ui.loading_en"))),
            Some(Ok(data)) if data.site != LinkSiteKind::Mail => {
                body = body.child(
                    ns::text(
                        data.title
                            .clone()
                            .unwrap_or_else(|| snapshot.content.target.clone()),
                    )
                    .font_weight(ns::FontWeight::MEDIUM)
                    .number_of_lines(2u32)
                    .max_h(ns::px(line_height * 2.))
                    .overflow_hidden()
                    .flex_shrink_0(),
                );
                if let Some(description) = &data.description {
                    body = body.child(
                        ns::text(description.clone())
                            .text_color(crate::nsh::color(colors[3]))
                            .number_of_lines(3u32)
                            .max_h(ns::px(line_height * 3.))
                            .overflow_hidden()
                            .flex_shrink_0(),
                    );
                }
                if data.effective_target
                    != snapshot
                        .content
                        .target
                        .split('#')
                        .next()
                        .unwrap_or(&snapshot.content.target)
                {
                    body = body.child(
                        ns::text(data.effective_target.clone())
                            .text_color(crate::nsh::color(colors[3])),
                    );
                }
            }
            Some(Ok(_)) => {}
            Some(Err(_)) => {
                let can_retry = retryable(&snapshot.content.data);
                body = body.child(
                    ns::text(vega_i18n::t(if can_retry {
                        "ui.cannot_load_preview"
                    } else {
                        "ui.web_preview_is_currently_unavailable"
                    }))
                    .text_color(crate::nsh::color(colors[3])),
                );
                if can_retry {
                    body = body.child(control(
                        incoming,
                        scrolling,
                        snapshot.clone(),
                        Action::Retry,
                        vega_i18n::t("ui.retry"),
                        retry,
                        commands.clone(),
                    ));
                }
            }
        }
        if snapshot.content.target.len() > 120 {
            body = body.child(control(
                incoming,
                scrolling,
                snapshot.clone(),
                Action::Address,
                vega_i18n::t(if snapshot.content.expanded {
                    "ui.hide_address"
                } else {
                    "ui.show_address"
                }),
                address,
                commands.clone(),
            ));
        }
        if snapshot.content.target.len() <= 120 || snapshot.content.expanded {
            body = body.child(
                ns::text(snapshot.content.target.clone())
                    .text_size(ns::px(Typography::METADATA))
                    .text_color(crate::nsh::color(colors[4])),
            );
        }
        let max_body = (snapshot.content.height
            - (2. * Layout::LINK_PREVIEW_PADDING + Layout::LINK_PREVIEW_ACTION_HEIGHT + 8. + 2.))
            .max(0.);
        ns::view()
            .element_ref(&root)
            .w_full()
            .max_h(ns::px(snapshot.content.height))
            .bg(crate::nsh::color(colors[0]))
            .border(ns::px(1.))
            .border_color(crate::nsh::color(colors[1]))
            .rounded(ns::px(Layout::LINK_PREVIEW_RADIUS))
            .p(ns::px(Layout::LINK_PREVIEW_PADDING))
            .text_size(ns::px(Typography::SIDEBAR))
            .line_height(nsh_core::LineHeight::Multiple(Typography::BODY_LINE_HEIGHT))
            .text_color(crate::nsh::color(colors[2]))
            .flex_col()
            .gap_2()
            .on_layout(move |_: &ns::Bounds<ns::Points>| layout_measure())
            .child(
                ns::view()
                    .h(ns::px(Layout::LINK_PREVIEW_ACTION_HEIGHT))
                    .flex_shrink_0()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(
                        ns::svg(nsh_core::element::ImageSrc::svg_markup(&icon_source(site)))
                            .w(ns::px(Layout::ICON_SIZE))
                            .h(ns::px(Layout::ICON_SIZE))
                            .text_color(crate::nsh::color(colors[3]))
                            .accessibility_elements_hidden(true),
                    )
                    .child(ns::text(host).min_w_0().flex_1().truncate())
                    .child(control(
                        incoming,
                        scrolling,
                        snapshot,
                        Action::Open,
                        vega_i18n::t("ui.open"),
                        open,
                        commands.clone(),
                    )),
            )
            .child(
                ns::scroll()
                    .element_ref(&viewport)
                    .w_full()
                    .min_h_0()
                    .max_h(ns::px(max_body))
                    .scrollbar(true)
                    .on_scroll(move |event: &nsh_core::element::ScrollEvent| {
                        scroll_offset.set(event.offset.y.0);
                        scrolling.set(scrolling.get_untracked().wrapping_add(1));
                        scroll_commands.send(Request::Scroll(
                            incoming.get_untracked(),
                            scrolling.get_untracked(),
                        ));
                        scroll_measure();
                    })
                    .child(body),
            )
            .into_element()
    })
    .into_element()
}
