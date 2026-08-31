//! IO-free bounded `@file` selector model (A2-12, S8-T47).
//!
//! Pure state only: the candidate list arrives as a typed projection
//! ([`FileIndexSnapshot`], capped at [`FILE_INDEX_LIMIT`] entries) supplied
//! by the app layer, which walks the project root on a worker thread. The
//! selector never touches the filesystem and never sends a provider request.
//!
//! Keyboard contract (ui-spec §6 全可达): an `@` token in the composer opens
//! the list; Up/Down move the highlight (clamped), Enter/Tab accept
//! first-wins (the first candidate is pre-highlighted), and Esc closes
//! without inserting anything. The composer focus chain stays untouched —
//! the dropdown is rendered by the conversation stream, actions are scoped
//! to the `FileSelect` key context so they shadow the composer bindings
//! only while the list is open.

use gpui::actions;
use vega_conversation::types::FileIndexSnapshot;

/// UI-side hard cap mirroring the producer contract: the app layer walks the
/// project root with a hard entry cap; the selector additionally never
/// renders more than this many candidates regardless of input size.
pub const FILE_INDEX_LIMIT: usize = 512;

actions!(
    vega_file_selector,
    [AcceptFile, NextFile, PreviousFile, CancelFile]
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

/// Focus-capable rendering is owned by the conversation stream: it renders
/// the dropdown rows from [`FileSelectorModel::candidates`] inside the
/// composer card and registers the [`AcceptFile`]/[`NextFile`]/[`PreviousFile`]/[`CancelFile`]
/// actions scoped to the `FileSelect` key context, so the model above stays
/// the single source of truth and the selector adds no extra focus stops to
/// the composer chain.

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(entries: &[&str]) -> FileIndexSnapshot {
        FileIndexSnapshot {
            entries: entries.iter().map(|entry| (*entry).to_string()).collect(),
        }
    }

    #[test]
    fn open_filters_first_wins_and_bounded() {
        let snapshot = snapshot(&[
            "src/lib.rs",
            "src/main.rs",
            "docs/readme.md",
            "tests/e2e.rs",
        ]);
        let mut model = FileSelectorModel::default();
        assert!(model.open_for(&snapshot, "src"));
        assert_eq!(model.candidates().len(), 2, "contains-filter is bounded");
        assert_eq!(model.highlighted(), 0, "first candidate starts highlighted");
        // First-wins Enter without moving.
        assert_eq!(model.accept().as_deref(), Some("src/lib.rs"));
        assert!(!model.is_open());

        // Empty query lists at most FILE_SUGGESTION_LIMIT candidates.
        assert!(model.open_for(&snapshot, ""));
        assert_eq!(model.candidates().len(), FILE_SUGGESTION_LIMIT.min(4));

        // No match stays closed.
        assert!(!model.open_for(&snapshot, "zzz"));
        assert!(!model.is_open());
    }

    #[test]
    fn keyboard_moves_clamp_and_esc_closes_without_inserting() {
        let snapshot = snapshot(&["a.rs", "b.rs"]);
        let mut model = FileSelectorModel::default();
        assert!(model.open_for(&snapshot, ""));
        assert!(model.move_highlight(5), "down clamps to last row");
        assert_eq!(model.highlighted(), 1);
        assert!(model.move_highlight(-5), "up clamps to first row");
        assert_eq!(model.highlighted(), 0);
        assert!(model.close());
        assert!(!model.is_open());
        assert_eq!(model.selected(), None, "closed selector inserts nothing");
        assert!(!model.close(), "double close is a no-op");
    }
}
