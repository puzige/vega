use super::*;
use gpui_kit::{TextAlign, point};

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

fn styled_text(
    text: String,
    epoch: Instant,
    font_size: f32,
    height: f32,
    base_color: Rgba,
    highlight: Rgba,
    window: &Window,
) -> AnyElement {
    let font = window.text_style().font();
    let base = TextRun {
        len: text.len(),
        font: font.clone(),
        color: base_color.into(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let line = window.text_system().shape_line(
        text.clone().into(),
        px(font_size),
        std::slice::from_ref(&base),
        None,
    );
    let width = line.width();
    let label = text.clone();
    let canvas = gpui_kit::canvas(
        |_, _, _| (),
        move |bounds, _, window, cx| {
            let visible = !bounds.intersect(&window.content_mask().bounds).is_empty()
                && window.is_window_active();
            let moving = visible && !cx.reduce_motion();
            let phase = epoch.elapsed().as_secs_f32() * 1000.
                / vega_theme::Motion::THINKING_SHIMMER_MS as f32;
            let center = phase.fract() * 1.6 - 0.3;
            let mut shown = text.clone();
            if width > bounds.size.width + px(1.) {
                let ellipsis = window.text_system().shape_line(
                    "…".into(),
                    px(font_size),
                    &[TextRun {
                        len: "…".len(),
                        ..base.clone()
                    }],
                    None,
                );
                let cut =
                    line.closest_index_for_x((bounds.size.width - ellipsis.width()).max(px(0.)));
                shown.truncate(cut);
                shown.push('…');
            }
            let runs = shown
                .char_indices()
                .map(|(index, ch)| {
                    let x = f32::from(line.x_for_index(index.min(text.len())))
                        / f32::from(width.max(px(1.)));
                    let strength = if moving {
                        (1. - (x - center).abs() / 0.24).max(0.)
                    } else {
                        0.
                    };
                    let a = base_color;
                    let b = highlight;
                    TextRun {
                        len: ch.len_utf8(),
                        color: Rgba {
                            r: a.r + (b.r - a.r) * strength,
                            g: a.g + (b.g - a.g) * strength,
                            b: a.b + (b.b - a.b) * strength,
                            a: a.a,
                        }
                        .into(),
                        ..base.clone()
                    }
                })
                .collect::<Vec<_>>();
            let shaped = window
                .text_system()
                .shape_line(shown.into(), px(font_size), &runs, None);
            let _ = shaped.paint(
                point(bounds.left(), bounds.top()),
                bounds.size.height,
                TextAlign::Left,
                None,
                window,
                cx,
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
