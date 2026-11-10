use super::{
    Authority, Child, ClockLease, Headers, LayoutAck, LayoutStamp, Mount, Publication, Snapshot,
    Target, pixels,
};
use crate::icons::Icon;
use crate::nsh::ProjectionId;
use gpui_kit::EntityId;
use nsh_core::element::JsEvent;
use nsh_core::prelude as ns;
use nsh_core::{ApproxTextEngine, Core, EventKind, MountItem};
use std::cell::RefCell;
use tokio::sync::mpsc;
use vega_theme::{Layout, Theme};

thread_local! {
    static INPUT: std::cell::RefCell<Option<Snapshot>> = const { std::cell::RefCell::new(None) };
    static CHANNELS: RefCell<Option<Channels>> = const { RefCell::new(None) };
}

#[test]
fn co_mounted_run_and_stage_headers_keep_distinct_retained_state_slots() {
    let group = EntityId::from(100_u64);
    let other = EntityId::from(101_u64);
    let segment = EntityId::from(102_u64);
    let next_segment = EntityId::from(103_u64);
    let run = super::header_mount_id(group, Target::Run, None);
    let neighbor = super::header_mount_id(other, Target::Run, None);
    let stage = super::header_mount_id(group, Target::Stage(0), Some(segment));
    let next = super::header_mount_id(group, Target::Stage(1), Some(next_segment));
    assert_ne!(run, neighbor);
    assert_ne!(run, stage);
    assert_ne!(stage, next);
    assert_eq!(run, super::header_mount_id(group, Target::Run, None));
    assert_eq!(
        stage,
        super::header_mount_id(group, Target::Stage(0), Some(segment))
    );
}

fn snapshot() -> Snapshot {
    Snapshot {
        id: ProjectionId::new(),
        projection: ProjectionId::new(),
        layout: Some(LayoutStamp {
            frame: ProjectionId::new(),
            size: [640., Layout::CONVERSATION_CLOCK_HEIGHT],
        }),
        appearance: vega_theme::Appearance::Light,
        language: "en",
        revision: ProjectionId::new(),
        owner: Some(ProjectionId::new()),
        authority: Authority {
            target: Target::Run,
            segment: None,
            children: vec![Child::Tool(EntityId::from(12_u64))],
            expanded: false,
            can_expand: true,
            title: "Completed in 7s".into(),
            icon: None,
            live: false,
            status: super::RunActivityStatus::Completed,
            released: true,
            details: true,
        },
        label: "Completed in 7s, expand".into(),
        colors: Theme::light().colors,
        focused: false,
        hovered: false,
        pressed: false,
        title_width: 80.,
        divider: false,
        attached: false,
    }
}

#[test]
fn activation_rejects_owner_rebind_aba_revision_and_recycled_stage() {
    let original = snapshot();
    assert!(original.accepts(&original));
    let mut current = original.clone();
    current.owner = Some(ProjectionId::new());
    assert!(!original.accepts(&current));
    current = original.clone();
    current.revision = ProjectionId::new();
    assert!(!original.accepts(&current));
    current = original.clone();
    current.id = ProjectionId::new();
    assert!(!original.accepts(&current));
    current = original.clone();
    current.authority.segment = Some(EntityId::from(90_u64));
    assert!(!original.accepts(&current));
    current = original.clone();
    current.owner = None;
    assert!(!original.accepts(&current));
}

#[test]
fn activation_rechecks_children_availability_headline_status_and_expansion() {
    let original = snapshot();
    let mut changed = original.clone();
    changed
        .authority
        .children
        .push(Child::Compaction("model".into(), 2));
    assert!(!original.accepts(&changed));
    changed = original.clone();
    changed.authority.can_expand = false;
    assert!(!original.accepts(&changed));
    changed = original.clone();
    changed.authority.expanded = true;
    assert!(!original.accepts(&changed));
    changed = original.clone();
    changed.authority.title = "Failed in 7s".into();
    assert!(!original.accepts(&changed));
    changed = original.clone();
    changed.authority.status = super::RunActivityStatus::Failed;
    assert!(!original.accepts(&changed));
    changed = original.clone();
    changed.focused = true;
    changed.hovered = true;
    assert!(original.accepts(&changed));
}

