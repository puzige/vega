use super::{ConversationStream, ImagePreview, VegaWindow, native};
use gpui_kit::{
    AppContext, Bounds, ClipboardItem, Entity, Focusable, Image, ImageFormat, Pixels, WindowBounds,
    WindowHandle, WindowOptions, point, px, size,
};
use gpui_kit::{TestAppContext, VisualTestContext};
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use vega_conversation::types::ImageAttachment;

struct Fixture {
    _directory: tempfile::TempDir,
    root: Entity<VegaWindow>,
    stream: Entity<ConversationStream>,
    window: WindowHandle<VegaWindow>,
}

fn fixture(cx: &mut TestAppContext) -> Fixture {
    let directory = tempfile::tempdir().unwrap();
    let store = vega_store::Store::open(":memory:").unwrap();
    store.migrate().unwrap();
    let project = vega_store::projects::create(
        store.conn(),
        directory.path().to_str().unwrap(),
        "images",
        None,
    )
    .unwrap();
    let thread =
        vega_conversation::threads::create_thread(&store, &project.id, "mock", "confirm").unwrap();
    cx.update(|cx| crate::tests::install_diff_window_globals(store, thread, cx));
    let root = cx.new(VegaWindow::new);
    let view = root.clone();
    let window = cx.update(|cx| {
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    point(px(0.), px(0.)),
                    size(px(1403.), px(1000.)),
                ))),
                ..Default::default()
            },
            move |_, _| view,
        )
        .unwrap()
    });
    cx.run_until_parked();
    let stream = root.read_with(cx, |root, _| root.stream_view.as_ref().unwrap().1.clone());
    Fixture {
        _directory: directory,
        root,
        stream,
        window,
    }
}

fn picture(portrait: bool) -> ImageAttachment {
    ImageAttachment::from_bytes(if portrait {
        include_bytes!(
            "../../../../vega_ui/src/conversation_stream/tests/fixtures/screenshot-portrait.png"
        )
        .to_vec()
    } else {
        include_bytes!(
            "../../../../vega_ui/src/conversation_stream/tests/fixtures/screenshot-landscape.png"
        )
        .to_vec()
    })
    .unwrap()
}

fn page(
    entries: Vec<vega_conversation::history::HistoryEntry>,
) -> vega_conversation::history::HistoryPage {
    vega_conversation::history::HistoryPage {
        subagents: Vec::new(),
        entries,
        older_cursor: None,
        newer_cursor: None,
        newest_seq: None,
    }
}

fn screenshot(portrait: bool, call: &str) -> vega_conversation::history::HistoryEntry {
    vega_conversation::history::HistoryEntry::AssistantScreenshot {
        seq: 1,
        message_id: "assistant-image".into(),
        call_id: call.into(),
        image: picture(portrait),
    }
}

fn bounds(f: &Fixture, selector: &'static str, cx: &mut TestAppContext) -> Bounds<Pixels> {
    cx.run_until_parked();
    VisualTestContext::from_window(f.window.into(), cx)
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"))
}

fn click(f: &Fixture, selector: &'static str, cx: &mut TestAppContext) {
    let bounds = bounds(f, selector, cx);
    let mut visual = VisualTestContext::from_window(f.window.into(), cx);
    visual.simulate_click(bounds.center(), Default::default());
    visual.run_until_parked();
}

fn absent(f: &Fixture, selector: &'static str, cx: &mut TestAppContext) -> bool {
    cx.run_until_parked();
    VisualTestContext::from_window(f.window.into(), cx)
        .debug_bounds(selector)
        .is_none()
}

fn active(f: &Fixture, cx: &TestAppContext) -> Entity<ImagePreview> {
    f.root.read_with(cx, |root, _| {
        root.image_preview.active.as_ref().unwrap().image.clone()
    })
}

fn paste(f: &Fixture, cx: &mut TestAppContext) {
    let input = f.stream.read_with(cx, |stream, _| stream.composer_input());
    f.window
        .update(cx, |_, window, cx| {
            input.read(cx).focus_handle(cx).focus(window, cx)
        })
        .unwrap();
    cx.update(|cx| {
        cx.write_to_clipboard(ClipboardItem::new_image(&Image::from_bytes(
            ImageFormat::Png,
            picture(false).bytes().to_vec(),
        )))
    });
    cx.dispatch_action(f.window.into(), vega_ui::text_input::Paste);
    cx.run_until_parked();
}

