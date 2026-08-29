//! Built-in read-only tools: `read`, `glob`, and `grep` (tech-spec §4.4,
//! A3-05~07 部分 / S4-T21).
//!
//! [`Tools`] is the single entry point: bind one instance to the canonical
//! project root and every path argument is fenced against it — interpreted
//! relative to the root, canonicalized, and rejected with
//! [`ToolError::PathEscape`] when it would escape the root (`..` traversal,
//! absolute-path injection, or a symlink jumping out; tech-spec §3 red
//! line, risks #4).
//!
//! All tools return the same success shape, [`ToolOutput`] (text plus a
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

mod error;
mod fence;
mod output;
mod read;
mod tools;

pub use error::ToolError;
pub use output::{
    LINE_TRUNCATION_MARKER, MAX_LINE_CHARS, MAX_RESULTS, RESULT_TRUNCATION_MARKER, ToolOutput,
};
pub use tools::Tools;
