use std::cell::{Cell, RefCell};
use std::ops::Range;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use gpui_kit::{
    Context, Entity, InteractiveElement as _, IntoElement as _, ParentElement as _, Styled as _,
    Subscription, Window,
};
use nsh_core::prelude::*;
use tokio::sync::{mpsc, oneshot, watch};
use vega_conversation::attachments::{MAX_IMAGE_BYTES, MAX_IMAGES, MAX_TURN_IMAGE_BYTES};
use vega_theme::Typography;

use super::{AcceptPrediction, DismissPrediction, Redo, TextInput, Undo};

#[derive(Clone)]
pub(crate) struct EditorSnapshot {
    pub revision: u64,
    pub text: Arc<str>,
    pub placeholder: String,
    pub label: String,
    pub selection: (u32, u32),
    pub composing: bool,
    pub multiline: bool,
    pub min_rows: usize,
    pub max_rows: usize,
    pub prediction: Option<String>,
    pub selection_revision: u64,
}

#[derive(Clone, PartialEq, Eq)]
struct EditorStamp {
    revision: u64,
    selection_revision: u64,
    text: Arc<str>,
    selection: (u32, u32),
}

impl EditorStamp {
    fn from_snapshot(snapshot: &EditorSnapshot) -> Self {
        Self {
            revision: snapshot.revision,
            selection_revision: snapshot.selection_revision,
            text: snapshot.text.clone(),
            selection: snapshot.selection,
        }
    }
}

#[derive(Clone)]
pub(crate) struct EditorGate(watch::Sender<Option<EditorStamp>>);

impl EditorGate {
    pub(crate) fn new() -> Self {
        Self(watch::channel(None).0)
    }

    fn close(&self) {
        self.0.send_if_modified(|stamp| stamp.take().is_some());
    }

    fn open(&self, snapshot: &EditorSnapshot) {
        let next = EditorStamp::from_snapshot(snapshot);
        self.0.send_if_modified(|stamp| {
            if stamp.as_ref() == Some(&next) {
                false
            } else {
                *stamp = Some(next);
                true
            }
        });
    }

    pub(crate) fn matches(&self, input: &TextInput) -> bool {
        !input.is_composing()
            && input.nsh_snapshot().is_some_and(|snapshot| {
                self.0.borrow().as_ref() == Some(&EditorStamp::from_snapshot(&snapshot))
            })
    }
}

pub(crate) enum EditorOperation {
    Replace {
        before: Arc<str>,
        after: String,
    },
    Select {
        text: Arc<str>,
        range: (u32, u32),
    },
    Compose {
        text: Arc<str>,
        active: bool,
    },
    Paste {
        before: Arc<str>,
        after: String,
        selection: (u32, u32),
        payload: Arc<nsh_core::PasteEvent>,
    },
    Undo,
    Redo,
    AcceptPrediction,
    DismissPrediction,
}

pub(crate) struct EditorRequest {
    pub revision: u64,
    pub operation: EditorOperation,
    pub selection_revision: u64,
    pub reply: oneshot::Sender<EditorOutcome>,
}