#[gpui_kit::test]
async fn issue296_image_preview_occurrences_keyboard_and_history_replacement(
    cx: &mut TestAppContext,
) {
    let f = fixture(cx);
    paste(&f, cx);
    click(&f, "attachment-thumbnail", cx);
    let first = active(&f, cx);
    let first_name = first.read_with(cx, |view, _| view.name().to_owned());
    let first_focus = first.read_with(cx, |view, _| view.thumbnail_focus());
    assert!(!absent(&f, "image-preview", cx));
    paste(&f, cx);
    assert_eq!(
        active(&f, cx),
        first,
        "new import preserves existing source"
    );
    f.window
        .update(cx, |_, window, cx| {
            first.read(cx).focus_handle(cx).focus(window, cx)
        })
        .unwrap();
    cx.simulate_keystrokes(f.window.into(), "escape");
    assert!(absent(&f, "image-preview", cx));
    assert!(
        f.window
            .update(cx, |_, window, _| first_focus.is_focused(window))
            .unwrap()
    );
    cx.simulate_keystrokes(f.window.into(), "enter");
    assert_eq!(active(&f, cx), first);
    cx.simulate_keystrokes(f.window.into(), "escape space");
    assert_eq!(active(&f, cx), first);
    let id = first_name
        .trim_start_matches("图片-")
        .trim_end_matches(".png");
    click(
        &f,
        Box::leak(format!("remove-image-{id}").into_boxed_str()),
        cx,
    );
    assert!(absent(&f, "image-preview", cx));
    click(&f, "attachment-thumbnail", cx);
    assert_ne!(
        active(&f, cx),
        first,
        "identical bytes remain a distinct occurrence"
    );
    assert_eq!(
        active(&f, cx).read_with(cx, |view, _| view.image().clone()),
        picture(false)
    );
    let history = || {
        page(vec![vega_conversation::history::HistoryEntry::UserImages {
            seq: 1,
            message_id: "user-image".into(),
            images: vec![picture(false)],
        }])
    };
    f.stream.update(cx, |stream, cx| {
        assert!(stream.replace_history_window(history(), "user-image", cx));
    });
    cx.simulate_keystrokes(f.window.into(), "escape");
    let user = bounds(&f, "user-message-images", cx);
    VisualTestContext::from_window(f.window.into(), cx).simulate_click(
        user.bottom_right() - point(px(32.), px(32.)),
        Default::default(),
    );
    cx.run_until_parked();
    let loaded = active(&f, cx);
    assert_ne!(loaded, first);
    f.stream.update(cx, |stream, cx| {
        assert!(stream.replace_history_window(history(), "user-image", cx));
    });
    assert!(
        absent(&f, "image-preview", cx),
        "same durable key with a rebuilt occurrence retires the old pane"
    );
}

#[gpui_kit::test]
async fn issue296_image_preview_workspace_nonmodal_focus_and_narrow_fallback(
    cx: &mut TestAppContext,
) {
    let f = fixture(cx);
    f.stream.update(cx, |stream, cx| {
        stream.apply_history_page(page(vec![screenshot(false, "first")]), cx)
    });
    click(&f, "screenshot-expand", cx);
    let image = active(&f, cx);
    let origin = image.read_with(cx, |view, _| view.thumbnail_focus());
    let pane = bounds(&f, "image-preview", cx);
    assert!(pane.left() > px(500.));
    click(&f, "composer-add", cx);
    assert!(
        !absent(&f, "composer-action-images", cx),
        "outside controls remain interactive"
    );
    assert!(!absent(&f, "image-preview", cx));
    cx.simulate_keystrokes(f.window.into(), "escape");
    assert!(!absent(&f, "image-preview", cx), "Composer Escape is local");
    click(&f, "right-workspace-dock", cx);
    assert!(!absent(&f, "bottom-tabs", cx));
    click(&f, "screenshot-expand", cx);
    assert!(!absent(&f, "right-tabs", cx));
    assert_eq!(active(&f, cx), image);
    let visual = VisualTestContext::from_window(f.window.into(), cx);
    visual.simulate_resize(size(px(780.), px(800.)));
    visual.run_until_parked();
    assert!(
        !absent(&f, "image-preview", cx),
        "narrow resize retains a visible image page"
    );
    let visual = VisualTestContext::from_window(f.window.into(), cx);
    visual.simulate_resize(size(px(1403.), px(1000.)));
    visual.run_until_parked();
    assert_eq!(bounds(&f, "image-preview", cx).size, pane.size);
    f.window
        .update(cx, |_, window, cx| {
            image.read(cx).focus_handle(cx).focus(window, cx)
        })
        .unwrap();
    cx.simulate_keystrokes(f.window.into(), "escape");
    assert!(absent(&f, "image-preview", cx));
    assert!(
        f.window
            .update(cx, |_, window, _| origin.is_focused(window))
            .unwrap()
    );
    cx.simulate_keystrokes(f.window.into(), "enter");
    assert_eq!(active(&f, cx), image);
}

