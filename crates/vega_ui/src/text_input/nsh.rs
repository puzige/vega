use nsh_platform_gpui::editing::InputPastePayload;
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

use super::editing_core::{
    ComposerNativeBridge, EditLedger, EditTicket, EditorOwner, NativeCommandToken, NativeEdit,
    NativeEditorCommand, NativeEditorPolicy, NativeEditorRoute, NativeRestoreToken,
    ReceiptDecision, valid_selection,
};
use super::native_adapter::{NativeAdapter, NativeDelivery};
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

    pub(super) fn close(&self) {
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
    Restore {
        token: NativeRestoreToken,
        after: String,
    },
    ReloadNative,
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
}

pub(crate) struct EditorRequest {
    ticket: EditTicket,
    pub revision: u64,
    pub operation: EditorOperation,
    pub selection_revision: u64,
    pub reply: oneshot::Sender<EditorOutcome>,
}

pub(crate) struct EditorOutcome {
    ticket: EditTicket,
    pub accepted: bool,
    pub snapshot: Option<EditorSnapshot>,
    pub selection_revision: u64,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ComposerRoute {
    pub policy: super::editing_core::NativeEditorPolicy,
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

impl ComposerRoute {
    pub(crate) fn single_line(session: gpui_kit::EntityId) -> Self {
        Self {
            policy: super::editing_core::NativeEditorPolicy::SingleLine(session),
            ..Self::default()
        }
    }
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
    native: Option<NativeCommandToken>,
}

impl gpui_kit::EventEmitter<ComposerShortcut> for TextInput {}

impl ComposerShortcut {
    pub(crate) fn matches(&self, input: &TextInput, route: ComposerRoute) -> bool {
        self.route == route
            && self.native.as_ref().is_none_or(|token| {
                input
                    .nsh_composer
                    .as_ref()
                    .is_some_and(|editor| editor.native.validates(token))
            })
            && input.composer_editor_ready()
            && input
                .nsh_snapshot()
                .is_some_and(|snapshot| self.stamp == EditorStamp::from_snapshot(&snapshot))
    }
}

#[derive(Clone)]
pub(super) struct DraftRecovery(Arc<Mutex<RecoveryState>>);

struct RecoveryState {
    local: String,
    saved: Option<String>,
}

impl DraftRecovery {
    pub(super) fn new(text: String) -> Self {
        Self(Arc::new(Mutex::new(RecoveryState {
            local: text,
            saved: None,
        })))
    }

    pub(super) fn local(&self, text: &str) {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .local = text.to_owned();
    }

    pub(super) fn saved(&self) -> Option<String> {
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

    pub(super) fn preserve(&self) {
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

#[derive(Clone, Copy)]
struct SingleLineChrome {
    colors: vega_theme::ThemeColors,
    rem: f32,
}

impl SingleLineChrome {
    fn new(route: ComposerRoute, bare: bool, window: &Window, cx: &gpui_kit::App) -> Option<Self> {
        (matches!(
            route.policy,
            super::editing_core::NativeEditorPolicy::SingleLine(_)
        ) && !bare)
            .then(|| Self {
                colors: vega_theme::theme(cx).colors,
                rem: f32::from(window.rem_size()),
            })
    }
}

#[derive(Clone)]
struct ComposerProjection {
    snapshot: EditorSnapshot,
    colors: vega_theme::ThemeColors,
    chrome: Option<SingleLineChrome>,
    gate: EditorGate,
    focus_revision: u64,
    height: watch::Sender<f32>,
    host_failed: bool,
    draft: DraftRecovery,
    native: Option<ComposerNativeBridge>,
    node: watch::Sender<Option<u32>>,
}

pub(super) struct ComposerEditor {
    id: crate::nsh::ProjectionId,
    draft: DraftRecovery,
    recovery_page: Option<crate::nsh::Surface<RecoveryProjection, ReloadEditor>>,
    restarting: bool,
    surface: crate::nsh::Surface<ComposerProjection, EditorRequest>,
    gate: EditorGate,
    native: ComposerNativeBridge,
    focus_revision: u64,
    _focus_subscription: Subscription,
    height: watch::Receiver<f32>,
    height_sender: watch::Sender<f32>,
    route: ComposerRoute,
    adapter: Option<Rc<NativeAdapter>>,
    node: watch::Receiver<Option<u32>>,
    node_tx: watch::Sender<Option<u32>>,
    _activation_subscription: Subscription,
}

fn render_composer_editor(
    state: Signal<ComposerProjection>,
    commands: mpsc::Sender<EditorRequest>,
) -> AnyElement {
    let initial = state.get_untracked();
    let snapshot = signal_with(initial.snapshot.clone(), SignalEquals::Never);
    let focus_revision = signal(initial.focus_revision);
    let host_failed = signal(initial.host_failed);
    let chrome = signal_with(initial.chrome, SignalEquals::Never);
    let colors = signal_with(initial.colors, SignalEquals::Never);
    effect(move || {
        let projection = state.get();
        snapshot.set(projection.snapshot);
        focus_revision.set(projection.focus_revision);
        host_failed.set(projection.host_failed);
        chrome.set(projection.chrome);
        colors.set(projection.colors);
    });
    let editor = editor(
        snapshot,
        commands,
        focus_revision,
        host_failed,
        colors,
        &initial,
    );
    let element = view()
        .w_full()
        .px(move || px(chrome.get().map_or(0., |chrome| chrome.rem * 0.5)))
        .py(move || px(chrome.get().map_or(0., |chrome| chrome.rem * 0.25)))
        .border(move || px(if chrome.get().is_some() { 1. } else { 0. }))
        .rounded(move || px(chrome.get().map_or(0., |chrome| chrome.rem * 0.5)))
        .border_color(move || {
            chrome.get().map_or_else(
                || rgba(0),
                |chrome| crate::nsh::color(chrome.colors.border_subtle),
            )
        })
        .bg(move || {
            chrome.get().map_or_else(
                || rgba(0),
                |chrome| crate::nsh::color(chrome.colors.bg_elevated),
            )
        })
        .child(editor.element);
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
        .child(element)
        .into_element()
}

impl TextInput {
    pub fn render_nsh_preview(
        input: Entity<Self>,
        window: &mut Window,
        cx: &mut gpui_kit::App,
    ) -> gpui_kit::AnyElement {
        Self::render_nsh_editor(input, ComposerRoute::default(), false, window, cx)
    }

    pub(crate) fn composer_editor_ready(&self) -> bool {
        self.nsh_composer.as_ref().is_none_or(|editor| {
            !editor.restarting
                && editor.surface.errors().borrow().is_none()
                && editor.gate.matches(self)
                && self
                    .nsh_snapshot()
                    .is_some_and(|snapshot| editor.native.ready(&snapshot.text, snapshot.selection))
        })
    }

    pub(crate) fn composer_contains_focus(&self, window: &Window, cx: &gpui_kit::App) -> bool {
        self.focus_handle.contains_focused(window, cx)
            || self.nsh_composer.as_ref().is_some_and(|editor| {
                editor.surface.focus_handle(cx).contains_focused(window, cx)
                    || editor
                        .recovery_page
                        .as_ref()
                        .is_some_and(|page| page.focus_handle(cx).contains_focused(window, cx))
            })
    }

    pub fn composer_native_bridge(&self) -> Option<ComposerNativeBridge> {
        self.nsh_composer
            .as_ref()
            .filter(|editor| !editor.restarting)
            .map(|editor| editor.native.clone())
    }

    pub fn project_native_draft(
        &self,
        owner: &EditorOwner,
        expected_serial: u64,
        apply_native: impl FnOnce(&NativeEdit) -> bool,
    ) -> bool {
        let Some(editor) = self
            .nsh_composer
            .as_ref()
            .filter(|editor| !editor.restarting && editor.surface.errors().borrow().is_none())
        else {
            return false;
        };
        let Some(snapshot) = self.nsh_snapshot().filter(|snapshot| !snapshot.composing) else {
            return false;
        };
        editor.native.project(
            owner,
            expected_serial,
            snapshot.text,
            snapshot.selection,
            (snapshot.revision, snapshot.selection_revision),
            apply_native,
        )
    }

    pub fn suspend_nsh_composer(&mut self, cx: &mut Context<Self>) {
        if let Some(editor) = &mut self.nsh_composer {
            editor.gate.close();
            editor.native.retire();
            if editor.native.attached() {
                editor.draft.local(&editor.native.local_draft().text);
            }
            editor.draft.preserve();
            editor.restarting = true;
            editor.surface.entity().read(cx).stop();
            if let Some(page) = &editor.recovery_page {
                page.entity().read(cx).stop();
            }
        }
        cx.notify();
    }

    pub fn apply_native_edit(
        &mut self,
        owner: &EditorOwner,
        edit: &NativeEdit,
        cx: &mut Context<Self>,
    ) -> bool {
        self.apply_native_transaction(owner, edit, None, cx)
    }

    pub fn apply_native_paste(
        &mut self,
        owner: &EditorOwner,
        edit: &NativeEdit,
        payload: std::rc::Rc<InputPastePayload>,
        cx: &mut Context<Self>,
    ) -> bool {
        self.apply_native_transaction(owner, edit, Some(payload), cx)
    }

    fn apply_native_transaction(
        &mut self,
        owner: &EditorOwner,
        edit: &NativeEdit,
        payload: Option<std::rc::Rc<InputPastePayload>>,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(editor) = self
            .nsh_composer
            .as_ref()
            .filter(|editor| !editor.restarting && editor.surface.errors().borrow().is_none())
        else {
            return false;
        };
        let bridge = editor.native.clone();
        let draft = editor.draft.clone();
        let accepted = self.commit_native_transaction(&bridge, owner, edit, payload, cx);
        if !accepted {
            let local = bridge.local_draft();
            draft.local(&local.text);
            draft.preserve();
        }
        cx.notify();
        accepted
    }

    fn commit_native_transaction(
        &mut self,
        bridge: &ComposerNativeBridge,
        owner: &EditorOwner,
        edit: &NativeEdit,
        payload: Option<std::rc::Rc<InputPastePayload>>,
        cx: &mut Context<Self>,
    ) -> bool {
        let policy = if payload.is_some() {
            let Some(policy) = bridge.paste_policy(edit) else {
                return false;
            };
            policy
        } else {
            NativeEditorPolicy::Composer
        };
        let mut payload = match payload.map(std::rc::Rc::try_unwrap).transpose() {
            Ok(payload) => payload,
            Err(_) => return false,
        };
        bridge.commit(owner, edit, |before, revisions| {
            let snapshot = self.nsh_snapshot()?;
            if self.masked
                || (snapshot.revision, snapshot.selection_revision) != revisions
                || snapshot.text != before.text
                || snapshot.selection != before.selection
                || snapshot.composing != before.composing
            {
                return None;
            }
            if !edit.follows(before) {
                return None;
            }
            let attachments = if let Some(payload) = payload.as_mut() {
                if !self.image_paste
                    && (payload.files().next().is_some() || !payload.uris().is_empty())
                {
                    return None;
                }
                let (text, selection) =
                    pasted_text(&before.text, before.selection, payload, policy)?;
                if edit.composing || text != edit.text.as_ref() || selection != edit.selection {
                    return None;
                }
                if payload.uris().is_empty() && !payload.text().is_empty() {
                    let mutation = edit.mutation.as_ref()?;
                    if mutation.range_utf16
                        != (
                            before.selection.0.min(before.selection.1),
                            before.selection.0.max(before.selection.1),
                        )
                        || mutation.inserted.as_ref() != policy.paste_text(payload.text())
                    {
                        return None;
                    }
                } else if edit.mutation.is_some() {
                    return None;
                }
                if payload.files().next().is_none() && payload.uris().is_empty() {
                    None
                } else {
                    Some(match payload {
                        InputPastePayload::Native(_) => payload.take_original()?,
                        InputPastePayload::Synthetic(paste) => pasted_attachments(paste)?,
                    })
                }
            } else {
                None
            };
            let selection_revision = self.nsh_selection_revision.checked_add(1)?;
            if let Some(payload) = payload.as_ref() {
                self.apply_pasted_replacement(before, payload, policy)?;
            } else if let Some(mutation) = &edit.mutation {
                let range = mutation.byte_range(&before.text)?;
                self.content_revision.checked_add(1)?;
                self.record_edit();
                self.apply_edit(range, &mutation.inserted);
            } else if edit.composing && !before.composing {
                self.record_edit();
            }
            self.selected_range = self.range_from_utf16(
                &(edit.selection.0.min(edit.selection.1) as usize
                    ..edit.selection.0.max(edit.selection.1) as usize),
            );
            self.selection_reversed = edit.selection.0 > edit.selection.1;
            self.marked_range = edit.composing.then(|| self.selected_range.clone());
            self.composition_history_active = edit.composing;
            self.nsh_selection_revision = selection_revision;
            self.prediction = None;
            self.vertical_goal = None;
            self.update_logical_viewport();
            if let Some(item) = attachments.filter(|item| !item.entries.is_empty()) {
                cx.emit(super::ImagePaste(Arc::new(item)));
            }
            Some((self.content_revision, self.nsh_selection_revision))
        })
    }

    fn apply_pasted_replacement(
        &mut self,
        before: &NativeEdit,
        payload: &impl PasteView,
        policy: NativeEditorPolicy,
    ) -> Option<()> {
        if !valid_selection(&before.text, before.selection) || self.text() != before.text.as_ref() {
            return None;
        }
        if payload.uris().is_empty() && !payload.text().is_empty() {
            self.content_revision.checked_add(1)?;
            let range = self.range_from_utf16(
                &(before.selection.0.min(before.selection.1) as usize
                    ..before.selection.0.max(before.selection.1) as usize),
            );
            self.record_edit();
            self.apply_edit(range, &policy.paste_text(payload.text()));
        }
        Some(())
    }

    fn restore_native_draft(
        &mut self,
        bridge: &ComposerNativeBridge,
        token: &NativeRestoreToken,
        after: &str,
    ) -> bool {
        if self.masked
            || self.is_composing()
            || self.content_revision == u64::MAX
            || self.nsh_selection_revision == u64::MAX
            || after.len() > 2 * 1024 * 1024
        {
            return false;
        }
        let Some(snapshot) = self.nsh_snapshot() else {
            return false;
        };
        bridge.restore_authority(
            token,
            (snapshot.revision, snapshot.selection_revision),
            &snapshot.text,
            snapshot.selection,
            || {
                self.record_edit();
                self.apply_edit(0..self.content.len(), after);
                self.nsh_selection_revision += 1;
                true
            },
        )
    }

    pub fn dispatch_native_command(
        &mut self,
        token: NativeCommandToken,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(editor) = &self.nsh_composer else {
            return false;
        };
        let bridge = editor.native.clone();
        if !token.claim()
            || !bridge.validates(&token)
            || !self.composer_contains_focus(window, cx)
            || !self.composer_editor_ready()
        {
            bridge.finish_command(&token);
            return false;
        }
        let route = editor.route;
        let Some(snapshot) = self.nsh_snapshot() else {
            bridge.finish_command(&token);
            return false;
        };
        let key = match token.command {
            NativeEditorCommand::Submit => Some(ComposerKey::Send),
            NativeEditorCommand::Accept => Some(ComposerKey::Accept),
            NativeEditorCommand::Cancel => Some(ComposerKey::Cancel),
            NativeEditorCommand::Previous => Some(ComposerKey::Previous),
            NativeEditorCommand::Next => Some(ComposerKey::Next),
            NativeEditorCommand::History => Some(ComposerKey::History),
            NativeEditorCommand::FocusNext => Some(ComposerKey::FocusNext),
            NativeEditorCommand::FocusPrevious => Some(ComposerKey::FocusPrevious),
            NativeEditorCommand::Undo => {
                self.undo(&Undo, window, cx);
                None
            }
            NativeEditorCommand::Redo => {
                self.redo(&Redo, window, cx);
                None
            }
            NativeEditorCommand::AcceptPrediction => {
                self.accept_prediction(&AcceptPrediction, window, cx);
                None
            }
            NativeEditorCommand::DismissPrediction => {
                self.dismiss_prediction(&DismissPrediction, window, cx);
                None
            }
        };
        if let Some(key) = key {
            cx.emit(ComposerShortcut {
                stamp: EditorStamp::from_snapshot(&snapshot),
                route,
                key,
                native: Some(token.clone()),
            });
        }
        bridge.finish_command(&token);
        cx.notify();
        true
    }

    pub fn render_nsh_input(
        input: Entity<Self>,
        window: &mut Window,
        cx: &mut gpui_kit::App,
    ) -> gpui_kit::AnyElement {
        Self::render_nsh_composer(input, ComposerRoute::default(), window, cx)
    }

    pub(crate) fn render_nsh_single_line(
        input: Entity<Self>,
        session: gpui_kit::EntityId,
        window: &mut Window,
        cx: &mut gpui_kit::App,
    ) -> gpui_kit::AnyElement {
        Self::render_nsh_composer(input, ComposerRoute::single_line(session), window, cx)
    }

    pub(crate) fn render_nsh_composer(
        input: Entity<Self>,
        route: ComposerRoute,
        window: &mut Window,
        cx: &mut gpui_kit::App,
    ) -> gpui_kit::AnyElement {
        Self::render_nsh_editor(input, route, true, window, cx)
    }

    fn reload_native_editor(
        &mut self,
        id: &crate::nsh::ProjectionId,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(selection_revision) = self.nsh_selection_revision.checked_add(1) else {
            return false;
        };
        let Some(editor) = self.nsh_composer.as_mut().filter(|editor| {
            !self.masked
                && editor.id.matches(id)
                && !editor.restarting
                && (editor.native.failed() || editor.surface.errors().borrow().is_some())
        }) else {
            return false;
        };
        editor.gate.close();
        if editor.native.attached() {
            editor.draft.local(&editor.native.local_draft().text);
        }
        editor.draft.preserve();
        if let Some(adapter) = &editor.adapter {
            adapter.fail();
        }
        editor.native.retire();
        editor.restarting = true;
        editor.surface.entity().read(cx).stop();
        if let Some(page) = &editor.recovery_page {
            page.entity().read(cx).stop();
        }
        self.marked_range = None;
        self.composition_history_active = false;
        self.nsh_selection_revision = selection_revision;
        cx.notify();
        true
    }

    fn register_native_hooks(&self, trusted_focus: bool, cx: &gpui_kit::App) {
        let Some(editor) = self
            .nsh_composer
            .as_ref()
            .filter(|editor| !editor.restarting)
        else {
            return;
        };
        let Some(adapter) = &editor.adapter else {
            return;
        };
        let Some(node) = *editor.node.borrow() else {
            return;
        };
        let view = editor.surface.entity();
        let Some(identity) = view.read(cx).input_identity(node) else {
            return;
        };
        if adapter.needs_registration(&identity, trusted_focus) {
            let hooks = adapter.hooks(identity.clone());
            if !view.read(cx).set_input_hooks(&identity, Some(hooks)) {
                adapter.fail();
            }
        }
    }

    fn render_nsh_editor(
        input: Entity<Self>,
        route: ComposerRoute,
        transactional: bool,
        window: &mut Window,
        cx: &mut gpui_kit::App,
    ) -> gpui_kit::AnyElement {
        input.update(cx, |this, cx| {
            let Some(snapshot) = this.nsh_snapshot() else {
                return gpui_kit::div().into_any_element();
            };
            let restore_focus =
                window.is_window_active() && this.composer_contains_focus(window, cx);
            if let Some(editor) = this
                .nsh_composer
                .as_mut()
                .filter(|editor| !editor.restarting)
            {
                if editor.adapter.is_some() != transactional {
                    editor.native.retire();
                    editor.restarting = true;
                } else if transactional
                    && !editor.native.failed()
                    && !editor.native.authority_matches(
                        (snapshot.revision, snapshot.selection_revision),
                        &snapshot.text,
                        snapshot.selection,
                    )
                {
                    if editor.native.seal_authority(
                        (snapshot.revision, snapshot.selection_revision),
                        &snapshot.text,
                        snapshot.selection,
                    ) {
                        editor.restarting = true;
                    } else if let Some(adapter) = &editor.adapter {
                        adapter.fail();
                    }
                }
            }
            let restarting = this
                .nsh_composer
                .as_ref()
                .is_some_and(|editor| editor.restarting);
            let retained = restarting
                .then(|| this.nsh_composer.take())
                .flatten()
                .map(|editor| {
                    editor.gate.close();
                    editor.native.retire();
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
                let native = ComposerNativeBridge::new(
                    NativeEdit {
                        text: snapshot.text.clone(),
                        selection: snapshot.selection,
                        composing: snapshot.composing,
                        serial: 0,
                        mutation: None,
                    },
                    (snapshot.revision, snapshot.selection_revision),
                    gate.clone(),
                );
                let mut updates = gate.0.subscribe();
                let (height_tx, height) = watch::channel(
                    Typography::BODY * Typography::BODY_LINE_HEIGHT * snapshot.min_rows as f32,
                );
                let mut sizes = height.clone();
                cx.spawn(async move |this, cx| {
                    while sizes.changed().await.is_ok() {
                        if this.update(cx, |_, cx| cx.notify()).is_err() {
                            break;
                        }
                    }
                })
                .detach();
                let initial_focus =
                    u64::from(restore_focus || this.focus_handle.is_focused(window));
                let (node_tx, node) = watch::channel(None);
                let (native_tx, mut native_rx) = mpsc::channel(crate::nsh::COMMAND_CAPACITY);
                let adapter = transactional
                    .then(|| {
                        NativeAdapter::new(
                            native.clone(),
                            draft.clone(),
                            native_tx,
                            this.image_paste,
                        )
                    })
                    .flatten();
                let initial = ComposerProjection {
                    colors: vega_theme::theme(cx).colors,
                    snapshot: snapshot.clone(),
                    chrome: SingleLineChrome::new(route, this.bare_chrome, window, cx),
                    gate: gate.clone(),
                    focus_revision: initial_focus,
                    height: height_tx.clone(),
                    host_failed: false,
                    draft: draft.clone(),
                    native: transactional.then(|| native.clone()),
                    node: node_tx.clone(),
                };
                let (surface, mut requests) =
                    crate::nsh::Surface::new(initial, render_composer_editor, window, cx);
                let mut errors = surface.errors();
                let error_gate = gate.clone();
                let error_draft = draft.clone();
                let error_native = native.clone();
                cx.spawn(async move |this, cx| {
                    loop {
                        if errors.borrow_and_update().is_some() {
                            error_gate.close();
                            error_native.fail();
                            if error_native.attached() {
                                error_draft.local(&error_native.local_draft().text);
                            }
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
                                colors: vega_theme::theme(cx).colors,
                                snapshot,
                                chrome: SingleLineChrome::new(
                                    editor.route,
                                    this.bare_chrome,
                                    window,
                                    cx,
                                ),
                                gate: editor.gate.clone(),
                                focus_revision: editor.focus_revision,
                                height: editor.height_sender.clone(),
                                host_failed: editor.surface.errors().borrow().is_some(),
                                draft: editor.draft.clone(),
                                native: editor.adapter.as_ref().map(|_| editor.native.clone()),
                                node: editor.node_tx.clone(),
                            });
                            if let Some(page) = &editor.recovery_page {
                                page.focus(window, cx);
                            } else if editor.surface.errors().borrow().is_none() {
                                editor.surface.focus(window, cx);
                            }
                        }
                    });
                let activation_subscription =
                    cx.observe_window_activation(window, |this, window, cx| {
                        if window.is_window_active() && this.composer_contains_focus(window, cx) {
                            cx.on_next_frame(window, |this, window, cx| {
                                if window.is_window_active()
                                    && this.composer_contains_focus(window, cx)
                                {
                                    this.register_native_hooks(true, cx);
                                }
                            });
                        }
                        cx.notify();
                    });
                let mut nodes = node.clone();
                let native_id = id.clone();
                cx.spawn_in(window, async move |this, cx| {
                    loop {
                        let delivery = tokio::select! {
                            delivery = native_rx.recv() => {
                                let Some(delivery) = delivery else { break; };
                                Some(delivery)
                            }
                            changed = nodes.changed() => {
                                if changed.is_err() { break; }
                                None
                            }
                        };
                        if this
                            .update_in(cx, |this, window, cx| {
                                let Some(editor) = this.nsh_composer.as_ref().filter(|editor| {
                                    editor.id.matches(&native_id) && !editor.restarting
                                }) else {
                                    return;
                                };
                                let Some(adapter) = editor.adapter.clone() else {
                                    return;
                                };
                                match delivery {
                                    Some(NativeDelivery::Edit(edit)) => {
                                        if !this.apply_native_edit(adapter.owner(), &edit, cx) {
                                            adapter.fail();
                                        }
                                        if let Some(snapshot) = this.nsh_snapshot()
                                            && let Some(editor) = &this.nsh_composer
                                            && editor
                                                .native
                                                .ready(&snapshot.text, snapshot.selection)
                                        {
                                            editor.gate.open(&snapshot);
                                        }
                                    }
                                    Some(NativeDelivery::Paste {
                                        edit,
                                        payload,
                                        _budget,
                                    }) => {
                                        if !this.apply_native_paste(
                                            adapter.owner(),
                                            &edit,
                                            payload,
                                            cx,
                                        ) {
                                            adapter.fail();
                                        }
                                        if let Some(snapshot) = this.nsh_snapshot()
                                            && let Some(editor) = &this.nsh_composer
                                            && editor
                                                .native
                                                .ready(&snapshot.text, snapshot.selection)
                                        {
                                            editor.gate.open(&snapshot);
                                        }
                                        drop(_budget);
                                    }
                                    Some(NativeDelivery::Command(token)) => {
                                        this.dispatch_native_command(token, window, cx);
                                    }
                                    Some(NativeDelivery::Failed) => {}
                                    None => {
                                        if adapter.is_failed() {
                                            adapter.fail();
                                        }
                                        this.register_native_hooks(
                                            window.is_window_active()
                                                && this.composer_contains_focus(window, cx),
                                            cx,
                                        );
                                    }
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
                if restore_focus {
                    surface.focus(window, cx);
                }
                this.nsh_composer = Some(ComposerEditor {
                    id: id.clone(),
                    draft,
                    recovery_page: None,
                    restarting: false,
                    surface,
                    gate,
                    native,
                    focus_revision: initial_focus,
                    _focus_subscription: focus_subscription,
                    height,
                    height_sender: height_tx,
                    route,
                    adapter,
                    node,
                    node_tx,
                    _activation_subscription: activation_subscription,
                });
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
                                        ticket: request.ticket,
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
            this.register_native_hooks(
                window.is_window_active() && this.composer_contains_focus(window, cx),
                cx,
            );
            let focus = this.focus_handle.clone();
            let Some(editor) = this.nsh_composer.as_mut() else {
                return gpui_kit::div().into_any_element();
            };
            editor.route = route;
            editor.native.route(NativeEditorRoute {
                policy: route.policy,
                popup: route.files || route.actions,
                prediction: snapshot.prediction.is_some(),
                history: route.has_history,
                recalling_history: route.recalling_history,
                file_generation: route.file_generation,
                file_highlight: route.file_highlight,
                action_generation: route.action_generation,
                action_highlight: route.action_highlight,
            });
            editor.surface.sync_environment(cx);
            editor.surface.publish(ComposerProjection {
                colors: vega_theme::theme(cx).colors,
                snapshot,
                chrome: SingleLineChrome::new(route, this.bare_chrome, window, cx),
                gate: editor.gate.clone(),
                focus_revision: editor.focus_revision,
                height: editor.height_sender.clone(),
                host_failed: editor.surface.errors().borrow().is_some(),
                draft: editor.draft.clone(),
                native: editor.adapter.as_ref().map(|_| editor.native.clone()),
                node: editor.node_tx.clone(),
            });
            if editor.surface.errors().borrow().is_some() || editor.native.failed() {
                editor.gate.close();
                editor.native.fail();
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
                    if restore_focus {
                        page.focus(window, cx);
                    }
                    editor.recovery_page = Some(page);
                    cx.spawn(async move |this, cx| {
                        while let Some(request) = reloads.recv().await {
                            if this
                                .update(cx, |this, cx| {
                                    let accepted = this.reload_native_editor(&request.id, cx);
                                    let _ = request.reply.send(accepted);
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
        let current = self.content_revision == request.revision
            && self.nsh_selection_revision == request.selection_revision;
        let accepted = match request.operation {
            EditorOperation::Restore { token, after } => {
                let editor = self.nsh_composer.as_ref().filter(|editor| {
                    !editor.restarting
                        && editor.surface.errors().borrow().is_none()
                        && editor.draft.saved().as_deref() == Some(after.as_str())
                });
                let bridge = editor.map(|editor| editor.native.clone());
                let accepted = current
                    && bridge
                        .is_some_and(|bridge| self.restore_native_draft(&bridge, &token, &after));
                if accepted && let Some(editor) = &mut self.nsh_composer {
                    editor.restarting = true;
                    editor.surface.entity().read(cx).stop();
                }
                accepted
            }
            EditorOperation::ReloadNative => {
                let id = self.nsh_composer.as_ref().map(|editor| editor.id.clone());
                current && id.is_some_and(|id| self.reload_native_editor(&id, cx))
            }
            operation => {
                self.nsh_selection_revision < u64::MAX
                    && self.content_revision < u64::MAX
                    && !self.masked
                    && self
                        .nsh_composer
                        .as_ref()
                        .is_none_or(|editor| !editor.native.attached())
                    && self.content_revision == request.revision
                    && self.nsh_selection_revision == request.selection_revision
                    && self.apply_nsh_operation(operation, window, cx)
            }
        };
        let _ = request.reply.send(EditorOutcome {
            ticket: request.ticket,
            accepted,
            snapshot: self.nsh_snapshot(),
            selection_revision: self.nsh_selection_revision,
        });
    }

    fn apply_nsh_operation(
        &mut self,
        operation: EditorOperation,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        match operation {
            EditorOperation::Restore { .. } | EditorOperation::ReloadNative => return false,
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
                if text.as_ref() != self.text() || !valid_selection(&text, range) {
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
                if before.as_ref() != self.text()
                    || !self.image_paste
                    || !valid_selection(&after, selection)
                {
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
        }
        self.nsh_selection_revision += 1;
        true
    }
}

pub(super) trait PasteView {
    fn text(&self) -> &str;
    fn html(&self) -> &str;
    fn uris(&self) -> &[String];
    fn files(&self) -> impl Iterator<Item = nsh_platform_gpui::editing::InputPasteFile<'_>>;
}

impl PasteView for InputPastePayload {
    fn text(&self) -> &str {
        self.text()
    }
    fn html(&self) -> &str {
        self.html()
    }
    fn uris(&self) -> &[String] {
        self.uris()
    }
    fn files(&self) -> impl Iterator<Item = nsh_platform_gpui::editing::InputPasteFile<'_>> {
        self.files()
    }
}

impl PasteView for nsh_core::PasteEvent {
    fn text(&self) -> &str {
        &self.text
    }
    fn html(&self) -> &str {
        &self.html
    }
    fn uris(&self) -> &[String] {
        &self.uris
    }
    fn files(&self) -> impl Iterator<Item = nsh_platform_gpui::editing::InputPasteFile<'_>> {
        self.files
            .iter()
            .map(nsh_platform_gpui::editing::InputPasteFile::Synthetic)
    }
}

pub(super) fn pasted_text(
    before: &str,
    selection: (u32, u32),
    payload: &impl PasteView,
    policy: NativeEditorPolicy,
) -> Option<(String, (u32, u32))> {
    if !valid_selection(before, selection) {
        return None;
    }
    let inserted = if payload.uris().is_empty() {
        policy.paste_text(payload.text())
    } else {
        String::new()
    };
    let mut after = before.to_owned();
    if inserted.is_empty() {
        return Some((after, selection));
    }
    let start = utf16_offset(before, selection.0.min(selection.1));
    let end = utf16_offset(before, selection.0.max(selection.1));
    after.replace_range(start..end, &inserted);
    let caret = utf16_index(after[..start + inserted.len()].encode_utf16().count());
    Some((after, (caret, caret)))
}

pub(super) fn paste_text_bytes(payload: &impl PasteView) -> usize {
    payload
        .uris()
        .iter()
        .fold(
            payload.text().len().saturating_add(payload.html().len()),
            |total, uri| total.saturating_add(uri.len()),
        )
        .saturating_add(payload.files().fold(0usize, |total, file| {
            total
                .saturating_add(file.name().len())
                .saturating_add(file.mime().len())
        }))
}

pub(super) fn valid_paste_payload(payload: &impl PasteView) -> bool {
    paste_text_bytes(payload) <= 2 * 1024 * 1024
        && payload.files().count().saturating_add(payload.uris().len()) <= MAX_IMAGES
        && payload
            .files()
            .all(|file| file.bytes().len() <= MAX_IMAGE_BYTES)
        && payload.files().fold(0usize, |total, file| {
            total.saturating_add(file.bytes().len())
        }) <= MAX_TURN_IMAGE_BYTES
        && payload.uris().iter().all(|uri| pasted_path(uri).is_some())
}

fn pasted_attachments(payload: &nsh_core::PasteEvent) -> Option<gpui_kit::ClipboardItem> {
    if !valid_paste_payload(payload) {
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
    ledger: Rc<RefCell<EditLedger>>,
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
        self.ledger.borrow_mut().fail();
        self.gate.close();
        let text = self.value.get_untracked();
        self.draft.save(text.clone());
        self.recovery.set(Some(text));
        self.failed.set(true);
        self.generation
            .update(|generation| *generation = generation.wrapping_add(1));
    }

    fn send_authority(
        &self,
        operation: EditorOperation,
        snapshot: EditorSnapshot,
        failed: Signal<bool>,
    ) {
        if self.pending.get_untracked() != 0 {
            return;
        }
        let Some(ticket) = EditLedger::new().issue() else {
            failed.set(true);
            return;
        };
        self.gate.close();
        self.pending.set(1);
        failed.set(false);
        let dispatch = self.clone();
        spawn(async move {
            let (reply, response) = oneshot::channel();
            let request = EditorRequest {
                ticket: ticket.clone(),
                revision: snapshot.revision,
                selection_revision: snapshot.selection_revision,
                operation,
                reply,
            };
            let accepted = dispatch.commands.send(request).await.is_ok()
                && matches!(response.await, Ok(outcome) if outcome.accepted && ticket.matches(&outcome.ticket));
            if !accepted {
                dispatch.gate.close();
                failed.set(true);
            }
            dispatch.pending.set(0);
        });
    }

    fn send(&self, operation: EditorOperation) {
        self.gate.close();
        if self.failed.get_untracked() {
            return;
        }
        let bytes = match &operation {
            EditorOperation::Restore { after, .. } => after.len(),
            EditorOperation::ReloadNative => 0,
            EditorOperation::Replace { before, after }
            | EditorOperation::Paste { before, after, .. } => {
                before.len().saturating_add(after.len())
            }
            EditorOperation::Select { text, .. } | EditorOperation::Compose { text, .. } => {
                text.len()
            }
        };
        let payload_bytes = match &operation {
            EditorOperation::Paste { payload, .. } => paste_text_bytes(payload.as_ref()),
            _ => 0,
        };
        let bytes = bytes.saturating_add(payload_bytes);
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
        let ticket = self.ledger.borrow_mut().issue();
        let Some(ticket) = ticket else {
            self.fail();
            return;
        };
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
                    ticket: ticket.clone(),
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
                                ticket: receipt,
                                accepted: true,
                                snapshot: Some(snapshot),
                                selection_revision,
                            }) if ticket.matches(&receipt) => {
                                let decision = dispatch.ledger.borrow_mut().accept(&receipt);
                                if decision == ReceiptDecision::Reject {
                                    dispatch.fail();
                                } else {
                                    dispatch.revision.set(snapshot.revision);
                                    dispatch.selection_revision.set(selection_revision);
                                }
                                if decision == ReceiptDecision::Synchronize {
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
    host_failed: Signal<bool>,
    colors: Signal<vega_theme::ThemeColors>,
    host: &ComposerProjection,
) -> EditorView {
    let gate = host.gate.clone();
    let draft = host.draft.clone();
    let first = state.get();
    let dispatch = Dispatch {
        commands,
        previous: Rc::new(RefCell::new(None)),
        ledger: Rc::new(RefCell::new(EditLedger::new())),
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
    let transactional = host.native.is_some();
    let reload_native = host.native.clone();
    let restore_native = host.native.clone();
    let authority_failed = signal(false);
    let failure = dispatch.clone();
    effect(move || {
        if host_failed.get() && !failure.failed.get_untracked() {
            if transactional {
                failure.gate.close();
                failure.failed.set(true);
            } else {
                failure.fail();
            }
        }
    });
    let native = host.native.clone();
    let node = host.node.clone();
    let focus_node = node.clone();
    let sync = dispatch.clone();
    effect(move || {
        let snapshot = state.get();
        if !transactional
            && !sync.failed.get()
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
        if let Some(native) = &native {
            synchronized.set(native.ready(&snapshot.text, snapshot.selection));
            return;
        }
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
    let readonly = dispatch.failed;
    let pending = dispatch.pending;
    let recovery = dispatch.recovery;
    let input_height =
        signal(Typography::BODY * Typography::BODY_LINE_HEIGHT * first.min_rows as f32);
    let input_ref = element_ref();
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
        .on_focus(move || {
            focus_node.send_replace(input_ref.node());
        })
        .on_layout(move |_: &Bounds<Points>| {
            let current = input_ref.node();
            node.send_if_modified(|previous| {
                if *previous == current {
                    false
                } else {
                    *previous = current;
                    true
                }
            });
            let next = focus_revision.get_untracked();
            if next != last_focus.get() {
                last_focus.set(next);
                input_ref.focus();
            }
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
            if transactional {
                return;
            }
            if paste.failed.get_untracked() {
                return;
            }
            paste.gate.close();
            let before: Arc<str> = Arc::from(value.get_untracked());
            let range = selection.get_untracked();
            if !valid_selection(&before, range) {
                paste.fail();
                return;
            }
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
            if !valid_paste_payload(payload) {
                paste.fail();
                return;
            }
            paste.send(EditorOperation::Paste {
                before,
                after,
                selection: next_selection,
                payload: Arc::new(payload.clone()),
            });
        })
        .on_change(move |after: &str| {
            if transactional {
                return;
            }
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
            if transactional {
                return;
            }
            select.gate.close();
            selection.set((range.start, range.end));
            select.send(EditorOperation::Select {
                text: Arc::from(value.get()),
                range: (range.start, range.end),
            });
        })
        .on_composition_change(move |event: &Composition| {
            if transactional {
                return;
            }
            composition.gate.close();
            composing.set(event.composing);
            composition.send(EditorOperation::Compose {
                text: Arc::from(value.get()),
                active: event.composing,
            });
        })
        .into_element();
    let recovery_button = move || {
        nsh_base::button()
            .min_h(px(vega_theme::Layout::SETTINGS_CONTROL_MIN_HEIGHT))
            .px(px(vega_theme::Layout::SETTINGS_CONTROL_PADDING_X))
            .py_1()
            .self_start()
            .flex_row()
            .items_center()
            .justify_center()
            .rounded(px(vega_theme::Layout::SETTINGS_CONTROL_RADIUS))
            .border(px(1.))
            .border_color(move || crate::nsh::color(colors.get().border_subtle))
            .bg(move || crate::nsh::color(colors.get().bg_elevated))
            .text_color(move || crate::nsh::color(colors.get().text_primary))
            .focus(|style| style.border_color(move || crate::nsh::color(colors.get().accent)))
            .hover(|style| style.bg(move || crate::nsh::color(colors.get().bg_hover)))
            .disabled_style(|style| {
                style.text_color(move || crate::nsh::color(colors.get().text_tertiary))
            })
    };
    let element = view()
        .w_full()
        .gap_2()
        .text_size(px(Typography::BODY))
        .line_height(px(Typography::BODY * Typography::BODY_LINE_HEIGHT))
        .text_color(move || crate::nsh::color(colors.get().text_primary))
        .child(input)
        .child(when(
            move || authority_failed.get(),
            move || text(vega_i18n::t("ui.command_failed")),
        ))
        .child(when(
            move || readonly.get(),
            move || {
                let reload = reload.clone();
                let native = reload_native.clone();
                view()
                    .gap_2()
                    .child(text(move || {
                        state.with(|_| vega_i18n::t("ui.editor_sync_failed"))
                    }))
                    .child(
                        recovery_button()
                            .disabled(move || pending.get() != 0 || host_failed.get())
                            .on_press(move || {
                                if reload.pending.get_untracked() != 0
                                    || host_failed.get_untracked()
                                {
                                    return;
                                }
                                let snapshot = state.get_untracked();
                                if let Some(native) = &native {
                                    if native.failed() {
                                        reload.send_authority(
                                            EditorOperation::ReloadNative,
                                            snapshot,
                                            authority_failed,
                                        );
                                    }
                                    return;
                                }
                                reload
                                    .generation
                                    .update(|generation| *generation = generation.wrapping_add(1));
                                *reload.ledger.borrow_mut() = EditLedger::new();
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
                let native = restore_native.clone();
                view()
                    .flex_row()
                    .flex_wrap()
                    .gap_2()
                    .child(
                        recovery_button()
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
                        recovery_button()
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
                                    if let Some(native) = &native {
                                        if let Some(token) = native.restore_token() {
                                            restore.send_authority(
                                                EditorOperation::Restore { token, after },
                                                state.get_untracked(),
                                                authority_failed,
                                            );
                                        } else {
                                            authority_failed.set(true);
                                        }
                                    } else {
                                        restore.gate.close();
                                        let before = Arc::from(value.get_untracked());
                                        restore.set_value(after.clone());
                                        restore.send(EditorOperation::Replace { before, after });
                                    }
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