struct Channels {
    updates: mpsc::Sender<Snapshot>,
    requests: mpsc::Receiver<LayoutAck>,
}

#[ns::component]
fn HeaderFixture() -> impl ns::IntoElement {
    let snapshot = INPUT
        .with(|input| input.borrow().clone())
        .expect("fixture projection");
    let state = ns::signal_with(snapshot, ns::SignalEquals::Never);
    let (updates, mut incoming) = mpsc::channel(8);
    let (sender, requests) = mpsc::channel(1);
    CHANNELS.with(|slot| *slot.borrow_mut() = Some(Channels { updates, requests }));
    ns::spawn(async move {
        while let Some(next) = incoming.recv().await {
            state.set(next);
        }
    });
    pixels::render(state, sender)
}

#[test]
fn production_renderer_mounts_real_run_text_and_stage_chrome_without_duplicate_title() {
    for width in [110_f32, 640_f32] {
        for stage in [false, true] {
            let mut projected = snapshot();
            if stage {
                projected.authority.target = Target::Stage(0);
                projected.authority.segment = Some(EntityId::from(45_u64));
                projected.authority.icon = Some(Icon::Wrench);
                projected.authority.title = "Read example.rs".into();
            }
            let title = projected.authority.title.clone();
            INPUT.with(|input| *input.borrow_mut() = Some(projected));
            let height = if stage {
                Layout::CONVERSATION_TOOL_HEIGHT
            } else {
                Layout::CONVERSATION_CLOCK_HEIGHT
            };
            let mut core = Core::new(ApproxTextEngine, ns::size(ns::px(width), ns::px(height)));
            core.mount_root(&nsh_core::element::Root::of::<HeaderFixture>());
            let batch = core.commit().expect("production renderer mount");
            let text = batch
                .items
                .iter()
                .any(|item| matches!(item, MountItem::Text { text, .. } if text.text == title));
            assert_eq!(
                text, !stage,
                "stage source remains the original NSH StageText, not a duplicate plain title"
            );
            assert!(batch.items.iter().any(|item| matches!(item, MountItem::Frame { frame, .. }
                if (frame.size.width.0 - width).abs() < 0.01 && (frame.size.height.0 - height).abs() < 0.01)));
            for item in &batch.items {
                if let MountItem::Events { events, .. } = item {
                    assert!(
                        events.bits() & !(1_u128 << EventKind::Layout as u8) == 0,
                        "passive chrome may observe layout but cannot consume host gestures"
                    );
                }
            }
            INPUT.with(|input| *input.borrow_mut() = None);
            CHANNELS.with(|slot| slot.borrow_mut().take());
        }
    }
}

fn ack(snapshot: &Snapshot) -> LayoutAck {
    let size = snapshot.layout.as_ref().expect("mounted snapshot").size;
    LayoutAck {
        snapshot: snapshot.clone(),
        root: [0., 0., size[0], size[1]],
    }
}

