use nsh_core::{
    canvas::{FillRule, LineCap, LineJoin, Path, svg_path},
    prelude::*,
};
use vega_theme::{Layout, ThemeColors};
use vega_ui::nsh::color;

use super::model::Snapshot;

pub(super) fn blend(index: usize, phase: f32) -> f32 {
    let x = (index as f32 + 0.5) / 64.;
    let center = phase * 1.4 - 0.2;
    (1. - (x - center).abs() / 0.18).max(0.)
}

fn tint(colors: ThemeColors, amount: f32) -> nsh_core::Rgba {
    let base = colors.text_tertiary;
    let bright = colors.text_secondary;
    color(gpui_kit::Rgba {
        r: base.r + (bright.r - base.r) * amount,
        g: base.g + (bright.g - base.g) * amount,
        b: base.b + (bright.b - base.b) * amount,
        a: base.a + (bright.a - base.a) * amount,
    })
}

struct Symbol {
    chevron: Path,
    star: Path,
    smile: Path,
}

impl Symbol {
    fn new() -> Self {
        Self {
            chevron: svg_path("M316 316 L512 512 L316 686"),
            star: svg_path(
                "M686 371 C688 371 689 373 690 377 C698 409 702 425 714 436 C725 446 738 450 751 454 C755 455 755 457 751 458 C738 462 725 466 714 476 C702 487 698 503 690 534 C689 538 688 540 686 540 C684 540 683 538 682 534 C674 503 670 487 658 476 C647 466 634 462 621 458 C617 457 617 455 621 454 C634 450 647 446 658 436 C670 425 674 409 682 377 C683 373 684 371 686 371 Z",
            ),
            smile: svg_path("M473 727 Q586 757 702 727"),
        }
    }

    fn draw(&self, painter: Painter, tint: nsh_core::Rgba) -> Painter {
        painter
            .stroke_style(tint)
            .fill_style(tint)
            .line_cap(LineCap::Round)
            .line_join(LineJoin::Round)
            .line_width(90.)
            .stroke_path(&self.chevron)
            .line_width(2.)
            .fill_path(&self.star, FillRule::NonZero)
            .stroke_path(&self.star)
            .line_width(56.)
            .stroke_path(&self.smile)
    }
}

pub(super) fn logo(state: Signal<Snapshot>) -> impl IntoElement {
    let symbol = Symbol::new();
    canvas(move || {
        let snapshot = state.get();
        let scale = Layout::CONVERSATION_LOADING_LOGO_SIZE / 1024.;
        let mut painter = symbol.draw(
            Painter::new().scale(scale, scale),
            tint(snapshot.colors, 0.),
        );
        if let Some(phase) = snapshot.phase {
            for index in 0..64 {
                let amount = blend(index, phase);
                if amount == 0. {
                    continue;
                }
                let left = index as f32 * 1024. / 64.;
                painter = symbol
                    .draw(
                        painter
                            .save()
                            .begin_path()
                            .move_to(left, 0.)
                            .line_to(left + 1024. / 64., 0.)
                            .line_to(left + 1024. / 64., 1024.)
                            .line_to(left, 1024.)
                            .close_path()
                            .clip(),
                        tint(snapshot.colors, amount),
                    )
                    .restore();
            }
        }
        painter
    })
    .size(px(Layout::CONVERSATION_LOADING_LOGO_SIZE))
    .overflow_hidden()
}
