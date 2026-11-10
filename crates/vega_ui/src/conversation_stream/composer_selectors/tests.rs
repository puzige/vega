use super::*;
use nsh_core::{ApproxTextEngine, Core, MountItem, element::JsEvent, prelude as ns};
use std::cell::RefCell;
use tokio::sync::mpsc;

#[gpui_kit::test]
fn owner_notifications_converge_and_real_source_aba_revokes_published_intent(
    cx: &mut gpui_kit::TestAppContext,
) {
    let notifications = Rc::new(Cell::new(0));
    let count = notifications.clone();
    let (owner, host, _feedback, original) = cx.update(|cx| {
        cx.set_global(vega_theme::Theme::light());
        let owner = cx.new(|cx| ConversationStream::new(snapshot().source.thread, cx));
        let initial = Snapshot::capture(owner.read(cx), Kind::ModelTrigger, cx);
        let host = cx.new(|cx| Host {
            owner: owner.downgrade(),
            snapshot: initial,
            surface: None,
            published: None,
            lease: Weak::new(),
            focus: cx.focus_handle(),
            height: Some(28.),
            activation_focus: None,
            menu: Rc::new(RefCell::new(None)),
            _subscriptions: vec![Host::observe_owner(&owner, cx)],
        });
        let original = host.update(cx, |host, _| {
            host.snapshot.activation = Some((0, Origin::Keyboard));
            host.activation_focus = Some(host.focus.clone());
            host.published = Some(host.snapshot.clone());
            host.snapshot.clone()
        });
        let parent = owner.downgrade();
        let feedback = cx.observe(&host, move |_, cx| {
            count.set(count.get() + 1);
            assert!(
                count.get() <= 8,
                "selector notification feedback did not converge"
            );
            if let Some(parent) = parent.upgrade() {
                cx.notify(parent.entity_id());
            }
        });
        (owner, host, feedback, original)
    });

    owner.update(cx, |_, cx| cx.notify());
    assert_eq!(notifications.get(), 0);
    host.update(cx, |_, cx| cx.notify());
    assert_eq!(notifications.get(), 1);
    cx.update(|cx| {
        let host = host.read(cx);
        assert!(host.snapshot.id.matches(&original.id));
        assert!(host.snapshot.intent.matches(&original.intent));
        assert!(host.published.as_ref().unwrap().id.matches(&original.id));
        assert!(host.snapshot.activation == Some((0, Origin::Keyboard)));
        assert!(host.activation_focus.is_some());
        assert_eq!(host.height, Some(28.));
    });

    owner.update(cx, |stream, cx| {
        stream.next_turn_model = Some("changed-model".into());
        cx.notify();
    });
    assert_eq!(notifications.get(), 2);
    let changed = cx.update(|cx| {
        let host = host.read(cx);
        assert_eq!(
            host.snapshot.source.next_model.as_deref(),
            Some("changed-model")
        );
        assert!(host.published.is_none());
        assert!(host.snapshot.activation.is_none());
        assert!(host.activation_focus.is_none());
        assert!(host.height.is_none());
        for origin in [Origin::Pointer, Origin::Keyboard, Origin::Accessibility] {
            assert!(!host.snapshot.accepts(&original, 0, origin, 0));
        }
        host.snapshot.clone()
    });
    owner.update(cx, |_, cx| cx.notify());
    assert_eq!(notifications.get(), 2);
    owner.update(cx, |stream, cx| {
        stream.next_turn_model = None;
        cx.notify();
    });
    assert_eq!(notifications.get(), 3);
    cx.update(|cx| {
        let host = host.read(cx);
        assert!(host.snapshot.same_source(&original));
        assert!(!host.snapshot.id.matches(&original.id));
        assert!(!host.snapshot.id.matches(&changed.id));
        for origin in [Origin::Pointer, Origin::Keyboard, Origin::Accessibility] {
            assert!(!host.snapshot.accepts(&original, 0, origin, 0));
        }
    });
}

thread_local! {
    static INITIAL: RefCell<Option<Snapshot>> = const { RefCell::new(None) };
}

#[ns::component]
fn ProductionSelectors() -> impl ns::IntoElement {
    let snapshot = INITIAL.with(|slot| slot.borrow_mut().take().expect("snapshot"));
    let state = ns::signal_with(snapshot, ns::SignalEquals::Never);
    let (sender, _) = mpsc::channel(64);
    super::pixels::render(state, sender)
}

fn snapshot() -> Snapshot {
    let option = ModelOption {
        provider: "Provider A".into(),
        model: "same-model".into(),
    };
    Snapshot {
        id: ProjectionId::new(),
        epoch: ProjectionId::new(),
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
                title: "Task".into(),
                mode: ThreadMode::Ask,
                permission_mode: PermissionMode::ReadOnly,
                permission_revision: 1,
                draft_authority_epoch: None,
                full_access_grant: None,
                default_permission: None,
                model: "same-model".into(),
                status: vega_conversation::types::ThreadStatus::Active,
                pinned: false,
                unread: false,
                created_at: 0,
                updated_at: 0,
            },
            input: gpui_kit::EntityId::from(1_u64),
            input_revision: 1,
            models: vec![option.clone()],
            defaults: ComposerDefaults::default(),
            next_model: None,
            pending: None,
            save_owner: None,
            level: ModelPickerLevel::List,
            highlight: 0,
            permission_open: false,
            active_mode: None,
            progress: None,
            slider: super::super::thinking_slider::native::Snapshot {
                model: ThinkingSliderModel::new(
                    vec!["low".into(), "high".into()],
                    true,
                    "provider_default",
                    "low",
                ),
                model_name: "same-model".into(),
            },
            reasoning: None,
            reasoning_intents: HashMap::new(),
            state: vec![false; 9],
            appearance: vega_theme::Appearance::Light,
        },
        kind: Kind::ModelMenu,
        rows: vec![Row {
            action: Action::Model(option),
            label: "same-model".into(),
            description: None,
            group: Some("Provider A".into()),
            checked: true,
            warning: false,
            enabled: true,
            glyph: None,
        }],
        header: vec!["Models".into()],
        colors: vega_theme::Theme::light().colors,
        geometry: None,
        mount: None,
        selected: Some(0),
        intent: ProjectionId::new(),
        activation: None,
        native_revision: Arc::new(AtomicU64::new(0)),
    }
}

