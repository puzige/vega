use std::cell::RefCell;
use std::rc::{Rc, Weak};

use nsh_platform_gpui::editing::{
    InputEdit, InputHooks, InputIdentity, InputKey, InputPaste, InputPasteCompletion,
    InputPastePayload, PasteDecision,
};
use tokio::sync::mpsc;

use super::editing_core::{
    ComposerNativeBridge, EditorOwner, NativeCommandToken, NativeEdit, NativeEditorKey,
    NativeKeyContext, NativeKeyDecision, NativeModifiers, NativeMutation,
};
use super::nsh::{DraftRecovery, pasted_text, valid_paste_payload};

#[derive(Default)]
struct PasteUsage {
    count: usize,
    text: usize,
    images: usize,
}

thread_local! {
    static PASTE_USAGE: Rc<RefCell<PasteUsage>> = Rc::default();
}

pub(super) struct PasteBudget {
    usage: Rc<RefCell<PasteUsage>>,
    text: usize,
    images: usize,
}

impl PasteBudget {
    fn reserve(payload: &InputPastePayload, before: &NativeEdit) -> Option<Self> {
        if !valid_paste_payload(payload) {
            return None;
        }
        let (original_text, original_images) = original_storage(payload.original())?;
        let text = payload
            .normalized_storage()?
            .checked_add(original_text)?
            .checked_add(before.text.len().checked_mul(2)?)?
            .checked_add(payload.text().len().checked_mul(2)?)?;
        let images = match payload {
            InputPastePayload::Native(_) => original_images,
            InputPastePayload::Synthetic(_) => payload
                .files()
                .try_fold(0usize, |total, file| total.checked_add(file.capacity()))?,
        };
        if text > 2 * 1024 * 1024 || images > vega_conversation::attachments::MAX_TURN_IMAGE_BYTES {
            return None;
        }
        PASTE_USAGE.with(|usage| {
            let mut current = usage.borrow_mut();
            if current.count >= 64
                || current.text.checked_add(text)? > 2 * 1024 * 1024
                || current.images.checked_add(images)?
                    > vega_conversation::attachments::MAX_TURN_IMAGE_BYTES
            {
                return None;
            }
            current.count += 1;
            current.text += text;
            current.images += images;
            Some(Self {
                usage: usage.clone(),
                text,
                images,
            })
        })
    }
}

impl Drop for PasteBudget {
    fn drop(&mut self) {
        let mut usage = self.usage.borrow_mut();
        usage.count -= 1;
        usage.text -= self.text;
        usage.images -= self.images;
    }
}

fn original_storage(original: Option<&gpui_kit::ClipboardItem>) -> Option<(usize, usize)> {
    let Some(original) = original else {
        return Some((0, 0));
    };
    let mut text = 0usize.checked_add(
        original
            .entries
            .capacity()
            .checked_mul(std::mem::size_of::<gpui_kit::ClipboardEntry>())?,
    )?;
    let mut images = 0usize;
    let mut attachments = 0usize;
    for entry in original.entries() {
        match entry {
            gpui_kit::ClipboardEntry::String(value) => {
                text = text
                    .checked_add(value.text.capacity())?
                    .checked_add(value.metadata.as_ref().map_or(0, String::capacity))?;
            }
            gpui_kit::ClipboardEntry::Image(image) => {
                attachments = attachments.checked_add(1)?;
                if image.bytes.len() > vega_conversation::attachments::MAX_IMAGE_BYTES {
                    return None;
                }
                images = images.checked_add(image.bytes.capacity())?;
            }
            gpui_kit::ClipboardEntry::ExternalPaths(paths) => {
                attachments = attachments.checked_add(paths.paths().len())?;
                if paths.0.spilled() {
                    text = text.checked_add(
                        paths
                            .0
                            .capacity()
                            .checked_mul(std::mem::size_of::<std::path::PathBuf>())?,
                    )?;
                }
                for path in paths.paths() {
                    if !path.is_absolute() || path.as_os_str().len() > 32 * 1024 {
                        return None;
                    }
                    text = text.checked_add(path.capacity())?;
                }
            }
        }
    }
    (attachments <= vega_conversation::attachments::MAX_IMAGES).then_some((text, images))
}

struct AuthorizedPaste {
    after: String,
    selection: (u32, u32),
    payload: Weak<InputPastePayload>,
    budget: PasteBudget,
}

struct PendingPaste {
    before: NativeEdit,
    insertion: Option<AuthorizedPaste>,
}

pub(super) enum NativeDelivery {
    Edit(NativeEdit),
    Paste {
        edit: NativeEdit,
        payload: Rc<InputPastePayload>,
        _budget: PasteBudget,
    },
    Command(NativeCommandToken),
    Failed,
}

pub(super) struct NativeAdapter {
    bridge: ComposerNativeBridge,
    owner: EditorOwner,
    identity: RefCell<Option<InputIdentity>>,
    epoch: RefCell<Option<u64>>,
    initialized: RefCell<bool>,
    composer: bool,
    pending_paste: RefCell<Option<PendingPaste>>,
    draft: DraftRecovery,
    sender: mpsc::Sender<NativeDelivery>,
}

