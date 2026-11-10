use std::{collections::HashMap, rc::Rc, time::Duration};

use gpui_kit::{
    base::{Align, Placement, Positioner},
    prelude::*,
    *,
};
use tokio_util::sync::CancellationToken;
use vega_conversation::{
    link_preview::{LinkPreviewService, classify_target, display_host},
    types::{LinkPreviewData, LinkPreviewFailure, LinkSiteKind},
};
use vega_theme::{Layout, Typography, theme};

use crate::conversation_stream::links::{LinkRange, link_regions};

#[cfg(feature = "nsh-reading-integration")]
pub(crate) mod integration_support;

mod decoration;
#[cfg(feature = "nsh-reading-integration")]
mod focus_integration;
mod native;
#[cfg(feature = "nsh-reading-integration")]
pub use focus_integration::measured_focus_paints_wrapped_fragments_and_tracks_tab_reverse_and_escape;

#[derive(Clone, Default)]
struct Dependencies {
    service: LinkPreviewService,
}

#[derive(Default)]
struct Registry {
    windows: HashMap<WindowId, WeakEntity<Coordinator>>,
    stack: Vec<ReadingScope>,
    dependencies: Dependencies,
    #[cfg(feature = "nsh-reading-integration")]
    request: Option<integration_support::Request>,
}

impl Global for Registry {}

fn registry(cx: &mut App) -> &mut Registry {
    if !cx.has_global::<Registry>() {
        cx.set_global(Registry::default());
    }
    cx.global_mut::<Registry>()
}

fn coordinator(window: &Window, cx: &App) -> Option<Entity<Coordinator>> {
    cx.try_global::<Registry>()?
        .windows
        .get(&window.window_handle().window_id())?
        .upgrade()
}

#[derive(Clone)]
enum ScrollSource {
    None,
    Handle(ScrollHandle),
    List(ListState),
}

#[derive(Clone, PartialEq)]
enum ScrollStamp {
    None,
    Handle(Point<Pixels>),
    List(usize, Pixels),
}

impl ScrollSource {
    fn stamp(&self) -> ScrollStamp {
        match self {
            Self::None => ScrollStamp::None,
            Self::Handle(handle) => ScrollStamp::Handle(handle.offset()),
            Self::List(list) => {
                let top = list.logical_scroll_top();
                ScrollStamp::List(top.item_ix, top.offset_in_item)
            }
        }
    }
}

#[derive(Clone)]
struct ReadingScope {
    id: String,
    focus: FocusHandle,
    scroll: ScrollSource,
}

pub(crate) type LinkValidity = Rc<dyn Fn(&Window, &App) -> bool>;

#[derive(Clone)]
struct VisibleLink {
    key: String,
    target: String,
    label: String,
    owner: String,
    occurrence: usize,
    role: Role,
    description: String,
    text_bounds: Vec<Bounds<Pixels>>,
    icon_bounds: Vec<Bounds<Pixels>>,
    focus: FocusHandle,
    scopes: Vec<ReadingScope>,
    scroll: Vec<ScrollStamp>,
    validity: Option<LinkValidity>,
}

impl VisibleLink {
    fn current(&self, window: &Window, cx: &App) -> bool {
        self.validity.as_ref().is_none_or(|valid| valid(window, cx))
    }
}

struct ActivePreview {
    key: String,
    target: String,
    generation: u64,
    cancellation: CancellationToken,
    data: Option<Result<LinkPreviewData, LinkPreviewFailure>>,
    open: FocusHandle,
    retry: FocusHandle,
    address: FocusHandle,
    expanded: bool,
    validity: Option<LinkValidity>,
}

impl Drop for ActivePreview {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}

pub struct Coordinator {
    dependencies: Dependencies,
    visible: Vec<VisibleLink>,
    staged: Vec<VisibleLink>,
    scopes: Vec<ReadingScope>,
    focused: Option<String>,
    hovered: Option<String>,
    suppressed: Option<String>,
    armed: Option<(VisibleLink, CancellationToken)>,
    leaving: Option<CancellationToken>,
    active: Option<ActivePreview>,
    card_bounds: Option<Bounds<Pixels>>,
    presentation: Option<native::Presentation>,
    pointer_down: bool,
    generation: u64,
    _activation: Subscription,
    _keys: Subscription,
}

impl Coordinator {
    fn dismiss(&mut self, keyboard: bool, window: &mut Window, cx: &mut App) {
        let dismissed = self
            .active
            .as_ref()
            .map(|active| active.key.clone())
            .or_else(|| self.armed.as_ref().map(|(link, _)| link.key.clone()));
        if let Some(dismissed) = dismissed {
            self.suppressed = Some(dismissed);
        }
        if let Some((_, token)) = self.armed.take() {
            token.cancel();
        }
        if let Some(token) = self.leaving.take() {
            token.cancel();
        }
        let card_focused = self.card_focused(window, cx);
        if let Some(active) = self.active.take()
            && keyboard
            && card_focused
            && let Some(link) = self
                .visible
                .iter()
                .find(|link| link.key == active.key && link.current(window, cx))
        {
            window.focus(&link.focus, cx);
            self.focused = Some(link.key.clone());
        }
        if let Some(page) = self.presentation.take() {
            page.stop(cx);
        }
        self.card_bounds = None;
        self.generation = self.generation.wrapping_add(1);
        window.refresh();
    }

