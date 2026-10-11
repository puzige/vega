use super::*;
use gpui_kit::{TextAlign, point};

pub(crate) fn text(text: String, epoch: Instant, window: &Window, cx: &App) -> AnyElement {
    let colors = theme(cx).colors;
    let font = window.text_style().font();
    let base = TextRun {
        len: text.len(),
        font: font.clone(),
        color: colors.text_tertiary.into(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let line = window.text_system().shape_line(
        text.clone().into(),
        px(Typography::BODY),
        &[base.clone()],
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
                    px(Typography::BODY),
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
                    let a = colors.text_tertiary;
                    let b = colors.text_primary;
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
            #[cfg(test)]
            if let Some(probe) = cx.try_global::<PaintProbe>() {
                probe.0.borrow_mut().push(PaintSample {
                    text: shown.clone(),
                    moving,
                    colors: runs.iter().map(|run| run.color).collect(),
                });
            }
            let shaped =
                window
                    .text_system()
                    .shape_line(shown.into(), px(Typography::BODY), &runs, None);
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
    .h(px(Layout::DISCLOSURE_HEADER_HEIGHT));
    div()
        .id("thinking-shimmer")
        .min_w_0()
        .max_w_full()
        .aria_label(label)
        .child(canvas)
        .into_any_element()
}

#[cfg(test)]
#[derive(Default)]
pub(crate) struct PaintProbe(pub std::cell::RefCell<Vec<PaintSample>>);

#[cfg(test)]
pub(crate) struct PaintSample {
    pub text: String,
    pub moving: bool,
    pub colors: Vec<gpui_kit::Hsla>,
}

#[cfg(test)]
impl gpui_kit::Global for PaintProbe {}
