use super::*;
use gpui_kit::VisualTestContext;

fn picture() -> ImageAttachment {
    ImageAttachment::from_bytes(
        include_bytes!("../../../../../assets/logo/raster/vega-icon-f1-original.png").to_vec(),
    )
    .unwrap()
}
