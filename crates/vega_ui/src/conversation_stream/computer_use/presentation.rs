use super::{ComputerUseControl, ConversationStream, TaskContext};
use crate::nsh::keyboard::{Key, Selection, Step};
use crate::nsh::{ProjectionId, Surface};
use gpui_kit::{prelude::*, *};
use std::cell::Cell;
use std::rc::{Rc, Weak};
use vega_conversation::types::{Capability, ControlPhase, PermissionMode, Target};
use vega_theme::{Layout, ThemeColors, Typography, theme};

mod mounted;
mod pixels;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    TakeOver,
    AllowBrowser,
    Resume,
}

impl Action {
    fn label(self) -> &'static str {
        vega_i18n::t(match self {
            Self::TakeOver => "ui.take_over_computer_control",
            Self::AllowBrowser => "ui.allow_this_tab",
            Self::Resume => "ui.continue",
        })
    }
}

#[derive(Clone)]
struct Source {
    presentation_epoch: ProjectionId,
    control: ComputerUseControl,
    task: TaskContext,
    thread: String,
    phase: ControlPhase,
    epoch: u64,
    capability: Option<Capability>,
    target: Option<Target>,
    target_label: String,
    generation: u64,
    active_message: Option<(String, usize)>,
    permission: PermissionMode,
    language: &'static str,
}

impl Source {
    fn capture(stream: &ConversationStream) -> Option<Self> {
        let control = stream.computer_use_control.as_ref()?;
        let status = control.status();
        if !matches!(
            status.phase,
            ControlPhase::Running
                | ControlPhase::AwaitingApproval
                | ControlPhase::Paused
                | ControlPhase::Pausing
        ) {
            return None;
        }
        Some(Self {
            presentation_epoch: stream.run_notice_epoch.clone(),
            control: control.clone(),
            task: stream.computer_use_task.clone(),
            thread: stream.thread.id.clone(),
            phase: status.phase,
            epoch: status.epoch,
            capability: status.capability,
            target: status.browser_target,
            target_label: status.target_label,
            generation: stream.actions.control_generation,
            active_message: stream.active_agent_message.clone(),
            permission: stream.thread.permission_mode,
            language: vega_i18n::language(),
        })
    }

    fn matches(&self, other: &Self) -> bool {
        self.presentation_epoch.matches(&other.presentation_epoch)
            && self.control.same_instance(&other.control)
            && self.task == other.task
            && self.thread == other.thread
            && self.phase == other.phase
            && self.epoch == other.epoch
            && self.capability == other.capability
            && self.target == other.target
            && self.target_label == other.target_label
            && self.generation == other.generation
            && self.active_message == other.active_message
            && self.permission == other.permission
            && self.language == other.language
    }

    fn current(&self, stream: &ConversationStream, cx: &App) -> bool {
        !stream.reading_preview
            && !stream.permissions_revoked
            && !stream.permission_change_pending()
            && !stream.entries.is_empty()
            && stream.owns_computer_use_control(&self.task, &self.control)
            && cx
                .try_global::<crate::settings::SettingsOpen>()
                .is_some_and(|settings| !settings.0)
            && cx
                .try_global::<crate::sidebar::OpenedThread>()
                .is_some_and(|route| {
                    route
                        .0
                        .as_ref()
                        .is_some_and(|thread| thread.id == self.thread)
                })
            && Self::capture(stream).is_some_and(|current| self.matches(&current))
    }

    fn action(&self) -> Option<Action> {
        match self.phase {
            ControlPhase::Running if self.active_message.is_some() => Some(Action::TakeOver),
            ControlPhase::AwaitingApproval
                if self.capability == Some(Capability::Browser) && self.target.is_some() =>
            {
                Some(Action::AllowBrowser)
            }
            ControlPhase::Paused => Some(Action::Resume),
            _ => None,
        }
    }
}

