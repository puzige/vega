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
//! display paints one shaped line per `\n` segment stacked top-down; the
//! 1~8 行自适应高度 is deferred per task card (fixed rows, overflow clipped).

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
    /// Visible row count (1 for single-line inputs; the Composer uses a
    /// fixed 3 — 自适应 1~8 行后置 per task card).
    rows: usize,
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

impl TextInput {
    /// Creates an empty single-line input with a placeholder; `masked`
    /// renders bullets.
    pub fn new(cx: &mut Context<Self>, placeholder: impl Into<SharedString>, masked: bool) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            content: "".into(),
            placeholder: placeholder.into(),
            masked,
            multiline: false,
            rows: 1,
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            last_lines: Vec::new(),
            last_line_height: px(0.),
            last_bounds: None,
            is_selecting: false,
        }
    }

    /// Creates an empty fixed-`rows` multi-line input (S3-T18 Composer):
    /// Enter inserts a newline, paste preserves line breaks.
    pub fn new_multiline(
        cx: &mut Context<Self>,
        placeholder: impl Into<SharedString>,
        rows: usize,
    ) -> Self {
        let mut input = Self::new(cx, placeholder, false);
        input.multiline = true;
        input.rows = rows.max(1);
        input
    }

    /// The current content. Masked fields expose it only here (callers must
    /// never render it back into the UI).
    pub fn text(&self) -> &str {
        &self.content
    }

    /// Clears content and selection (used to reset the add-provider form).
    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.content = "".into();
        self.selected_range = 0..0;
        self.selection_reversed = false;
        self.marked_range = None;
        cx.notify();
    }

    /// Replaces the whole content and collapses the selection to the end
    /// (used to seed the T13 inline rename editor with the current title).
    pub fn set_text(&mut self, text: &str, cx: &mut Context<Self>) {
        self.content = text.to_string().into();
        self.selected_range = self.content.len()..self.content.len();
        self.selection_reversed = false;
        self.marked_range = None;
        cx.notify();
    }

    /// Text as painted: the content itself, or one bullet per character.
    fn display_text(&self) -> SharedString {
        if self.masked && !self.content.is_empty() {
            self.content
                .chars()
                .map(|_| MASK_CHAR)
                .collect::<String>()
                .into()
        } else {
            self.content.clone()
        }
    }

    /// Maps a content byte offset to the byte offset in the painted text.
    ///
    /// Tolerant of out-of-range or mid-character offsets: the platform may
    /// query arbitrary indices, and paint must never panic.
    fn display_offset_for_content_offset(&self, offset: usize) -> usize {
        if self.masked {
            self.content
                .char_indices()
                .take_while(|(index, _)| *index < offset)
                .count()
                * MASK_CHAR.len_utf8()
        } else {
            offset
        }
    }

    /// Maps a byte offset in the painted text back to a content byte offset.
    ///
    /// The masked display is a uniform run of [`MASK_CHAR`] (3 UTF-8 bytes
    /// each), so the display offset determines the character index directly.
    fn content_offset_for_display_offset(&self, offset: usize) -> usize {
        if !self.masked {
            return offset;
        }
        let char_index = (offset / MASK_CHAR.len_utf8()).min(self.content.chars().count());
        self.content
            .char_indices()
            .nth(char_index)
            .map_or(self.content.len(), |(byte, _)| byte)
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.previous_boundary(self.cursor_offset()), cx);
        } else {
            self.move_to(self.selected_range.start, cx);
        }
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.next_boundary(self.selected_range.end), cx);
        } else {
            self.move_to(self.selected_range.end, cx);
        }
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.previous_boundary(self.cursor_offset()), cx);
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_boundary(self.selected_range.end), cx);
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
        self.select_to(self.content.len(), cx);
    }

    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.content.len(), cx);
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let prev = self.previous_boundary(self.cursor_offset());
            if self.cursor_offset() == prev {
                window.play_system_bell();
                return;
            }
            self.select_to(prev, cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let next = self.next_boundary(self.cursor_offset());
            if self.cursor_offset() == next {
                window.play_system_bell();
                return;
            }
            self.select_to(next, cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus_handle, cx);
        self.is_selecting = true;
        if event.modifiers.shift {
            self.select_to(self.index_for_mouse_position(event.position), cx);
        } else {
            self.move_to(self.index_for_mouse_position(event.position), cx);
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _window: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting {
            self.select_to(self.index_for_mouse_position(event.position), cx);
        }
    }

    fn show_character_palette(
        &mut self,
        _: &ShowCharacterPalette,
        window: &mut Window,
        _: &mut Context<Self>,
    ) {
        window.show_character_palette();
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            // Multi-line keeps line breaks (normalized to \n); single-line
            // inputs flatten them.
            let text = if self.multiline {
                text.replace("\r\n", "\n")
            } else {
                text.replace('\n', " ")
            };
            self.replace_text_in_range(None, &text, window, cx);
        }
    }

    /// Enter in a multi-line input inserts a newline (Composer 裁定：
    /// Enter=换行；发送走 Cmd+Enter / 发送按钮). Single-line inputs have no
    /// newline concept — ring the bell.
    fn insert_newline(&mut self, _: &InsertNewline, window: &mut Window, cx: &mut Context<Self>) {
        if !self.multiline {
            window.play_system_bell();
            return;
        }
        self.replace_text_in_range(None, "\n", window, cx);
    }

    fn copy(&mut self, _: &Copy, window: &mut Window, cx: &mut Context<Self>) {
        // Masked (key) fields never leave the app: refuse copying credentials.
        if self.masked {
            window.play_system_bell();
            return;
        }
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
        }
    }

    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        // Masked (key) fields never leave the app: refuse cutting credentials.
        if self.masked {
            window.play_system_bell();
            return;
        }
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
            self.replace_text_in_range(None, "", window, cx);
        }
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.selected_range = offset..offset;
        cx.notify();
    }

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        if self.content.is_empty() {
            return 0;
        }
        let Some(bounds) = self.last_bounds.as_ref() else {
            return 0;
        };
        if self.last_lines.is_empty() || self.last_line_height <= px(0.) {
            return 0;
        }
        if position.y < bounds.top() {
            return 0;
        }
        if position.y > bounds.bottom() {
            return self.content.len();
        }
        let rel_y = position.y - bounds.top();
        if rel_y >= self.last_line_height * self.last_lines.len() as f32 {
            // Below the last laid-out line (clipped overflow area): end of
            // the line's text.
            if let Some(last) = self.last_lines.last() {
                return last.start + last.len;
            }
            return 0;
        }
        let Some(layout) = self.layout_for_y(rel_y, self.last_line_height) else {
            return 0;
        };
        // The layout is over the painted text, so map back through the mask.
        let display_offset = layout.line.closest_index_for_x(position.x - bounds.left());
        let local = self.content_offset_for_display_offset(display_offset);
        let offset = layout.start + local;
        if self.masked {
            offset.min(self.content.len())
        } else {
            offset.min(layout.start + layout.len)
        }
    }

    /// The display line containing `y` (relative to the element top), clamped
    /// into the laid-out range.
    fn layout_for_y(&self, y: Pixels, line_height: Pixels) -> Option<&LineLayout> {
        let row = (f32::from(y) / f32::from(line_height)).floor().max(0.0) as usize;
        self.last_lines
            .get(row.min(self.last_lines.len().saturating_sub(1)))
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        if self.selection_reversed {
            self.selected_range.start = offset;
        } else {
            self.selected_range.end = offset;
        }
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        cx.notify();
    }

    fn offset_from_utf16(&self, offset: usize) -> usize {
        let mut utf8_offset = 0;
        let mut utf16_count = 0;
        for ch in self.content.chars() {
            if utf16_count >= offset {
                break;
            }
            utf16_count += ch.len_utf16();
            utf8_offset += ch.len_utf8();
        }
        utf8_offset
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        let mut utf16_offset = 0;
        let mut utf8_count = 0;
        for ch in self.content.chars() {
            if utf8_count >= offset {
                break;
            }
            utf8_count += ch.len_utf8();
            utf16_offset += ch.len_utf16();
        }
        utf16_offset
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range_utf16: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range_utf16.start)..self.offset_from_utf16(range_utf16.end)
    }

    /// Clamps a byte offset into the content, rounding down to the nearest
    /// character boundary, so platform-supplied indices never slice
    /// mid-character or past the end.
    fn clamp_offset(&self, offset: usize) -> usize {
        if offset >= self.content.len() {
            return self.content.len();
        }
        let mut boundary = 0;
        for (index, _) in self.content.char_indices() {
            if index > offset {
                break;
            }
            boundary = index;
        }
        boundary
    }

    /// Clamps a byte range into the content on character boundaries.
    fn clamp_range(&self, range: Range<usize>) -> Range<usize> {
        let start = self.clamp_offset(range.start);
        let end = self.clamp_offset(range.end);
        start..start.max(end)
    }

    // Character boundaries instead of the example's grapheme boundaries: no
    // unicode-segmentation dependency is allowed for this crate (T08 ruling).
    // Both helpers tolerate out-of-range offsets (they are fed selection
    // state that the platform may have written between our edits).
    fn previous_boundary(&self, offset: usize) -> usize {
        self.content
            .char_indices()
            .rev()
            .find(|(index, _)| *index < offset)
            .map_or(0, |(index, _)| index)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.content
            .char_indices()
            .find(|(index, _)| *index > offset)
            .map_or(self.content.len(), |(index, _)| index)
    }
}

