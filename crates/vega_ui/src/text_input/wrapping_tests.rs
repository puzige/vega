use std::borrow::Cow;
use std::sync::Arc;

use gpui_kit::prelude::*;
use gpui_kit::{
    AppContext, Bounds, Context, DevicePixels, Entity, Font, FontId, FontMetrics, FontRun, GlyphId,
    Pixels, PlatformTextSystem, Render, RenderGlyphParams, Result, Size, TextRenderingMode, Window,
    div, px, size,
};

use super::TextInput;
use crate::overflow_test_support::{
    ObservableFont, PaintedFrame, PaintedGeometry, context_with_font, logical,
};

struct KerningFont;

impl PlatformTextSystem for KerningFont {
    fn add_fonts(&self, fonts: Vec<Cow<'static, [u8]>>) -> Result<()> {
        ObservableFont.add_fonts(fonts)
    }

    fn all_font_names(&self) -> Vec<String> {
        ObservableFont.all_font_names()
    }

    fn font_id(&self, descriptor: &Font) -> Result<FontId> {
        ObservableFont.font_id(descriptor)
    }

    fn font_metrics(&self, font: FontId) -> FontMetrics {
        ObservableFont.font_metrics(font)
    }

    fn typographic_bounds(&self, font: FontId, glyph: GlyphId) -> Result<Bounds<f32>> {
        ObservableFont.typographic_bounds(font, glyph)
    }

    fn advance(&self, font: FontId, _glyph: GlyphId) -> Result<Size<f32>> {
        Ok(size(
            self.font_metrics(font).units_per_em as f32 * 8. / 13.,
            0.,
        ))
    }

    fn glyph_for_char(&self, font: FontId, character: char) -> Option<GlyphId> {
        ObservableFont.glyph_for_char(font, character)
    }

    fn glyph_raster_bounds(&self, params: &RenderGlyphParams) -> Result<Bounds<DevicePixels>> {
        let mut bounds = ObservableFont.glyph_raster_bounds(params)?;
        bounds.size.width = DevicePixels(
            (f32::from(params.font_size) * 8. / 13. * params.scale_factor).round() as i32,
        );
        Ok(bounds)
    }

    fn rasterize_glyph(
        &self,
        params: &RenderGlyphParams,
        bounds: Bounds<DevicePixels>,
    ) -> Result<(Size<DevicePixels>, Vec<u8>)> {
        ObservableFont.rasterize_glyph(params, bounds)
    }

    fn layout_line(&self, text: &str, font_size: Pixels, runs: &[FontRun]) -> gpui_kit::LineLayout {
        let mut line = ObservableFont.layout_line(text, font_size, runs);
        let mut position = px(0.);
        let mut previous = None;
        for (glyph, character) in line
            .runs
            .iter_mut()
            .flat_map(|run| run.glyphs.iter_mut())
            .zip(text.chars())
        {
            if previous == Some('A') && character == 'V' {
                position -= font_size * (2. / 13.);
            }
            glyph.position.x = position;
            position += font_size * (8. / 13.);
            previous = Some(character);
        }
        line.width = position;
        line
    }

    fn recommended_rendering_mode(&self, font: FontId, size: Pixels) -> TextRenderingMode {
        ObservableFont.recommended_rendering_mode(font, size)
    }
}

struct WrappingHost {
    input: Entity<TextInput>,
    width: Pixels,
}

impl Render for WrappingHost {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(self.width)
            .h_full()
            .flex()
            .flex_col()
            .child(self.input.clone())
    }
}

struct RenderedDraft {
    frame: PaintedFrame,
    viewport: Bounds<Pixels>,
    scale: f32,
    shaped_rows: Vec<(String, Pixels)>,
    content: String,
}