pub(crate) struct EditorOutcome {
    pub accepted: bool,
    pub snapshot: Option<EditorSnapshot>,
    pub selection_revision: u64,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ComposerRoute {
    pub files: bool,
    pub file_retry: bool,
    pub actions: bool,
    pub file_generation: u64,
    pub file_highlight: usize,
    pub action_generation: u64,
    pub action_highlight: usize,
    pub has_history: bool,
    pub recalling_history: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ComposerKey {
    Send,
    Previous,
    History,
    Next,
    Accept,
    Cancel,
    FocusNext,
    FocusPrevious,
}

pub(crate) struct ComposerShortcut {
    stamp: EditorStamp,
    route: ComposerRoute,
    pub key: ComposerKey,
}

impl gpui_kit::EventEmitter<ComposerShortcut> for TextInput {}

impl ComposerShortcut {
    pub(crate) fn matches(&self, input: &TextInput, route: ComposerRoute) -> bool {
        self.route == route
            && input.composer_editor_ready()
            && input
                .nsh_snapshot()
                .is_some_and(|snapshot| self.stamp == EditorStamp::from_snapshot(&snapshot))
    }
}

#[derive(Clone, PartialEq, Eq)]
struct ShortcutIntent {
    text: String,
    selection: (u32, u32),
    generation: u64,
    route: ComposerRoute,
    key: ComposerKey,
}

struct ShortcutRequest {
    event: ComposerShortcut,
    reply: oneshot::Sender<()>,
}

#[derive(Clone)]
struct DraftRecovery(Arc<Mutex<RecoveryState>>);

struct RecoveryState {
    local: String,
    saved: Option<String>,
}

impl DraftRecovery {
    fn new(text: String) -> Self {
        Self(Arc::new(Mutex::new(RecoveryState {
            local: text,
            saved: None,
        })))
    }

    fn local(&self, text: &str) {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .local = text.to_owned();
    }

    fn saved(&self) -> Option<String> {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .saved
            .clone()
    }

    fn save(&self, text: String) {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .saved = Some(text);
    }

    fn preserve(&self) {
        let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        state.saved = Some(state.local.clone());
    }

    fn restarted(&self, text: String) -> Self {
        Self(Arc::new(Mutex::new(RecoveryState {
            local: text,
            saved: self.saved(),
        })))
    }
}

#[derive(Clone)]
struct RecoveryProjection {
    id: crate::nsh::ProjectionId,
    draft: DraftRecovery,
}

struct ReloadEditor {
    id: crate::nsh::ProjectionId,
    reply: oneshot::Sender<bool>,
}

fn render_host_recovery(
    state: Signal<RecoveryProjection>,
    commands: mpsc::Sender<ReloadEditor>,
) -> AnyElement {
    let pending = signal(false);
    let failed = signal(false);
    view()
        .w_full()
        .gap_2()
        .child(text(move || {
            state.with(|_| vega_i18n::t("ui.editor_sync_failed"))
        }))
        .child(
            nsh_base::button()
                .on_press(move || {
                    if let Some(text) = state.get_untracked().draft.saved() {
                        set_clipboard(text);
                    }
                })
                .child(text(move || {
                    state.with(|_| vega_i18n::t("ui.copy_local_draft"))
                })),
        )
        .child(
            nsh_base::button()
                .disabled(pending)
                .on_press(move || {
                    if pending.get_untracked() {
                        return;
                    }
                    pending.set(true);
                    let commands = commands.clone();
                    let id = state.get_untracked().id;
                    spawn(async move {
                        let (reply, response) = oneshot::channel();
                        if commands.send(ReloadEditor { id, reply }).await.is_err()
                            || !matches!(response.await, Ok(true))
                        {
                            failed.set(true);
                            pending.set(false);
                        }
                    });
                })
                .child(text(move || {
                    state.with(|_| vega_i18n::t("settings.action.reload"))
                })),
        )
        .child(when(
            move || failed.get(),
            move || text(vega_i18n::t("ui.command_failed")),
        ))
        .into_element()
}

#[derive(Clone)]
struct ComposerProjection {
    snapshot: EditorSnapshot,
    gate: EditorGate,
    focus_revision: u64,
    height: watch::Sender<f32>,
    shortcuts: mpsc::Sender<ShortcutRequest>,
    route: ComposerRoute,
    host_failed: bool,
    draft: DraftRecovery,
}

pub(super) struct ComposerEditor {
    id: crate::nsh::ProjectionId,
    draft: DraftRecovery,
    recovery_page: Option<crate::nsh::Surface<RecoveryProjection, ReloadEditor>>,
    restarting: bool,
    surface: crate::nsh::Surface<ComposerProjection, EditorRequest>,
    gate: EditorGate,
    focus_revision: u64,
    _focus_subscription: Subscription,
    height: watch::Receiver<f32>,
    height_sender: watch::Sender<f32>,
    shortcuts: mpsc::Sender<ShortcutRequest>,
    route: ComposerRoute,
}

fn render_composer_editor(
    state: Signal<ComposerProjection>,
    commands: mpsc::Sender<EditorRequest>,
) -> AnyElement {
    let initial = state.get_untracked();
    let snapshot = signal_with(initial.snapshot.clone(), SignalEquals::Never);
    let focus_revision = signal(initial.focus_revision);
    let route = signal(initial.route);
    let host_failed = signal(initial.host_failed);
    effect(move || {
        let projection = state.get();
        snapshot.set(projection.snapshot);
        focus_revision.set(projection.focus_revision);
        route.set(projection.route);
        host_failed.set(projection.host_failed);
    });
    let editor = editor(
        snapshot,
        commands,
        focus_revision,
        route,
        host_failed,
        &initial,
    );
    let height = initial.height;
    let measured = Cell::new(0.0f32);
    let failed = editor.failed;
    let synchronized = editor.synchronized;
    cleanup(move || initial.gate.close());
    view()
        .w_full()
        .accessibility_state(move || AccessibilityState {
            busy: !failed.get() && !synchronized.get(),
            ..Default::default()
        })
        .on_layout(move |bounds: &Bounds<Points>| {
            let next = bounds.size.height.0;
            if next.is_finite() && next > 0. && (measured.get() - next).abs() > 0.5 {
                measured.set(next);
                height.send_replace(next);
            }
        })
        .child(editor.element)
        .into_element()
}

impl TextInput {
    pub fn render_nsh_preview(
        input: Entity<Self>,
        window: &mut Window,
        cx: &mut gpui_kit::App,
    ) -> gpui_kit::AnyElement {
        Self::render_nsh_composer(input, ComposerRoute::default(), window, cx)
    }

    pub(crate) fn composer_editor_ready(&self) -> bool {
        self.nsh_composer.as_ref().is_none_or(|editor| {
            !editor.restarting
                && editor.surface.errors().borrow().is_none()
                && editor.gate.matches(self)
        })
    }

    pub(crate) fn composer_contains_focus(&self, window: &Window, cx: &gpui_kit::App) -> bool {
        self.focus_handle.contains_focused(window, cx)
    }

    pub(crate) fn render_nsh_composer(
        input: Entity<Self>,
        route: ComposerRoute,
        window: &mut Window,
        cx: &mut gpui_kit::App,
    ) -> gpui_kit::AnyElement {
        input.update(cx, |this, cx| {
            let Some(snapshot) = this
                .nsh_snapshot()
                .filter(|_| this.image_paste && this.multiline)
            else {
                return gpui_kit::div().into_any_element();
            };
            let restarting = this
                .nsh_composer
                .as_ref()
                .is_some_and(|editor| editor.restarting);
            let retained = restarting
                .then(|| this.nsh_composer.take())
                .flatten()
                .map(|editor| {
                    editor.gate.close();
                    editor.surface.entity().read(cx).stop();
                    if let Some(page) = &editor.recovery_page {
                        page.entity().read(cx).stop();
                    }
                    editor.draft.restarted(snapshot.text.to_string())
                });
            if this.nsh_composer.is_none() {
                let id = crate::nsh::ProjectionId::new();
                let draft =
                    retained.unwrap_or_else(|| DraftRecovery::new(snapshot.text.to_string()));
                let gate = EditorGate::new();
                let mut updates = gate.0.subscribe();
                let (height_tx, height) = watch::channel(
                    Typography::BODY * Typography::BODY_LINE_HEIGHT * snapshot.min_rows as f32,
                );
                let mut sizes = height.clone();
                let (shortcuts, mut shortcut_requests) =
                    mpsc::channel::<ShortcutRequest>(crate::nsh::COMMAND_CAPACITY);
                let initial_focus = u64::from(restarting || this.focus_handle.is_focused(window));
                let initial = ComposerProjection {
                    snapshot: snapshot.clone(),
                    gate: gate.clone(),
                    focus_revision: initial_focus,
                    height: height_tx.clone(),
                    shortcuts: shortcuts.clone(),
                    route,
                    host_failed: false,
                    draft: draft.clone(),
                };
                let (surface, mut requests) =
                    crate::nsh::Surface::new(initial, render_composer_editor, window, cx);
                let mut errors = surface.errors();
                let error_gate = gate.clone();
                let error_draft = draft.clone();
                cx.spawn(async move |this, cx| {
                    loop {
                        if errors.borrow_and_update().is_some() {
                            error_gate.close();
                            error_draft.preserve();
                            if this.update(cx, |_, cx| cx.notify()).is_err() {
                                break;
                            }
                        }
                        if errors.changed().await.is_err() {
                            break;
                        }
                    }
                })
                .detach();
                let focus_subscription =
                    cx.on_focus(&this.focus_handle, window, |this, window, cx| {
                        let snapshot = this.nsh_snapshot();
                        if let Some(editor) = &mut this.nsh_composer
                            && let Some(snapshot) = snapshot
                        {
                            editor.focus_revision = editor.focus_revision.wrapping_add(1);
                            editor.surface.publish(ComposerProjection {
                                snapshot,
                                gate: editor.gate.clone(),
                                focus_revision: editor.focus_revision,
                                height: editor.height_sender.clone(),
                                shortcuts: editor.shortcuts.clone(),
                                route: editor.route,
                                host_failed: editor.surface.errors().borrow().is_some(),
                                draft: editor.draft.clone(),
                            });
                            if let Some(page) = &editor.recovery_page {
                                page.focus(window, cx);
                            } else if editor.surface.errors().borrow().is_none() {
                                editor.surface.focus(window, cx);
                            }
                        }
                    });
                this.nsh_composer = Some(ComposerEditor {
                    id: id.clone(),
                    draft,
                    recovery_page: None,
                    restarting: false,
                    surface,
                    gate,
                    focus_revision: initial_focus,
                    _focus_subscription: focus_subscription,
                    height,
                    height_sender: height_tx,
                    shortcuts,
                    route,
                });
                let shortcut_id = id.clone();
                cx.spawn(async move |this, cx| {
                    while let Some(request) = shortcut_requests.recv().await {
                        if this
                            .update(cx, |this, cx| {
                                let route = this
                                    .nsh_composer
                                    .as_ref()
                                    .map_or(ComposerRoute::default(), |editor| editor.route);
                                if this
                                    .nsh_composer
                                    .as_ref()
                                    .is_some_and(|editor| editor.id.matches(&shortcut_id))
                                    && request.event.matches(this, route)
                                {
                                    cx.emit(request.event);
                                }
                                let _ = request.reply.send(());
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                })
                .detach();
                cx.spawn(async move |this, cx| {
                    while sizes.changed().await.is_ok() {
                        if this.update(cx, |_, cx| cx.notify()).is_err() {
                            break;
                        }
                    }
                })
                .detach();
                cx.spawn_in(window, async move |this, cx| {
                    while let Some(request) = requests.recv().await {
                        if this
                            .update_in(cx, |this, window, cx| {
                                if this.nsh_composer.as_ref().is_some_and(|editor| {
                                    editor.id.matches(&id)
                                        && !editor.restarting
                                        && editor.surface.errors().borrow().is_none()
                                }) {
                                    this.apply_nsh_request(request, window, cx);
                                } else {
                                    let _ = request.reply.send(EditorOutcome {
                                        accepted: false,
                                        snapshot: this.nsh_snapshot(),
                                        selection_revision: this.nsh_selection_revision,
                                    });
                                }
                                cx.notify();
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                })
                .detach();
                cx.spawn(async move |this, cx| {
                    while updates.changed().await.is_ok() {
                        if this.update(cx, |_, cx| cx.notify()).is_err() {
                            break;
                        }
                    }
                })
                .detach();
            }
            let focus = this.focus_handle.clone();
            let Some(editor) = this.nsh_composer.as_mut() else {
                return gpui_kit::div().into_any_element();
            };
            editor.route = route;
            editor.surface.sync_environment(cx);
            editor.surface.publish(ComposerProjection {
                snapshot,
                gate: editor.gate.clone(),
                focus_revision: editor.focus_revision,
                height: editor.height_sender.clone(),
                shortcuts: editor.shortcuts.clone(),
                route: editor.route,
                host_failed: editor.surface.errors().borrow().is_some(),
                draft: editor.draft.clone(),
            });
            if editor.surface.errors().borrow().is_some() {
                editor.gate.close();
                if editor.recovery_page.is_none() {
                    editor.draft.preserve();
                    let (page, mut reloads) = crate::nsh::Surface::new(
                        RecoveryProjection {
                            id: editor.id.clone(),
                            draft: editor.draft.clone(),
                        },
                        render_host_recovery,
                        window,
                        cx,
                    );
                    page.focus(window, cx);
                    editor.recovery_page = Some(page);
                    cx.spawn(async move |this, cx| {
                        while let Some(request) = reloads.recv().await {
                            if this
                                .update(cx, |this, cx| {
                                    let accepted =
                                        this.nsh_composer.as_mut().is_some_and(|editor| {
                                            if !editor.id.matches(&request.id) || editor.restarting
                                            {
                                                return false;
                                            }
                                            editor.gate.close();
                                            editor.restarting = true;
                                            true
                                        });
                                    let _ = request.reply.send(accepted);
                                    if accepted {
                                        this.marked_range = None;
                                        this.composition_history_active = false;
                                        this.nsh_selection_revision =
                                            this.nsh_selection_revision.wrapping_add(1);
                                        cx.notify();
                                    }
                                })
                                .is_err()
                            {
                                break;
                            }
                        }
                    })
                    .detach();
                }
                if let Some(page) = &mut editor.recovery_page {
                    page.sync_environment(cx);
                    return gpui_kit::div()
                        .w_full()
                        .track_focus(&focus)
                        .child(page.entity())
                        .into_any_element();
                }
            }
            gpui_kit::div()
                .w_full()
                .track_focus(&focus)
                .h(gpui_kit::px(*editor.height.borrow()))
                .child(editor.surface.entity())
                .into_any_element()
        })
    }

    pub(crate) fn nsh_snapshot(&self) -> Option<EditorSnapshot> {
        if self.masked {
            return None;
        }
        let selection = self.range_to_utf16(&self.selected_range);
        Some(EditorSnapshot {
            revision: self.content_revision,
            text: Arc::from(self.text()),
            placeholder: self.localized_placeholder().to_string(),
            label: self.accessible_label(),
            selection: if self.selection_reversed {
                (utf16_index(selection.end), utf16_index(selection.start))
            } else {
                (utf16_index(selection.start), utf16_index(selection.end))
            },
            composing: self.is_composing(),
            multiline: self.multiline,
            min_rows: self.min_rows,
            max_rows: self.max_rows,
            prediction: self.prediction_text().map(str::to_owned),
            selection_revision: self.nsh_selection_revision,
        })
    }

    pub(crate) fn apply_nsh_request(
        &mut self,
        request: EditorRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let accepted = !self.masked
            && self.content_revision == request.revision
            && self.nsh_selection_revision == request.selection_revision
            && self.apply_nsh_operation(request.operation, window, cx);
        let _ = request.reply.send(EditorOutcome {
            accepted,
            snapshot: self.nsh_snapshot(),
            selection_revision: self.nsh_selection_revision,
        });
    }

    fn apply_nsh_operation(
        &mut self,
        operation: EditorOperation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        match operation {
            EditorOperation::Replace { before, after } => {
                if before.as_ref() != self.text() {
                    return false;
                }
                if before.as_ref() != after {
                    let (range, inserted) = replacement(&before, &after);
                    let composing = self.is_composing();
                    self.record_edit();
                    self.apply_edit(range, &after[inserted]);
                    if composing {
                        self.marked_range = Some(self.selected_range.clone());
                    }
                    cx.notify();
                }
            }
            EditorOperation::Select { text, range } => {
                if text.as_ref() != self.text() {
                    return false;
                }
                let selection = self.range_from_utf16(
                    &(range.0.min(range.1) as usize..range.0.max(range.1) as usize),
                );
                self.selected_range = self.clamp_range(selection);
                self.selection_reversed = range.0 > range.1;
                self.vertical_goal = None;
                self.update_logical_viewport();
                cx.notify();
            }
            EditorOperation::Compose { text, active } => {
                if text.as_ref() != self.text() {
                    return false;
                }
                if active && !self.is_composing() {
                    self.record_edit();
                }
                self.marked_range = active.then(|| self.selected_range.clone());
                self.composition_history_active = active;
                self.prediction = None;
                cx.notify();
            }
            EditorOperation::Paste {
                before,
                after,
                selection,
                payload,
            } => {
                if before.as_ref() != self.text() || !self.image_paste {
                    return false;
                }
                let Some(item) = pasted_attachments(&payload) else {
                    return false;
                };
                if before.as_ref() != after {
                    let (range, inserted) = replacement(&before, &after);
                    self.record_edit();
                    self.apply_edit(range, &after[inserted]);
                }
                self.selected_range = self.range_from_utf16(
                    &(selection.0.min(selection.1) as usize..selection.0.max(selection.1) as usize),
                );
                self.selection_reversed = selection.0 > selection.1;
                self.marked_range = None;
                self.composition_history_active = false;
                if !item.entries.is_empty() {
                    cx.emit(super::ImagePaste(Arc::new(item)));
                }
                cx.notify();
            }
            EditorOperation::Undo => self.undo(&Undo, window, cx),
            EditorOperation::Redo => self.redo(&Redo, window, cx),
            EditorOperation::AcceptPrediction => {
                self.accept_prediction(&AcceptPrediction, window, cx);
            }
            EditorOperation::DismissPrediction => {
                self.dismiss_prediction(&DismissPrediction, window, cx);
            }
        }
        self.nsh_selection_revision = self.nsh_selection_revision.wrapping_add(1);
        true
    }
}

fn pasted_attachments(payload: &nsh_core::PasteEvent) -> Option<gpui_kit::ClipboardItem> {
    if payload.files.len().saturating_add(payload.uris.len()) > MAX_IMAGES
        || payload
            .files
            .iter()
            .any(|file| file.data.len() > MAX_IMAGE_BYTES)
        || payload
            .files
            .iter()
            .fold(0usize, |total, file| total.saturating_add(file.data.len()))
            > MAX_TURN_IMAGE_BYTES
    {
        return None;
    }
    let mut entries = Vec::new();
    for file in &payload.files {
        let format = match nsh_core::platform::clipboard::image_mime(&file.data)? {
            "image/png" => gpui_kit::ImageFormat::Png,
            "image/jpeg" => gpui_kit::ImageFormat::Jpeg,
            "image/webp" => gpui_kit::ImageFormat::Webp,
            _ => return None,
        };
        entries.push(gpui_kit::ClipboardEntry::Image(
            gpui_kit::Image::from_bytes(format, file.data.clone()),
        ));
    }
    if !payload.uris.is_empty() {
        let paths = payload
            .uris
            .iter()
            .map(|uri| pasted_path(uri))
            .collect::<Option<Vec<_>>>()?;
        entries.push(gpui_kit::ClipboardEntry::ExternalPaths(
            gpui_kit::ExternalPaths(paths.into_iter().collect()),
        ));
    }
    Some(gpui_kit::ClipboardItem { entries })
}

fn pasted_path(uri: &str) -> Option<std::path::PathBuf> {
    if uri.len() > 32 * 1024 {
        return None;
    }
    let rest = uri.strip_prefix("file://")?;
    let (host, encoded) = rest.split_once('/')?;
    let mut decoded = Vec::with_capacity(encoded.len());
    let mut bytes = encoded.bytes();
    while let Some(byte) = bytes.next() {
        let byte = if byte == b'%' {
            let high = (bytes.next()? as char).to_digit(16)?;
            let low = (bytes.next()? as char).to_digit(16)?;
            u8::try_from(high * 16 + low).ok()?
        } else {
            if matches!(byte, b'?' | b'#') {
                return None;
            }
            byte
        };
        if byte == 0 {
            return None;
        }
        decoded.push(byte);
    }
    let decoded = String::from_utf8(decoded).ok()?;
    let path = if cfg!(target_os = "windows") {
        if host.is_empty() || host.eq_ignore_ascii_case("localhost") {
            decoded
        } else {
            format!("//{host}/{decoded}")
        }
    } else {
        if !host.is_empty() && !host.eq_ignore_ascii_case("localhost") {
            return None;
        }
        format!("/{decoded}")
    };
    let path = std::path::PathBuf::from(path);
    path.is_absolute().then_some(path)
}

fn utf16_offset(text: &str, offset: u32) -> usize {
    let mut units = 0u32;
    for (index, ch) in text.char_indices() {
        units = units.saturating_add(ch.len_utf16() as u32);
        if units > offset {
            return index;
        }
    }
    text.len()
}

fn utf16_index(index: usize) -> u32 {
    u32::try_from(index).unwrap_or(u32::MAX)
}

fn replacement(before: &str, after: &str) -> (Range<usize>, Range<usize>) {
    let prefix = before
        .chars()
        .zip(after.chars())
        .take_while(|(left, right)| left == right)
        .map(|(ch, _)| ch.len_utf8())
        .sum::<usize>();
    let suffix = before[prefix..]
        .chars()
        .rev()
        .zip(after[prefix..].chars().rev())
        .take_while(|(left, right)| left == right)
        .map(|(ch, _)| ch.len_utf8())
        .sum::<usize>();
    (prefix..before.len() - suffix, prefix..after.len() - suffix)
}

#[derive(Clone)]
struct Dispatch {
    commands: mpsc::Sender<EditorRequest>,
    previous: Rc<RefCell<Option<oneshot::Receiver<()>>>>,
    revision: Signal<u64>,
    selection_revision: Signal<u64>,
    pending: Signal<usize>,
    pending_bytes: Signal<usize>,
    pending_attachment_bytes: Signal<usize>,
    recovery: Signal<Option<String>>,
    draft: DraftRecovery,
    value: Signal<String>,
    composing: Signal<bool>,
    selection: Signal<(u32, u32)>,
    failed: Signal<bool>,
    generation: Signal<u64>,
    gate: EditorGate,
}

impl Dispatch {
    fn set_value(&self, text: String) {
        self.draft.local(&text);
        self.value.set(text);
    }

    fn fail(&self) {
        self.gate.close();
        let text = self.value.get_untracked();
        self.draft.save(text.clone());
        self.recovery.set(Some(text));
        self.failed.set(true);
        self.generation
            .update(|generation| *generation = generation.wrapping_add(1));
    }

    fn send(&self, operation: EditorOperation) {
        self.gate.close();
        if self.failed.get_untracked() {
            return;
        }
        let bytes = match &operation {
            EditorOperation::Replace { before, after }
            | EditorOperation::Paste { before, after, .. } => {
                before.len().saturating_add(after.len())
            }
            EditorOperation::Select { text, .. } | EditorOperation::Compose { text, .. } => {
                text.len()
            }
            _ => 0,
        };
        let attachment_bytes = match &operation {
            EditorOperation::Paste { payload, .. } => payload
                .files
                .iter()
                .fold(0usize, |total, file| total.saturating_add(file.data.len())),
            _ => 0,
        };
        if self.pending.get_untracked() >= crate::nsh::COMMAND_CAPACITY
            || self.pending_bytes.get_untracked().saturating_add(bytes) > 2 * 1024 * 1024
            || self
                .pending_attachment_bytes
                .get_untracked()
                .saturating_add(attachment_bytes)
                > MAX_TURN_IMAGE_BYTES
        {
            self.fail();
            return;
        }
        let (done, previous) = oneshot::channel();
        let predecessor = self.previous.borrow_mut().replace(previous);
        let dispatch = self.clone();
        let generation = self.generation.get_untracked();
        self.pending.update(|pending| *pending += 1);
        self.pending_bytes.update(|pending| *pending += bytes);
        self.pending_attachment_bytes
            .update(|pending| *pending += attachment_bytes);
        spawn(async move {
            if let Some(predecessor) = predecessor {
                let _ = predecessor.await;
            }
            if generation == dispatch.generation.get_untracked() && !dispatch.failed.get_untracked()
            {
                let (reply, response) = oneshot::channel();
                let request = EditorRequest {
                    revision: dispatch.revision.get_untracked(),
                    selection_revision: dispatch.selection_revision.get_untracked(),
                    operation,
                    reply,
                };
                if dispatch.commands.send(request).await.is_err() {
                    dispatch.fail();
                } else {
                    let response = response.await;
                    if generation == dispatch.generation.get_untracked()
                        && !dispatch.failed.get_untracked()
                    {
                        match response {
                            Ok(EditorOutcome {
                                accepted: true,
                                snapshot: Some(snapshot),
                                selection_revision,
                            }) => {
                                dispatch.revision.set(snapshot.revision);
                                dispatch.selection_revision.set(selection_revision);
                                if dispatch.pending.get_untracked() == 1 {
                                    dispatch.set_value(snapshot.text.to_string());
                                    dispatch.selection.set(snapshot.selection);
                                    dispatch.composing.set(snapshot.composing);
                                }
                            }
                            _ => dispatch.fail(),
                        }
                    }
                }
            }
            dispatch
                .pending
                .update(|pending| *pending = pending.saturating_sub(1));
            dispatch
                .pending_bytes
                .update(|pending| *pending = pending.saturating_sub(bytes));
            dispatch
                .pending_attachment_bytes
                .update(|pending| *pending = pending.saturating_sub(attachment_bytes));
            let _ = done.send(());
        });
    }
}

pub(crate) struct EditorView {
    pub element: AnyElement,
    pub failed: Signal<bool>,
    pub synchronized: Signal<bool>,
}

fn editor(
    state: Signal<EditorSnapshot>,
    commands: mpsc::Sender<EditorRequest>,
    focus_revision: Signal<u64>,
    route: Signal<ComposerRoute>,
    host_failed: Signal<bool>,
    host: &ComposerProjection,
) -> EditorView {
    let gate = host.gate.clone();
    let shortcuts = host.shortcuts.clone();
    let draft = host.draft.clone();
    let first = state.get();
    let dispatch = Dispatch {
        commands,
        previous: Rc::new(RefCell::new(None)),
        revision: signal(first.revision),
        selection_revision: signal(first.selection_revision),
        pending: signal(0),
        pending_bytes: signal(0),
        pending_attachment_bytes: signal(0),
        recovery: signal(draft.saved()),
        draft,
        value: signal(first.text.to_string()),
        composing: signal(first.composing),
        selection: signal(first.selection),
        failed: signal(false),
        generation: signal(0),
        gate: gate.clone(),
    };
    let failure = dispatch.clone();
    effect(move || {
        if host_failed.get() && !failure.failed.get_untracked() {
            failure.fail();
        }
    });
    let sync = dispatch.clone();
    effect(move || {
        let snapshot = state.get();
        if !sync.failed.get()
            && sync.pending.get() == 0
            && snapshot.revision >= sync.revision.get_untracked()
            && snapshot.selection_revision >= sync.selection_revision.get_untracked()
        {
            sync.revision.set(snapshot.revision);
            sync.selection_revision.set(snapshot.selection_revision);
            sync.set_value(snapshot.text.to_string());
            sync.selection.set(snapshot.selection);
            sync.composing.set(snapshot.composing);
        }
    });
    let synchronized = signal(false);
    let barrier = dispatch.clone();
    effect(move || {
        let snapshot = state.get();
        let ready = !host_failed.get()
            && !barrier.failed.get()
            && barrier.pending.get() == 0
            && !barrier.composing.get()
            && !snapshot.composing
            && snapshot.revision == barrier.revision.get()
            && snapshot.selection_revision == barrier.selection_revision.get()
            && snapshot.text.as_ref() == barrier.value.get().as_str()
            && snapshot.selection == barrier.selection.get();
        synchronized.set(ready);
        if ready {
            barrier.gate.open(&snapshot);
        } else {
            barrier.gate.close();
        }
    });
    let value = dispatch.value;
    let composing = dispatch.composing;
    let selection = dispatch.selection;
    let change = dispatch.clone();
    let select = dispatch.clone();
    let composition = dispatch.clone();
    let paste = dispatch.clone();
    let reload = dispatch.clone();
    let restore = dispatch.clone();
    let copy = dispatch.clone();
    let history = dispatch.clone();
    let predict = dispatch.clone();
    let shortcut_dispatch = dispatch.clone();
    let shortcut_pending = signal(false);
    let focused = signal(false);
    let shortcut_intent = signal(None::<ShortcutIntent>);
    let deliver = dispatch.clone();
    effect(move || {
        let Some(intent) = shortcut_intent.get() else {
            return;
        };
        if !focused.get()
            || deliver.failed.get()
            || deliver.composing.get()
            || intent.generation != deliver.generation.get()
            || intent.text != deliver.value.get()
            || intent.selection != deliver.selection.get()
        {
            shortcut_intent.set(None);
            return;
        }
        if !synchronized.get() || shortcut_pending.get() {
            return;
        }
        shortcut_intent.set(None);
        if intent.route != route.get() {
            return;
        }
        let snapshot = state.get();
        let event = ComposerShortcut {
            stamp: EditorStamp::from_snapshot(&snapshot),
            route: intent.route,
            key: intent.key,
        };
        shortcut_pending.set(true);
        let shortcuts = shortcuts.clone();
        let dispatch = deliver.clone();
        spawn(async move {
            let (reply, response) = oneshot::channel();
            if shortcuts
                .send(ShortcutRequest { event, reply })
                .await
                .is_err()
                || response.await.is_err()
            {
                dispatch.fail();
            }
            shortcut_pending.set(false);
        });
    });
    let readonly = dispatch.failed;
    let pending = dispatch.pending;
    let recovery = dispatch.recovery;
    let input_height =
        signal(Typography::BODY * Typography::BODY_LINE_HEIGHT * first.min_rows as f32);
    let input_ref = element_ref();
    let scroll_y = signal(None::<f32>);
    let caret = signal(None::<(String, (u32, u32), f32, f32)>);
    nsh_core::element::watch_scroll(
        move || input_ref.node(),
        move |event| {
            scroll_y.set(Some(event.offset.y.0));
        },
    );
    let last_focus = Rc::new(Cell::new(0));
    let requested_focus = last_focus.clone();
    effect(move || {
        let next = focus_revision.get();
        if next != requested_focus.get() && input_ref.node().is_some() {
            requested_focus.set(next);
            input_ref.focus();
        }
    });
    let input = nsh_base::input()
        .element_ref(&input_ref)
        .label(move || state.with(|state| state.label.clone()))
        .placeholder(move || {
            state.with(|state| {
                state
                    .prediction
                    .clone()
                    .unwrap_or_else(|| state.placeholder.clone())
            })
        })
        .value(value)
        .selected_range(selection)
        .multiline(move || state.with(|state| state.multiline))
        .submit_behavior(nsh_core::SubmitBehavior::Newline)
        .text_size(px(Typography::BODY))
        .line_height(px(Typography::BODY * Typography::BODY_LINE_HEIGHT))
        .w_full()
        .h(move || px(input_height.get()))
        .on_layout(move |_: &Bounds<Points>| {
            let next = focus_revision.get_untracked();
            if next != last_focus.get() {
                last_focus.set(next);
                input_ref.focus();
            }
        })
        .on_caret_move(move |event: &nsh_core::element::CaretEvent| {
            caret.set(Some((
                value.get_untracked(),
                selection.get_untracked(),
                event.bounds.origin.y.0,
                event.bounds.size.height.0,
            )));
        })
        .on_content_size_change(move |size: &Size<Points>| {
            let snapshot = state.get_untracked();
            let line = Typography::BODY * Typography::BODY_LINE_HEIGHT;
            let min = line * snapshot.min_rows.max(1) as f32;
            let max = line * snapshot.max_rows.max(snapshot.min_rows).max(1) as f32;
            if size.height.0.is_finite() {
                input_height.set(size.height.0.clamp(min, max));
            }
        })
        .readonly(readonly)
        .paste_behavior(nsh_core::PasteBehavior::Report)
        .on_paste(move |payload: &nsh_core::PasteEvent| {
            if paste.failed.get_untracked() {
                return;
            }
            paste.gate.close();
            let before: Arc<str> = Arc::from(value.get_untracked());
            let range = selection.get_untracked();
            let start = utf16_offset(&before, range.0.min(range.1));
            let end = utf16_offset(&before, range.0.max(range.1));
            let inserted = if payload.uris.is_empty() {
                payload.text.replace("\r\n", "\n")
            } else {
                String::new()
            };
            let mut after = before.to_string();
            let next_selection = if inserted.is_empty() {
                range
            } else {
                after.replace_range(start..end, &inserted);
                let caret = utf16_index(after[..start + inserted.len()].encode_utf16().count());
                (caret, caret)
            };
            paste.set_value(after.clone());
            selection.set(next_selection);
            if payload.files.len().saturating_add(payload.uris.len()) > MAX_IMAGES
                || payload.uris.iter().any(|uri| uri.len() > 32 * 1024)
                || payload
                    .files
                    .iter()
                    .any(|file| file.data.len() > MAX_IMAGE_BYTES)
                || payload
                    .files
                    .iter()
                    .fold(0usize, |bytes, file| bytes.saturating_add(file.data.len()))
                    > MAX_TURN_IMAGE_BYTES
            {
                paste.fail();
                return;
            }
            paste.send(EditorOperation::Paste {
                before,
                after,
                selection: next_selection,
                payload: Arc::new({
                    let mut attachments = nsh_core::PasteEvent::default();
                    attachments.uris = payload.uris.clone();
                    attachments.files = payload
                        .files
                        .iter()
                        .map(|file| nsh_core::PastedFile::new("", "", file.data.clone()))
                        .collect();
                    attachments
                }),
            });
        })
        .on_focus(move || focused.set(true))
        .on_blur(move || focused.set(false))
        .on_change(move |after: &str| {
            change.gate.close();
            let before = Arc::from(value.get());
            change.set_value(after.to_owned());
            if change.failed.get_untracked() {
                change.draft.save(after.to_owned());
                change.recovery.set(Some(after.to_owned()));
                return;
            }
            change.send(EditorOperation::Replace {
                before,
                after: after.to_owned(),
            });
        })
        .on_selection_change(move |range: &Selection| {
            select.gate.close();
            selection.set((range.start, range.end));
            select.send(EditorOperation::Select {
                text: Arc::from(value.get()),
                range: (range.start, range.end),
            });
        })
        .on_composition_change(move |event: &Composition| {
            composition.gate.close();
            composing.set(event.composing);
            composition.send(EditorOperation::Compose {
                text: Arc::from(value.get()),
                active: event.composing,
            });
        })
        .into_element();
    let element = view()
        .w_full()
        .gap_2()
        .child(when(
            move || focused.get() && !composing.get() && !readonly.get(),
            move || {
                for (key, redo) in [("mod+z", false), ("mod+shift+z", true), ("ctrl+y", true)] {
                    let history = history.clone();
                    on_key_with(
                        key,
                        KeyOptions {
                            when: nsh_core::keymap::When::Always,
                            ..Default::default()
                        },
                        move |_| {
                            history.send(if redo {
                                EditorOperation::Redo
                            } else {
                                EditorOperation::Undo
                            })
                        },
                    );
                }
                view()
            },
        ))
        .child(when(
            move || {
                focused.get()
                    && !composing.get()
                    && !readonly.get()
                    && pending.get() == 0
                    && !route.get().files
                    && !route.get().actions
                    && state.with(|snapshot| {
                        snapshot.prediction.is_some()
                            && snapshot.text.as_ref() == value.get().as_str()
                    })
            },
            move || {
                for (key, accept) in [
                    ("tab", true),
                    ("right", true),
                    ("enter", true),
                    ("escape", false),
                ] {
                    let predict = predict.clone();
                    on_key_with(
                        key,
                        KeyOptions {
                            when: nsh_core::keymap::When::Always,
                            ..Default::default()
                        },
                        move |_| {
                            predict.send(if accept {
                                EditorOperation::AcceptPrediction
                            } else {
                                EditorOperation::DismissPrediction
                            })
                        },
                    );
                }
                view()
            },
        ))
        .child(when(
            move || focused.get() && !composing.get() && !readonly.get(),
            move || {
                let current_route = route.get();
                let prediction = state.with(|snapshot| snapshot.prediction.is_some());
                let mut bindings = vec![
                    ("mod+enter", ComposerKey::Send),
                    ("shift+tab", ComposerKey::FocusPrevious),
                ];
                if current_route.files || current_route.actions {
                    bindings.extend([
                        ("enter", ComposerKey::Accept),
                        ("tab", ComposerKey::Accept),
                        ("escape", ComposerKey::Cancel),
                        ("up", ComposerKey::Previous),
                        ("down", ComposerKey::Next),
                    ]);
                } else {
                    if !prediction {
                        bindings.extend([
                            ("enter", ComposerKey::Send),
                            ("tab", ComposerKey::FocusNext),
                        ]);
                    }
                    let first_row = scroll_y.get().is_some_and(|offset| offset.abs() < 0.5)
                        && caret.get().is_some_and(|(text, range, y, height)| {
                            text == value.get()
                                && range == selection.get()
                                && y.is_finite()
                                && height.is_finite()
                                && height > 0.
                                && y >= -0.5
                                && y < height * 0.5
                        });
                    if current_route.has_history && (current_route.recalling_history || first_row) {
                        bindings.push(("up", ComposerKey::History));
                    }
                }
                for (keystroke, key) in bindings {
                    let dispatch = shortcut_dispatch.clone();
                    on_key_with(
                        keystroke,
                        KeyOptions {
                            when: nsh_core::keymap::When::Always,
                            ..Default::default()
                        },
                        move |_| {
                            if shortcut_pending.get_untracked()
                                || shortcut_intent.get_untracked().is_some()
                            {
                                return;
                            }
                            shortcut_intent.set(Some(ShortcutIntent {
                                text: value.get_untracked(),
                                selection: selection.get_untracked(),
                                generation: dispatch.generation.get_untracked(),
                                route: route.get_untracked(),
                                key,
                            }));
                        },
                    );
                }
                view()
            },
        ))
        .child(input)
        .child(when(
            move || readonly.get(),
            move || {
                let reload = reload.clone();
                view()
                    .gap_2()
                    .child(text(move || {
                        state.with(|_| vega_i18n::t("ui.editor_sync_failed"))
                    }))
                    .child(
                        nsh_base::button()
                            .disabled(move || pending.get() != 0 || host_failed.get())
                            .on_press(move || {
                                if reload.pending.get_untracked() != 0
                                    || host_failed.get_untracked()
                                {
                                    return;
                                }
                                let snapshot = state.get_untracked();
                                reload
                                    .generation
                                    .update(|generation| *generation = generation.wrapping_add(1));
                                reload.revision.set(snapshot.revision);
                                reload.selection_revision.set(snapshot.selection_revision);
                                reload.set_value(snapshot.text.to_string());
                                reload.selection.set(snapshot.selection);
                                reload.composing.set(snapshot.composing);
                                reload.failed.set(false);
                            })
                            .child(text(move || {
                                state.with(|_| vega_i18n::t("settings.action.reload"))
                            })),
                    )
            },
        ))
        .child(when(
            move || recovery.get().is_some(),
            move || {
                let copy = copy.clone();
                let restore = restore.clone();
                view()
                    .flex_row()
                    .flex_wrap()
                    .gap_2()
                    .child(
                        nsh_base::button()
                            .on_press(move || {
                                if let Some(text) = copy.recovery.get_untracked() {
                                    set_clipboard(text);
                                }
                            })
                            .child(text(move || {
                                state.with(|_| vega_i18n::t("ui.copy_local_draft"))
                            })),
                    )
                    .child(
                        nsh_base::button()
                            .disabled(move || {
                                readonly.get() || pending.get() != 0 || composing.get()
                            })
                            .on_press(move || {
                                if readonly.get_untracked()
                                    || restore.pending.get_untracked() != 0
                                    || composing.get_untracked()
                                {
                                    return;
                                }
                                if let Some(after) = restore.recovery.get_untracked() {
                                    restore.gate.close();
                                    let before = Arc::from(value.get_untracked());
                                    restore.set_value(after.clone());
                                    restore.send(EditorOperation::Replace { before, after });
                                }
                            })
                            .child(text(move || {
                                state.with(|_| vega_i18n::t("ui.restore_local_draft"))
                            })),
                    )
            },
        ))
        .into_element();
    EditorView {
        element,
        failed: dispatch.failed,
        synchronized,
    }
}

#[cfg(test)]
mod recovery_tests {
    use super::DraftRecovery;

    #[test]
    fn reload_preserves_unsynchronized_text_without_replacing_authoritative_draft() {
        let failed = DraftRecovery::new("accepted".into());
        failed.local("unsent 中文 draft");
        failed.preserve();
        let restarted = failed.restarted("accepted after in-flight edit".into());
        assert_eq!(restarted.saved().as_deref(), Some("unsent 中文 draft"));
        restarted.preserve();
        assert_eq!(
            restarted.saved().as_deref(),
            Some("accepted after in-flight edit")
        );
    }

    #[test]
    fn late_failure_from_old_host_cannot_overwrite_restarted_recovery() {
        let failed = DraftRecovery::new("original".into());
        failed.save("local recovery".into());
        let restarted = failed.restarted("authoritative".into());
        failed.local("late old host state");
        failed.preserve();
        assert_eq!(restarted.saved().as_deref(), Some("local recovery"));
        restarted.local("new host draft");
        restarted.preserve();
        assert_eq!(restarted.saved().as_deref(), Some("new host draft"));
        assert_eq!(failed.saved().as_deref(), Some("late old host state"));
    }
}
