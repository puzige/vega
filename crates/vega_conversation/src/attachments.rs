use crate::types::ImageAttachment;
use std::os::unix::fs::OpenOptionsExt;
use std::{fs::OpenOptions, io::Read, path::PathBuf};
pub use vega_runtime::images::{ImageError, validate_images};
pub use vega_runtime::images::{MAX_IMAGE_BYTES, MAX_IMAGES, MAX_TURN_IMAGE_BYTES};

/// Atomically imports an explicitly selected set; never follows symlinks/FIFOs.
pub fn import_images(paths: &[PathBuf]) -> Result<Vec<ImageAttachment>, ImageError> {
    if paths.len() > vega_runtime::images::MAX_IMAGES {
        return Err(ImageError("at most 4 images per turn"));
    }
    let mut images = Vec::new();
    for path in paths {
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)
            .map_err(|_| ImageError("could not open selected image"))?;
        if !file
            .metadata()
            .map_err(|_| ImageError("could not inspect selected image"))?
            .is_file()
        {
            return Err(ImageError("select regular image files"));
        }
        let mut bytes = Vec::new();
        file.take((vega_runtime::images::MAX_IMAGE_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| ImageError("could not read selected image"))?;
        images.push(ImageAttachment::from_bytes(bytes)?);
        validate_images(&images)?;
    }
    Ok(images)
}

pub fn save_screenshot(image: &ImageAttachment, path: &std::path::Path) -> Result<(), ImageError> {
    use std::io::Write;
    validate_images(std::slice::from_ref(image))?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| ImageError("could not create screenshot file; choose a new filename"))?;
    file.write_all(image.bytes())
        .map_err(|_| ImageError("could not save screenshot file"))?;
    file.sync_all()
        .map_err(|_| ImageError("could not finish saving screenshot file"))
}
