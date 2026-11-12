use super::*;
use crate::nsh::{ProjectionId, Surface};
use gpui_kit::{
    AccessibleAction, Bounds, Element, ElementId, GlobalElementId, InspectorElementId, LayoutId,
    RenderOnce, Size, Subscription, WeakEntity, WindowId,
};
use std::cell::Cell;
use std::rc::{Rc, Weak};
use std::sync::Arc;

mod pixels;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    PermissionTrigger,
    PermissionMenu,
    ModelTrigger,
    ModelMenu,
    Thinking,
}

#[derive(Clone, PartialEq, Eq)]
struct Source {
    thread: Thread,
    input: gpui_kit::EntityId,
    input_revision: u64,
    models: Vec<ModelOption>,
    defaults: ComposerDefaults,
    next_model: Option<String>,
    pending: Option<(u64, String)>,
    save_owner: Option<u64>,
    level: ModelPickerLevel,
    highlight: usize,
    permission_open: bool,
    active_mode: Option<PermissionMode>,
    progress: Option<&'static str>,
    slider: super::thinking_slider::native::Snapshot,
    reasoning: Option<ReasoningProfileProjection>,
    reasoning_intents: HashMap<String, (u64, ReasoningChoice)>,
    state: Vec<bool>,
    appearance: vega_theme::Appearance,
}

