use std::path::Path;

#[derive(Clone, PartialEq, Eq)]
pub struct ReadIdentity {
    pub canonical_path: String,
    pub sha256: String,
    pub complete: bool,
}

use crate::Tools;
use crate::error::ToolError;
use crate::output::{LINE_TRUNCATION_MARKER, MAX_LINE_CHARS, ToolOutput};
use crate::sha256::{Sha256, hex};
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
        self.read_with_identity(path, offset, limit)
            .map(|(output, _)| output)
    }

    pub fn read_context_file(&self, path: &str) -> Result<ToolOutput, ToolError> {
        use std::io::Read;
        const MAX_BYTES: u64 = 1024 * 1024;
        let canonical = self.file_path(path)?;
        if Path::new(path).is_absolute() && canonical != Path::new(path) {
            return Err(crate::MutationError::new(crate::MutationErrorCode::TargetChanged).into());
        }
        let file = crate::text_file::open_regular(&canonical, self.credential_policy.is_some())?;
        if file.metadata()?.len() > MAX_BYTES {
            return Ok(ToolOutput {
                text: String::new(),
                truncated: true,
            });
        }
        let mut bytes = Vec::new();
        file.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_BYTES {
            return Ok(ToolOutput {
                text: String::new(),
                truncated: true,
            });
        }
        let text = TextFile::decode(&bytes)?;
        if text.content.len().div_ceil(4) > 5000 {
            return Ok(ToolOutput {
                text: String::new(),
                truncated: true,
            });
        }
        Ok(ToolOutput {
            text: text.content,
            truncated: false,
        })
    }

    pub fn read_with_identity(
        &self,
        path: &str,
        offset: Option<usize>,
        limit: Option<usize>,
    ) -> Result<(ToolOutput, ReadIdentity), ToolError> {
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
        let output = render_numbered(&text.content, offset.unwrap_or(1), limit);
        let mut hash = Sha256::new();
        hash.update(&bytes);
        let identity = ReadIdentity {
            canonical_path: canonical.to_string_lossy().into_owned(),
            sha256: hex(&hash.finalize()),
            complete: offset.is_none() && limit.is_none() && !output.truncated,
        };
        Ok((output, identity))
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

fn truncate_line(line: &str) -> (String, bool) {
    let mut chars = line.chars();
    let head: String = chars.by_ref().take(MAX_LINE_CHARS).collect();
    if chars.next().is_some() {
        (format!("{head}{LINE_TRUNCATION_MARKER}"), true)
    } else {
        (head, false)
    }
}

#[cfg(test)]
mod context_tests {
    use super::*;

    #[test]
    fn context_read_does_not_create_or_refresh_edit_read_versions()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = std::env::temp_dir().join(format!("vega-context-read-{}", std::process::id()));
        std::fs::create_dir_all(&root)?;
        let path = root.join("notes.txt");
        std::fs::write(&path, "old")?;
        let tools = Tools::new(&root)?;
        assert_eq!(tools.read_context_file("notes.txt")?.text, "old");
        assert!(tools.reads.validate(&path.canonicalize()?, b"old").is_err());
        tools.read("notes.txt", None, None)?;
        std::fs::write(&path, "new")?;
        assert_eq!(tools.read_context_file("notes.txt")?.text, "new");
        assert!(tools.reads.validate(&path.canonicalize()?, b"new").is_err());
        assert!(tools.reads.validate(&path.canonicalize()?, b"old").is_ok());
        std::fs::remove_dir_all(root)?;
        Ok(())
    }
}
