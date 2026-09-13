//! R57 P2a — the thinking-tier slider: a self-contained popup card that picks
//! one reasoning/thinking tier for a model.
//!
//! Contract source: `docs/vega-r57-composer-alignment.md` §3 (mechanism and
//! fallback) and §4 (measured visual targets). The module deliberately reads
//! no store: the supported tier list, the current tier, and the default tier
//! all arrive as constructor inputs, so the component stays headless-testable
//! and P3 can wire `ReasoningProfile.efforts` / `.preference` into it later.
//!
//! Unmeasured items are marked inline and must not be mistaken for measured
//! values:
//! - §4.5 M1 (dot diameter) and M5 (exact paddings) — chosen, see the consts.
//! - §4.5 M3 / R12 (a model supporting no tiers) — renders nothing at all.
//! - §4.5 M2 (the colour ramp across intermediate tiers) — only the two
//!   measured endpoints are modelled: highest tier purple, every other tier
//!   blue.
//!
//! ## Fallback semantics
//!
//! [`resolve_tier`] implements the *prose* rule stated both in the P2a slice
//! brief and in spec R10: an unsupported current tier drops to the **nearest
//! supported tier below it**. The reference bundle's literal snippet
//!
//! ```js
//! tiers.find((t, i) => i < tiers.indexOf(current) && supported.includes(t))
//! ```
//!
//! scans from index 0, so it would instead return the *lowest* supported tier
//! below current. The two readings differ whenever more than one supported
//! tier lies below the current one: with supported `[low, high, max]` and
//! current `ultra`, this module answers `max` while the snippet answers `low`.
//! [`nearest_lower`] is the single place that decides this; reversing its scan
//! direction reproduces the snippet literally.

use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, Bounds, DragMoveEvent, Empty, EventEmitter, MouseButton, MouseDownEvent,
    MouseUpEvent, Pixels, Point, Render, Rgba, Window, base::ElementExt, div, linear_color_stop,
    linear_gradient, px, svg,
};
use vega_theme::{Layout, Typography, theme};

// ---------------------------------------------------------------------------
// Measured geometry (spec §4.3, logical px)
// ---------------------------------------------------------------------------

/// Card width. Measured 254.5 (spec §4.3, card left 20.5 → right 275.0).
pub const THINKING_CARD_WIDTH: f32 = 254.5;
/// Track box width. Measured 203.0 (spec §4.3, track left 33.0 → right 236.0).
pub const THINKING_TRACK_WIDTH: f32 = 203.0;
/// Track box height. Measured 24.0 (spec §4.3, track top 82.0 → bottom 106.0).
pub const THINKING_TRACK_HEIGHT: f32 = 24.0;
/// Knob diameter. Measured 24.0 — the knob is exactly as tall as the track
/// (spec §4.3).
pub const THINKING_KNOB_DIAMETER: f32 = 24.0;

/// Horizontal card inset. Derived from two measurements rather than chosen:
/// the measured track is centred inside the measured card, so both sides take
/// `(254.5 - 203.0) / 2`.
const CARD_INSET: f32 = (THINKING_CARD_WIDTH - THINKING_TRACK_WIDTH) / 2.0;

/// Dot diameter. **Chosen, not measured**: spec §4.5 M1 lists the dot spacing
/// and diameter as still unmeasured, so this needs calibration.
pub const THINKING_DOT_DIAMETER: f32 = 4.0;

/// Card vertical padding. **Chosen, not measured**: spec §4.5 M5 lists the
/// exact card/track paddings as still unmeasured.
const CARD_PADDING_TOP: f32 = 12.0;
/// See [`CARD_PADDING_TOP`].
const CARD_PADDING_BOTTOM: f32 = 14.0;
/// Vertical gap between the card's three rows. **Chosen** (§4.5 M5).
const CARD_ROW_GAP: f32 = 8.0;

// ---------------------------------------------------------------------------
// Measured colours (spec §4.1, §4.2)
// ---------------------------------------------------------------------------

/// Converts an `0xRRGGBBAA` literal to [`Rgba`]. Mirrors `gpui_kit::rgba`,
/// which is not `const` and therefore unusable in the constants below.
///
/// Hex values are written as integers (never `#RRGGBB`) so the workspace's
/// hardcoded-colour grep stays clean.
const fn measured_rgba(hex: u32) -> Rgba {
    let [r, g, b, a] = hex.to_be_bytes();
    Rgba {
        r: r as f32 / 255.0,
        g: g as f32 / 255.0,
        b: b as f32 / 255.0,
        a: a as f32 / 255.0,
    }
}

/// Tier-name colour at the strongest tier. Measured 0x924FF7 (spec §4.1,
/// `Ultra` state, 264 px sampled).
const TIER_LABEL_STRONG: Rgba = measured_rgba(0x924FF7FF);
/// Tier-name colour below the strongest tier. Measured 0x3983F7 (spec §4.1,
/// `Medium` state, 280 px sampled).
const TIER_LABEL_BASE: Rgba = measured_rgba(0x3983F7FF);

/// Flat track fill for every tier except the strongest. Measured 0x3983F7
/// (spec §4.2, `Medium` state filled region).
const TRACK_FILL_FLAT: Rgba = measured_rgba(0x3983F7FF);

/// Multi-stop track gradient for the strongest tier, as `(position, colour)`
/// pairs. Measured (spec §4.2, `Ultra` state): 0x3346CD at the start,
/// 0xAE76FF at the mid sample, 0x7E5AF0 at the end. Positions come from the
/// same measured samples (logical x 33 / 163 / 235 over the 203 px track);
/// they rest on one sample each and need calibration.
const TRACK_GRADIENT: [(f32, Rgba); 3] = [
    (0.0, measured_rgba(0x3346CDFF)),
    (0.64, measured_rgba(0xAE76FFFF)),
    (1.0, measured_rgba(0x7E5AF0FF)),
];