#[test]
fn provider_reorder_or_same_name_replacement_never_authorizes_an_old_model_row() {
    let original = snapshot();
    assert!(original.accepts(&original, 0, Origin::Pointer, 0));
    let mut changed = original.clone();
    changed.source.models[0].provider = "Provider B".into();
    assert!(!changed.accepts(&original, 0, Origin::Pointer, 0));
    let mut changed = original.clone();
    changed.rows[0].action = Action::Model(ModelOption {
        provider: "Provider B".into(),
        model: "same-model".into(),
    });
    assert!(!changed.accepts(&original, 0, Origin::Pointer, 0));
}

#[test]
fn measured_action_rejects_layout_scroll_intent_and_publication_aba() {
    let original = snapshot();
    let mut current = original.clone();
    current.native_revision.fetch_add(1, Ordering::AcqRel);
    assert!(!current.accepts(&original, 0, Origin::Pointer, 0));
    assert!(current.accepts(&original, 0, Origin::Pointer, 1));
    current.id = ProjectionId::new();
    assert!(!current.accepts(&original, 0, Origin::Pointer, 1));
    let mut current = original.clone();
    current.intent = ProjectionId::new();
    assert!(!current.accepts(&original, 0, Origin::Pointer, 1));
    let mut current = original.clone();
    current.native_revision = Arc::new(AtomicU64::new(1));
    assert!(!current.accepts(&original, 0, Origin::Pointer, 1));
}

#[test]
fn keyboard_and_accessibility_require_their_own_fresh_activation_intent() {
    let mut current = snapshot();
    assert!(!current.accepts(&current, 0, Origin::Keyboard, 0));
    current.activation = Some((0, Origin::Keyboard));
    assert!(current.accepts(&current, 0, Origin::Keyboard, 0));
    assert!(!current.accepts(&current, 0, Origin::Accessibility, 0));
    current.rows[0].enabled = false;
    assert!(!current.accepts(&current, 0, Origin::Keyboard, 0));
}

#[test]
fn original_slider_choice_model_keeps_off_default_declared_order_and_exact_centers() {
    use super::super::thinking_slider::native::position_for_x;
    let mut slider = ThinkingSliderModel::new(
        vec!["low".into(), "high".into()],
        true,
        "provider_default",
        "low",
    );
    assert!(slider.is_provider_default());
    assert_eq!(slider.selected_position(), None);
    assert_eq!(slider.strongest_position(), Some(2));
    assert_eq!(position_for_x(12., 3), Some(0));
    assert_eq!(position_for_x(101.5, 3), Some(1));
    assert_eq!(position_for_x(191., 3), Some(2));
    assert_eq!(position_for_x(f32::NAN, 3), None);
    assert_eq!(position_for_x(1., 0), None);
    slider.set_tiers(
        vec!["low".into(), "high".into()],
        true,
        OFF_CHOICE_NAME,
        "low",
    );
    assert!(slider.is_off());
    assert_eq!(slider.choice_name(), Some(OFF_CHOICE_NAME));
    slider.set_tiers(vec!["low".into(), "high".into()], false, "high", "low");
    assert_eq!(slider.selected_position(), Some(1));
    assert_eq!(slider.choice_name(), Some("high"));
}

#[test]
fn real_native_permission_projection_retains_warning_description_and_learn_more() {
    let mut snapshot = snapshot();
    snapshot.kind = Kind::PermissionMenu;
    snapshot.header = vec!["Next run".into(), "Learn more".into()];
    snapshot.rows = vec![Row {
        action: Action::Permission(PermissionMode::FullAccess),
        label: "Full access".into(),
        description: Some("Unsafe actions still require explicit confirmation".into()),
        group: None,
        checked: false,
        warning: true,
        enabled: true,
        glyph: Some(crate::icons::Icon::Warning),
    }];
    INITIAL.with(|slot| *slot.borrow_mut() = Some(snapshot));
    let mut core = Core::new(ApproxTextEngine, ns::size(ns::px(320.), ns::px(240.)));
    core.mount_root(&nsh_core::element::Root::of::<ProductionSelectors>());
    let mut items = Vec::new();
    let mut settled = false;
    for _ in 0..24 {
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
        core.run_component_tasks();
    }
    assert!(settled, "production selector did not settle");
    for text in [
        "Full access",
        "Unsafe actions still require explicit confirmation",
        "Learn more",
    ] {
        assert!(items.iter().any(
            |item| matches!(item, MountItem::Text { text: actual, .. } if actual.text == text)
        ));
    }
    assert!(items.iter().any(|item| matches!(item, MountItem::ViewProps { props, .. }
        if props.accessibility.label.as_deref() == Some("Full access\nUnsafe actions still require explicit confirmation"))));
}
