use super::{Request, Snapshot, render};
use crate::nsh::ProjectionId;
use nsh_core::element::{Component, NoEvent, Root, Scope};
use nsh_core::prelude as ns;
use nsh_core::{ApproxTextEngine, Core, MountBatch, MountItem};
use std::cell::RefCell;
use tokio::sync::mpsc;

thread_local! {
    static INITIAL: RefCell<Option<Snapshot>> = const { RefCell::new(None) };
    static REQUESTS: RefCell<Option<mpsc::Receiver<Request>>> = const { RefCell::new(None) };
}

struct GridFixture;

impl Component for GridFixture {
    type Props = ();
    type Event = NoEvent;
    fn view(_: (), _: Scope<NoEvent>) -> impl ns::IntoElement {
        let snapshot = INITIAL.with(|slot| slot.borrow_mut().take().expect("grid snapshot"));
        let (sender, requests) = mpsc::channel(crate::nsh::COMMAND_CAPACITY);
        REQUESTS.with(|slot| *slot.borrow_mut() = Some(requests));
        render(ns::signal_with(snapshot, ns::SignalEquals::Never), sender)
    }
}

fn snapshot(history: bool) -> Snapshot {
    Snapshot {
        id: ProjectionId::new(),
        items: vec![11, 17, 23, 29],
        history,
        pending: false,
        error: None,
        recovery: false,
        focused: false,
        language: vega_i18n::language(),
        colors: vega_theme::LIGHT,
    }
}

fn mount(snapshot: Snapshot, width: f32) -> (Core<ApproxTextEngine>, MountBatch) {
    INITIAL.with(|slot| *slot.borrow_mut() = Some(snapshot));
    let mut core = Core::new(ApproxTextEngine, ns::size(ns::px(width), ns::px(600.)));
    core.mount_root(&Root::of::<GridFixture>());
    let batch = core.commit().expect("production attachment grid");
    (core, batch)
}

#[test]
fn history_native_thumbnail_slots_wrap_right_with_original_top_inset() {
    let (_, batch) = mount(snapshot(true), 180.);
    let slots = batch
        .items
        .iter()
        .filter_map(|item| match item {
            MountItem::Component { id, name } if name.starts_with("attachment-thumbnail-") => {
                Some(*id)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(slots.len(), 4);
    assert!(batch.items.iter().all(|item| !matches!(item,
        MountItem::Component { name, .. } if name.starts_with("attachment-remove-"))));
    let frames = slots
        .iter()
        .map(|slot| {
            let mut frame = batch
                .items
                .iter()
                .find_map(|item| match item {
                    MountItem::Frame { id, frame } if id == slot => Some(*frame),
                    _ => None,
                })
                .expect("thumbnail frame");
            let mut current = *slot;
            while let Some(parent) = batch.items.iter().rev().find_map(|item| match item {
                MountItem::Insert { parent, child, .. } if *child == current => Some(*parent),
                _ => None,
            }) {
                if parent == nsh_core::ROOT_ID {
                    break;
                }
                let origin = batch
                    .items
                    .iter()
                    .find_map(|item| match item {
                        MountItem::Frame { id, frame } if *id == parent => Some(frame.origin),
                        _ => None,
                    })
                    .expect("ancestor frame");
                frame.origin.x.0 += origin.x.0;
                frame.origin.y.0 += origin.y.0;
                current = parent;
            }
            frame
        })
        .collect::<Vec<_>>();
    let expected = [(28., 24.), (108., 24.), (28., 104.), (108., 104.)];
    for (frame, (x, y)) in frames.iter().zip(expected) {
        assert_eq!((frame.origin.x.0, frame.origin.y.0), (x, y));
        assert_eq!((frame.size.width.0, frame.size.height.0), (72., 72.));
    }
}

#[test]
fn composer_mount_preserves_pending_error_and_remove_slots() {
    let mut shown = snapshot(false);
    shown.pending = true;
    shown.error = Some("ui.could_not_add_images_choose_valid_png_jpeg");
    let (mut core, batch) = mount(shown.clone(), 180.);
    let text = batch
        .items
        .iter()
        .filter_map(|item| match item {
            MountItem::Text { text, .. } => Some(text.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        text,
        [
            vega_i18n::t("ui.adding_images"),
            vega_i18n::t(shown.error.expect("error key"))
        ]
    );
    let removals = batch
        .items
        .iter()
        .filter(|item| {
            matches!(item,
        MountItem::Component { name, .. } if name.starts_with("attachment-remove-"))
        })
        .count();
    assert_eq!(removals, 4);
    core.run_component_tasks();
    let mut requests = REQUESTS.with(|slot| slot.borrow_mut().take().expect("grid requests"));
    while let Ok(request) = requests.try_recv() {
        assert!(
            matches!(request, Request::Layout { .. }),
            "mount must not replay a business action"
        );
    }
}