#[derive(Clone)]
struct Snapshot {
    id: ProjectionId,
    source: Source,
    colors: ThemeColors,
    text: Option<String>,
    target: Option<String>,
    label: Option<String>,
    enabled: bool,
    selected: bool,
}

impl Snapshot {
    fn new(source: Source, colors: ThemeColors, enabled: bool) -> Self {
        let text = match source.phase {
            ControlPhase::AwaitingApproval if source.target.is_some() => {
                Some(vega_i18n::t("ui.confirm_that_this_task_may_access_this_browser").to_owned())
            }
            ControlPhase::Paused => {
                Some(vega_i18n::t("ui.computer_control_is_paused_resume_this_task_when").to_owned())
            }
            ControlPhase::Pausing => Some(vega_i18n::t("ui.returning_control").to_owned()),
            _ => None,
        };
        let target = if source.phase == ControlPhase::AwaitingApproval {
            source.target.as_ref().map(Target::label)
        } else {
            None
        };
        let label = source.action().map(|action| action.label().to_owned());
        Self {
            id: ProjectionId::new(),
            source,
            colors,
            text,
            target,
            label,
            enabled,
            selected: false,
        }
    }

    fn actions(&self) -> Vec<Action> {
        if self.enabled {
            self.source.action().into_iter().collect()
        } else {
            Vec::new()
        }
    }

    fn estimated_height(&self) -> f32 {
        let lines = usize::from(self.text.is_some())
            + usize::from(self.target.is_some())
            + usize::from(self.label.is_some());
        2. * Layout::STATUS_PADDING
            + lines as f32 * Typography::BODY * Typography::BODY_LINE_HEIGHT
            + lines.saturating_sub(1) as f32 * Layout::STATUS_GAP
    }
}

enum Request {
    Activate {
        observed: Box<Snapshot>,
        action: Action,
    },
    Layout {
        id: ProjectionId,
        width: f32,
        height: f32,
    },
}

#[derive(Clone, Copy)]
struct Geometry {
    visible: Bounds<Pixels>,
    bounds: Bounds<Pixels>,
    viewport: Size<Pixels>,
    list: Bounds<Pixels>,
    foreground: Pixels,
}

#[derive(Default)]
struct Mount {
    geometry: Cell<Option<Geometry>>,
}

struct NoticeHost {
    owner: WeakEntity<ConversationStream>,
    snapshot: Snapshot,
    surface: Option<Surface<Snapshot, Request>>,
    focus: FocusHandle,
    selected: Option<Selection<Action>>,
    lease: Weak<Mount>,
    measured: Option<(f32, f32)>,
    consumed: bool,
    _subscriptions: Vec<Subscription>,
}

impl NoticeHost {
    fn stop(&mut self, cx: &App) {
        if let Some(surface) = self.surface.take() {
            surface.entity().read(cx).stop();
        }
        self.snapshot.id = ProjectionId::new();
        self.selected = None;
        self.measured = None;
        self.consumed = false;
        self.lease = Weak::new();
    }

    fn activate(
        &mut self,
        observed: &Snapshot,
        action: Action,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.consumed
            || !self.snapshot.id.matches(&observed.id)
            || !self.snapshot.source.matches(&observed.source)
            || !self.snapshot.actions().contains(&action)
            || !observed.actions().contains(&action)
            || self
                .surface
                .as_ref()
                .is_none_or(|surface| surface.errors().borrow().is_some())
            || !window.is_window_active()
        {
            return;
        }
        let Some(geometry) = self.lease.upgrade().and_then(|mount| mount.geometry.get()) else {
            return;
        };
        if geometry.viewport != window.viewport_size()
            || geometry.visible.is_empty()
            || self.measured.is_none_or(|(width, height)| {
                (width - f32::from(geometry.bounds.size.width)).abs() > 0.5
                    || (height - f32::from(geometry.bounds.size.height)).abs() > 0.5
            })
        {
            return;
        }
        let Some(owner) = self.owner.upgrade() else {
            self.stop(cx);
            return;
        };
        let accepted = owner.update(cx, |stream, cx| {
            if !observed.source.current(stream, cx)
                || geometry.list != stream.list.viewport_bounds()
                || geometry.foreground != stream.foreground_height
            {
                return false;
            }
            let source = &observed.source;
            let result = match action {
                Action::TakeOver => source.control.pause_at(source.epoch),
                Action::AllowBrowser => {
                    let Some(target) = &source.target else {
                        return false;
                    };
                    source.control.approve_browser(source.epoch, target)
                }
                Action::Resume => source.control.resume(source.epoch),
            };
            cx.notify();
            result.is_ok()
        });
        if accepted {
            self.consumed = true;
            self.selected = None;
            window.focus(&self.focus, cx);
            cx.notify();
        }
    }

