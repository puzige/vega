//! Minimal text input for GPUI (single-line and fixed-row multi-line).
//!
//! Implemented following the official paradigm in the gpui source tree at the
//! pinned rev (`crates/gpui/examples/input.rs`): a [`TextInput`] entity holds
//! the editing state, a private [`TextElement`] shapes/paints the line and
//! registers the platform input handler, and editing keys are wired through
//! actions so the platform IME keeps working.
//!
//! Deviations from the example (required by the T08 dependency ruling "zero
//! additional dependencies"):
//! - character boundaries instead of `unicode_segmentation` grapheme
//!   boundaries (single-line inputs for this skeleton never need ZWJ clusters);
//! - optional password-style masking: the field displays one `•` per content
//!   character and never paints the real value; cut/copy are refused on
//!   masked fields so credentials cannot leave the app through the clipboard.
//!
//! S3-T18 Composer extension: `new_multiline` builds a fixed-`rows` input
//! (Enter inserts `\n` via the [`InsertNewline`] action — 架构师裁定
//! Enter=换行、Cmd+Enter=发送; paste preserves line breaks). Multi-line
//! display paints one shaped visual line per `\n` segment (plus soft wraps)
//! stacked top-down: 1~8 行自适应高度 + 超出 8 行后按光标跟随的内滚视口
//! (visual-wrap viewport, cursor-follow), each verified by a GPUI test.

use std::ops::Range;

use gpui::prelude::*;
use gpui::{
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
struct LineLayout {
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
    focus_handle: FocusHandle,
    content: SharedString,
    placeholder: SharedString,
    masked: bool,
    /// Multi-line mode (S3-T18 Composer): Enter inserts `\n`, paste keeps
    /// line breaks, and the element paints `rows` stacked lines.
    multiline: bool,
    /// Visible row count, dynamically clamped to 1..=8 for the Composer.
    rows: usize,
    /// First painted visual row when wrapped content exceeds eight rows.
    first_visible_row: usize,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    /// Layout cache from the last paint (one entry per display line).
    last_lines: Vec<LineLayout>,
    /// Line height used at the last paint (for y → line mapping).
    last_line_height: Pixels,
    last_bounds: Option<Bounds<Pixels>>,
    is_selecting: bool,
}

mod element;
mod input_handler;
mod state;

#[cfg(test)]
mod tests;
