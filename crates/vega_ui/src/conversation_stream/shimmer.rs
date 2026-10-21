use super::*;
use gpui_kit::{ContentMask, DecorationRun, LineLayout, TextAlign, point};

#[cfg(test)]
mod logical_prefix_tests;
#[cfg(test)]
mod tests;

pub(crate) struct StageText {
    title: String,
    epoch: Option<Instant>,
}

impl StageText {
    pub(crate) fn new() -> Self {
        Self {
            title: String::new(),
            epoch: None,
        }
    }

    pub(crate) fn set(&mut self, title: String, epoch: Option<Instant>, cx: &mut Context<Self>) {
        if self.title != title || self.epoch != epoch {
            self.title = title;
            self.epoch = epoch;
            cx.notify();
        }
    }
}

impl Render for StageText {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(epoch) = self.epoch {
            stage_text(self.title.clone(), epoch, window, cx)
        } else {
            div().child(self.title.clone()).into_any_element()
        }
    }
}

pub(crate) fn text(text: String, epoch: Instant, window: &Window, cx: &App) -> AnyElement {
    let colors = theme(cx).colors;
    styled_text(
        text,
        epoch,
        Typography::CONVERSATION,
        Layout::CONVERSATION_TOOL_HEIGHT,
        colors.text_tertiary,
        colors.text_primary,
        window,
    )
}

pub(crate) fn stage_text(text: String, epoch: Instant, window: &Window, cx: &App) -> AnyElement {
    let colors = theme(cx).colors;
    styled_text(
        text,
        epoch,
        Typography::CONVERSATION,
        Layout::CONVERSATION_TOOL_HEIGHT,
        colors.conversation_stage,
        colors.bg_base,
        window,
    )
}

fn display_layout(
    text: &str,
    full: Arc<LineLayout>,
    available: Pixels,
    mut shape: impl FnMut(&str) -> Arc<LineLayout>,
) -> Option<(String, Arc<LineLayout>)> {
    if text.is_empty() || available <= px(0.) {
        return None;
    }
    let ellipsis = shape("…");
    if available < ellipsis.width {
        return None;
    }
    if full.width <= available + px(1.) {
        return Some((text.to_owned(), full));
    }
    if available == ellipsis.width {
        return Some(("…".to_owned(), ellipsis));
    }
    let mut boundaries = full
        .runs
        .iter()
        .flat_map(|run| run.glyphs.iter())
        .map(|glyph| glyph.index)
        .filter(|&index| index > 0 && index < text.len() && text.is_char_boundary(index))
        .collect::<Vec<_>>();
    boundaries.sort_unstable();
    boundaries.dedup();
    let mut measure_prefix = |index| {
        let mut candidate = text[..index].to_owned();
        candidate.push('…');
        let layout = shape(&candidate);
        (candidate, layout)
    };
    let mut best = ("…".to_owned(), ellipsis);
    let mut low = 0;
    let mut high = boundaries.len();
    if let Some(&index) = boundaries.first() {
        let (candidate, layout) = measure_prefix(index);
        if layout.width <= available {
            best = (candidate, layout);
        }
        low = 1;
    }
    while low < high {
        let mid = low + (high - low) / 2;
        let (candidate, layout) = measure_prefix(boundaries[mid]);
        if layout.width <= available {
            best = (candidate, layout);
            low = mid + 1;
        } else {
            high = mid;
        }
    }
    Some(best)
}

fn frame_decorations(
    text: &str,
    layout: &LineLayout,
    original_width: Pixels,
    phase: f32,
    base: Rgba,
    highlight: Rgba,
    moving: bool,
) -> Vec<DecorationRun> {
    let center = phase.fract() * 1.6 - 0.3;
    let width = f32::from(original_width.max(px(1.)));
    text.char_indices()
        .map(|(index, ch)| {
            let x = f32::from(layout.x_for_index(index)) / width;
            let strength = if moving {
                (1. - (x - center).abs() / 0.24).max(0.)
            } else {
                0.
            };
            DecorationRun {
                len: ch.len_utf8() as u32,
                color: Rgba {
                    r: base.r + (highlight.r - base.r) * strength,
                    g: base.g + (highlight.g - base.g) * strength,
                    b: base.b + (highlight.b - base.b) * strength,
                    a: base.a,
                }
                .into(),
                background_color: None,
                underline: None,
                strikethrough: None,
            }
        })
        .collect()
}

fn should_animate(
    layout: Option<&LineLayout>,
    visible: bool,
    active: bool,
    reduce_motion: bool,
) -> bool {
    visible
        && active
        && !reduce_motion
        && layout.is_some_and(|layout| layout.runs.iter().any(|run| !run.glyphs.is_empty()))
}

fn styled_text(
    text: String,
    epoch: Instant,
    font_size: f32,
    height: f32,
    base_color: Rgba,
    highlight: Rgba,
    window: &Window,
) -> AnyElement {
    let base = TextRun {
        len: text.len(),
        font: window.text_style().font(),
        color: gpui_kit::black(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let line =
        window
            .text_system()
            .layout_line(&text, px(font_size), std::slice::from_ref(&base), None);
    let width = line.width;
    let label = text.clone();
    let canvas = gpui_kit::canvas(
        move |bounds, window, _| {
            display_layout(&text, line, bounds.size.width, |candidate| {
                window.text_system().layout_line(
                    candidate,
                    px(font_size),
                    &[TextRun {
                        len: candidate.len(),
                        ..base.clone()
                    }],
                    None,
                )
            })
        },
        move |bounds, display, window, cx| {
            let Some((shown, layout)) = display else {
                return;
            };
            let clipped_bounds = bounds.intersect(&window.content_mask().bounds);
            let moving = should_animate(
                Some(&layout),
                !clipped_bounds.is_empty(),
                window.is_window_active(),
                cx.reduce_motion(),
            );
            let phase = epoch.elapsed().as_secs_f32() * 1000.
                / vega_theme::Motion::THINKING_SHIMMER_MS as f32;
            let decorations =
                frame_decorations(&shown, &layout, width, phase, base_color, highlight, moving);
            window.with_content_mask(
                Some(ContentMask {
                    bounds: clipped_bounds,
                }),
                |window| {
                    let _ = layout.paint(
                        point(bounds.left(), bounds.top()),
                        bounds.size.height,
                        TextAlign::Left,
                        None,
                        &decorations,
                        window,
                        cx,
                    );
                },
            );
            if moving {
                window.request_animation_frame();
            }
        },
    )
    .w(width)
    .max_w_full()
    .h(px(height));
    div()
        .id("thinking-shimmer")
        .min_w_0()
        .max_w_full()
        .aria_label(label)
        .child(canvas)
        .into_any_element()
}
