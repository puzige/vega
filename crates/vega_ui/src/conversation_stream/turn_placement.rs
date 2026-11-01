use std::{cell::RefCell, rc::Rc};

use gpui_kit::{
    AnyElement, App, AvailableSpace, Bounds, Element, ElementId, FollowMode, GlobalElementId,
    InspectorElementId, IntoElement, KeyDownEvent, LayoutId, ListOffset, ListState, MouseDownEvent,
    Pixels, Refineable, ScrollWheelEvent, Size, Style, StyleRefinement, Styled, Window, list, px,
    size,
};

#[derive(Default)]
pub(super) struct TurnPlacement {
    target: Option<usize>,
    pending: bool,
    interrupted: bool,
    spacer: Pixels,
    viewport: Option<Size<Pixels>>,
    last_offset: Option<Pixels>,
    user_distance: Pixels,
    scrolled: bool,
    release: bool,
}

pub(super) type PlacementHandle = Rc<RefCell<TurnPlacement>>;

impl TurnPlacement {
    pub(super) fn begin(&mut self, index: usize, list: &ListState) {
        *self = Self::default();
        self.target = Some(index);
        self.pending = true;
        self.interrupted = false;
        self.spacer = px(0.);
        self.last_offset = None;
        self.user_distance = px(0.);
        list.set_follow_mode(FollowMode::Normal);
        list.scroll_to(ListOffset {
            item_ix: index,
            offset_in_item: px(0.),
        });
    }

    pub(super) fn reset(&mut self) {
        let release = self.active() || self.release;
        *self = Self {
            release,
            ..Self::default()
        };
    }

    pub(super) fn scrolled(&mut self) {
        self.scrolled = true;
    }

    pub(super) fn active(&self) -> bool {
        self.target.is_some()
    }

    pub(super) fn inserted(&mut self, index: usize) {
        if let Some(target) = &mut self.target
            && index <= *target
        {
            *target += 1;
        }
    }

    pub(super) fn removed(&mut self, index: usize) {
        match self.target {
            Some(target) if target == index => self.reset(),
            Some(target) if target > index => self.target = Some(target - 1),
            _ => {}
        }
    }

    fn interrupt(&mut self, list: &ListState) {
        self.pending = false;
        self.interrupted = true;
        list.pause_following_tail();
    }
}

type RenderRow = dyn FnMut(usize, &mut Window, &mut App) -> AnyElement;

pub(super) struct TurnList {
    list: ListState,
    placement: PlacementHandle,
    foreground: Pixels,
    count: usize,
    render: Rc<RefCell<RenderRow>>,
    style: StyleRefinement,
}

impl TurnList {
    pub(super) fn new(
        list: ListState,
        placement: PlacementHandle,
        foreground: Pixels,
        count: usize,
        render: impl FnMut(usize, &mut Window, &mut App) -> AnyElement + 'static,
    ) -> Self {
        let scrolled = placement.clone();
        list.set_scroll_handler(move |_, _, _| {
            scrolled.borrow_mut().scrolled();
        });
        Self {
            list,
            placement,
            foreground,
            count,
            render: Rc::new(RefCell::new(render)),
            style: StyleRefinement::default(),
        }
    }
}

impl Styled for TurnList {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl IntoElement for TurnList {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for TurnList {
    type RequestLayoutState = ();
    type PrepaintState = AnyElement;

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
        let mut style = Style::default();
        style.refine(&self.style);
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let available = size(
            AvailableSpace::Definite(bounds.size.width),
            AvailableSpace::MinContent,
        );
        let usable = (bounds.size.height - self.foreground).max(px(0.));
        let offset = -self.list.scroll_px_offset_for_scrollbar().y;
        let mut placement = self.placement.borrow_mut();
        if !placement.pending
            && placement
                .viewport
                .is_some_and(|previous| previous != bounds.size)
        {
            placement.reset();
        }
        placement.viewport = Some(bounds.size);
        if let Some(target) = placement.target {
            if target >= self.count {
                placement.reset();
            } else {
                let mut height = px(0.);
                for index in target..self.count {
                    if !placement.pending && placement.spacer == px(0.) {
                        break;
                    }
                    if height >= usable {
                        break;
                    }
                    let mut row = (self.render.borrow_mut())(index, window, cx);
                    height += row.layout_as_root(available, window, cx).height;
                }
                let required = (usable - height).max(px(0.));
                if placement.pending {
                    placement.spacer = required;
                    self.list.set_follow_mode(FollowMode::Normal);
                    self.list.scroll_to(ListOffset {
                        item_ix: target,
                        offset_in_item: px(0.),
                    });
                    placement.pending = false;
                } else {
                    placement.spacer = placement.spacer.min(required);
                }
                if placement.interrupted {
                    if placement.scrolled
                        && let Some(previous) = placement.last_offset
                    {
                        let delta = previous - offset;
                        placement.user_distance += delta.abs();
                        placement.spacer = (placement.spacer - delta.max(px(0.))).max(px(0.));
                    }
                    let bottom = self.list.max_offset_for_scrollbar().y;
                    if placement.user_distance > px(24.) {
                        self.list.pause_following_tail();
                    }
                    if placement.scrolled
                        && self.list.is_scrolled_to_end() == Some(true)
                        && placement.user_distance > px(0.)
                    {
                        placement.reset();
                        placement.release = false;
                        self.list.set_follow_mode(FollowMode::Tail);
                    } else if bottom - offset >= placement.spacer {
                        placement.spacer = px(0.);
                    }
                }
            }
        }
        if placement.release {
            let top = self.list.logical_scroll_top();
            self.list.set_follow_mode(FollowMode::Tail);
            self.list.scroll_to(top);
            placement.release = false;
        }
        placement.scrolled = false;
        let padding = self.foreground + placement.spacer;
        drop(placement);
        let render = self.render.clone();
        let mut body = list(self.list.clone(), move |index, window, cx| {
            (render.borrow_mut())(index, window, cx)
        })
        .w_full()
        .h_full()
        .pb(padding)
        .into_any_element();
        body.layout_as_root(
            size(
                AvailableSpace::Definite(bounds.size.width),
                AvailableSpace::Definite(bounds.size.height),
            ),
            window,
            cx,
        );
        body.prepaint_at(bounds.origin, window, cx);
        self.placement.borrow_mut().last_offset =
            Some(-self.list.scroll_px_offset_for_scrollbar().y);
        body
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        body: &mut AnyElement,
        window: &mut Window,
        cx: &mut App,
    ) {
        let placement = self.placement.clone();
        let list = self.list.clone();
        window.on_mouse_event(move |event: &ScrollWheelEvent, phase, _, _| {
            if phase.capture() && bounds.contains(&event.position) {
                placement.borrow_mut().interrupt(&list);
            }
        });
        let placement = self.placement.clone();
        let list = self.list.clone();
        window.on_mouse_event(move |event: &MouseDownEvent, phase, _, _| {
            if phase.capture() && bounds.contains(&event.position) {
                placement.borrow_mut().interrupt(&list);
            }
        });
        let placement = self.placement.clone();
        let list = self.list.clone();
        window.on_key_event(move |_: &KeyDownEvent, phase, _, _| {
            if phase.capture() {
                placement.borrow_mut().interrupt(&list);
            }
        });
        body.paint(window, cx);
    }
}
