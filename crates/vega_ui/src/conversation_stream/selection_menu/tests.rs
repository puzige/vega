use super::*;
use core::prelude::v1::test;
use nsh_core::element::{JsEvent, Root};
use nsh_core::prelude as ns;
use nsh_core::{ApproxTextEngine, Core, EventKind, EventPayload, MountItem, NodeId};
use std::cell::RefCell;
use std::collections::BTreeMap;
use tokio::sync::mpsc;
use vega_theme::Theme;

thread_local! {
    static INITIAL: RefCell<Option<Snapshot>> = const { RefCell::new(None) };
    static CHANNELS: RefCell<Option<Channels>> = const { RefCell::new(None) };
}

struct Channels {
    updates: mpsc::Sender<Snapshot>,
    requests: mpsc::Receiver<Request>,
}

#[ns::component]
fn Fixture() -> impl ns::IntoElement {
    let initial = INITIAL.with(|slot| slot.borrow_mut().take().unwrap());
    let state = ns::signal_with(initial, ns::SignalEquals::Never);
    let (updates, mut incoming) = mpsc::channel(8);
    let (sender, requests) = mpsc::channel(64);
    CHANNELS.with(|slot| *slot.borrow_mut() = Some(Channels { updates, requests }));
    ns::spawn(async move {
        while let Some(next) = incoming.recv().await {
            state.set(next);
        }
    });
    pixels::render(state, sender)
}

struct Harness {
    core: Core<ApproxTextEngine>,
    channels: Channels,
    presses: BTreeMap<NodeId, bool>,
    text: BTreeMap<NodeId, String>,
}

impl Harness {
    fn mount(snapshot: Snapshot) -> Self {
        let viewport = ns::size(ns::px(snapshot.width), ns::px(snapshot.height));
        INITIAL.with(|slot| *slot.borrow_mut() = Some(snapshot));
        let mut core = Core::new(ApproxTextEngine, viewport);
        core.mount_root(&Root::of::<Fixture>());
        let batch = core.commit().unwrap();
        let channels = CHANNELS.with(|slot| slot.borrow_mut().take().unwrap());
        let mut harness = Self {
            core,
            channels,
            presses: BTreeMap::new(),
            text: BTreeMap::new(),
        };
        harness.apply(batch.items);
        harness.settle();
        harness
    }

    fn apply(&mut self, items: Vec<MountItem>) {
        for item in items {
            match item {
                MountItem::Events { id, events } => {
                    self.presses.insert(id, events.contains(EventKind::Press));
                }
                MountItem::Delete { id } => {
                    self.presses.remove(&id);
                    self.text.remove(&id);
                }
                MountItem::Text { id, text, .. } => {
                    self.text.insert(id, text.text.clone());
                }
                _ => {}
            }
        }
    }

    fn settle(&mut self) {
        self.core.run_component_tasks();
        for _ in 0..24 {
            let batch = self.core.commit();
            let mut events = Vec::new();
            self.core.take_events(&mut events);
            if batch.is_none() && events.is_empty() {
                return;
            }
            if let Some(batch) = batch {
                self.apply(batch.items);
            }
            for event in events {
                match event {
                    JsEvent::Event(id, kind, payload) => {
                        self.core.component_event(id, kind, &payload);
                    }
                    JsEvent::LayoutListener {
                        host,
                        id,
                        listener,
                        frame,
                    } => self.core.component_layout(host, id, &listener, frame),
                    _ => {}
                }
            }
            self.core.run_component_tasks();
        }
        panic!("menu did not settle");
    }

    fn drain(&mut self) -> Vec<Request> {
        let mut commands = Vec::new();
        for _ in 0..64 {
            self.core.run_component_tasks();
            match self.channels.requests.try_recv() {
                Ok(request) => commands.push(request),
                Err(mpsc::error::TryRecvError::Empty) => return commands,
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    panic!("menu renderer disconnected")
                }
            }
        }
        panic!("menu requests did not drain");
    }

    fn press(&mut self, id: NodeId) {
        assert!(
            self.core
                .component_event(id, EventKind::Press, &EventPayload::None)
        );
        self.core.run_component_tasks();
    }
}

fn snapshot() -> Snapshot {
    Snapshot {
        id: ProjectionId::new(),
        content: 3,
        selection: 7,
        original: "**source**".into(),
        selected: Some("source".into()),
        items: vec![
            Item {
                label: "Copy original".into(),
                enabled: true,
                command: Command::CopyOriginal,
            },
            Item {
                label: "Copy".into(),
                enabled: false,
                command: Command::CopySelection,
            },
        ],
        colors: Theme::light().colors,
        focused: None,
        hovered: None,
        pressed: None,
        language: "en",
        appearance: Theme::light().appearance,
        width: WIDTH,
        height: ROW * 2. + PAD * 2. + 2.,
    }
}

