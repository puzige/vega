use std::rc::Rc;

use gpui_kit::component::menu::PopupMenu;
use gpui_kit::{
    Anchor, AnyElement, App, Context, DismissEvent, Entity, FocusHandle, Focusable, IntoElement,
    MouseButton, Pixels, Point, RenderOnce, Subscription, Window, anchored, deferred, div,
    prelude::*, px,
};

type MenuBuilder = dyn Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu;

#[derive(Default)]
struct MenuState {
    menu: Option<Entity<PopupMenu>>,
    previous_focus: Option<FocusHandle>,
    position: Point<Pixels>,
    subscription: Option<Subscription>,
}

impl MenuState {
    fn clear(&mut self) {
        self.menu = None;
        self.previous_focus = None;
        self.subscription = None;
    }
}

#[derive(IntoElement)]
pub(super) struct SelectionMenu {
    id: u64,
    body: AnyElement,
    builder: Rc<MenuBuilder>,
}

impl SelectionMenu {
    pub(super) fn new(
        id: u64,
        body: AnyElement,
        builder: impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static,
    ) -> Self {
        Self {
            id,
            body,
            builder: Rc::new(builder),
        }
    }
}

impl RenderOnce for SelectionMenu {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = window.use_keyed_state(("selection-menu-state", self.id), cx, |_, _| {
            MenuState::default()
        });
        let popup = state.read(cx).menu.clone().map(|menu| {
            let position = state.read(cx).position;
            let focus = menu.focus_handle(cx);
            if !focus.contains_focused(window, cx) {
                focus.focus(window, cx);
            }
            deferred(
                anchored().child(
                    div()
                        .w(window.bounds().size.width)
                        .h(window.bounds().size.height)
                        .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                        .child(
                            anchored()
                                .position(position)
                                .snap_to_window_with_margin(px(8.))
                                .anchor(Anchor::TopLeft)
                                .child(menu),
                        ),
                ),
            )
            .with_priority(gpui_kit::base::POPUP_PRIORITY)
        });
        let open_state = state.downgrade();
        let builder = self.builder;
        div()
            .id(("message-selection-context-menu", self.id))
            .child(self.body)
            .children(popup)
            .on_mouse_down(MouseButton::Right, move |event, window, cx| {
                let Some(state) = open_state.upgrade() else {
                    return;
                };
                let previous_focus = window.focused(cx).and_then(|focus| {
                    let state = state.read(cx);
                    if state
                        .menu
                        .as_ref()
                        .is_some_and(|menu| menu.focus_handle(cx) == focus)
                    {
                        state.previous_focus.clone()
                    } else {
                        Some(focus)
                    }
                });
                state.update(cx, |state, _| state.clear());
                let open_state = state.downgrade();
                let builder = builder.clone();
                let position = event.position;
                window.defer(cx, move |window, cx| {
                    let Some(state) = open_state.upgrade() else {
                        return;
                    };
                    let menu = PopupMenu::build(window, cx, move |menu, window, cx| {
                        builder(menu, window, cx)
                    });
                    let dismiss_state = state.downgrade();
                    let subscription =
                        window.subscribe(&menu, cx, move |menu, _: &DismissEvent, window, cx| {
                            let restore_focus = window.focused(cx).is_none()
                                || menu.focus_handle(cx).contains_focused(window, cx);
                            let _ = dismiss_state.update(cx, |state, cx| {
                                if restore_focus && let Some(focus) = &state.previous_focus {
                                    focus.focus(window, cx);
                                }
                                state.clear();
                                cx.notify();
                            });
                            window.refresh();
                        });
                    state.update(cx, |state, cx| {
                        state.menu = Some(menu);
                        state.previous_focus = previous_focus;
                        state.position = position;
                        state.subscription = Some(subscription);
                        cx.notify();
                    });
                    window.refresh();
                });
            })
    }
}
