use super::*;
use crate::types::ImageAttachment;

fn fixture() -> ImageAttachment {
    let image = image::RgbImage::from_pixel(4, 3, image::Rgb([200, 20, 70]));
    let mut encoded = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut encoded, image::ImageFormat::Png)
        .unwrap();
    ImageAttachment::from_bytes(encoded.into_inner()).unwrap()
}
