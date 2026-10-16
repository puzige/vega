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
    let mut tool_bytes = 0usize;
    let mut tool_count = 0usize;
    for message in messages {
        if message.role == crate::ChatRole::Tool && !message.images.is_empty() {
            tool_bytes += message
                .images
                .iter()
                .map(|image| image.bytes.len())
                .sum::<usize>();
            tool_count += message.images.len();
            if message.tool_call_id.as_ref().is_none_or(|id| id.is_empty())
                || tool_bytes > 8 * 1024 * 1024
                || tool_count > 2
            {
                return Err(crate::VegaError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "tool observation image budget exceeded",
                )));
            }
            continue;
        }
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
