use super::*;
use nsh_core::element::{JsEvent, Root};
use nsh_core::prelude as ns;
use nsh_core::{AccessibilityRole, ApproxTextEngine, Core, MountItem};
use std::cell::RefCell;
use tokio::sync::mpsc;

fn message(id: &str) -> PendingMessage {
    PendingMessage {
        id: id.into(),
        thread_id: "thread".into(),
        generation: "run".into(),
        behavior: PendingMessageBehavior::Queue,
        content: format!("draft {id}"),
        images: Vec::new(),
        references: Vec::new(),
        reference_mentions: Vec::new(),
        skill_mentions: Vec::new(),
        permission_fence: None,
    }
}

fn snapshot() -> Snapshot {
    Snapshot {
        id: ProjectionId::new(),
        epoch: ProjectionId::new(),
        source_entity: gpui_kit::EntityId::from(1_u64),
        source: Source {
            thread: Thread {
                last_response_failed: false,
                reasoning_preferences: None,
                parent_thread_id: None,
                forked_from_thread_id: None,
                forked_from_message_id: None,
                id: "thread".into(),
                backend: TaskBackend::Native,
                project_id: "project".into(),
                title: "Synthetic".into(),
                mode: ThreadMode::Ask,
                permission_mode: PermissionMode::ReadOnly,
                permission_revision: 1,
                draft_authority_epoch: None,
                full_access_grant: None,
                default_permission: None,
                model: "model".into(),
                status: vega_conversation::types::ThreadStatus::Active,
                pinned: false,
                unread: false,
                created_at: 0,
                updated_at: 0,
            },
            input: gpui_kit::EntityId::from(2_u64),
            revision: 3,
            messages: vec![message("first"), message("second")],
            visible: vec!["first".into(), "second".into()],
            recovered: Vec::new(),
            timestamps: HashMap::new(),
            generation: None,
            menu: None,
            error: None,
            projection_error: None,
            default: PendingMessageBehavior::Queue,
            global_default: PendingMessageBehavior::Queue,
            active_agent: None,
            active_response: None,
            active_sequence: None,
            attachment_generation: 0,
            state: vec![false; 16],
            appearance: vega_theme::Appearance::Light,
        },
        content: Content::Button {
            action: PendingControlAction::Queue(PendingQueueAction::Remove("first".into())),
            label: "Delete message".into(),
            reason: None,
            style: ButtonStyle::Icon,
            enabled: true,
            selected: false,
        },
        glyph: crate::icons::nsh_source(Icon::Trash),
        focused: false,
        geometry: None,
        colors: vega_theme::Theme::light().colors,
    }
}

#[test]
fn delayed_action_rejects_reordered_projection_replaced_source_epoch_and_presentation() {
    let original = snapshot();
    assert!(original.accepts(&original, true, false, false));
    let mut changed = original.clone();
    changed.source.messages.swap(0, 1);
    assert!(!changed.accepts(&original, true, false, false));
    let mut changed = original.clone();
    changed.source_entity = gpui_kit::EntityId::from(9_u64);
    assert!(!changed.accepts(&original, true, false, false));
    let mut changed = original.clone();
    changed.epoch = ProjectionId::new();
    assert!(!changed.accepts(&original, true, false, false));
    let mut changed = original.clone();
    changed.id = ProjectionId::new();
    assert!(!changed.accepts(&original, true, false, false));
    let mut changed = original.clone();
    changed.source.revision += 1;
    assert!(!changed.accepts(&original, true, false, false));
    let mut changed = original.clone();
    changed.source.menu = Some("second".into());
    assert!(!changed.accepts(&original, true, false, false));
    let mut changed = original.clone();
    changed.source.visible.remove(0);
    assert!(!changed.accepts(&original, true, false, false));
    for index in 0..original.source.state.len() {
        let mut changed = original.clone();
        changed.source.state[index] = !changed.source.state[index];
        assert!(!changed.accepts(&original, true, false, false));
    }
    let mut changed = original.clone();
    changed.source.thread.permission_revision += 1;
    assert!(!changed.accepts(&original, true, false, false));
    let mut changed = original.clone();
    changed.source.active_response = Some("replacement-stream".into());
    assert!(!changed.accepts(&original, true, false, false));
    let mut changed = original.clone();
    changed.source.recovered.push(message("recovered"));
    assert!(!changed.accepts(&original, true, false, false));
    let mut changed = original.clone();
    changed.source.global_default = PendingMessageBehavior::Steer;
    assert!(!changed.accepts(&original, true, false, false));
}

