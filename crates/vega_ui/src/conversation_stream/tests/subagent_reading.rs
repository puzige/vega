use super::issue147_markdown_selection::{assistant_entry, drag, layout_for, text_point};
use super::*;
use gpui_kit::{VisualTestContext, size};

struct ReadingHarness {
    parent: Entity<ConversationStream>,
    preview: Option<Entity<ConversationStream>>,
}

impl Render for ReadingHarness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .key_context("VegaWindow")
            .size_full()
            .flex()
            .child(gpui_kit::base::TextSelectionLayer)
            .child(div().w_1_2().h_full().child(self.parent.clone()))
            .children(
                self.preview
                    .clone()
                    .map(|preview| div().w_1_2().h_full().child(preview)),
            )
    }
}

#[gpui_kit::test]
fn reading_preview_mount_preserves_parent_focus_selection_and_draft(cx: &mut TestAppContext) {
    init_permission_test(cx);
    let parent = cx.new(|cx| {
        let mut stream = ConversationStream::new(permission_thread(), cx);
        stream.entries.push(assistant_entry("Parent selected text"));
        stream.list_append(0);
        stream
    });
    let root = cx.new(|_| ReadingHarness {
        parent: parent.clone(),
        preview: None,
    });
    let window_root = root.clone();
    let window = cx
        .update(|cx| cx.open_window(Default::default(), move |_, _| window_root))
        .unwrap();
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.simulate_resize(size(px(1400.), px(900.)));
    visual.run_until_parked();
    let input = parent.read_with(&visual, |stream, _| stream.input.clone());
    input.update(&mut visual, |input, cx| input.set_text("Parent draft", cx));
    let focus = input.read_with(&visual, |input, cx| input.focus_handle(cx));
    visual.update(|window, cx| window.focus(&focus, cx));
    let child = visual.new(|cx| {
        let mut thread = permission_thread();
        thread.id = "preview-child".into();
        let mut stream = ConversationStream::new(thread, cx);
        stream.set_reading_preview(true, cx);
        stream.actions.restore_focus = true;
        stream
    });
    root.update(&mut visual, |root, cx| {
        root.preview = Some(child.clone());
        cx.notify();
    });
    visual.run_until_parked();
    assert!(visual.update(|window, _| focus.is_focused(window)));
    assert_eq!(
        input.read_with(&visual, |input, _| input.text().to_string()),
        "Parent draft"
    );
    child.read_with(&visual, |stream, _| {
        assert!(!stream.selection_view_initialized);
        assert!(stream.actions.restore_focus);
    });
    root.update(&mut visual, |root, cx| {
        root.preview = None;
        cx.notify();
    });
    visual.run_until_parked();
    let layout = layout_for("Parent selected text");
    drag(
        &mut visual,
        text_point(&layout, 0, false),
        text_point(&layout, "Parent selected".len(), true),
    );
    visual.run_until_parked();
    let selected = visual.update(gpui_kit::base::TextSelection::selected_text);
    assert_eq!(selected, "Parent selected");
    root.update(&mut visual, |root, cx| {
        root.preview = Some(child);
        cx.notify();
    });
    visual.run_until_parked();
    assert_eq!(
        visual.update(gpui_kit::base::TextSelection::selected_text),
        selected
    );
}

#[gpui_kit::test]
fn reading_preview_releases_abandoned_history_and_restores_submission(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "preview-history");
    let submitted = Arc::new(Mutex::new(Vec::new()));
    stream.update(cx, |stream, cx| {
        let captured = submitted.clone();
        cx.subscribe(&cx.entity(), move |_, _, event: &ComposerSubmitted, _| {
            captured.lock().unwrap().push(event.thread_id.clone())
        })
        .detach();
        stream
            .input
            .update(cx, |input, cx| input.set_text("Continue child", cx));
        stream.hydration = HistoryHydration {
            older_cursor: Some(40),
            newer_cursor: Some(100),
            loading: true,
            paused: false,
            paused_direction: None,
        };
        stream.set_reading_preview(true, cx);
        assert_eq!(stream.history_page_request(true), Some(40));
        assert_eq!(stream.newer_history_page_request(true), Some(100));
        stream.submit_message(cx);
        assert!(!stream.composer_submit_pending);
    });
    cx.run_until_parked();
    assert!(submitted.lock().unwrap().is_empty());
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("composer-shell").is_none());
    stream.update(&mut visual, |stream, cx| {
        stream.hydration.loading = true;
        stream.set_reading_preview(false, cx);
        assert_eq!(stream.history_page_request(true), Some(40));
        assert_eq!(stream.newer_history_page_request(true), Some(100));
        stream.submit_message(cx);
    });
    visual.run_until_parked();
    assert_eq!(*submitted.lock().unwrap(), vec!["preview-history"]);
    assert!(visual.debug_bounds("composer-shell").is_some());
}

#[gpui_kit::test]
async fn reading_preview_has_no_permission_responder_and_full_route_retains_it(
    cx: &mut TestAppContext,
) {
    init_permission_test(cx);
    let (window, queue) = open_permission_stream(cx);
    window
        .update(cx, |stream, _, cx| stream.set_reading_preview(true, cx))
        .unwrap();
    propose(window, cx, bash_call("preview-approval", "printf preview"));
    let decision = request_permission(&queue, "preview-approval", "printf preview");
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("permission-action").is_none());
    visual.simulate_keystrokes("enter");
    visual.run_until_parked();
    assert!(queue.has_pending());
    window
        .update(&mut visual, |stream, _, cx| {
            stream.set_reading_preview(false, cx)
        })
        .unwrap();
    visual.run_until_parked();
    assert!(visual.debug_bounds("permission-action").is_some());
    visual.simulate_keystrokes("enter");
    visual.run_until_parked();
    assert_eq!(decision.await, PermissionDecision::Once);
    assert!(!queue.has_pending());
    window
        .update(&mut visual, |stream, _, cx| {
            assert!(stream.active_permission.is_none());
            stream.set_reading_preview(true, cx);
        })
        .unwrap();
    visual.run_until_parked();
    assert!(visual.debug_bounds("permission-action").is_none());
}
