use super::*;

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
        self.update_logical_viewport();
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

        self.update_logical_viewport();

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
