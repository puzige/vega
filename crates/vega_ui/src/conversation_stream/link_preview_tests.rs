use std::{cell::RefCell, rc::Rc, time::Duration};

use gpui_kit::{
    AppContext, Context, Entity, Focusable, InputEvent, IntoElement, ListOffset, ListState,
    MouseMoveEvent, Pixels, Point, Render, Role, ScrollDelta, ScrollWheelEvent, TestAppContext,
    Window, WindowHandle, div, point, prelude::*, px, size, test::TestWindowExt,
};
use vega_conversation::{
    history::{AssistantStatus, HistoryEntry, HistoryPage},
    link_preview::test_support::{ManualClock, ScriptedNetwork, thumbnail_png},
    types::{
        ConversationEvent, PaletteFilePreview, PermissionMode, TaskBackend, Thread, ThreadMode,
        ThreadStatus,
    },
};
use vega_theme::Theme;

use super::{ConversationStream, StreamEntry};
use crate::{
    file_preview::FilePreview,
    link_preview::{
        MarkdownLinkPreviewLayer,
        test_support::{LinkObservation, install_dependencies, visible_links},
    },
};

const TARGET: &str = "https://example.org/document?keep=1#section";
const REQUEST: &str = "https://example.org/document?keep=1";
const NAKED: &str = "https://example.net/reference#part";
const TABLE: &str = "https://example.com/table";
const IMAGE: &str = "https://example.org/thumbnail.png";
const PAGE: &[u8] =
    b"<head><title>Preview target</title><meta name=description content='Preview summary'></head>";

#[derive(Clone)]
enum Pane {
    Conversation(Entity<ConversationStream>),
    File(Entity<FilePreview>),
}

enum Content {
    Conversation(Vec<HistoryEntry>),
    File(String),
}

struct Host {
    pane: Pane,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let pane = match &self.pane {
            Pane::Conversation(stream) => stream.clone().into_any_element(),
            Pane::File(file) => file.clone().into_any_element(),
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(gpui_kit::base::TextSelectionLayer)
            .child(pane)
            .child(MarkdownLinkPreviewLayer)
    }
}

struct Harness {
    window: WindowHandle<Host>,
    pane: Pane,
    network: ScriptedNetwork,
    clock: ManualClock,
    opened: Rc<RefCell<Vec<String>>>,
}

fn thread() -> Thread {
    Thread {
        parent_thread_id: None,
        id: "fixture-conversation".into(),
        backend: TaskBackend::Native,
        project_id: String::new(),
        title: "Reading fixture".into(),
        mode: ThreadMode::Ask,
        permission_mode: PermissionMode::ReadOnly,
        model: "fixture-model".into(),
        status: ThreadStatus::Active,
        pinned: false,
        unread: false,
        created_at: 0,
        updated_at: 0,
    }
}

fn assistant(seq: i64, content: impl Into<String>) -> HistoryEntry {
    HistoryEntry::AssistantText {
        seq,
        message_id: format!("assistant-{seq}"),
        content: content.into(),
        status: AssistantStatus::Done,
        execution_duration_ms: None,
    }
}

fn page(entries: Vec<HistoryEntry>) -> HistoryPage {
    HistoryPage {
        subagents: Vec::new(),
        entries,
        older_cursor: None,
        newer_cursor: None,
        newest_seq: None,
    }
}

