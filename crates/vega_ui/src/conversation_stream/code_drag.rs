use std::{
    cell::RefCell,
    rc::{Rc, Weak},
    time::Duration,
};

use gpui_kit::base::{TextSelectionEvent, TextSelectionHandle};
use gpui_kit::{
    AnyElement, App, AppContext, Bounds, Element, ElementId, Entity, FocusHandle, GlobalElementId,
    InspectorElementId, IntoElement, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, PlatformInput, Point, Subscription, Task, Window, WindowId, px,
};

use super::{code_viewport::CodeScrollHandle, model::MessageCopy};

const INTERVAL: Duration = Duration::from_millis(16);

#[derive(Clone)]
pub(super) struct CodeDragSelection {
    copy: MessageCopy,
    focus: FocusHandle,
    handle: TextSelectionHandle,
    hit_text: Rc<dyn Fn(Point<Pixels>) -> Option<u64>>,
}

impl CodeDragSelection {
    pub(super) fn new(
        copy: MessageCopy,
        focus: FocusHandle,
        handle: TextSelectionHandle,
        hit_text: impl Fn(Point<Pixels>) -> Option<u64> + 'static,
    ) -> Self {
        Self {
            copy,
            focus,
            handle,
            hit_text: Rc::new(hit_text),
        }
    }
}

#[derive(Default)]
struct RefreshEpoch {
    value: u64,
    pending: Option<u64>,
}

impl RefreshEpoch {
    fn invalidate(&mut self) {
        self.value = self.value.wrapping_add(1);
        self.pending = None;
    }

    fn queue(&mut self) -> u64 {
        self.value = self.value.wrapping_add(1);
        self.pending = Some(self.value);
        self.value
    }

    fn take(&mut self, token: u64) -> bool {
        if self.pending == Some(token) && self.value == token {
            self.pending = None;
            true
        } else {
            false
        }
    }
}

struct DragOrigin {
    anchor: u64,
    content_generation: u64,
    content: Option<String>,
}

pub(super) struct CodeDragController {
    selection: CodeDragSelection,
    scroll: CodeScrollHandle,
    window: WindowId,
    visible: Bounds<Pixels>,
    mounted: bool,
    origin: Option<DragOrigin>,
    pointer: Point<Pixels>,
    epoch: RefreshEpoch,
    timer: Option<Task<()>>,
    synthetic: bool,
    focus_out: Option<Subscription>,
    selection_changed: Option<Subscription>,
    activation: Option<Entity<()>>,
}

impl CodeDragController {
    fn pause(&mut self) {
        self.epoch.invalidate();
        self.timer = None;
    }

    fn cancel(&mut self) {
        self.pause();
        self.origin = None;
        self.focus_out = None;
    }

    fn valid_origin(&self, window: &Window, cx: &App) -> bool {
        let Some(origin) = &self.origin else {
            return false;
        };
        self.mounted
            && self.window == window.window_handle().window_id()
            && window.is_window_active()
            && !cx.has_active_drag()
            && window.mouse_position() == self.pointer
            && self.selection.focus.contains_focused(window, cx)
            && self.selection.copy.content_generation() == origin.content_generation
            && self.selection.copy.visible_text() == origin.content
            && !self.visible.is_empty()
    }

    fn valid(&self, window: &Window, cx: &App) -> bool {
        self.valid_origin(window, cx)
            && self.origin.as_ref().is_some_and(|origin| {
                self.selection.handle.snapshot(cx).is_some_and(|snapshot| {
                    snapshot.is_selecting()
                        && snapshot.anchor().entity_id() == Some(self.selection.handle.entity_id())
                        && snapshot.anchor().content_key().map(|key| key.value())
                            == Some(origin.anchor)
                        && snapshot.cursor().entity_id() == Some(self.selection.handle.entity_id())
                })
            })
    }

