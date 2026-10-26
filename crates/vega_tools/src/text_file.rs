//! Lossless supported text encodings and reference-compatible newline handling.
use crate::{MutationError, MutationErrorCode, ToolError};

pub(crate) struct TextFile {
    pub content: String,
    utf16: bool,
    crlf: bool,
    bom: bool,
}

impl TextFile {
    pub fn decode(bytes: &[u8]) -> Result<Self, ToolError> {
        let utf16 = bytes.starts_with(&[0xff, 0xfe]);
        let content = if utf16 {
            if !bytes.len().is_multiple_of(2) {
                return Err(unsupported());
            }
            let units: Vec<_> = bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .collect();
            String::from_utf16(&units).map_err(|_| unsupported())?
        } else {
            std::str::from_utf8(bytes)
                .map_err(|_| unsupported())?
                .to_owned()
        };
        if content.contains('\0') {
            return Err(unsupported());
        }
        let mut crlf_count = 0;
        let mut lf_count = 0;
        let mut previous = 0;
        for unit in content.encode_utf16().take(4096) {
            if unit == 10 {
                if previous == 13 {
                    crlf_count += 1;
                } else {
                    lf_count += 1;
                }
            }
            previous = unit;
        }
        let crlf = crlf_count > lf_count;
        let bom = content.starts_with('\u{feff}');
        Ok(Self {
            content: content.replace("\r\n", "\n"),
            utf16,
            crlf,
            bom,
        })
    }

    pub fn encode(&self, content: &str, preserve_endings: bool) -> Vec<u8> {
        let text = if preserve_endings && self.crlf {
            content.replace("\r\n", "\n").replace('\n', "\r\n")
        } else {
            content.to_owned()
        };
        let text = if self.bom && !text.starts_with('\u{feff}') {
            format!("\u{feff}{text}")
        } else {
            text
        };
        if self.utf16 {
            text.encode_utf16().flat_map(u16::to_le_bytes).collect()
        } else {
            text.into_bytes()
        }
    }
}

fn unsupported() -> ToolError {
    MutationError::new(MutationErrorCode::UnsupportedEncoding).into()
}

/// Open only regular files, without following the final component or blocking on FIFOs.
pub(crate) fn read_regular(
    path: &std::path::Path,
    reject_hardlinks: bool,
) -> Result<Vec<u8>, ToolError> {
    use std::io::Read;
    const MAX_BYTES: u64 = 1024 * 1024 * 1024;
    let file = open_regular(path, reject_hardlinks)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(MutationError::new(MutationErrorCode::PathNotFile).into());
    }
    if metadata.len() > MAX_BYTES {
        return Err(MutationError::new(MutationErrorCode::FileTooLarge).into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(MutationError::new(MutationErrorCode::FileTooLarge).into());
    }
    Ok(bytes)
}

pub(crate) fn open_directory(path: &std::path::Path) -> Result<std::fs::File, ToolError> {
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;
    if !path.is_absolute() {
        return Err(MutationError::new(MutationErrorCode::PathNotFile).into());
    }
    let mut directory = std::fs::File::open("/")?;
    for component in path.components() {
        let std::path::Component::Normal(name) = component else {
            if matches!(component, std::path::Component::RootDir) {
                continue;
            }
            return Err(MutationError::new(MutationErrorCode::PathNotFile).into());
        };
        let name = std::ffi::CString::new(name.as_bytes())
            .map_err(|_| MutationError::new(MutationErrorCode::PathNotFile))?;
        // SAFETY: directory is a live descriptor and name is a NUL-terminated single path component.
        let fd = unsafe {
            libc::openat(
                directory.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        // SAFETY: successful openat returned a new descriptor owned only by this File.
        directory = unsafe { std::fs::File::from_raw_fd(fd) };
    }
    Ok(directory)
}

pub(crate) fn open_regular(
    path: &std::path::Path,
    reject_hardlinks: bool,
) -> Result<std::fs::File, ToolError> {
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::{ffi::OsStrExt, fs::MetadataExt};
    let parent = path
        .parent()
        .ok_or_else(|| MutationError::new(MutationErrorCode::PathNotFile))?;
    let directory = open_directory(parent)?;
    let name = path
        .file_name()
        .ok_or_else(|| MutationError::new(MutationErrorCode::PathNotFile))?;
    let name = std::ffi::CString::new(name.as_bytes())
        .map_err(|_| MutationError::new(MutationErrorCode::PathNotFile))?;
    // SAFETY: directory is live and name is a NUL-terminated single component; no links are followed.
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    // SAFETY: successful openat returned a new descriptor owned only by this File.
    let file = unsafe { std::fs::File::from_raw_fd(fd) };
    let metadata = file.metadata()?;
    if !metadata.is_file() || (reject_hardlinks && metadata.nlink() != 1) {
        return Err(MutationError::new(MutationErrorCode::PathNotFile).into());
    }
    Ok(file)
}