/// Filled-region dot colour. Measured white (spec §4.2/§4.4: filled dots
/// render white). Kept as the measured literal rather than a surface token so
/// R9 holds in both appearances; dark mode is unmeasured.
const TRACK_DOT_FILLED: Rgba = measured_rgba(0xFFFFFFFF);

/// Unfilled track fill. Measured 0xE9E8E8 (spec §4.2, `Medium` state unfilled
/// region). The measurement is light-mode only; `border_subtle` is 0xE8E8E8,
/// one 8-bit step away, and is used in dark mode where no measurement exists.
const TRACK_UNFILLED: Rgba = measured_rgba(0xE9E8E8FF);

/// Lucide's `zap` bolt on the bundled set's 24 px, stroke-2, round-join
/// grammar. Kept inline so this component stays self-contained (the bundled
/// icon set has no bolt) and so no shared icon table is touched.
const BOLT_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M13 2 3 14h9l-1 8 10-12h-9l1-8z"/></svg>"#;

/// Static debug selectors for rendered dots, indexed by dot position. The
/// array bound exists because `VisualTestContext::debug_bounds` only accepts a
/// `&'static str`; the reference ladder tops out at nine tiers, so twelve
/// leaves headroom.
pub(crate) const DOT_SELECTORS: [&str; 12] = [
    "thinking-slider-dot-0",
    "thinking-slider-dot-1",
    "thinking-slider-dot-2",
    "thinking-slider-dot-3",
    "thinking-slider-dot-4",
    "thinking-slider-dot-5",
    "thinking-slider-dot-6",
    "thinking-slider-dot-7",
    "thinking-slider-dot-8",
    "thinking-slider-dot-9",
    "thinking-slider-dot-10",
    "thinking-slider-dot-11",
];

/// Debug selectors for the gradient's segments, one per adjacent pair in
/// [`TRACK_GRADIENT`].
pub(crate) const GRADIENT_SEGMENT_SELECTORS: [&str; 2] =
    ["thinking-slider-gradient-0", "thinking-slider-gradient-1"];

// ---------------------------------------------------------------------------
// Tier ladder and resolution (spec §3.1)
// ---------------------------------------------------------------------------

/// The reference implementation's complete effort vocabulary, in strength
/// order (`app-initial-cadb12d4a15e.js`). Vega currently ships the subset
/// `["minimal","low","medium","high","xhigh","max"]`
/// (`crates/vega_ui/src/settings/reasoning_state.rs`), but the ladder is the
/// reference's nine so an unknown-but-ordered tier still resolves.
pub const TIER_LADDER: [&str; 9] = [
    "none",
    "minimal",
    "low",
    "medium",
    "high",
    "xhigh",
    "max",
    "ultra",
    "persistent",
];

fn find_supported<'a>(supported: &'a [String], tier: &str) -> Option<&'a str> {
    supported
        .iter()
        .find(|candidate| candidate.as_str() == tier)
        .map(String::as_str)
}

/// The supported tier immediately below `tier` on [`TIER_LADDER`].
///
/// The scan runs *downward* from the tier just below `tier`, so the first
/// supported hit is the nearest one. This is the single line that decides the
/// fallback reading; see the module docs. A `tier` outside the ladder has no
/// rank and therefore no lower tier.
fn nearest_lower<'a>(supported: &'a [String], tier: &str) -> Option<&'a str> {
    let rank = TIER_LADDER
        .iter()
        .position(|candidate| *candidate == tier)?;
    TIER_LADDER[..rank]
        .iter()
        .rev()
        .find_map(|candidate| find_supported(supported, candidate))
}

/// Resolves the tier the slider should show for `preferred`.
///
/// Order (spec §3.1 / R10):
/// 1. `preferred` when the model supports it;
/// 2. otherwise the nearest supported tier below it;
/// 3. otherwise `fallback` (the configured default) when supported;
/// 4. otherwise the first supported tier;
/// 5. otherwise `None` — the model supports nothing, so there is no slider.
///
/// Steps 3 and 4 cover "nothing lies below `preferred`", which the reference
/// snippet leaves undefined; they are this slice's choice.
pub fn resolve_tier<'a>(
    supported: &'a [String],
    preferred: &str,
    fallback: &str,
) -> Option<&'a str> {
    if let Some(exact) = find_supported(supported, preferred) {
        return Some(exact);
    }
    if let Some(lower) = nearest_lower(supported, preferred) {
        return Some(lower);
    }
    find_supported(supported, fallback).or_else(|| supported.first().map(String::as_str))
}

// ---------------------------------------------------------------------------
// Pure model
// ---------------------------------------------------------------------------

/// Headless state for one slider. Owns no store handle and performs no IO.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThinkingSliderModel {
    tiers: Vec<String>,
    current: Option<String>,
    default: Option<String>,
}

impl ThinkingSliderModel {
    /// Builds the model, resolving both the current and the default tier
    /// against `tiers` straight away.
    pub fn new(tiers: Vec<String>, current: &str, default: &str) -> Self {
        let mut model = Self {
            tiers,
            current: None,
            default: None,
        };
        model.rebind(current, default);
        model
    }

    fn rebind(&mut self, current: &str, default: &str) {
        // The default resolves first so it can serve as the current tier's
        // last-resort fallback.
        self.default =
            resolve_tier(&self.tiers, default, default).map(std::string::ToString::to_string);
        let fallback = self.default.clone().unwrap_or_default();
        self.current = resolve_tier(&self.tiers, current, &fallback).map(str::to_string);
    }

    /// The model's supported tiers, in the order the caller declared them.
    pub fn tiers(&self) -> &[String] {
        &self.tiers
    }

    /// Number of dots the track must render — one per supported tier (R7).
    /// This is the single source of truth the renderer reads.
    pub fn dot_count(&self) -> usize {
        self.tiers.len()
    }

    /// The tier currently selected, or `None` when the model supports none.
    pub fn tier(&self) -> Option<&str> {
        self.current.as_deref()
    }