    fn pointer_link(&self, position: Point<Pixels>, window: &Window, cx: &App) -> Option<String> {
        self.visible
            .iter()
            .find(|link| {
                link.current(window, cx)
                    && link
                        .text_bounds
                        .iter()
                        .chain(&link.icon_bounds)
                        .any(|bounds| bounds.contains(&position))
            })
            .map(|link| link.key.clone())
    }

    fn active_current(&self, window: &Window, cx: &App) -> bool {
        self.active.as_ref().is_some_and(|active| {
            active
                .validity
                .as_ref()
                .is_none_or(|valid| valid(window, cx))
                && self.visible.iter().any(|link| {
                    link.key == active.key
                        && link.target == active.target
                        && link.current(window, cx)
                        && link
                            .scopes
                            .iter()
                            .map(|scope| scope.scroll.stamp())
                            .collect::<Vec<_>>()
                            == link.scroll
                })
        })
    }

    fn card_focused(&self, window: &Window, cx: &App) -> bool {
        if self
            .presentation
            .as_ref()
            .is_some_and(|page| page.focused(window, cx))
        {
            return true;
        }
        self.active.as_ref().is_some_and(|active| {
            active.open.is_focused(window)
                || active.retry.is_focused(window)
                || active.address.is_focused(window)
        })
    }
}

impl Drop for Coordinator {
    fn drop(&mut self) {
        if let Some((_, token)) = &self.armed {
            token.cancel();
        }
        if let Some(token) = &self.leaving {
            token.cancel();
        }
    }
}

fn acquire(window: &mut Window, cx: &mut App) -> Entity<Coordinator> {
    if let Some(current) = coordinator(window, cx) {
        return current;
    }
    let id = window.window_handle().window_id();
    let dependencies = registry(cx).dependencies.clone();
    let entity = cx.new(|cx| {
        let weak = cx.entity().downgrade();
        let keys = cx.intercept_keystrokes(move |event, window, cx| {
            if window.window_handle().window_id() != id {
                return;
            }
            if event.keystroke.modifiers.control
                || event.keystroke.modifiers.alt
                || event.keystroke.modifiers.platform
                || event.keystroke.modifiers.function
                || (event.keystroke.modifiers.shift && event.keystroke.key != "tab")
            {
                return;
            }
            if let Some(entity) = weak.upgrade() {
                keyboard(
                    &entity,
                    &event.keystroke.key,
                    event.keystroke.modifiers.shift,
                    window,
                    cx,
                );
            }
        });
        let activation =
            cx.observe_window_activation(window, |state: &mut Coordinator, window, cx| {
                if !window.is_window_active() {
                    state.dismiss(false, window, cx);
                }
            });
        cx.on_release(move |state: &mut Coordinator, cx| {
            if let Some(page) = state.presentation.take() {
                page.stop(cx);
            }
            if cx.has_global::<Registry>() {
                cx.global_mut::<Registry>().windows.remove(&id);
            }
        })
        .detach();
        Coordinator {
            dependencies,
            visible: Vec::new(),
            staged: Vec::new(),
            scopes: Vec::new(),
            focused: None,
            hovered: None,
            suppressed: None,
            armed: None,
            leaving: None,
            active: None,
            card_bounds: None,
            presentation: None,
            pointer_down: false,
            generation: 0,
            _activation: activation,
            _keys: keys,
        }
    });
    registry(cx).windows.insert(id, entity.downgrade());
    entity
}

pub(crate) fn open_target(target: &str, cx: &mut App) {
    cx.open_url(target);
}

pub struct MarkdownReadingScope {
    id: String,
    focus: FocusHandle,
    scroll: ScrollSource,
    children: Vec<AnyElement>,
    stretch: bool,
}

impl MarkdownReadingScope {
    pub fn new(id: impl Into<String>, focus: FocusHandle) -> Self {
        Self {
            id: id.into(),
            focus,
            scroll: ScrollSource::None,
            children: Vec::new(),
            stretch: false,
        }
    }
    pub fn fill_height(mut self) -> Self {
        self.stretch = true;
        self
    }
    pub fn track_scroll(mut self, handle: ScrollHandle) -> Self {
        self.scroll = ScrollSource::Handle(handle);
        self
    }
    pub fn track_list(mut self, list: ListState) -> Self {
        self.scroll = ScrollSource::List(list);
        self
    }
}

