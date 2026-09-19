//! Issue 63 R3/R6: immutable validated images, with content-free errors/debug.
use image::{ImageDecoder, ImageFormat, ImageReader, Limits};
use std::{fmt, io::Cursor, sync::Arc};

/// Encoded bytes accepted per explicit image.
pub const MAX_IMAGE_BYTES: usize = 8 * 1024 * 1024;
/// Encoded bytes accepted per user turn.
pub const MAX_TURN_IMAGE_BYTES: usize = 16 * 1024 * 1024;
/// Encoded bytes accepted per history request.
pub const MAX_HISTORY_IMAGE_BYTES: usize = 32 * 1024 * 1024;
/// Number of explicit images accepted per user turn.
pub const MAX_IMAGES: usize = 4;

/// Safe validation failure. Never includes file names or decoder payloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("Image attachment unavailable: {0}")]
pub struct ImageError(pub &'static str);

/// Encoded image validated once off the UI thread; clones share immutable bytes.
#[derive(Clone, PartialEq, Eq)]
pub struct ImageAttachment {
    bytes: Arc<[u8]>,
    mime: &'static str,
    width: u32,
    height: u32,
}

impl fmt::Debug for ImageAttachment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ImageAttachment")
            .field("bytes", &self.bytes.len())
            .field("mime", &self.mime)
            .field("width", &self.width)
            .field("height", &self.height)
            .finish()
    }
}

impl ImageAttachment {
    /// Validates actual format, dimensions, animation and complete decode.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, ImageError> {
        if bytes.is_empty() || bytes.len() > MAX_IMAGE_BYTES {
            return Err(ImageError("image must be 1 byte to 8 MiB"));
        }
        let format = image::guess_format(&bytes).map_err(|_| ImageError("unsupported format"))?;
        let mime = match format {
            ImageFormat::Png => "image/png",
            ImageFormat::Jpeg => "image/jpeg",
            ImageFormat::WebP => "image/webp",
            _ => return Err(ImageError("use PNG, JPEG or WebP")),
        };
        let mut limits = Limits::default();
        limits.max_image_width = Some(8192);
        limits.max_image_height = Some(8192);
        limits.max_alloc = Some(64 * 1024 * 1024);
        // R3: animation metadata is checked before decoding a static first frame.
        let animated = match format {
            ImageFormat::Png => {
                image::codecs::png::PngDecoder::with_limits(Cursor::new(&bytes), limits.clone())
                    .and_then(|decoder| decoder.is_apng())
                    .map_err(|_| ImageError("invalid PNG"))?
            }
            ImageFormat::WebP => image::codecs::webp::WebPDecoder::new(Cursor::new(&bytes))
                .map_err(|_| ImageError("invalid WebP"))?
                .has_animation(),
            _ => false,
        };
        if animated {
            return Err(ImageError("animated images are not supported"));
        }
        let mut reader = ImageReader::with_format(Cursor::new(&bytes), format);
        reader.limits(limits.clone());
        let decoder = reader
            .into_decoder()
            .map_err(|_| ImageError("invalid image"))?;
        let (width, height) = decoder.dimensions();
        if width == 0
            || height == 0
            || u64::from(width) * u64::from(height) > 16_000_000
            || decoder.total_bytes() > 64 * 1024 * 1024
        {
            return Err(ImageError("image dimensions exceed limits"));
        }
        drop(decoder);
        let mut reader = ImageReader::with_format(Cursor::new(&bytes), format);
        reader.limits(limits);
        reader
            .decode()
            .map_err(|_| ImageError("image decode failed or exceeded limits"))?;
        Ok(Self {
            bytes: bytes.into(),
            mime,
            width,
            height,
        })
    }
    /// Original validated encoded bytes, never a path or inferred URL.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    /// MIME derived from actual image format, not the selected extension.
    pub fn mime_type(&self) -> &str {
        self.mime
    }
    /// Validated image width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }
    /// Validated image height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }
}

/// Validates turn-level limits, including callers not originating in the UI.
pub fn validate_images(images: &[ImageAttachment]) -> Result<(), ImageError> {
    if images.len() > MAX_IMAGES
        || images.iter().map(|image| image.bytes.len()).sum::<usize>() > MAX_TURN_IMAGE_BYTES
    {
        return Err(ImageError("at most 4 images and 16 MiB per turn"));
    }
    Ok(())
}

