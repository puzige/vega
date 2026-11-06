use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, Pixels, ShapedLine, SharedString, TextStyle, Window, canvas, point, px,
};

pub(super) fn render(title: String) -> AnyElement {
    let text: SharedString = title.replace(['\n', '\r'], " ").into();
    canvas(
        move |bounds, window, _| {
            let style = window.text_style();
            let font_size = style.font_size.to_pixels(window.rem_size());
            let line_height = window.pixel_snap(
                style
                    .line_height
                    .to_pixels(font_size.into(), window.rem_size()),
            );
            let line = fit_title(text, bounds.size.width, &style, font_size, window);
            (line, line_height)
        },
        |bounds, (line, line_height), window, cx| {
            if let Some(line) = line {
                let origin = point(
                    bounds.origin.x,
                    bounds.origin.y + (bounds.size.height - line_height) / 2.,
                );
                if let Err(error) = line.paint(
                    origin,
                    line_height,
                    gpui_kit::TextAlign::Left,
                    Some(bounds.size.width),
                    window,
                    cx,
                ) {
                    tracing::warn!(%error, "Failed to paint sidebar title");
                }
            }
        },
    )
    .size_full()
    .into_any_element()
}

fn shape_title(
    text: SharedString,
    style: &TextStyle,
    font_size: Pixels,
    window: &Window,
) -> ShapedLine {
    let run = style.to_run(text.len());
    window
        .text_system()
        .shape_line(text, font_size, &[run], None)
}

fn fit_title(
    text: SharedString,
    width: Pixels,
    style: &TextStyle,
    font_size: Pixels,
    window: &Window,
) -> Option<ShapedLine> {
    if width <= px(0.) {
        return None;
    }
    let full = shape_title(text.clone(), style, font_size, window);
    if full.width() <= width {
        return Some(full);
    }
    let mut best = shape_title("…".into(), style, font_size, window);
    if best.width() > width {
        return None;
    }
    let mut cuts: Vec<usize> = full
        .runs
        .iter()
        .flat_map(|run| run.glyphs.iter())
        .map(|glyph| glyph.index)
        .filter(|index| *index > 0 && *index < text.len() && text.is_char_boundary(*index))
        .collect();
    cuts.sort_unstable();
    cuts.dedup();
    let (mut low, mut high) = (0, cuts.len());
    while low < high {
        let middle = low + (high - low) / 2;
        let candidate = shape_title(
            format!("{}…", &text[..cuts[middle]]).into(),
            style,
            font_size,
            window,
        );
        if candidate.width() <= width {
            best = candidate;
            low = middle + 1;
        } else {
            high = middle;
        }
    }
    Some(best)
}
