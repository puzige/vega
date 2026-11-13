use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug)]
pub struct EditorOwner(Arc<()>);

impl EditorOwner {
    pub(crate) fn new() -> Self {
        Self(Arc::new(()))
    }

    pub(crate) fn matches(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct EditTicket {
    owner: EditorOwner,
    sequence: u64,
}

impl EditTicket {
    pub(crate) fn matches(&self, other: &Self) -> bool {
        self.owner.matches(&other.owner) && self.sequence == other.sequence
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ReceiptDecision {
    Advance,
    Synchronize,
    Reject,
}

pub(crate) struct EditLedger {
    owner: EditorOwner,
    sequence: u64,
    pending: VecDeque<u64>,
    failed: bool,
}

impl EditLedger {
    pub(crate) fn new() -> Self {
        Self {
            owner: EditorOwner::new(),
            sequence: 0,
            pending: VecDeque::new(),
            failed: false,
        }
    }

    pub(crate) fn issue(&mut self) -> Option<EditTicket> {
        if self.failed {
            return None;
        }
        let Some(sequence) = self.sequence.checked_add(1) else {
            self.failed = true;
            return None;
        };
        self.sequence = sequence;
        self.pending.push_back(sequence);
        Some(EditTicket {
            owner: self.owner.clone(),
            sequence,
        })
    }

    pub(crate) fn accept(&mut self, ticket: &EditTicket) -> ReceiptDecision {
        if self.failed
            || !self.owner.matches(&ticket.owner)
            || self.pending.front() != Some(&ticket.sequence)
        {
            self.fail();
            return ReceiptDecision::Reject;
        }
        self.pending.pop_front();
        if ticket.sequence == self.sequence && self.pending.is_empty() {
            ReceiptDecision::Synchronize
        } else {
            ReceiptDecision::Advance
        }
    }

    pub(crate) fn fail(&mut self) {
        self.failed = true;
    }
}

pub(crate) fn valid_selection(text: &str, selection: (u32, u32)) -> bool {
    let mut offset = 0usize;
    let mut anchor = selection.0 == 0;
    let mut head = selection.1 == 0;
    for ch in text.chars() {
        offset += ch.len_utf16();
        anchor |= offset == selection.0 as usize;
        head |= offset == selection.1 as usize;
    }
    anchor && head
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeMutation {
    pub range_utf16: (u32, u32),
    pub inserted: Arc<str>,
}

impl NativeMutation {
    pub(crate) fn byte_range(&self, before: &str) -> Option<std::ops::Range<usize>> {
        if self.range_utf16.0 > self.range_utf16.1 {
            return None;
        }
        let boundary = |target: u32| {
            let mut units = 0usize;
            for (byte, ch) in before.char_indices() {
                if units == target as usize {
                    return Some(byte);
                }
                units += ch.len_utf16();
            }
            (units == target as usize).then_some(before.len())
        };
        Some(boundary(self.range_utf16.0)?..boundary(self.range_utf16.1)?)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeEdit {
    pub text: Arc<str>,
    pub selection: (u32, u32),
    pub composing: bool,
    pub serial: u64,
    pub mutation: Option<NativeMutation>,
}

impl NativeEdit {
    pub(crate) fn same_snapshot(&self, other: &Self) -> bool {
        self.text == other.text
            && self.selection == other.selection
            && self.composing == other.composing
            && self.serial == other.serial
    }

    pub(crate) fn follows(&self, before: &Self) -> bool {
        let Some(mutation) = &self.mutation else {
            return self.text == before.text;
        };
        let Some(range) = mutation.byte_range(&before.text) else {
            return false;
        };
        let suffix = &before.text[range.end..];
        let Some(inserted_end) = range.start.checked_add(mutation.inserted.len()) else {
            return false;
        };
        inserted_end.checked_add(suffix.len()) == Some(self.text.len())
            && self.text.get(..range.start) == Some(&before.text[..range.start])
            && self.text.get(range.start..inserted_end) == Some(mutation.inserted.as_ref())
            && self.text.get(inserted_end..) == Some(suffix)
    }

    fn retained_bytes(&self) -> usize {
        self.text.len().saturating_add(
            self.mutation
                .as_ref()
                .map_or(0, |mutation| mutation.inserted.len()),
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeEditorKey {
    Return,
    Escape,
    Tab,
    Up,
    Down,
    Right,
    Undo,
    Redo,
    Other,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NativeModifiers {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    pub meta: bool,
}

pub(super) struct NativeKeyContext {
    pub composing: bool,
    pub repeat: bool,
    pub input: Option<nsh_platform_gpui::editing::InputKeyContext>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NativeEditorPolicy {
    #[default]
    Composer,
    SingleLine(gpui_kit::EntityId),
}

impl NativeEditorPolicy {
    pub(super) fn paste_text(self, text: &str) -> String {
        match self {
            Self::Composer => text.replace("\r\n", "\n"),
            Self::SingleLine(_) => text.replace('\n', " "),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct NativePasteContext {
    policy: NativeEditorPolicy,
    policy_epoch: u64,
    focus_epoch: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NativeEditorRoute {
    pub policy: NativeEditorPolicy,
    pub popup: bool,
    pub prediction: bool,
    pub history: bool,
    pub recalling_history: bool,
    pub file_generation: u64,
    pub file_highlight: usize,
    pub action_generation: u64,
    pub action_highlight: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeEditorCommand {
    Submit,
    Accept,
    Cancel,
    Previous,
    Next,
    History,
    FocusNext,
    FocusPrevious,
    AcceptPrediction,
    DismissPrediction,
    Undo,
    Redo,
}

#[derive(Clone, Debug)]
pub struct NativeCommandToken {
    pub(super) owner: EditorOwner,
    pub(super) focus_epoch: u64,
    pub(super) edit: NativeEdit,
    pub(super) route: NativeEditorRoute,
    pub(super) command: NativeEditorCommand,
    claimed: Arc<AtomicBool>,
}

impl NativeCommandToken {
    pub(crate) fn claim(&self) -> bool {
        !self.claimed.swap(true, Ordering::AcqRel)
    }
}

#[derive(Clone, Debug)]
pub enum NativeKeyDecision {
    Pass,
    Consume,
    Command(NativeCommandToken),
}

struct NativeState {
    owner: EditorOwner,
    active: bool,
    attached: bool,
    focused: bool,
    focus_epoch: u64,
    policy_epoch: u64,
    edit: NativeEdit,
    accepted: NativeEdit,
    revisions: (u64, u64),
    command_pending: bool,
    pending_paste: Option<(NativeEdit, NativePasteContext)>,
    paste_contexts: VecDeque<(u64, NativePasteContext)>,
    pending: VecDeque<NativeEdit>,
    pending_bytes: usize,
    settled: Option<NativeEdit>,
    failed: bool,
    route: NativeEditorRoute,
}

#[derive(Clone)]
pub(crate) struct NativeRestoreToken {
    owner: EditorOwner,
    edit: NativeEdit,
    revisions: (u64, u64),
}

#[derive(Clone)]
pub struct ComposerNativeBridge {
    state: Arc<Mutex<NativeState>>,
    gate: super::nsh::EditorGate,
}

impl ComposerNativeBridge {
    pub(crate) fn new(
        edit: NativeEdit,
        revisions: (u64, u64),
        gate: super::nsh::EditorGate,
    ) -> Self {
        Self {
            state: Arc::new(Mutex::new(NativeState {
                owner: EditorOwner::new(),
                active: true,
                attached: false,
                focused: false,
                focus_epoch: 0,
                policy_epoch: 0,
                accepted: edit.clone(),
                revisions,
                command_pending: false,
                pending_paste: None,
                paste_contexts: VecDeque::new(),
                pending: VecDeque::new(),
                pending_bytes: 0,
                edit,
                settled: None,
                failed: false,
                route: NativeEditorRoute::default(),
            })),
            gate,
        }
    }

    pub fn attach(&self) -> Option<EditorOwner> {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if !state.active || state.attached || state.failed {
            return None;
        }
        state.attached = true;
        state.settled = (!state.edit.composing).then(|| state.edit.clone());
        self.gate.close();
        Some(state.owner.clone())
    }

    pub(crate) fn initialize(&self, owner: &EditorOwner, edit: &NativeEdit) -> bool {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if !state.active
            || !state.attached
            || state.failed
            || !state.owner.matches(owner)
            || state.edit.serial != 0
            || !state.pending.is_empty()
            || edit.serial == 0
            || edit.mutation.is_some()
            || state.edit.text != edit.text
            || state.edit.selection != edit.selection
            || state.edit.composing != edit.composing
        {
            if state.active && state.owner.matches(owner) {
                state.edit = edit.clone();
                state.failed = true;
                state.settled = None;
                self.gate.close();
            }
            return false;
        }
        state.edit.serial = edit.serial - 1;
        state.accepted.serial = edit.serial - 1;
        state.settled = None;
        true
    }

    pub(crate) fn failed(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .failed
    }

    pub fn focus(&self, owner: &EditorOwner, focused: bool) -> Option<u64> {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if !state.active || !state.attached || !state.owner.matches(owner) || state.failed {
            return None;
        }
        let Some(epoch) = state.focus_epoch.checked_add(1) else {
            state.failed = true;
            state.focused = false;
            self.gate.close();
            return None;
        };
        state.focus_epoch = epoch;
        state.focused = focused;
        state.command_pending = false;
        Some(state.focus_epoch)
    }

    pub(crate) fn begin_paste(&self, owner: &EditorOwner, before: &NativeEdit) -> bool {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        self.gate.close();
        if !state.active
            || !state.attached
            || !state.focused
            || state.failed
            || !state.owner.matches(owner)
            || state.pending_paste.is_some()
            || !state.edit.same_snapshot(before)
            || before.composing
        {
            return false;
        }
        let context = Self::paste_context(&state);
        state.pending_paste = Some((before.clone(), context));
        state.settled = None;
        true
    }

    pub(crate) fn observe_paste(
        &self,
        owner: &EditorOwner,
        before: &NativeEdit,
        edit: NativeEdit,
    ) -> bool {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        self.gate.close();
        if !state.active
            || !state.attached
            || !state.focused
            || state.failed
            || !state.owner.matches(owner)
            || !state
                .pending_paste
                .as_ref()
                .is_some_and(|(pending, context)| {
                    pending.same_snapshot(before) && *context == Self::paste_context(&state)
                })
            || !state.edit.same_snapshot(before)
        {
            return false;
        }
        let Some((_, context)) = state.pending_paste.take() else {
            return false;
        };
        let serial = edit.serial;
        if !Self::observe_locked(&mut state, edit) {
            return false;
        }
        state.paste_contexts.push_back((serial, context));
        true
    }

    fn paste_context(state: &NativeState) -> NativePasteContext {
        NativePasteContext {
            policy: state.route.policy,
            policy_epoch: state.policy_epoch,
            focus_epoch: state.focus_epoch,
        }
    }

    pub(super) fn pending_paste_policy(&self) -> Option<NativeEditorPolicy> {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        let (_, context) = state.pending_paste.as_ref()?;
        (state.active && !state.failed && *context == Self::paste_context(&state))
            .then_some(context.policy)
    }

    pub(super) fn paste_policy(&self, edit: &NativeEdit) -> Option<NativeEditorPolicy> {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        let (_, context) = state
            .paste_contexts
            .iter()
            .find(|(serial, _)| *serial == edit.serial)?;
        (state.active && !state.failed && *context == Self::paste_context(&state))
            .then_some(context.policy)
    }

    pub fn observe(&self, owner: &EditorOwner, edit: NativeEdit) -> bool {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if !state.active || !state.attached || !state.owner.matches(owner) || state.failed {
            return false;
        }
        self.gate.close();
        if state.pending_paste.is_some() {
            state.edit = edit;
            state.failed = true;
            state.settled = None;
            return false;
        }
        Self::observe_locked(&mut state, edit)
    }

    fn observe_locked(state: &mut NativeState, edit: NativeEdit) -> bool {
        if edit.serial <= state.edit.serial {
            state.failed = true;
            state.settled = None;
            return false;
        }
        let valid = state.edit.serial.checked_add(1) == Some(edit.serial)
            && valid_selection(&edit.text, edit.selection)
            && edit.follows(&state.edit)
            && state.pending.len() < 64
            && state.pending_bytes.saturating_add(edit.retained_bytes()) <= 2 * 1024 * 1024;
        state.edit = edit.clone();
        state.settled = None;
        if !valid {
            state.failed = true;
            return false;
        }
        state.pending_bytes += edit.retained_bytes();
        state.pending.push_back(edit);
        true
    }

    pub fn local_draft(&self) -> NativeEdit {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .edit
            .clone()
    }

    pub(crate) fn commit(
        &self,
        owner: &EditorOwner,
        edit: &NativeEdit,
        apply: impl FnOnce(&NativeEdit, (u64, u64)) -> Option<(u64, u64)>,
    ) -> bool {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if !state.active || !state.attached || state.failed || !state.owner.matches(owner) {
            return false;
        }
        if state.accepted.serial.checked_add(1) != Some(edit.serial)
            || state.pending.front() != Some(edit)
            || !valid_selection(&edit.text, edit.selection)
            || !edit.follows(&state.accepted)
            || state.paste_contexts.iter().any(|(serial, context)| {
                *serial == edit.serial && *context != Self::paste_context(&state)
            })
        {
            state.failed = true;
            state.settled = None;
            self.gate.close();
            return false;
        }
        let Some(revisions) = apply(&state.accepted, state.revisions) else {
            state.failed = true;
            state.settled = None;
            self.gate.close();
            return false;
        };
        state.revisions = revisions;
        state.pending.pop_front();
        if state
            .paste_contexts
            .front()
            .is_some_and(|(serial, _)| *serial == edit.serial)
        {
            state.paste_contexts.pop_front();
        }
        state.pending_bytes = state.pending_bytes.saturating_sub(edit.retained_bytes());
        state.accepted = edit.clone();
        if state.edit == *edit && !edit.composing && state.pending_paste.is_none() {
            state.settled = Some(edit.clone());
        }
        true
    }

    pub(crate) fn project(
        &self,
        owner: &EditorOwner,
        expected_serial: u64,
        text: Arc<str>,
        selection: (u32, u32),
        revisions: (u64, u64),
        apply: impl FnOnce(&NativeEdit) -> bool,
    ) -> bool {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if !state.active
            || !state.attached
            || state.failed
            || !state.owner.matches(owner)
            || state.edit.serial != expected_serial
            || !state.pending.is_empty()
            || state.pending_paste.is_some()
            || state.edit.composing
            || revisions.0 < state.revisions.0
            || revisions.1 < state.revisions.1
            || !valid_selection(&text, selection)
        {
            return false;
        }
        let Some(serial) = expected_serial.checked_add(1) else {
            state.failed = true;
            self.gate.close();
            return false;
        };
        let edit = NativeEdit {
            text,
            selection,
            composing: false,
            serial,
            mutation: None,
        };
        self.gate.close();
        if !apply(&edit) {
            state.failed = true;
            state.settled = None;
            return false;
        }
        state.revisions = revisions;
        state.edit = edit.clone();
        state.accepted = edit.clone();
        state.settled = Some(edit);
        true
    }

    pub(crate) fn ready(&self, text: &str, selection: (u32, u32)) -> bool {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.active
            && !state.failed
            && state.pending_paste.is_none()
            && (!state.attached
                || (state.settled.as_ref() == Some(&state.edit)
                    && !state.edit.composing
                    && state.edit.text.as_ref() == text
                    && state.edit.selection == selection))
    }

    pub(super) fn has_single_line_policy(&self) -> bool {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        matches!(state.route.policy, NativeEditorPolicy::SingleLine(_))
    }

    pub(crate) fn route(&self, route: NativeEditorRoute) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if state.route.policy != route.policy {
            let Some(epoch) = state.policy_epoch.checked_add(1) else {
                state.failed = true;
                state.settled = None;
                self.gate.close();
                return;
            };
            state.policy_epoch = epoch;
        }
        state.route = route;
    }

    pub(crate) fn seal_authority(
        &self,
        revisions: (u64, u64),
        text: &str,
        selection: (u32, u32),
    ) -> bool {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if !state.active
            || state.failed
            || !state.pending.is_empty()
            || state.pending_paste.is_some()
            || state.edit.composing
        {
            return false;
        }
        if revisions.0 < state.revisions.0
            || revisions.1 < state.revisions.1
            || !valid_selection(text, selection)
        {
            return false;
        }
        state.active = false;
        state.focused = false;
        state.settled = None;
        self.gate.close();
        true
    }

    pub(super) fn restore_token(&self) -> Option<NativeRestoreToken> {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        (state.active
            && state.attached
            && !state.failed
            && !state.command_pending
            && state.pending.is_empty()
            && state.pending_paste.is_none()
            && !state.edit.composing
            && state.settled.as_ref() == Some(&state.edit))
        .then(|| NativeRestoreToken {
            owner: state.owner.clone(),
            edit: state.edit.clone(),
            revisions: state.revisions,
        })
    }

    pub(super) fn restore_authority(
        &self,
        token: &NativeRestoreToken,
        revisions: (u64, u64),
        text: &str,
        selection: (u32, u32),
        apply: impl FnOnce() -> bool,
    ) -> bool {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if !state.active
            || !state.attached
            || state.failed
            || !state.owner.matches(&token.owner)
            || state.edit != token.edit
            || state.revisions != token.revisions
            || revisions != token.revisions
            || state.accepted.text.as_ref() != text
            || state.accepted.selection != selection
            || state.command_pending
            || !state.pending.is_empty()
            || state.pending_paste.is_some()
            || state.edit.composing
            || state.settled.as_ref() != Some(&state.edit)
        {
            return false;
        }
        self.gate.close();
        if !apply() {
            return false;
        }
        state.active = false;
        state.focused = false;
        state.settled = None;
        true
    }

    pub(crate) fn authority_matches(
        &self,
        revisions: (u64, u64),
        text: &str,
        selection: (u32, u32),
    ) -> bool {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.revisions == revisions
            && state.accepted.text.as_ref() == text
            && state.accepted.selection == selection
    }

    pub(crate) fn retire(&self) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.active = false;
        state.focused = false;
        state.settled = None;
        self.gate.close();
    }

    pub(crate) fn fail(&self) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.failed = true;
        state.settled = None;
        self.gate.close();
    }

    pub(crate) fn attached(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .attached
    }

    pub fn key(
        &self,
        owner: &EditorOwner,
        focus_epoch: u64,
        key: NativeEditorKey,
        modifiers: NativeModifiers,
        native_composing: bool,
        repeat: bool,
    ) -> NativeKeyDecision {
        self.key_with_context(
            owner,
            focus_epoch,
            key,
            modifiers,
            NativeKeyContext {
                composing: native_composing,
                repeat,
                input: None,
            },
        )
    }

    pub(super) fn key_with_context(
        &self,
        owner: &EditorOwner,
        focus_epoch: u64,
        key: NativeEditorKey,
        modifiers: NativeModifiers,
        context: NativeKeyContext,
    ) -> NativeKeyDecision {
        let NativeKeyContext {
            composing: native_composing,
            repeat,
            input: context,
        } = context;
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if native_composing
            || state.edit.composing
            || !state.active
            || !state.attached
            || !state.focused
            || !state.owner.matches(owner)
            || focus_epoch != state.focus_epoch
        {
            return NativeKeyDecision::Pass;
        }
        let primary = if cfg!(target_os = "macos") {
            modifiers.meta && !modifiers.control
        } else {
            modifiers.control && !modifiers.meta
        };
        let plain = !modifiers.meta && !modifiers.control && !modifiers.alt;
        let command = match key {
            NativeEditorKey::Return
                if plain
                    && !modifiers.shift
                    && matches!(state.route.policy, NativeEditorPolicy::SingleLine(_)) =>
            {
                Some(NativeEditorCommand::Submit)
            }
            NativeEditorKey::Escape
                if plain
                    && !modifiers.shift
                    && matches!(state.route.policy, NativeEditorPolicy::SingleLine(_)) =>
            {
                Some(NativeEditorCommand::Cancel)
            }
            NativeEditorKey::Return if primary && !modifiers.shift && !modifiers.alt => {
                Some(NativeEditorCommand::Submit)
            }
            NativeEditorKey::Return | NativeEditorKey::Tab
                if plain && !modifiers.shift && state.route.popup =>
            {
                Some(NativeEditorCommand::Accept)
            }
            NativeEditorKey::Escape if plain && state.route.popup => {
                Some(NativeEditorCommand::Cancel)
            }
            NativeEditorKey::Up if plain && state.route.popup => {
                Some(NativeEditorCommand::Previous)
            }
            NativeEditorKey::Down if plain && state.route.popup => Some(NativeEditorCommand::Next),
            NativeEditorKey::Return | NativeEditorKey::Tab | NativeEditorKey::Right
                if plain && !modifiers.shift && state.route.prediction =>
            {
                Some(NativeEditorCommand::AcceptPrediction)
            }
            NativeEditorKey::Escape if plain && state.route.prediction => {
                Some(NativeEditorCommand::DismissPrediction)
            }
            NativeEditorKey::Tab if plain && modifiers.shift => {
                Some(NativeEditorCommand::FocusPrevious)
            }
            NativeEditorKey::Tab if plain => Some(NativeEditorCommand::FocusNext),
            NativeEditorKey::Up
                if plain
                    && !modifiers.shift
                    && state.route.history
                    && context.is_some_and(|context| {
                        context.native_edit_serial == state.edit.serial
                            && (state.route.recalling_history
                                || (context.first_visual_row
                                    && context.viewport_top
                                    && context.selection_collapsed
                                    && state.edit.selection.0 == state.edit.selection.1))
                    }) =>
            {
                Some(NativeEditorCommand::History)
            }
            NativeEditorKey::Undo if primary && !modifiers.alt => Some(NativeEditorCommand::Undo),
            NativeEditorKey::Redo if primary && !modifiers.alt => Some(NativeEditorCommand::Redo),
            _ => None,
        };
        let Some(command) = command else {
            return NativeKeyDecision::Pass;
        };
        if state.failed
            || state.edit.composing
            || repeat
            || state.command_pending
            || state.pending_paste.is_some()
            || state.settled.as_ref() != Some(&state.edit)
        {
            return NativeKeyDecision::Consume;
        }
        state.command_pending = true;
        NativeKeyDecision::Command(NativeCommandToken {
            owner: state.owner.clone(),
            focus_epoch,
            edit: state.edit.clone(),
            route: state.route,
            command,
            claimed: Arc::new(AtomicBool::new(false)),
        })
    }

    pub fn finish_command(&self, token: &NativeCommandToken) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if state.owner.matches(&token.owner) && state.focus_epoch == token.focus_epoch {
            state.command_pending = false;
        }
    }

    pub fn validates(&self, token: &NativeCommandToken) -> bool {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.active
            && state.attached
            && state.focused
            && !state.failed
            && state.pending_paste.is_none()
            && state.owner.matches(&token.owner)
            && state.focus_epoch == token.focus_epoch
            && state.edit == token.edit
            && !state.edit.composing
            && state.settled.as_ref() == Some(&token.edit)
            && state.route == token.route
    }
}
