use super::*;
use gpui_kit::{
    CursorStyle, EntityInputHandler, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, Pixels,
    Point, VisualTestContext, point,
};

use super::issue147_markdown_selection::{
    assistant_entry, drag, layout_for, open_selection_harness, text_point,
};

struct CursorCapture;

impl CursorCapture {
    fn start() -> Self {
        selection::CURSOR_REQUESTS.with_borrow_mut(|requests| *requests = Some(Vec::new()));
        links::LINK_LAYOUTS.with_borrow_mut(|layouts| *layouts = Some(Vec::new()));
        selection::SELECTION_RUN_LAYOUTS.with_borrow_mut(Vec::clear);
        Self
    }

    fn clear(&self) {
        selection::CURSOR_REQUESTS.with_borrow_mut(|requests| {
            requests.as_mut().expect("active cursor capture").clear();
        });
    }

    fn requests(&self) -> Vec<selection::SelectionCursorRequest> {
        selection::CURSOR_REQUESTS
            .with_borrow(|requests| requests.as_ref().expect("active cursor capture").clone())
    }
}

impl Drop for CursorCapture {
    fn drop(&mut self) {
        selection::CURSOR_REQUESTS.with_borrow_mut(|requests| *requests = None);
        links::LINK_LAYOUTS.with_borrow_mut(|layouts| *layouts = None);
    }
}

fn prepare_pointer_window(visual: &mut VisualTestContext) {
    visual.update(|window, cx| {
        gpui_kit::init(cx);
        window.activate_window();
        window.refresh();
    });
    visual.run_until_parked();
    assert!(visual.update(|window, _| window.is_window_active()));
}

fn open_active_selection_harness(
    cx: &mut TestAppContext,
    entry: StreamEntry,
) -> (Entity<ConversationStream>, VisualTestContext) {
    let (stream, mut visual) = open_selection_harness(cx, entry);
    prepare_pointer_window(&mut visual);
    (stream, visual)
}

fn hover(visual: &mut VisualTestContext, capture: &CursorCapture, position: Point<Pixels>) {
    capture.clear();
    visual.simulate_mouse_move(position, None, Modifiers::default());
    visual.update(|window, cx| {
        window.refresh();
        let _ = window.draw(cx);
    });
    visual.run_until_parked();
}

fn multi_click(visual: &mut VisualTestContext, position: Point<Pixels>, count: usize) {
    visual.simulate_event(MouseDownEvent {
        position,
        button: MouseButton::Left,
        click_count: count,
        modifiers: Modifiers::default(),
        first_mouse: false,
    });
    visual.simulate_event(MouseUpEvent {
        position,
        button: MouseButton::Left,
        click_count: count,
        modifiers: Modifiers::default(),
    });
}

fn composer_point(visual: &mut VisualTestContext) -> Point<Pixels> {
    let shell = visual
        .debug_bounds("composer-shell")
        .expect("production composer");
    point(shell.left() + px(18.), shell.top() + px(21.))
}

#[gpui_kit::test]
fn issue432_plain_markdown_containers_request_ibeam_on_their_own_text(cx: &mut TestAppContext) {
    let capture = CursorCapture::start();
    let source =
        "## Heading\n\nParagraph plain\n\n- List\n\n> Quote\n\n| Header |\n|---|\n| Cell |";
    let (_, mut visual) = open_active_selection_harness(cx, assistant_entry(source));
    for text in [
        "Heading",
        "Paragraph plain",
        "List",
        "Quote",
        "Header",
        "Cell",
    ] {
        let layout = layout_for(text);
        let position = text_point(&layout, 0, false);
        hover(&mut visual, &capture, position);
        let requests = capture.requests();
        assert!(
            requests.iter().any(|request| {
                request.text == text
                    && request.style == CursorStyle::IBeam
                    && request.hovered
                    && request.bounds.contains(&position)
            }),
            "missing production IBeam request for {text}: {requests:?}"
        );
        assert!(
            !requests
                .iter()
                .any(|request| request.style == CursorStyle::PointingHand)
        );
    }
    let button = visual
        .debug_bounds("composer-add")
        .expect("composer button");
    hover(&mut visual, &capture, button.center());
    assert!(capture.requests().iter().all(|request| !request.hovered));
}