    fn delta(&self) -> Pixels {
        if self.visible.is_empty()
            || self.pointer.y < self.visible.top()
            || self.pointer.y >= self.visible.bottom()
        {
            return px(0.);
        }
        let edge = px(24.).min(self.visible.size.width / 3.);
        if self.pointer.x >= self.visible.right() - edge {
            -px(12.)
        } else if self.pointer.x <= self.visible.left() + edge {
            px(12.)
        } else {
            px(0.)
        }
    }

    fn next_x(&self) -> Pixels {
        let max = self.scroll.max_offset().x.max(px(0.));
        (self.scroll.offset().x + self.delta()).clamp(-max, px(0.))
    }

    fn advance(&mut self, window: &Window, cx: &App) -> Option<u64> {
        if !self.valid(window, cx) {
            self.cancel();
            return None;
        }
        let mut offset = self.scroll.offset();
        let next = self.next_x();
        if next == offset.x {
            return None;
        }
        offset.x = next;
        self.scroll.set_offset(offset);
        Some(self.epoch.queue())
    }
}

struct MountLease(Rc<RefCell<CodeDragController>>);

impl Drop for MountLease {
    fn drop(&mut self) {
        let mut state = self.0.borrow_mut();
        state.mounted = false;
        state.cancel();
    }
}

pub(super) struct CodeDragElement {
    id: ElementId,
    body: AnyElement,
    selection: CodeDragSelection,
    scroll: CodeScrollHandle,
}

impl CodeDragElement {
    pub(super) fn new(
        id: ElementId,
        body: AnyElement,
        selection: CodeDragSelection,
        scroll: CodeScrollHandle,
    ) -> Self {
        Self {
            id,
            body,
            selection,
            scroll,
        }
    }
}

