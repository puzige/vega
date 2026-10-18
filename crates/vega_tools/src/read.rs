use std::path::Path;

use crate::Tools;
use crate::error::ToolError;
use crate::output::{LINE_TRUNCATION_MARKER, MAX_LINE_CHARS, ToolOutput};
use crate::text_file::TextFile;

/// Bytes probed from the file head for the NUL-byte binary check
/// (tech-spec §4.4: 读头部探 NUL 字节).
const BINARY_PROBE_LEN: u64 = 8192;

impl Tools {
    /// Read the file at `path` (absolute or project-root-relative) and render its lines
    /// with a right-aligned line-number gutter, `"<no> | <line>"`, padded to
    /// the width of the window's last line number (e.g. `" 12 | fn"` for a
    /// window ending at line 12).
    ///
    /// - `offset` is 1-based (`None` = 1); `limit` caps the number of lines
    ///   (`None` = whole file). Reading past EOF yields empty text.
    /// - Lines longer than 2000 chars are cut to 2000 chars and suffixed
    ///   with [`LINE_TRUNCATION_MARKER`]; any cut sets
    ///   [`ToolOutput::truncated`].
    /// - Binary files (NUL byte in the first 8 KiB) are rejected with
    ///   [`ToolError::BinaryFile`]; directories with [`ToolError::InvalidInput`].
    pub fn read(
        &self,
        path: &str,
        offset: Option<usize>,
        limit: Option<usize>,
    ) -> Result<ToolOutput, ToolError> {
        if let Some(0) = offset {
            return Err(ToolError::InvalidInput(
                "offset is 1-based and must be >= 1".to_string(),
            ));
        }
        let canonical = self.file_path(path)?;
        if canonical.is_dir() {
            return Err(ToolError::InvalidInput(format!("{path} is a directory")));
        }
        let bytes = read_text_head_probed(&canonical, path, self.credential_policy.is_some())?;
        let text = TextFile::decode(&bytes)?;
        self.reads.record(&canonical, &bytes)?;
        Ok(render_numbered(&text.content, offset.unwrap_or(1), limit))
    }
}

/// Read a bounded regular text file. Unsupported encodings are never decoded lossily.
fn read_text_head_probed(
    canonical: &Path,
    display: &str,
    reject_hardlinks: bool,
) -> Result<Vec<u8>, ToolError> {
    let bytes =
        crate::text_file::read_regular(canonical, reject_hardlinks).map_err(
            |error| match error {
                ToolError::Io(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    ToolError::NotFound(display.into())
                }
                other => other,
            },
        )?;
    if bytes
        .iter()
        .take(BINARY_PROBE_LEN as usize)
        .any(|b| *b == 0)
        && !bytes.starts_with(&[0xff, 0xfe])
    {
        return Err(ToolError::BinaryFile(display.into()));
    }
    Ok(bytes)
}

/// Render `content` as the 1-based window starting at `offset` with at most
/// `limit` lines (none = whole file).
fn render_numbered(content: &str, offset: usize, limit: Option<usize>) -> ToolOutput {
    let mut window: Vec<(usize, String)> = Vec::new();
    let mut truncated = false;
    for (idx, line) in content.lines().enumerate() {
        let no = idx + 1;
        if no < offset {
            continue;
        }
        if window.len() >= limit.unwrap_or(usize::MAX) {
            break;
        }
        let (text, cut) = truncate_line(line);
        truncated |= cut;
        window.push((no, text));
    }

    let width = window.last().map_or(1, |(no, _)| no.to_string().len());
    let mut parts: Vec<String> = Vec::with_capacity(window.len());
    for (no, text) in window {
        parts.push(format!("{no:>width$} | {text}"));
    }
    ToolOutput {
        text: parts.join("\n"),
        truncated,
    }
}

/// Cut `line` to [`MAX_LINE_CHARS`] chars, appending the truncation marker
/// when a cut happened (char-based, CJK-safe).
fn truncate_line(line: &str) -> (String, bool) {
    let mut chars = line.chars();
    let head: String = chars.by_ref().take(MAX_LINE_CHARS).collect();
    if chars.next().is_some() {
        (format!("{head}{LINE_TRUNCATION_MARKER}"), true)
    } else {
        (head, false)
    }
}
