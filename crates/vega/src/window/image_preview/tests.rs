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
