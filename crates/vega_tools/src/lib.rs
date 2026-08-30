//! Built-in project tools: `read`, `glob`, `grep`, `write`, and `edit`
//! (tech-spec §4.4, A3-05~07 / S4-T21 / S5-T23).
//!
//! [`Tools`] is the single entry point: bind one instance to the canonical
//! project root and every path argument is fenced against it — interpreted
//! relative to the root, canonicalized, and rejected with
//! [`ToolError::PathEscape`] when it would escape the root (`..` traversal,
//! absolute-path injection, or a symlink jumping out; tech-spec §3 red
//! line, risks #4).
//!
//! Mutations remain disabled until the caller explicitly supplies a
//! checkpoint root plus project/thread/call ids. All tools return the same
//! success shape, [`ToolOutput`] (text plus a
//! truncation flag), so the agentic loop (T20) can append every result as a
//! `tool_result` without per-tool branching.
//!
//! The crate is headless — no gpui, no UI crate (`cargo tree -p vega_tools`
//! must stay gpui-free, exec-guide §3).
//!
//! # Example
//!
//! ```
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let dir = tempfile::tempdir()?;
//! std::fs::write(dir.path().join("lib.rs"), "fn main() {}\n")?;
//!
//! let tools = vega_tools::Tools::new(dir.path())?;
//! let out = tools.read("lib.rs", None, None)?;
//! assert_eq!(out.text, "1 | fn main() {}");
//! assert!(!out.truncated);
//!
//! assert!(matches!(
//!     tools.read("../escape.txt", None, None),
//!     Err(vega_tools::ToolError::PathEscape(_))
//! ));
//! # Ok(())
//! # }
//! ```

mod bash;
mod checkpoint;
mod codec;
pub mod danger;
mod error;
mod fence;
mod glob;
mod grep;
mod mutation;
mod output;
mod read;
mod sandbox;
mod sha256;
mod tools;

pub use bash::{DEFAULT_BASH_TIMEOUT_MS, PreparedBash};
pub use codec::{
    CheckpointIds, CheckpointRef, CreatedNewFileMetadata, EditSuccessOutput, InvalidWriteEditAudit,
    MutationTool, WriteEditAudit, WriteSuccessOutput,
};
pub use error::{
    BashError, BashErrorCode, EditFailureContext, MutationError, MutationErrorCode, ToolError,
};
pub use mutation::{InvalidMutation, PrepareMutationError, PreparedEdit, PreparedWrite};
pub use output::{
    BASH_LINE_MIDDLE_MARKER, BASH_MAX_BYTES_PER_SIDE, BASH_MAX_LINE_BYTES, BASH_MAX_LINES_PER_SIDE,
    BASH_OUTPUT_MIDDLE_MARKER, BASH_READ_CHUNK_BYTES, BashOutput, LINE_TRUNCATION_MARKER,
    MAX_LINE_CHARS, MAX_RESULTS, RESULT_TRUNCATION_MARKER, ToolOutput,
};
pub use tools::Tools;
