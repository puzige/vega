use std::time::Instant;

use gpui_kit::{prelude::*, *};
use vega_theme::{Layout, Motion, ThemeColors};

pub(crate) fn loading_logo(epoch: Instant, colors: ThemeColors) -> impl IntoElement {
    const LOGO: &[u8] = include_bytes!("../../../assets/logo/vega-symbol-smile-mono.svg");
    canvas(
        |_, _, _| (),
        move |bounds, _, window, cx| {
            let clipped = bounds.intersect(&window.content_mask().bounds);
            let moving = !clipped.is_empty() && window.is_window_active() && !cx.reduce_motion();
            let _ = window.paint_svg(
                bounds,
                "vega-loading-logo".into(),
                Some(LOGO),
                Default::default(),
                colors.text_tertiary.into(),
                cx,
            );
            if moving {
                let phase = (epoch.elapsed().as_secs_f32() * 1000.
                    / Motion::THINKING_SHIMMER_MS as f32)
                    .fract();
                let center = phase * 1.4 - 0.2;
                for index in 0..64 {
                    let x = (index as f32 + 0.5) / 64.;
                    let blend = (1. - (x - center).abs() / 0.18).max(0.);
                    if blend == 0. {
                        continue;
                    }
                    let base = colors.text_tertiary;
                    let bright = colors.text_secondary;
                    let color = Rgba {
                        r: base.r + (bright.r - base.r) * blend,
                        g: base.g + (bright.g - base.g) * blend,
                        b: base.b + (bright.b - base.b) * blend,
                        a: base.a + (bright.a - base.a) * blend,
                    };
                    let slice = Bounds::new(
                        point(
                            bounds.left() + bounds.size.width * (index as f32 / 64.),
                            bounds.top(),
                        ),
                        size(bounds.size.width / 64., bounds.size.height),
                    );
                    window.with_content_mask(
                        Some(ContentMask {
                            bounds: clipped.intersect(&slice),
                        }),
                        |window| {
                            let _ = window.paint_svg(
                                bounds,
                                "vega-loading-logo".into(),
                                Some(LOGO),
                                Default::default(),
                                color.into(),
                                cx,
                            );
                        },
                    );
                }
                window.request_animation_frame();
            }
        },
    )
    .size(px(Layout::CONVERSATION_LOADING_LOGO_SIZE))
}