impl Focusable for TextInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EntityInputHandler for TextInput {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        Some(self.content[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.marked_range = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or(self.marked_range.clone())
            .unwrap_or_else(|| self.selected_range.clone());
        let range = self.clamp_range(range);

        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..])
                .into();
        self.selected_range = range.start + new_text.len()..range.start + new_text.len();
        self.marked_range.take();
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or(self.marked_range.clone())
            .unwrap_or_else(|| self.selected_range.clone());
        let range = self.clamp_range(range);

        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..])
                .into();
        if !new_text.is_empty() {
            self.marked_range = Some(range.start..range.start + new_text.len());
        } else {
            self.marked_range = None;
        }
        // Both ends of the new selection are relative to the *start* of the
        // replaced range, matching the platform contract (see Zed's editor
        // InputHandler); the gpui input example maps the end against
        // `range.end` instead, which corrupts the selection whenever a
        // non-empty marked range is replaced (e.g. IME recomposition) and
        // would panic in the masked display mapping below.
        self.selected_range = new_selected_range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .map(|new_range| range.start + new_range.start..range.start + new_range.end)
            .unwrap_or_else(|| range.start + new_text.len()..range.start + new_text.len());
        self.selected_range = self.clamp_range(self.selected_range.clone());

        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let range = self.range_from_utf16(&range_utf16);
        // The line containing the range start (masked single-line inputs have
        // exactly one line starting at 0).
        let row = self
            .last_lines
            .iter()
            .rposition(|layout| range.start >= layout.start)?;
        let layout = &self.last_lines[row];
        let line_top = bounds.top() + self.last_line_height * row as f32;
        // The layout is over the painted text: map through the mask first,
        // then back out of the line-local offset (multiline is unmasked, so
        // the mapping is the identity there).
        let local_start = (range.start - layout.start).min(layout.len);
        let local_end = (range.end - layout.start).min(layout.len).max(local_start);
        let display_start = self.display_offset_for_content_offset(layout.start + local_start);
        let display_end = self.display_offset_for_content_offset(layout.start + local_end);
        Some(Bounds::from_corners(
            point(
                bounds.left() + layout.line.x_for_index(display_start - layout.start),
                line_top,
            ),
            point(
                bounds.left() + layout.line.x_for_index(display_end - layout.start),
                line_top + self.last_line_height,
            ),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let line_point = self.last_bounds?.localize(&point)?;
        let layout = self.layout_for_y(line_point.y, self.last_line_height)?;
        // The layout is over the painted text: map through the mask first.
        let display_index = layout.line.index_for_x(line_point.x)?;
        Some(
            self.offset_to_utf16(
                layout.start + self.content_offset_for_display_offset(display_index),
            ),
        )
    }
}

