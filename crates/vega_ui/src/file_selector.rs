use gpui_kit::actions;
use vega_conversation::types::FileIndexSnapshot;

/// UI-side hard cap mirroring the producer contract: the app layer walks the
/// project root with a hard entry cap; the selector additionally never
/// renders more than this many candidates regardless of input size.
pub const FILE_INDEX_LIMIT: usize = 512;

actions!(
    vega_file_selector,
    [
        AcceptFile,
        NextFile,
        PreviousFile,
        CancelFile,
        RetryFile,
        FocusRetry,
        FocusPreviousRetry
    ]
);

/// How many fuzzy candidates the selector presents at once (bounded list).
pub const FILE_SUGGESTION_LIMIT: usize = 8;

/// Pure selector state (headless-testable): open/closed, the `@` query body
/// being completed, the highlight row, and the bounded candidates.
#[derive(Debug, Default)]
pub struct FileSelectorModel {
    open: bool,
    query: String,
    highlighted: usize,
    candidates: Vec<String>,
}

impl FileSelectorModel {
    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn highlighted(&self) -> usize {
        self.highlighted
    }

    /// Opens (or re-filters) the selector for an `@` token body `query`
    /// against the bounded snapshot. The first candidate starts highlighted
    /// (first-wins on Enter). `false` when nothing matches (selector stays
    /// closed).
    pub fn open_for(&mut self, snapshot: &FileIndexSnapshot, query: &str) -> bool {
        let lowercase = query.to_lowercase();
        self.candidates = snapshot
            .entries
            .iter()
            .filter(|entry| entry.to_lowercase().contains(&lowercase))
            .take(FILE_SUGGESTION_LIMIT)
            .cloned()
            .collect();
        if self.candidates.is_empty() {
            self.close();
            return false;
        }
        self.open = true;
        self.query = query.to_string();
        self.highlighted = 0;
        true
    }

    /// Moves the highlight one row (clamped, no wrap); `false` when closed.
    pub fn move_highlight(&mut self, delta: isize) -> bool {
        if !self.open {
            return false;
        }
        let len = self.candidates.len() as isize;
        let next = (self.highlighted as isize + delta).clamp(0, len - 1);
        self.highlighted = next as usize;
        true
    }

    /// The currently highlighted candidate (first-wins on Enter/Tab).
    pub fn selected(&self) -> Option<&str> {
        if !self.open {
            return None;
        }
        self.candidates.get(self.highlighted).map(String::as_str)
    }

    /// Accepts the highlighted candidate: closes and returns it.
    pub fn accept(&mut self) -> Option<String> {
        let selected = self.selected()?.to_string();
        self.close();
        Some(selected)
    }

    /// Accepts a row chosen with the mouse through the same first-wins path as
    /// keyboard Enter/Tab.
    pub fn accept_at(&mut self, index: usize) -> Option<String> {
        if !self.open || index >= self.candidates.len() {
            return None;
        }
        self.highlighted = index;
        self.accept()
    }

    /// Closes without inserting anything (Esc). `false` when already closed.
    pub fn close(&mut self) -> bool {
        if !self.open {
            return false;
        }
        self.open = false;
        self.query.clear();
        self.highlighted = 0;
        self.candidates.clear();
        true
    }

    /// Visible candidates for rendering (already bounded).
    pub fn candidates(&self) -> &[String] {
        &self.candidates
    }
}

// Focus-capable rendering is owned by the conversation stream: it renders
// the dropdown rows from [`FileSelectorModel::candidates`] inside the
// composer card and registers the [`AcceptFile`]/[`NextFile`]/[`PreviousFile`]/[`CancelFile`]
// actions scoped to the `FileSelect` key context, so the model above stays
// the single source of truth and the selector adds no extra focus stops to
// the composer chain. (Tests for the model live below.)
