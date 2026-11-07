use super::{
    ArtifactProjection, GitWorkspaceErrorCode, OPEN_TARGETS, OpenInTarget, artifact_path,
    error_label,
};
use crate::conversation_stream::{MONOFONT, ROW_HEIGHT};
use crate::icons::Icon;
use crate::nsh::{OrderedCommands, ProjectionId};
use nsh_core::element::ImageSrc;
use nsh_core::prelude::*;
use tokio::sync::{mpsc, oneshot};
use vega_theme::{Layout, ThemeColors, Typography};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(super) enum Mode {
    Compact,
    Row(usize),
    Group,
}

#[derive(Clone)]
pub(super) struct File {
    pub thread: String,
    pub project: String,
    pub projection: ArtifactProjection,
    pub revision: ProjectionId,
    pub preview: Vec<String>,
    pub error: Option<GitWorkspaceErrorCode>,
    pub opening: Option<OpenInTarget>,
    pub reading: bool,
    pub menu: bool,
    pub capture: u64,
}

impl File {
    fn same(&self, other: &Self) -> bool {
        self.thread == other.thread
            && self.project == other.project
            && self.projection == other.projection
            && self.revision.matches(&other.revision)
            && self.preview == other.preview
            && self.error == other.error
            && self.opening == other.opening
            && self.reading == other.reading
            && self.menu == other.menu
            && self.capture == other.capture
    }
    pub(super) fn policy(&self) -> Policy {
        Policy {
            available: self.projection.current_file_id.is_some(),
            preview: self.projection.preview_available,
            reading: self.reading,
            opening: self.opening.is_some(),
            menu: self.menu,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct Policy {
    pub available: bool,
    pub preview: bool,
    pub reading: bool,
    pub opening: bool,
    pub menu: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum FileAction {
    Preview,
    Menu,
    Open(OpenInTarget),
    Clear,
}

impl Policy {
    pub(super) fn allows(self, action: FileAction, mode: Mode) -> bool {
        match action {
            FileAction::Preview => self.available && self.preview && !self.opening,
            FileAction::Menu => {
                mode != Mode::Row(1) && self.available && !self.opening && !self.reading
            }
            FileAction::Open(target) => {
                OPEN_TARGETS.iter().any(|(fixed, _)| *fixed == target)
                    && self.available
                    && !self.opening
                    && !self.reading
                    && (mode == Mode::Row(1) || self.menu)
            }
            FileAction::Clear => mode == Mode::Row(1) && !self.reading,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Action {
    File(usize, FileAction),
    Review,
    Toggle,
    CloseMenu,
    Reload,
}

#[derive(Clone)]
pub(super) struct Snapshot {
    pub id: ProjectionId,
    pub mount: ProjectionId,
    pub revision: ProjectionId,
    pub mode: Mode,
    pub message: String,
    pub route: (String, String),
    pub files: Vec<File>,
    pub visible: usize,
    pub expanded: bool,
    pub review_reason: Option<&'static str>,
    pub errors: Vec<GitWorkspaceErrorCode>,
    pub colors: ThemeColors,
    pub environment: (&'static str, vega_theme::Appearance),
    pub selected: Option<Action>,
    pub selection: ProjectionId,
    pub focused: bool,
    pub recovery: bool,
    pub menu_surface: Option<ProjectionId>,
}

#[derive(Clone)]
pub(super) struct Control {
    pub action: Action,
    pub label: String,
    pub hint: String,
    pub enabled: bool,
    pub expanded: Option<bool>,
}

impl Snapshot {
    pub(super) fn same(&self, other: &Self) -> bool {
        self.id.matches(&other.id)
            && self.mount.matches(&other.mount)
            && self.revision.matches(&other.revision)
            && self.mode == other.mode
            && self.message == other.message
            && self.route == other.route
            && self.files.len() == other.files.len()
            && self.files.iter().zip(&other.files).all(|(a, b)| a.same(b))
            && self.visible == other.visible
            && self.expanded == other.expanded
            && self.review_reason == other.review_reason
            && self.errors == other.errors
            && self.recovery == other.recovery
            && self.environment == other.environment
            && match (&self.menu_surface, &other.menu_surface) {
                (Some(current), Some(observed)) => current.matches(observed),
                (None, None) => true,
                _ => false,
            }
    }
    pub(super) fn accepts(&self, observed: &Self, action: Action) -> bool {
        self.same(observed)
            && (!self.recovery || action == Action::Reload)
            && match action {
                Action::Reload => self.recovery,
                Action::CloseMenu => self.files.iter().take(self.visible).any(|file| file.menu),
                Action::File(index, FileAction::Clear) => {
                    self.mode == Mode::Row(1)
                        && self
                            .files
                            .get(index)
                            .is_some_and(|file| file.policy().allows(FileAction::Clear, self.mode))
                }
                _ => self
                    .controls()
                    .iter()
                    .any(|control| control.action == action && control.enabled),
            }
    }
    pub(super) fn selection_matches(&self, other: &Self, action: Action) -> bool {
        self.accepts(other, action)
            && self.focused
            && other.focused
            && self.selection.matches(&other.selection)
            && self.selected == Some(action)
            && other.selected == Some(action)
    }
    pub(super) fn controls(&self) -> Vec<Control> {
        if self.recovery {
            return vec![Control {
                action: Action::Reload,
                label: vega_i18n::t("ui.retry").into(),
                hint: String::new(),
                enabled: true,
                expanded: None,
            }];
        }
        let mut controls = Vec::new();
        if self.mode == Mode::Group {
            controls.push(Control {
                action: Action::Review,
                label: vega_i18n::t("ui.open_review").into(),
                hint: vega_i18n::t(
                    self.review_reason
                        .unwrap_or("ui.artifact.open_workspace_changes"),
                )
                .into(),
                enabled: self.review_reason.is_none() && !self.files.is_empty(),
                expanded: None,
            });
        }
        for (index, file) in self.files.iter().take(self.visible).enumerate() {
            if matches!(self.mode, Mode::Row(row) if row != 1) {
                continue;
            }
            if self.mode == Mode::Row(1) && file.reading {
                continue;
            }
            let policy = file.policy();
            let hint = if policy.opening {
                vega_i18n::t("ui.opening_file_in_progress")
            } else {
                vega_i18n::t("ui.file_unavailable")
            };
            controls.push(Control {
                action: Action::File(index, FileAction::Preview),
                label: if self.mode == Mode::Row(1) {
                    vega_i18n::t("ui.preview").into()
                } else {
                    vega_i18n::t_args("ui.preview_name", &[("name", &file.projection.label)])
                },
                hint: if policy.allows(FileAction::Preview, self.mode) {
                    vega_i18n::t("ui.preview_file").into()
                } else {
                    hint.into()
                },
                enabled: policy.allows(FileAction::Preview, self.mode),
                expanded: None,
            });
            if !file.reading {
                if self.mode != Mode::Row(1) {
                    controls.push(Control {
                        action: Action::File(index, FileAction::Menu),
                        label: vega_i18n::t("ui.open_file_in").into(),
                        hint: hint.into(),
                        enabled: policy.allows(FileAction::Menu, self.mode),
                        expanded: Some(file.menu),
                    });
                }
                if self.mode == Mode::Row(1) || file.menu {
                    for (target, label) in OPEN_TARGETS {
                        controls.push(Control {
                            action: Action::File(index, FileAction::Open(target)),
                            label: target_label(target, label).into(),
                            hint: hint.into(),
                            enabled: policy.allows(FileAction::Open(target), self.mode),
                            expanded: None,
                        });
                    }
                }
            }
        }
        if self.mode == Mode::Group && self.files.len() > Layout::ARTIFACT_VISIBLE_FILES {
            controls.push(Control {
                action: Action::Toggle,
                label: if self.expanded {
                    vega_i18n::t("ui.artifact.collapse_files").into()
                } else {
                    vega_i18n::t_args(
                        "ui.artifact.show_more_files",
                        &[("count", &(self.files.len() - self.visible).to_string())],
                    )
                },
                hint: String::new(),
                enabled: true,
                expanded: Some(self.expanded),
            });
        }
        controls
    }
    pub(super) fn enabled(&self) -> Vec<Action> {
        self.controls()
            .into_iter()
            .filter(|control| control.enabled)
            .map(|control| control.action)
            .collect()
    }
    pub(super) fn menu_index(&self) -> Option<usize> {
        (!self.recovery && matches!(self.mode, Mode::Compact | Mode::Group))
            .then(|| {
                self.files
                    .iter()
                    .take(self.visible)
                    .position(|file| file.menu)
            })
            .flatten()
    }
    pub(super) fn menu_top(&self) -> Option<f32> {
        self.menu_index()
            .map(|index| menu_anchor_top(self.mode, index))
    }
    pub(super) fn uses_menu_viewport(&self, action: Action) -> bool {
        self.menu_index().is_some_and(
            |index| matches!(action, Action::File(file, FileAction::Open(_)) if file == index),
        )
    }
    pub(super) fn height(&self) -> f32 {
        match self.mode {
            Mode::Compact => Layout::ARTIFACT_ROW_HEIGHT,
            Mode::Row(1) if self.files.first().is_some_and(|file| file.reading) => 0.,
            Mode::Row(_) => ROW_HEIGHT,
            Mode::Group => {
                let rows = self.visible
                    + usize::from(self.files.len() > Layout::ARTIFACT_VISIBLE_FILES)
                    + self.errors.len();
                if rows == 0 {
                    0.
                } else {
                    Layout::ARTIFACT_BODY_GAP
                        + Layout::ARTIFACT_HEADER_HEIGHT
                        + rows as f32 * Layout::ARTIFACT_ROW_HEIGHT
                        + self.visible.saturating_sub(1) as f32 * Layout::ARTIFACT_FILE_GAP
                        + 2. * Layout::ARTIFACT_BORDER
                }
            }
        }
    }
}

fn target_label(target: OpenInTarget, label: &'static str) -> &'static str {
    match target {
        OpenInTarget::Terminal => vega_i18n::t("ui.terminal"),
        OpenInTarget::DefaultApplication => vega_i18n::t("ui.default"),
        _ => label,
    }
}

pub(super) enum Request {
    Act(Box<Snapshot>, Action),
    Accessible(Box<Snapshot>, Action),
    Displayed(Box<Snapshot>, Action, [f32; 4], u64),
    Reveal(
        Box<Snapshot>,
        Action,
        [f32; 4],
        oneshot::Sender<Option<u64>>,
    ),
    LayoutChanged(Box<Snapshot>),
}

fn glyph(kind: Icon, color: gpui_kit::Rgba) -> AnyElement {
    view()
        .size(px(Layout::ICON_SIZE))
        .flex_shrink_0()
        .child(crate::icons::nsh_source(kind).map(|source| {
            image(ImageSrc::svg_markup(&source))
                .size(px(Layout::ICON_SIZE))
                .tint_color(crate::nsh::color(color))
        }))
        .into_element()
}

fn label(value: String, color: gpui_kit::Rgba) -> AnyElement {
    text(value)
        .text_size(px(Typography::BODY))
        .text_color(crate::nsh::color(color))
        .truncate()
        .into_element()
}

fn hint(value: String, colors: ThemeColors) -> AnyElement {
    view()
        .px_2()
        .py_1()
        .rounded(px(Layout::PANEL_RADIUS))
        .bg(crate::nsh::color(colors.bg_elevated))
        .child(label(value, colors.text_primary))
        .into_element()
}

fn send(commands: &OrderedCommands<Request>, observed: &Snapshot, action: Action) {
    commands.send(Request::Act(Box::new(observed.clone()), action));
}

fn button(
    state: Signal<Snapshot>,
    observed: &Snapshot,
    control: Control,
    commands: OrderedCommands<Request>,
    content: AnyElement,
    target: ElementRef,
) -> AnyElement {
    measured_button(state, observed, control, commands, content, target, None)
}

pub(super) fn horizontal_offset(
    offset: f32,
    left: f32,
    width: f32,
    viewport_left: f32,
    viewport_width: f32,
) -> Option<f32> {
    if ![offset, left, width, viewport_left, viewport_width]
        .iter()
        .all(|value| value.is_finite())
        || width <= 0.
        || viewport_width <= 0.
    {
        return None;
    }
    let delta = if left < viewport_left {
        left - viewport_left
    } else {
        (left + width - viewport_left - viewport_width).max(0.)
    };
    Some((offset + delta).max(0.))
}

fn measured_button(
    state: Signal<Snapshot>,
    observed: &Snapshot,
    control: Control,
    commands: OrderedCommands<Request>,
    content: AnyElement,
    target: ElementRef,
    horizontal: Option<(ElementRef, Signal<f32>)>,
) -> AnyElement {
    let snapshot = observed.clone();
    let action = control.action;
    let colors = observed.colors;
    let enabled = control.enabled;
    let expanded = control.expanded;
    let name = control.label;
    let description = if control.hint.is_empty() {
        name.clone()
    } else {
        control.hint
    };
    let owner = Owner::current();
    let attempts = stored((None::<ProjectionId>, 0usize, false, 0f32));
    let wake = signal(0u64);
    let measurements = commands.clone();
    effect(move || {
        let selected = state.get();
        wake.get();
        let offset = horizontal.map_or(0., |(_, offset)| offset.get());
        if !selected.focused
            || selected.selected != Some(action)
            || !selected.accepts(&snapshot, action)
        {
            return;
        }
        let start = attempts.with(|attempts| {
            if !attempts
                .0
                .as_ref()
                .is_some_and(|id| id.matches(&selected.selection))
            {
                *attempts = (Some(selected.selection.clone()), 0, false, offset);
            }
            if attempts.3 != offset && !attempts.2 {
                attempts.1 = 0;
                attempts.3 = offset;
            }
            if attempts.2 || attempts.1 >= 4 {
                return false;
            }
            attempts.1 += 1;
            attempts.2 = true;
            true
        });
        if !start {
            return;
        }
        let sender = measurements.clone();
        let observed = selected.clone();
        owner.run(move || {
            spawn(async move {
                let measured = target.measure().await;
                let mut horizontally_visible = true;
                if let Some((viewport, offset)) = horizontal {
                    horizontally_visible = false;
                    if let Some(measured) = measured
                        && let Some(viewport_bounds) = viewport.measure().await
                        && state.get_untracked().selection_matches(&observed, action)
                        && let Some(next) = horizontal_offset(
                            offset.get_untracked(),
                            measured.page.x.0,
                            measured.frame.size.width.0,
                            viewport_bounds.page.x.0,
                            viewport_bounds.frame.size.width.0,
                        )
                    {
                        horizontally_visible = (next - offset.get_untracked()).abs() < 0.5;
                        if !horizontally_visible {
                            viewport.scroll_to(
                                nsh_core::geometry::point(
                                    nsh_core::Points(next),
                                    nsh_core::Points(0.),
                                ),
                                false,
                            );
                        }
                    }
                }
                if horizontally_visible
                    && state.get_untracked().selection_matches(&observed, action)
                    && let Some(measured) = measured
                {
                    let bounds = [
                        measured.page.x.0,
                        measured.page.y.0,
                        measured.frame.size.width.0,
                        measured.frame.size.height.0,
                    ];
                    let (reply, result) = oneshot::channel();
                    sender.send(Request::Reveal(
                        Box::new(observed.clone()),
                        action,
                        bounds,
                        reply,
                    ));
                    if let Ok(Some(geometry)) = result.await
                        && state.get_untracked().selection_matches(&observed, action)
                    {
                        sender.send(Request::Displayed(
                            Box::new(observed.clone()),
                            action,
                            bounds,
                            geometry,
                        ));
                        attempts.with(|attempts| {
                            if attempts
                                .0
                                .as_ref()
                                .is_some_and(|id| id.matches(&observed.selection))
                            {
                                attempts.1 = 4;
                            }
                        });
                    }
                }
                let current = attempts.with(|attempts| {
                    if !attempts
                        .0
                        .as_ref()
                        .is_some_and(|id| id.matches(&observed.selection))
                    {
                        return false;
                    }
                    attempts.2 = false;
                    attempts.1 < 4
                });
                if current {
                    wake.update(|wake| *wake = wake.saturating_add(1));
                }
            })
        });
    });
    let accessible_observed = observed.clone();
    let accessible_commands = commands.clone();
    let observed = observed.clone();
    view()
        .element_ref(&target)
        .role(AccessibilityRole::Button)
        .label(name)
        .accessibility_actions(vec![nsh_core::AccessibilityActionDef {
            name: "activate".into(),
            label: None,
        }])
        .on_accessibility_action(move |name: &str| {
            if enabled && name == "activate" {
                accessible_commands.send(Request::Accessible(
                    Box::new(accessible_observed.clone()),
                    action,
                ));
            }
        })
        .hint(description.clone())
        .disabled(!enabled)
        .accessibility_state(move || AccessibilityState {
            selected: state.get().focused && state.get().selected == Some(action),
            disabled: !enabled,
            expanded,
            ..Default::default()
        })
        .min_w_0()
        .h_full()
        .px_2()
        .flex_row()
        .items_center()
        .gap_1()
        .rounded(px(Layout::PANEL_RADIUS))
        .border(px(Layout::ARTIFACT_BORDER))
        .border_color(move || {
            crate::nsh::color(
                if state.get().focused && state.get().selected == Some(action) {
                    colors.accent
                } else {
                    colors.bg_elevated
                },
            )
        })
        .hover(move |style| {
            style.bg(crate::nsh::color(if enabled {
                colors.bg_hover
            } else {
                colors.bg_elevated
            }))
        })
        .tooltip(hint(description, colors))
        .child(content)
        .on_press(move || {
            if enabled {
                send(&commands, &observed, action);
            }
        })
        .into_element()
}

fn control(snapshot: &Snapshot, action: Action) -> Option<Control> {
    snapshot
        .controls()
        .into_iter()
        .find(|control| control.action == action)
}

const COMPACT_ROW_PADDING: f32 = 12.;

pub(super) fn menu_trigger_rect(mode: Mode, index: usize, width: f32) -> [f32; 4] {
    let border = if mode == Mode::Group {
        Layout::ARTIFACT_BORDER
    } else {
        0.
    };
    [
        width - border - COMPACT_ROW_PADDING - Layout::ARTIFACT_ACTION_SIZE,
        menu_anchor_top(mode, index)
            + Layout::ARTIFACT_BORDER
            + (Layout::ARTIFACT_ROW_HEIGHT
                - Layout::ARTIFACT_BORDER
                - Layout::ARTIFACT_ACTION_SIZE)
                / 2.,
        Layout::ARTIFACT_ACTION_SIZE,
        Layout::ARTIFACT_ACTION_SIZE,
    ]
}

fn compact(
    state: Signal<Snapshot>,
    snapshot: &Snapshot,
    index: usize,
    commands: OrderedCommands<Request>,
) -> AnyElement {
    let file = &snapshot.files[index];
    let colors = snapshot.colors;
    let (directory, filename) = artifact_path(&file.projection.label);
    let mut name = view()
        .flex_row()
        .items_center()
        .min_w_0()
        .flex_1()
        .child(glyph(Icon::Document, colors.text_secondary));
    if !directory.is_empty() {
        name = name.child(label(directory.into(), colors.text_secondary));
    }
    name = name.child(label(filename.into(), colors.text_primary));
    let preview = control(snapshot, Action::File(index, FileAction::Preview));
    let mut row = view()
        .h(px(Layout::ARTIFACT_ROW_HEIGHT))
        .min_w_0()
        .w_full()
        .flex_row()
        .items_center()
        .px(px(COMPACT_ROW_PADDING))
        .gap_2()
        .bg(crate::nsh::color(colors.bg_elevated))
        .border_t(px(Layout::ARTIFACT_BORDER))
        .border_color(crate::nsh::color(colors.border_subtle));
    if let Some(preview) = preview {
        let mut details = file.projection.label.clone();
        if let Some(size) = file.projection.size_bytes {
            details = format!("{details} · {}", crate::file_size::format_file_size(size));
        }
        if !preview.enabled {
            details = format!("{details} · {}", preview.hint);
        }
        let mut preview = preview;
        preview.hint = details;
        row = row.child(view().min_w_0().flex_1().h_full().child(button(
            state,
            snapshot,
            preview,
            commands.clone(),
            name.into_element(),
            element_ref(),
        )));
    }
    if let Some(menu) = control(snapshot, Action::File(index, FileAction::Menu)) {
        let anchor = element_ref();
        let active = snapshot.focused
            && matches!(snapshot.selected, Some(Action::File(selected, _)) if selected == index);
        let shown = signal(active || file.menu);
        let show_enter = shown;
        let show_leave = shown;
        let trigger = view()
            .size(px(Layout::ARTIFACT_ACTION_SIZE))
            .flex_shrink_0()
            .opacity(move || if shown.get() { 1_f32 } else { 0_f32 })
            .child(button(
                state,
                snapshot,
                menu,
                commands.clone(),
                glyph(Icon::More, colors.text_secondary),
                anchor,
            ));
        let menu_open = file.menu;
        row = row
            .on_mouse_enter(move |_: &nsh_core::MouseEvent| show_enter.set(true))
            .on_mouse_leave(move |_: &nsh_core::MouseEvent| show_leave.set(active || menu_open))
            .child(trigger);
    }
    row.into_element()
}

pub(super) fn menu_anchor_top(mode: Mode, index: usize) -> f32 {
    match mode {
        Mode::Group => {
            Layout::ARTIFACT_BODY_GAP
                + Layout::ARTIFACT_BORDER
                + Layout::ARTIFACT_HEADER_HEIGHT
                + index as f32 * (Layout::ARTIFACT_ROW_HEIGHT + Layout::ARTIFACT_FILE_GAP)
        }
        _ => 0.,
    }
}

pub(super) fn menu_height() -> f32 {
    OPEN_TARGETS.len() as f32 * Layout::ARTIFACT_ROW_HEIGHT
        + Layout::TOOLBAR_TRAILING_INSET
        + 2. * Layout::ARTIFACT_BORDER
}

fn presentation_state(incoming: Signal<Snapshot>) -> (Signal<Snapshot>, Signal<Snapshot>) {
    let state = signal_with(incoming.get_untracked(), SignalEquals::Never);
    let content = signal_with(incoming.get_untracked(), SignalEquals::Never);
    effect(move || {
        let next = incoming.get();
        let old = content.get_untracked();
        state.set(next.clone());
        if !next.same(&old)
            || next.focused != old.focused
            || next.selected != old.selected
            || !next.selection.matches(&old.selection)
        {
            content.set(next);
        }
    });
    (state, content)
}

pub(super) fn render_menu(
    incoming: Signal<Snapshot>,
    commands: mpsc::Sender<Request>,
) -> AnyElement {
    let (state, content) = presentation_state(incoming);
    let commands = OrderedCommands::new(commands);
    view()
        .w_full()
        .h_full()
        .child(dynamic(move || {
            let snapshot = content.get();
            let Some(index) = snapshot.menu_index() else {
                return view().into_element();
            };
            let colors = snapshot.colors;
            let mut popup = view()
                .w_full()
                .h_full()
                .py(px(Layout::TOOLBAR_TRAILING_INSET / 2.))
                .bg(crate::nsh::color(colors.bg_elevated))
                .rounded(px(Layout::PANEL_RADIUS))
                .border(px(Layout::ARTIFACT_BORDER))
                .border_color(crate::nsh::color(colors.border_subtle));
            for (target, _) in OPEN_TARGETS {
                if let Some(control) =
                    control(&snapshot, Action::File(index, FileAction::Open(target)))
                {
                    let content = label(control.label.clone(), colors.text_primary);
                    popup = popup.child(view().h(px(Layout::ARTIFACT_ROW_HEIGHT)).child(button(
                        state,
                        &snapshot,
                        control,
                        commands.clone(),
                        content,
                        element_ref(),
                    )));
                }
            }
            popup.into_element()
        }))
        .into_element()
}

pub(super) fn render(incoming: Signal<Snapshot>, commands: mpsc::Sender<Request>) -> AnyElement {
    let (state, content) = presentation_state(incoming);
    let commands = OrderedCommands::new(commands);
    view()
        .w_full()
        .h_full()
        .child(dynamic(move || {
            let snapshot = content.get();
            let colors = snapshot.colors;
            if snapshot.recovery {
                let mut root = view().h_full().flex_row().items_center().px_3();
                for control in snapshot.controls() {
                    let child = label(control.label.clone(), colors.text_primary);
                    root = root.child(button(
                        state,
                        &snapshot,
                        control,
                        commands.clone(),
                        child,
                        element_ref(),
                    ));
                }
                return root.into_element();
            }
            match snapshot.mode {
                Mode::Compact => compact(state, &snapshot, 0, commands.clone()),
                Mode::Row(row) => {
                    let Some(file) = snapshot.files.first() else {
                        return view().into_element();
                    };
                    let mut root = view()
                        .h(px(if row == 1 && file.reading {
                            0.
                        } else {
                            ROW_HEIGHT
                        }))
                        .w_full()
                        .min_w_0()
                        .px_3()
                        .bg(crate::nsh::color(
                            if row >= 2 && row - 2 < file.preview.len() {
                                colors.code_bg
                            } else {
                                colors.bg_elevated
                            },
                        ));
                    if row == 0 {
                        root = root.child(
                            text(file.projection.label.clone())
                                .font_family(MONOFONT.to_string())
                                .text_size(px(Typography::CODE))
                                .text_color(crate::nsh::color(colors.text_primary))
                                .truncate(),
                        );
                    } else if row == 1 && !file.reading {
                        let viewport = element_ref();
                        let offset = signal(0f32);
                        let mut actions = view().flex_row().gap_1().h_full().flex_shrink_0();
                        for control in snapshot.controls() {
                            let color = if control.enabled {
                                colors.text_primary
                            } else {
                                colors.text_tertiary
                            };
                            let child = label(control.label.clone(), color);
                            actions = actions.child(measured_button(
                                state,
                                &snapshot,
                                control,
                                commands.clone(),
                                child,
                                element_ref(),
                                Some((viewport, offset)),
                            ));
                        }
                        let observed = snapshot.clone();
                        let scroll_commands = commands.clone();
                        root = root.child(
                            scroll()
                                .horizontal()
                                .element_ref(&viewport)
                                .w_full()
                                .h_full()
                                .on_scroll(move |event: &ScrollEvent| {
                                    offset.set(event.offset.x.0);
                                    scroll_commands
                                        .send(Request::LayoutChanged(Box::new(observed.clone())));
                                })
                                .child(actions),
                        );
                    } else if row >= 2 {
                        let (value, color) = match file.preview.get(row - 2) {
                            Some(line) => (line.clone(), colors.text_primary),
                            None => (error_label(file.error), colors.danger),
                        };
                        root = root.child(
                            text(value)
                                .font_family(MONOFONT.to_string())
                                .text_size(px(Typography::CODE))
                                .text_color(crate::nsh::color(color))
                                .truncate(),
                        );
                    }
                    root.into_element()
                }
                Mode::Group => {
                    if snapshot.height() == 0. {
                        return view().into_element();
                    }
                    let title = if snapshot.files.len() == 1 {
                        vega_i18n::t_args(
                            "ui.artifact.edited_file",
                            &[("name", artifact_path(&snapshot.files[0].projection.label).1)],
                        )
                    } else {
                        vega_i18n::t_args(
                            "ui.artifact.edited_files",
                            &[("count", &snapshot.files.len().to_string())],
                        )
                    };
                    let mut header = view()
                        .h(px(Layout::ARTIFACT_HEADER_HEIGHT))
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .px_3()
                        .child(glyph(Icon::Document, colors.text_secondary))
                        .child(
                            view().min_w_0().flex_1().child(
                                text(title)
                                    .text_size(px(Typography::HEADING_CARD))
                                    .text_color(crate::nsh::color(colors.text_primary))
                                    .truncate(),
                            ),
                        );
                    if let Some(review) = control(&snapshot, Action::Review) {
                        let child = label(
                            review.label.clone(),
                            if review.enabled {
                                colors.text_primary
                            } else {
                                colors.text_tertiary
                            },
                        );
                        header =
                            header.child(view().h(px(Layout::ARTIFACT_ACTION_SIZE)).child(button(
                                state,
                                &snapshot,
                                review,
                                commands.clone(),
                                child,
                                element_ref(),
                            )));
                    }
                    let mut body = view()
                        .w_full()
                        .min_w_0()
                        .bg(crate::nsh::color(colors.bg_elevated))
                        .border(px(Layout::ARTIFACT_BORDER))
                        .border_color(crate::nsh::color(colors.border_subtle))
                        .rounded(px(Layout::TOOL_DETAIL_RADIUS))
                        .child(header);
                    for index in 0..snapshot.visible {
                        let mut row = view().w_full().min_w_0();
                        if index > 0 {
                            row = row.mt(px(Layout::ARTIFACT_FILE_GAP));
                        }
                        body = body.child(row.child(compact(
                            state,
                            &snapshot,
                            index,
                            commands.clone(),
                        )));
                    }
                    if let Some(toggle) = control(&snapshot, Action::Toggle) {
                        let child = view()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .child(label(toggle.label.clone(), colors.text_secondary))
                            .child(
                                view()
                                    .rotate(Radians(if snapshot.expanded {
                                        std::f32::consts::PI
                                    } else {
                                        0.
                                    }))
                                    .child(glyph(Icon::ChevronDown, colors.text_secondary)),
                            )
                            .into_element();
                        body = body.child(
                            view()
                                .h(px(Layout::ARTIFACT_ROW_HEIGHT))
                                .border_t(px(Layout::ARTIFACT_BORDER))
                                .border_color(crate::nsh::color(colors.border_subtle))
                                .child(button(
                                    state,
                                    &snapshot,
                                    toggle,
                                    commands.clone(),
                                    child,
                                    element_ref(),
                                )),
                        );
                    }
                    for error in &snapshot.errors {
                        body = body.child(
                            view()
                                .h(px(Layout::ARTIFACT_ROW_HEIGHT))
                                .px_3()
                                .flex_row()
                                .items_center()
                                .border_t(px(Layout::ARTIFACT_BORDER))
                                .border_color(crate::nsh::color(colors.border_subtle))
                                .child(label(error_label(Some(*error)), colors.danger)),
                        );
                    }
                    view()
                        .pt(px(Layout::ARTIFACT_BODY_GAP))
                        .child(body)
                        .into_element()
                }
            }
        }))
        .into_element()
}