#[test]
fn delayed_layout_cannot_authorize_new_expansion_or_focus_selection_roundtrip() {
    let initial = snapshot();
    let mut publication = Publication::new(initial.clone());
    assert!(!publication.accepts(&initial));
    assert!(publication.acknowledge(&ack(&initial)));
    assert!(publication.accepts(&initial));
    assert!(
        !publication.acknowledge(&ack(&initial)),
        "duplicate ack must not request another frame"
    );
    let mut next = initial.clone();
    next.authority.expanded = true;
    assert!(publication.publish(next));
    let expanded = publication.published.clone();
    assert!(!publication.accepts(&expanded));
    assert!(!publication.acknowledge(&ack(&initial)));
    assert!(publication.acknowledge(&ack(&expanded)));
    let mut focused = expanded.clone();
    focused.focused = true;
    publication.publish(focused);
    let selected = publication.published.clone();
    assert!(!publication.accepts(&selected));
    assert!(!publication.acknowledge(&ack(&expanded)));
    assert!(publication.acknowledge(&ack(&selected)));
    publication.publish(expanded.clone());
    let restored = publication.published.clone();
    assert!(!restored.projection.matches(&expanded.projection));
    assert!(
        !publication.acknowledge(&ack(&expanded)),
        "restored selection is not the old frame"
    );
    assert!(!publication.acknowledge(&ack(&selected)));
    let mut wrong_size = ack(&restored);
    wrong_size.root[2] -= 1.;
    assert!(!publication.acknowledge(&wrong_size));
    assert!(publication.acknowledge(&ack(&restored)));
    let mut cosmetic = restored.clone();
    cosmetic.hovered = true;
    cosmetic.pressed = true;
    publication.publish(cosmetic);
    assert!(
        publication.accepts(&restored),
        "paint feedback must preserve the down-time authority"
    );
    assert!(
        !publication.publish(publication.published.clone()),
        "identical frame must stabilize"
    );
}

#[test]
fn native_move_clip_roundtrip_and_recycled_mount_require_fresh_engine_ack() {
    use gpui_kit::{Bounds, point, px, size};
    let bounds = Bounds {
        origin: point(px(20.), px(40.)),
        size: size(px(640.), px(Layout::CONVERSATION_CLOCK_HEIGHT)),
    };
    let viewport = size(px(800.), px(600.));
    let mut mount = Mount::default();
    mount.update(bounds, bounds, viewport, 2.);
    let mut initial = snapshot();
    initial.layout = mount.layout();
    let mut publication = Publication::new(initial.clone());
    assert!(publication.acknowledge(&ack(&initial)));
    mount.update(bounds, bounds, viewport, 2.);
    assert!(
        initial
            .layout
            .as_ref()
            .unwrap()
            .matches(&mount.layout().unwrap())
    );
    let mut moved = bounds;
    moved.origin.y += px(10.);
    mount.update(moved, moved, viewport, 2.);
    let mut next = initial.clone();
    next.layout = mount.layout();
    publication.publish(next);
    assert!(!publication.acknowledge(&ack(&initial)));
    mount.update(bounds, bounds, viewport, 2.);
    let mut restored = initial.clone();
    restored.layout = mount.layout();
    publication.publish(restored);
    assert!(!publication.acknowledge(&ack(&initial)));
    let current = publication.published.clone();
    assert!(publication.acknowledge(&ack(&current)));
    let mut clipped = bounds;
    clipped.size.width -= px(8.);
    mount.update(bounds, clipped, viewport, 2.);
    let mut next = current.clone();
    next.layout = mount.layout();
    publication.publish(next);
    assert!(!publication.accepts(&current));
    let mut recycled = Mount::default();
    recycled.update(bounds, bounds, viewport, 2.);
    let mut next = current.clone();
    next.layout = recycled.layout();
    publication.publish(next);
    assert!(!publication.acknowledge(&ack(&current)));
    assert!(publication.acknowledge(&ack(&publication.published.clone())));
}

#[test]
fn current_paint_replaces_stale_clock_and_old_expiry_cannot_clear_replacement() {
    let initial = snapshot();
    let lease = ClockLease {
        id: ProjectionId::new(),
        snapshot: initial.clone(),
    };
    let mut headers = Headers {
        clock: Some(lease.clone()),
        ..Headers::default()
    };
    assert!(headers.retains_clock(&initial, true));
    assert!(!headers.retains_clock(&initial, false));
    let mut resized = initial.clone();
    resized.layout.as_mut().unwrap().frame = ProjectionId::new();
    assert!(!headers.retains_clock(&resized, true));
    let replacement = ClockLease {
        id: ProjectionId::new(),
        snapshot: resized.clone(),
    };
    headers.clock = Some(replacement.clone());
    assert!(!headers.finish_clock(&lease));
    assert!(headers.retains_clock(&resized, true));
    assert!(headers.finish_clock(&replacement));
    assert!(headers.clock.is_none(), "expiry cannot self-rearm");
    headers.clock = Some(replacement.clone());
    headers.invalidate();
    assert!(!headers.retains_clock(&resized, true));
    assert!(!headers.finish_clock(&replacement));
    let mut changed = resized.clone();
    changed.revision = headers.revision.clone();
    changed.projection = ProjectionId::new();
    let fresh = ClockLease {
        id: ProjectionId::new(),
        snapshot: changed.clone(),
    };
    headers.clock = Some(fresh.clone());
    assert!(headers.retains_clock(&changed, true));
    assert!(!headers.finish_clock(&replacement));
    assert!(headers.finish_clock(&fresh));
}