impl Source {
    fn capture(stream: &ConversationStream, cx: &App) -> Self {
        Self {
            thread: stream.thread.clone(),
            input: stream.input.entity_id(),
            input_revision: stream.input.read(cx).content_revision(),
            models: stream.model_options.clone(),
            defaults: stream.composer_defaults.clone(),
            next_model: stream.next_turn_model.clone(),
            pending: stream.model_selection_pending.clone(),
            save_owner: stream.model_selection_save_owner,
            level: stream.model_picker_level,
            highlight: stream.model_selector_highlight,
            permission_open: stream.permission_picker_open,
            active_mode: stream.permission_editor.active_mode,
            progress: stream.permission_editor.progress_label(),
            slider: stream.thinking_slider.read(cx).native_snapshot(),
            reasoning: stream.selector_reasoning.clone(),
            reasoning_intents: stream.reasoning_intents.clone(),
            state: vec![
                stream.actions.running,
                stream.trusted_action_busy,
                stream.model_selection_save_busy(),
                stream.permission_change_pending(),
                stream.reading_preview,
                stream.draft_route,
                stream.permissions_revoked,
                stream.input.read(cx).is_composing(),
                stream.ui_presentations_suspended,
            ],
            appearance: theme(cx).appearance,
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
enum Action {
    PermissionTrigger,
    Permission(PermissionMode),
    ModelTrigger,
    Model(ModelOption),
    ThinkingTitle,
    Tier(usize),
    Default,
}

#[derive(Clone, PartialEq, Eq)]
struct Row {
    action: Action,
    label: String,
    description: Option<String>,
    group: Option<String>,
    checked: bool,
    warning: bool,
    enabled: bool,
    glyph: Option<crate::icons::Icon>,
}

#[derive(Default)]
struct NativeRevisions {
    action: AtomicU64,
    height: AtomicU64,
}

#[derive(Clone)]
struct Snapshot {
    id: ProjectionId,
    epoch: ProjectionId,
    source: Source,
    kind: Kind,
    rows: Vec<Row>,
    header: Vec<String>,
    colors: ThemeColors,
    geometry: Option<Geometry>,
    mount: Option<ProjectionId>,
    selected: Option<usize>,
    keyboard_focus_visible: bool,
    intent: ProjectionId,
    activation: Option<(usize, Origin)>,
    native_revision: Arc<NativeRevisions>,
}

impl Snapshot {
    fn capture(stream: &ConversationStream, kind: Kind, cx: &App) -> Self {
        use super::composer_actions::*;
        let permission_enabled = !stream.permission_change_pending();
        let model_enabled = !stream.trusted_action_busy
            && !stream.model_selection_save_busy()
            && stream.model_selection_pending.is_none();
        let row = |action, label: String, enabled, checked| Row {
            action,
            label,
            description: None,
            group: None,
            checked,
            warning: false,
            enabled,
            glyph: None,
        };
        let mut header = Vec::new();
        let rows = match kind {
            Kind::PermissionTrigger => {
                let mut item = row(
                    Action::PermissionTrigger,
                    stream
                        .permission_editor
                        .progress_label()
                        .unwrap_or(permission_label(stream.thread.permission_mode))
                        .to_owned(),
                    permission_enabled,
                    stream.permission_picker_open,
                );
                item.warning = permission_is_warning(stream.thread.permission_mode);
                item.glyph = Some(permission_icon(stream.thread.permission_mode));
                vec![item]
            }
            Kind::PermissionMenu => {
                header.push(
                    vega_i18n::t(if stream.actions.running {
                        "ui.permission_picker_next_run"
                    } else {
                        PERMISSION_PICKER_TITLE
                    })
                    .to_owned(),
                );
                header.push(vega_i18n::t(PERMISSION_PICKER_LEARN_MORE).to_owned());
                if let Some(mode) = stream
                    .permission_editor
                    .active_mode
                    .filter(|mode| *mode != stream.thread.permission_mode)
                {
                    header.push(vega_i18n::t_args(
                        "ui.permission_current_run",
                        &[("mode", permission_label(mode))],
                    ));
                }
                PERMISSION_ORDER
                    .into_iter()
                    .map(|mode| Row {
                        action: Action::Permission(mode),
                        label: permission_label(mode).to_owned(),
                        description: Some(permission_description(mode).to_owned()),
                        group: None,
                        checked: mode == stream.thread.permission_mode,
                        warning: permission_is_warning(mode),
                        enabled: permission_enabled,
                        glyph: Some(permission_icon(mode)),
                    })
                    .collect()
            }
            Kind::ModelTrigger => vec![row(
                Action::ModelTrigger,
                super::render::model_trigger_label(
                    stream.selector_model(),
                    stream.model_picker_level,
                    stream.model_selection_pending.is_some(),
                )
                .to_owned(),
                model_enabled && !stream.model_options.is_empty(),
                stream.model_picker_level.is_open(),
            )],
            Kind::ModelMenu => {
                header.push(vega_i18n::t(super::render::PICKER_LIST_HEADING).to_owned());
                let mut rows: Vec<_> = stream
                    .model_options
                    .iter()
                    .enumerate()
                    .map(|(index, option)| Row {
                        action: Action::Model(option.clone()),
                        label: option.model.clone(),
                        description: None,
                        group: model_picker_starts_group(&stream.model_options, index)
                            .then(|| option.provider.clone()),
                        checked: option.model == stream.selector_model(),
                        warning: false,
                        enabled: model_enabled,
                        glyph: None,
                    })
                    .collect();
                if stream.can_restore_default_effort(cx) {
                    rows.push(row(
                        Action::Default,
                        vega_i18n::t("ui.use_default_effort").to_owned(),
                        model_enabled,
                        false,
                    ));
                }
                rows
            }
            Kind::Thinking => {
                let slider = stream.thinking_slider.read(cx).native_snapshot();
                let mut rows = vec![row(
                    Action::ThinkingTitle,
                    slider.model.label(),
                    model_enabled,
                    false,
                )];
                rows.extend((0..slider.model.dot_count()).map(|index| {
                    row(
                        Action::Tier(index),
                        slider.position_label(index),
                        model_enabled,
                        slider.model.selected_position() == Some(index),
                    )
                }));
                rows
            }
        };
        let selected = match kind {
            Kind::ModelMenu => Some(
                stream
                    .model_selector_highlight
                    .min(rows.len().saturating_sub(1)),
            ),
            Kind::Thinking => Some(
                stream
                    .thinking_slider
                    .read(cx)
                    .native_snapshot()
                    .model
                    .selected_position()
                    .map_or(0, |index| index + 1),
            ),
            _ => None,
        };
        Self {
            id: ProjectionId::new(),
            epoch: stream.run_notice_epoch.clone(),
            source: Source::capture(stream, cx),
            kind,
            rows,
            header,
            colors: theme(cx).colors,
            geometry: None,
            mount: None,
            selected,
            keyboard_focus_visible: false,
            intent: ProjectionId::new(),
            activation: None,
            native_revision: Arc::new(NativeRevisions::default()),
        }
    }

    fn same_source(&self, other: &Self) -> bool {
        self.epoch.matches(&other.epoch)
            && self.kind == other.kind
            && self.source == other.source
            && self.rows == other.rows
            && self.header == other.header
    }

    fn accepts(&self, observed: &Self, index: usize, origin: Origin, native: u64) -> bool {
        self.id.matches(&observed.id)
            && self.intent.matches(&observed.intent)
            && self.same_source(observed)
            && Arc::ptr_eq(&self.native_revision, &observed.native_revision)
            && observed.native_revision.action.load(Ordering::Acquire) == native
            && self.rows.get(index) == observed.rows.get(index)
            && self.rows.get(index).is_some_and(|row| row.enabled)
            && (origin == Origin::Pointer
                || (self.activation == Some((index, origin))
                    && observed.activation == self.activation))
    }

    fn visible(&self, stream: &ConversationStream, cx: &App) -> bool {
        !stream.reading_preview
            && !stream.ui_presentations_suspended
            && cx
                .try_global::<crate::settings::SettingsOpen>()
                .is_some_and(|settings| !settings.0)
            && cx
                .try_global::<crate::sidebar::OpenedThread>()
                .is_some_and(|route| match &route.0 {
                    Some(thread) => thread.id == stream.thread.id,
                    None => stream.draft_route,
                })
            && match self.kind {
                Kind::PermissionMenu => stream.permission_picker_open,
                Kind::ModelMenu => stream.model_picker_level == ModelPickerLevel::List,
                Kind::Thinking => {
                    stream.model_picker_level == ModelPickerLevel::Slider
                        && stream.thinking_slider.read(cx).has_tiers()
                }
                _ => true,
            }
    }

    fn current(&self, stream: &ConversationStream, cx: &App) -> bool {
        self.visible(stream, cx)
            && !stream.input.read(cx).is_composing()
            && self.same_source(&Self::capture(stream, self.kind, cx))
    }
}

#[derive(Clone, Copy, PartialEq)]
struct Geometry {
    bounds: Bounds<Pixels>,
    viewport: Size<Pixels>,
    foreground: Pixels,
    scale: f32,
    window: WindowId,
}

#[derive(Default)]
struct Mount {
    identity: ProjectionId,
    geometry: Cell<Option<Geometry>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Origin {
    Pointer,
    Keyboard,
    Accessibility,
}

enum Request {
    Intent(Box<Snapshot>, usize, Origin, u64),
    Activate(Box<Snapshot>, usize, Origin, u64),
    Height(Box<Snapshot>, u64, f32, f32),
}

struct Host {
    owner: WeakEntity<ConversationStream>,
    snapshot: Snapshot,
    surface: Option<Surface<Snapshot, Request>>,
    published: Option<Snapshot>,
    lease: Weak<Mount>,
    focus: FocusHandle,
    height: Option<f32>,
    activation_focus: Option<FocusHandle>,
    accessibility_generation: ProjectionId,
    paired_host: Rc<std::cell::RefCell<Option<WeakEntity<Host>>>>,
    _subscriptions: Vec<Subscription>,
}

impl Host {
    fn sync_snapshot(&mut self, mut snapshot: Snapshot, cx: &App) -> bool {
        let changed = !self.snapshot.same_source(&snapshot);
        if changed {
            if self.fixed_trigger()
                && self.snapshot.epoch.matches(&snapshot.epoch)
                && self.snapshot.source.thread.id == snapshot.source.thread.id
                && self.snapshot.source.input == snapshot.source.input
            {
                snapshot.geometry = self.snapshot.geometry;
                snapshot.mount = self.snapshot.mount.clone();
                self.activation_focus = None;
            } else {
                self.stop(cx);
            }
            self.height = None;
        } else {
            snapshot.id = self.snapshot.id.clone();
            snapshot.intent = self.snapshot.intent.clone();
            snapshot.selected = self.snapshot.selected;
            snapshot.activation = self.snapshot.activation;
            snapshot.geometry = self.snapshot.geometry;
            snapshot.mount = self.snapshot.mount.clone();
            snapshot.native_revision = self.snapshot.native_revision.clone();
        }
        self.snapshot = snapshot;
        changed
    }

    fn observe_owner(owner: &Entity<ConversationStream>, cx: &mut Context<Self>) -> Subscription {
        cx.observe(owner, |host, owner, cx| {
            let snapshot = Snapshot::capture(owner.read(cx), host.snapshot.kind, cx);
            if host.sync_snapshot(snapshot, cx) {
                cx.notify();
            }
        })
    }

    fn stop(&mut self, cx: &App) {
        if let Some(surface) = self.surface.take() {
            surface.entity().read(cx).stop();
        }
        self.invalidate_interaction();
    }

    fn fixed_trigger(&self) -> bool {
        matches!(
            self.snapshot.kind,
            Kind::PermissionTrigger | Kind::ModelTrigger
        )
    }

    fn window_changed(&mut self, cx: &App) {
        if self.fixed_trigger() {
            self.invalidate_interaction();
        } else {
            self.stop(cx);
        }
    }

    fn invalidate_interaction(&mut self) {
        self.published = None;
        self.snapshot.id = ProjectionId::new();
        self.snapshot.intent = ProjectionId::new();
        self.snapshot.activation = None;
        self.activation_focus = None;
        self.snapshot.native_revision = Arc::new(NativeRevisions::default());
    }

    fn sync_geometry(&mut self, lease: &Mount) -> bool {
        if self.snapshot.geometry != lease.geometry.get()
            || !self
                .snapshot
                .mount
                .as_ref()
                .is_some_and(|mount| mount.matches(&lease.identity))
        {
            self.snapshot.geometry = lease.geometry.get();
            self.snapshot.mount = Some(lease.identity.clone());
            self.snapshot.id = ProjectionId::new();
            self.snapshot.intent = ProjectionId::new();
            self.snapshot.activation = None;
            self.snapshot.native_revision = Arc::new(NativeRevisions::default());
            return true;
        }
        false
    }

    fn valid(&self, observed: &Snapshot, window: &Window, cx: &App) -> bool {
        let Some(owner) = self.owner.upgrade() else {
            return false;
        };
        let stream = owner.read(cx);
        let Some(lease) = self.lease.upgrade() else {
            return false;
        };
        let Some(geometry) = lease.geometry.get() else {
            return false;
        };
        window.is_window_active()
            && self.snapshot.id.matches(&observed.id)
            && self.snapshot.intent.matches(&observed.intent)
            && self.snapshot.same_source(observed)
            && observed
                .mount
                .as_ref()
                .is_some_and(|mount| mount.matches(&lease.identity))
            && self.published.as_ref().is_some_and(|published| {
                published.id.matches(&observed.id)
                    && published.intent.matches(&observed.intent)
                    && published
                        .mount
                        .as_ref()
                        .is_some_and(|mount| mount.matches(&lease.identity))
            })
            && observed.current(stream, cx)
            && observed.geometry == Some(geometry)
            && geometry.window == window.window_handle().window_id()
            && geometry.viewport == window.viewport_size()
            && geometry.scale == window.scale_factor()
            && geometry.foreground == stream.foreground_height
            && !geometry.bounds.is_empty()
            && self
                .surface
                .as_ref()
                .is_some_and(|surface| surface.errors().borrow().is_none())
    }

    fn activate(
        &mut self,
        observed: &Snapshot,
        index: usize,
        origin: Origin,
        scroll: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.valid(observed, window, cx)
            || !self.snapshot.accepts(observed, index, origin, scroll)
            || (origin == Origin::Keyboard
                && !self
                    .activation_focus
                    .as_ref()
                    .is_some_and(|focus| focus.is_focused(window)))
        {
            return;
        }
        let Some(action) = observed.rows.get(index).map(|row| row.action.clone()) else {
            return;
        };
        let Some(owner) = self.owner.upgrade() else {
            return;
        };
        if action == Action::ModelTrigger
            && observed.source.level == ModelPickerLevel::List
            && origin == Origin::Keyboard
        {
            let target = self.paired_host.borrow().clone();
            if let Some(target) = target {
                let _ = target.update(cx, |host, cx| {
                    host.request(observed.source.highlight, origin, window, cx);
                });
            }
            return;
        }
        self.snapshot.intent = ProjectionId::new();
        self.snapshot.id = ProjectionId::new();
        self.snapshot.activation = None;
        owner.update(cx, |stream, cx| {
            if !observed.current(stream, cx) {
                return;
            }
            match action {
                Action::PermissionTrigger => {
                    let open = !stream.permission_picker_open;
                    stream.close_composer_popovers(cx);
                    if open {
                        stream.permission_picker_open = true;
                        let index = super::composer_actions::PERMISSION_ORDER
                            .iter()
                            .position(|mode| *mode == stream.thread.permission_mode)
                            .unwrap_or(1);
                        window.focus(&stream.permission_editor.options[index], cx);
                    } else {
                        window.focus(&stream.permission_editor.trigger, cx);
                    }
                    cx.notify();
                }
                Action::Permission(mode) => stream.select_permission_mode(mode, window, cx),
                Action::ModelTrigger if stream.model_picker_level.is_open() => {
                    stream.model_picker_level = ModelPickerLevel::Closed;
                    window.focus(&stream.model_focus, cx);
                    cx.notify();
                }
                Action::ModelTrigger => stream.on_activate_model(&ActivateModel, window, cx),
                Action::Model(option) => {
                    if stream.model_options.get(index) == Some(&option) {
                        stream.select_model_option(&option.model, cx);
                        stream.focus_composer(window, cx);
                    }
                }
                Action::ThinkingTitle => {
                    stream.on_thinking_slider_title_activated(&ThinkingSliderTitleActivated, cx);
                    if stream.model_picker_level == ModelPickerLevel::List {
                        window.focus(&stream.model_focus, cx);
                    }
                }
                Action::Tier(position) => {
                    stream.thinking_slider.update(cx, |slider, cx| {
                        slider.select_index(position, cx);
                    });
                }
                Action::Default => {
                    stream.restore_default_effort(window, cx);
                }
            }
        });
        cx.notify();
    }

    fn request(&mut self, index: usize, origin: Origin, window: &Window, cx: &mut Context<Self>) {
        if !self.valid(&self.snapshot, window, cx)
            || !self.snapshot.rows.get(index).is_some_and(|row| row.enabled)
        {
            return;
        }
        self.snapshot.intent = ProjectionId::new();
        self.snapshot.selected = Some(index);
        self.snapshot.activation = Some((index, origin));
        self.activation_focus = (origin == Origin::Keyboard)
            .then(|| window.focused(cx))
            .flatten();
        cx.notify();
    }

    fn key(&mut self, event: &gpui_kit::KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.prefer_character_input || event.is_held || !self.valid(&self.snapshot, window, cx)
        {
            return;
        }
        let modifiers = event.keystroke.modifiers;
        if modifiers.control || modifiers.alt || modifiers.platform || modifiers.function {
            return;
        }
        let key = event.keystroke.key.as_str();
        let Some(owner) = self.owner.upgrade() else {
            return;
        };
        let stream = owner.read(cx);
        if self.snapshot.kind == Kind::Thinking
            && stream.model_focus.is_focused(window)
            && matches!(key, "enter" | "space" | " ")
        {
            owner.update(cx, |stream, cx| {
                stream.on_activate_model(&ActivateModel, window, cx)
            });
            cx.stop_propagation();
            return;
        }
        // A permission activation must originate from the option that actually has focus.
        let permission_focus = if self.snapshot.kind == Kind::PermissionMenu {
            let Some(index) = stream
                .permission_editor
                .options
                .iter()
                .position(|option| option.is_focused(window))
            else {
                return;
            };
            Some(index)
        } else {
            None
        };
        let focused = self.focus.contains_focused(window, cx)
            || self
                .surface
                .as_ref()
                .is_some_and(|surface| surface.focus_handle(cx).contains_focused(window, cx))
            || stream.model_focus.contains_focused(window, cx)
            || stream.permission_editor.trigger.is_focused(window)
            || stream
                .permission_editor
                .options
                .iter()
                .any(|focus| focus.is_focused(window));
        if !focused {
            return;
        }
        if key == "escape" {
            if !matches!(
                self.snapshot.kind,
                Kind::PermissionMenu | Kind::ModelMenu | Kind::Thinking
            ) {
                return;
            }
            owner.update(cx, |stream, cx| {
                if self.snapshot.kind == Kind::PermissionMenu {
                    stream.permission_picker_open = false;
                    window.focus(&stream.permission_editor.trigger, cx);
                    cx.notify();
                } else {
                    stream.on_model_close(&CloseModel, window, cx);
                }
            });
            self.snapshot.intent = ProjectionId::new();
            self.snapshot.activation = None;
            cx.stop_propagation();
            return;
        }
        if self.snapshot.kind == Kind::ModelMenu && matches!(key, "up" | "down") {
            owner.update(cx, |stream, cx| {
                if key == "up" {
                    stream.on_model_previous(&PreviousModel, window, cx);
                } else {
                    stream.on_model_next(&NextModel, window, cx);
                }
            });
            cx.stop_propagation();
            return;
        }
        if matches!(
            key,
            "up" | "down" | "left" | "right" | "home" | "end" | "tab"
        ) && matches!(self.snapshot.kind, Kind::PermissionMenu | Kind::Thinking)
        {
            let count = self.snapshot.rows.len();
            if permission_focus.is_some() && !matches!(key, "up" | "down" | "tab") {
                return;
            }
            let current = permission_focus.or(self.snapshot.selected).unwrap_or(0);
            let next = if self.snapshot.kind == Kind::PermissionMenu {
                if key == "up" || key == "left" || (key == "tab" && modifiers.shift) {
                    (current + count - 1) % count
                } else {
                    (current + 1) % count
                }
            } else {
                match key {
                    "home" => 1.min(count.saturating_sub(1)),
                    "end" => count.saturating_sub(1),
                    "up" | "left" => current.saturating_sub(1).max(1),
                    "tab" if modifiers.shift => (current + count - 1) % count,
                    "tab" => (current + 1) % count,
                    _ => (current + 1).min(count.saturating_sub(1)),
                }
            };
            if self.snapshot.kind == Kind::Thinking && key != "tab" {
                window.focus(&self.focus, cx);
                self.request(next, Origin::Keyboard, window, cx);
                cx.stop_propagation();
                return;
            }
            self.snapshot.selected = Some(next);
            self.snapshot.intent = ProjectionId::new();
            self.snapshot.activation = None;
            if self.snapshot.kind == Kind::PermissionMenu {
                owner.update(cx, |stream, cx| {
                    window.focus(&stream.permission_editor.options[next], cx)
                });
            } else {
                window.focus(&self.focus, cx);
            }
            cx.notify();
            cx.stop_propagation();
            return;
        }
        if matches!(key, "enter" | "space" | " ") {
            let index = permission_focus.or(self.snapshot.selected).unwrap_or(0);
            self.request(index, Origin::Keyboard, window, cx);
            cx.stop_propagation();
        }
    }

    fn mount(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.surface.is_some() {
            return;
        }
        let (surface, mut requests) =
            Surface::new_transparent(self.snapshot.clone(), pixels::render, window, cx);
        cx.spawn(async move |this, cx| {
            while let Some(request) = requests.recv().await {
                if this
                    .update_in(cx, |host, window, cx| match request {
                        Request::Intent(observed, index, origin, native) => {
                            if host.valid(&observed, window, cx)
                                && observed.native_revision.action.load(Ordering::Acquire) == native
                            {
                                host.request(index, origin, window, cx);
                            }
                        }
                        Request::Activate(observed, index, origin, scroll) => {
                            host.activate(&observed, index, origin, scroll, window, cx)
                        }
                        Request::Height(observed, native, width, height) => {
                            if host.valid(&observed, window, cx)
                                && observed.native_revision.height.load(Ordering::Acquire) == native
                                && width.is_finite()
                                && height.is_finite()
                                && height > 0.
                                && host
                                    .lease
                                    .upgrade()
                                    .and_then(|lease| lease.geometry.get())
                                    .is_some_and(|geometry| {
                                        (f32::from(geometry.bounds.size.width) - width).abs() < 1.
                                    })
                                && host.height.is_none_or(|previous| {
                                    (previous - height.min(Layout::COMPOSER_PICKER_MAX_HEIGHT))
                                        .abs()
                                        > 0.5
                                })
                            {
                                host.height = Some(height.min(Layout::COMPOSER_PICKER_MAX_HEIGHT));
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
        surface.focus_handle(cx).tab_stop(false);
        self.surface = Some(surface);
        self.published = Some(self.snapshot.clone());
    }
}

#[derive(gpui_kit::IntoElement)]
struct Native {
    owner: WeakEntity<ConversationStream>,
    kind: Kind,
    route: Rc<std::cell::RefCell<Option<WeakEntity<Host>>>>,
    paired_host: Rc<std::cell::RefCell<Option<WeakEntity<Host>>>>,
}

impl RenderOnce for Native {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Some(owner) = self.owner.upgrade() else {
            return div().into_any_element();
        };
        let snapshot = Snapshot::capture(owner.read(cx), self.kind, cx);
        if !snapshot.visible(owner.read(cx), cx) {
            return div().into_any_element();
        }
        let key = format!(
            "nsh-composer-selector-{}-{}",
            owner.entity_id(),
            self.kind as u8
        );
        let host = window.use_keyed_state(key, cx, |window, cx| {
            let focus = match self.kind {
                Kind::PermissionTrigger => owner.read(cx).permission_editor.trigger.clone(),
                Kind::ModelTrigger | Kind::ModelMenu => owner.read(cx).model_focus.clone(),
                _ => cx.focus_handle().tab_stop(true),
            };
            let mut subscriptions = vec![cx.on_release(|host: &mut Host, cx| host.stop(cx))];
            subscriptions.push(
                cx.observe_window_activation(window, |host: &mut Host, _, cx| {
                    host.window_changed(cx);
                    cx.notify();
                }),
            );
            subscriptions.push(cx.observe_window_bounds(window, |host: &mut Host, _, cx| {
                host.window_changed(cx);
                cx.notify();
            }));
            subscriptions.push(
                cx.on_focus_out(&focus, window, |host: &mut Host, _, _, cx| {
                    if host.snapshot.kind == Kind::ModelMenu && host.snapshot.activation.is_none() {
                        host.accessibility_generation = ProjectionId::new();
                        host.snapshot
                            .native_revision
                            .action
                            .fetch_add(1, Ordering::AcqRel);
                        cx.notify();
                        return;
                    }
                    host.snapshot.intent = ProjectionId::new();
                    host.snapshot.activation = None;
                    host.activation_focus = None;
                    cx.notify();
                }),
            );
            if self.kind == Kind::PermissionMenu {
                let host = cx.weak_entity();
                subscriptions.push(cx.intercept_keystrokes(move |event, window, cx| {
                    if event.keystroke.key != "escape"
                        || event.input_preference == gpui_kit::InputPreference::CharacterInput
                        || event.keystroke.modifiers != gpui_kit::Modifiers::default()
                    {
                        return;
                    }
                    let _ = host.update(cx, |host: &mut Host, cx| {
                        let Some(owner) = host.owner.upgrade() else {
                            return;
                        };
                        let stream = owner.read(cx);
                        if host.valid(&host.snapshot, window, cx)
                            && stream
                                .permission_editor
                                .options
                                .iter()
                                .any(|option| option.is_focused(window))
                        {
                            owner.update(cx, |stream, cx| {
                                stream.permission_picker_open = false;
                                window.focus(&stream.permission_editor.trigger, cx);
                                cx.notify();
                            });
                            cx.stop_propagation();
                        }
                    });
                }));
            }
            subscriptions.push(Host::observe_owner(&owner, cx));
            Host {
                owner: self.owner.clone(),
                snapshot: snapshot.clone(),
                surface: None,
                published: None,
                lease: Weak::new(),
                focus,
                height: None,
                activation_focus: None,
                accessibility_generation: ProjectionId::new(),
                paired_host: self.paired_host.clone(),
                _subscriptions: subscriptions,
            }
        });
        host.update(cx, |host, cx| {
            host.paired_host = self.paired_host.clone();
            host.sync_snapshot(snapshot, cx);
        });
        *self.route.borrow_mut() = Some(host.downgrade());
        host.into_any_element()
    }
}

impl Render for Host {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(owner) = self.owner.upgrade() else {
            self.stop(cx);
            return div().into_any_element();
        };
        if !self.snapshot.current(owner.read(cx), cx) {
            self.stop(cx);
            return div().into_any_element();
        }
        let lease = self.lease.upgrade().unwrap_or_else(|| {
            self.stop(cx);
            Rc::new(Mount::default())
        });
        self.lease = Rc::downgrade(&lease);
        if self.snapshot.kind == Kind::PermissionMenu {
            let index = owner
                .read(cx)
                .permission_editor
                .options
                .iter()
                .position(|focus| focus.is_focused(window));
            self.snapshot.keyboard_focus_visible =
                index.is_some() && window.last_input_was_keyboard();
            if index.is_some() && self.snapshot.selected != index {
                self.snapshot.selected = index;
                self.snapshot.intent = ProjectionId::new();
                self.snapshot.activation = None;
            }
        }
        if self.snapshot.kind == Kind::Thinking {
            self.snapshot.keyboard_focus_visible =
                self.focus.is_focused(window) && window.last_input_was_keyboard();
        }
        self.sync_geometry(&lease);
        self.mount(window, cx);
        let Some(surface) = &mut self.surface else {
            return div().into_any_element();
        };
        surface.sync_environment(cx);
        if self.published.as_ref().is_none_or(|published| {
            !published.id.matches(&self.snapshot.id)
                || !published.intent.matches(&self.snapshot.intent)
                || published.keyboard_focus_visible != self.snapshot.keyboard_focus_visible
                || published.selected != self.snapshot.selected
                || published.geometry != self.snapshot.geometry
        }) {
            surface.publish(self.snapshot.clone());
            self.published = Some(self.snapshot.clone());
        }
        let popup = matches!(
            self.snapshot.kind,
            Kind::PermissionMenu | Kind::ModelMenu | Kind::Thinking
        );
        let mut element = div()
            .id("nsh-composer-selector-host")
            .when(self.snapshot.kind == Kind::PermissionMenu, |host| {
                host.key_context("PermissionOptions")
            })
            .relative()
            .min_w_0()
            .when(!popup || self.snapshot.kind == Kind::Thinking, |element| {
                element.track_focus(&self.focus)
            })
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .when(popup, |host| {
                let shadow = if self.snapshot.kind == Kind::Thinking {
                    super::foreground_chrome::Decoration::ThinkingCardShadow
                } else {
                    super::foreground_chrome::Decoration::SelectorMenuShadow
                };
                let edge = px(-super::thinking_slider::SHADOW_MARGIN);
                host.child(
                    div()
                        .id("composer-selector-shadow")
                        .role(gpui_kit::Role::Group)
                        .a11y_synthetic_children(|tree| tree.parent_node().set_hidden())
                        .absolute()
                        .top(edge)
                        .bottom(edge)
                        .left(edge)
                        .right(edge)
                        .child(super::foreground_chrome::decoration(shadow)),
                )
            })
            .child(surface.entity());
        if popup {
            element = element
                .w(px(if self.snapshot.kind == Kind::Thinking {
                    super::thinking_slider::THINKING_CARD_WIDTH
                } else {
                    Layout::MENU_MAX_WIDTH
                }))
                .h(px(self.height.unwrap_or(
                    if self.snapshot.kind == Kind::Thinking {
                        132.
                    } else {
                        240.
                    },
                )))
                .occlude()
                .on_mouse_down_out(cx.listener(|host, event: &MouseDownEvent, window, cx| {
                    // The trigger handles its own toggle on release. Dismissing here
                    // would turn that same click into an open request.
                    let on_trigger = event.button == MouseButton::Left
                        && host
                            .paired_host
                            .borrow()
                            .as_ref()
                            .and_then(WeakEntity::upgrade)
                            .is_some_and(|trigger| {
                                let trigger = trigger.read(cx);
                                trigger.snapshot.rows.first().is_some_and(|row| row.enabled)
                                    && trigger.valid(&trigger.snapshot, window, cx)
                                    && trigger.snapshot.geometry.is_some_and(|geometry| {
                                        geometry.bounds.contains(&event.position)
                                    })
                            });
                    if on_trigger || !host.valid(&host.snapshot, window, cx) {
                        return;
                    }
                    let Some(owner) = host.owner.upgrade() else {
                        return;
                    };
                    owner.update(cx, |stream, cx| {
                        if host.snapshot.kind == Kind::PermissionMenu {
                            stream.permission_picker_open = false;
                            window.focus(&stream.permission_editor.trigger, cx);
                            cx.notify();
                        } else {
                            stream.on_model_close(&CloseModel, window, cx);
                        }
                    });
                    host.stop(cx);
                    cx.notify();
                }));
        } else {
            let width = self
                .snapshot
                .rows
                .first()
                .map(|row| {
                    f32::from(
                        window
                            .text_system()
                            .shape_line(
                                row.label.clone().into(),
                                px(Typography::SIDEBAR),
                                &[window.text_style().to_run(row.label.len())],
                                None,
                            )
                            .width()
                            .ceil(),
                    ) + 52.
                })
                .unwrap_or(52.);
            element = element
                .w(px(width.min(Layout::MENU_MAX_WIDTH)))
                .h(px(Layout::COMPOSER_TEXT_TRIGGER_HEIGHT))
                .flex_shrink_0();
        }
        for (index, row) in self.snapshot.rows.iter().enumerate() {
            let weak = cx.weak_entity();
            let observed = self.snapshot.clone();
            let focused = weak.clone();
            let focus_observed = observed.clone();
            let click_generation = self.accessibility_generation.clone();
            let focus_generation = click_generation.clone();
            let focus_target = if self.snapshot.kind == Kind::PermissionMenu {
                owner.read(cx).permission_editor.options[index].clone()
            } else {
                self.focus.clone()
            };
            let label = row.description.as_ref().map_or_else(
                || row.label.clone(),
                |description| format!("{}\n{description}", row.label),
            );
            element = element.child(
                div()
                    .id(format!("nsh-selector-ax-{index}"))
                    .when(self.snapshot.kind == Kind::PermissionMenu, |node| {
                        node.track_focus(&owner.read(cx).permission_editor.options[index])
                    })
                    .absolute()
                    .inset_0()
                    .role(gpui_kit::Role::Button)
                    .aria_label(label)
                    .aria_toggled(if row.checked {
                        gpui_kit::Toggled::True
                    } else {
                        gpui_kit::Toggled::False
                    })
                    .on_a11y_action(AccessibleAction::Focus, move |_, window, cx| {
                        let _ = focused.update(cx, |host, cx| {
                            if host.accessibility_generation.matches(&focus_generation)
                                && host.valid(&focus_observed, window, cx)
                            {
                                host.snapshot.selected = Some(index);
                                host.snapshot.intent = ProjectionId::new();
                                host.snapshot.activation = None;
                                window.focus(&focus_target, cx);
                                cx.notify();
                            }
                        });
                    })
                    .on_a11y_action(AccessibleAction::Click, move |_, window, cx| {
                        let _ = weak.update(cx, |host, cx| {
                            if host.accessibility_generation.matches(&click_generation)
                                && host.valid(&observed, window, cx)
                            {
                                host.request(index, Origin::Accessibility, window, cx);
                            }
                        });
                    }),
            );
        }
        Mounted {
            child: element.into_any_element(),
            lease,
            host: cx.weak_entity(),
            owner: self.owner.clone(),
        }
        .into_any_element()
    }
}

struct Mounted {
    child: AnyElement,
    lease: Rc<Mount>,
    host: WeakEntity<Host>,
    owner: WeakEntity<ConversationStream>,
}
impl IntoElement for Mounted {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl Element for Mounted {
    type RequestLayoutState = ();
    type PrepaintState = ();
    fn id(&self) -> Option<ElementId> {
        Some("nsh-selector-mount".into())
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
        (self.child.request_layout(window, cx), ())
    }
    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let previous = self.lease.geometry.replace(None);
        if let Some(id) = id {
            window.with_element_state::<Rc<Mount>, _>(id, |_, _| ((), self.lease.clone()));
            if let Some(owner) = self.owner.upgrade() {
                let visible = bounds.intersect(&window.content_mask().bounds);
                if !visible.is_empty() {
                    self.lease.geometry.set(Some(Geometry {
                        bounds: visible,
                        viewport: window.viewport_size(),
                        foreground: owner.read(cx).foreground_height,
                        scale: window.scale_factor(),
                        window: window.window_handle().window_id(),
                    }));
                }
            }
        }
        if previous != self.lease.geometry.get() {
            let lease = &self.lease;
            let _ = self.host.update(cx, |host, cx| {
                if host
                    .lease
                    .upgrade()
                    .is_some_and(|active| Rc::ptr_eq(&active, lease))
                    && host.sync_geometry(lease)
                {
                    if let Some(surface) = &host.surface {
                        surface.publish(host.snapshot.clone());
                        host.published = Some(host.snapshot.clone());
                    }
                    cx.on_next_frame(window, |_, _, cx| cx.notify());
                }
            });
        }
        self.child.prepaint(window, cx);
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
        self.child.paint(window, cx);
    }
}

impl ConversationStream {
    pub(crate) fn render_nsh_permission_selector(&self, cx: &mut Context<Self>) -> AnyElement {
        selector(
            cx.weak_entity(),
            Kind::PermissionTrigger,
            self.permission_picker_open.then_some(Kind::PermissionMenu),
        )
    }
    pub(crate) fn render_nsh_model_selector(&self, cx: &mut Context<Self>) -> AnyElement {
        selector(
            cx.weak_entity(),
            Kind::ModelTrigger,
            match self.model_picker_level {
                ModelPickerLevel::Closed => None,
                ModelPickerLevel::Slider => Some(Kind::Thinking),
                ModelPickerLevel::List => Some(Kind::ModelMenu),
            },
        )
    }
}

fn selector(owner: WeakEntity<ConversationStream>, kind: Kind, popup: Option<Kind>) -> AnyElement {
    let trigger_route = Rc::new(std::cell::RefCell::new(None::<WeakEntity<Host>>));
    let popup_route = Rc::new(std::cell::RefCell::new(None::<WeakEntity<Host>>));
    let key_trigger = trigger_route.clone();
    let key_popup = popup_route.clone();
    let activation_trigger = trigger_route.clone();
    let activation_popup = popup_route.clone();
    let activation_owner = owner.clone();
    let previous_route = popup_route.clone();
    let next_route = popup_route.clone();
    let close_route = popup_route.clone();
    let mut root = div()
        .relative()
        .when(kind == Kind::ModelTrigger, |root| {
            root.key_context("ModelSelector")
        })
        .capture_any_mouse_down(|_, _, cx| cx.stop_propagation())
        .capture_key_down(move |event, window, cx| {
            let target = key_popup
                .borrow()
                .clone()
                .or_else(|| key_trigger.borrow().clone());
            if let Some(target) = target {
                let _ = target.update(cx, |host, cx| host.key(event, window, cx));
            }
        })
        .on_action(move |_: &ActivateModel, window, cx| {
            if kind != Kind::ModelTrigger {
                return;
            }
            if popup == Some(Kind::Thinking)
                && activation_owner
                    .upgrade()
                    .is_some_and(|owner| owner.read(cx).model_focus.is_focused(window))
            {
                if let Some(owner) = activation_owner.upgrade() {
                    owner.update(cx, |stream, cx| {
                        stream.on_activate_model(&ActivateModel, window, cx)
                    });
                }
                cx.stop_propagation();
                return;
            }
            if let Some(target) = activation_popup
                .borrow()
                .clone()
                .or_else(|| activation_trigger.borrow().clone())
            {
                let _ = target.update(cx, |host, cx| {
                    host.request(
                        host.snapshot.selected.unwrap_or(0),
                        Origin::Keyboard,
                        window,
                        cx,
                    );
                });
            }
            cx.stop_propagation();
        })
        .on_action(move |_: &PreviousModel, window, cx| {
            if kind != Kind::ModelTrigger {
                return;
            }
            if let Some(target) = previous_route.borrow().clone() {
                let _ = target.update(cx, |host, cx| {
                    if host.valid(&host.snapshot, window, cx)
                        && let Some(owner) = host.owner.upgrade()
                    {
                        owner.update(cx, |stream, cx| {
                            stream.on_model_previous(&PreviousModel, window, cx)
                        });
                    }
                });
            }
            cx.stop_propagation();
        })
        .on_action(move |_: &NextModel, window, cx| {
            if kind != Kind::ModelTrigger {
                return;
            }
            if let Some(target) = next_route.borrow().clone() {
                let _ = target.update(cx, |host, cx| {
                    if host.valid(&host.snapshot, window, cx)
                        && let Some(owner) = host.owner.upgrade()
                    {
                        owner.update(cx, |stream, cx| {
                            stream.on_model_next(&NextModel, window, cx)
                        });
                    }
                });
            }
            cx.stop_propagation();
        })
        .on_action(move |_: &CloseModel, window, cx| {
            if kind != Kind::ModelTrigger {
                return;
            }
            if let Some(target) = close_route.borrow().clone() {
                let _ = target.update(cx, |host, cx| {
                    if host.valid(&host.snapshot, window, cx)
                        && let Some(owner) = host.owner.upgrade()
                    {
                        owner.update(cx, |stream, cx| {
                            stream.on_model_close(&CloseModel, window, cx)
                        });
                    }
                });
            }
            cx.stop_propagation();
        })
        .child(Native {
            owner: owner.clone(),
            kind,
            route: trigger_route.clone(),
            paired_host: popup_route.clone(),
        });
    if let Some(kind) = popup {
        root = root.child(
            gpui_kit::deferred(
                div()
                    .absolute()
                    .bottom(gpui_kit::relative(1.))
                    .mb(px(Layout::COMPOSER_PICKER_TRIGGER_GAP))
                    .map(|popup| {
                        if kind == Kind::PermissionMenu {
                            popup.left_0()
                        } else {
                            popup.right_0()
                        }
                    })
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(Native {
                        owner,
                        kind,
                        route: popup_route,
                        paired_host: trigger_route,
                    }),
            )
            .with_priority(2),
        );
    }
    root.into_any_element()
}