fn harness(cx: &mut TestAppContext, content: Content) -> Harness {
    let network = ScriptedNetwork::default();
    let clock = ManualClock::default();
    let opened = Rc::new(RefCell::new(Vec::new()));
    let recorded = opened.clone();
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_global(Theme::light());
        crate::init(cx);
        install_dependencies(
            network.service(&clock),
            Rc::new(move |target| recorded.borrow_mut().push(target.into())),
            cx,
        );
    });
    let window = cx.open_window(size(px(760.), px(720.)), |_, cx| {
        let pane = match content {
            Content::Conversation(entries) => Pane::Conversation(cx.new(|cx| {
                let mut stream = ConversationStream::new(thread(), cx);
                stream.apply_history_page(page(entries), cx);
                stream
            })),
            Content::File(content) => Pane::File(cx.new(|cx| {
                FilePreview::new(
                    PaletteFilePreview {
                        relative_path: "fixture.md".into(),
                        content,
                    },
                    cx,
                )
            })),
        };
        Host { pane }
    });
    let pane = window.read_with(cx, |host, _| host.pane.clone()).unwrap();
    let focus = cx.update(|cx| match &pane {
        Pane::Conversation(stream) => stream.read(cx).input.read(cx).focus_handle(cx),
        Pane::File(file) => file.read(cx).focus_handle(cx),
    });
    cx.update_window(window.into(), |_, window, cx| {
        window.activate_window();
        window.focus(&focus, cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    Harness {
        window,
        pane,
        network,
        clock,
        opened,
    }
}

fn conversation(test: &Harness) -> Entity<ConversationStream> {
    match &test.pane {
        Pane::Conversation(stream) => stream.clone(),
        Pane::File(_) => panic!("expected conversation fixture"),
    }
}

fn list(cx: &mut TestAppContext, test: &Harness) -> ListState {
    conversation(test).update(cx, |stream, _| stream.list.clone())
}

fn frame(cx: &mut TestAppContext, test: &Harness) {
    cx.update_window(test.window.into(), |_, window, cx| window.render_frame(cx))
        .unwrap();
    cx.run_until_parked();
}

fn links(cx: &mut TestAppContext, test: &Harness) -> Vec<LinkObservation> {
    frame(cx, test);
    cx.update_window(test.window.into(), |_, window, cx| {
        visible_links(window, cx)
    })
    .unwrap()
}

fn card_visible(cx: &mut TestAppContext, test: &Harness) -> bool {
    cx.update_window(test.window.into(), |_, window, _| {
        window.try_find("markdown-link-preview").is_some()
    })
    .unwrap()
}

fn tick(cx: &mut TestAppContext, test: &Harness, millis: u64) {
    let duration = Duration::from_millis(millis);
    test.clock.advance(duration);
    cx.executor().advance_clock(duration);
    cx.run_until_parked();
    frame(cx, test);
}

fn pointer(cx: &mut TestAppContext, test: &Harness, position: Point<Pixels>) {
    cx.update_window(test.window.into(), |_, window, cx| {
        window.dispatch_event(
            MouseMoveEvent {
                position,
                pressed_button: None,
                modifiers: Default::default(),
            }
            .to_platform_input(),
            cx,
        );
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
}

fn wheel(cx: &mut TestAppContext, test: &Harness, position: Point<Pixels>) {
    cx.update_window(test.window.into(), |_, window, cx| {
        assert_eq!(window.mouse_position(), position);
        window.dispatch_event(
            ScrollWheelEvent {
                position,
                delta: ScrollDelta::Pixels(point(px(0.), px(-96.))),
                ..Default::default()
            }
            .to_platform_input(),
            cx,
        );
        window.render_frame(cx);
        assert_eq!(window.mouse_position(), position);
    })
    .unwrap();
    cx.run_until_parked();
}

fn scroll_signature(list: &ListState) -> (usize, Pixels) {
    let top = list.logical_scroll_top();
    (top.item_ix, top.offset_in_item)
}

fn assert_no_effects(cx: &mut TestAppContext, test: &Harness) {
    assert!(!card_visible(cx, test));
    assert!(test.network.requests().is_empty());
    assert_eq!(test.network.resolutions(), 0);
    assert!(test.opened.borrow().is_empty());
}

#[gpui_kit::test]
fn conversation_first_and_repeated_frames_register_links_without_preview_requests(
    cx: &mut TestAppContext,
) {
    let test = harness(
        cx,
        Content::Conversation(vec![
            HistoryEntry::UserText {
                seq: 1,
                message_id: "user-reference".into(),
                content: format!("Reference {NAKED}"),
            },
            assistant(2, format!("[Assistant target]({TARGET})")),
        ]),
    );
    let list = list(cx, &test);
    assert_eq!(list.item_count(), 2);
    assert!(list.viewport_bounds().size.height > px(0.));
    for _ in 0..4 {
        let visible = links(cx, &test);
        assert_eq!(visible.len(), 2);
        assert!(visible.iter().any(|link| link.target == NAKED));
        assert!(
            visible
                .iter()
                .any(|link| link.target == TARGET && link.label == "Assistant target")
        );
        for link in &visible {
            assert_eq!(link.role, Role::Link);
            assert_eq!(link.description, link.target);
            assert!(!link.text_bounds.is_empty());
            assert_eq!(link.icon_bounds.len(), 1);
            assert!(
                link.text_bounds
                    .iter()
                    .all(|bounds| { bounds.size.width > px(0.) && bounds.size.height > px(0.) })
            );
        }
        assert_no_effects(cx, &test);
    }
}

#[gpui_kit::test]
fn virtualized_wrapped_and_table_links_register_when_revealed_and_reentered(
    cx: &mut TestAppContext,
) {
    let mut history = vec![assistant(
        1,
        format!(
            "[{}]({TARGET})\n\n| Reference |\n| --- |\n| [Table target]({TABLE}) |",
            "wrapped link ".repeat(45)
        ),
    )];
    history.extend((2..82).map(|seq| assistant(seq, format!("Historical paragraph {seq}."))));
    let test = harness(cx, Content::Conversation(history));
    let list = list(cx, &test);
    assert_eq!(list.item_count(), 81);
    assert!(links(cx, &test).is_empty());
    assert_no_effects(cx, &test);
    for _ in 0..2 {
        list.scroll_to(ListOffset {
            item_ix: 0,
            offset_in_item: px(0.),
        });
        let visible = links(cx, &test);
        let wrapped = visible.iter().find(|link| link.target == TARGET).unwrap();
        assert!(wrapped.text_bounds.len() > 1);
        assert!(visible.iter().any(|link| link.target == TABLE));
        assert_no_effects(cx, &test);
        list.scroll_to_end();
        assert!(links(cx, &test).is_empty());
        assert_no_effects(cx, &test);
    }
}

#[gpui_kit::test]
fn list_wheel_and_programmatic_scroll_close_loaded_and_pending_previews_without_reopening(
    cx: &mut TestAppContext,
) {
    for use_wheel in [true, false] {
        for pending in [true, false] {
            let test = harness(
                cx,
                Content::Conversation(vec![assistant(
                    1,
                    format!(
                        "[Target]({TARGET}){}",
                        "\n\nA paragraph keeps the real list scrollable.".repeat(40)
                    ),
                )]),
            );
            let list = list(cx, &test);
            list.scroll_to(ListOffset {
                item_ix: 0,
                offset_in_item: px(0.),
            });
            let link = links(cx, &test).remove(0);
            let position = link.text_bounds[0].center();
            let mut gate = Some(
                test.network
                    .enqueue(REQUEST, "text/html; charset=utf-8", PAGE),
            );
            if !pending {
                gate.take().unwrap().release();
            }
            pointer(cx, &test, position);
            tick(cx, &test, 399);
            assert!(!card_visible(cx, &test));
            assert!(test.network.requests().is_empty());
            tick(cx, &test, 1);
            assert!(card_visible(cx, &test));
            assert_eq!(test.network.requests(), [REQUEST]);
            cx.update_window(test.window.into(), |_, window, cx| {
                assert!(window.find("link-preview-open").visible());
                assert_eq!(window.try_find("link-preview-title").is_none(), pending);
                window.focus(&link.focus, cx);
                window.render_frame(cx);
            })
            .unwrap();
            cx.simulate_keystrokes(test.window.into(), "enter");
            assert_eq!(*test.opened.borrow(), [TARGET]);
            let before = scroll_signature(&list);
            if use_wheel {
                wheel(cx, &test, position);
            } else {
                list.scroll_by(px(96.));
                frame(cx, &test);
            }
            assert_ne!(scroll_signature(&list), before);
            assert!(!card_visible(cx, &test));
            cx.update_window(test.window.into(), |_, window, _| {
                assert_eq!(window.mouse_position(), position)
            })
            .unwrap();
            if pending {
                assert!(test.network.request_cancelled(0));
            }
            if let Some(gate) = gate {
                gate.release();
            }
            tick(cx, &test, 1000);
            assert!(!card_visible(cx, &test));
            assert_eq!(test.network.requests(), [REQUEST]);
            assert_eq!(*test.opened.borrow(), [TARGET]);
        }
    }
}

#[gpui_kit::test]
fn unchanged_list_scroll_preserves_preview_during_redraw_and_neighbor_stream_materialization(
    cx: &mut TestAppContext,
) {
    let test = harness(
        cx,
        Content::Conversation(vec![assistant(
            1,
            format!(
                "[Target]({TARGET}){}",
                "\n\nStable reading paragraph.".repeat(10)
            ),
        )]),
    );
    let stream = conversation(&test);
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "neighbor-stream".into(),
                seq: 2,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "neighbor-stream".into(),
                delta: "Neighbor begins.\n\n".into(),
            },
            cx,
        );
    });
    let list = list(cx, &test);
    list.scroll_to(ListOffset {
        item_ix: 0,
        offset_in_item: px(0.),
    });
    let link = links(cx, &test).remove(0);
    let signature = scroll_signature(&list);
    test.network
        .enqueue(REQUEST, "text/html; charset=utf-8", PAGE)
        .release();
    pointer(cx, &test, link.text_bounds[0].center());
    tick(cx, &test, 400);
    assert!(card_visible(cx, &test));
    for index in 0..3 {
        frame(cx, &test);
        assert!(card_visible(cx, &test));
        stream.update(cx, |stream, cx| {
            stream.apply_event(
                ConversationEvent::TextDelta {
                    message_id: "neighbor-stream".into(),
                    delta: format!("Neighbor update {index}.\n\n"),
                },
                cx,
            );
        });
        frame(cx, &test);
        let materialized = stream.update(cx, |stream, _| {
            let (_, index) = stream.active_agent_message.as_ref().unwrap();
            let StreamEntry::Assistant { model, .. } = &stream.entries[*index] else {
                panic!("expected materialized assistant neighbor");
            };
            model
                .committed_lines
                .iter()
                .chain(&model.pending_lines)
                .flat_map(|line| line.spans.iter().map(|span| span.text.as_str()))
                .collect::<String>()
        });
        assert!(materialized.contains(&format!("Neighbor update {index}.")));
        assert_eq!(scroll_signature(&list), signature);
        let current = links(cx, &test);
        assert!(current.iter().any(|current| current.key == link.key));
        assert!(card_visible(cx, &test));
        cx.update_window(test.window.into(), |_, window, _| {
            assert_eq!(
                window.find("link-preview-title").label(),
                Some("Preview target")
            )
        })
        .unwrap();
        tick(cx, &test, 500);
        assert!(card_visible(cx, &test));
        assert_eq!(test.network.requests(), [REQUEST]);
    }
    assert!(test.opened.borrow().is_empty());
}