#[test]
fn unowned_static_chrome_stabilizes_without_gaining_display_authority() {
    let mut initial = snapshot();
    initial.owner = None;
    let mut publication = Publication::new(initial.clone());
    for _ in 0..64 {
        assert!(!publication.publish(initial.clone()));
        assert!(
            publication
                .published
                .projection
                .matches(&initial.projection)
        );
        assert!(!publication.acknowledge(&ack(&initial)));
        assert!(!publication.accepts(&initial));
        assert!(!initial.accepts(&initial));
    }
    let mut owned = initial.clone();
    owned.owner = Some(ProjectionId::new());
    assert!(publication.publish(owned));
    let current = publication.published.clone();
    assert!(publication.acknowledge(&ack(&current)));
    assert!(publication.accepts(&current));
    assert!(publication.publish(initial.clone()));
    assert!(!publication.accepts(&current));
    assert!(!publication.acknowledge(&ack(&current)));
    assert!(!publication.acknowledge(&ack(&publication.published.clone())));
    assert!(!publication.publish(initial));
}

#[test]
fn stable_header_updates_preserve_ack_but_real_identity_and_visual_changes_republish() {
    let initial = snapshot();
    let mut publication = Publication::new(initial.clone());
    assert!(publication.acknowledge(&ack(&initial)));
    for _ in 0..64 {
        assert!(!publication.publish(initial.clone()));
        assert!(publication.accepts(&initial));
        assert!(!publication.acknowledge(&ack(&initial)));
    }
    for change in 0..7 {
        let previous = publication.published.clone();
        let mut next = previous.clone();
        match change {
            0 => next.authority.title = "Completed in 8s".into(),
            1 => next.owner = Some(ProjectionId::new()),
            2 => next.revision = ProjectionId::new(),
            3 => next.focused = true,
            4 => next.layout.as_mut().unwrap().size[0] += 8.,
            5 => next.layout.as_mut().unwrap().frame = ProjectionId::new(),
            6 => next.id = ProjectionId::new(),
            _ => unreachable!(),
        }
        assert!(publication.publish(next));
        let current = publication.published.clone();
        assert!(!publication.accepts(&previous));
        assert!(!publication.accepts(&current));
        assert!(!publication.acknowledge(&ack(&previous)));
        assert!(publication.acknowledge(&ack(&current)));
        assert!(!publication.publish(current.clone()));
        assert!(publication.accepts(&current));
    }
    let current = publication.published.clone();
    let mut hovered = current.clone();
    hovered.hovered = true;
    assert!(publication.publish(hovered));
    assert!(publication.accepts(&current));
    assert!(!publication.publish(publication.published.clone()));
    let mut pressed = publication.published.clone();
    pressed.pressed = true;
    assert!(publication.publish(pressed));
    assert!(publication.accepts(&current));
    assert!(!publication.publish(publication.published.clone()));
    assert!(publication.publish(current.clone()));
    assert!(publication.accepts(&current));
}