#[test]
fn action_requires_display_focus_and_exact_message_identity_not_row_index() {
    let original = snapshot();
    assert!(!original.accepts(&original, false, false, false));
    assert!(!original.accepts(&original, true, true, false));
    assert!(original.accepts(&original, true, true, true));
    assert!(
        matches!(original.action(), Some(PendingControlAction::Queue(PendingQueueAction::Remove(id))) if id == "first")
    );
    let mut changed = original.clone();
    if let Content::Button { action, .. } = &mut changed.content {
        *action = PendingControlAction::Queue(PendingQueueAction::Remove("second".into()));
    }
    assert!(!changed.accepts(&original, true, false, false));
    if let Content::Button { enabled, .. } = &mut changed.content {
        *enabled = false;
    }
    assert!(changed.action().is_none());
    assert!(!changed.accepts(&changed, true, false, false));
}

thread_local! {
    static INITIAL: RefCell<Option<Snapshot>> = const { RefCell::new(None) };
    static REQUESTS: RefCell<Option<mpsc::Receiver<Request>>> = const { RefCell::new(None) };
}

#[ns::component]
fn PendingFixture() -> impl ns::IntoElement {
    let initial = INITIAL.with(|slot| slot.borrow_mut().take().expect("pending snapshot"));
    let (sender, receiver) = mpsc::channel(crate::nsh::COMMAND_CAPACITY);
    REQUESTS.with(|slot| *slot.borrow_mut() = Some(receiver));
    pixels::render(ns::signal_with(initial, ns::SignalEquals::Never), sender)
}

fn mount(snapshot: Snapshot) -> Vec<MountItem> {
    INITIAL.with(|slot| *slot.borrow_mut() = Some(snapshot));
    let mut core = Core::new(ApproxTextEngine, ns::size(ns::px(240.), ns::px(120.)));
    core.mount_root(&Root::of::<PendingFixture>());
    let mut items = Vec::new();
    let mut settled = false;
    for _ in 0..24 {
        core.run_component_tasks();
        let batch = core.commit();
        let mut events = Vec::new();
        core.take_events(&mut events);
        if batch.is_none() && events.is_empty() {
            settled = true;
            break;
        }
        if let Some(batch) = batch {
            items.extend(batch.items);
        }
        for event in events {
            match event {
                JsEvent::Event(id, kind, payload) => {
                    core.component_event(id, kind, &payload);
                }
                JsEvent::LayoutListener {
                    host,
                    id,
                    listener,
                    frame,
                } => core.component_layout(host, id, &listener, frame),
                _ => {}
            }
        }
    }
    assert!(settled, "pending renderer must settle");
    let mut receiver = REQUESTS.with(|slot| slot.borrow_mut().take().expect("pending requests"));
    while let Ok(request) = receiver.try_recv() {
        assert!(
            !matches!(request, Request::Activate(_)),
            "mount must never activate a pending operation"
        );
    }
    items
}

#[test]
fn real_native_menu_keeps_disabled_reason_and_never_executes_on_mount() {
    let mut initial = snapshot();
    initial.content = Content::Button {
        action: PendingControlAction::Queue(PendingQueueAction::Edit("first".into())),
        label: "Edit message".into(),
        reason: Some("Send or clear the current draft first".into()),
        style: ButtonStyle::Menu,
        enabled: false,
        selected: false,
    };
    let items = mount(initial);
    assert!(
        items.iter().any(
            |item| matches!(item, MountItem::Text { text, .. } if text.text == "Edit message")
        )
    );
    assert!(items.iter().any(|item| matches!(item, MountItem::Text { text, .. } if text.text == "Send or clear the current draft first")));
    assert!(items.iter().any(|item| matches!(item, MountItem::ViewProps { props, .. }
        if props.accessibility.role == AccessibilityRole::Button
        && props.accessibility.label.as_deref() == Some("Edit message. Send or clear the current draft first")
        && props.accessibility.state.disabled)));
}

#[test]
fn real_native_status_keeps_original_error_without_durable_success_claim() {
    let mut initial = snapshot();
    initial.content = Content::Text("Could not update the queue".into(), TextKind::Error);
    initial.glyph = None;
    let items = mount(initial);
    let texts = items
        .iter()
        .filter_map(|item| match item {
            MountItem::Text { text, .. } => Some(text.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(texts, vec!["Could not update the queue"]);
}
