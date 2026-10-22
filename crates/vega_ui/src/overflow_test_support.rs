use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use gpui_kit::prelude::*;
use gpui_kit::{
    App, AtlasKey, AtlasTextureId, AtlasTile, Bounds, DevicePixels, Element, ElementId, EntityId,
    Font, FontId, FontMetrics, FontRun, Global, GlobalElementId, GlyphId, HeadlessAppContext,
    InspectorElementId, LayoutId, NoopTextSystem, Pixels, PlatformAtlas, PlatformHeadlessRenderer,
    PlatformTextSystem, Quad, RenderGlyphParams, Result, ScaledPixels, Scene, Size,
    TextRenderingMode, TileId, Window, point, px, size,
};

#[derive(Default)]
pub(crate) struct PaintedGeometry {
    pub inputs: HashMap<EntityId, Bounds<Pixels>>,
    pub thumbnails: HashMap<EntityId, Bounds<Pixels>>,
    pub regions: HashMap<&'static str, Bounds<Pixels>>,
}

impl Global for PaintedGeometry {}

pub(crate) fn input_bounds(id: EntityId, bounds: Bounds<Pixels>, cx: &mut App) {
    if cx.has_global::<PaintedGeometry>() {
        cx.global_mut::<PaintedGeometry>().inputs.insert(id, bounds);
    }
}

enum BoundsTarget {
    Region(&'static str),
    Thumbnail(EntityId),
}

struct ObservedBounds<E> {
    inner: E,
    target: BoundsTarget,
}

pub(crate) fn observe_bounds(
    element: impl IntoElement,
    name: &'static str,
) -> gpui_kit::AnyElement {
    ObservedBounds {
        inner: element.into_element(),
        target: BoundsTarget::Region(name),
    }
    .into_any_element()
}

pub(crate) fn observe_thumbnail_bounds(
    element: impl IntoElement,
    id: EntityId,
) -> gpui_kit::AnyElement {
    ObservedBounds {
        inner: element.into_element(),
        target: BoundsTarget::Thumbnail(id),
    }
    .into_any_element()
}

impl<E: Element> IntoElement for ObservedBounds<E> {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl<E: Element> Element for ObservedBounds<E> {
    type RequestLayoutState = E::RequestLayoutState;
    type PrepaintState = E::PrepaintState;

    fn id(&self) -> Option<ElementId> {
        self.inner.id()
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        self.inner.source_location()
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        self.inner.request_layout(id, inspector, window, cx)
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.inner
            .prepaint(id, inspector, bounds, layout, window, cx)
    }

    fn a11y_role(&self) -> Option<gpui_kit::Role> {
        self.inner.a11y_role()
    }

    fn write_a11y_info(&self, node: &mut gpui_kit::accesskit::Node) {
        self.inner.write_a11y_info(node);
    }

    fn a11y_synthetic_children(
        &mut self,
        state: &mut Self::PrepaintState,
        builder: &mut gpui_kit::A11ySubtreeBuilder,
    ) {
        self.inner.a11y_synthetic_children(state, builder);
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout: &mut Self::RequestLayoutState,
        state: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        if cx.has_global::<PaintedGeometry>() {
            let geometry = cx.global_mut::<PaintedGeometry>();
            match self.target {
                BoundsTarget::Region(name) => {
                    geometry.regions.insert(name, bounds);
                }
                BoundsTarget::Thumbnail(id) => {
                    geometry.thumbnails.insert(id, bounds);
                }
            }
        }
        self.inner
            .paint(id, inspector, bounds, layout, state, window, cx);
    }
}

pub(crate) struct ObservableFont;

impl PlatformTextSystem for ObservableFont {
    fn add_fonts(&self, fonts: Vec<Cow<'static, [u8]>>) -> Result<()> {
        NoopTextSystem.add_fonts(fonts)
    }

    fn all_font_names(&self) -> Vec<String> {
        NoopTextSystem.all_font_names()
    }

    fn font_id(&self, descriptor: &Font) -> Result<FontId> {
        NoopTextSystem.font_id(descriptor)
    }

    fn font_metrics(&self, font: FontId) -> FontMetrics {
        NoopTextSystem.font_metrics(font)
    }

    fn typographic_bounds(&self, font: FontId, glyph: GlyphId) -> Result<Bounds<f32>> {
        NoopTextSystem.typographic_bounds(font, glyph)
    }

    fn advance(&self, _font: FontId, _glyph: GlyphId) -> Result<Size<f32>> {
        Ok(size(600., 0.))
    }