fn render_draft(content: &str, width: f32) -> RenderedDraft {
    let (mut cx, recorder) = context_with_font(false, Arc::new(KerningFont));
    let window = cx
        .open_window(size(px(240.), px(240.)), |_, cx| {
            let input = cx.new(|cx| TextInput::new_multiline(cx, "", 1));
            input.update(cx, |input, cx| input.set_text(content, cx));
            cx.new(|_| WrappingHost {
                input,
                width: px(width),
            })
        })
        .unwrap();
    let root = window.root(&mut cx).unwrap();
    let input = cx.update(|cx| root.read(cx).input.clone());
    for _ in 0..3 {
        cx.run_until_parked();
        cx.update_window(window.into(), |_, window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
            drop(window.render_to_image().unwrap());
        })
        .unwrap();
    }
    let (viewport, scale, shaped_rows, content) = cx
        .update_window(window.into(), |_, window, cx| {
            let state = input.read(cx);
            (
                cx.global::<PaintedGeometry>().inputs[&input.entity_id()],
                window.scale_factor(),
                state
                    .last_lines
                    .iter()
                    .map(|layout| (layout.line.text.to_string(), layout.line.width()))
                    .collect(),
                state.text().to_string(),
            )
        })
        .unwrap();
    RenderedDraft {
        frame: recorder.frame(),
        viewport,
        scale,
        shaped_rows,
        content,
    }
}

fn assert_preserved_draft(draft: &RenderedDraft, expected: &str) -> Vec<String> {
    let rows = draft.frame.text_rows(draft.viewport, draft.scale);
    assert_eq!(rows.concat(), expected, "painted rows={rows:?}");
    assert_eq!(draft.content, expected);
    assert_eq!(
        rows,
        draft
            .shaped_rows
            .iter()
            .map(|(text, _)| text.clone())
            .collect::<Vec<_>>()
    );
    rows
}

fn assert_unclipped_glyphs(draft: &RenderedDraft) {
    for glyph in &draft.frame.glyphs {
        let bounds = logical(glyph.bounds, draft.scale);
        let mask = logical(glyph.mask, draft.scale);
        assert!(
            bounds.left() >= draft.viewport.left() && bounds.right() <= draft.viewport.right(),
            "glyph {:?} exceeds viewport: {bounds:?} vs {:?}",
            glyph.character,
            draft.viewport
        );
        assert!(
            bounds.left() >= mask.left() && bounds.right() <= mask.right(),
            "glyph {:?} is clipped: {bounds:?} vs {mask:?}",
            glyph.character
        );
    }
}

#[test]
fn contextual_kerning_prefix_is_remeasured_before_accepting_a_wrapped_row() {
    let draft = render_draft("éAVZ", 14.5);
    assert_eq!(draft.viewport.size.width, px(14.5));
    assert_preserved_draft(&draft, "éAVZ");
    for (text, width) in &draft.shaped_rows {
        assert!(
            text.chars().count() == 1 || *width <= draft.viewport.size.width,
            "independently shaped prefix {text:?} is {width:?} wide, viewport={:?}",
            draft.viewport
        );
    }
    assert_unclipped_glyphs(&draft);
}

#[test]
fn unkerned_fitting_prefix_keeps_complete_glyphs_and_text() {
    let draft = render_draft("éACZ", 16.5);
    assert_eq!(draft.viewport.size.width, px(16.5));
    assert_eq!(assert_preserved_draft(&draft, "éACZ"), ["éA", "CZ"]);
    assert!(
        draft
            .shaped_rows
            .iter()
            .all(|(_, width)| *width <= draft.viewport.size.width),
        "shaped rows={:?}, viewport={:?}",
        draft.shaped_rows,
        draft.viewport
    );
    assert_unclipped_glyphs(&draft);
}

#[test]
fn narrower_than_one_glyph_wrap_makes_utf8_character_progress_without_losing_text() {
    let draft = render_draft("é中A", 3.5);
    assert_eq!(draft.viewport.size.width, px(3.5));
    assert_eq!(assert_preserved_draft(&draft, "é中A"), ["é", "中", "A"]);
    assert!(
        draft
            .shaped_rows
            .iter()
            .all(|(text, width)| text.chars().count() == 1 && *width > draft.viewport.size.width),
        "single-character fallback rows={:?}, viewport={:?}",
        draft.shaped_rows,
        draft.viewport
    );
}
