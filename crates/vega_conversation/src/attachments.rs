//! Explicit attachment import facade (Issue 63 R1/R3); call on a worker thread.
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn issue63_explicit_import_is_bounded_atomic_and_rejects_nonregular_files() {
        let dir = tempfile::tempdir().unwrap();
        let valid = dir.path().join("not-trusting-extension.dat");
        let image = image::RgbImage::from_pixel(2, 3, image::Rgb([255, 0, 0]));
        image
            .save_with_format(&valid, image::ImageFormat::Png)
            .unwrap();
        let imported = import_images(std::slice::from_ref(&valid)).unwrap();
        assert_eq!(imported[0].mime_type(), "image/png");
        let invalid = dir.path().join("bad.png");
        std::fs::write(&invalid, b"private-sentinel-not-image").unwrap();
        let error = import_images(&[valid.clone(), invalid]).unwrap_err();
        assert!(!error.to_string().contains("private-sentinel"));
        let link = dir.path().join("link.png");
        std::os::unix::fs::symlink(&valid, &link).unwrap();
        assert!(import_images(&[link]).is_err());
        assert!(import_images(&[dir.path().to_path_buf()]).is_err());
        assert!(import_images(&vec![valid; 5]).is_err());
    }
}