    fn glyph_for_char(&self, _font: FontId, character: char) -> Option<GlyphId> {
        Some(GlyphId(character as u32))
    }

    fn glyph_raster_bounds(&self, params: &RenderGlyphParams) -> Result<Bounds<DevicePixels>> {
        let height = (f32::from(params.font_size) * params.scale_factor).ceil() as i32;
        Ok(Bounds::new(
            point(DevicePixels(0), DevicePixels(-height)),
            size(DevicePixels((height / 2).max(1)), DevicePixels(height)),
        ))
    }

    fn rasterize_glyph(
        &self,
        _params: &RenderGlyphParams,
        bounds: Bounds<DevicePixels>,
    ) -> Result<(Size<DevicePixels>, Vec<u8>)> {
        let count = (bounds.size.width.0 * bounds.size.height.0) as usize;
        Ok((bounds.size, vec![255; count]))
    }

    fn layout_line(&self, text: &str, font_size: Pixels, runs: &[FontRun]) -> gpui_kit::LineLayout {
        let mut line = NoopTextSystem.layout_line(text, font_size, runs);
        for (glyph, character) in line
            .runs
            .iter_mut()
            .flat_map(|run| run.glyphs.iter_mut())
            .zip(text.chars())
        {
            glyph.id = GlyphId(character as u32);
            glyph.is_emoji = false;
        }
        line
    }

    fn recommended_rendering_mode(&self, _font: FontId, _size: Pixels) -> TextRenderingMode {
        TextRenderingMode::Grayscale
    }
}

#[derive(Clone, Debug)]
pub(crate) struct PaintedGlyph {
    pub character: char,
    pub bounds: Bounds<ScaledPixels>,
    pub mask: Bounds<ScaledPixels>,
}

#[derive(Clone, Debug)]
pub(crate) struct PaintedImage {
    pub decoded_size: Size<DevicePixels>,
    pub bounds: Bounds<ScaledPixels>,
    pub mask: Bounds<ScaledPixels>,
}

#[derive(Clone, Default)]
pub(crate) struct PaintedFrame {
    pub glyphs: Vec<PaintedGlyph>,
    pub images: Vec<PaintedImage>,
    pub quads: Vec<Quad>,
}

#[derive(Default)]
struct AtlasState {
    tiles: HashMap<AtlasKey, AtlasTile>,
    next_id: u32,
}

#[derive(Default)]
struct RecordingAtlas(Mutex<AtlasState>);

impl PlatformAtlas for RecordingAtlas {
    fn get_or_insert_with<'a>(
        &self,
        key: &AtlasKey,
        build: &mut dyn FnMut() -> Result<Option<(Size<DevicePixels>, Cow<'a, [u8]>)>>,
    ) -> Result<Option<AtlasTile>> {
        if let Some(tile) = self.0.lock().unwrap().tiles.get(key) {
            return Ok(Some(*tile));
        }
        let Some((dimensions, _pixels)) = build()? else {
            return Ok(None);
        };
        let mut state = self.0.lock().unwrap();
        state.next_id += 1;
        let tile = AtlasTile {
            texture_id: AtlasTextureId {
                index: state.next_id,
                kind: key.texture_kind(),
            },
            tile_id: TileId(state.next_id),
            padding: 0,
            bounds: Bounds::new(point(DevicePixels(0), DevicePixels(0)), dimensions),
        };
        state.tiles.insert(key.clone(), tile);
        Ok(Some(tile))
    }

    fn remove(&self, key: &AtlasKey) {
        self.0.lock().unwrap().tiles.remove(key);
    }

    fn contains(&self, key: &AtlasKey) -> bool {
        self.0.lock().unwrap().tiles.contains_key(key)
    }
}

#[derive(Clone, Default)]
pub(crate) struct SceneRecorder(Arc<Mutex<PaintedFrame>>);

impl SceneRecorder {
    pub fn frame(&self) -> PaintedFrame {
        self.0.lock().unwrap().clone()
    }
}

struct RecordingRenderer {
    recorder: SceneRecorder,
    atlas: Arc<RecordingAtlas>,
}

impl PlatformHeadlessRenderer for RecordingRenderer {
    fn render_scene_to_image(
        &mut self,
        scene: &Scene,
        dimensions: Size<DevicePixels>,
    ) -> Result<image::RgbaImage> {
        self.render_scene(scene, dimensions)?;
        Ok(image::RgbaImage::new(0, 0))
    }

