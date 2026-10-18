use super::mouse_selection::{MouseSelection, SelectionGranularity};
use super::*;

impl TextInput {
    /// Creates an empty single-line input with a placeholder; `masked`
    /// renders bullets.
    pub fn new(cx: &mut Context<Self>, placeholder: impl Into<SharedString>, masked: bool) -> Self {
        Self {
            image_paste: false,
            focus_handle: cx.focus_handle(),
            content: "".into(),
            content_revision: 0,
            prediction: None,
            placeholder: placeholder.into(),
            masked,
            multiline: false,
            rows: 1,
            min_rows: 1,
            max_rows: 1,
            first_visible_row: 0,
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            last_lines: Vec::new(),
            last_line_height: px(0.),
            last_bounds: None,
            mouse_selection: None,
            bare_chrome: false,
            editor_scroll: None,
            scrollable_editor: false,
            scroll_fade_on_base: false,
            total_visual_rows: 1,
        }
    }

    pub fn new_multiline(
        cx: &mut Context<Self>,
        placeholder: impl Into<SharedString>,
        rows: usize,
    ) -> Self {
        let mut input = Self::new(cx, placeholder, false);
        input.multiline = true;
        input.rows = rows.clamp(1, 8);
        input.max_rows = 8;
        input
    }

    /// Sets the minimum and maximum visible rows for a multi-line field.
    ///
    /// The default range for [`Self::new_multiline`] is 1..=8, matching the
    /// Composer. A bounded form can reserve a minimum number of complete rows
    /// and cap its viewport without changing input, paste, or keyboard
    /// semantics.
    pub fn with_row_bounds(mut self, min_rows: usize, max_rows: usize) -> Self {
        let max_rows = max_rows.clamp(1, 8);
        let min_rows = min_rows.clamp(1, max_rows);
        self.min_rows = min_rows;
        self.max_rows = max_rows;
        self.rows = self.rows.clamp(min_rows, max_rows);
        self
    }

    /// Marks this input as reachable by the window's Tab traversal.
    pub fn with_tab_stop(mut self, tab_stop: bool) -> Self {
        self.focus_handle = self.focus_handle.tab_stop(tab_stop);
        self
    }

    pub fn with_editor_viewport(mut self) -> Self {
        self.scrollable_editor = true;
        self.scroll_fade_on_base = true;
        self.min_rows = 1;
        self.max_rows = 24;
        self
    }

    pub(crate) fn with_scroll_fade_on_base(mut self) -> Self {
        self.scroll_fade_on_base = true;
        self
    }

    pub fn with_bare_chrome(mut self) -> Self {
        self.bare_chrome = true;
        self
    }

    /// The current content. Masked fields expose it only here (callers must
    /// never render it back into the UI).
    pub fn text(&self) -> &str {
        &self.content
    }

