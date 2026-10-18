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
