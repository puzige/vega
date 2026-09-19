use super::*;
use gpui_kit::{ClipboardItem, Image, ImageFormat};

fn fixture() -> Vec<u8> {
    include_bytes!("../../../../../assets/logo/raster/vega-icon-f1-original.png").to_vec()
}

fn paste(
    window: WindowHandle<StreamHarness>,
    stream: &Entity<ConversationStream>,
    cx: &mut TestAppContext,
) {
    focus_composer(window, stream, cx);
    cx.update(|cx| {
        cx.write_to_clipboard(ClipboardItem::new_image(&Image::from_bytes(
            ImageFormat::Png,
            fixture(),
        )))
    });
    cx.dispatch_action(window.into(), crate::text_input::Paste);
    cx.run_until_parked();
}

#[gpui_kit::test]
async fn issue63_paste_image_only_submit_reject_and_ack_preserve_new_identical_image(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "image-submit");
    paste(window, &stream, cx);
    assert_eq!(stream.read_with(cx, |s, _| s.attachments.len()), 1);
    stream.update(cx, |s, cx| {
        s.submit_message(cx);
        assert!(s.composer_submit_pending);
        s.reject_composer_submission(cx);
    });
    assert_eq!(stream.read_with(cx, |s, _| s.attachments.len()), 1);
    stream.update(cx, |s, cx| s.submit_message(cx));
    paste(window, &stream, cx);
    assert_eq!(stream.read_with(cx, |s, _| s.attachments.len()), 2);
    stream.update(cx, |s, cx| s.accept_composer_submission("", cx));
    assert_eq!(stream.read_with(cx, |s, _| s.attachments.len()), 1);
    assert_eq!(
        stream.read_with(cx, |s, _| s
            .entries
            .iter()
            .filter(|e| matches!(e, StreamEntry::UserImages { .. }))
            .count()),
        1
    );
}

#[gpui_kit::test]
async fn issue63_mixed_clipboard_preserves_text_and_freezes_image(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "image-mixed");
    focus_composer(window, &stream, cx);
    cx.update(|cx| {
        cx.write_to_clipboard(ClipboardItem {
            entries: vec![
                gpui_kit::ClipboardEntry::String(gpui_kit::ClipboardString::new(
                    "describe this\r\nimage".into(),
                )),
                gpui_kit::ClipboardEntry::Image(Image::from_bytes(ImageFormat::Png, fixture())),
            ],
        })
    });
    cx.dispatch_action(window.into(), crate::text_input::Paste);
    cx.run_until_parked();
    assert_eq!(
        stream.read_with(cx, |s, cx| s.input.read(cx).text().to_string()),
        "describe this\nimage"
    );
    stream.update(cx, |s, cx| {
        s.submit_message(cx);
        assert_eq!(s.submitted_attachments.len(), 1);
    });
}

#[gpui_kit::test]
async fn issue63_invalid_clipboard_batch_is_atomic_and_route_retires_import(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "image-invalid");
    paste(window, &stream, cx);
    let item = ClipboardItem {
        entries: vec![
            gpui_kit::ClipboardEntry::Image(Image::from_bytes(ImageFormat::Png, fixture())),
            gpui_kit::ClipboardEntry::Image(Image::from_bytes(ImageFormat::Png, vec![0])),
        ],
    };
    cx.update(|cx| cx.write_to_clipboard(item));
    cx.dispatch_action(window.into(), crate::text_input::Paste);
    cx.run_until_parked();
    assert_eq!(stream.read_with(cx, |s, _| s.attachments.len()), 1);
    assert!(stream.read_with(cx, |s, _| s.attachment_error.is_some()));
    stream.update(cx, |s, cx| {
        s.paste_images(
            Arc::new(ClipboardItem::new_image(&Image::from_bytes(
                ImageFormat::Png,
                fixture(),
            ))),
            cx,
        );
        assert!(s.attachment_import_pending);
        s.submit_message(cx);
        assert!(!s.composer_submit_pending);
        s.cancel_image_import(cx);
    });
    cx.run_until_parked();
    assert_eq!(stream.read_with(cx, |s, _| s.attachments.len()), 1);
}

