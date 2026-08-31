use super::*;

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
            first_visible_row: 0,
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
        input.rows = rows.clamp(1, 8);
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
        self.rows = 1;
        self.first_visible_row = 0;
        cx.notify();
    }

    /// Replaces the whole content and collapses the selection to the end
    /// (used to seed the T13 inline rename editor with the current title).
    pub fn set_text(&mut self, text: &str, cx: &mut Context<Self>) {
        self.content = text.to_string().into();
        self.selected_range = self.content.len()..self.content.len();
        self.selection_reversed = false;
        self.marked_range = None;
        self.update_logical_viewport();
        cx.notify();
    }

    /// Number of currently visible rows (always 1..=8).
    pub fn visible_rows(&self) -> usize {
        self.rows
    }

    /// Whether Up should enter Composer history rather than move inside a
    /// multi-line draft.
    pub fn cursor_on_first_logical_line(&self) -> bool {
        !self.content[..self.cursor_offset()].contains('\n')
    }

    /// Whether history recall may consume Up at the current visual caret.
    pub fn cursor_allows_history(&self) -> bool {
        if self.first_visible_row > 0 {
            return false;
        }
        self.last_lines.first().map_or_else(
            || self.cursor_on_first_logical_line(),
            |line| self.cursor_offset() <= line.start + line.len,
        )
    }

    /// Byte range + body of a trailing `@token` at the caret (A2-12): the
    /// token starts after an `@` that sits at text start or right after
    /// whitespace, extends to the caret, and contains no whitespace.
    /// `None` when the caret is not completing an `@` token.
    pub fn trailing_at_query(&self) -> Option<(Range<usize>, String)> {
        let cursor = self.cursor_offset();
        let prefix = self.content.get(..cursor)?;
        let at = prefix.rfind('@')?;
        if at > 0 && !prefix[..at].ends_with(|ch: char| ch.is_whitespace()) {
            return None;
        }
        let query = &prefix[at + 1..];
        if query.chars().any(|ch: char| ch.is_whitespace()) {
            return None;
        }
        Some((at + 1..cursor, query.to_string()))
    }

    /// Completes the trailing `@token` with `path`: the token body is
    /// replaced with `@path ` (the trailing space terminates the token so
    /// the selector closes deterministically). No-op without a trailing
    /// token (A2-12 completion seam).
    pub fn complete_at_query(&mut self, path: &str, cx: &mut Context<Self>) {
        let Some((range, _)) = self.trailing_at_query() else {
            return;
        };
        let replacement = format!("@{path} ");
        self.content =
            (self.content[..range.start].to_owned() + &replacement + &self.content[range.end..])
                .into();
        let end = range.start + replacement.len();
        self.selected_range = end..end;
        self.selection_reversed = false;
        self.update_logical_viewport();
        cx.notify();
    }

    pub(crate) fn update_logical_viewport(&mut self) {
        if !self.multiline {
            self.rows = 1;
            self.first_visible_row = 0;
            return;
        }
        let count = self.content.bytes().filter(|byte| *byte == b'\n').count() + 1;
        // Before the first layout, logical rows are the best safe estimate.
        // Afterwards the visual-wrap cache owns height until prepaint
        // measures the edited glyphs and requests another layout frame.
        if self.last_bounds.is_none() {
            self.rows = count.clamp(1, 8);
        }
        let cursor_row = self.content[..self.cursor_offset()]
            .bytes()
            .filter(|byte| *byte == b'\n')
            .count();
        if cursor_row < self.first_visible_row {
            self.first_visible_row = cursor_row;
        } else if cursor_row >= self.first_visible_row + 8 {
            self.first_visible_row = cursor_row + 1 - 8;
        }
        if self.last_bounds.is_none() {
            self.first_visible_row = self.first_visible_row.min(count.saturating_sub(self.rows));
        }
    }

    /// Text as painted: the content itself, or one bullet per character.
    pub(crate) fn display_text(&self) -> SharedString {
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
    pub(crate) fn display_offset_for_content_offset(&self, offset: usize) -> usize {
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
    pub(crate) fn content_offset_for_display_offset(&self, offset: usize) -> usize {
        if !self.masked {
            return offset;
        }
        let char_index = (offset / MASK_CHAR.len_utf8()).min(self.content.chars().count());
        self.content
            .char_indices()
            .nth(char_index)
            .map_or(self.content.len(), |(byte, _)| byte)
    }

    pub(crate) fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.previous_boundary(self.cursor_offset()), cx);
        } else {
            self.move_to(self.selected_range.start, cx);
        }
    }

    pub(crate) fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.next_boundary(self.selected_range.end), cx);
        } else {
            self.move_to(self.selected_range.end, cx);
        }
    }

    pub(crate) fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.previous_boundary(self.cursor_offset()), cx);
    }

    pub(crate) fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_boundary(self.selected_range.end), cx);
    }

    pub(crate) fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
        self.select_to(self.content.len(), cx);
    }

    pub(crate) fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
    }

    pub(crate) fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.content.len(), cx);
    }

    pub(crate) fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
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

    pub(crate) fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
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

    pub(crate) fn on_mouse_down(
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

    pub(crate) fn on_mouse_up(
        &mut self,
        _: &MouseUpEvent,
        _window: &mut Window,
        _: &mut Context<Self>,
    ) {
        self.is_selecting = false;
    }

    pub(crate) fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_selecting {
            self.select_to(self.index_for_mouse_position(event.position), cx);
        }
    }

    pub(crate) fn show_character_palette(
        &mut self,
        _: &ShowCharacterPalette,
        window: &mut Window,
        _: &mut Context<Self>,
    ) {
        window.show_character_palette();
    }

    pub(crate) fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
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
    pub(crate) fn insert_newline(
        &mut self,
        _: &InsertNewline,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.multiline {
            window.play_system_bell();
            return;
        }
        self.replace_text_in_range(None, "\n", window, cx);
    }

    pub(crate) fn copy(&mut self, _: &Copy, window: &mut Window, cx: &mut Context<Self>) {
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

    pub(crate) fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
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

    pub(crate) fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.selected_range = offset..offset;
        cx.notify();
    }

    pub(crate) fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    pub(crate) fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
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
    pub(crate) fn layout_for_y(&self, y: Pixels, line_height: Pixels) -> Option<&LineLayout> {
        let row = (f32::from(y) / f32::from(line_height)).floor().max(0.0) as usize;
        self.last_lines
            .get(row.min(self.last_lines.len().saturating_sub(1)))
    }

    pub(crate) fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
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

    pub(crate) fn offset_from_utf16(&self, offset: usize) -> usize {
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

    pub(crate) fn offset_to_utf16(&self, offset: usize) -> usize {
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

    pub(crate) fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    pub(crate) fn range_from_utf16(&self, range_utf16: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range_utf16.start)..self.offset_from_utf16(range_utf16.end)
    }

    /// Clamps a byte offset into the content, rounding down to the nearest
    /// character boundary, so platform-supplied indices never slice
    /// mid-character or past the end.
    pub(crate) fn clamp_offset(&self, offset: usize) -> usize {
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
    pub(crate) fn clamp_range(&self, range: Range<usize>) -> Range<usize> {
        let start = self.clamp_offset(range.start);
        let end = self.clamp_offset(range.end);
        start..start.max(end)
    }

    // Character boundaries instead of the example's grapheme boundaries: no
    // unicode-segmentation dependency is allowed for this crate (T08 ruling).
    // Both helpers tolerate out-of-range offsets (they are fed selection
    // state that the platform may have written between our edits).
    pub(crate) fn previous_boundary(&self, offset: usize) -> usize {
        self.content
            .char_indices()
            .rev()
            .find(|(index, _)| *index < offset)
            .map_or(0, |(index, _)| index)
    }

    pub(crate) fn next_boundary(&self, offset: usize) -> usize {
        self.content
            .char_indices()
            .find(|(index, _)| *index > offset)
            .map_or(self.content.len(), |(index, _)| index)
    }
}
