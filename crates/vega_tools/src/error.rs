//! Tool error taxonomy: what the read-only tools report back to the agentic
//! loop (S4-T21). One variant per failure shape the loop (T20) can explain
//! to the model inside a `tool_result`; `Io` carries the underlying
//! `std::io::Error` untouched. `Send + Sync` by construction.

/// Errors surfaced by the built-in read-only tools.
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    /// A requested path resolved outside the project root: `..` traversal,
    /// absolute-path injection, or a symlink jumping out of the root.
    /// Path-fence red line (tech-spec §3, risks #4) — never softened.
    #[error("path escapes the project root: {0}")]
    PathEscape(String),

    /// The requested path does not exist.
    #[error("not found: {0}")]
    NotFound(String),

    /// The file was rejected by the NUL-byte binary probe (tech-spec §4.4).
    #[error("binary file: {0}")]
    BinaryFile(String),

    /// Caller input is malformed (bad regex, bad glob, 0 offset, …).
    #[error("invalid input: {0}")]
    InvalidInput(String),

    /// Result set exceeded a tool's hard limit. Reserved for callers that
    /// prefer a hard failure; T21's glob/grep truncate into
    /// [`ToolOutput::truncated`](crate::ToolOutput::truncated) instead.
    #[error("too many results (limit: {limit})")]
    TooManyResults { limit: usize },

    /// Underlying filesystem error.
    #[error(transparent)]
    Io(#[from] std::io::Error),

    /// Directory traversal failed before a file could be inspected.
    #[error("filesystem traversal failed: {0}")]
    Traversal(String),
}
