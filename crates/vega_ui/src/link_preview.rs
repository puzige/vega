use std::{collections::HashMap, sync::Arc, time::Duration};

use gpui_kit::{
    base::{Align, Placement, Positioner, SelectableText},
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

#[cfg(test)]
type LinkOpener = std::rc::Rc<dyn Fn(&str)>;

#[derive(Clone, Default)]
struct Dependencies {
    service: LinkPreviewService,
    #[cfg(test)]
    opener: Option<LinkOpener>,
}

#[derive(Default)]
struct Registry {
    windows: HashMap<WindowId, WeakEntity<Coordinator>>,
    stack: Vec<ReadingScope>,
    dependencies: Dependencies,
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
}

struct ActivePreview {
    key: String,
    target: String,
    generation: u64,
    cancellation: CancellationToken,
    data: Option<Result<LinkPreviewData, LinkPreviewFailure>>,
    image: Option<Arc<Image>>,
    open: FocusHandle,
    retry: FocusHandle,
    address: FocusHandle,
    expanded: bool,
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
    armed: Option<(String, CancellationToken)>,
    leaving: Option<CancellationToken>,
    active: Option<ActivePreview>,
    card_bounds: Option<Bounds<Pixels>>,
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
            .or_else(|| self.armed.as_ref().map(|(key, _)| key.clone()));
        if let Some(dismissed) = dismissed {
            self.suppressed = Some(dismissed);
        }
        if let Some((_, token)) = self.armed.take() {
            token.cancel();
        }
        if let Some(token) = self.leaving.take() {
            token.cancel();
        }
        if let Some(active) = self.active.take()
            && keyboard
            && (active.open.is_focused(window)
                || active.retry.is_focused(window)
                || active.address.is_focused(window))
            && let Some(link) = self.visible.iter().find(|link| link.key == active.key)
        {
            window.focus(&link.focus, cx);
            self.focused = Some(link.key.clone());
        }
        self.card_bounds = None;
        self.generation = self.generation.wrapping_add(1);
        window.refresh();
    }

    fn pointer_link(&self, position: Point<Pixels>) -> Option<String> {
        self.visible
            .iter()
            .find(|link| {
                link.text_bounds
                    .iter()
                    .chain(&link.icon_bounds)
                    .any(|bounds| bounds.contains(&position))
            })
            .map(|link| link.key.clone())
    }

    fn card_focused(&self, window: &Window) -> bool {
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
        cx.on_release(move |_, cx| {
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
    #[cfg(test)]
    if let Some(opener) = cx
        .try_global::<Registry>()
        .and_then(|registry| registry.dependencies.opener.clone())
    {
        opener(target);
        return;
    }
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
            let mut closed = false;
            let current_key = state
                .active
                .as_ref()
                .map(|active| active.key.clone())
                .or_else(|| state.armed.as_ref().map(|(key, _)| key.clone()))
                .or_else(|| state.focused.clone());
            if let Some(key) = current_key {
                let old = previous.iter().find(|link| link.key == key);
                let current = state.visible.iter().find(|link| link.key == key);
                if current.is_none()
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
                    let restore = state.card_focused(window)
                        || old.is_some_and(|link| link.focus.is_focused(window));
                    state.dismiss(false, window, cx);
                    if let Some(hovered) = state.pointer_link(window.mouse_position()) {
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
                .find(|link| link.focus.is_focused(window))
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
                .on_children_prepainted(move |bounds, _, cx| {
                    let _ = bounds_state
                        .update(cx, |state, _| state.card_bounds = bounds.first().copied());
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
                let suppressed = state.pointer_link(event.position);
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
        window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
            if phase.capture() {
                let _ = weak.update(cx, |state, _| {
                    state.pointer_down = false;
                    if !state
                        .card_bounds
                        .is_some_and(|bounds| bounds.contains(&event.position))
                        && let Some(key) = state.pointer_link(event.position)
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
    let Some(entity) = coordinator(window, cx) else {
        return Vec::new();
    };
    let scopes = cx
        .try_global::<Registry>()
        .map(|registry| registry.stack.clone())
        .unwrap_or_default();
    let scope_key = scopes
        .iter()
        .map(|scope| scope.id.as_str())
        .collect::<Vec<_>>()
        .join("/");
    let mask = window.content_mask().bounds;
    let regions = link_regions(layout, links, window.text_style().text_align);
    let mut targets = Vec::new();
    for (index, link) in links.iter().enumerate() {
        let bounds = regions
            .iter()
            .filter(|region| region.index == index)
            .map(|region| region.bounds.intersect(&mask))
            .filter(|bounds| !bounds.is_empty())
            .collect::<Vec<_>>();
        if bounds.is_empty() {
            continue;
        }
        if let Some(icon) = icon_bounds(layout, link) {
            let mut glyph = crate::icons::link_icon(
                classify_target(&link.target).map_or(LinkSiteKind::Web, |target| target.site),
                theme(cx).colors.accent,
            );
            glyph.prepaint_as_root(
                icon.origin,
                size(
                    AvailableSpace::Definite(icon.size.width),
                    AvailableSpace::Definite(icon.size.height),
                ),
                window,
                cx,
            );
            targets.push(glyph);
        }
        let icons = icon_bounds(layout, link)
            .filter(|bounds| !bounds.intersect(&mask).is_empty())
            .into_iter()
            .collect::<Vec<_>>();
        let key = format!(
            "{scope_key}/{}:{}:{}",
            link.owner, link.occurrence, link.target
        );
        let proxy = entity.update(cx, |state, cx| {
            if let Some(existing) = state.staged.iter_mut().find(|existing| existing.key == key) {
                existing.text_bounds.extend(bounds.clone());
                existing.icon_bounds.extend(icons.clone());
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
            let mut proxy = div()
                .id(ElementId::Name(
                    format!("markdown-link-target-{key}").into(),
                ))
                .track_focus(&focus)
                .tab_index(0)
                .role(role)
                .aria_label(label)
                .aria_description(description)
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
    }
    targets
}

pub(crate) fn paint_link_decorations(
    layout: &TextLayout,
    links: &[LinkRange],
    window: &mut Window,
    cx: &mut App,
) {
    let regions = link_regions(layout, links, window.text_style().text_align);
    let color = theme(cx).colors.accent;
    let current = coordinator(window, cx);
    for (index, link) in links.iter().enumerate() {
        let (focused, hovered) = current.as_ref().map_or((false, false), |entity| {
            let state = entity.read(cx);
            let occurrence = state.visible.iter().find(|visible| {
                visible.owner == link.owner
                    && visible.target == link.target
                    && visible.occurrence == link.occurrence
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
                window.paint_quad(fill(
                    Bounds::new(point(x, baseline), size(width, thickness)),
                    color,
                ));
                x += dash + gap;
            }
        }
    }
}

fn arm(entity: &Entity<Coordinator>, key: String, window: &mut Window, cx: &mut App) {
    let token = entity.update(cx, |state, cx| {
        if state.pointer_down
            || state.suppressed.as_ref() == Some(&key)
            || state
                .active
                .as_ref()
                .is_some_and(|active| active.key == key)
            || state.armed.as_ref().is_some_and(|(armed, _)| armed == &key)
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
        state.armed = Some((key.clone(), token.clone()));
        Some(token)
    });
    let Some(token) = token else { return };
    let weak = entity.downgrade();
    let handle = window.window_handle();
    cx.spawn(async move |cx| {
        tokio::select! { biased; _ = token.cancelled() => return, _ = cx.background_executor().timer(Duration::from_millis(400)) => {} }
        let _ = cx.update_window(handle, |_, window, cx| {
            let Some(entity) = weak.upgrade() else { return };
            let show = entity.update(cx, |state, cx| {
                if token.is_cancelled() || !window.is_window_active() || state.suppressed.as_ref() == Some(&key) { return false; }
                let Some(link) = state.visible.iter().find(|link| link.key == key).cloned() else { return false };
                if state.hovered.as_ref() != Some(&key) && state.focused.as_ref() != Some(&key) { return false; }
                state.generation = state.generation.wrapping_add(1);
                state.armed = None;
                state.active = Some(ActivePreview { key, target: link.target, generation: state.generation, cancellation: CancellationToken::new(), data: None, image: None, open: cx.focus_handle().tab_stop(true), retry: cx.focus_handle().tab_stop(true), address: cx.focus_handle().tab_stop(true), expanded: false });
                true
            });
            if show { load(&entity, false, window, cx); }
        });
    }).detach();
}

fn load(entity: &Entity<Coordinator>, retry: bool, window: &mut Window, cx: &mut App) {
    let request = entity.update(cx, |state, cx| {
        state.generation = state.generation.wrapping_add(1);
        let active = state.active.as_mut()?;
        if active.retry.is_focused(window) {
            window.focus(&active.open, cx);
        }
        active.cancellation.cancel();
        active.cancellation = CancellationToken::new();
        active.generation = state.generation;
        active.data = None;
        active.image = None;
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
    window.refresh();
    let weak = entity.downgrade();
    let handle = window.window_handle();
    cx.spawn(async move |cx| {
        let result = cx
            .background_executor()
            .spawn(async move { service.load(&target, retry, cancellation).await })
            .await;
        let _ = cx.update_window(handle, |_, window, cx| {
            let _ = weak.update(cx, |state, _| {
                if let Some(active) = &mut state.active
                    && active.key == key
                    && active.generation == generation
                    && !active.cancellation.is_cancelled()
                {
                    active.image = result
                        .as_ref()
                        .ok()
                        .and_then(|data| data.image.as_ref())
                        .map(|image| {
                            Arc::new(Image::from_bytes(ImageFormat::Png, image.png.clone()))
                        });
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
    let (candidate, leave) = entity.update(cx, |state, _| {
        let hovered = state.pointer_link(position);
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
        if card || hovered.is_some() || state.focused.is_some() || state.card_focused(window) {
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
            .find(|link| link.focus.is_focused(window))
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
        let card_focused = state.card_focused(window);
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
            if key == "enter" {
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
    let state = entity.read(cx);
    let active = state.active.as_ref()?;
    let link = state.visible.iter().find(|link| link.key == active.key)?;
    let anchor = link
        .text_bounds
        .iter()
        .find(|bounds| bounds.contains(&window.mouse_position()))
        .or_else(|| link.text_bounds.first())
        .copied()?;
    let colors = theme(cx).colors;
    let width = px(Layout::LINK_PREVIEW_WIDTH)
        .min((window.viewport_size().width - px(2. * Layout::LINK_PREVIEW_MARGIN)).max(px(0.)));
    let content_width = (width - px(2. * Layout::LINK_PREVIEW_PADDING)).max(px(0.));
    let height = px(Layout::LINK_PREVIEW_MAX_HEIGHT)
        .min((window.viewport_size().height - px(2. * Layout::LINK_PREVIEW_MARGIN)).max(px(0.)));
    let target = active.target.clone();
    let host = active
        .data
        .as_ref()
        .and_then(|result| result.as_ref().ok())
        .map(|data| data.host.clone())
        .unwrap_or_else(|| display_host(&target));
    let site = active
        .data
        .as_ref()
        .and_then(|result| result.as_ref().ok())
        .map(|data| data.site)
        .unwrap_or_else(|| {
            classify_target(&target).map_or(LinkSiteKind::Web, |target| target.site)
        });
    let weak = entity.downgrade();
    let open_target_value = target.clone();
    let open = control("link-preview-open", "Open", &active.open, colors)
        .on_click(move |_, _, cx| open_target(&open_target_value, cx));
    let card = div()
        .id("markdown-link-preview")
        .test_support()
        .occlude()
        .w(width)
        .max_h(height)
        .overflow_hidden()
        .bg(colors.bg_elevated)
        .border_1()
        .border_color(colors.border_subtle)
        .rounded(px(Layout::LINK_PREVIEW_RADIUS))
        .p(px(Layout::LINK_PREVIEW_PADDING))
        .text_size(px(Typography::SIDEBAR))
        .line_height(relative(Typography::BODY_LINE_HEIGHT))
        .text_color(colors.text_primary)
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .h(px(Layout::LINK_PREVIEW_ACTION_HEIGHT))
                .flex_shrink_0()
                .flex()
                .items_center()
                .gap_2()
                .child(crate::icons::link_icon(site, colors.text_secondary))
                .child(
                    div()
                        .id("link-preview-host")
                        .role(Role::Label)
                        .aria_label(host.clone())
                        .min_w_0()
                        .flex_1()
                        .truncate()
                        .child(host),
                )
                .child(open),
        );
    let line_height = window.pixel_snap(px(Typography::SIDEBAR * Typography::BODY_LINE_HEIGHT));
    let mut body = div()
        .id("link-preview-content")
        .min_h_0()
        .max_h(
            (height
                - px(2. * Layout::LINK_PREVIEW_PADDING
                    + Layout::LINK_PREVIEW_ACTION_HEIGHT
                    + 8.
                    + 2.))
            .max(px(0.)),
        )
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap_2();
    if let Some(image) = &active.image
        && let Some(dimensions) = active
            .data
            .as_ref()
            .and_then(|data| data.as_ref().ok())
            .and_then(|data| data.image.as_ref())
    {
        let image_width = content_width.min(px(dimensions.width as f32));
        let image_height = image_width * dimensions.height as f32 / dimensions.width.max(1) as f32;
        body = body.child(
            div()
                .id("link-preview-image")
                .test_support()
                .flex_shrink_0()
                .w(image_width)
                .h(image_height)
                .child(img(image.clone()).w(image_width).h(image_height)),
        );
    }
    match &active.data {
        None => body = body.child(div().child("Loading…")),
        Some(Ok(data)) if data.site != LinkSiteKind::Mail => {
            let title = data.title.clone().unwrap_or_else(|| target.clone());
            body = body.child(
                div()
                    .id("link-preview-title")
                    .test_support()
                    .role(Role::Label)
                    .aria_label(title.clone())
                    .font_weight(FontWeight::MEDIUM)
                    .line_clamp(2)
                    .flex_shrink_0()
                    .max_h(line_height * 2.)
                    .overflow_hidden()
                    .child(SelectableText::new("preview-selectable-title", title)),
            );
            if let Some(description) = &data.description {
                body = body.child(
                    div()
                        .id("link-preview-description")
                        .role(Role::Label)
                        .aria_label(description.clone())
                        .text_color(colors.text_secondary)
                        .line_clamp(3)
                        .flex_shrink_0()
                        .max_h(line_height * 3.)
                        .overflow_hidden()
                        .child(SelectableText::new(
                            "preview-selectable-description",
                            description.clone(),
                        )),
                );
            }
            if data.effective_target != target.split('#').next().unwrap_or(&target) {
                body = body.child(div().text_color(colors.text_secondary).child(
                    SelectableText::new("preview-effective-address", data.effective_target.clone()),
                ));
            }
        }
        Some(Ok(_)) => {}
        Some(Err(failure)) => {
            let retryable = matches!(
                failure,
                LinkPreviewFailure::Network
                    | LinkPreviewFailure::Timeout
                    | LinkPreviewFailure::Malformed
                    | LinkPreviewFailure::TooLarge
                    | LinkPreviewFailure::Busy
            );
            body = body.child(div().text_color(colors.text_secondary).child(if retryable {
                "无法加载预览"
            } else {
                "当前无法提供网页预览"
            }));
            if retryable {
                body = body.child(
                    control("link-preview-retry", "Retry", &active.retry, colors).on_click(
                        move |_, window, cx| {
                            if let Some(entity) = weak.upgrade() {
                                load(&entity, true, window, cx);
                            }
                        },
                    ),
                );
            }
        }
    }
    if target.len() > 120 {
        let weak = entity.downgrade();
        body = body.child(
            control(
                "link-preview-address-toggle",
                if active.expanded {
                    "Hide address"
                } else {
                    "Show address"
                },
                &active.address,
                colors,
            )
            .on_click(move |_, window, cx| {
                let _ = weak.update(cx, |state, _| {
                    if let Some(active) = &mut state.active {
                        active.expanded = !active.expanded;
                        window.refresh();
                    }
                });
            }),
        );
    }
    if target.len() <= 120 || active.expanded {
        body = body.child(
            div()
                .id("link-preview-address")
                .role(Role::Label)
                .aria_label(target.clone())
                .text_size(px(Typography::METADATA))
                .text_color(colors.text_tertiary)
                .child(SelectableText::new("preview-selectable-address", target)),
        );
    }
    Some((anchor, card.child(body).into_any_element()))
}

fn control(
    id: &'static str,
    label: &'static str,
    focus: &FocusHandle,
    colors: vega_theme::ThemeColors,
) -> gpui_kit::base::Button {
    gpui_kit::base::Button::new(id)
        .track_focus(focus)
        .tab_index(0)
        .accessibility_label(label)
        .text_color(colors.accent)
        .h(px(Layout::LINK_PREVIEW_ACTION_HEIGHT))
        .flex_shrink_0()
        .px_2()
        .child(label)
}

#[cfg(test)]
pub mod test_support {
    use super::*;

    pub struct LinkObservation {
        pub key: String,
        pub target: String,
        pub label: String,
        pub description: String,
        pub role: Role,
        pub text_bounds: Vec<Bounds<Pixels>>,
        pub icon_bounds: Vec<Bounds<Pixels>>,
        pub focus: FocusHandle,
    }

    pub fn install_dependencies(service: LinkPreviewService, opener: LinkOpener, cx: &mut App) {
        registry(cx).dependencies = Dependencies {
            service,
            opener: Some(opener),
        };
    }

    pub fn visible_links(window: &Window, cx: &App) -> Vec<LinkObservation> {
        coordinator(window, cx)
            .map(|entity| {
                entity
                    .read(cx)
                    .visible
                    .iter()
                    .map(|link| LinkObservation {
                        key: link.key.clone(),
                        target: link.target.clone(),
                        label: link.label.clone(),
                        description: link.description.clone(),
                        role: link.role,
                        text_bounds: link.text_bounds.clone(),
                        icon_bounds: link.icon_bounds.clone(),
                        focus: link.focus.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}