#[gpui_kit::test]
async fn issue296_image_preview_saves_exact_bytes_busy_and_late_picker(cx: &mut TestAppContext) {
    let f = fixture(cx);
    let temp = tempfile::tempdir().unwrap();
    for portrait in [false, true] {
        f.stream.update(cx, |stream, cx| {
            assert!(stream.replace_history_window(
                page(vec![screenshot(portrait, "save")]),
                "assistant-image",
                cx
            ));
        });
        let before = bounds(&f, "assistant-screenshot", cx);
        assert!(absent(&f, "screenshot-save", cx));
        click(&f, "screenshot-expand", cx);
        let image = active(&f, cx);
        click(&f, "image-preview-save", cx);
        assert!(cx.did_prompt_for_new_path());
        cx.simulate_new_path_selection(|parent| {
            assert_ne!(parent, std::path::Path::new("/"));
            None
        });
        cx.run_until_parked();
        assert_eq!(image.read_with(cx, |view, _| view.status()), None);
        assert!(!image.read_with(cx, |view, _| view.busy()));
        f.window
            .update(cx, |_, window, cx| {
                image.read(cx).focus_handle(cx).focus(window, cx);
                window.focus_next(cx);
                window.focus_next(cx);
            })
            .unwrap();
        cx.simulate_keystrokes(f.window.into(), "space");
        assert!(
            cx.did_prompt_for_new_path(),
            "pane save is reachable by keyboard while using the native picker"
        );
        assert!(image.read_with(cx, |view, _| view.busy()));
        click(&f, "image-preview-save", cx);
        let saved = temp.path().join(format!("{portrait}.png"));
        cx.simulate_new_path_selection(|_| Some(saved.clone()));
        cx.run_until_parked();
        assert_eq!(std::fs::read(&saved).unwrap(), picture(portrait).bytes());
        assert!(
            !cx.did_prompt_for_new_path(),
            "repeated save uses one pending operation"
        );
        assert_eq!(
            bounds(&f, "assistant-screenshot", cx).size.height,
            before.size.height
        );
        click(&f, "image-preview-save", cx);
        cx.simulate_new_path_selection(|_| Some(saved.clone()));
        cx.run_until_parked();
        assert_eq!(std::fs::read(&saved).unwrap(), picture(portrait).bytes());
        assert!(
            active(&f, cx).read_with(cx, |view, _| view.status().unwrap().starts_with("保存失败"))
        );
        assert!(!absent(&f, "image-preview-status", cx));
        assert!(absent(&f, "screenshot-save-status", cx));
        click(&f, "image-preview-save", cx);
        let image = active(&f, cx);
        f.window
            .update(cx, |_, window, cx| {
                image.read(cx).focus_handle(cx).focus(window, cx)
            })
            .unwrap();
        cx.simulate_keystrokes(f.window.into(), "escape");
        let late = temp.path().join(format!("late-{portrait}.png"));
        cx.simulate_new_path_selection(|_| Some(late.clone()));
        cx.run_until_parked();
        assert!(
            !late.exists(),
            "closing the matching preview fences the picker before writing"
        );
    }
}

struct FakeProcess {
    ready: bool,
    success: bool,
}
impl native::ImageProcess for FakeProcess {
    fn poll(&mut self) -> Result<Option<bool>, native::ImageOpenError> {
        Ok(self.ready.then_some(self.success))
    }
    fn cancel(&mut self) {}
}

struct FakeSystem {
    calls: Arc<Mutex<Vec<Vec<OsString>>>>,
    paths: Arc<Mutex<Vec<PathBuf>>>,
    ready: bool,
    success: bool,
}
impl native::ImageSystem for FakeSystem {
    fn applications(
        &self,
        path: &std::path::Path,
    ) -> Result<Vec<native::ImageApplication>, native::ImageOpenError> {
        assert_eq!(std::fs::read(path).unwrap(), picture(false).bytes());
        self.paths.lock().unwrap().push(path.to_owned());
        Ok(vec![native::ImageApplication {
            label: "Owned Viewer（默认）".into(),
            path: PathBuf::from("/Applications/Owned Viewer.app"),
        }])
    }
    fn spawn(
        &self,
        args: &[OsString],
    ) -> Result<Box<dyn native::ImageProcess>, native::ImageOpenError> {
        self.calls.lock().unwrap().push(args.to_vec());
        Ok(Box::new(FakeProcess {
            ready: self.ready,
            success: self.success,
        }))
    }
}