impl ParentElement for MarkdownReadingScope {
    fn extend(&mut self, children: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(children);
    }
}
impl IntoElement for MarkdownReadingScope {
    type Element = ReadingScopeElement;
    fn into_element(self) -> Self::Element {
        ReadingScopeElement {
            scope: ReadingScope {
                id: self.id.clone(),
                focus: self.focus.clone(),
                scroll: self.scroll,
            },
            body: div()
                .id(ElementId::Name(self.id.into()))
                .track_focus(&self.focus)
                .tab_group()
                .tab_stop(false)
                .w_full()
                .min_w_0()
                .when(self.stretch, |view| view.h_full().flex_1().min_h_0())
                .children(self.children)
                .into_any_element(),
        }
    }
}

pub struct ReadingScopeElement {
    scope: ReadingScope,
    body: AnyElement,
}

impl IntoElement for ReadingScopeElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for ReadingScopeElement {
    type RequestLayoutState = ();
    type PrepaintState = ();
    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.body.request_layout(window, cx), ())
    }
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        registry(cx).stack.push(self.scope.clone());
        if let Some(entity) = coordinator(window, cx) {
            entity.update(cx, |state, _| state.scopes.push(self.scope.clone()));
        }
        self.body.prepaint(window, cx);
        registry(cx).stack.pop();
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.body.paint(window, cx);
    }
}

pub struct MarkdownLinkPreviewLayer;
impl IntoElement for MarkdownLinkPreviewLayer {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for MarkdownLinkPreviewLayer {
    type RequestLayoutState = Entity<Coordinator>;
    type PrepaintState = ();
    fn id(&self) -> Option<ElementId> {
        Some("markdown-link-preview-layer".into())
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Entity<Coordinator>) {
        let entity = acquire(window, cx);
        (
            window.request_layout(
                Style {
                    size: size(px(0.).into(), px(0.).into()),
                    flex_shrink: 0.,
                    ..Default::default()
                },
                [],
                cx,
            ),
            entity,
        )
    }
    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        entity: &mut Entity<Coordinator>,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(id) = id {
            window.with_element_state::<Entity<Coordinator>, _>(id, |_, _| ((), entity.clone()));
        }
        let (focus_changed, focused) = entity.update(cx, |state, cx| {
            for link in &mut state.staged {
                link.scroll = link
                    .scopes
                    .iter()
                    .map(|scope| scope.scroll.stamp())
                    .collect();
            }
            let previous = std::mem::replace(&mut state.visible, std::mem::take(&mut state.staged));
            let stale_arm = state.armed.as_ref().is_some_and(|(armed, _)| {
                !armed.current(window, cx)
                    || !state.visible.iter().any(|link| {
                        link.key == armed.key
                            && link.current(window, cx)
                            && link.text_bounds == armed.text_bounds
                            && link.icon_bounds == armed.icon_bounds
                            && link.scroll == armed.scroll
                    })
            });
            if stale_arm && let Some((_, token)) = state.armed.take() {
                token.cancel();
            }
            let mut closed = false;
            let current_key = state
                .active
                .as_ref()
                .map(|active| active.key.clone())
                .or_else(|| state.armed.as_ref().map(|(link, _)| link.key.clone()))
                .or_else(|| state.focused.clone());
            if let Some(key) = current_key {
                let old = previous.iter().find(|link| link.key == key);
                let current = state
                    .visible
                    .iter()
                    .find(|link| link.key == key && link.current(window, cx));
                if current.is_none()
                    || (state.active.is_some() && !state.active_current(window, cx))
                    || old
                        .zip(current)
                        .is_some_and(|(old, current)| old.scroll != current.scroll)
                {
                    let fallback = old
                        .and_then(|link| {
                            link.scopes
                                .iter()
                                .rev()
                                .find(|scope| state.scopes.iter().any(|live| live.id == scope.id))
                        })
                        .map(|scope| scope.focus.clone());
                    let restore = state.card_focused(window, cx)
                        || old.is_some_and(|link| link.focus.is_focused(window));
                    state.dismiss(false, window, cx);
                    if let Some(hovered) = state.pointer_link(window.mouse_position(), window, cx) {
                        state.hovered = Some(hovered.clone());
                        state.suppressed = Some(hovered);
                    }
                    closed = true;
                    if restore && let Some(fallback) = fallback {
                        window.focus(&fallback, cx);
                    }
                }
            }
            let focused = state
                .visible
                .iter()
                .find(|link| link.focus.is_focused(window) && link.current(window, cx))
                .map(|link| link.key.clone());
            let changed = state.focused != focused;
            if changed
                && !closed
                && state.suppressed.as_ref() != state.hovered.as_ref()
                && (state.suppressed.as_ref() == state.focused.as_ref()
                    || state.suppressed.as_ref() == focused.as_ref())
            {
                state.suppressed = None;
            }
            state.focused = focused.clone();
            state.scopes.clear();
            (changed, focused)
        });
        if focus_changed && let Some(key) = focused {
            arm(entity, key, window, cx);
        }
        pointer(entity, window.mouse_position(), window, cx);
        let overlay = build_card(entity, window, cx);
        if let Some((anchor, card)) = overlay {
            let bounds_state = entity.downgrade();
            let measured = div()
                .child(card)
                .on_children_prepainted(move |bounds, window, cx| {
                    let _ = bounds_state.update(cx, |state, _| {
                        native::positioned(state, bounds.first().copied(), window);
                    });
                });
            let mut positioned = Positioner::side(anchor)
                .placement(Placement::Bottom)
                .align(Align::Start)
                .offset(px(Layout::MARKDOWN_LINK_ICON_GAP))
                .margin(px(Layout::LINK_PREVIEW_MARGIN))
                .occlude()
                .child(measured)
                .into_any_element();
            positioned.layout_as_root(
                size(
                    AvailableSpace::Definite(window.viewport_size().width),
                    AvailableSpace::Definite(window.viewport_size().height),
                ),
                window,
                cx,
            );
            window.defer_draw(positioned, point(px(0.), px(0.)), 100, None);
        }
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        entity: &mut Entity<Coordinator>,
        _: &mut (),
        window: &mut Window,
        _: &mut App,
    ) {
        let weak = entity.downgrade();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
            if !phase.capture() {
                return;
            }
            if let Some(entity) = weak.upgrade() {
                pointer(&entity, event.position, window, cx);
            }
        });
        let weak = entity.downgrade();
        window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
            if !phase.capture() {
                return;
            }
            let _ = weak.update(cx, |state, cx| {
                state.pointer_down = true;
                if state
                    .card_bounds
                    .is_some_and(|bounds| bounds.contains(&event.position))
                {
                    return;
                }
                let suppressed = state.pointer_link(event.position, window, cx);
                state.hovered = suppressed.clone();
                state.dismiss(false, window, cx);
                if let Some(key) = suppressed {
                    if event.button == MouseButton::Left
                        && event.click_count == 1
                        && !event.modifiers.modified()
                        && let Some(link) = state.visible.iter().find(|link| link.key == key)
                    {
                        window.focus(&link.focus, cx);
                    }
                    state.suppressed = Some(key);
                }
            });
        });
        let weak = entity.downgrade();
        window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
            if phase.capture() {
                let _ = weak.update(cx, |state, cx| {
                    state.pointer_down = false;
                    if !state
                        .card_bounds
                        .is_some_and(|bounds| bounds.contains(&event.position))
                        && let Some(key) = state.pointer_link(event.position, window, cx)
                    {
                        state.hovered = Some(key.clone());
                        state.suppressed = Some(key);
                    }
                });
            }
        });
        let weak = entity.downgrade();
        window.on_mouse_event(move |event: &ScrollWheelEvent, phase, window, cx| {
            if !phase.capture() {
                return;
            }
            let _ = weak.update(cx, |state, cx| {
                if !state
                    .card_bounds
                    .is_some_and(|bounds| bounds.contains(&event.position))
                {
                    state.dismiss(false, window, cx);
                }
            });
        });
    }
}

