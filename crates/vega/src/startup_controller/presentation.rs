use gpui_kit::{prelude::*, *};
use vega_theme::theme;
use vega_ui::nsh::{ProjectionId, Surface};

use super::{
    StartupView,
    model::{Action, Authority, Frame, Input, Key, RenderFence, Request, Snapshot, key, traverse},
    view,
};

pub(super) struct Presentation {
    pub(super) surface: Surface<Snapshot, Request>,
    pub(super) mount: ProjectionId,
    pub(super) recovery: bool,
    reported_failure: bool,
    fence: RenderFence,
}

impl StartupView {
    fn mount_presentation(&mut self, recovery: bool, window: &mut Window, cx: &mut Context<Self>) {
        let mount = ProjectionId::new();
        let mut initial = Snapshot {
            frame: ProjectionId::new(),
            authority: Authority {
                session: self.session.clone(),
                projection: self.projection.clone(),
                mount: mount.clone(),
                running: self.running,
                error: self.error,
                recovery,
            },
            colors: theme(cx).colors,
            selected: self.selected,
            phase: None,
            language: vega_i18n::language(),
            appearance: theme(cx).appearance,
        };
        let mut fence = RenderFence::default();
        fence.publish(&mut initial);
        let (surface, mut requests) = Surface::new(initial, view::render, window, cx);
        let mut errors = surface.errors();
        cx.spawn(async move |this, cx| {
            while errors.changed().await.is_ok() {
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
            }
        })
        .detach();
        cx.spawn_in(window, async move |this, cx| {
            while let Some(request) = requests.recv().await {
                if this
                    .update_in(cx, |this, window, cx| match request {
                        Request::Rendered(frame) => {
                            if this.is_current_root(window, cx)
                                && let Some(page) = &mut this.presentation
                                && page.fence.acknowledge(frame)
                            {
                                cx.notify();
                            }
                        }
                        Request::Activate { observed, action } => {
                            this.dispatch(&observed, action, Input::Pointer, window, cx);
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        self.presentation = Some(Presentation {
            surface,
            mount,
            recovery,
            reported_failure: false,
            fence,
        });
        if recovery {
            self.selected = self
                .authority()
                .and_then(|state| state.controls().first().copied());
            window.focus(&self.focus, cx);
        }
    }

    fn dispatch(
        &mut self,
        observed: &Frame,
        action: Action,
        input: Input,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.is_current_root(window, cx) {
            return false;
        }
        let Some(current) = self.snapshot(window, cx) else {
            return false;
        };
        let Some(page) = &mut self.presentation else {
            return false;
        };
        let failed = page.surface.errors().borrow().is_some();
        if !page.fence.claim(observed, &current, action, input, failed) {
            return false;
        }
        self.selected = Some(action);
        window.focus(&self.focus, cx);
        match action {
            Action::Retry => self.start(window, cx),
            Action::Quit => cx.quit(),
            Action::Reload => {
                self.presentation = None;
                self.selected = self.error.map(|_| Action::Retry);
            }
        }
        cx.notify();
        true
    }

    fn host_key(
        &mut self,
        observed: &Frame,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.is_current_root(window, cx) || !self.focus.contains_focused(window, cx) {
            return;
        }
        let Some(key) = key(event) else {
            return;
        };
        let Some(current) = self.authority() else {
            return;
        };
        match key {
            Key::Traverse { backwards } => {
                self.selected = traverse(self.selected, &current.controls(), backwards);
                window.focus(&self.focus, cx);
                cx.notify();
            }
            Key::Activate => {
                if let Some(action) = observed.selected
                    && self.selected == Some(action)
                {
                    self.dispatch(observed, action, Input::Keyboard, window, cx);
                }
            }
        }
        cx.stop_propagation();
    }

    pub(super) fn render_presentation(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.presentation.is_none() {
            self.mount_presentation(false, window, cx);
        }
        let failed = self
            .presentation
            .as_ref()
            .is_some_and(|page| !page.recovery && page.surface.errors().borrow().is_some());
        if failed {
            tracing::error!("startup NSH presentation failed; mounting UI-only recovery");
            self.mount_presentation(true, window, cx);
        }
        if let Some(page) = &mut self.presentation
            && page.recovery
            && !page.reported_failure
            && page.surface.errors().borrow().is_some()
        {
            page.reported_failure = true;
            tracing::error!(
                "startup NSH recovery engine failed; NSH-only visual recovery unavailable; native keyboard can still reload the view"
            );
        }
        let Some(mut snapshot) = self.snapshot(window, cx) else {
            return div().into_any_element();
        };
        if self.moving(window, cx) && !self.animation_pending {
            self.animation_pending = true;
            cx.on_next_frame(window, |this, window, cx| {
                this.animation_pending = false;
                if this.moving(window, cx) {
                    cx.notify();
                }
            });
        }
        let Some(page) = &mut self.presentation else {
            return div().into_any_element();
        };
        page.surface.sync_environment(cx);
        page.fence.publish(&mut snapshot);
        page.surface.publish(snapshot.clone());
        let keyboard_observed = snapshot.rendered_frame();
        let mut host = div()
            .id("startup-nsh-host")
            .relative()
            .size_full()
            .track_focus(&self.focus)
            .capture_key_down(cx.listener(move |this, event, window, cx| {
                this.host_key(&keyboard_observed, event, window, cx);
            }))
            .capture_any_mouse_down(cx.listener(|this, _: &MouseDownEvent, window, cx| {
                window.focus(&this.focus, cx);
            }))
            .child(page.surface.entity());
        if let Some(action) = snapshot.selected
            && snapshot.authority.controls().contains(&action)
        {
            let observed = snapshot.rendered_frame();
            let weak = cx.weak_entity();
            host = host.child(
                div()
                    .id("startup-nsh-active-control")
                    .absolute()
                    .inset_0()
                    .role(Role::Button)
                    .aria_label(action.label())
                    .aria_active_descendant()
                    .on_a11y_action(AccessibleAction::Click, move |_, window, cx| {
                        let _ = weak.update(cx, |this, cx| {
                            if this.focus.is_focused(window) && this.selected == Some(action) {
                                this.dispatch(&observed, action, Input::Accessibility, window, cx);
                            }
                        });
                    }),
            );
        }
        host.into_any_element()
    }
}
