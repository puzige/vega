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
pub(crate) fn read_regular(path: &std::path::Path) -> Result<Vec<u8>, ToolError> {
    use std::io::Read;
    use std::os::unix::fs::OpenOptionsExt;
    const MAX_BYTES: u64 = 1024 * 1024 * 1024;
    let file = std::fs::File::options()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
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
