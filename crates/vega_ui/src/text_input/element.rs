use super::*;
use crate::scroll_edge_fade::{ScrollFade, ScrollFadeMetrics};
use gpui_kit::base::{Scrollbar, ScrollbarHandle, ScrollbarMode};
use std::{cell::RefCell, rc::Rc};
use vega_theme::Layout;

#[derive(Clone, Default)]
pub(super) struct TextScrollHandle(Rc<RefCell<TextScrollState>>);

#[derive(Default)]
struct TextScrollState {
    bounds: Bounds<Pixels>,
    line_height: Pixels,
    rows: usize,
    total_rows: usize,
    first_row: usize,
    selection: Range<usize>,
    revision: u64,
    pending: Option<(Range<usize>, u64, usize)>,
}

impl TextScrollHandle {
    pub(super) fn reset(&self) {
        let mut state = self.0.borrow_mut();
        state.first_row = 0;
        state.pending = None;
    }
}

impl ScrollbarHandle for TextScrollHandle {
    fn viewport_bounds(&self) -> Bounds<Pixels> {
        self.0.borrow().bounds
    }

    fn offset(&self) -> Point<Pixels> {
        let state = self.0.borrow();
        point(px(0.), -state.line_height * state.first_row as f32)
    }

    fn set_offset(&self, offset: Point<Pixels>) {
        let mut state = self.0.borrow_mut();
        if state.line_height <= px(0.) {
            return;
        }
        let first = (f32::from(-offset.y) / f32::from(state.line_height))
            .round()
            .max(0.) as usize;
        state.first_row = first.min(state.total_rows.saturating_sub(state.rows));
        state.pending = Some((state.selection.clone(), state.revision, state.first_row));
    }