#[gpui_kit::test]
fn file_reading_keeps_delay_thumbnail_selection_keyboard_open_and_scroll_close(
    cx: &mut TestAppContext,
) {
    let test = harness(
        cx,
        Content::File(format!(
            "[File target]({TARGET}){}",
            "\n\nFile reading paragraph.".repeat(40)
        )),
    );
    let link = links(cx, &test).remove(0);
    assert_no_effects(cx, &test);
    test.network
        .enqueue(
            REQUEST,
            "text/html; charset=utf-8",
            b"<head><title>Preview target</title><meta property=og:image content='/thumbnail.png'></head>",
        )
        .release();
    test.network
        .enqueue(IMAGE, "image/png", &thumbnail_png())
        .release();
    cx.update_window(test.window.into(), |_, window, cx| {
        window.focus(&link.focus, cx);
        window.render_frame(cx);
    })
    .unwrap();
    tick(cx, &test, 399);
    assert!(!card_visible(cx, &test));
    assert!(test.network.requests().is_empty());
    tick(cx, &test, 1);
    assert!(card_visible(cx, &test));
    assert_eq!(test.network.requests(), [REQUEST, IMAGE]);
    cx.update_window(test.window.into(), |_, window, cx| {
        let image = window.find("link-preview-image").bounds();
        assert!(image.size.width > px(0.) && image.size.height > px(0.));
        assert!((f32::from(image.size.width) / f32::from(image.size.height) - 2.).abs() < 0.01);
        let title = window.find("link-preview-title").bounds();
        window.drag(
            title.origin + point(px(0.), title.size.height / 2.),
            point(title.right() - px(1.), title.center().y),
            cx,
        );
        assert_eq!(
            gpui_kit::base::TextSelection::selected_text(window, cx),
            "Preview target"
        );
        window.focus(&link.focus, cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.simulate_keystrokes(test.window.into(), "enter");
    assert_eq!(*test.opened.borrow(), [TARGET]);
    let position = links(cx, &test)
        .into_iter()
        .find(|current| current.target == TARGET)
        .unwrap()
        .text_bounds[0]
        .center();
    pointer(cx, &test, position);
    wheel(cx, &test, position);
    assert!(!card_visible(cx, &test));
    tick(cx, &test, 1000);
    assert!(!card_visible(cx, &test));
    assert_eq!(test.network.requests(), [REQUEST, IMAGE]);
    assert_eq!(*test.opened.borrow(), [TARGET]);
}