#[test]
fn production_menu_reports_actual_rows_and_dispatches_only_enabled_commands() {
    let initial = snapshot();
    let mut harness = Harness::mount(initial.clone());
    let commands = harness.drain();
    let layouts: Vec<_> = commands
        .iter()
        .map(|command| match command {
            Request::Layout(observed, root, rows) => {
                assert!(observed.matches(&initial));
                (root, rows)
            }
            Request::Activate(_, _) => panic!("layout activated a command"),
        })
        .collect();
    assert!(!layouts.is_empty());
    let (root, rows) = layouts.last().unwrap();
    assert!(valid_layout(&initial, root, rows));
    assert_eq!(root[2], WIDTH);
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|row| row[3] == ROW));
    assert_eq!(
        harness
            .text
            .values()
            .filter(|text| text.as_str() == "Copy original")
            .count(),
        1
    );
    assert_eq!(
        harness
            .text
            .values()
            .filter(|text| text.as_str() == "Copy")
            .count(),
        1
    );
    let controls: Vec<_> = harness
        .presses
        .iter()
        .filter_map(|(id, press)| press.then_some(*id))
        .collect();
    assert_eq!(controls.len(), 2);
    let mut dispatched = Vec::new();
    for control in controls {
        harness.press(control);
        dispatched.extend(harness.drain());
    }
    let activated: Vec<_> = dispatched
        .into_iter()
        .filter_map(|command| match command {
            Request::Activate(observed, index) => {
                assert!(observed.matches(&initial));
                Some(index)
            }
            Request::Layout(_, _, _) => None,
        })
        .collect();
    assert_eq!(activated, vec![0]);
}

#[test]
fn production_menu_updates_labels_and_rejects_old_projection_identity() {
    let original = snapshot();
    let mut harness = Harness::mount(original.clone());
    harness.drain();
    let mut next = original.clone();
    next.id = ProjectionId::new();
    next.items[0].label = "复制原文".into();
    next.language = "zh-CN";
    next.colors = Theme::dark().colors;
    next.appearance = Theme::dark().appearance;
    harness.channels.updates.try_send(next.clone()).unwrap();
    harness.settle();
    assert!(!next.matches(&original));
    assert_eq!(
        harness
            .text
            .values()
            .filter(|text| text.as_str() == "复制原文")
            .count(),
        1
    );
    for request in harness.drain() {
        if let Request::Layout(observed, root, rows) = request {
            assert!(observed.matches(&next));
            assert!(!observed.matches(&original));
            assert!(valid_layout(&observed, &root, &rows));
        }
    }
}

#[test]
fn selection_copy_retains_projected_source_and_revokes_replaced_content() {
    let copy = MessageCopy::new("**first** and second");
    let generation = copy.selection_generation();
    copy.set_selected_text(generation, "first".into());
    copy.set_projected_selection(Some(0..5));
    copy.publish_retained(
        8,
        std::sync::Arc::from("prefix first and second"),
        |range| Some(range.start + 7..range.end + 7),
    );
    assert_eq!(copy.selected_text_snapshot().as_deref(), Some("first"));
    assert_eq!(copy.local_selection_range(), Some(7..12));
    assert_eq!(copy.original_text(), "**first** and second");
    copy.publish_retained(9, std::sync::Arc::from("unrelated"), |_| None);
    assert_eq!(copy.selected_text_snapshot(), None);
    copy.set_selected_text(generation, "stale".into());
    assert_eq!(copy.selected_text_snapshot(), None);
}

#[test]
fn snapshot_rejects_changed_selection_and_retained_source_without_generation_change() {
    let before = snapshot();
    let mut selection = before.clone();
    selection.selected = Some("different".into());
    assert!(!before.matches(&selection));
    let mut source = before.clone();
    source.original.push_str(" streamed tail");
    assert!(!before.matches(&source));
    let mut recycled = before.clone();
    recycled.id = ProjectionId::new();
    assert!(!before.matches(&recycled));
}

#[test]
fn menu_layout_refuses_missing_nonfinite_or_clipped_rows() {
    let observed = snapshot();
    let root = [0., 0., WIDTH, observed.height];
    let rows = [
        [PAD, PAD, WIDTH - PAD * 2., ROW],
        [PAD, PAD + ROW, WIDTH - PAD * 2., ROW],
    ];
    assert!(valid_layout(&observed, &root, &rows));
    assert!(!valid_layout(&observed, &root, &rows[..1]));
    let mut invalid = rows;
    invalid[0][0] = f32::NAN;
    assert!(!valid_layout(&observed, &root, &invalid));
    let mut clipped = rows;
    clipped[1][1] += ROW;
    assert!(!valid_layout(&observed, &root, &clipped));
}

#[test]
fn menu_preserves_fork_pending_and_both_compaction_direction_descriptions() {
    let copy = MessageCopy::new("source");
    *copy.fork_menu.borrow_mut() = Some(ForkCommand {
        request: ForkRequested {
            thread_id: "thread".into(),
            message_id: "message".into(),
        },
        pending: true,
        enabled: false,
    });
    *copy.compaction_menu.borrow_mut() = [
        vega_conversation::types::PartialCompactionDirection::UpTo,
        vega_conversation::types::PartialCompactionDirection::From,
    ]
    .into_iter()
    .map(|direction| CompactCommand {
        thread: "thread".into(),
        model: "model".into(),
        source: copy.id,
        selection: PartialCompactionSelection {
            message_id: "message".into(),
            source_fingerprint: "fingerprint".into(),
            direction,
        },
    })
    .collect();
    let rows = items(&copy, true);
    assert_eq!(rows.len(), 5);
    assert!(
        matches!(&rows[2].command, Command::Fork(command) if command.pending && !command.enabled)
    );
    assert!(!rows[2].enabled);
    for (row, direction) in rows[3..].iter().zip([
        vega_conversation::types::PartialCompactionDirection::UpTo,
        vega_conversation::types::PartialCompactionDirection::From,
    ]) {
        assert!(
            matches!(&row.command, Command::Compact(command) if command.selection.direction == direction && command.source == copy.id && command.selection.source_fingerprint == "fingerprint")
        );
    }
}