#[gpui_kit::test]
fn issue432_user_message_requests_ibeam_for_selectable_text(cx: &mut TestAppContext) {
    let capture = CursorCapture::start();
    let text = "User plain text";
    let (_, mut visual) = open_active_selection_harness(
        cx,
        StreamEntry::User {
            copy: MessageCopy::new(text),
            lines: user_message_lines(432, text),
        },
    );
    let layout = layout_for(text);
    hover(&mut visual, &capture, text_point(&layout, 2, false));
    assert!(capture.requests().iter().any(|request| {
        request.text == text && request.style == CursorStyle::IBeam && request.hovered
    }));
}

#[gpui_kit::test]
fn issue432_link_hand_request_follows_ibeam_on_the_same_text_hitbox(cx: &mut TestAppContext) {
    let capture = CursorCapture::start();
    let (_, mut visual) = open_active_selection_harness(
        cx,
        assistant_entry("plain [Alpha](https://cursor.example) suffix"),
    );
    let text = "plain Alpha suffix";
    let layout = layout_for(text);
    let before = (layout.bounds(), layout.position_for_index(11));
    let position = links::LINK_LAYOUTS.with_borrow(|layouts| {
        layouts
            .as_ref()
            .unwrap()
            .iter()
            .rev()
            .find(|text| {
                text.links
                    .iter()
                    .any(|(_, url)| url == "https://cursor.example")
            })
            .expect("production link region")
            .regions[0]
            .bounds
            .center()
    });
    hover(&mut visual, &capture, position);
    let requests = capture.requests();
    let hand_index = requests
        .iter()
        .rposition(|request| request.style == CursorStyle::PointingHand && request.hovered)
        .expect("production link hand request");
    let hand = &requests[hand_index];
    let text_index = requests[..hand_index]
        .iter()
        .rposition(|request| {
            request.style == CursorStyle::IBeam && request.hitbox == hand.hitbox && request.hovered
        })
        .expect("IBeam must be requested before hand on the shared hitbox");
    assert_eq!(requests[text_index].text, text);
    assert_eq!(hand.text, text);
    assert_eq!(requests[text_index].bounds, hand.bounds);
    assert!(
        requests[hand_index + 1..]
            .iter()
            .all(|request| request.hitbox != hand.hitbox)
    );
    let after = layout_for(text);
    assert_eq!(before, (after.bounds(), after.position_for_index(11)));
    let plain = text_point(&after, 0, false);
    hover(&mut visual, &capture, plain);
    assert!(capture.requests().iter().any(|request| {
        request.text == text && request.style == CursorStyle::IBeam && request.hovered
    }));
    assert!(
        capture
            .requests()
            .iter()
            .all(|request| request.style != CursorStyle::PointingHand)
    );
    assert_eq!(visual.opened_url(), None);
}

#[gpui_kit::test]
async fn issue432_expanded_thinking_requests_ibeam_for_plain_text(cx: &mut TestAppContext) {
    let capture = CursorCapture::start();
    let (window, stream, _) = open_controller_stream(cx, "issue432-thinking");
    let source = "Thinking plain body";
    let card = stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "issue432-thinking".into(),
                seq: 2,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ThinkingDelta {
                message_id: "issue432-thinking".into(),
                delta: source.into(),
            },
            cx,
        );
        let card = stream.active_thinking.clone().expect("thinking card");
        stream.run_activity_groups["issue432-thinking"]
            .update(cx, |group, cx| group.reveal_segment(0, cx));
        card
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    prepare_pointer_window(&mut visual);
    assert!(card.read_with(&visual, |card, _| card.test_body_fragment_count()) > 0);
    let layout = layout_for(source);
    hover(&mut visual, &capture, text_point(&layout, 2, false));
    assert!(capture.requests().iter().any(|request| {
        request.text == source && request.style == CursorStyle::IBeam && request.hovered
    }));
    assert_eq!(
        card.read_with(&visual, |card, _| card.test_source_copy().original_text()),
        source
    );
}

