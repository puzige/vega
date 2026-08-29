//! read tool: numbered lines with per-line truncation (tech-spec §4.4,
//! A3-06 部分 / S4-T21).

use std::fs::File;
use std::io::Read;
use std::path::Path;

use crate::Tools;
use crate::error::ToolError;
use crate::fence::resolve_in_root;
use crate::output::{LINE_TRUNCATION_MARKER, MAX_LINE_CHARS, ToolOutput};

/// Bytes probed from the file head for the NUL-byte binary check
/// (tech-spec §4.4: 读头部探 NUL 字节).
const BINARY_PROBE_LEN: u64 = 8192;

impl Tools {
    /// Read the file at `path` (project-root-relative) and render its lines
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
        let canonical = resolve_in_root(&self.root, path)?;
        if canonical.is_dir() {
            return Err(ToolError::InvalidInput(format!("{path} is a directory")));
        }
        let content = read_text_head_probed(&canonical, path)?;
        Ok(render_numbered(&content, offset.unwrap_or(1), limit))
    }
}

/// Read the whole file as lossy UTF-8, probing its head for NUL bytes so a
/// binary file is rejected before the rest of it is loaded (非 UTF-8 文本不
/// 因此拒绝——spec 只以 NUL 探测定义二进制，编码问题降级 lossy).
fn read_text_head_probed(canonical: &Path, display: &str) -> Result<String, ToolError> {
    let mut file = File::open(canonical)?;
    let mut head = Vec::new();
    file.by_ref()
        .take(BINARY_PROBE_LEN)
        .read_to_end(&mut head)?;
    if head.contains(&0) {
        return Err(ToolError::BinaryFile(display.to_string()));
    }
    let mut rest = Vec::new();
    file.read_to_end(&mut rest)?;
    head.extend_from_slice(&rest);
    Ok(String::from_utf8_lossy(&head).into_owned())
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

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use tempfile::tempdir;

    use crate::error::ToolError;
    use crate::output::{LINE_TRUNCATION_MARKER, MAX_LINE_CHARS};

    /// Create `root`-relative file with parent dirs, returning the tools
    /// instance bound to the (canonicalized) root.
    fn setup(root: &Path, rel: &str, content: &[u8]) -> super::super::Tools {
        let target = root.join(rel);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(target, content).unwrap();
        super::super::Tools::new(root).unwrap()
    }

    #[test]
    fn numbers_lines_and_reports_clean_output() {
        let dir = tempdir().unwrap();
        let tools = setup(dir.path(), "a.txt", b"alpha\nbeta\ngamma\n");
        let out = tools.read("a.txt", None, None).unwrap();
        assert_eq!(out.text, "1 | alpha\n2 | beta\n3 | gamma");
        assert!(!out.truncated);
    }

    #[test]
    fn offset_and_limit_window_the_file() {
        let dir = tempdir().unwrap();
        let tools = setup(dir.path(), "a.txt", b"alpha\nbeta\ngamma\n");
        let out = tools.read("a.txt", Some(2), Some(1)).unwrap();
        assert_eq!(out.text, "2 | beta");
        assert!(!out.truncated);
    }

    #[test]
    fn window_ending_at_line_12_pads_the_gutter() {
        let body: String = (1..=15).map(|i| format!("line {i}\n")).collect();
        let dir = tempdir().unwrap();
        let tools = setup(dir.path(), "a.txt", body.as_bytes());
        let out = tools.read("a.txt", Some(10), Some(3)).unwrap();
        assert_eq!(out.text, "10 | line 10\n11 | line 11\n12 | line 12");
    }

    #[test]
    fn cuts_long_lines_with_marker() {
        let long = "x".repeat(3000);
        let dir = tempdir().unwrap();
        let tools = setup(dir.path(), "a.txt", long.as_bytes());
        let out = tools.read("a.txt", None, None).unwrap();
        let expected = format!("{}{LINE_TRUNCATION_MARKER}", "x".repeat(MAX_LINE_CHARS));
        assert_eq!(out.text, format!("1 | {expected}"));
        assert!(out.truncated);
        // 未截断的行不受影响
        fs::write(dir.path().join("a.txt"), "short\n").unwrap();
        let out = tools.read("a.txt", None, None).unwrap();
        assert_eq!(out.text, "1 | short");
        assert!(!out.truncated);
    }

    #[test]
    fn rejects_binary_files_by_nul_probe() {
        let dir = tempdir().unwrap();
        let tools = setup(dir.path(), "a.bin", b"hello\0world");
        let err = tools.read("a.bin", None, None).unwrap_err();
        assert!(matches!(err, ToolError::BinaryFile(_)));
    }

    #[test]
    fn rejects_missing_file_as_not_found() {
        let dir = tempdir().unwrap();
        let tools = setup(dir.path(), "a.txt", b"x");
        let err = tools.read("nope.txt", None, None).unwrap_err();
        assert!(matches!(err, ToolError::NotFound(_)));
    }

    #[test]
    fn rejects_directory_and_zero_offset_as_invalid_input() {
        let dir = tempdir().unwrap();
        let tools = setup(dir.path(), "sub/a.txt", b"x");
        assert!(matches!(
            tools.read("sub", None, None).unwrap_err(),
            ToolError::InvalidInput(_)
        ));
        assert!(matches!(
            tools.read("sub/a.txt", Some(0), None).unwrap_err(),
            ToolError::InvalidInput(_)
        ));
    }

    #[test]
    fn empty_file_and_offset_past_eof_yield_empty_text() {
        let dir = tempdir().unwrap();
        let tools = setup(dir.path(), "a.txt", b"");
        let out = tools.read("a.txt", None, None).unwrap();
        assert_eq!(out.text, "");
        assert!(!out.truncated);
        let out = tools.read("a.txt", Some(9), None).unwrap();
        assert_eq!(out.text, "");
    }
}