impl IntoElement for CodeDragElement {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for CodeDragElement {
    type RequestLayoutState = ();
    type PrepaintState = Rc<RefCell<CodeDragController>>;

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
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
        id: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let visible = bounds.intersect(&window.content_mask().bounds);
        let state = if let Some(id) = id {
            window.with_element_state(id, |lease: Option<MountLease>, window| {
                let lease = lease.unwrap_or_else(|| {
                    MountLease(Rc::new(RefCell::new(CodeDragController {
                        selection: self.selection.clone(),
                        scroll: self.scroll.clone(),
                        window: window.window_handle().window_id(),
                        visible,
                        mounted: true,
                        origin: None,
                        pointer: Point::default(),
                        epoch: RefreshEpoch::default(),
                        timer: None,
                        synthetic: false,
                        focus_out: None,
                        selection_changed: None,
                        activation: None,
                    })))
                });
                (lease.0.clone(), lease)
            })
        } else {
            Rc::new(RefCell::new(CodeDragController {
                selection: self.selection.clone(),
                scroll: self.scroll.clone(),
                window: window.window_handle().window_id(),
                visible: Bounds::default(),
                mounted: false,
                origin: None,
                pointer: Point::default(),
                epoch: RefreshEpoch::default(),
                timer: None,
                synthetic: false,
                focus_out: None,
                selection_changed: None,
                activation: None,
            }))
        };
        {
            let mut controller = state.borrow_mut();
            if !controller.scroll.same_binding(&self.scroll)
                || controller.selection.handle.entity_id() != self.selection.handle.entity_id()
                || controller.selection.copy.id != self.selection.copy.id
            {
                controller.cancel();
                controller.selection_changed = None;
            }
            controller.scroll = self.scroll.clone();
            controller.selection = self.selection.clone();
            controller.visible = visible;
            controller.mounted = id.is_some();
            if visible.is_empty()
                || !window.is_window_active()
                || cx.has_active_drag()
                || controller.origin.as_ref().is_some_and(|origin| {
                    controller.selection.copy.content_generation() != origin.content_generation
                        || controller.selection.copy.visible_text() != origin.content
                })
            {
                controller.cancel();
            }
        }
        if state.borrow().selection_changed.is_none() {
            let weak = Rc::downgrade(&state);
            let subscription = self.selection.handle.subscribe(move |event, _| {
                if matches!(event, TextSelectionEvent::Cleared)
                    || matches!(event, TextSelectionEvent::SelectionChanged(Some(snapshot)) if !snapshot.is_selecting())
                {
                    if let Some(state) = weak.upgrade() {
                        let mut state = state.borrow_mut();
                        if state.origin.is_some() { state.cancel(); }
                    }
                }
            }, cx);
            state.borrow_mut().selection_changed = Some(subscription);
        }
        if state.borrow().activation.is_none() {
            let weak = Rc::downgrade(&state);
            let observer = cx.new(|cx| {
                cx.observe_window_activation(window, move |_, window, _| {
                    if !window.is_window_active()
                        && let Some(state) = weak.upgrade()
                    {
                        state.borrow_mut().cancel();
                    }
                })
                .detach();
            });
            state.borrow_mut().activation = Some(observer);
        }
        self.body.prepaint(window, cx);
        state
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        state: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.body.paint(window, cx);
        #[cfg(test)]
        super::code_drag_tests::observe_driver(
            window.window_handle().window_id(),
            self.id.clone(),
            Rc::downgrade(state),
            cx,
        );
        let down = state.clone();
        window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
            if !phase.bubble() {
                return;
            }
            let mut state = down.borrow_mut();
            state.cancel();
            if event.button != MouseButton::Left
                || event.click_count != 1
                || !window.is_window_active()
                || cx.has_active_drag()
                || !state.visible.contains(&event.position)
            {
                return;
            }
            let Some(anchor) = (state.selection.hit_text)(event.position) else {
                return;
            };
            let origin = DragOrigin {
                anchor,
                content_generation: state.selection.copy.content_generation(),
                content: state.selection.copy.visible_text(),
            };
            state.pointer = event.position;
            let token = state.epoch.value;
            let weak = Rc::downgrade(&down);
            window.defer(cx, move |window, cx| {
                window.defer(cx, move |window, cx| {
                    let Some(controller) = weak.upgrade() else {
                        return;
                    };
                    let mut state = controller.borrow_mut();
                    if state.epoch.value != token
                        || !state.mounted
                        || !window.is_window_active()
                        || cx.has_active_drag()
                        || !state.selection.focus.contains_focused(window, cx)
                        || state.selection.copy.content_generation() != origin.content_generation
                        || state.selection.copy.visible_text() != origin.content
                    {
                        return;
                    }
                    state.origin = Some(origin);
                    let focus = state.selection.focus.clone();
                    state.focus_out = Some(window.on_focus_out(&focus, cx, move |_, _, _| {
                        if let Some(state) = weak.upgrade() {
                            state.borrow_mut().cancel();
                        }
                    }));
                });
            });
        });
        let up = state.clone();
        window.on_mouse_event(move |_: &MouseUpEvent, phase, _, _| {
            if phase.capture() {
                up.borrow_mut().cancel();
            }
        });
        let moved = state.clone();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
            if !phase.capture() || moved.borrow().synthetic {
                return;
            }
            {
                let mut state = moved.borrow_mut();
                if event.pressed_button != Some(MouseButton::Left) || cx.has_active_drag() {
                    state.cancel();
                    return;
                }
                state.pause();
                state.pointer = event.position;
                if state.origin.is_none() {
                    return;
                }
            }
            let weak = Rc::downgrade(&moved);
            window.defer(cx, move |window, cx| schedule(&weak, window, cx));
        });
        let pending = state.borrow().epoch.pending;
        if let Some(token) = pending {
            let weak = Rc::downgrade(state);
            window.defer(cx, move |window, cx| refresh(&weak, token, window, cx));
        }
    }
}