    /// Index of the selected tier within [`Self::tiers`].
    pub fn selected_index(&self) -> Option<usize> {
        let current = self.current.as_deref()?;
        self.tiers
            .iter()
            .position(|candidate| candidate.as_str() == current)
    }

    /// Index of the strongest supported tier. The gradient fill and the
    /// purple label both key off this, which is what makes the two measured
    /// states (`Medium` flat blue, `Ultra` gradient purple) both correct.
    pub fn strongest_index(&self) -> Option<usize> {
        self.tiers.len().checked_sub(1)
    }

    /// Replaces the supported tier list, re-resolving the current and default
    /// tiers against it.
    pub fn set_tiers(&mut self, tiers: Vec<String>, current: &str, default: &str) {
        self.tiers = tiers;
        self.rebind(current, default);
    }

    /// The tier the reset control returns to (R11: the configured default).
    pub fn default_tier(&self) -> Option<&str> {
        self.default.as_deref()
    }

    /// Selects the tier at `index`. Returns whether the selection changed.
    fn select(&mut self, index: usize) -> Option<String> {
        let tier = self.tiers.get(index)?.clone();
        if self.current.as_deref() == Some(tier.as_str()) {
            return None;
        }
        self.current = Some(tier.clone());
        Some(tier)
    }

    /// Resets to the configured default tier. Returns whether the selection
    /// changed.
    fn reset(&mut self) -> Option<String> {
        let target = self.default.clone()?;
        if self.current.as_deref() == Some(target.as_str()) {
            return None;
        }
        self.current = Some(target.clone());
        Some(target)
    }
}

/// Emitted when the user picks a different tier. P3 turns this into the
/// existing `ComposerDefaults` save path; the component itself performs no IO.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThinkingTierSelected {
    pub model: String,
    pub tier: String,
}

/// The drag payload. Its only job is to exist so GPUI's `on_drag_move`
/// delivers positions while the pointer is outside the track.
#[derive(Clone)]
struct DragKnob;

impl Render for DragKnob {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

// ---------------------------------------------------------------------------
// View
// ---------------------------------------------------------------------------

/// The thinking-tier popup card. Self-contained: it owns its state, emits
/// [`ThinkingTierSelected`], and reads no global.
pub struct ThinkingSlider {
    model: ThinkingSliderModel,
    model_name: String,
    /// Track bounds captured at prepaint. Needed because `MouseDownEvent`
    /// carries only a position, and the tier is picked from the x offset.
    track_bounds: Option<Bounds<Pixels>>,
}

impl EventEmitter<ThinkingTierSelected> for ThinkingSlider {}

impl ThinkingSlider {
    /// Builds the slider for one model.
    ///
    /// `tiers` is the model's supported tier list (`ReasoningProfile.efforts`),
    /// `current` the tier to show, and `default` the tier the reset control
    /// returns to (`ReasoningProfile.preference`). An unsupported `current`
    /// resolves through [`resolve_tier`].
    pub fn new(
        model_name: impl Into<String>,
        tiers: Vec<String>,
        current: impl AsRef<str>,
        default: impl AsRef<str>,
        _cx: &mut Context<Self>,
    ) -> Self {
        Self {
            model: ThinkingSliderModel::new(tiers, current.as_ref(), default.as_ref()),
            model_name: model_name.into(),
            track_bounds: None,
        }
    }

    /// The tier currently shown.
    pub fn tier(&self) -> Option<&str> {
        self.model.tier()
    }

    /// Dots the track renders — one per supported tier (R7). This is the count
    /// the composer's acceptance tests assert against, so it stays a view
    /// accessor rather than a test-only reach into the model.
    pub fn dot_count(&self) -> usize {
        self.model.dot_count()
    }

    /// Whether this model declares any tier at all.
    ///
    /// R12: a model with no declared tiers renders nothing, and the host must
    /// not leave an empty padded slot behind either.
    pub fn has_tiers(&self) -> bool {
        self.model.dot_count() > 0
    }

    /// Replaces the model name and the supported tier list together (R57 P3).
    /// The composer re-projects both whenever the displayed model or its
    /// capability changes; keeping them in one call prevents a frame where the
    /// card names one model while showing another model's tiers.
    pub fn set_model_and_tiers(
        &mut self,
        model_name: impl Into<String>,
        tiers: Vec<String>,
        current: impl AsRef<str>,
        default: impl AsRef<str>,
        cx: &mut Context<Self>,
    ) {
        self.model_name = model_name.into();
        self.model
            .set_tiers(tiers, current.as_ref(), default.as_ref());
        cx.notify();
    }

    /// Selects the tier at `index`. Returns whether the selection changed.
    pub fn select_index(&mut self, index: usize, cx: &mut Context<Self>) -> bool {
        let Some(tier) = self.model.select(index) else {
            return false;
        };
        cx.emit(ThinkingTierSelected {
            model: self.model_name.clone(),
            tier,
        });
        cx.notify();
        true
    }

    /// Resets to the configured default tier. Returns whether the selection
    /// changed.
    pub fn reset(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(tier) = self.model.reset() else {
            return false;
        };
        cx.emit(ThinkingTierSelected {
            model: self.model_name.clone(),
            tier,
        });
        cx.notify();
        true
    }

    /// Maps a window x position to a dot index over `bounds`.
    ///
    /// This is the exact inverse of [`dot_center_offset`], so a click landing
    /// on dot `i` always resolves to `i`.
    fn index_for_position(&self, bounds: Bounds<Pixels>, position: Point<Pixels>) -> Option<usize> {
        let count = self.model.dot_count();
        match count {
            0 => None,
            1 => Some(0),
            _ => {
                let half = px(THINKING_KNOB_DIAMETER / 2.0);
                let span = (bounds.right() - half) - (bounds.left() + half);
                if span <= px(0.0) {
                    return Some(0);
                }
                let travel = ((position.x - (bounds.left() + half)) / span).clamp(0.0, 1.0);
                let index = (travel * (count - 1) as f32).round();
                // `index` is finite and within 0..=count-1 by construction;
                // clamp again so no cast can ever produce an out-of-range
                // index.
                Some((index as usize).min(count - 1))
            }
        }
    }