pub(crate) fn interactive_bounds(
    mut bounds: Bounds<Pixels>,
    links: &[LinkRange],
) -> Bounds<Pixels> {
    if links.iter().any(|link| link.starts_here) {
        bounds.origin.x -= px(Layout::MARKDOWN_LINK_LEADING_WIDTH);
        bounds.size.width += px(Layout::MARKDOWN_LINK_LEADING_WIDTH);
    }
    bounds
}

pub(crate) fn icon_bounds(layout: &TextLayout, link: &LinkRange) -> Option<Bounds<Pixels>> {
    if !link.starts_here {
        return None;
    }
    let start = layout.position_for_index(link.range.start)?;
    Some(Bounds::new(
        point(
            start.x - px(Layout::MARKDOWN_LINK_LEADING_WIDTH),
            start.y + (layout.line_height() - px(Layout::ICON_SIZE)) / 2.,
        ),
        size(px(Layout::ICON_SIZE), px(Layout::ICON_SIZE)),
    ))
}

pub(crate) fn prepaint_link_targets(
    _: Option<&GlobalElementId>,
    layout: &TextLayout,
    links: &[LinkRange],
    window: &mut Window,
    cx: &mut App,
) -> Vec<AnyElement> {
    let regions = link_regions(layout, links, window.text_style().text_align);
    let mut targets = Vec::new();
    for (index, link) in links.iter().enumerate() {
        let bounds = regions
            .iter()
            .filter(|region| region.index == index)
            .map(|region| region.bounds)
            .collect::<Vec<_>>();
        if bounds.is_empty() {
            continue;
        }
        if let Some(icon) = icon_bounds(layout, link)
            && let Some(glyph) = decoration::prepare(
                format!(
                    "link-icon-{}-{}-{}",
                    link.owner, link.occurrence, link.range.start
                ),
                vec![icon],
                Some(classify_target(&link.target).map_or(LinkSiteKind::Web, |target| target.site)),
                theme(cx).colors.accent,
                window,
                cx,
            )
        {
            targets.push(glyph);
        }
        targets.extend(prepaint_measured_link_target(
            link,
            bounds,
            icon_bounds(layout, link).into_iter().collect(),
            None,
            window,
            cx,
        ));
    }
    targets.extend(prepaint_link_decorations(layout, links, window, cx));
    targets
}

