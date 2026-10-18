use std::ops::Range;

use gpui_kit::prelude::*;
use gpui_kit::{
    App, Bounds, ClipboardItem, Context, CursorStyle, Element, ElementId, ElementInputHandler,
    Entity, EntityInputHandler, FocusHandle, Focusable, GlobalElementId, InspectorElementId,
    LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point,
    ShapedLine, SharedString, Style, TextAlign, TextRun, UTF16Selection, UnderlineStyle, Window,
    actions, div, fill, point, px, relative, size,
};
use vega_theme::{Typography, theme};

actions!(
    vega_text_input,
    [
        Backspace,
        Delete,
        Left,
        Right,
        SelectLeft,
        SelectRight,
        SelectAll,
        Home,
        End,
        ShowCharacterPalette,
        Paste,
        Cut,
        Copy,
        InsertNewline
    ]
);

/// Character displayed for every content character in masked (key) fields.
const MASK_CHAR: char = '•';

/// One laid-out display line: its start offset in the content, the segment
/// length (excluding the separating `\n`), and the shaped line. Single-line
/// inputs keep exactly one entry (start 0).
pub(crate) struct LineLayout {
    start: usize,
    len: usize,
    line: ShapedLine,
}

/// A text input (single-line, or fixed-row multi-line for the Composer).
///
/// Editing state (content, selection, marked/IME range) lives on the entity;
/// the platform talks to it through [`EntityInputHandler`]. When `masked` is
/// set the field renders [`MASK_CHAR`] per character and refuses cut/copy, so
/// the real value is never shown or extracted through the UI.
pub struct TextInput {
    image_paste: bool,
    focus_handle: FocusHandle,
    content: SharedString,
    content_revision: u64,
    placeholder: SharedString,
    masked: bool,
    multiline: bool,
    /// Visible row count, dynamically clamped to the configured row range.
    rows: usize,
    /// Minimum visible rows for this input's layout viewport.
    min_rows: usize,
    /// Maximum visible rows for this input's layout viewport.
    max_rows: usize,
    /// First painted visual row when wrapped content exceeds the viewport.
    first_visible_row: usize,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    /// Layout cache from the last paint (one entry per display line).
    last_lines: Vec<LineLayout>,
    /// Line height used at the last paint (for y → line mapping).
    last_line_height: Pixels,
    last_bounds: Option<Bounds<Pixels>>,
    mouse_selection: Option<mouse_selection::MouseSelection>,
    bare_chrome: bool,
    editor_scroll: Option<(Range<usize>, u64, usize)>,
    scrollable_editor: bool,
    total_visual_rows: usize,
    #[cfg(test)]
    painted_cursor_bounds: Option<Bounds<Pixels>>,
}

mod element;
mod input_handler;
mod mouse_selection;
mod state;

/// Explicit Composer-only clipboard intent; never log clipboard payloads.
pub struct ImagePaste(pub std::sync::Arc<ClipboardItem>);
impl gpui_kit::EventEmitter<ImagePaste> for TextInput {}

impl TextInput {
    /// Enables attachment paste only for the conversation Composer.
    pub fn with_image_paste(mut self) -> Self {
        self.image_paste = true;
        self
    }
}

#[cfg(test)]
mod tests;