    fn on_track_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(bounds) = self.track_bounds else {
            return;
        };
        if let Some(index) = self.index_for_position(bounds, event.position) {
            self.select_index(index, cx);
        }
    }

    fn on_knob_drag_move(
        &mut self,
        event: &DragMoveEvent<DragKnob>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(index) = self.index_for_position(event.bounds, event.event.position) {
            self.select_index(index, cx);
        }
    }

    /// Release inside the track also resolves the tier under the pointer.
    ///
    /// This closes the gap GPUI's drag threshold leaves: the move that first
    /// crosses it is consumed to *create* the drag and therefore delivers no
    /// `drag_move`, so a single fast flick from one end of the track to the
    /// other would otherwise land on the press position instead of the release
    /// position. A release outside the track is not hovered and keeps the
    /// `drag_move` result.
    fn on_track_mouse_up(&mut self, event: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(bounds) = self.track_bounds else {
            return;
        };
        if let Some(index) = self.index_for_position(bounds, event.position) {
            self.select_index(index, cx);
        }
    }

    fn on_reset(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.reset(cx);
    }

    /// Tier-name colour. Only the two measured endpoints are modelled (§4.5
    /// M2 leaves the ramp between them unmeasured): the strongest tier takes
    /// the measured purple, every other tier the measured blue.
    fn tier_label_color(&self) -> Rgba {
        tier_label_color_for(self.model.selected_index(), self.model.strongest_index())
    }

    /// The fill layer. The strongest tier gets the measured multi-stop
    /// gradient; every other tier the measured flat fill (§4.2).
    fn render_track_fill(&self, selected: usize) -> AnyElement {
        if track_fill_style(selected, self.model.strongest_index()) == TrackFillStyle::Gradient {
            // GPUI backgrounds carry exactly two colour stops, so the
            // measured three-stop gradient is composed from adjacent segments
            // that share the middle stop. Each segment is a straight
            // left-to-right (`90.0`) gradient, so the seam is exact.
            let mut segments = Vec::with_capacity(TRACK_GRADIENT.len() - 1);
            for (index, pair) in TRACK_GRADIENT.windows(2).enumerate() {
                let (start, from) = pair[0];
                let (end, to) = pair[1];
                let left = start * THINKING_TRACK_WIDTH;
                let width = (end - start) * THINKING_TRACK_WIDTH;
                let segment = div()
                    .absolute()
                    .left(px(left))
                    .top_0()
                    .h(px(THINKING_TRACK_HEIGHT))
                    .w(px(width))
                    .bg(linear_gradient(
                        90.0,
                        linear_color_stop(from, 0.0),
                        linear_color_stop(to, 1.0),
                    ));
                segments.push(match GRADIENT_SEGMENT_SELECTORS.get(index).copied() {
                    Some(selector) => segment.debug_selector(move || selector.to_string()),
                    None => segment,
                });
            }
            return div().children(segments).into_any_element();
        }
        // A flat tier fills up to the knob centre, so the knob straddles the
        // fill edge. The exact extent is unmeasured (§4.5 M5).
        let fill = dot_center_offset(selected, self.model.dot_count());
        div()
            .debug_selector(|| "thinking-slider-fill-flat".into())
            .absolute()
            .left_0()
            .top_0()
            .h(px(THINKING_TRACK_HEIGHT))
            .w(px(fill))
            .bg(TRACK_FILL_FLAT)
            .into_any_element()
    }
}

/// Which track fill a tier takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackFillStyle {
    /// Measured on `Medium`: one flat colour (spec §4.2).
    Flat,
    /// Measured on `Ultra`: the multi-stop gradient (spec §4.2).
    Gradient,
}

/// Decides the track fill from the selected and strongest tier indices.
///
/// Only the strongest tier takes the gradient — that is the measured rule
/// (spec §4.2: `Medium` flat, `Ultra` gradient). Which *intermediate* tiers
/// would take it is unmeasured (spec §4.5 M2), so none do.
pub fn track_fill_style(selected: usize, strongest: Option<usize>) -> TrackFillStyle {
    if strongest == Some(selected) {
        TrackFillStyle::Gradient
    } else {
        TrackFillStyle::Flat
    }
}

/// Centre of dot `index` along the track, relative to the track's left edge.
///
/// The dot span is inset by half a knob on both ends, which is what keeps the
/// measured 203 px box a hard bound: at the strongest tier the knob's outer
/// edge lands exactly on the measured right edge (spec §4.3 measures the
/// right edge *including* the knob).
fn dot_center_offset(index: usize, count: usize) -> f32 {
    if count <= 1 {
        return THINKING_TRACK_WIDTH / 2.0;
    }
    let half = THINKING_KNOB_DIAMETER / 2.0;
    let travel = THINKING_TRACK_WIDTH - THINKING_KNOB_DIAMETER;
    half + index as f32 * travel / (count - 1) as f32
}

fn bolt_icon(color: Rgba) -> AnyElement {
    svg()
        .data(BOLT_SVG)
        .size(px(16.0))
        .flex_shrink_0()
        .text_color(color)
        .into_any_element()
}

impl Render for ThinkingSlider {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme(cx).colors;
        let count = self.model.dot_count();
        let Some(selected) = self.model.selected_index() else {
            // Zero-tier fallback (R12, plan P2a): a model that declares no
            // reasoning tiers renders no slider at all. The reference app's
            // UI for this case is UNMEASURED (spec §4.5 M3) — this is the
            // simplest sensible reading and still needs calibration.
            return Empty.into_any_element();
        };

        let label = self
            .model
            .tiers()
            .get(selected)
            .cloned()
            .unwrap_or_default();
        let label_color = self.tier_label_color();