pub(crate) fn prepaint_measured_link_target(
    link: &LinkRange,
    bounds: Vec<Bounds<Pixels>>,
    icons: Vec<Bounds<Pixels>>,
    validity: Option<LinkValidity>,
    window: &mut Window,
    cx: &mut App,
) -> Vec<AnyElement> {
    if validity.as_ref().is_some_and(|valid| !valid(window, cx)) {
        return Vec::new();
    }
    let Some(entity) = coordinator(window, cx) else {
        return Vec::new();
    };
    let mask = window.content_mask().bounds;
    let clip = |bounds: Vec<Bounds<Pixels>>| {
        bounds
            .into_iter()
            .filter(|bounds| {
                [
                    f32::from(bounds.origin.x),
                    f32::from(bounds.origin.y),
                    f32::from(bounds.size.width),
                    f32::from(bounds.size.height),
                ]
                .iter()
                .all(|value| value.is_finite())
            })
            .map(|bounds| bounds.intersect(&mask))
            .filter(|bounds| !bounds.is_empty())
            .collect::<Vec<_>>()
    };
    let bounds = clip(bounds);
    if bounds.is_empty() {
        return Vec::new();
    }
    let icons = clip(icons);
    let scopes = cx
        .try_global::<Registry>()
        .map(|registry| registry.stack.clone())
        .unwrap_or_default();
    let scope_key = scopes
        .iter()
        .map(|scope| scope.id.as_str())
        .collect::<Vec<_>>()
        .join("/");
    let key = format!(
        "{scope_key}/{}:{}:{}:{}",
        link.owner, link.occurrence, link.text_identity, link.target
    );
    let mut targets = Vec::new();
    let proxy = entity.update(cx, |state, cx| {
        if let Some(existing) = state.staged.iter_mut().find(|existing| existing.key == key) {
            existing.text_bounds.extend(bounds.clone());
            existing.icon_bounds.extend(icons.clone());
            if let Some(validity) = validity.clone() {
                let previous = existing.validity.take();
                existing.validity = Some(Rc::new(move |window, cx| {
                    previous.as_ref().is_none_or(|valid| valid(window, cx)) && validity(window, cx)
                }));
            }
            return None;
        }
        let focus = state
            .visible
            .iter()
            .find(|existing| existing.key == key && existing.target == link.target)
            .map(|existing| existing.focus.clone())
            .unwrap_or_else(|| cx.focus_handle().tab_stop(true));
        state.staged.push(VisibleLink {
            key: key.clone(),
            target: link.target.clone(),
            label: link.label.clone(),
            owner: link.owner.clone(),
            occurrence: link.occurrence,
            role: Role::Link,
            description: link.target.clone(),
            text_bounds: bounds.clone(),
            icon_bounds: icons,
            focus: focus.clone(),
            scroll: Vec::new(),
            scopes: scopes.clone(),
            validity: validity.clone(),
        });
        state.staged.last().map(|link| {
            (
                focus,
                link.role,
                link.label.clone(),
                link.description.clone(),
            )
        })
    });
    if let Some((focus, role, label, description)) = proxy {
        let weak = entity.downgrade();
        let action_key = key.clone();
        let action_validity = validity.clone();
        let mut proxy = div()
            .id(ElementId::Name(
                format!("markdown-link-target-{key}").into(),
            ))
            .track_focus(&focus)
            .tab_index(0)
            .role(role)
            .aria_label(label)
            .aria_description(description)
            .on_a11y_action(AccessibleAction::Click, move |_, window, cx| {
                if action_validity
                    .as_ref()
                    .is_some_and(|valid| !valid(window, cx))
                {
                    return;
                }
                let _ = weak.update(cx, |state, cx| {
                    if let Some(link) = state
                        .visible
                        .iter()
                        .find(|link| link.key == action_key && link.current(window, cx))
                    {
                        open_target(&link.target, cx);
                    }
                });
            })
            .w(bounds[0].size.width)
            .h(bounds[0].size.height)
            .into_any_element();
        proxy.prepaint_as_root(
            bounds[0].origin,
            size(
                AvailableSpace::Definite(bounds[0].size.width),
                AvailableSpace::Definite(bounds[0].size.height),
            ),
            window,
            cx,
        );
        targets.push(proxy);
    }
    let focused =
        entity.read(cx).staged.iter().any(|link| {
            link.key == key && link.focus.is_focused(window) && link.current(window, cx)
        });
    if focused {
        let rectangles = bounds
            .iter()
            .flat_map(|bounds| decoration::focus_rectangles(*bounds, window.scale_factor()))
            .collect();
        if let Some(focus) = decoration::prepare(
            format!("link-focus-{key}-{}", link.range.start),
            rectangles,
            None,
            theme(cx).colors.accent,
            window,
            cx,
        ) {
            targets.push(focus);
        }
    }
    targets
}