    fn render_scene(&mut self, scene: &Scene, _size: Size<DevicePixels>) -> Result<()> {
        let atlas = self.atlas.0.lock().unwrap();
        let mut frame = PaintedFrame {
            quads: scene.quads.clone(),
            ..PaintedFrame::default()
        };
        for sprite in &scene.monochrome_sprites {
            if let Some(character) = atlas.tiles.iter().find_map(|(key, tile)| {
                if tile.texture_id != sprite.tile.texture_id {
                    return None;
                }
                match key {
                    AtlasKey::Glyph(params) => char::from_u32(params.glyph_id.0),
                    _ => None,
                }
            }) {
                frame.glyphs.push(PaintedGlyph {
                    character,
                    bounds: sprite.bounds,
                    mask: sprite.content_mask.bounds,
                });
            }
        }
        for sprite in &scene.polychrome_sprites {
            if let Some(decoded_size) = atlas.tiles.iter().find_map(|(key, tile)| {
                (tile.texture_id == sprite.tile.texture_id && matches!(key, AtlasKey::Image(_)))
                    .then_some(tile.bounds.size)
            }) {
                frame.images.push(PaintedImage {
                    decoded_size,
                    bounds: sprite.bounds,
                    mask: sprite.content_mask.bounds,
                });
            }
        }
        *self.recorder.0.lock().unwrap() = frame;
        Ok(())
    }

    fn sprite_atlas(&self) -> Arc<dyn PlatformAtlas> {
        self.atlas.clone()
    }
}

pub(crate) fn context(dark: bool) -> (HeadlessAppContext, SceneRecorder) {
    context_with_font(dark, Arc::new(ObservableFont))
}

pub(crate) fn context_with_font(
    dark: bool,
    font: Arc<dyn PlatformTextSystem>,
) -> (HeadlessAppContext, SceneRecorder) {
    let recorder = SceneRecorder::default();
    let renderer_recorder = recorder.clone();
    let mut cx =
        HeadlessAppContext::with_platform(font, Arc::new(gpui_kit::assets::Assets), move || {
            Some(Box::new(RecordingRenderer {
                recorder: renderer_recorder.clone(),
                atlas: Arc::new(RecordingAtlas::default()),
            }))
        });
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::init(cx);
        let base = gpui_kit::base::Theme::global_mut(cx);
        base.scrollbar = base
            .scrollbar
            .clone()
            .with_mode(gpui_kit::base::ScrollbarMode::Always)
            .with_motion(gpui_kit::base::ScrollbarMotion::default());
        cx.set_global(if dark {
            vega_theme::Theme::dark()
        } else {
            vega_theme::Theme::light()
        });
        cx.set_global(PaintedGeometry::default());
    });
    (cx, recorder)
}

pub(crate) fn logical(bounds: Bounds<ScaledPixels>, scale: f32) -> Bounds<Pixels> {
    Bounds::new(
        point(px(bounds.origin.x.0 / scale), px(bounds.origin.y.0 / scale)),
        size(
            px(bounds.size.width.0 / scale),
            px(bounds.size.height.0 / scale),
        ),
    )
}

pub(crate) fn visible(bounds: Bounds<Pixels>) -> bool {
    bounds.size.width > px(0.) && bounds.size.height > px(0.)
}

pub(crate) fn contains(outer: Bounds<Pixels>, inner: Bounds<Pixels>) -> bool {
    let tolerance = px(1.);
    inner.left() >= outer.left() - tolerance
        && inner.top() >= outer.top() - tolerance
        && inner.right() <= outer.right() + tolerance
        && inner.bottom() <= outer.bottom() + tolerance
}

impl PaintedFrame {
    pub fn text_rows(&self, viewport: Bounds<Pixels>, scale: f32) -> Vec<String> {
        let mut glyphs: Vec<_> = self
            .glyphs
            .iter()
            .filter_map(|glyph| {
                let bounds = logical(glyph.bounds, scale);
                let clipped = bounds.intersect(&logical(glyph.mask, scale));
                (visible(clipped) && contains(viewport, clipped)).then_some((
                    bounds.top(),
                    bounds.left(),
                    glyph.character,
                ))
            })
            .collect();
        glyphs.sort_by(|left, right| {
            f32::from(left.0)
                .total_cmp(&f32::from(right.0))
                .then(f32::from(left.1).total_cmp(&f32::from(right.1)))
        });
        let mut rows: Vec<(Pixels, String)> = Vec::new();
        for (top, _, character) in glyphs {
            if let Some((previous, text)) = rows.last_mut()
                && (f32::from(top - *previous)).abs() < 1.
            {
                text.push(character);
            } else {
                rows.push((top, character.to_string()));
            }
        }
        rows.into_iter().map(|(_, text)| text).collect()
    }
}