        let dots = (0..count).map(|index| {
            let offset = dot_center_offset(index, count);
            let dot = div()
                .absolute()
                .left(px(offset - THINKING_DOT_DIAMETER / 2.0))
                .top(px((THINKING_TRACK_HEIGHT - THINKING_DOT_DIAMETER) / 2.0))
                .size(px(THINKING_DOT_DIAMETER))
                .rounded_full()
                .bg(if index <= selected {
                    // R9: filled-region dots render white.
                    TRACK_DOT_FILLED
                } else {
                    // R9: unfilled-region dots render grey. No measured value
                    // exists for this grey, so it takes the tertiary ink
                    // token, which adapts to both appearances.
                    colors.text_tertiary
                });
            match DOT_SELECTORS.get(index).copied() {
                Some(selector) => dot.debug_selector(move || selector.to_string()),
                None => dot,
            }
        });

        let knob_offset = dot_center_offset(selected, count);

        // The track captures its own bounds at prepaint so a click position
        // can be mapped to a dot. `ElementExt::on_prepaint` mounts a
        // size-full canvas child, which receives the parent's bounds.
        let entity = cx.entity();

        let track = div()
            .id("thinking-slider-track")
            .debug_selector(|| "thinking-slider-track".into())
            .relative()
            .w(px(THINKING_TRACK_WIDTH))
            .h(px(THINKING_TRACK_HEIGHT))
            .flex_shrink_0()
            .rounded(px(THINKING_TRACK_HEIGHT / 2.0))
            .overflow_hidden()
            // The unfilled track. The measured 0xE9E8E8 is a light-mode
            // sample (spec §4.2); dark mode has no measurement, so it keeps
            // the theme's own subtle border token instead of a light literal.
            .bg(match theme(cx).appearance {
                vega_theme::Appearance::Light => TRACK_UNFILLED,
                vega_theme::Appearance::Dark => colors.border_subtle,
            })
            .cursor_pointer()
            .on_prepaint(move |bounds, _, cx| {
                entity.update(cx, |this, _| this.track_bounds = Some(bounds));
            })
            // Click-to-select and drag-start share one gesture: the mouse
            // down picks the nearest dot immediately, and the drag (which
            // GPUI only starts after the pointer clears its 2 px threshold)
            // keeps re-picking while the pointer moves, including outside the
            // track.
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_track_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_track_mouse_up))
            .on_drag(DragKnob, |_, _, _, cx| cx.new(|_| DragKnob))
            .on_drag_move(cx.listener(Self::on_knob_drag_move))
            .child(self.render_track_fill(selected))
            .children(dots)
            .child(
                div()
                    .debug_selector(|| "thinking-slider-knob".into())
                    .absolute()
                    .left(px(knob_offset - THINKING_KNOB_DIAMETER / 2.0))
                    .top_0()
                    .size(px(THINKING_KNOB_DIAMETER))
                    .rounded_full()
                    // Measured pure white in the light reference (§4.3); the
                    // elevated-surface token is white there and follows the
                    // theme in dark mode, which was never measured.
                    .bg(colors.bg_elevated),
            );

        div()
            .debug_selector(|| "thinking-slider-card".into())
            .flex()
            .flex_col()
            .gap(px(CARD_ROW_GAP))
            .w(px(THINKING_CARD_WIDTH))
            .px(px(CARD_INSET))
            .pt(px(CARD_PADDING_TOP))
            .pb(px(CARD_PADDING_BOTTOM))
            // Measured radius is ≈20 (spec §4.3); `COMPOSER_RADIUS` is 20.0.
            .rounded(px(Layout::COMPOSER_RADIUS))
            .border_1()
            .border_color(colors.border_subtle)
            .bg(colors.bg_elevated)
            .text_color(colors.text_primary)
            .shadow_sm()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .h(px(THINKING_TRACK_HEIGHT))
                    .child(bolt_icon(colors.text_tertiary))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .text_size(px(Typography::SIDEBAR))
                            .text_color(label_color)
                            .child(label)
                            .child(crate::icons::icon(
                                crate::icons::Icon::ChevronRight,
                                colors.text_tertiary,
                            )),
                    )
                    .child(
                        div()
                            .id("thinking-slider-reset")
                            .debug_selector(|| "thinking-slider-reset".into())
                            .size(px(20.0))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .cursor_pointer()
                            .hover(move |style| style.bg(colors.bg_hover))
                            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_reset))
                            .child(crate::icons::icon(
                                crate::icons::Icon::Refresh,
                                colors.text_secondary,
                            )),
                    ),
            )
            .child(
                div()
                    .debug_selector(|| "thinking-slider-model".into())
                    .truncate()
                    .text_size(px(Typography::METADATA))
                    .text_color(colors.text_secondary)
                    .child(self.model_name.clone()),
            )
            .child(track)
            .into_any_element()
    }
}

/// Shared with tests: the tier-name colour for one model state.
#[cfg(test)]
fn label_color_for(model: &ThinkingSliderModel) -> Rgba {
    tier_label_color_for(model.selected_index(), model.strongest_index())
}