impl NativeAdapter {
    pub(super) fn new(
        bridge: ComposerNativeBridge,
        draft: DraftRecovery,
        sender: mpsc::Sender<NativeDelivery>,
        composer: bool,
    ) -> Option<Rc<Self>> {
        let owner = bridge.attach()?;
        Some(Rc::new(Self {
            bridge,
            owner,
            identity: RefCell::new(None),
            epoch: RefCell::new(None),
            initialized: RefCell::new(false),
            composer,
            pending_paste: RefCell::new(None),
            draft,
            sender,
        }))
    }

    pub(super) fn owner(&self) -> &EditorOwner {
        &self.owner
    }

    pub(super) fn is_failed(&self) -> bool {
        self.bridge.failed()
    }

    pub(super) fn needs_registration(&self, identity: &InputIdentity, trusted_focus: bool) -> bool {
        !self.bridge.failed()
            && self
                .identity
                .borrow()
                .as_ref()
                .is_none_or(|old| trusted_focus && !old.same_focus(identity))
    }

    pub(super) fn hooks(self: &Rc<Self>, identity: InputIdentity) -> InputHooks {
        if self
            .identity
            .borrow()
            .as_ref()
            .is_some_and(|old| !old.same_mount(&identity))
        {
            self.fail();
        }
        let registration = identity.clone();
        self.identity.replace(Some(identity));
        let key = self.clone();
        let edit = self.clone();
        let focus = self.clone();
        let begin = self.clone();
        let paste = self.clone();
        let complete = self.clone();
        let begin_identity = registration.clone();
        let paste_identity = registration.clone();
        let complete_identity = registration.clone();
        InputHooks {
            key: Rc::new(move |event, identity| key.key(event, identity)),
            edit: Rc::new(move |event| edit.edit(event)),
            focus: Rc::new(move |identity, focused| {
                if focus.identity.borrow().as_ref().is_some_and(|current| {
                    current.same_focus(&registration) && current.same_mount(&identity)
                }) {
                    focus
                        .epoch
                        .replace(focus.bridge.focus(&focus.owner, focused));
                    let pending_paste = focus.pending_paste.borrow().is_some();
                    if !focused && pending_paste {
                        focus.fail();
                    }
                }
            }),
            paste_begin: Rc::new(move |event| {
                begin.current(&begin_identity) && begin.paste_begin(event)
            }),
            paste: Rc::new(move |event| {
                if paste.current(&paste_identity) {
                    paste.paste(event)
                } else {
                    PasteDecision::Reject
                }
            }),
            paste_complete: Rc::new(move |event| {
                if complete.current(&complete_identity) {
                    complete.paste_complete(event);
                }
            }),
        }
    }

    fn native_edit(event: &InputEdit) -> NativeEdit {
        NativeEdit {
            text: event.text.as_str().into(),
            selection: (event.anchor_utf16, event.head_utf16),
            composing: event.composing,
            serial: event.native_edit_serial,
            mutation: event.mutation.as_ref().map(|mutation| NativeMutation {
                range_utf16: mutation.range_utf16,
                inserted: mutation.inserted.as_str().into(),
            }),
        }
    }

    fn paste_begin(&self, event: InputEdit) -> bool {
        if !self.current(&event.identity) {
            return false;
        }
        self.begin_paste(Self::native_edit(&event))
    }

    fn begin_paste(&self, before: NativeEdit) -> bool {
        if self.pending_paste.borrow().is_some() || !self.bridge.begin_paste(&self.owner, &before) {
            self.fail();
            return false;
        }
        self.pending_paste.replace(Some(PendingPaste {
            before,
            insertion: None,
        }));
        true
    }

    fn paste(&self, event: InputPaste<'_>) -> PasteDecision {
        if !self.current(&event.before.identity) {
            self.fail();
            return PasteDecision::Reject;
        }
        self.prepare_paste(&Self::native_edit(event.before), event.payload)
    }

    fn prepare_paste(&self, before: &NativeEdit, payload: &Rc<InputPastePayload>) -> PasteDecision {
        let prepared = (|| {
            let mut pending = self.pending_paste.borrow_mut();
            let pending = pending.as_mut()?;
            if self.bridge.failed()
                || !pending.before.same_snapshot(before)
                || pending.insertion.is_some()
                || (!self.composer
                    && (payload.files().next().is_some() || !payload.uris().is_empty()))
            {
                return None;
            }
            let budget = PasteBudget::reserve(payload, before)?;
            for file in payload.files() {
                if !matches!(
                    nsh_core::platform::clipboard::image_mime(file.bytes()),
                    Some("image/png" | "image/jpeg" | "image/webp")
                ) {
                    return None;
                }
            }
            let policy = self.bridge.pending_paste_policy()?;
            let (after, selection) =
                pasted_text(&before.text, before.selection, payload.as_ref(), policy)?;
            if after.len() > 2 * 1024 * 1024 {
                return None;
            }
            let inserted = if payload.uris().is_empty() {
                policy.paste_text(payload.text())
            } else {
                String::new()
            };
            pending.insertion = Some(AuthorizedPaste {
                after,
                selection,
                payload: Rc::downgrade(payload),
                budget,
            });
            Some(inserted)
        })();
        match prepared {
            Some(inserted) => PasteDecision::Insert(inserted),
            None => {
                self.fail();
                PasteDecision::Reject
            }
        }
    }

