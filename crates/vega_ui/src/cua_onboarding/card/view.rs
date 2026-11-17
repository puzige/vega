use super::GuidanceCard;
use super::nsh_view::{self, Action, PreviewSnapshot, Snapshot};
use crate::cua_onboarding::controller::CardAction;
use crate::nsh::Surface;
use gpui_kit::prelude::*;
use gpui_kit::{
    App, Context, DispatchPhase, ExternalDragPayload, FileDragPaths, KeyDownEvent, MouseButton,
    MouseDownEvent, MouseUpEvent, Render, Role, Window, canvas, div, px,
};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use vega_theme::Layout;

#[derive(Clone)]
struct AppDrag {
    bundle: Rc<RefCell<Option<PathBuf>>>,
    observed: Snapshot,
}

struct DragPreview {
    _gesture: Rc<()>,
    surface: Surface<PreviewSnapshot, ()>,
    icon: nsh_core::element::ImageSrc,
}

impl Render for DragPreview {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.surface.sync_environment(cx);
        self.surface.publish(PreviewSnapshot {
            icon: self.icon.clone(),
            colors: vega_theme::theme(cx).colors,
        });
        div()
            .h(px(Layout::PREPARATION_APP_TARGET_HEIGHT))
            .w(px(Layout::PREPARATION_GUIDE_CONTROL * 3.))
            .child(self.surface.entity())
    }
}

impl GuidanceCard {
    fn native_drag_host(&self, cx: &mut Context<Self>) -> Option<gpui_kit::AnyElement> {
        let observed = self.observed_drag(cx)?;
        let bounds = self.presentation.as_ref()?.drag_bounds?;
        let drag = AppDrag {
            bundle: Rc::new(RefCell::new(None)),
            observed: observed.clone(),
        };
        let weak = cx.weak_entity();
        let payload_owner = weak.clone();
        let app_icon = self.app_icon.clone();
        Some(
            div()
                .id("preparation-native-drag-host")
                .absolute()
                .left(px(Layout::PREPARATION_CARD_SHADOW_PADDING + bounds.x))
                .top(px(Layout::PREPARATION_CARD_SHADOW_PADDING + bounds.y))
                .w(px(bounds.width))
                .h(px(bounds.height))
                .cursor_pointer()
                .on_click(cx.listener(move |card, _, window, cx| {
                    card.nsh_action(
                        &observed,
                        Action::Controller(CardAction::RevealInFinder),
                        window,
                        cx,
                    );
                }))
                .on_drag(drag, move |drag, _, window, cx| {
                    let gesture = Rc::new(());
                    let bundle = weak
                        .update(cx, |card, cx| {
                            if !card.accepts_drag(&drag.observed, cx) {
                                return None;
                            }
                            let bundle = card.controller.begin_drag();
                            if bundle.is_some() {
                                card.drag_gesture = Some(Rc::downgrade(&gesture));
                            }
                            let bundle = bundle.filter(|path| {
                                if path.exists() {
                                    true
                                } else {
                                    card.path_error =
                                        Some(crate::localized_copy::LocalizedCopy::new(
                                            "ui.this_copy_of_vega_no_longer_exists_at",
                                        ));
                                    false
                                }
                            });
                            cx.notify();
                            bundle
                        })
                        .ok()
                        .flatten();
                    *drag.bundle.borrow_mut() = bundle;
                    cx.new(|cx| {
                        let (surface, _) = Surface::new(
                            PreviewSnapshot {
                                icon: app_icon.clone(),
                                colors: vega_theme::theme(cx).colors,
                            },
                            nsh_view::render_preview,
                            window,
                            cx,
                        );
                        nsh_view::transparent(&surface, cx);
                        DragPreview {
                            _gesture: gesture,
                            surface,
                            icon: app_icon.clone(),
                        }
                    })
                })
                .external_drag_payload(move |drag: &AppDrag, _, cx: &mut App| {
                    if !payload_owner
                        .read_with(cx, |card, cx| card.accepts_drag(&drag.observed, cx))
                        .unwrap_or(false)
                    {
                        return None;
                    }
                    let bundle = drag.bundle.borrow().clone()?;
                    if !bundle.exists() {
                        return None;
                    }
                    Some(ExternalDragPayload::Files(FileDragPaths::new([(
                        bundle, true,
                    )])))
                })
                .into_any_element(),
        )
    }
}

impl Render for GuidanceCard {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let surface = self.render_nsh(window, cx);
        let host = self.native_drag_host(cx);
        let accessibility = self.keyboard_accessibility_host(window, cx);
        let weak = cx.weak_entity();
        let pointer_events = canvas(
            |_, _, _| (),
            move |bounds, _, window, _| {
                let down = weak.clone();
                window.on_mouse_event(move |event: &MouseDownEvent, phase, _, cx| {
                    if phase == DispatchPhase::Capture
                        && event.button == MouseButton::Left
                        && bounds.contains(&event.position)
                    {
                        let _ = down.update(cx, |card, cx| card.keyboard_pointer_down(cx));
                    }
                });
                window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
                    if phase == DispatchPhase::Capture
                        && event.button == MouseButton::Left
                        && bounds.contains(&event.position)
                    {
                        let _ = weak.update(cx, |card, cx| {
                            window.focus(&card.focus, cx);
                            cx.notify();
                        });
                    }
                });
            },
        )
        .absolute()
        .size_full();
        div()
            .id("preparation-card-window")
            .size_full()
            .relative()
            .track_focus(&self.focus)
            .role(Role::Group)
            .capture_key_down(cx.listener(|card, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    card.dispatch(CardAction::Close, window, cx);
                    cx.stop_propagation();
                } else if card.keyboard_key(event, window, cx) {
                    cx.stop_propagation();
                }
            }))
            .child(pointer_events)
            .children(surface)
            .children(host)
            .children(accessibility)
    }
}
