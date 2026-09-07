//! In-memory terminal projections shared by the service and UI.

/// Fixed terminal lifecycle; never includes process error text or terminal contents.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalStatus {
    Starting,
    Running,
    Exited(u32),
    Failed,
}

/// A terminal color supplied by ANSI output (Default uses the active UI theme).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalColor {
    Default,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

/// One rendered terminal cell, with parsed attributes and wide-cell continuation.
#[derive(Clone)]
pub struct TerminalCell {
    pub text: String,
    pub foreground: TerminalColor,
    pub background: TerminalColor,
    pub bold: bool,
    pub inverse: bool,
    pub continuation: bool,
}

/// A bounded viewport; terminal text intentionally has no Debug implementation.
#[derive(Clone)]
pub struct TerminalSnapshot {
    pub generation: u64,
    pub status: TerminalStatus,
    pub cells: Vec<Vec<TerminalCell>>,
    pub cursor: Option<(u16, u16)>,
    pub application_cursor: bool,
    pub bracketed_paste: bool,
    pub scrollback: usize,
}

/// Trusted startup authority: either a caller-owned directory or a registered project.
#[derive(Clone)]
pub enum TerminalTarget {
    Directory(std::path::PathBuf),
    Project {
        database_path: std::path::PathBuf,
        project_id: String,
    },
}