    fn mount(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.surface.is_some() {
            return;
        }
        let (surface, mut requests) =
            Surface::new(self.snapshot.clone(), pixels::render, window, cx);
        let mut errors = surface.errors();
        let surface_id = self.snapshot.id.clone();
        cx.spawn(async move |this, cx| {
            while let Some(request) = requests.recv().await {
                if this
                    .update_in(cx, |this, window, cx| match request {
                        Request::Activate { observed, action } => {
                            this.activate(&observed, action, window, cx)
                        }
                        Request::Layout { id, width, height } => {
                            if this.snapshot.id.matches(&id)
                                && this.lease.upgrade().is_some()
                                && width.is_finite()
                                && width > 0.
                                && height.is_finite()
                                && height > 0.
                                && this.owner.upgrade().is_some_and(|owner| {
                                    owner
                                        .read(cx)
                                        .run_notice_epoch
                                        .matches(&this.snapshot.source.presentation_epoch)
                                })
                                && this.measured.is_none_or(|previous| {
                                    (previous.0 - width).abs() > 0.5
                                        || (previous.1 - height).abs() > 0.5
                                })
                            {
                                this.measured = Some((width, height));
                                cx.notify();
                            }
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        cx.spawn(async move |this, cx| {
            while errors.changed().await.is_ok() {
                if errors.borrow_and_update().is_some() {
                    let _ = this.update(cx, |this, cx| {
                        if this.snapshot.id.matches(&surface_id) {
                            this.selected = None;
                            cx.notify();
                        }
                    });
                    break;
                }
            }
        })
        .detach();
        self.surface = Some(surface);
    }

    fn key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !self.focus.contains_focused(window, cx) {
            return;
        }
        match Key::from_event(event) {
            Key::Next(backwards) => {
                match Selection::advance(
                    self.selected.as_ref(),
                    &self.snapshot.id,
                    &self.snapshot.actions(),
                    backwards,
                ) {
                    Step::Selected(selected) => {
                        self.selected = Some(selected);
                        window.focus(&self.focus, cx);
                    }
                    Step::Exit => crate::nsh::keyboard::leave(&self.focus, backwards, window, cx),
                }
                cx.stop_propagation();
                cx.notify();
            }
            Key::Activate if self.focus.is_focused(window) => {
                if let Some(action) = self.selected.as_ref().and_then(|selected| {
                    selected.action(&self.snapshot.id, &self.snapshot.actions())
                }) {
                    self.activate(&self.snapshot.clone(), action, window, cx);
                }
                cx.stop_propagation();
            }
            _ => {}
        }
    }
}

#[derive(gpui_kit::IntoElement)]
pub(super) struct NativeNotice {
    pub(super) owner: WeakEntity<ConversationStream>,
}

impl RenderOnce for NativeNotice {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Some(owner) = self.owner.upgrade() else {
            return div().into_any_element();
        };
        let stream = owner.read(cx);
        let Some(source) = Source::capture(stream) else {
            return div().into_any_element();
        };
        let enabled = source.current(stream, cx);
        let snapshot = Snapshot::new(source, theme(cx).colors, enabled);
        let state = window.use_keyed_state(
            format!("nsh-cua-notice-{}", owner.entity_id()),
            cx,
            |window, cx| {
                let focus = cx.focus_handle().tab_stop(!snapshot.actions().is_empty());
                let subscriptions = vec![
                    cx.on_focus_in(&focus, window, |this: &mut NoticeHost, window, cx| {
                        if this.focus.contains_focused(window, cx) {
                            if let Step::Selected(selected) = Selection::advance(
                                None,
                                &this.snapshot.id,
                                &this.snapshot.actions(),
                                window.modifiers().shift,
                            ) {
                                this.selected = Some(selected);
                            }
                            window.focus(&this.focus, cx);
                        }
                        cx.notify();
                    }),
                    cx.on_focus_out(&focus, window, |_, _, _, cx| cx.notify()),
                ];
                NoticeHost {
                    owner: self.owner.clone(),
                    snapshot: snapshot.clone(),
                    surface: None,
                    focus,
                    selected: None,
                    lease: Weak::new(),
                    measured: None,
                    consumed: false,
                    _subscriptions: subscriptions,
                }
            },
        );
        state.update(cx, |this, cx| {
            let mut snapshot = snapshot;
            let lease = if !this.snapshot.source.matches(&snapshot.source)
                || this.snapshot.enabled != snapshot.enabled
                || this.lease.upgrade().is_none()
            {
                this.stop(cx);
                Rc::new(Mount::default())
            } else {
                this.lease
                    .upgrade()
                    .unwrap_or_else(|| Rc::new(Mount::default()))
            };
            snapshot.id = this.snapshot.id.clone();
            this.lease = Rc::downgrade(&lease);
            let actions = if this.consumed {
                Vec::new()
            } else {
                snapshot.actions()
            };
            this.focus = this.focus.clone().tab_stop(!actions.is_empty());
            snapshot.selected = this.focus.is_focused(window)
                && this
                    .selected
                    .as_ref()
                    .and_then(|selected| selected.action(&snapshot.id, &actions))
                    .is_some();
            this.snapshot = snapshot.clone();
            this.mount(window, cx);
            let Some(surface) = &mut this.surface else {
                return div().into_any_element();
            };
            surface.sync_environment(cx);
            surface.publish(snapshot.clone());
            let mut host = div()
                .id("nsh-cua-notice-native")
                .relative()
                .w_full()
                .min_w_0()
                .flex_shrink_0()
                .h(px(this
                    .measured
                    .map(|(_, height)| height)
                    .unwrap_or_else(|| snapshot.estimated_height())))
                .role(Role::Group)
                .aria_label(
                    [snapshot.text.clone(), snapshot.target.clone()]
                        .into_iter()
                        .flatten()
                        .collect::<Vec<_>>()
                        .join("\n"),
                )
                .track_focus(&this.focus)
                .capture_key_down(
                    cx.listener(|this, event, window, cx| this.key(event, window, cx)),
                )
                .child(surface.entity());
            if let (Some(action), Some(label)) = (actions.first().copied(), snapshot.label.clone())
            {
                let weak = cx.weak_entity();
                let observed = snapshot.clone();
                host = host.child(
                    div()
                        .id("nsh-cua-notice-action")
                        .absolute()
                        .inset_0()
                        .role(Role::Button)
                        .aria_label(label)
                        .when(snapshot.selected, |node| node.aria_active_descendant())
                        .on_a11y_action(AccessibleAction::Click, move |_, window, cx| {
                            let _ = weak.update(cx, |this, cx| {
                                this.activate(&observed, action, window, cx)
                            });
                        }),
                );
            }
            mounted::Mounted {
                child: host.into_any_element(),
                lease,
                owner: self.owner,
            }
            .into_any_element()
        })
    }
}

#[cfg(test)]
mod tests;