#[gpui_kit::test]
async fn issue63_native_picker_cancel_and_selected_file(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "image-picker");
    click_composer_add(window, cx);
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    let bounds = visual
        .debug_bounds("composer-action-images")
        .expect("image menu entry");
    visual.simulate_click(bounds.center(), gpui_kit::Modifiers::default());
    visual.run_until_parked();
    assert!(cx.did_prompt_for_paths());
    assert!(stream.read_with(cx, |s, _| s.attachment_import_pending));
    cx.simulate_path_prompt_response(|options| {
        assert!(options.files && options.multiple && !options.directories);
        None
    });
    cx.run_until_parked();
    assert!(!stream.read_with(cx, |s, _| s.attachment_import_pending));
    let temp = tempfile::tempdir().expect("owned image dir");
    let path = temp.path().join("image.png");
    std::fs::write(&path, fixture()).expect("owned image fixture");
    stream.update(cx, |s, cx| s.pick_images(cx));
    cx.simulate_path_prompt_response(|_| Some(vec![path]));
    cx.run_until_parked();
    assert_eq!(stream.read_with(cx, |s, _| s.attachments.len()), 1);
}

#[gpui_kit::test]
async fn issue63_route_change_discards_late_picker_and_empty_composer_has_no_attachment_surface(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "image-route");
    assert!(
        gpui_kit::VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("composer-attachments")
            .is_none()
    );
    let thread = stream.read_with(cx, |s, _| s.thread.clone());
    cx.update(|cx| cx.set_global(crate::sidebar::OpenedThread(Some(thread))));
    stream.update(cx, |s, cx| s.pick_images(cx));
    cx.update(|cx| cx.set_global(crate::sidebar::OpenedThread(None)));
    cx.run_until_parked();
    let temp = tempfile::tempdir().expect("owned stale picker dir");
    let path = temp.path().join("image.png");
    std::fs::write(&path, fixture()).expect("owned image fixture");
    cx.simulate_path_prompt_response(|_| Some(vec![path]));
    cx.run_until_parked();
    assert!(stream.read_with(cx, |s, _| s.attachments.is_empty()
        && !s.attachment_import_pending));
}

#[gpui_kit::test]
async fn issue63_external_drop_and_remove_use_rendered_handlers(cx: &mut TestAppContext) {
    use gpui_kit::{ExternalPaths, FileDropEvent, InputEvent, Modifiers, VisualTestContext};
    let (window, stream, _) = open_controller_stream(cx, "image-drop");
    let temp = tempfile::tempdir().expect("owned drop dir");
    let path = temp.path().join("image.png");
    std::fs::write(&path, fixture()).expect("owned image fixture");
    let position = VisualTestContext::from_window(window.into(), cx)
        .debug_bounds("composer-shell")
        .expect("composer bounds")
        .center();
    window
        .update(cx, |_, window, cx| {
            window.dispatch_event(
                FileDropEvent::Entered {
                    position,
                    paths: ExternalPaths(vec![path].into()),
                }
                .to_platform_input(),
                cx,
            );
            window.dispatch_event(FileDropEvent::Submit { position }.to_platform_input(), cx);
        })
        .expect("drop events");
    cx.run_until_parked();
    let id = stream.read_with(cx, |s, _| {
        assert_eq!(s.attachments.len(), 1);
        s.attachments[0].0
    });
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let selector: &'static str = Box::leak(format!("remove-image-{id}").into_boxed_str());
    let bounds = visual.debug_bounds(selector).expect("remove button bounds");
    visual.simulate_click(bounds.center(), Modifiers::default());
    visual.run_until_parked();
    assert!(stream.read_with(cx, |s, _| s.attachments.is_empty()));
}
