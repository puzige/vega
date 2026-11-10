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

#[cfg(test)]
mod tests {
    use super::super::editing_core::{NativeEditorCommand, NativeEditorPolicy};
    use super::super::nsh::EditorGate;
    use super::*;

    fn edit(text: &str, selection: (u32, u32), serial: u64) -> NativeEdit {
        NativeEdit {
            text: text.into(),
            selection,
            composing: false,
            serial,
            mutation: None,
        }
    }

    fn mutated(mut edit: NativeEdit, range_utf16: (u32, u32), inserted: &str) -> NativeEdit {
        edit.mutation = Some(NativeMutation {
            range_utf16,
            inserted: inserted.into(),
        });
        edit
    }

    fn adapter(capacity: usize) -> (Rc<NativeAdapter>, mpsc::Receiver<NativeDelivery>) {
        let bridge = ComposerNativeBridge::new(edit("draft", (5, 5), 0), (0, 0), EditorGate::new());
        let (sender, receiver) = mpsc::channel(capacity);
        (
            NativeAdapter::new(bridge, DraftRecovery::new("draft".into()), sender, true)
                .expect("adapter"),
            receiver,
        )
    }

    fn receive_edit(receiver: &mut mpsc::Receiver<NativeDelivery>) -> NativeEdit {
        match receiver.try_recv().expect("delivery") {
            NativeDelivery::Edit(edit) => edit,
            _ => panic!("expected application edit"),
        }
    }

    fn key(vk: u16, primary: bool) -> InputKey {
        InputKey {
            virtual_key: vk,
            shift: false,
            control: primary && !cfg!(target_os = "macos"),
            alt: false,
            meta: primary && cfg!(target_os = "macos"),
            down: true,
            repeat: false,
            context: None,
        }
    }

