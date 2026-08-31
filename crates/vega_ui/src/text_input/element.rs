use super::*;

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
    visible_rows: usize,
    first_visible_row: usize,
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

        // Per-visual-line layout. Multi-line content is shaped and split at
        // the available pixel width, so Latin and CJK drafts both drive the
        // 1..8 row viewport from actual glyph advances.
        let mut lines: Vec<LineLayout> = Vec::new();
        if input.multiline {
            let mut start = 0;
            for segment in display_text.split('\n') {
                let len = segment.len();
                let mut local_start = 0;
                loop {
                    let remainder = &segment[local_start..];
                    let global_start = start + local_start;
                    let make_runs = |chunk_len: usize| -> Vec<TextRun> {
                        let runs = match input.marked_range.as_ref() {
                            None => vec![TextRun {
                                len: chunk_len,
                                ..run.clone()
                            }],
                            Some(marked) => {
                                let marked_start =
                                    marked.start.saturating_sub(global_start).min(chunk_len);
                                let marked_end =
                                    marked.end.saturating_sub(global_start).min(chunk_len);
                                [
                                    TextRun {
                                        len: marked_start,
                                        ..run.clone()
                                    },
                                    TextRun {
                                        len: marked_end.saturating_sub(marked_start),
                                        underline: Some(UnderlineStyle {
                                            color: Some(run.color),
                                            thickness: px(1.0),
                                            wavy: false,
                                        }),
                                        ..run.clone()
                                    },
                                    TextRun {
                                        len: chunk_len.saturating_sub(marked_end),
                                        ..run.clone()
                                    },
                                ]
                                .into_iter()
                                .filter(|run| run.len > 0)
                                .collect()
                            }
                        };
                        if runs.is_empty() {
                            vec![TextRun {
                                len: 0,
                                ..run.clone()
                            }]
                        } else {
                            runs
                        }
                    };
                    let initial_runs = make_runs(remainder.len());
                    let initial = window.text_system().shape_line(
                        SharedString::from(remainder),
                        font_size,
                        &initial_runs,
                        None,
                    );
                    let cut = if initial.width() <= bounds.size.width || remainder.is_empty() {
                        remainder.len()
                    } else {
                        let nearest = initial.closest_index_for_x(bounds.size.width);
                        if nearest == 0 {
                            remainder
                                .char_indices()
                                .nth(1)
                                .map_or(remainder.len(), |(index, _)| index)
                        } else {
                            nearest.min(remainder.len())
                        }
                    };
                    let shaped = if cut == remainder.len() {
                        initial
                    } else {
                        let chunk = &remainder[..cut];
                        window.text_system().shape_line(
                            SharedString::from(chunk),
                            font_size,
                            &make_runs(chunk.len()),
                            None,
                        )
                    };
                    lines.push(LineLayout {
                        start: global_start,
                        len: cut,
                        line: shaped,
                    });
                    local_start += cut;
                    if local_start >= len {
                        break;
                    }
                }
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

        let (visible_rows, first_visible_row) = if input.multiline {
            let total = lines.len().max(1);
            let visible = total.clamp(1, 8);
            let cursor_row = lines
                .iter()
                .rposition(|layout| cursor >= layout.start)
                .unwrap_or_default();
            let mut first = input.first_visible_row.min(total.saturating_sub(visible));
            if cursor_row < first {
                first = cursor_row;
            } else if cursor_row >= first + visible {
                first = cursor_row + 1 - visible;
            }
            lines = lines.into_iter().skip(first).take(visible).collect();
            (visible, first)
        } else {
            (1, 0)
        };

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
            visible_rows,
            first_visible_row,
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

        self.input.update(cx, |input, cx| {
            let layout_changed = input.rows != prepaint.visible_rows
                || input.first_visible_row != prepaint.first_visible_row;
            input.last_lines = std::mem::take(&mut prepaint.lines);
            input.last_line_height = line_height;
            input.last_bounds = Some(bounds);
            input.rows = prepaint.visible_rows;
            input.first_visible_row = prepaint.first_visible_row;
            if layout_changed {
                cx.notify();
            }
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