#[test]
fn hidden_mount_roundtrip_rehydrates_with_new_frame_not_numeric_geometry() {
    use gpui_kit::{Bounds, point, px, size};
    let bounds = Bounds::new(point(px(20.), px(40.)), size(px(640.), px(28.)));
    let hidden = Bounds::new(bounds.origin, size(px(0.), px(0.)));
    let viewport = size(px(800.), px(600.));
    let mut mount = Mount::default();
    mount.update(bounds, bounds, viewport, 2.);
    let mut initial = snapshot();
    initial.layout = mount.layout();
    let mut publication = Publication::new(initial.clone());
    assert!(publication.acknowledge(&ack(&initial)));
    mount.update(bounds, hidden, viewport, 2.);
    let mut next = initial.clone();
    next.layout = mount.layout();
    assert!(next.layout.is_none());
    assert!(publication.publish(next));
    assert!(!publication.accepts(&initial));
    assert!(!publication.acknowledge(&ack(&initial)));
    for _ in 0..64 {
        mount.update(bounds, hidden, viewport, 2.);
        let mut next = publication.published.clone();
        next.layout = mount.layout();
        assert!(!publication.publish(next));
    }
    mount.update(bounds, bounds, viewport, 2.);
    let mut restored = initial.clone();
    restored.layout = mount.layout();
    assert!(publication.publish(restored));
    let current = publication.published.clone();
    assert!(
        !current
            .layout
            .as_ref()
            .unwrap()
            .matches(initial.layout.as_ref().unwrap())
    );
    assert!(!publication.acknowledge(&ack(&initial)));
    assert!(publication.acknowledge(&ack(&current)));
    assert!(publication.accepts(&current));
    assert!(!publication.publish(current));
}

fn settle(core: &mut Core<ApproxTextEngine>) {
    core.run_component_tasks();
    for _ in 0..16 {
        let batch = core.commit();
        let mut events = Vec::new();
        core.take_events(&mut events);
        if batch.is_none() && events.is_empty() {
            return;
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
    panic!("production header did not stabilize");
}

#[test]
fn real_renderer_acks_same_size_updates_after_dynamic_owner_disposal() {
    let initial = snapshot();
    INPUT.with(|slot| *slot.borrow_mut() = Some(initial.clone()));
    let mut core = Core::new(
        ApproxTextEngine,
        ns::size(ns::px(640.), ns::px(Layout::CONVERSATION_CLOCK_HEIGHT)),
    );
    core.mount_root(&nsh_core::element::Root::of::<HeaderFixture>());
    core.commit().expect("production renderer mount");
    let mut channels = CHANNELS.with(|slot| slot.borrow_mut().take().expect("fixture channels"));
    settle(&mut core);
    let mut publication = Publication::new(initial.clone());
    assert!(
        !publication.accepts(&initial),
        "engine ack remains delayed in the bounded queue"
    );
    let mut selected = initial.clone();
    selected.focused = true;
    publication.publish(selected);
    let selected = publication.published.clone();
    channels
        .updates
        .try_send(selected.clone())
        .expect("selection publication");
    settle(&mut core);
    publication.publish(initial.clone());
    let restored = publication.published.clone();
    channels
        .updates
        .try_send(restored.clone())
        .expect("selection restoration");
    settle(&mut core);
    let first = channels
        .requests
        .try_recv()
        .expect("initial queued layout ack");
    assert!(first.snapshot.same_projection(&initial));
    assert!(initial.layout.as_ref().unwrap().accepts_root(first.root));
    assert!(!publication.acknowledge(&first));
    settle(&mut core);
    let pending_selected = channels
        .requests
        .try_recv()
        .expect("same-size selected ack survived disposed region while send blocked");
    assert!(pending_selected.snapshot.same_projection(&selected));
    assert!(!publication.acknowledge(&pending_selected));
    settle(&mut core);
    let pending_restored = channels
        .requests
        .try_recv()
        .expect("restored frame ack survives dynamic-owner disposal");
    assert!(!publication.accepts(&restored));
    assert!(pending_restored.snapshot.same_projection(&restored));
    assert!(publication.acknowledge(&pending_restored));
    assert!(publication.accepts(&restored));
    assert!(channels.requests.try_recv().is_err());
}