    fn paste_complete(&self, completion: Option<InputPasteCompletion>) {
        let Some(completion) = completion else {
            self.fail();
            return;
        };
        if !self.current(&completion.edit.identity) {
            self.fail();
            return;
        }
        self.complete_paste(Self::native_edit(&completion.edit), completion.payload);
    }

    fn complete_paste(&self, edit: NativeEdit, payload: Rc<InputPastePayload>) {
        if self.bridge.failed() {
            return;
        }
        let pending = self.pending_paste.borrow_mut().take();
        let accepted = (|| {
            let pending = pending?;
            let authorized = pending.insertion?;
            if !authorized.payload.ptr_eq(&Rc::downgrade(&payload))
                || Rc::strong_count(&payload) != 1
            {
                return None;
            }
            self.draft.local(&edit.text);
            if authorized.after != edit.text.as_ref()
                || authorized.selection != edit.selection
                || edit.composing
                || !self
                    .bridge
                    .observe_paste(&self.owner, &pending.before, edit.clone())
            {
                return None;
            }
            self.sender
                .try_send(NativeDelivery::Paste {
                    edit,
                    payload,
                    _budget: authorized.budget,
                })
                .ok()
        })();
        if accepted.is_none() {
            self.fail();
        }
    }

    fn current(&self, identity: &InputIdentity) -> bool {
        self.identity
            .borrow()
            .as_ref()
            .is_some_and(|current| current.same_focus(identity))
    }

    pub(super) fn fail(&self) {
        self.bridge.fail();
        self.pending_paste.borrow_mut().take();
        self.draft.preserve();
        let _ = self.sender.try_send(NativeDelivery::Failed);
    }

    fn edit(&self, event: InputEdit) {
        if !self.current(&event.identity) {
            return;
        }
        self.receive(NativeEdit {
            text: event.text.into(),
            selection: (event.anchor_utf16, event.head_utf16),
            composing: event.composing,
            serial: event.native_edit_serial,
            mutation: event.mutation.map(|mutation| NativeMutation {
                range_utf16: mutation.range_utf16,
                inserted: mutation.inserted.into(),
            }),
        });
    }

    pub(super) fn receive(&self, edit: NativeEdit) {
        if self.bridge.failed() {
            return;
        }
        self.draft.local(&edit.text);
        if !*self.initialized.borrow() {
            if !self.bridge.initialize(&self.owner, &edit) {
                self.draft.preserve();
                self.fail();
                return;
            }
            self.initialized.replace(true);
        }
        if !self.bridge.observe(&self.owner, edit.clone())
            || self.sender.try_send(NativeDelivery::Edit(edit)).is_err()
        {
            self.fail();
        }
    }

    fn key(&self, event: InputKey, identity: InputIdentity) -> bool {
        if !event.down || !self.current(&identity) {
            return false;
        }
        let Some(epoch) = *self.epoch.borrow() else {
            return false;
        };
        self.route_key(event, epoch)
    }

    fn route_key(&self, event: InputKey, epoch: u64) -> bool {
        if !self.composer
            && !self.bridge.has_single_line_policy()
            && !matches!(event.virtual_key, 0x59 | 0x5a)
        {
            return false;
        }
        let key = match event.virtual_key {
            0x0d => NativeEditorKey::Return,
            0x1b => NativeEditorKey::Escape,
            0x09 => NativeEditorKey::Tab,
            0x26 => NativeEditorKey::Up,
            0x28 => NativeEditorKey::Down,
            0x27 => NativeEditorKey::Right,
            0x5a if event.shift => NativeEditorKey::Redo,
            0x5a => NativeEditorKey::Undo,
            0x59 => NativeEditorKey::Redo,
            _ => NativeEditorKey::Other,
        };
        match self.bridge.key_with_context(
            &self.owner,
            epoch,
            key,
            NativeModifiers {
                shift: event.shift,
                control: event.control,
                alt: event.alt,
                meta: event.meta,
            },
            NativeKeyContext {
                composing: false,
                repeat: event.repeat,
                input: event.context,
            },
        ) {
            NativeKeyDecision::Pass => false,
            NativeKeyDecision::Consume => true,
            NativeKeyDecision::Command(token) => {
                if self
                    .sender
                    .try_send(NativeDelivery::Command(token))
                    .is_err()
                {
                    self.fail();
                }
                true
            }
        }
    }
}