    /// Clears content and selection (used to reset the add-provider form).
    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.prediction = None;
        self.content_revision = self.content_revision.wrapping_add(1);
        self.content = "".into();
        self.selected_range = 0..0;
        self.selection_reversed = false;
        self.marked_range = None;
        self.rows = self.min_rows;
        self.first_visible_row = 0;
        cx.notify();
    }

    pub fn set_text(&mut self, text: &str, cx: &mut Context<Self>) {
        self.prediction = None;
        self.content_revision = self.content_revision.wrapping_add(1);
        self.content = text.to_string().into();
        self.selected_range = self.content.len()..self.content.len();
        self.selection_reversed = false;
        self.marked_range = None;
        self.update_logical_viewport();
        cx.notify();
    }

    pub fn prediction_text(&self) -> Option<&str> {
        self.prediction
            .as_deref()
            .filter(|_| self.content.is_empty() && !self.is_composing())
    }

    pub(crate) fn set_prediction(&mut self, suggestion: Option<String>, cx: &mut Context<Self>) {
        let prediction = suggestion
            .filter(|_| self.content.is_empty() && !self.is_composing())
            .map(SharedString::from);
        if self.prediction != prediction {
            self.prediction = prediction;
            cx.notify();
        }
    }

    pub(crate) fn accept_prediction(
        &mut self,
        _: &AcceptPrediction,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.prediction_text().is_none() {
            cx.propagate();
            return;
        }
        if let Some(suggestion) = self.prediction.take() {
            self.set_text(&suggestion, cx);
        }
    }

    pub(crate) fn dismiss_prediction(
        &mut self,
        _: &DismissPrediction,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.prediction.take().is_some() {
            self.content_revision = self.content_revision.wrapping_add(1);
            cx.notify();
        } else {
            cx.propagate();
        }
    }

    pub(crate) fn content_revision(&self) -> u64 {
        self.content_revision
    }

    /// Whether a platform IME composition is still active.
    pub fn is_composing(&self) -> bool {
        self.marked_range.is_some()
    }

    /// Number of currently visible rows (always within the configured range).
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

    pub fn complete_at_query(&mut self, path: &str, cx: &mut Context<Self>) {
        let Some((range, _)) = self.trailing_at_query() else {
            return;
        };
        let replacement = format!("{path} ");
        self.prediction = None;
        self.content_revision = self.content_revision.wrapping_add(1);
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
        let logical_rows = count.clamp(self.min_rows, self.max_rows);
        // Keep new explicit lines visible to the very next request-layout
        // pass. Once the input has been measured, retain the measured visual
        // viewport until prepaint can refine it; assigning `logical_rows`
        // unconditionally would briefly collapse a soft-wrapped draft on
        // every edit. An empty edit still shrinks immediately via `clear`,
        // while ordinary deletion is refined by the next prepaint pass.
        if self.last_bounds.is_none() {
            self.rows = logical_rows;
        } else {
            self.rows = self.rows.max(logical_rows).min(self.max_rows);
        }
        let cursor_row = self.content[..self.cursor_offset()]
            .bytes()
            .filter(|byte| *byte == b'\n')
            .count();
        if cursor_row < self.first_visible_row {
            self.first_visible_row = cursor_row;
        } else if cursor_row >= self.first_visible_row + self.max_rows {
            self.first_visible_row = cursor_row + 1 - self.max_rows;
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
        gpui_kit::base::GlobalState::init(cx);
        gpui_kit::base::GlobalState::suppress_text_selection(cx);
        window.focus(&self.focus_handle, cx);
        if event.modifiers.shift {
            let anchor = if self.selection_reversed {
                self.selected_range.end
            } else {
                self.selected_range.start
            };
            self.select_to(self.index_for_mouse_position(event.position), cx);
            self.mouse_selection = Some(MouseSelection {
                anchor: anchor..anchor,
                granularity: SelectionGranularity::Character,
                content_revision: self.content_revision,
            });
        } else {
            let granularity = match event.click_count {
                0 | 1 => SelectionGranularity::Character,
                2 => SelectionGranularity::Word,
                _ => SelectionGranularity::Line,
            };
            let offset = self.mouse_index_for_position(
                event.position,
                !matches!(granularity, SelectionGranularity::Character),
            );
            let range = granularity.range(&self.content, offset);
            self.selected_range = range.clone();
            self.selection_reversed = false;
            self.mouse_selection = Some(MouseSelection {
                anchor: range,
                granularity,
                content_revision: self.content_revision,
            });
            cx.notify();
        }
    }

    pub(crate) fn on_mouse_up(
        &mut self,
        _: &MouseUpEvent,
        _window: &mut Window,
        _: &mut Context<Self>,
    ) {
        self.mouse_selection = None;
    }

    pub(crate) fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.pressed_button != Some(MouseButton::Left) {
            self.mouse_selection = None;
            return;
        }
        let Some(selection) = self.mouse_selection.as_ref() else {
            return;
        };
        if selection.content_revision != self.content_revision {
            self.mouse_selection = None;
            return;
        }
        let offset = self.mouse_index_for_position(
            event.position,
            !matches!(selection.granularity, SelectionGranularity::Character),
        );
        let unit = selection.granularity.range(&self.content, offset);
        (self.selected_range, self.selection_reversed) = selection.extend(unit);
        cx.notify();
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
        let item = cx.read_from_clipboard();
        if self.image_paste
            && item.as_ref().is_some_and(|item| {
                item.entries().iter().any(|entry| {
                    matches!(
                        entry,
                        gpui_kit::ClipboardEntry::Image(_)
                            | gpui_kit::ClipboardEntry::ExternalPaths(_)
                    )
                })
            })
        {
            if let Some(item) = item {
                if !item
                    .entries()
                    .iter()
                    .any(|entry| matches!(entry, gpui_kit::ClipboardEntry::ExternalPaths(_)))
                    && let Some(text) = item.text()
                {
                    self.replace_text_in_range(None, &text.replace("\r\n", "\n"), window, cx);
                }
                cx.emit(ImagePaste(std::sync::Arc::new(item)));
            }
            return;
        }
        if let Some(text) = item.and_then(|item| item.text()) {
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
        self.mouse_index_for_position(position, false)
    }

    fn mouse_index_for_position(&self, position: Point<Pixels>, glyph: bool) -> usize {
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
        let x = position.x - bounds.left();
        let display_offset = if glyph {
            layout.line.index_for_x(x).unwrap_or_else(|| {
                layout
                    .line
                    .text
                    .char_indices()
                    .next_back()
                    .map_or(0, |(index, _)| index)
            })
        } else {
            layout.line.closest_index_for_x(x)
        };
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