/// Enforces budgets and role ownership at both public runtime entry points.
pub(crate) fn validate_messages(messages: &[crate::ChatMessage]) -> Result<(), crate::VegaError> {
    let mut total = 0usize;
    for message in messages {
        let invalid = validate_images(&message.images).is_err()
            || (message.role != crate::ChatRole::User && !message.images.is_empty());
        total = total.saturating_add(
            message
                .images
                .iter()
                .map(|image| image.bytes.len())
                .sum::<usize>(),
        );
        if invalid || total > MAX_HISTORY_IMAGE_BYTES {
            return Err(crate::VegaError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "image attachment request exceeds limits or has invalid role",
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn encode(width: u32, height: u32, format: ImageFormat) -> Vec<u8> {
        let pixels = image::RgbImage::from_pixel(width, height, image::Rgb([180, 20, 90]));
        let mut output = Cursor::new(Vec::new());
        pixels.write_to(&mut output, format).unwrap();
        output.into_inner()
    }
    #[test]
    fn issue63_codec_limits_and_redaction() {
        for (format, mime) in [
            (ImageFormat::Png, "image/png"),
            (ImageFormat::Jpeg, "image/jpeg"),
            (ImageFormat::WebP, "image/webp"),
        ] {
            let image = ImageAttachment::from_bytes(encode(3, 2, format)).unwrap();
            assert_eq!(
                (image.width(), image.height(), image.mime_type()),
                (3, 2, mime)
            );
            use base64::Engine;
            assert!(
                !format!("{image:?}")
                    .contains(&base64::engine::general_purpose::STANDARD.encode(image.bytes()))
            );
        }
        for invalid in [
            Vec::new(),
            b"GIF89a".to_vec(),
            b"not an image".to_vec(),
            vec![0; MAX_IMAGE_BYTES + 1],
            encode(8193, 1, ImageFormat::Png),
        ] {
            assert!(ImageAttachment::from_bytes(invalid).is_err());
        }
        let mut truncated = encode(3, 2, ImageFormat::Png);
        truncated.truncate(truncated.len() / 2);
        assert!(ImageAttachment::from_bytes(truncated).is_err());
        let image = ImageAttachment::from_bytes(encode(3, 2, ImageFormat::Png)).unwrap();
        assert!(validate_images(&vec![image.clone(); 5]).is_err());
        let mut large = image.bytes().to_vec();
        large.resize(MAX_IMAGE_BYTES, 0);
        let large = ImageAttachment::from_bytes(large).unwrap();
        assert!(validate_images(&vec![large.clone(); 2]).is_ok());
        assert!(validate_images(&vec![large.clone(); 3]).is_err());
        let mut message = crate::ChatMessage::new(crate::ChatRole::User, "");
        message.images = vec![large; 2];
        assert!(validate_messages(&vec![message.clone(); 2]).is_ok());
        assert!(validate_messages(&vec![message; 3]).is_err());
    }
    #[test]
    fn issue63_pixel_budget_rejects_valid_overbudget_header_before_decode() {
        assert!(ImageAttachment::from_bytes(encode(4001, 4000, ImageFormat::Png)).is_err());
    }
    #[test]
    fn issue63_valid_apng_is_rejected_instead_of_sending_first_frame() {
        fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
            let mut chunk = (data.len() as u32).to_be_bytes().to_vec();
            chunk.extend_from_slice(kind);
            chunk.extend_from_slice(data);
            let mut crc = !0u32;
            for byte in &chunk[4..] {
                crc ^= u32::from(*byte);
                for _ in 0..8 {
                    crc = (crc >> 1) ^ (0xedb88320 & (0u32.wrapping_sub(crc & 1)));
                }
            }
            chunk.extend_from_slice(&(!crc).to_be_bytes());
            chunk
        }
        let png = encode(3, 2, ImageFormat::Png);
        let mut apng = png[..33].to_vec(); // signature + IHDR
        apng.extend(chunk(b"acTL", &[0, 0, 0, 1, 0, 0, 0, 0]));
        let mut frame = Vec::new();
        for value in [0u32, 3, 2, 0, 0] {
            frame.extend_from_slice(&value.to_be_bytes());
        }
        frame.extend_from_slice(&[0, 1, 0, 10, 0, 0]);
        apng.extend(chunk(b"fcTL", &frame));
        apng.extend_from_slice(&png[33..]);
        assert!(
            image::codecs::png::PngDecoder::new(Cursor::new(&apng))
                .unwrap()
                .is_apng()
                .unwrap()
        );
        assert!(
            image::load_from_memory(&apng).is_ok(),
            "fixture must decode as real PNG"
        );
        assert_eq!(
            ImageAttachment::from_bytes(apng).unwrap_err(),
            ImageError("animated images are not supported")
        );
    }
    #[tokio::test]
    async fn issue63_wire_parts_and_direct_provider_role_guard() {
        let image = ImageAttachment::from_bytes(encode(3, 2, ImageFormat::Png)).unwrap();
        let mut message = crate::ChatMessage::new(crate::ChatRole::User, "describe");
        message.images = vec![image.clone(), image];
        let request = crate::ChatRequest {
            messages: vec![
                message.clone(),
                crate::ChatMessage::new(crate::ChatRole::User, "plain"),
            ],
            ..Default::default()
        };
        let body = crate::openai::build_request_body(&request);
        assert_eq!(
            body["messages"][0]["content"][0],
            serde_json::json!({"type":"text", "text":"describe"})
        );
        assert_eq!(body["messages"][0]["content"].as_array().unwrap().len(), 3);
        assert_eq!(body["messages"][1]["content"], "plain");
        message.role = crate::ChatRole::Assistant;
        let provider = crate::OpenAiProvider::new("http://127.0.0.1:1", "owned").unwrap();
        use crate::Provider;
        assert!(
            provider
                .chat_stream(
                    crate::ChatRequest {
                        messages: vec![message],
                        ..Default::default()
                    },
                    tokio_util::sync::CancellationToken::new()
                )
                .await
                .is_err()
        );
    }
}