#[gpui_kit::test]
async fn issue296_image_preview_open_with_real_controller_and_owned_export(
    cx: &mut TestAppContext,
) {
    let f = fixture(cx);
    let calls = Arc::new(Mutex::new(Vec::new()));
    let paths = Arc::new(Mutex::new(Vec::new()));
    f.root.update(cx, |root, _| {
        root.image_preview.system = Arc::new(FakeSystem {
            calls: calls.clone(),
            paths: paths.clone(),
            ready: true,
            success: true,
        })
    });
    f.stream.update(cx, |stream, cx| {
        stream.apply_history_page(page(vec![screenshot(false, "open")]), cx)
    });
    click(&f, "screenshot-expand", cx);
    assert!(
        paths.lock().unwrap().is_empty(),
        "preview alone never exports"
    );
    f.window
        .update(cx, |_, window, cx| window.focus_next(cx))
        .unwrap();
    cx.simulate_keystrokes(f.window.into(), "enter");
    cx.run_until_parked();
    assert!(!absent(&f, "image-preview-applications", cx));
    assert!(
        calls.lock().unwrap().is_empty(),
        "enumeration does not launch an app"
    );
    let first = paths.lock().unwrap()[0].clone();
    assert_eq!(std::fs::read(&first).unwrap(), picture(false).bytes());
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        std::fs::metadata(&first).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        std::fs::metadata(first.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    cx.simulate_keystrokes(f.window.into(), "enter");
    cx.run_until_parked();
    assert_eq!(
        *calls.lock().unwrap(),
        vec![vec![
            OsString::from("-a"),
            OsString::from("/Applications/Owned Viewer.app"),
            OsString::from("--"),
            first.as_os_str().to_owned()
        ]]
    );
    let image = active(&f, cx);
    assert_eq!(image.read_with(cx, |view, _| view.status()), None);
    assert!(!image.read_with(cx, |view, _| view.busy()));
    assert!(absent(&f, "image-preview-status", cx));
    assert!(absent(&f, "image-preview-applications", cx));
    assert!(absent(&f, "screenshot-save-status", cx));
    cx.simulate_keystrokes(f.window.into(), "escape");
    assert!(
        absent(&f, "image-preview", cx),
        "selection returns focus to the pane toolbar"
    );
    assert!(first.exists(), "handed-off file outlives its pane");
    std::fs::write(&first, b"external edit").unwrap();
    click(&f, "screenshot-expand", cx);
    click(&f, "image-preview-open-with", cx);
    let second = paths.lock().unwrap()[1].clone();
    assert_ne!(first, second);
    assert_eq!(std::fs::read(&first).unwrap(), b"external edit");
    assert_eq!(std::fs::read(second).unwrap(), picture(false).bytes());
    click(&f, "image-preview-open-with", cx);
    assert!(
        absent(&f, "image-preview-applications", cx),
        "trigger dismisses without reopening"
    );
    click(&f, "image-preview-open-with", cx);
    click(&f, "image-preview-app-0", cx);
    assert_eq!(
        calls.lock().unwrap().len(),
        2,
        "mouse selection also dispatches exactly once"
    );
    click(&f, "image-preview-open-with", cx);
    click(&f, "composer-add", cx);
    assert!(
        absent(&f, "image-preview-applications", cx),
        "outside click dismisses only the menu"
    );
    assert!(!absent(&f, "image-preview", cx));
}

#[gpui_kit::test]
async fn issue296_image_preview_open_failure_stays_visible_and_success_clears_status(
    cx: &mut TestAppContext,
) {
    let f = fixture(cx);
    let calls = Arc::new(Mutex::new(Vec::new()));
    let paths = Arc::new(Mutex::new(Vec::new()));
    f.stream.update(cx, |stream, cx| {
        stream.apply_history_page(page(vec![screenshot(false, "open-error")]), cx)
    });
    click(&f, "screenshot-expand", cx);
    let image = active(&f, cx);
    let card = bounds(&f, "assistant-screenshot", cx);
    for (ready, success, error) in [
        (true, false, native::ImageOpenError::Launch),
        (false, true, native::ImageOpenError::TimedOut),
    ] {
        f.root.update(cx, |root, _| {
            root.image_preview.timeout = std::time::Duration::ZERO;
            root.image_preview.system = Arc::new(FakeSystem {
                calls: calls.clone(),
                paths: paths.clone(),
                ready,
                success,
            });
        });
        let before = calls.lock().unwrap().len();
        click(&f, "image-preview-open-with", cx);
        click(&f, "image-preview-app-0", cx);
        assert_eq!(calls.lock().unwrap().len(), before + 1);
        assert_eq!(
            image.read_with(cx, |view, _| view.status()),
            Some(error.message())
        );
        assert!(!image.read_with(cx, |view, _| view.busy()));
        assert!(!absent(&f, "image-preview-status", cx));
        assert!(absent(&f, "image-preview-applications", cx));
        assert_eq!(bounds(&f, "assistant-screenshot", cx), card);
        f.root.update(cx, |root, _| {
            root.image_preview.system = Arc::new(FakeSystem {
                calls: calls.clone(),
                paths: paths.clone(),
                ready: true,
                success: true,
            });
        });
        click(&f, "image-preview-open-with", cx);
        click(&f, "image-preview-app-0", cx);
        assert_eq!(calls.lock().unwrap().len(), before + 2);
        assert_eq!(image.read_with(cx, |view, _| view.status()), None);
        assert!(!image.read_with(cx, |view, _| view.busy()));
        assert!(absent(&f, "image-preview-status", cx));
        assert!(absent(&f, "image-preview-applications", cx));
        assert_eq!(bounds(&f, "assistant-screenshot", cx), card);
    }
}

#[gpui_kit::test]
async fn issue296_image_preview_task_roundtrip_fences_picker(cx: &mut TestAppContext) {
    let f = fixture(cx);
    let (thread, other) = cx.update(|cx| {
        let thread = cx
            .global::<vega_ui::sidebar::OpenedThread>()
            .0
            .clone()
            .unwrap();
        let store = cx.global::<crate::VegaStore>().0.as_ref().unwrap();
        let other =
            vega_conversation::threads::create_thread(store, &thread.project_id, "mock", "confirm")
                .unwrap();
        (thread, other)
    });
    f.root.update(cx, |root, _| {
        root.agent_controller
            .begin(thread.id.clone(), f.stream.clone(), None, None)
            .unwrap();
    });
    f.stream.update(cx, |stream, cx| {
        stream.apply_history_page(page(vec![screenshot(false, "route")]), cx)
    });
    click(&f, "screenshot-expand", cx);
    click(&f, "image-preview-save", cx);
    assert!(cx.did_prompt_for_new_path());
    let image = active(&f, cx);
    cx.update(|cx| cx.set_global(vega_ui::sidebar::OpenedThread(Some(other))));
    cx.run_until_parked();
    cx.update(|cx| cx.set_global(vega_ui::sidebar::OpenedThread(Some(thread))));
    cx.run_until_parked();
    assert_eq!(
        f.root
            .read_with(cx, |root, _| root.stream_view.as_ref().unwrap().1.clone()),
        f.stream
    );
    let destination = f._directory.path().join("retired-picker.png");
    cx.simulate_new_path_selection(|_| Some(destination.clone()));
    cx.run_until_parked();
    assert!(!destination.exists());
    assert_eq!(image.read_with(cx, |view, _| view.status()), None);
    assert!(!image.read_with(cx, |view, _| view.busy()));
    assert!(absent(&f, "image-preview", cx));
    f.root.update(cx, |root, _| root.window_terminal_cleanup());
}

#[gpui_kit::test]
async fn issue296_image_preview_reading_anchor_survives_split_and_background_scroll(
    cx: &mut TestAppContext,
) {
    let f = fixture(cx);
    let mut entries = vec![screenshot(false, "reading")];
    for seq in 2..=30 {
        entries.push(vega_conversation::history::HistoryEntry::AssistantText {
            seq,
            message_id: format!("body-{seq}"),
            content: "可选择的连续正文 text wrapping remains intact. ".repeat(16),
            status: vega_conversation::history::AssistantStatus::Done,
            execution_duration_ms: None,
        });
    }
    f.stream.update(cx, |stream, cx| {
        stream.apply_history_page(page(entries), cx)
    });
    cx.run_until_parked();
    f.stream.update(cx, |stream, cx| {
        stream.request_message_location("assistant-image", cx);
    });
    cx.run_until_parked();
    let before = f
        .stream
        .read_with(cx, |stream, _| stream.scroll_anchor_snapshot());
    click(&f, "screenshot-expand", cx);
    let after = f
        .stream
        .read_with(cx, |stream, _| stream.scroll_anchor_snapshot());
    assert_eq!(before.identity, after.identity);
    assert_eq!(before.following_tail, after.following_tail);
    assert!((before.offset_in_item_px - after.offset_in_item_px).abs() <= 1.);
    let area = bounds(&f, "conversation-scroll", cx);
    let mut visual = VisualTestContext::from_window(f.window.into(), cx);
    visual.simulate_event(gpui_kit::ScrollWheelEvent {
        position: area.center(),
        delta: gpui_kit::ScrollDelta::Pixels(point(px(0.), px(-400.))),
        touch_phase: gpui_kit::TouchPhase::Moved,
        modifiers: Default::default(),
    });
    visual.run_until_parked();
    let moved = f
        .stream
        .read_with(cx, |stream, _| stream.scroll_anchor_snapshot());
    assert_ne!(
        moved, before,
        "nonmodal preview does not occlude conversation wheel input"
    );
    assert!(!absent(&f, "image-preview", cx));
    let old_height = f32::from(bounds(&f, "assistant-message", cx).size.height);
    let current = active(&f, cx);
    f.window
        .update(cx, |_, window, cx| {
            current.read(cx).focus_handle(cx).focus(window, cx)
        })
        .unwrap();
    cx.simulate_keystrokes(f.window.into(), "escape");
    let closed = f
        .stream
        .read_with(cx, |stream, _| stream.scroll_anchor_snapshot());
    assert_eq!(closed.identity, moved.identity);
    assert!(!closed.following_tail);
    let new_height = f32::from(bounds(&f, "assistant-message", cx).size.height);
    assert!(
        (closed.offset_in_item_px - moved.offset_in_item_px * new_height / old_height).abs() <= 1.,
        "moved={moved:?} closed={closed:?}, heights={old_height}/{new_height}"
    );
    let composer = f.stream.read_with(cx, |stream, _| stream.composer_input());
    assert!(
        f.window
            .update(cx, |_, window, cx| composer
                .read(cx)
                .focus_handle(cx)
                .is_focused(window))
            .unwrap(),
        "offscreen source falls back to composer without seeking it"
    );
}

#[gpui_kit::test]
async fn issue296_image_preview_empty_failed_menu_and_window_retirement(cx: &mut TestAppContext) {
    struct Query(bool);
    impl native::ImageSystem for Query {
        fn applications(
            &self,
            _: &std::path::Path,
        ) -> Result<Vec<native::ImageApplication>, native::ImageOpenError> {
            if self.0 {
                Ok(Vec::new())
            } else {
                Err(native::ImageOpenError::Applications)
            }
        }
        fn spawn(
            &self,
            _: &[OsString],
        ) -> Result<Box<dyn native::ImageProcess>, native::ImageOpenError> {
            panic!("query cannot launch")
        }
    }
    let f = fixture(cx);
    f.stream.update(cx, |stream, cx| {
        stream.apply_history_page(page(vec![screenshot(false, "empty")]), cx)
    });
    click(&f, "screenshot-expand", cx);
    for empty in [true, false] {
        f.root.update(cx, |root, _| {
            root.image_preview.system = Arc::new(Query(empty))
        });
        click(&f, "image-preview-open-with", cx);
        assert!(absent(&f, "image-preview-applications", cx));
        let status = active(&f, cx).read_with(cx, |view, _| view.status());
        assert_eq!(
            status,
            Some(if empty {
                "没有可用的应用，请保存到本地后打开"
            } else {
                "无法查询打开方式，请保存到本地后打开"
            })
        );
        assert!(!absent(&f, "image-preview-status", cx));
        assert!(!active(&f, cx).read_with(cx, |view, _| view.busy()));
        click(&f, "image-preview-save", cx);
        assert!(cx.did_prompt_for_new_path());
        cx.simulate_new_path_selection(|_| None);
        cx.run_until_parked();
    }
    let cancel = tokio_util::sync::CancellationToken::new();
    let image = active(&f, cx);
    f.root.update(cx, |root, _| {
        root.image_preview.operation = Some((image, cancel.clone(), 77))
    });
    f.root.update(cx, |root, _| root.window_terminal_cleanup());
    assert!(cancel.is_cancelled());
    assert!(
        f.root
            .read_with(cx, |root, _| root.image_preview.active.is_none())
    );
}