#[cfg(feature = "nsh-reading-integration")]
fn measured_focus_rects(
    links: &[VisibleLink],
    owners: &[LinkRange],
    window: &Window,
    cx: &App,
) -> Vec<Bounds<Pixels>> {
    links
        .iter()
        .filter(|link| {
            owners.iter().any(|owner| {
                owner.owner == link.owner
                    && owner.target == link.target
                    && owner.occurrence == link.occurrence
            }) && link.focus.is_focused(window)
                && link.current(window, cx)
        })
        .flat_map(|link| link.text_bounds.iter().copied())
        .collect()
}

pub(crate) fn prepaint_link_decorations(
    layout: &TextLayout,
    links: &[LinkRange],
    window: &mut Window,
    cx: &mut App,
) -> Vec<AnyElement> {
    let regions = link_regions(layout, links, window.text_style().text_align);
    let color = theme(cx).colors.accent;
    let current = coordinator(window, cx);
    let mut targets = Vec::new();
    for (index, link) in links.iter().enumerate() {
        let (focused, hovered) = current.as_ref().map_or((false, false), |entity| {
            let state = entity.read(cx);
            let occurrence = state.staged.iter().find(|visible| {
                visible.owner == link.owner
                    && visible.target == link.target
                    && visible.occurrence == link.occurrence
                    && visible.current(window, cx)
            });
            occurrence.map_or((false, false), |visible| {
                (
                    visible.focus.is_focused(window),
                    state.hovered.as_ref() == Some(&visible.key),
                )
            })
        });
        let hovered = hovered
            || regions
                .iter()
                .filter(|region| region.index == index)
                .any(|region| region.bounds.contains(&window.mouse_position()))
            || icon_bounds(layout, link)
                .is_some_and(|bounds| bounds.contains(&window.mouse_position()));
        if !focused && !hovered {
            continue;
        }
        let mut rectangles = Vec::new();
        for region in regions.iter().filter(|region| region.index == index) {
            let scale = window.scale_factor();
            let baseline = px(
                (f32::from(region.baseline + px(Layout::MARKDOWN_LINK_DASH_OFFSET)) * scale)
                    .round()
                    / scale,
            );
            let mut x = px((f32::from(region.bounds.left()) * scale).ceil() / scale);
            let right = px((f32::from(region.bounds.right()) * scale).floor() / scale);
            let dash = px((Layout::MARKDOWN_LINK_DASH * scale).round().max(1.) / scale);
            let gap = px((Layout::MARKDOWN_LINK_DASH_GAP * scale).round().max(1.) / scale);
            let thickness = px((Layout::MARKDOWN_LINK_DASH_THICKNESS * scale)
                .round()
                .max(1.)
                / scale);
            while x < right {
                let width = dash.min(right - x);
                rectangles.push(Bounds::new(point(x, baseline), size(width, thickness)));
                x += dash + gap;
            }
        }
        if let Some(decoration) = decoration::prepare(
            format!(
                "link-dashes-{}-{}-{}",
                link.owner, link.occurrence, link.range.start
            ),
            rectangles,
            None,
            color,
            window,
            cx,
        ) {
            targets.push(decoration);
        }
    }
    targets
}

