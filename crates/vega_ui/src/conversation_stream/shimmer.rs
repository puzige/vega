use super::*;
use gpui_kit::LineLayout;

#[path = "newshimmer/mod.rs"]
mod native;

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
            let colors = theme(cx).colors;
            native::element(
                self.title.clone(),
                None,
                colors.conversation_stage,
                colors.bg_base,
                window,
            )
        }
    }
}

pub(crate) fn stage_text(text: String, epoch: Instant, window: &Window, cx: &App) -> AnyElement {
    let colors = theme(cx).colors;
    native::element(
        text,
        Some(epoch),
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

fn frame_colors(
    text: &str,
    layout: &LineLayout,
    original_width: Pixels,
    phase: f32,
    base: Rgba,
    highlight: Rgba,
    moving: bool,
) -> Vec<Rgba> {
    let center = phase.fract() * 1.6 - 0.3;
    let width = f32::from(original_width.max(px(1.)));
    text.char_indices()
        .map(|(index, _)| {
            let x = f32::from(layout.x_for_index(index)) / width;
            let strength = if moving {
                (1. - (x - center).abs() / 0.24).max(0.)
            } else {
                0.
            };
            Rgba {
                r: base.r + (highlight.r - base.r) * strength,
                g: base.g + (highlight.g - base.g) * strength,
                b: base.b + (highlight.b - base.b) * strength,
                a: base.a,
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