    #[test]
    fn initial_snapshot_and_reregister_snapshot_do_not_duplicate_draft_history() {
        let (adapter, mut receiver) = adapter(64);
        adapter.receive(edit("draft", (5, 5), 7));
        let initial = receive_edit(&mut receiver);
        let mut revisions = (0, 0);
        assert!(
            adapter
                .bridge
                .commit(adapter.owner(), &initial, |before, current| {
                    assert_eq!(before.text.as_ref(), "draft");
                    assert_eq!(current, revisions);
                    revisions.1 += 1;
                    Some(revisions)
                })
        );
        adapter.receive(edit("draft", (5, 5), 8));
        let refocused = receive_edit(&mut receiver);
        assert!(
            adapter
                .bridge
                .commit(adapter.owner(), &refocused, |before, current| {
                    assert_eq!(before.text, refocused.text);
                    assert_eq!(current, (0, 1));
                    Some((0, 2))
                })
        );
        assert!(adapter.bridge.ready("draft", (5, 5)));
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn conflicting_initial_native_draft_is_preserved_and_not_applied() {
        let (adapter, mut receiver) = adapter(64);
        adapter.receive(edit("unsent 中文", (9, 9), 1));
        assert!(matches!(receiver.try_recv(), Ok(NativeDelivery::Failed)));
        assert!(adapter.is_failed());
        assert_eq!(adapter.draft.saved().as_deref(), Some("unsent 中文"));
        assert_eq!(adapter.bridge.local_draft().text.as_ref(), "unsent 中文");
        adapter.receive(edit("stale echo", (10, 10), 2));
        assert_eq!(adapter.draft.saved().as_deref(), Some("unsent 中文"));
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn application_queue_overflow_preserves_latest_native_text_without_retry() {
        let (adapter, mut receiver) = adapter(1);
        adapter.receive(edit("draft", (5, 5), 1));
        adapter.receive(mutated(edit("draft 中文", (8, 8), 2), (5, 5), " 中文"));
        assert!(adapter.is_failed());
        assert_eq!(adapter.draft.saved().as_deref(), Some("draft 中文"));
        let queued = receive_edit(&mut receiver);
        let mut writes = 0;
        assert!(!adapter.bridge.commit(adapter.owner(), &queued, |_, _| {
            writes += 1;
            Some((1, 1))
        }));
        assert_eq!(writes, 0);
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn dropped_application_receiver_preserves_local_draft_and_never_replays() {
        let (adapter, receiver) = adapter(64);
        drop(receiver);
        adapter.receive(edit("draft", (5, 5), 1));
        assert!(adapter.is_failed());
        assert_eq!(adapter.draft.saved().as_deref(), Some("draft"));
        adapter.receive(edit("later", (5, 5), 2));
        assert_eq!(adapter.draft.saved().as_deref(), Some("draft"));
    }

    #[test]
    fn actual_key_translation_keeps_newline_and_single_claim_submit_separate() {
        let (adapter, mut receiver) = adapter(64);
        adapter.receive(edit("draft", (5, 5), 1));
        let initial = receive_edit(&mut receiver);
        assert!(
            adapter
                .bridge
                .commit(adapter.owner(), &initial, |_, _| Some((0, 1)))
        );
        let epoch = adapter.bridge.focus(adapter.owner(), true).expect("focus");
        assert!(!adapter.route_key(key(0x0d, false), epoch));
        assert!(receiver.try_recv().is_err());
        assert!(adapter.route_key(key(0x0d, true), epoch));
        let NativeDelivery::Command(token) = receiver.try_recv().expect("submit") else {
            panic!("command");
        };
        assert_eq!(token.command, NativeEditorCommand::Submit);
        assert!(token.claim());
        assert!(!token.claim());
        adapter.bridge.focus(adapter.owner(), false).expect("blur");
        assert!(!adapter.bridge.validates(&token));
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn explicit_single_line_keys_deliver_only_after_latest_receipt_and_reject_rebound_route() {
        use super::super::editing_core::{NativeEditorPolicy, NativeEditorRoute};
        let (adapter, mut receiver, epoch) = settled_adapter();
        let mut route = NativeEditorRoute {
            policy: NativeEditorPolicy::SingleLine(gpui_kit::EntityId::from(1_u64)),
            ..Default::default()
        };
        adapter.bridge.route(route);
        adapter.receive(mutated(edit("draft 中文", (8, 8), 2), (5, 5), " 中文"));
        let latest = receive_edit(&mut receiver);
        for vk in [0x0d, 0x1b] {
            assert!(adapter.route_key(key(vk, false), epoch));
            assert!(receiver.try_recv().is_err());
        }
        assert!(
            adapter
                .bridge
                .commit(adapter.owner(), &latest, |_, _| Some((1, 2)))
        );
        assert!(receiver.try_recv().is_err());
        assert!(adapter.route_key(key(0x0d, false), epoch));
        let NativeDelivery::Command(submit) = receiver.try_recv().expect("submit") else {
            panic!("submit command");
        };
        assert_eq!(submit.command, NativeEditorCommand::Submit);
        assert_eq!(submit.edit.text.as_ref(), "draft 中文");
        assert!(adapter.bridge.validates(&submit));
        assert!(submit.claim());
        assert!(!submit.clone().claim());
        adapter.bridge.finish_command(&submit);
        assert!(adapter.route_key(key(0x1b, false), epoch));
        let NativeDelivery::Command(cancel) = receiver.try_recv().expect("cancel") else {
            panic!("cancel command");
        };
        assert_eq!(cancel.command, NativeEditorCommand::Cancel);
        route.policy = NativeEditorPolicy::SingleLine(gpui_kit::EntityId::from(2_u64));
        adapter.bridge.route(route);
        assert!(!adapter.bridge.validates(&cancel));
        adapter.bridge.finish_command(&cancel);
        adapter.bridge.route(NativeEditorRoute::default());
        assert!(!adapter.route_key(key(0x0d, false), epoch));
        assert!(!adapter.route_key(key(0x1b, false), epoch));
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn single_line_without_image_paste_delivers_only_current_acknowledged_commands() {
        use super::super::editing_core::{NativeEditorPolicy, NativeEditorRoute};
        let bridge = ComposerNativeBridge::new(edit("draft", (5, 5), 0), (0, 0), EditorGate::new());
        let (sender, mut receiver) = mpsc::channel(64);
        let adapter = NativeAdapter::new(bridge, DraftRecovery::new("draft".into()), sender, false)
            .expect("single-line adapter without image paste");
        adapter.receive(edit("draft", (5, 5), 1));
        let initial = receive_edit(&mut receiver);
        assert!(
            adapter
                .bridge
                .commit(adapter.owner(), &initial, |_, _| Some((0, 1)))
        );
        let epoch = adapter.bridge.focus(adapter.owner(), true).expect("focus");
        for event in [key(0x0d, false), key(0x0d, true), key(0x1b, false)] {
            assert!(!adapter.route_key(event, epoch));
            assert!(receiver.try_recv().is_err());
        }
        let route = NativeEditorRoute {
            policy: NativeEditorPolicy::SingleLine(gpui_kit::EntityId::from(1_u64)),
            ..Default::default()
        };
        adapter.bridge.route(route);
        adapter.receive(mutated(edit("draft中", (6, 6), 2), (5, 5), "中"));
        let latest = receive_edit(&mut receiver);
        for vk in [0x0d, 0x1b] {
            assert!(adapter.route_key(key(vk, false), epoch));
            assert!(receiver.try_recv().is_err());
        }
        assert!(
            adapter
                .bridge
                .commit(adapter.owner(), &latest, |_, _| Some((1, 2)))
        );
        assert!(receiver.try_recv().is_err());
        assert!(adapter.route_key(key(0x0d, false), epoch));
        let NativeDelivery::Command(submit) = receiver.try_recv().expect("acknowledged submit")
        else {
            panic!("submit command");
        };
        assert_eq!(submit.command, NativeEditorCommand::Submit);
        assert_eq!(submit.edit.text.as_ref(), "draft中");
        assert!(adapter.bridge.validates(&submit));
        assert!(submit.claim());
        assert!(!submit.clone().claim());
        adapter.bridge.finish_command(&submit);
        assert!(adapter.bridge.validates(&submit));
        assert!(adapter.route_key(key(0x1b, false), epoch));
        let NativeDelivery::Command(cancel) = receiver.try_recv().expect("acknowledged cancel")
        else {
            panic!("cancel command");
        };
        assert_eq!(cancel.command, NativeEditorCommand::Cancel);
        assert!(adapter.bridge.validates(&cancel));
        adapter.bridge.route(NativeEditorRoute {
            policy: NativeEditorPolicy::SingleLine(gpui_kit::EntityId::from(2_u64)),
            ..route
        });
        assert!(!adapter.bridge.validates(&cancel));
        adapter.bridge.finish_command(&cancel);
        let mut composing = edit("draft中候", (7, 7), 3);
        composing.composing = true;
        adapter.receive(mutated(composing, (6, 6), "候"));
        let marked = receive_edit(&mut receiver);
        for vk in [0x0d, 0x1b] {
            assert!(!adapter.route_key(key(vk, false), epoch));
            assert!(receiver.try_recv().is_err());
        }
        assert!(
            adapter
                .bridge
                .commit(adapter.owner(), &marked, |_, _| Some((2, 3)))
        );
        for vk in [0x0d, 0x1b] {
            assert!(!adapter.route_key(key(vk, false), epoch));
            assert!(receiver.try_recv().is_err());
        }
        adapter.receive(edit("draft中候", (7, 7), 4));
        let unmarked = receive_edit(&mut receiver);
        assert!(
            adapter
                .bridge
                .commit(adapter.owner(), &unmarked, |_, _| Some((2, 4)))
        );
        adapter.bridge.route(NativeEditorRoute::default());
        for event in [key(0x0d, false), key(0x0d, true), key(0x1b, false)] {
            assert!(!adapter.route_key(event, epoch));
            assert!(receiver.try_recv().is_err());
        }
    }

    #[test]
    fn single_line_composition_and_repeat_never_deliver_a_form_command() {
        use super::super::editing_core::{NativeEditorPolicy, NativeEditorRoute};
        let (adapter, mut receiver, epoch) = settled_adapter();
        adapter.bridge.route(NativeEditorRoute {
            policy: NativeEditorPolicy::SingleLine(gpui_kit::EntityId::from(1_u64)),
            ..Default::default()
        });
        for vk in [0x0d, 0x1b] {
            let mut event = key(vk, false);
            event.repeat = true;
            assert!(adapter.route_key(event, epoch));
            assert!(receiver.try_recv().is_err());
        }
        let mut composing = edit("draft中", (6, 6), 2);
        composing.composing = true;
        adapter.receive(mutated(composing, (5, 5), "中"));
        let composition = receive_edit(&mut receiver);
        for vk in [0x0d, 0x1b] {
            assert!(!adapter.route_key(key(vk, false), epoch));
            assert!(receiver.try_recv().is_err());
        }
        assert!(
            adapter
                .bridge
                .commit(adapter.owner(), &composition, |_, _| Some((1, 2)))
        );
        for vk in [0x0d, 0x1b] {
            assert!(!adapter.route_key(key(vk, false), epoch));
            assert!(receiver.try_recv().is_err());
        }
        adapter.receive(edit("draft中", (6, 6), 3));
        let end = receive_edit(&mut receiver);
        assert!(adapter.route_key(key(0x0d, false), epoch));
        assert!(receiver.try_recv().is_err());
        assert!(
            adapter
                .bridge
                .commit(adapter.owner(), &end, |_, _| Some((1, 3)))
        );
        assert!(receiver.try_recv().is_err());
        assert!(adapter.route_key(key(0x1b, false), epoch));
        let NativeDelivery::Command(cancel) = receiver.try_recv().expect("fresh cancel") else {
            panic!("cancel command");
        };
        assert_eq!(cancel.command, NativeEditorCommand::Cancel);
    }

    #[test]
    fn sealed_authority_replacement_refuses_pending_text_and_retires_old_commands() {
        let (adapter, mut receiver) = adapter(64);
        adapter.receive(edit("draft", (5, 5), 1));
        assert!(!adapter.bridge.seal_authority((1, 0), "", (0, 0)));
        let initial = receive_edit(&mut receiver);
        assert!(
            adapter
                .bridge
                .commit(adapter.owner(), &initial, |_, _| Some((0, 1)))
        );
        assert!(adapter.bridge.seal_authority((1, 1), "", (0, 0)));
        assert!(!adapter.bridge.ready("draft", (5, 5)));
        let mut writes = 0;
        assert!(!adapter.bridge.commit(adapter.owner(), &initial, |_, _| {
            writes += 1;
            Some((1, 2))
        }));
        assert_eq!(writes, 0);
    }

    fn settled_adapter() -> (Rc<NativeAdapter>, mpsc::Receiver<NativeDelivery>, u64) {
        let (adapter, mut receiver) = adapter(64);
        adapter.receive(edit("draft", (5, 5), 1));
        let initial = receive_edit(&mut receiver);
        assert!(
            adapter
                .bridge
                .commit(adapter.owner(), &initial, |_, _| Some((0, 1)))
        );
        let epoch = adapter.bridge.focus(adapter.owner(), true).expect("focus");
        (adapter, receiver, epoch)
    }

    #[test]
    fn missing_mutation_preserves_local_recovery_without_delivering_an_edit() {
        let (adapter, mut receiver, _) = settled_adapter();
        adapter.receive(edit("draftx", (6, 6), 2));
        assert!(adapter.is_failed());
        assert_eq!(adapter.draft.saved().as_deref(), Some("draftx"));
        assert!(!adapter.bridge.ready("draftx", (6, 6)));
        assert!(matches!(receiver.try_recv(), Ok(NativeDelivery::Failed)));
        assert!(receiver.try_recv().is_err());
        adapter.receive(mutated(edit("draftxy", (7, 7), 3), (6, 6), "y"));
        assert_eq!(adapter.draft.saved().as_deref(), Some("draftx"));
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn fresh_paste_snapshot_after_mutation_keeps_barrier_and_exact_provenance() {
        let (adapter, mut receiver, epoch) = settled_adapter();
        adapter.receive(mutated(edit("draftx", (6, 6), 2), (5, 5), "x"));
        let first = receive_edit(&mut receiver);
        assert_eq!(
            first.mutation.as_ref().expect("mutation").range_utf16,
            (5, 5)
        );
        let before = edit("draftx", (6, 6), 2);
        assert!(adapter.begin_paste(before.clone()));
        assert!(
            adapter
                .bridge
                .commit(adapter.owner(), &first, |_, _| Some((1, 2)))
        );
        assert!(!adapter.bridge.ready("draftx", (6, 6)));
        let payload = Rc::new(InputPastePayload::Synthetic(nsh_core::PasteEvent::new("y")));
        assert!(
            matches!(adapter.prepare_paste(&before, &payload), PasteDecision::Insert(text) if text == "y")
        );
        let completion = mutated(edit("draftxy", (7, 7), 3), (6, 6), "y");
        adapter.complete_paste(completion.clone(), payload);
        assert!(adapter.route_key(key(0x0d, true), epoch));
        let NativeDelivery::Paste {
            edit,
            payload,
            _budget,
        } = receiver.try_recv().expect("paste")
        else {
            panic!("paste completion");
        };
        assert_eq!(edit, completion);
        assert_eq!(payload.text(), "y");
        assert!(receiver.try_recv().is_err());
        assert!(
            adapter
                .bridge
                .commit(adapter.owner(), &edit, |previous, revisions| {
                    assert_eq!(previous, &first);
                    assert_eq!(revisions, (1, 2));
                    Some((2, 3))
                })
        );
        assert!(adapter.bridge.ready("draftxy", (7, 7)));
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn paste_begin_blocks_earlier_receipt_and_submit_until_completion_receipt() {
        let (adapter, mut receiver) = adapter(64);
        adapter.receive(edit("draft", (5, 5), 1));
        let initial = receive_edit(&mut receiver);
        let epoch = adapter.bridge.focus(adapter.owner(), true).expect("focus");
        assert!(adapter.begin_paste(initial.clone()));
        assert!(
            adapter
                .bridge
                .commit(adapter.owner(), &initial, |_, _| Some((0, 1)))
        );
        assert!(!adapter.bridge.ready("draft", (5, 5)));
        assert!(!adapter.bridge.seal_authority((0, 1), "draft", (5, 5)));
        assert!(adapter.route_key(key(0x0d, true), epoch));
        assert!(receiver.try_recv().is_err());
        let mut payload = nsh_core::PasteEvent::new("\r\n中文");
        payload.html = "<p>original</p>".into();
        let payload = Rc::new(InputPastePayload::Synthetic(payload));
        assert!(
            matches!(adapter.prepare_paste(&initial, &payload), PasteDecision::Insert(text) if text == "\n中文")
        );
        adapter.complete_paste(
            mutated(edit("draft\n中文", (8, 8), 2), (5, 5), "\n中文"),
            payload,
        );
        assert!(!adapter.bridge.ready("draft\n中文", (8, 8)));
        let NativeDelivery::Paste {
            edit,
            payload,
            _budget,
        } = receiver.try_recv().expect("paste")
        else {
            panic!("one paste transaction");
        };
        assert_eq!(payload.text(), "\r\n中文");
        assert_eq!(payload.html(), "<p>original</p>");
        assert!(
            adapter
                .bridge
                .commit(adapter.owner(), &edit, |before, revisions| {
                    assert_eq!(before.text.as_ref(), "draft");
                    assert_eq!(revisions, (0, 1));
                    Some((1, 2))
                })
        );
        assert!(adapter.bridge.ready("draft\n中文", (8, 8)));
        assert!(receiver.try_recv().is_err());
        drop(_budget);
    }

    #[test]
    fn cancelled_paste_freezes_without_replay_or_late_draft_replacement() {
        let (adapter, mut receiver, epoch) = settled_adapter();
        let before = adapter.bridge.local_draft();
        assert!(adapter.begin_paste(before.clone()));
        let payload = Rc::new(InputPastePayload::Synthetic(nsh_core::PasteEvent::new("x")));
        assert!(matches!(
            adapter.prepare_paste(&before, &payload),
            PasteDecision::Insert(_)
        ));
        adapter.paste_complete(None);
        assert!(adapter.is_failed());
        assert_eq!(adapter.draft.saved().as_deref(), Some("draft"));
        assert!(!adapter.bridge.ready("draft", (5, 5)));
        adapter.complete_paste(edit("draftx", (6, 6), 2), payload);
        assert_eq!(adapter.draft.saved().as_deref(), Some("draft"));
        assert!(adapter.route_key(key(0x0d, true), epoch));
        assert!(matches!(receiver.try_recv(), Ok(NativeDelivery::Failed)));
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn rejected_original_payload_keeps_barrier_closed_without_insertion() {
        let (adapter, mut receiver, _) = settled_adapter();
        let before = adapter.bridge.local_draft();
        assert!(adapter.begin_paste(before.clone()));
        let mut payload = nsh_core::PasteEvent::new("x");
        payload.uris.push("file:///tmp/%00.png".into());
        let payload = Rc::new(InputPastePayload::Synthetic(payload));
        assert!(matches!(
            adapter.prepare_paste(&before, &payload),
            PasteDecision::Reject
        ));
        assert!(adapter.is_failed());
        assert_eq!(adapter.draft.saved().as_deref(), Some("draft"));
        assert!(!adapter.bridge.ready("draft", (5, 5)));
        assert!(matches!(receiver.try_recv(), Ok(NativeDelivery::Failed)));
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn attachment_only_completion_is_one_serial_and_preserves_original_metadata() {
        let (adapter, mut receiver, _) = settled_adapter();
        let before = adapter.bridge.local_draft();
        assert!(adapter.begin_paste(before.clone()));
        let mut payload = nsh_core::PasteEvent::new("file label not inserted");
        payload.html = "<img>".into();
        payload.uris.push("file:///tmp/original%20name.png".into());
        let payload = Rc::new(InputPastePayload::Synthetic(payload));
        assert!(
            matches!(adapter.prepare_paste(&before, &payload), PasteDecision::Insert(text) if text.is_empty())
        );
        adapter.complete_paste(edit("draft", (5, 5), 2), payload);
        let NativeDelivery::Paste {
            edit,
            payload,
            _budget,
        } = receiver.try_recv().expect("attachment")
        else {
            panic!("attachment transaction");
        };
        assert_eq!(edit.serial, 2);
        assert_eq!(payload.uris(), ["file:///tmp/original%20name.png"]);
        assert_eq!(payload.text(), "file label not inserted");
        assert_eq!(payload.html(), "<img>");
        assert!(
            adapter
                .bridge
                .commit(adapter.owner(), &edit, |_, _| Some((0, 2)))
        );
        assert!(adapter.bridge.ready("draft", (5, 5)));
        assert!(receiver.try_recv().is_err());
        drop(_budget);
    }

    #[test]
    fn inflight_budget_is_shared_and_released_when_queued_completion_is_discarded() {
        let (first, receiver, _) = settled_adapter();
        let before = first.bridge.local_draft();
        assert!(first.begin_paste(before.clone()));
        let payload = Rc::new(InputPastePayload::Synthetic(nsh_core::PasteEvent::new(
            "x".repeat(600 * 1024),
        )));
        assert!(matches!(
            first.prepare_paste(&before, &payload),
            PasteDecision::Insert(_)
        ));
        let mut text = "draft".to_owned();
        text.push_str(payload.text());
        first.complete_paste(
            mutated(edit(&text, (614405, 614405), 2), (5, 5), payload.text()),
            payload,
        );
        let (second, _, _) = settled_adapter();
        let second_before = second.bridge.local_draft();
        assert!(second.begin_paste(second_before.clone()));
        assert!(matches!(
            second.prepare_paste(
                &second_before,
                &Rc::new(InputPastePayload::Synthetic(nsh_core::PasteEvent::new(
                    "y".repeat(600 * 1024)
                )))
            ),
            PasteDecision::Reject
        ));
        assert!(second.is_failed());
        drop(receiver);
        let (third, _, _) = settled_adapter();
        let third_before = third.bridge.local_draft();
        assert!(third.begin_paste(third_before.clone()));
        assert!(matches!(
            third.prepare_paste(
                &third_before,
                &Rc::new(InputPastePayload::Synthetic(nsh_core::PasteEvent::new(
                    "z".repeat(600 * 1024)
                )))
            ),
            PasteDecision::Insert(_)
        ));
        third.paste_complete(None);
    }

    #[test]
    fn inflight_image_bytes_reject_before_retaining_another_payload() {
        let before = edit("draft", (5, 5), 1);
        let mut payload = nsh_core::PasteEvent::default();
        payload.files.push(nsh_core::PastedFile::new(
            "a.png",
            "image/png",
            vec![0; 8 * 1024 * 1024],
        ));
        let payload = InputPastePayload::Synthetic(payload);
        let first = PasteBudget::reserve(&payload, &before).expect("first image budget");
        let second = PasteBudget::reserve(&payload, &before).expect("second image budget");
        assert!(PasteBudget::reserve(&payload, &before).is_none());
        drop(first);
        assert!(PasteBudget::reserve(&payload, &before).is_some());
        drop(second);
    }

    fn history_key(serial: u64, first: bool, top: bool, collapsed: bool) -> InputKey {
        let mut event = key(0x26, false);
        event.context = Some(nsh_platform_gpui::editing::InputKeyContext {
            native_edit_serial: serial,
            first_visual_row: first,
            viewport_top: top,
            selection_collapsed: collapsed,
        });
        event
    }

    #[test]
    fn history_uses_same_dispatch_visual_geometry_serial_and_selection() {
        let (adapter, mut receiver, epoch) = settled_adapter();
        adapter
            .bridge
            .route(super::super::editing_core::NativeEditorRoute {
                history: true,
                ..Default::default()
            });
        for event in [
            key(0x26, false),
            history_key(0, true, true, true),
            history_key(1, false, true, true),
            history_key(1, true, false, true),
            history_key(1, true, true, false),
        ] {
            assert!(!adapter.route_key(event, epoch));
            assert!(receiver.try_recv().is_err());
        }
        let mut shifted = history_key(1, true, true, true);
        shifted.shift = true;
        assert!(!adapter.route_key(shifted, epoch));
        assert!(adapter.route_key(history_key(1, true, true, true), epoch));
        let NativeDelivery::Command(token) = receiver.try_recv().expect("history") else {
            panic!("history command");
        };
        assert_eq!(token.command, NativeEditorCommand::History);
        assert_eq!(token.edit.text.as_ref(), "draft");
        assert!(adapter.bridge.validates(&token));
    }

    #[test]
    fn recalled_history_continues_but_popup_and_availability_keep_priority() {
        let (adapter, mut receiver, epoch) = settled_adapter();
        let mut route = super::super::editing_core::NativeEditorRoute {
            history: true,
            recalling_history: true,
            ..Default::default()
        };
        adapter.bridge.route(route);
        assert!(adapter.route_key(history_key(1, false, false, true), epoch));
        let NativeDelivery::Command(token) = receiver.try_recv().expect("recall") else {
            panic!("recall");
        };
        assert_eq!(token.command, NativeEditorCommand::History);
        adapter.bridge.finish_command(&token);
        route.popup = true;
        adapter.bridge.route(route);
        assert!(adapter.route_key(history_key(1, true, true, true), epoch));
        let NativeDelivery::Command(token) = receiver.try_recv().expect("popup") else {
            panic!("popup");
        };
        assert_eq!(token.command, NativeEditorCommand::Previous);
        adapter.bridge.finish_command(&token);
        route.popup = false;
        route.history = false;
        adapter.bridge.route(route);
        assert!(!adapter.route_key(history_key(1, true, true, true), epoch));
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn paste_receipt_never_overwrites_or_settles_later_native_typing() {
        let (adapter, mut receiver, _) = settled_adapter();
        let before = adapter.bridge.local_draft();
        assert!(adapter.begin_paste(before.clone()));
        let payload = Rc::new(InputPastePayload::Synthetic(nsh_core::PasteEvent::new("x")));
        assert!(matches!(
            adapter.prepare_paste(&before, &payload),
            PasteDecision::Insert(_)
        ));
        adapter.complete_paste(mutated(edit("draftx", (6, 6), 2), (5, 5), "x"), payload);
        adapter.receive(mutated(edit("draftxy", (7, 7), 3), (6, 6), "y"));
        let NativeDelivery::Paste { edit, _budget, .. } = receiver.try_recv().expect("paste")
        else {
            panic!("paste");
        };
        assert!(
            adapter
                .bridge
                .commit(adapter.owner(), &edit, |_, _| Some((1, 2)))
        );
        assert_eq!(adapter.bridge.local_draft().text.as_ref(), "draftxy");
        assert!(!adapter.bridge.ready("draftx", (6, 6)));
        assert!(!adapter.bridge.ready("draftxy", (7, 7)));
        let next = receive_edit(&mut receiver);
        assert!(adapter.bridge.commit(adapter.owner(), &next, |before, _| {
            assert_eq!(before.text.as_ref(), "draftx");
            Some((2, 3))
        }));
        assert!(adapter.bridge.ready("draftxy", (7, 7)));
        drop(_budget);
    }

    #[test]
    fn unexpected_paste_serial_freezes_and_retains_actual_local_completion() {
        let (adapter, mut receiver, _) = settled_adapter();
        let before = adapter.bridge.local_draft();
        assert!(adapter.begin_paste(before.clone()));
        let payload = Rc::new(InputPastePayload::Synthetic(nsh_core::PasteEvent::new("x")));
        assert!(matches!(
            adapter.prepare_paste(&before, &payload),
            PasteDecision::Insert(_)
        ));
        adapter.complete_paste(mutated(edit("draftx", (6, 6), 4), (5, 5), "x"), payload);
        assert!(adapter.is_failed());
        assert_eq!(adapter.draft.saved().as_deref(), Some("draftx"));
        assert!(!adapter.bridge.ready("draftx", (6, 6)));
        assert!(matches!(receiver.try_recv(), Ok(NativeDelivery::Failed)));
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn ordinary_text_input_pastes_text_without_attachment_authority() {
        let bridge = ComposerNativeBridge::new(edit("draft", (5, 5), 0), (0, 0), EditorGate::new());
        let (sender, mut receiver) = mpsc::channel(64);
        let adapter = NativeAdapter::new(bridge, DraftRecovery::new("draft".into()), sender, false)
            .expect("text input");
        adapter.receive(edit("draft", (5, 5), 1));
        let before = receive_edit(&mut receiver);
        assert!(
            adapter
                .bridge
                .commit(adapter.owner(), &before, |_, _| Some((0, 1)))
        );
        adapter.bridge.focus(adapter.owner(), true).expect("focus");
        assert!(adapter.begin_paste(before.clone()));
        let payload = Rc::new(InputPastePayload::Synthetic(nsh_core::PasteEvent::new(
            "text",
        )));
        assert!(
            matches!(adapter.prepare_paste(&before, &payload), PasteDecision::Insert(text) if text == "text")
        );
        adapter.complete_paste(
            mutated(edit("drafttext", (9, 9), 2), (5, 5), "text"),
            payload,
        );
        assert!(matches!(
            receiver.try_recv(),
            Ok(NativeDelivery::Paste { .. })
        ));
        assert!(!adapter.is_failed());
    }

    #[test]
    fn history_with_newer_native_edit_cannot_authorize_delayed_business_intent() {
        let (adapter, mut receiver, epoch) = settled_adapter();
        adapter
            .bridge
            .route(super::super::editing_core::NativeEditorRoute {
                history: true,
                ..Default::default()
            });
        adapter.receive(mutated(edit("draftx", (6, 6), 2), (5, 5), "x"));
        assert!(adapter.route_key(history_key(2, true, true, true), epoch));
        let pending = receive_edit(&mut receiver);
        assert!(receiver.try_recv().is_err());
        assert!(
            adapter
                .bridge
                .commit(adapter.owner(), &pending, |_, _| Some((1, 2)))
        );
        assert!(receiver.try_recv().is_err());
        assert!(!adapter.route_key(history_key(1, true, true, true), epoch));
        assert!(adapter.route_key(history_key(2, true, true, true), epoch));
        let NativeDelivery::Command(token) = receiver.try_recv().expect("fresh history") else {
            panic!("history");
        };
        assert_eq!(token.command, NativeEditorCommand::History);
        assert_eq!(token.edit.text.as_ref(), "draftx");
    }

    #[test]
    fn original_metadata_budget_rejects_before_retention_and_releases_on_cancel() {
        let (adapter, mut receiver, _) = settled_adapter();
        let before = adapter.bridge.local_draft();
        assert!(adapter.begin_paste(before.clone()));
        let payload = Rc::new(InputPastePayload::native(Some(
            gpui_kit::ClipboardItem::new_string_with_metadata(
                "x".into(),
                "m".repeat(2 * 1024 * 1024),
            ),
        )));
        let observed = Rc::downgrade(&payload);
        assert!(matches!(
            adapter.prepare_paste(&before, &payload),
            PasteDecision::Reject
        ));
        assert_eq!(Rc::strong_count(&payload), 1);
        assert!(adapter.pending_paste.borrow().is_none());
        drop(payload);
        assert!(observed.upgrade().is_none());
        assert!(matches!(receiver.try_recv(), Ok(NativeDelivery::Failed)));
        assert!(receiver.try_recv().is_err());
        PASTE_USAGE.with(|usage| assert_eq!(usage.borrow().count, 0));

        let (adapter, _, _) = settled_adapter();
        let before = adapter.bridge.local_draft();
        assert!(adapter.begin_paste(before.clone()));
        let payload = Rc::new(InputPastePayload::native(Some(
            gpui_kit::ClipboardItem::new_string_with_metadata("x".into(), "not-json".into()),
        )));
        assert!(matches!(
            adapter.prepare_paste(&before, &payload),
            PasteDecision::Insert(_)
        ));
        assert_eq!(Rc::strong_count(&payload), 1);
        assert_eq!(Rc::weak_count(&payload), 1);
        adapter.paste_complete(None);
        PASTE_USAGE.with(|usage| assert_eq!(usage.borrow().count, 0));
    }

    fn native_images(count: usize, size: usize, spare: usize) -> InputPastePayload {
        let mut entries = vec![gpui_kit::ClipboardEntry::String(
            gpui_kit::ClipboardString {
                text: String::new(),
                metadata: Some("opaque".into()),
            },
        )];
        for _ in 0..count {
            let mut bytes = Vec::with_capacity(size + spare);
            bytes.resize(size, 0);
            if size >= 8 {
                bytes[..8].copy_from_slice(&[137, 80, 78, 71, 13, 10, 26, 10]);
            }
            entries.push(gpui_kit::ClipboardEntry::Image(
                gpui_kit::Image::from_bytes(gpui_kit::ImageFormat::Png, bytes),
            ));
        }
        InputPastePayload::native(Some(gpui_kit::ClipboardItem { entries }))
    }

    #[test]
    fn original_storage_counts_capacities_paths_and_single_image_buffer_once() {
        let before = edit("draft", (5, 5), 1);
        let mut payload = native_images(1, 8 * 1024 * 1024, 4096);
        let item = payload.take_original().expect("item");
        let mut entries = item.entries;
        entries.push(gpui_kit::ClipboardEntry::ExternalPaths(
            gpui_kit::ExternalPaths(
                [std::path::PathBuf::from("/tmp/original.png")]
                    .into_iter()
                    .collect(),
            ),
        ));
        let payload = InputPastePayload::native(Some(gpui_kit::ClipboardItem { entries }));
        let (text, images) = original_storage(payload.original()).expect("original storage");
        assert_eq!(images, 8 * 1024 * 1024);
        assert!(text >= "opaque".len() + "/tmp/original.png".len());
        let first = PasteBudget::reserve(&payload, &before).expect("one original buffer");
        assert_eq!(first.images, 8 * 1024 * 1024);
        assert_eq!(
            first.text,
            payload.normalized_storage().expect("normalized storage")
                + text
                + before.text.len() * 2
                + payload.text().len() * 2
        );
        let second = PasteBudget::reserve(&payload, &before).expect("second original buffer");
        assert!(PasteBudget::reserve(&payload, &before).is_none());
        drop(first);
        assert!(PasteBudget::reserve(&payload, &before).is_some());
        drop(second);
        let payload =
            InputPastePayload::native(Some(gpui_kit::ClipboardItem::new_string_with_metadata(
                "x".into(),
                String::with_capacity(2 * 1024 * 1024),
            )));
        assert!(PasteBudget::reserve(&payload, &before).is_none());
    }

    #[test]
    fn native_full_image_budget_and_metadata_are_independent_and_release_inflight() {
        let before = edit("draft", (5, 5), 1);
        for count in [2, 4] {
            let payload = native_images(count, 16 * 1024 * 1024 / count, 4096);
            assert!(
                payload
                    .files()
                    .all(|file| file.bytes().len() == file.capacity())
            );
            let first = PasteBudget::reserve(&payload, &before).expect("full old business budget");
            assert_eq!(first.images, 16 * 1024 * 1024);
            assert!(first.text < 2 * 1024 * 1024);
            assert!(PasteBudget::reserve(&payload, &before).is_none());
            drop(first);
            assert!(PasteBudget::reserve(&payload, &before).is_some());
        }
        for (count, size) in [(5, 8), (1, 8 * 1024 * 1024 + 1), (3, 6 * 1024 * 1024)] {
            let payload = native_images(count, size, 0);
            assert!(PasteBudget::reserve(&payload, &before).is_none());
            let (adapter, mut receiver, _) = settled_adapter();
            let before = adapter.bridge.local_draft();
            assert!(adapter.begin_paste(before.clone()));
            assert!(matches!(
                adapter.prepare_paste(&before, &Rc::new(payload)),
                PasteDecision::Reject
            ));
            assert!(adapter.is_failed());
            assert!(matches!(receiver.try_recv(), Ok(NativeDelivery::Failed)));
            assert!(receiver.try_recv().is_err());
        }
        let payload = native_images(1, 8, 0);
        let InputPastePayload::Native(_) = payload else {
            panic!("native")
        };
        let mut item = payload.clone().take_original().expect("item");
        let gpui_kit::ClipboardEntry::Image(image) = &mut item.entries[1] else {
            panic!("image")
        };
        image.bytes[0] = 0;
        let (adapter, _, _) = settled_adapter();
        let before = adapter.bridge.local_draft();
        assert!(adapter.begin_paste(before.clone()));
        assert!(matches!(
            adapter.prepare_paste(&before, &Rc::new(InputPastePayload::native(Some(item)))),
            PasteDecision::Reject
        ));
    }

    #[test]
    fn completion_rejects_replaced_payload_original_and_changed_metadata() {
        for replacement in 0..7 {
            let (adapter, mut receiver, _) = settled_adapter();
            let before = adapter.bridge.local_draft();
            assert!(adapter.begin_paste(before.clone()));
            let mut payload = Rc::new(InputPastePayload::native(Some(
                gpui_kit::ClipboardItem::new_string_with_metadata("x".into(), "opaque".into()),
            )));
            assert!(matches!(
                adapter.prepare_paste(&before, &payload),
                PasteDecision::Insert(_)
            ));
            assert!(Rc::get_mut(&mut payload).is_none());
            match replacement {
                0 => payload = Rc::new((*payload).clone()),
                1 | 2 => {
                    let mut item = payload.original().expect("original").clone();
                    if replacement == 2 {
                        let gpui_kit::ClipboardEntry::String(value) = &mut item.entries[0] else {
                            panic!("string")
                        };
                        value.metadata = Some("replacement".into());
                    }
                    payload = Rc::new(InputPastePayload::native(Some(item)));
                }
                3 => payload = Rc::new(InputPastePayload::native(None)),
                4 => {
                    let changed = Rc::make_mut(&mut payload);
                    *changed = InputPastePayload::Synthetic(nsh_core::PasteEvent::new("x"));
                    if let InputPastePayload::Synthetic(paste) = changed {
                        paste.html = "replacement".into();
                    }
                }
                5 => {
                    *Rc::make_mut(&mut payload) =
                        InputPastePayload::Synthetic(nsh_core::PasteEvent::new("x"))
                }
                6 => {
                    let alias = payload.clone();
                    adapter
                        .complete_paste(mutated(edit("draftx", (6, 6), 2), (5, 5), "x"), payload);
                    drop(alias);
                    assert!(adapter.is_failed());
                    assert!(matches!(receiver.try_recv(), Ok(NativeDelivery::Failed)));
                    assert!(receiver.try_recv().is_err());
                    PASTE_USAGE.with(|usage| assert_eq!(usage.borrow().count, 0));
                    continue;
                }
                _ => unreachable!(),
            }
            adapter.complete_paste(mutated(edit("draftx", (6, 6), 2), (5, 5), "x"), payload);
            assert!(adapter.is_failed());
            assert!(matches!(receiver.try_recv(), Ok(NativeDelivery::Failed)));
            assert!(receiver.try_recv().is_err());
            PASTE_USAGE.with(|usage| assert_eq!(usage.borrow().count, 0));
        }
    }

    #[test]
    fn original_metadata_inflight_budget_is_shared_and_released() {
        let before = edit("draft", (5, 5), 1);
        let payload =
            InputPastePayload::native(Some(gpui_kit::ClipboardItem::new_string_with_metadata(
                "x".into(),
                "opaque".repeat(200 * 1024),
            )));
        let first = PasteBudget::reserve(&payload, &before).expect("first metadata budget");
        assert!(PasteBudget::reserve(&payload, &before).is_none());
        drop(first);
        assert!(PasteBudget::reserve(&payload, &before).is_some());
    }

    #[test]
    fn queued_original_is_same_carrier_and_discard_releases_storage() {
        let (adapter, receiver, _) = settled_adapter();
        let before = adapter.bridge.local_draft();
        assert!(adapter.begin_paste(before.clone()));
        let payload = Rc::new(InputPastePayload::native(Some(
            gpui_kit::ClipboardItem::new_string_with_metadata("x".into(), "opaque".into()),
        )));
        let observed = Rc::downgrade(&payload);
        assert!(matches!(
            adapter.prepare_paste(&before, &payload),
            PasteDecision::Insert(_)
        ));
        adapter.complete_paste(mutated(edit("draftx", (6, 6), 2), (5, 5), "x"), payload);
        assert_eq!(observed.strong_count(), 1);
        PASTE_USAGE.with(|usage| assert_eq!(usage.borrow().count, 1));
        drop(receiver);
        assert!(observed.upgrade().is_none());
        PASTE_USAGE.with(|usage| assert_eq!(usage.borrow().count, 0));
    }
    #[test]
    fn native_full_image_reservation_releases_on_cancel_and_queue_discard() {
        for cancel in [true, false] {
            let (adapter, receiver, _) = settled_adapter();
            let before = adapter.bridge.local_draft();
            assert!(adapter.begin_paste(before.clone()));
            let payload = Rc::new(native_images(2, 8 * 1024 * 1024, 4096));
            let observed = Rc::downgrade(&payload);
            assert!(
                matches!(adapter.prepare_paste(&before,&payload),PasteDecision::Insert(text) if text.is_empty())
            );
            PASTE_USAGE.with(|usage| assert_eq!(usage.borrow().images, 16 * 1024 * 1024));
            if cancel {
                adapter.paste_complete(None);
                drop(payload);
            } else {
                adapter.complete_paste(edit("draft", (5, 5), 2), payload);
                assert!(!adapter.is_failed());
                drop(receiver);
            }
            assert!(observed.upgrade().is_none());
            PASTE_USAGE.with(|usage| {
                assert_eq!(usage.borrow().count, 0);
                assert_eq!(usage.borrow().images, 0);
                assert_eq!(usage.borrow().text, 0);
            });
            assert!(PasteBudget::reserve(&native_images(2, 8 * 1024 * 1024, 0), &before).is_some());
        }
    }

    #[test]
    fn native_carrier_cow_image_replacement_is_not_authorized_by_equal_text() {
        let (adapter, mut receiver, _) = settled_adapter();
        let before = adapter.bridge.local_draft();
        assert!(adapter.begin_paste(before.clone()));
        let mut payload = Rc::new(native_images(1, 16, 0));
        assert!(matches!(
            adapter.prepare_paste(&before, &payload),
            PasteDecision::Insert(_)
        ));
        assert!(Rc::get_mut(&mut payload).is_none());
        let changed = Rc::make_mut(&mut payload);
        let mut original = changed.take_original().expect("original");
        let gpui_kit::ClipboardEntry::Image(image) = &mut original.entries[1] else {
            panic!("image")
        };
        image.bytes[15] = 1;
        *changed = InputPastePayload::native(Some(original));
        adapter.complete_paste(edit("draft", (5, 5), 2), payload);
        assert!(adapter.is_failed());
        assert!(matches!(receiver.try_recv(), Ok(NativeDelivery::Failed)));
        assert!(receiver.try_recv().is_err());
        PASTE_USAGE.with(|usage| assert_eq!(usage.borrow().count, 0));
    }

    #[test]
    fn paste_policy_preserves_original_carrier_and_directed_utf16_replacement() {
        for (policy, original, inserted) in [
            (NativeEditorPolicy::Composer, "x\ny", "x\ny"),
            (NativeEditorPolicy::Composer, "x\r\ny", "x\ny"),
            (NativeEditorPolicy::Composer, "x\ry", "x\ry"),
            (
                NativeEditorPolicy::SingleLine(gpui_kit::EntityId::from(1_u64)),
                "x\ny",
                "x y",
            ),
            (
                NativeEditorPolicy::SingleLine(gpui_kit::EntityId::from(1_u64)),
                "x\r\ny",
                "x\r y",
            ),
            (
                NativeEditorPolicy::SingleLine(gpui_kit::EntityId::from(1_u64)),
                "x\ry",
                "x\ry",
            ),
        ] {
            let bridge =
                ComposerNativeBridge::new(edit("中😀文", (3, 1), 0), (0, 0), EditorGate::new());
            bridge.route(super::super::editing_core::NativeEditorRoute {
                policy,
                ..Default::default()
            });
            let (sender, mut receiver) = mpsc::channel(64);
            let adapter = NativeAdapter::new(
                bridge.clone(),
                DraftRecovery::new("中😀文".into()),
                sender,
                false,
            )
            .expect("adapter");
            adapter.receive(edit("中😀文", (3, 1), 1));
            let before = receive_edit(&mut receiver);
            assert!(bridge.commit(adapter.owner(), &before, |_, _| Some((0, 1))));
            bridge.focus(adapter.owner(), true).expect("focus");
            assert!(adapter.begin_paste(before.clone()));
            let item = gpui_kit::ClipboardItem::new_string_with_metadata(
                original.into(),
                "opaque-not-html".into(),
            );
            let expected_original = item.clone();
            let payload = Rc::new(InputPastePayload::native(Some(item)));
            let observed = Rc::downgrade(&payload);
            assert!(
                matches!(adapter.prepare_paste(&before, &payload), PasteDecision::Insert(text) if text == inserted)
            );
            assert_eq!(payload.original(), Some(&expected_original));
            let after = format!("中{inserted}文");
            let caret = 1 + inserted.encode_utf16().count() as u32;
            adapter.complete_paste(
                mutated(edit(&after, (caret, caret), 2), (1, 3), inserted),
                payload,
            );
            let NativeDelivery::Paste {
                edit,
                payload,
                _budget,
            } = receiver.try_recv().expect("paste")
            else {
                panic!("paste")
            };
            assert_eq!(payload.original(), Some(&expected_original));
            assert_eq!(bridge.paste_policy(&edit), Some(policy));
            assert!(!bridge.ready(&after, (caret, caret)));
            assert!(bridge.commit(adapter.owner(), &edit, |previous, _| {
                assert_eq!(previous.selection, (3, 1));
                assert_eq!(
                    edit.mutation.as_ref().expect("mutation").inserted.as_ref(),
                    inserted
                );
                Some((1, 2))
            }));
            assert!(bridge.ready(&after, (caret, caret)));
            drop(payload);
            drop(_budget);
            assert!(observed.upgrade().is_none());
            assert!(receiver.try_recv().is_err());
        }
    }

    #[test]
    fn changed_paste_policy_session_or_focus_rejects_each_transaction_stage() {
        for change in 0..4 {
            for stage in 0..3 {
                let (adapter, mut receiver, _) = settled_adapter();
                let route = super::super::editing_core::NativeEditorRoute {
                    policy: NativeEditorPolicy::SingleLine(gpui_kit::EntityId::from(1_u64)),
                    ..Default::default()
                };
                adapter.bridge.route(route);
                let before = adapter.bridge.local_draft();
                assert!(adapter.begin_paste(before.clone()));
                let payload = Rc::new(InputPastePayload::native(Some(
                    gpui_kit::ClipboardItem::new_string("x\ny".into()),
                )));
                if stage > 0 {
                    assert!(
                        matches!(adapter.prepare_paste(&before, &payload), PasteDecision::Insert(text) if text == "x y")
                    );
                }
                let mut payload = Some(payload);
                if stage == 2 {
                    adapter.complete_paste(
                        mutated(edit("draftx y", (8, 8), 2), (5, 5), "x y"),
                        payload.take().expect("original carrier"),
                    );
                    assert!(!adapter.is_failed());
                }
                match change {
                    0 => adapter.bridge.route(Default::default()),
                    1 => adapter
                        .bridge
                        .route(super::super::editing_core::NativeEditorRoute {
                            policy: NativeEditorPolicy::SingleLine(gpui_kit::EntityId::from(2_u64)),
                            ..route
                        }),
                    2 => {
                        adapter
                            .bridge
                            .focus(adapter.owner(), true)
                            .expect("new focus epoch");
                    }
                    _ => {
                        adapter.bridge.route(Default::default());
                        adapter.bridge.route(route);
                    }
                }
                if stage == 2 {
                    let NativeDelivery::Paste {
                        edit,
                        payload,
                        _budget,
                    } = receiver.try_recv().expect("queued paste")
                    else {
                        panic!("paste")
                    };
                    assert!(adapter.bridge.paste_policy(&edit).is_none());
                    assert!(
                        !adapter
                            .bridge
                            .commit(adapter.owner(), &edit, |_, _| panic!("stale paste applied"))
                    );
                    drop(payload);
                    drop(_budget);
                } else {
                    let payload = payload.take().expect("original carrier");
                    if stage == 0 {
                        assert!(matches!(
                            adapter.prepare_paste(&before, &payload),
                            PasteDecision::Reject
                        ));
                    } else {
                        adapter.complete_paste(
                            mutated(edit("draftx y", (8, 8), 2), (5, 5), "x y"),
                            payload,
                        );
                    }
                    assert!(matches!(receiver.try_recv(), Ok(NativeDelivery::Failed)));
                }
                assert!(adapter.is_failed());
                assert!(receiver.try_recv().is_err());
                assert!(!adapter.bridge.ready("draft", (5, 5)));
            }
        }
    }

    #[test]
    fn carrier_reservations_keep_existing_sixty_four_limit_and_release() {
        let before = edit("", (0, 0), 1);
        let payload = InputPastePayload::native(None);
        let mut budgets = Vec::new();
        for _ in 0..64 {
            budgets.push(PasteBudget::reserve(&payload, &before).expect("bounded slot"));
        }
        assert!(PasteBudget::reserve(&payload, &before).is_none());
        budgets.pop();
        assert!(PasteBudget::reserve(&payload, &before).is_some());
        drop(budgets);
        PASTE_USAGE.with(|usage| assert_eq!(usage.borrow().count, 0));
    }
}