fn schedule(weak: &Weak<RefCell<CodeDragController>>, window: &mut Window, cx: &mut App) {
    let Some(state) = weak.upgrade() else {
        return;
    };
    let mut controller = state.borrow_mut();
    if !controller.valid_origin(window, cx) {
        controller.cancel();
        return;
    }
    if controller.selection.handle.snapshot(cx).is_none() {
        return;
    }
    if !controller.valid(window, cx) {
        controller.cancel();
        return;
    }
    if controller.timer.is_some()
        || controller.epoch.pending.is_some()
        || controller.delta() == px(0.)
        || controller.next_x() == controller.scroll.offset().x
    {
        return;
    }
    let token = controller.epoch.value;
    let weak = weak.clone();
    controller.timer = Some(window.spawn(cx, async move |cx| {
        cx.background_executor().timer(INTERVAL).await;
        let _ = cx.update(|window, cx| {
            let Some(state) = weak.upgrade() else {
                return;
            };
            let mut controller = state.borrow_mut();
            if controller.epoch.value != token {
                return;
            }
            controller.timer = None;
            if controller.advance(window, cx).is_some() {
                window.refresh();
            }
        });
    }));
}

fn refresh(
    weak: &Weak<RefCell<CodeDragController>>,
    token: u64,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(state) = weak.upgrade() else {
        return;
    };
    let event = {
        let mut controller = state.borrow_mut();
        if !controller.epoch.take(token) {
            return;
        }
        if !controller.valid(window, cx) || controller.delta() == px(0.) {
            controller.cancel();
            return;
        }
        controller.synthetic = true;
        MouseMoveEvent {
            position: window.mouse_position(),
            pressed_button: Some(MouseButton::Left),
            modifiers: window.modifiers(),
        }
    };
    window.dispatch_event(PlatformInput::MouseMove(event), cx);
    state.borrow_mut().synthetic = false;
    schedule(weak, window, cx);
}

#[cfg(test)]
pub(super) fn verify_pending_refresh_cancellation(
    state: &Rc<RefCell<CodeDragController>>,
    replace: bool,
    window: &mut Window,
    cx: &mut App,
) {
    let (token, handle, scroll, selection) = {
        let mut controller = state.borrow_mut();
        assert!(controller.valid(window, cx));
        controller.timer = None;
        let before = controller.scroll.offset();
        let token = controller
            .advance(window, cx)
            .expect("production horizontal advance");
        assert!(controller.scroll.offset().x < before.x);
        assert_eq!(controller.scroll.offset().y, before.y);
        (
            token,
            controller.selection.handle.clone(),
            controller.scroll.clone(),
            controller.selection.clone(),
        )
    };
    if replace {
        selection.copy.replace_text("replacement🚀");
    } else {
        gpui_kit::base::TextSelection::end(window, cx);
        state.borrow_mut().cancel();
    }
    let stopped = scroll.offset();
    let snapshot = handle.snapshot(cx);
    refresh(&Rc::downgrade(state), token, window, cx);
    assert_eq!(scroll.offset(), stopped);
    assert_eq!(handle.snapshot(cx), snapshot);
    assert!(state.borrow().origin.is_none());
    assert!(state.borrow().timer.is_none());
    assert!(!state.borrow().synthetic);
}

#[cfg(test)]
mod tests {
    use super::RefreshEpoch;

    #[test]
    fn invalidating_pending_refresh_rejects_it_without_consuming_the_next_drag() {
        let mut epoch = RefreshEpoch::default();
        let pending = epoch.queue();
        epoch.invalidate();
        let replacement = epoch.queue();
        assert!(!epoch.take(pending));
        assert!(epoch.take(replacement));
        assert!(!epoch.take(replacement));
    }

    #[test]
    fn duplicate_layout_callback_cannot_consume_the_next_scroll_refresh() {
        let mut epoch = RefreshEpoch::default();
        let first = epoch.queue();
        assert!(epoch.take(first));
        let second = epoch.queue();
        assert_ne!(first, second);
        assert!(!epoch.take(first));
        assert!(epoch.take(second));
    }
}