#[gpui_kit::test]
fn issue432_composer_owns_selection_and_focus_alongside_window_text_selection(
    cx: &mut TestAppContext,
) {
    let (stream, mut visual) =
        open_active_selection_harness(cx, assistant_entry("Body alpha beta"));
    let input = stream.read_with(&visual, |stream, _| stream.input.clone());
    input.update(&mut visual, |input, cx| input.set_text("ComposerDraft", cx));
    visual.run_until_parked();
    let layout = layout_for("Body alpha beta");
    drag(
        &mut visual,
        text_point(&layout, 5, false),
        text_point(&layout, 10, true),
    );
    assert_eq!(
        visual.update(gpui_kit::base::TextSelection::selected_text),
        "alpha"
    );
    visual.simulate_keystrokes("cmd-c");
    assert_eq!(
        visual
            .read_from_clipboard()
            .and_then(|item| item.text())
            .as_deref(),
        Some("alpha")
    );
    let position = composer_point(&mut visual);
    multi_click(&mut visual, position, 2);
    visual.update(|window, cx| {
        assert!(input.read(cx).focus_handle(cx).is_focused(window));
        let range = input.update(cx, |input, cx| input.selected_text_range(false, window, cx));
        assert_eq!(range.expect("input selection").range, 0..13);
        assert!(gpui_kit::base::TextSelection::selected_text(window, cx).is_empty());
    });
    visual.simulate_mouse_move(text_point(&layout, 0, false), None, Modifiers::default());
    visual.simulate_keystrokes("cmd-c");
    assert_eq!(
        visual
            .read_from_clipboard()
            .and_then(|item| item.text())
            .as_deref(),
        Some("ComposerDraft")
    );
    visual.simulate_keystrokes("x");
    assert_eq!(
        input.read_with(&visual, |input, _| input.text().to_owned()),
        "x"
    );
    let layout = layout_for("Body alpha beta");
    drag(
        &mut visual,
        text_point(&layout, 11, false),
        text_point(&layout, 15, true),
    );
    visual.simulate_keystrokes("cmd-c");
    assert_eq!(
        visual
            .read_from_clipboard()
            .and_then(|item| item.text())
            .as_deref(),
        Some("beta")
    );
    assert_eq!(
        input.read_with(&visual, |input, _| input.text().to_owned()),
        "x"
    );
}

#[gpui_kit::test]
fn issue432_mouse_selection_preserves_ime_and_single_flight_submit_guards(cx: &mut TestAppContext) {
    for key in ["enter", "cmd-enter"] {
        let (stream, mut visual) = open_active_selection_harness(cx, assistant_entry("Body"));
        let submitted = Arc::new(Mutex::new(Vec::<ComposerSubmitted>::new()));
        let captured = submitted.clone();
        visual.update(|_, cx| {
            cx.subscribe(&stream, move |_, event: &ComposerSubmitted, _| {
                captured.lock().expect("submit capture").push(event.clone());
            })
            .detach();
        });
        let input = stream.read_with(&visual, |stream, _| stream.input.clone());
        input.update(&mut visual, |input, cx| input.set_text("draft", cx));
        visual.run_until_parked();
        let position = composer_point(&mut visual);
        multi_click(&mut visual, position, 2);
        visual.update(|window, cx| {
            input.update(cx, |input, cx| {
                input.replace_and_mark_text_in_range(None, "ni", Some(2..2), window, cx);
            });
            stream.update(cx, |stream, cx| {
                stream.on_send_action(&SendMessage, window, cx)
            });
        });
        assert!(submitted.lock().expect("submit capture").is_empty());
        assert!(!stream.read_with(&visual, |stream, _| stream.composer_submit_pending));
        assert!(input.read_with(&visual, |input, _| input.is_composing()));
        visual.update(|window, cx| input.update(cx, |input, cx| input.unmark_text(window, cx)));
        visual.run_until_parked();
        visual.simulate_keystrokes(key);
        visual.simulate_keystrokes(key);
        let events = submitted.lock().expect("submit capture");
        assert_eq!(events.len(), 1, "{key}");
        assert_eq!(events[0].content, "ni");
        assert!(stream.read_with(&visual, |stream, _| stream.composer_submit_pending));
    }
}