fn arm(entity: &Entity<Coordinator>, key: String, window: &mut Window, cx: &mut App) {
    let token = entity.update(cx, |state, cx| {
        if !state
            .visible
            .iter()
            .any(|link| link.key == key && link.current(window, cx))
            || state.pointer_down
            || state.suppressed.as_ref() == Some(&key)
            || state
                .active
                .as_ref()
                .is_some_and(|active| active.key == key)
            || state
                .armed
                .as_ref()
                .is_some_and(|(armed, _)| armed.key == key)
        {
            return None;
        }
        if let Some((_, token)) = state.armed.take() {
            token.cancel();
        }
        if let Some(active) = state.active.as_ref()
            && active.key != key
        {
            state.dismiss(false, window, cx);
            state.suppressed = None;
        }
        let token = CancellationToken::new();
        let link = state.visible.iter().find(|link| link.key == key)?.clone();
        state.armed = Some((link.clone(), token.clone()));
        Some((token, link))
    });
    let Some((token, armed_link)) = token else {
        return;
    };
    let weak = entity.downgrade();
    let handle = window.window_handle();
    cx.spawn(async move |cx| {
        tokio::select! { biased; _ = token.cancelled() => return, _ = cx.background_executor().timer(Duration::from_millis(400)) => {} }
        let _ = cx.update_window(handle, |_, window, cx| {
            let Some(entity) = weak.upgrade() else { return };
            let show = entity.update(cx, |state, cx| {
                if token.is_cancelled() || !window.is_window_active() || state.suppressed.as_ref() == Some(&key) || !armed_link.current(window, cx) || armed_link.scopes.iter().map(|scope| scope.scroll.stamp()).collect::<Vec<_>>() != armed_link.scroll { return false; }
                let Some(link) = state.visible.iter().find(|link| link.key == key && link.current(window, cx)).cloned() else { return false };
                if link.text_bounds != armed_link.text_bounds || link.icon_bounds != armed_link.icon_bounds || link.scroll != armed_link.scroll { return false; }
                if state.hovered.as_ref() != Some(&key) && state.focused.as_ref() != Some(&key) { return false; }
                state.generation = state.generation.wrapping_add(1);
                state.armed = None;
                state.active = Some(ActivePreview { key, target: link.target, generation: state.generation, cancellation: CancellationToken::new(), data: None, open: cx.focus_handle().tab_stop(true), retry: cx.focus_handle().tab_stop(true), address: cx.focus_handle().tab_stop(true), expanded: false, validity: link.validity });
                true
            });
            if show { load(&entity, false, window, cx); }
            else if !token.is_cancelled() {
                entity.update(cx, |state, _| {
                    if state.armed.as_ref().is_some_and(|(armed, _)| armed.key == armed_link.key)
                        && let Some((_, token)) = state.armed.take() { token.cancel(); }
                });
            }
        });
    }).detach();
}

fn load(entity: &Entity<Coordinator>, retry: bool, window: &mut Window, cx: &mut App) {
    let request = entity.update(cx, |state, cx| {
        if !state.active_current(window, cx) {
            state.dismiss(false, window, cx);
            return None;
        }
        state.generation = state.generation.wrapping_add(1);
        let active = state.active.as_mut()?;
        if active.retry.is_focused(window) {
            window.focus(&active.open, cx);
        }
        active.cancellation.cancel();
        active.cancellation = CancellationToken::new();
        active.generation = state.generation;
        active.data = None;
        Some((
            state.dependencies.service.clone(),
            active.target.clone(),
            active.cancellation.clone(),
            active.generation,
            active.key.clone(),
        ))
    });
    let Some((service, target, cancellation, generation, key)) = request else {
        return;
    };
    #[cfg(feature = "nsh-reading-integration")]
    let external_request = registry(cx).request.clone();
    window.refresh();
    let weak = entity.downgrade();
    let handle = window.window_handle();
    cx.spawn(async move |cx| {
        let result = cx
            .background_executor()
            .spawn(async move {
                #[cfg(feature = "nsh-reading-integration")]
                if let Some(request) = external_request {
                    return request(target, retry, cancellation).await;
                }
                service.load(&target, retry, cancellation).await
            })
            .await;
        let _ = cx.update_window(handle, |_, window, cx| {
            let _ = weak.update(cx, |state, cx| {
                if !state.active_current(window, cx) {
                    state.dismiss(false, window, cx);
                    return;
                }
                if let Some(active) = &mut state.active
                    && active.key == key
                    && active.generation == generation
                    && !active.cancellation.is_cancelled()
                {
                    active.data = Some(result);
                    window.refresh();
                }
            });
        });
    })
    .detach();
}

fn pointer(
    entity: &Entity<Coordinator>,
    position: Point<Pixels>,
    window: &mut Window,
    cx: &mut App,
) {
    let (candidate, leave) = entity.update(cx, |state, cx| {
        let hovered = state.pointer_link(position, window, cx);
        if state.hovered != hovered {
            if !state.pointer_down
                && (state.suppressed.as_ref() == state.hovered.as_ref()
                    || state.suppressed.as_ref() == hovered.as_ref())
            {
                state.suppressed = None;
            }
            state.hovered = hovered.clone();
        }
        let card = state
            .card_bounds
            .is_some_and(|bounds| bounds.contains(&position));
        if card || hovered.is_some() || state.focused.is_some() || state.card_focused(window, cx) {
            if let Some(token) = state.leaving.take() {
                token.cancel();
            }
            ((!state.pointer_down).then_some(hovered).flatten(), None)
        } else {
            if let Some((_, token)) = state.armed.take() {
                token.cancel();
            }
            if state.active.is_some() && state.leaving.is_none() {
                let token = CancellationToken::new();
                state.leaving = Some(token.clone());
                (None, Some(token))
            } else {
                (None, None)
            }
        }
    });
    if let Some(key) = candidate {
        arm(entity, key, window, cx);
    }
    if let Some(token) = leave {
        let weak = entity.downgrade();
        let handle = window.window_handle();
        cx.spawn(async move |cx| {
            tokio::select! { biased; _ = token.cancelled() => return, _ = cx.background_executor().timer(Duration::from_millis(150)) => {} }
            let _ = cx.update_window(handle, |_, window, cx| { let _ = weak.update(cx, |state, cx| if !token.is_cancelled() { state.dismiss(false, window, cx); }); });
        }).detach();
    }
}

