//! Shared command-palette projections.
/// Executable application operations; route availability is decided by the app.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaletteAction {
    NewTask,
    OpenWorkspace,
    Settings,
    ToggleSidebar,
    Terminal,
    Preview,
    Review,
}
/// Search scope; filtering is local over bounded worker results.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PaletteScope {
    #[default]
    All,
    Actions,
    Tasks,
    Files,
}
/// A persisted task identity and display label.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaletteTask {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub project_name: String,
}
/// Bounded worker search result with explicit file-index failure.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PaletteSearch {
    pub tasks: Vec<PaletteTask>,
    pub files: Vec<String>,
    pub files_unavailable: bool,
}
/// Selected executable result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PaletteTarget {
    Action(PaletteAction),
    Task(PaletteTask),
    File(String),
}
/// Read-only text preview; no editor authority accompanies this value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaletteFilePreview {
    pub relative_path: String,
    pub content: String,
}
/// Content-free failure vocabulary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PaletteError {
    #[error("Search is unavailable")]
    Unavailable,
    #[error("Project is unavailable")]
    ProjectUnavailable,
    #[error("File is outside the project or changed")]
    UnsafePath,
    #[error("Binary files cannot be previewed")]
    Binary,
    #[error("File exceeds the 128 KiB preview limit")]
    TooLarge,
    #[error("Search was cancelled")]
    Cancelled,
}