    fn content_size(&self) -> gpui_kit::Size<Pixels> {
        let state = self.0.borrow();
        size(
            state.bounds.size.width,
            state.line_height * state.total_rows as f32,
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
    navigation_lines: Vec<LineLayout>,
    content_revision: u64,
    line_height: Pixels,
    cursor: Option<PaintQuad>,
    selections: Vec<PaintQuad>,
    visible_rows: usize,
    first_visible_row: usize,
    total_visual_rows: usize,
    editor_scroll: Option<(Range<usize>, u64, usize)>,
    fade: ScrollFadeMetrics,
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
        // One text line per row: single-line inputs are one row tall; a
        // multi-line input uses its bounded visible-row viewport.
        let input = self.input.read(cx);
        style.size.height = if input.scrollable_editor {
            relative(1.).into()
        } else {
            (window.line_height() * input.rows as f32).into()
        };
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
        let display_selection = input.display_offset_for_content_offset(selected_range.start)
            ..input.display_offset_for_content_offset(selected_range.end);
        let cursor = input.cursor_offset();
        let style = window.text_style();

        // Never paint the real value of a masked field.
        let (display_text, text_color) = if input.content.is_empty() {
            (
                input
                    .prediction_text()
                    .map_or_else(|| input.localized_placeholder(), SharedString::from),
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

        let accent = theme(cx).colors.accent;
        let mention_spans: Vec<Range<usize>> = if input.masked {
            Vec::new()
        } else {
            input
                .mentions()
                .iter()
                .map(|mention| mention.range())
                .collect()
        };
        let marked = input.marked_range.clone();
        let base_run = run.clone();
        let build_runs = move |global_start: usize, chunk_len: usize| -> Vec<TextRun> {
            let chunk_end = global_start + chunk_len;
            let mut bounds = vec![0usize, chunk_len];
            for span in mention_spans.iter().chain(marked.iter()) {
                if span.start > global_start && span.start < chunk_end {
                    bounds.push(span.start - global_start);
                }
                if span.end > global_start && span.end < chunk_end {
                    bounds.push(span.end - global_start);
                }
            }
            bounds.sort_unstable();
            bounds.dedup();
            if bounds.len() < 2 {
                return vec![TextRun {
                    len: 0,
                    ..base_run.clone()
                }];
            }
            let mut runs = Vec::with_capacity(bounds.len() - 1);
            for pair in bounds.windows(2) {
                let (start, end) = (pair[0], pair[1]);
                let absolute_start = global_start + start;
                let absolute_end = global_start + end;
                let in_mention = mention_spans
                    .iter()
                    .any(|span| span.start <= absolute_start && absolute_end <= span.end);
                let in_marked = marked
                    .as_ref()
                    .is_some_and(|span| span.start <= absolute_start && absolute_end <= span.end);
                runs.push(TextRun {
                    len: end - start,
                    color: if in_mention {
                        accent.into()
                    } else {
                        base_run.color
                    },
                    underline: in_marked.then_some(UnderlineStyle {
                        color: Some(base_run.color),
                        thickness: px(1.0),
                        wavy: false,
                    }),
                    ..base_run.clone()
                });
            }
            runs
        };

        // Per-visual-line layout. Multi-line content is shaped and split at
        // the available pixel width, so Latin and CJK drafts both drive the
        // 1..8 row viewport from actual glyph advances.
        let shape_lines = |width: Pixels| {
            let mut lines: Vec<LineLayout> = Vec::new();
            if input.multiline {
                let mut start = 0;
                for segment in display_text.split('\n') {
                    let len = segment.len();
                    let mut local_start = 0;
                    loop {
                        let remainder = &segment[local_start..];
                        let global_start = start + local_start;
                        let initial_runs = build_runs(global_start, remainder.len());
                        let initial = window.text_system().shape_line(
                            SharedString::from(remainder),
                            font_size,
                            &initial_runs,
                            None,
                        );
                        let mut cut = if initial.width() <= width || remainder.is_empty() {
                            remainder.len()
                        } else {
                            let nearest = initial.closest_index_for_x(width);
                            let fitting = if initial.x_for_index(nearest) > width {
                                remainder[..nearest]
                                    .char_indices()
                                    .next_back()
                                    .map_or(0, |(index, _)| index)
                            } else {
                                nearest
                            };
                            if fitting == 0 {
                                remainder
                                    .char_indices()
                                    .nth(1)
                                    .map_or(remainder.len(), |(index, _)| index)
                            } else {
                                fitting.min(remainder.len())
                            }
                        };
                        let mut shaped = if cut == remainder.len() {
                            initial
                        } else {
                            let chunk = &remainder[..cut];
                            window.text_system().shape_line(
                                SharedString::from(chunk),
                                font_size,
                                &build_runs(global_start, cut),
                                None,
                            )
                        };
                        while shaped.width() > width {
                            let Some((previous, _)) = remainder[..cut].char_indices().next_back()
                            else {
                                break;
                            };
                            if previous == 0 {
                                break;
                            }
                            cut = previous;
                            shaped = window.text_system().shape_line(
                                SharedString::from(&remainder[..cut]),
                                font_size,
                                &build_runs(global_start, cut),
                                None,
                            );
                        }
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
                            ..run.clone()
                        },
                    ]
                    .into_iter()
                    .filter(|run| run.len > 0)
                    .collect()
                } else {
                    vec![run.clone()]
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

            lines
        };
        let mut lines = shape_lines(bounds.size.width);
        let ghost = input.prediction_text().is_some();
        if input.multiline && !input.scrollable_editor && !ghost && lines.len() > input.max_rows {
            lines = shape_lines(
                (bounds.size.width - px(Layout::DISCLOSURE_SCROLLBAR_GUTTER)).max(px(1.)),
            );
        }
        if ghost {
            lines.truncate(input.rows);
        }
        let navigation_lines = if input.multiline && !input.content.is_empty() {
            lines.clone()
        } else {
            Vec::new()
        };
        let total_visual_rows = lines.len().max(1);
        let manual_scroll = input
            .scroll_handle
            .0
            .borrow_mut()
            .pending
            .take()
            .or_else(|| input.editor_scroll.clone())
            .filter(|(selection, revision, _)| {
                *selection == input.selected_range && *revision == input.content_revision
            });
        let (visible_rows, first_visible_row) = if input.multiline {
            let total = lines.len().max(1);
            let visible = if input.scrollable_editor {
                (f32::from(bounds.size.height) / f32::from(line_height)).floor() as usize
            } else {
                total
            }
            .clamp(input.min_rows, input.max_rows);
            let cursor_row = lines
                .iter()
                .rposition(|layout| cursor >= layout.start)
                .unwrap_or_default();
            let mut first = input.first_visible_row.min(total.saturating_sub(visible));
            if let Some((_, _, start)) = &manual_scroll {
                first = (*start).min(total.saturating_sub(visible));
            } else if cursor_row < first {
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
            let start = display_selection.start.max(layout.start);
            let end = display_selection.end.min(layout.start + layout.len);
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
                gpui_kit::base::Theme::global(cx).tokens.colors.selection,
            ));
        }

        // Cursor on the line containing the offset (identity mapping for
        // unmasked text; masked single-line maps through the mask).
        let cursor = if selected_range.is_empty() {
            lines
                .iter()
                .rposition(|layout| cursor >= layout.start && cursor <= layout.start + layout.len)
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

        let mut fade_bounds = bounds;
        fade_bounds.size.height = bounds.size.height.min(line_height * visible_rows as f32);
        let horizontal_end = if input.multiline {
            px(0.)
        } else {
            lines.first().map_or(px(0.), |line| {
                (line.line.width() - bounds.size.width).max(px(0.))
            })
        };
        let mut fade = ScrollFadeMetrics::new(
            fade_bounds,
            point(px(0.), line_height * first_visible_row as f32),
            point(
                horizontal_end,
                line_height
                    * total_visual_rows.saturating_sub(first_visible_row + visible_rows) as f32,
            ),
        );
        if input.focus_handle.is_focused(window) {
            if let Some(cursor) = cursor.as_ref() {
                fade = fade.protect(cursor.bounds, ScrollFade::compact());
            }
            for selection in &selections {
                fade = fade.protect(selection.bounds, ScrollFade::compact());
            }
        }
        if let Some(marked) = input.marked_range.as_ref() {
            for (row, layout) in lines.iter().enumerate() {
                if marked.start <= layout.start + layout.len && marked.end >= layout.start {
                    fade = fade.protect(
                        Bounds::new(
                            point(bounds.left(), bounds.top() + line_height * row as f32),
                            size(bounds.size.width, line_height),
                        ),
                        ScrollFade::compact(),
                    );
                }
            }
        }

        {
            let mut state = input.scroll_handle.0.borrow_mut();
            state.bounds = bounds;
            state.line_height = line_height;
            state.rows = visible_rows;
            state.total_rows = total_visual_rows;
            state.first_row = first_visible_row;
            state.selection = input.selected_range.clone();
            state.revision = input.content_revision;
        }

        PrepaintState {
            navigation_lines,
            content_revision: input.content_revision,
            editor_scroll: manual_scroll
                .map(|(selection, revision, _)| (selection, revision, first_visible_row)),
            total_visual_rows,
            lines,
            line_height,
            cursor,
            selections,
            visible_rows,
            first_visible_row,
            fade,
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
        window.with_content_mask(Some(gpui_kit::ContentMask { bounds }), |window| {
            for selection in prepaint.selections.drain(..) {
                window.paint_quad(selection);
            }
            let line_height = prepaint.line_height;
            for (row, layout) in prepaint.lines.iter().enumerate() {
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
        });
        let line_height = prepaint.line_height;
        let colors = theme(cx).colors;
        let color = if self.input.read(cx).scroll_fade_on_base {
            colors.bg_base
        } else {
            colors.bg_elevated
        };
        window.with_content_mask(Some(gpui_kit::ContentMask { bounds }), |window| {
            prepaint.fade.paint(color, ScrollFade::compact(), window);
        });

        self.input.update(cx, |input, cx| {
            let layout_changed = input.rows != prepaint.visible_rows
                || input.first_visible_row != prepaint.first_visible_row
                || (input.total_visual_rows > input.rows)
                    != (prepaint.total_visual_rows > prepaint.visible_rows);
            input.last_lines = std::mem::take(&mut prepaint.lines);
            input.navigation_lines = std::mem::take(&mut prepaint.navigation_lines);
            input.navigation_revision = prepaint.content_revision;
            input.last_line_height = line_height;
            input.last_bounds = Some(bounds);
            input.rows = prepaint.visible_rows;
            input.first_visible_row = prepaint.first_visible_row;
            input.total_visual_rows = prepaint.total_visual_rows;
            input.editor_scroll = prepaint.editor_scroll.take();
            if layout_changed {
                cx.notify();
                // Rows are learned from shaped visual lines during prepaint.
                // Explicitly request the next frame so the parent composer
                // re-runs layout immediately after a paste/IME update rather
                // than waiting for another keyboard event.
                window.request_animation_frame();
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
            .when(self.prediction_text().is_some(), |input| {
                input.key_context("TextInput TextInputPrediction")
            })
            .track_focus(&self.focus_handle)
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::up))
            .on_action(cx.listener(Self::down))
            .on_action(cx.listener(Self::select_up))
            .on_action(cx.listener(Self::select_down))
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
            .on_action(cx.listener(Self::accept_prediction))
            .on_action(cx.listener(Self::dismiss_prediction))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .when(self.scrollable_editor, |input| input.size_full().min_h_0())
            .when(self.multiline, |input| {
                input.relative().on_scroll_wheel(cx.listener(
                    |this, event: &gpui_kit::ScrollWheelEvent, _, cx| {
                        if this.last_line_height <= px(0.) {
                            return;
                        }
                        let delta = event.delta.pixel_delta(this.last_line_height);
                        if delta.y == px(0.) || delta.x.abs() > delta.y.abs() {
                            return;
                        }
                        let rows = (delta.y.abs() / this.last_line_height).ceil() as usize;
                        let first = if delta.y > px(0.) {
                            this.first_visible_row.saturating_sub(rows)
                        } else {
                            this.first_visible_row.saturating_add(rows)
                        }
                        .min(this.total_visual_rows.saturating_sub(this.rows));
                        if first == this.first_visible_row {
                            return;
                        }
                        this.first_visible_row = first;
                        this.editor_scroll =
                            Some((this.selected_range.clone(), this.content_revision, first));
                        cx.stop_propagation();
                        cx.notify();
                    },
                ))
            });
        if !self.multiline {
            if !self.bare_chrome {
                container = container
                    .bg(colors.bg_elevated)
                    .border_1()
                    .border_color(colors.border_subtle)
                    .rounded_lg()
                    .px_2()
                    .py_1();
            }
        } else {
            container = container.overflow_hidden();
        }
        // Typography per UI spec §3: body text 13px / 1.55 line height.
        let container = container
            .text_size(px(Typography::BODY))
            .line_height(relative(Typography::BODY_LINE_HEIGHT))
            .text_color(colors.text_primary)
            .child(TextElement { input: cx.entity() })
            .when(
                self.multiline
                    && !self.scrollable_editor
                    && self.prediction_text().is_none()
                    && self.total_visual_rows > self.rows,
                |input| {
                    input.child(Scrollbar::vertical(&self.scroll_handle).mode(
                        if cx.should_auto_hide_scrollbars() {
                            ScrollbarMode::Hover
                        } else {
                            ScrollbarMode::Always
                        },
                    ))
                },
            );
        if !self.edit_menu {
            return container.into_any_element();
        }
        use gpui_kit::component::menu::{ContextMenuExt, PopupMenuItem};

        let input = cx.entity().downgrade();
        container
            .id("text-input-edit-menu")
            .context_menu(move |menu, _, cx| {
                let Some(input) = input.upgrade() else {
                    return menu;
                };
                let input = input.read(cx);
                let can_copy = !input.masked && !input.selected_range.is_empty();
                let edit_item =
                    |label: &'static str, action: Box<dyn gpui_kit::Action>, enabled: bool| {
                        let label = vega_i18n::t(label);
                        if enabled {
                            PopupMenuItem::new(label).action(action)
                        } else {
                            PopupMenuItem::Label(label.into())
                        }
                    };
                menu.action_context(input.focus_handle.clone())
                    .menu(vega_i18n::t("ui.paste"), Box::new(Paste))
                    .item(edit_item("ui.cut", Box::new(Cut), can_copy))
                    .item(edit_item("ui.copy", Box::new(Copy), can_copy))
                    .item(edit_item(
                        "ui.select_all",
                        Box::new(SelectAll),
                        !input.content.is_empty(),
                    ))
            })
            .into_any_element()
    }
}