fn keyboard(
    entity: &Entity<Coordinator>,
    key: &str,
    backwards: bool,
    window: &mut Window,
    cx: &mut App,
) {
    let mut retry = false;
    let handled = entity.update(cx, |state, cx| {
        if key == "escape" && (state.active.is_some() || state.armed.is_some()) {
            state.dismiss(true, window, cx);
            return true;
        }
        if let Some(link) = state
            .visible
            .iter()
            .find(|link| link.focus.is_focused(window) && link.current(window, cx))
            .cloned()
        {
            if key == "enter" {
                open_target(&link.target, cx);
                return true;
            }
            if key == "tab" {
                if backwards {
                    if let Some(index) = state.visible.iter().position(|item| item.key == link.key)
                        && let Some(previous) = index
                            .checked_sub(1)
                            .and_then(|index| state.visible.get(index))
                    {
                        window.focus(&previous.focus, cx);
                    } else {
                        window.focus_prev(cx);
                    }
                    return true;
                }
                if let Some(active) = &state.active
                    && active.key == link.key
                {
                    window.focus(&active.open, cx);
                } else {
                    window.focus_next(cx);
                }
                return true;
            }
        }
        if state.active.is_some() && !state.active_current(window, cx) {
            state.dismiss(false, window, cx);
            return false;
        }
        let card_focused = state.card_focused(window, cx);
        if let Some(active) = &mut state.active {
            let can_retry = active.data.as_ref().is_some_and(|data| {
                matches!(
                    data,
                    Err(LinkPreviewFailure::Network
                        | LinkPreviewFailure::Timeout
                        | LinkPreviewFailure::Malformed
                        | LinkPreviewFailure::TooLarge
                        | LinkPreviewFailure::Busy)
                )
            });
            let details = active.target.len() > 120;
            if matches!(key, "enter" | "space") {
                if active.open.is_focused(window) {
                    open_target(&active.target, cx);
                    return true;
                }
                if active.retry.is_focused(window) && can_retry {
                    retry = true;
                    return true;
                }
                if active.address.is_focused(window) {
                    active.expanded = !active.expanded;
                    window.refresh();
                    return true;
                }
            }
            if key == "tab" && card_focused {
                if !active.open.is_focused(window)
                    && !active.retry.is_focused(window)
                    && !active.address.is_focused(window)
                {
                    window.focus(
                        if backwards && details {
                            &active.address
                        } else if backwards && can_retry {
                            &active.retry
                        } else {
                            &active.open
                        },
                        cx,
                    );
                    return true;
                }
                if backwards {
                    if active.address.is_focused(window) && can_retry {
                        window.focus(&active.retry, cx);
                    } else if active.address.is_focused(window) || active.retry.is_focused(window) {
                        window.focus(&active.open, cx);
                    } else if let Some(link) =
                        state.visible.iter().find(|link| link.key == active.key)
                    {
                        window.focus(&link.focus, cx);
                    }
                    return true;
                }
                if active.open.is_focused(window) && can_retry {
                    window.focus(&active.retry, cx);
                } else if (active.open.is_focused(window) || active.retry.is_focused(window))
                    && details
                {
                    window.focus(&active.address, cx);
                } else if let Some(index) =
                    state.visible.iter().position(|link| link.key == active.key)
                {
                    if let Some(next) = state.visible.get(index + 1) {
                        window.focus(&next.focus, cx);
                    } else {
                        window.focus_next(cx);
                        for _ in 0..3 {
                            if active.open.is_focused(window)
                                || active.retry.is_focused(window)
                                || active.address.is_focused(window)
                            {
                                window.focus_next(cx);
                            } else {
                                break;
                            }
                        }
                    }
                }
                return true;
            }
        }
        if key == "tab"
            && !backwards
            && state
                .visible
                .iter()
                .flat_map(|link| &link.scopes)
                .any(|scope| scope.focus.is_focused(window))
            && let Some(first) = state.visible.first()
        {
            window.focus(&first.focus, cx);
            return true;
        }
        false
    });
    if handled {
        cx.stop_propagation();
    }
    if retry {
        load(entity, true, window, cx);
    }
}

fn build_card(
    entity: &Entity<Coordinator>,
    window: &mut Window,
    cx: &mut App,
) -> Option<(Bounds<Pixels>, AnyElement)> {
    native::card(entity, window, cx)
}

#[cfg(test)]
#[path = "link_preview_tests.rs"]
mod tests;