/// The raw text element: shapes and paints the (possibly masked) line, the
/// selection, and the cursor, and registers the platform input handler.
struct TextElement {
    input: Entity<TextInput>,
}

struct PrepaintState {
    lines: Vec<LineLayout>,
    line_height: Pixels,
    cursor: Option<PaintQuad>,
    selections: Vec<PaintQuad>,
}

impl IntoElement for TextElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        // One text line per row: single-line inputs are one row tall; the
        // multi-line Composer input is a fixed row count (task card, T18).
        let rows = self.input.read(cx).rows as f32;
        style.size.height = (window.line_height() * rows).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let input = self.input.read(cx);
        let selected_range = input.selected_range.clone();
        let cursor = input.cursor_offset();
        let style = window.text_style();

        // Never paint the real value of a masked field.
        let (display_text, text_color) = if input.content.is_empty() {
            (
                input.placeholder.clone(),
                theme(cx).colors.text_tertiary.into(),
            )
        } else {
            (input.display_text(), style.color)
        };

        let run = TextRun {
            len: display_text.len(),
            font: style.font(),
            color: text_color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line_height = window.line_height();

        // Per-line layout. Single-line keeps the historical mask-aware path;
        // multi-line (unmasked by construction — masked fields are single
        // -line only) lays out each `\n` segment independently.
        let mut lines: Vec<LineLayout> = Vec::new();
        if input.multiline {
            let mut start = 0;
            for segment in display_text.split('\n') {
                let len = segment.len();
                let runs: Vec<TextRun> = match input.marked_range.as_ref() {
                    // Runs must cover exactly this segment (not the whole
                    // display text — shape_line slices the segment by lens).
                    None => vec![TextRun { len, ..run.clone() }],
                    Some(marked) => [
                        TextRun {
                            len: marked.start.saturating_sub(start).min(len),
                            ..run.clone()
                        },
                        TextRun {
                            len: marked.end.saturating_sub(start).min(len)
                                - marked.start.saturating_sub(start).min(len),
                            underline: Some(UnderlineStyle {
                                color: Some(run.color),
                                thickness: px(1.0),
                                wavy: false,
                            }),
                            ..run.clone()
                        },
                        TextRun {
                            len: len - marked.end.saturating_sub(start).min(len),
                            ..run.clone()
                        },
                    ]
                    .into_iter()
                    .filter(|run| run.len > 0)
                    .collect(),
                };
                // Empty segments (blank lines) shape with a zero-length run.
                let runs = if runs.is_empty() {
                    vec![TextRun {
                        len: 0,
                        ..run.clone()
                    }]
                } else {
                    runs
                };
                let shaped = window.text_system().shape_line(
                    SharedString::from(segment),
                    font_size,
                    &runs,
                    None,
                );
                lines.push(LineLayout {
                    start,
                    len,
                    line: shaped,
                });
                start += len + 1;
            }
        } else {
            let runs = if let Some(marked_range) = input.marked_range.as_ref() {
                let marked_start = input.display_offset_for_content_offset(marked_range.start);
                let marked_end = input.display_offset_for_content_offset(marked_range.end);
                vec![
                    TextRun {
                        len: marked_start,
                        ..run.clone()
                    },
                    TextRun {
                        len: marked_end - marked_start,
                        underline: Some(UnderlineStyle {
                            color: Some(run.color),
                            thickness: px(1.0),
                            wavy: false,
                        }),
                        ..run.clone()
                    },
                    TextRun {
                        len: display_text.len() - marked_end,
                        ..run
                    },
                ]
                .into_iter()
                .filter(|run| run.len > 0)
                .collect()
            } else {
                vec![run]
            };
            let shaped =
                window
                    .text_system()
                    .shape_line(display_text.clone(), font_size, &runs, None);
            lines.push(LineLayout {
                start: 0,
                len: display_text.len(),
                line: shaped,
            });
        }

        // Selection: one quad per intersected line (multi-line selections
        // cover whole lines between the ends).
        let mut selections: Vec<PaintQuad> = Vec::new();
        for (row, layout) in lines.iter().enumerate() {
            let start = selected_range.start.max(layout.start);
            let end = selected_range.end.min(layout.start + layout.len);
            if start >= end {
                continue;
            }
            let x0 = layout.line.x_for_index(start - layout.start);
            let x1 = layout.line.x_for_index(end - layout.start);
            selections.push(fill(
                Bounds::new(
                    point(bounds.left() + x0, bounds.top() + line_height * row as f32),
                    size(x1 - x0, line_height),
                ),
                theme(cx).colors.bg_active,
            ));
        }

        // Cursor on the line containing the offset (identity mapping for
        // unmasked text; masked single-line maps through the mask).
        let cursor = if selected_range.is_empty() {
            lines
                .iter()
                .rposition(|layout| cursor >= layout.start)
                .map(|row| {
                    let layout = &lines[row];
                    let display_cursor = input.display_offset_for_content_offset(cursor);
                    let display_start = input.display_offset_for_content_offset(layout.start);
                    let x = layout.line.x_for_index(display_cursor - display_start);
                    fill(
                        Bounds::new(
                            point(bounds.left() + x, bounds.top() + line_height * row as f32),
                            size(px(2.), line_height),
                        ),
                        theme(cx).colors.accent,
                    )
                })
        } else {
            None
        };

        PrepaintState {
            lines,
            line_height,
            cursor,
            selections,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.input.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );
        for selection in prepaint.selections.drain(..) {
            window.paint_quad(selection);
        }
        let line_height = prepaint.line_height;
        for (row, layout) in prepaint.lines.iter().enumerate() {
            // Nothing sensible can be done about a paint failure mid-frame,
            // and this crate may not depend on a logger (T08 dependency
            // ruling), so the result is intentionally discarded.
            let _ = layout.line.paint(
                point(bounds.origin.x, bounds.origin.y + line_height * row as f32),
                line_height,
                TextAlign::Left,
                None,
                window,
                cx,
            );
        }

        if focus_handle.is_focused(window)
            && let Some(cursor) = prepaint.cursor.take()
        {
            window.paint_quad(cursor);
        }

        self.input.update(cx, |input, _cx| {
            input.last_lines = std::mem::take(&mut prepaint.lines);
            input.last_line_height = line_height;
            input.last_bounds = Some(bounds);
        });
    }
}

impl Render for TextInput {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme(cx).colors;
        // The multi-line Composer input renders bare: the composer card
        // around it supplies bg/border/rounding for the whole row.
        let mut container = div()
            .flex_1()
            .key_context("TextInput")
            .track_focus(&self.focus_handle)
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::show_character_palette))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::insert_newline))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move));
        if !self.multiline {
            container = container
                .bg(colors.bg_elevated)
                .border_1()
                .border_color(colors.border_subtle)
                .rounded_lg()
                .px_2()
                .py_1();
        } else {
            // Fixed-row viewport: content beyond `rows` lines is clipped
            // (自适应滚动后置，任务卡注明).
            container = container.overflow_hidden();
        }
        // Typography per UI spec §3: body text 13px / 1.55 line height.
        container
            .text_size(px(Typography::BODY))
            .line_height(relative(Typography::BODY_LINE_HEIGHT))
            .text_color(colors.text_primary)
            .child(TextElement { input: cx.entity() })
    }
}
