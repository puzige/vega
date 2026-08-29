//! Unified success payload shared by all read-only tools (S4-T21): the
//! agentic loop (T20) appends every tool result as a `tool_result`, so the
//! shape must stay stable across tools.

/// Hard cap on glob/grep result entries (tech-spec §4.4: 结果上限 500 条).
pub const MAX_RESULTS: usize = 500;

/// Per-line character cap for the read tool (tech-spec §4.4: 单行 >2k 截断).
pub const MAX_LINE_CHARS: usize = 2000;

/// Marker appended to a read line cut at [`MAX_LINE_CHARS`].
pub const LINE_TRUNCATION_MARKER: &str = "…[截断]";

/// Marker line appended when glob/grep results exceed [`MAX_RESULTS`]
/// (kept in sync with the 500-entry cap).
pub const RESULT_TRUNCATION_MARKER: &str = "…[截断：结果超过上限 500 条]";

/// What a tool returns on success: rendered text plus a truncation flag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolOutput {
    /// Rendered text payload (numbered lines, path list, match list).
    pub text: String,
    /// True when any content was cut: read lines past [`MAX_LINE_CHARS`]
    /// or glob/grep entries past [`MAX_RESULTS`].
    pub truncated: bool,
}

impl ToolOutput {
    /// A clean (untruncated) output with the given text.
    pub fn clean(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            truncated: false,
        }
    }
}

/// Render a result list while keeping at most [`MAX_RESULTS`] entries.
///
/// Callers collect at most `MAX_RESULTS + 1`, so the extra entry is only a
/// truncation sentinel and never reaches the output.
pub(crate) fn capped_results(mut entries: Vec<String>) -> ToolOutput {
    let truncated = entries.len() > MAX_RESULTS;
    entries.truncate(MAX_RESULTS);
    if truncated {
        entries.push(RESULT_TRUNCATION_MARKER.to_string());
    }
    ToolOutput {
        text: entries.join("\n"),
        truncated,
    }
}