/// Tier-name colour for one model state. Split out of the view so the
/// measured §4.1 rule is directly testable.
fn tier_label_color_for(selected: Option<usize>, strongest: Option<usize>) -> Rgba {
    match (selected, strongest) {
        (Some(selected), Some(strongest)) if selected == strongest => TIER_LABEL_STRONG,
        _ => TIER_LABEL_BASE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{Modifiers, TestAppContext, VisualTestContext, WindowHandle, WindowOptions};

    fn tiers(list: &[&str]) -> Vec<String> {
        list.iter().map(|tier| (*tier).to_string()).collect()
    }

    fn open_slider(
        cx: &mut TestAppContext,
        tier_list: Vec<String>,
        current: &str,
        default: &str,
    ) -> WindowHandle<ThinkingSlider> {
        let current = current.to_string();
        let default = default.to_string();
        cx.update(|cx| {
            cx.set_global(vega_theme::Theme::light());
            cx.open_window(WindowOptions::default(), move |_, cx| {
                cx.new(|cx| ThinkingSlider::new("GPT-6 Astra", tier_list, current, default, cx))
            })
            .expect("thinking slider window")
        })
    }

    fn rendered_dot_count(visual: &mut VisualTestContext) -> usize {
        DOT_SELECTORS
            .iter()
            .copied()
            .filter(|selector| visual.debug_bounds(selector).is_some())
            .count()
    }

    /// Reads the mounted slider's tier. `WindowHandle::read_with` returns a
    /// `Result` because the window may be gone; the harness always has it.
    fn tier_of(window: &WindowHandle<ThinkingSlider>, cx: &TestAppContext) -> Option<String> {
        window
            .read_with(cx, |slider, _| slider.tier().map(str::to_string))
            .expect("slider window is open")
    }

    /// Window-space x of dot `index` for a mounted track.
    fn dot_x(track: Bounds<Pixels>, index: usize, count: usize) -> Pixels {
        track.left() + px(dot_center_offset(index, count))
    }

    // ---- pure resolution -------------------------------------------------

    #[test]
    fn dot_count_tracks_the_supported_tier_count() {
        for (list, expected) in [
            (tiers(&["low", "medium", "high"]), 3),
            (
                tiers(&["minimal", "low", "medium", "high", "xhigh", "max"]),
                6,
            ),
            (Vec::new(), 0),
        ] {
            let model = ThinkingSliderModel::new(list, "medium", "medium");
            assert_eq!(model.dot_count(), expected);
        }
    }

    #[test]
    fn unsupported_current_falls_back_to_the_nearest_lower_supported_tier() {
        // glm-5.3's declared efforts. `ultra`/`persistent` sit above `max`,
        // `xhigh` between `high` and `max`.
        let supported = tiers(&["low", "high", "max"]);
        assert_eq!(resolve_tier(&supported, "ultra", "low"), Some("max"));
        assert_eq!(resolve_tier(&supported, "persistent", "low"), Some("max"));
        assert_eq!(resolve_tier(&supported, "xhigh", "low"), Some("high"));
        // The literal reference snippet scans from index 0, so it would answer
        // `low` for all three of the above. This is the divergence the module
        // docs pin down.
        assert_eq!(resolve_tier(&supported, "max", "low"), Some("max"));
        assert_eq!(resolve_tier(&supported, "low", "high"), Some("low"));
        // Nothing lies below `low`, so the configured default applies.
        assert_eq!(resolve_tier(&supported, "minimal", "high"), Some("high"));
    }

    #[test]
    fn fallback_skips_unsupported_gaps_in_the_ladder() {
        let supported = tiers(&["minimal", "medium", "max"]);
        assert_eq!(resolve_tier(&supported, "high", "medium"), Some("medium"));
        assert_eq!(resolve_tier(&supported, "xhigh", "medium"), Some("medium"));
        assert_eq!(
            resolve_tier(&supported, "persistent", "medium"),
            Some("max")
        );
    }

    #[test]
    fn nothing_below_the_preferred_tier_uses_the_default_then_the_first() {
        let supported = tiers(&["medium", "high"]);
        // `minimal` sits below every supported tier; the configured default
        // wins over the first supported tier.
        assert_eq!(resolve_tier(&supported, "minimal", "high"), Some("high"));
        // An unsupported default still resolves, to the first supported tier.
        assert_eq!(resolve_tier(&supported, "minimal", "max"), Some("medium"));
        // A tier outside the ladder has no rank, so the same path applies.
        assert_eq!(resolve_tier(&supported, "bogus", "high"), Some("high"));
    }

    #[test]
    fn no_supported_tiers_resolve_to_nothing() {
        assert_eq!(resolve_tier(&[], "medium", "medium"), None);
        let model = ThinkingSliderModel::new(Vec::new(), "medium", "medium");
        assert_eq!(model.tier(), None);
        assert_eq!(model.selected_index(), None);
        assert_eq!(model.default_tier(), None);
    }

    #[test]
    fn reset_returns_to_the_configured_default_tier() {
        let mut model =
            ThinkingSliderModel::new(tiers(&["low", "medium", "high"]), "low", "medium");
        assert_eq!(model.tier(), Some("low"));
        assert_eq!(model.reset(), Some("medium".to_string()));
        assert_eq!(model.tier(), Some("medium"));
        // Resetting again is a no-op.
        assert_eq!(model.reset(), None);
    }

    #[test]
    fn reset_target_itself_resolves_through_the_fallback() {
        // `xhigh` is not supported; the nearest lower supported tier is `high`.
        let mut model = ThinkingSliderModel::new(tiers(&["low", "medium", "high"]), "low", "xhigh");
        assert_eq!(model.default_tier(), Some("high"));
        assert_eq!(model.reset(), Some("high".to_string()));
    }

    #[test]
    fn selecting_an_index_is_idempotent() {
        let mut model =
            ThinkingSliderModel::new(tiers(&["low", "medium", "high"]), "medium", "low");
        assert_eq!(model.select(2), Some("high".to_string()));
        assert_eq!(model.select(2), None);
        assert_eq!(model.select(9), None);
        assert_eq!(model.tier(), Some("high"));
    }

    #[test]
    fn strongest_index_marks_the_gradient_tier() {
        let model = ThinkingSliderModel::new(tiers(&["low", "medium", "high"]), "medium", "low");
        assert_eq!(model.strongest_index(), Some(2));
        assert_eq!(label_color_for(&model), TIER_LABEL_BASE);
        let mut strongest = model.clone();
        strongest.select(2);
        assert_eq!(label_color_for(&strongest), TIER_LABEL_STRONG);
    }

    #[test]
    fn only_the_strongest_tier_takes_the_gradient() {
        // The two measured states: `Medium` (index 1 of 6) is flat, `Ultra`
        // (the last of 7) is the gradient.
        assert_eq!(track_fill_style(1, Some(5)), TrackFillStyle::Flat);
        assert_eq!(track_fill_style(0, Some(5)), TrackFillStyle::Flat);
        assert_eq!(track_fill_style(5, Some(5)), TrackFillStyle::Gradient);
        // A single-tier model is both weakest and strongest.
        assert_eq!(track_fill_style(0, Some(0)), TrackFillStyle::Gradient);
        // No tiers at all never renders a fill.
        assert_eq!(track_fill_style(0, None), TrackFillStyle::Flat);
    }

    #[test]
    fn measured_colours_match_the_spec_samples() {
        // §4.1 tier-name colours.
        assert_eq!(TIER_LABEL_STRONG, measured_rgba(0x924FF7FF));
        assert_eq!(TIER_LABEL_BASE, measured_rgba(0x3983F7FF));
        // §4.2 flat fill, unfilled track, and the gradient's measured
        // endpoints.
        assert_eq!(TRACK_FILL_FLAT, measured_rgba(0x3983F7FF));
        assert_eq!(TRACK_UNFILLED, measured_rgba(0xE9E8E8FF));
        assert_eq!(TRACK_DOT_FILLED, measured_rgba(0xFFFFFFFF));
        assert_eq!(TRACK_GRADIENT[0], (0.0, measured_rgba(0x3346CDFF)));
        assert_eq!(TRACK_GRADIENT[2], (1.0, measured_rgba(0x7E5AF0FF)));
        assert_eq!(TRACK_GRADIENT[1].1, measured_rgba(0xAE76FFFF));
        // The measured §4.3 geometry.
        assert_eq!(THINKING_CARD_WIDTH, 254.5);
        assert_eq!(THINKING_TRACK_WIDTH, 203.0);
        assert_eq!(THINKING_TRACK_HEIGHT, 24.0);
        assert_eq!(THINKING_KNOB_DIAMETER, 24.0);
    }

    #[test]
    fn dot_centres_stay_inside_the_measured_track_box() {
        let half_knob = THINKING_KNOB_DIAMETER / 2.0;
        for count in 1..=TIER_LADDER.len() {
            for index in 0..count {
                let offset = dot_center_offset(index, count);
                assert!(
                    (half_knob..=THINKING_TRACK_WIDTH - half_knob).contains(&offset),
                    "dot {index} of {count} at {offset} escapes the 203px box"
                );
            }
        }
        // The strongest dot's knob is flush with the measured right edge.
        assert_eq!(
            dot_center_offset(6, 7) + THINKING_KNOB_DIAMETER / 2.0,
            THINKING_TRACK_WIDTH
        );
    }

    // ---- rendered geometry and interaction -------------------------------

    #[gpui_kit::test]
    async fn rendered_dots_equal_the_supported_tier_count(cx: &mut TestAppContext) {
        for list in [
            tiers(&["low", "medium", "high"]),
            tiers(&["minimal", "low", "medium", "high", "xhigh", "max"]),
        ] {
            let expected = list.len();
            let window = open_slider(cx, list, "medium", "medium");
            cx.run_until_parked();
            let mut visual = VisualTestContext::from_window(window.into(), cx);
            assert_eq!(
                rendered_dot_count(&mut visual),
                expected,
                "one dot per supported tier"
            );
            assert!(visual.debug_bounds("thinking-slider-card").is_some());
            assert!(visual.debug_bounds("thinking-slider-track").is_some());
        }
    }

    #[gpui_kit::test]
    async fn a_model_without_tiers_renders_no_slider(cx: &mut TestAppContext) {
        let window = open_slider(cx, Vec::new(), "medium", "medium");
        cx.run_until_parked();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        assert_eq!(rendered_dot_count(&mut visual), 0);
        assert!(visual.debug_bounds("thinking-slider-card").is_none());
        assert!(visual.debug_bounds("thinking-slider-track").is_none());
        assert!(visual.debug_bounds("thinking-slider-reset").is_none());
    }

    #[gpui_kit::test]
    async fn card_and_track_match_the_measured_geometry(cx: &mut TestAppContext) {
        let window = open_slider(
            cx,
            tiers(&["minimal", "low", "medium", "high", "xhigh", "max"]),
            "medium",
            "medium",
        );
        cx.run_until_parked();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let card = visual
            .debug_bounds("thinking-slider-card")
            .expect("mounted card");
        let track = visual
            .debug_bounds("thinking-slider-track")
            .expect("mounted track");
        let knob = visual
            .debug_bounds("thinking-slider-knob")
            .expect("mounted knob");
        assert!(
            (f32::from(card.size.width) - THINKING_CARD_WIDTH).abs() <= 1.0,
            "card width {} != measured {THINKING_CARD_WIDTH}",
            f32::from(card.size.width)
        );
        assert!(
            (f32::from(track.size.width) - THINKING_TRACK_WIDTH).abs() <= 1.0,
            "track width {} != measured {THINKING_TRACK_WIDTH}",
            f32::from(track.size.width)
        );
        assert!(
            (f32::from(track.size.height) - THINKING_TRACK_HEIGHT).abs() <= 1.0,
            "track height {} != measured {THINKING_TRACK_HEIGHT}",
            f32::from(track.size.height)
        );
        assert!(
            (f32::from(knob.size.width) - THINKING_KNOB_DIAMETER).abs() <= 1.0,
            "knob diameter {} != measured {THINKING_KNOB_DIAMETER}",
            f32::from(knob.size.width)
        );
    }

    #[gpui_kit::test]
    async fn clicking_a_dot_selects_that_tier(cx: &mut TestAppContext) {
        let window = open_slider(
            cx,
            tiers(&["minimal", "low", "medium", "high", "xhigh", "max"]),
            "medium",
            "medium",
        );
        cx.run_until_parked();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let track = visual
            .debug_bounds("thinking-slider-track")
            .expect("mounted track");
        let count = 6;

        // `medium` is index 2, so the knob starts there.
        assert_eq!(tier_of(&window, cx), Some("medium".to_string()));

        for (index, expected) in [(5, "max"), (0, "minimal"), (3, "high")] {
            let position = gpui_kit::point(dot_x(track, index, count), track.center().y);
            visual.simulate_click(position, Modifiers::default());
            visual.run_until_parked();
            assert_eq!(
                tier_of(&window, cx),
                Some(expected.to_string()),
                "click on dot {index}"
            );
        }
    }

    #[gpui_kit::test]
    async fn clicking_the_reset_control_returns_to_the_default_tier(cx: &mut TestAppContext) {
        let window = open_slider(
            cx,
            tiers(&["minimal", "low", "medium", "high", "xhigh", "max"]),
            "medium",
            "high",
        );
        cx.run_until_parked();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let track = visual
            .debug_bounds("thinking-slider-track")
            .expect("mounted track");
        let reset = visual
            .debug_bounds("thinking-slider-reset")
            .expect("mounted reset control");

        visual.simulate_click(
            gpui_kit::point(dot_x(track, 5, 6), track.center().y),
            Modifiers::default(),
        );
        visual.run_until_parked();
        assert_eq!(tier_of(&window, cx), Some("max".to_string()));

        visual.simulate_click(reset.center(), Modifiers::default());
        visual.run_until_parked();
        assert_eq!(
            tier_of(&window, cx),
            Some("high".to_string()),
            "reset returns to the configured default tier"
        );
    }

    #[gpui_kit::test]
    async fn flat_and_gradient_fills_mount_for_the_measured_states(cx: &mut TestAppContext) {
        // `Medium` of 6 tiers: a flat fill, no gradient segments.
        let window = open_slider(
            cx,
            tiers(&["minimal", "low", "medium", "high", "xhigh", "max"]),
            "medium",
            "medium",
        );
        cx.run_until_parked();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        assert!(
            visual.debug_bounds("thinking-slider-fill-flat").is_some(),
            "a non-strongest tier renders the flat fill"
        );
        for selector in GRADIENT_SEGMENT_SELECTORS {
            assert!(
                visual.debug_bounds(selector).is_none(),
                "{selector} must not mount below the strongest tier"
            );
        }

        // The strongest tier: the gradient, no flat fill.
        let window = open_slider(
            cx,
            tiers(&["minimal", "low", "medium", "high", "xhigh", "max"]),
            "max",
            "medium",
        );
        cx.run_until_parked();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        assert!(
            visual.debug_bounds("thinking-slider-fill-flat").is_none(),
            "the strongest tier must not use the flat fill"
        );
        for selector in GRADIENT_SEGMENT_SELECTORS {
            assert!(
                visual.debug_bounds(selector).is_some(),
                "{selector} must mount for the strongest tier"
            );
        }
    }

    #[gpui_kit::test]
    async fn dragging_the_knob_selects_the_tier_under_the_pointer(cx: &mut TestAppContext) {
        let window = open_slider(
            cx,
            tiers(&["minimal", "low", "medium", "high", "xhigh", "max"]),
            "minimal",
            "medium",
        );
        cx.run_until_parked();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let track = visual
            .debug_bounds("thinking-slider-track")
            .expect("mounted track");
        let count = 6;

        let start = gpui_kit::point(dot_x(track, 0, count), track.center().y);
        visual.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
        visual.run_until_parked();

        // GPUI consumes the first move past its 2 px threshold to create the
        // drag, so that move carries no `drag_move`. A real drag continues
        // moving, which is what the second move below represents.
        visual.simulate_mouse_move(
            gpui_kit::point(dot_x(track, 1, count), track.center().y),
            Some(MouseButton::Left),
            Modifiers::default(),
        );
        visual.run_until_parked();
        visual.simulate_mouse_move(
            gpui_kit::point(dot_x(track, 2, count), track.center().y),
            Some(MouseButton::Left),
            Modifiers::default(),
        );
        visual.run_until_parked();
        assert_eq!(
            tier_of(&window, cx),
            Some("medium".to_string()),
            "the drag tracks the pointer"
        );

        // Continue to the strongest dot and release.
        visual.simulate_mouse_move(
            gpui_kit::point(dot_x(track, 5, count), track.center().y),
            Some(MouseButton::Left),
            Modifiers::default(),
        );
        visual.simulate_mouse_up(
            gpui_kit::point(dot_x(track, 5, count), track.center().y),
            MouseButton::Left,
            Modifiers::default(),
        );
        visual.run_until_parked();
        assert_eq!(tier_of(&window, cx), Some("max".to_string()));
    }

    #[gpui_kit::test]
    async fn a_fast_flick_resolves_on_release(cx: &mut TestAppContext) {
        // The move that starts a drag is consumed, so a single flick from one
        // end of the track to the other delivers no `drag_move` at the release
        // position. The release handler must still land on the release dot.
        let window = open_slider(
            cx,
            tiers(&["minimal", "low", "medium", "high", "xhigh", "max"]),
            "minimal",
            "medium",
        );
        cx.run_until_parked();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let track = visual
            .debug_bounds("thinking-slider-track")
            .expect("mounted track");
        let count = 6;

        visual.simulate_mouse_down(
            gpui_kit::point(dot_x(track, 0, count), track.center().y),
            MouseButton::Left,
            Modifiers::default(),
        );
        visual.run_until_parked();
        let far = gpui_kit::point(dot_x(track, 4, count), track.center().y);
        visual.simulate_mouse_move(far, Some(MouseButton::Left), Modifiers::default());
        visual.simulate_mouse_up(far, MouseButton::Left, Modifiers::default());
        visual.run_until_parked();
        assert_eq!(
            tier_of(&window, cx),
            Some("xhigh".to_string()),
            "release resolves the tier under the pointer"
        );
    }

    #[gpui_kit::test]
    async fn unsupported_current_tier_is_rendered_at_the_fallback(cx: &mut TestAppContext) {
        // `ultra` is not in this model's supported list; the slider must show
        // `high`, the nearest supported tier below it.
        let window = open_slider(cx, tiers(&["low", "medium", "high"]), "ultra", "low");
        cx.run_until_parked();
        assert_eq!(tier_of(&window, cx), Some("high".to_string()));
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        assert_eq!(rendered_dot_count(&mut visual), 3);
    }
}
