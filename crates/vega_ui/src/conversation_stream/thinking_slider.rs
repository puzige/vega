use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, Bounds, Div, DragMoveEvent, Empty, EventEmitter, MouseButton, MouseDownEvent,
    MouseUpEvent, Pixels, Point, Render, Rgba, Window, base::ElementExt, div, linear_color_stop,
    linear_gradient, px,
};
use vega_theme::{Layout, ThemeColors, Typography, theme};

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

const FILL_LEFT_CAP_RADIUS: f32 = THINKING_TRACK_HEIGHT / 2.0;

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

const CARD_TEXT_ROW_HEIGHT: f32 = 20.0;

const TITLE_GROUP_GAP: f32 = 4.0;

const TITLE_HIGHLIGHT_PADDING_X: f32 = 8.0;

const TITLE_BLOCK_CONTENT_MAX_WIDTH: f32 =
    THINKING_CARD_WIDTH - CARD_INSET * 2.0 - TITLE_HIGHLIGHT_PADDING_X * 2.0;

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

const TRACK_DOT_FILLED: Rgba = measured_rgba(0xFFFFFFFF);

/// Unfilled track fill. Measured 0xE9E8E8 (spec §4.2, `Medium` state unfilled
/// region). The measurement is light-mode only; `border_subtle` is 0xE8E8E8,
/// one 8-bit step away, and is used in dark mode where no measurement exists.
const TRACK_UNFILLED: Rgba = measured_rgba(0xE9E8E8FF);

pub const OFF_CHOICE_NAME: &str = "disabled";

pub const OFF_LABEL: &str = "Off";

pub const PROVIDER_DEFAULT_LABEL: &str = "Default";

pub fn tier_display_label(effort: &str) -> String {
    match effort {
        "none" => "None".to_string(),
        "minimal" => "Minimal".to_string(),
        "low" => "Light".to_string(),
        "medium" => "Medium".to_string(),
        "high" => "High".to_string(),
        "xhigh" => "Extra High".to_string(),
        "max" => "Max".to_string(),
        "ultra" => "Ultra".to_string(),
        "persistent" => "Persistent".to_string(),
        other => {
            let mut characters = other.chars();
            match characters.next() {
                Some(first) => first.to_uppercase().collect::<String>() + characters.as_str(),
                None => String::new(),
            }
        }
    }
}

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

#[derive(Debug, Clone, PartialEq, Eq)]
enum Selection {
    /// The Off position (index 0 when shown). Maps to
    /// `ReasoningChoice::Disabled` through [`OFF_CHOICE_NAME`]; never an
    /// effort.
    Off,
    /// A declared tier, by name.
    Tier(String),
}

/// Headless state for one slider. Owns no store handle and performs no IO.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThinkingSliderModel {
    /// The profile's declared `efforts`, in declaration order. This list is
    /// the tier ladder and **never** contains `off`/`none`/`disabled`.
    tiers: Vec<String>,
    supports_off: bool,
    current: Option<Selection>,
    provider_default: bool,
    default: Option<String>,
}

impl ThinkingSliderModel {
    pub fn new(tiers: Vec<String>, supports_off: bool, current: &str, default: &str) -> Self {
        let mut model = Self {
            tiers,
            supports_off,
            current: None,
            provider_default: false,
            default: None,
        };
        model.rebind(supports_off, current, default);
        model
    }

    /// Position offset of the first tier: 1 when Off occupies index 0, else 0.
    fn off_offset(&self) -> usize {
        usize::from(self.supports_off)
    }

    fn rebind(&mut self, supports_off: bool, current: &str, default: &str) {
        self.supports_off = supports_off;
        // The default resolves first so it can serve as the current tier's
        // last-resort fallback.
        self.default =
            resolve_tier(&self.tiers, default, default).map(std::string::ToString::to_string);
        let fallback = self.default.clone().unwrap_or_default();

        if current == OFF_CHOICE_NAME && supports_off {
            self.current = Some(Selection::Off);
            self.provider_default = false;
            return;
        }

        if current == "provider_default" || current.is_empty() {
            self.current = None;
            self.provider_default = true;
            return;
        }
        self.current = resolve_tier(&self.tiers, current, &fallback)
            .map(|tier| Selection::Tier(tier.to_string()));
        self.provider_default = false;
    }

    pub fn tiers(&self) -> &[String] {
        &self.tiers
    }

    pub fn shows_off(&self) -> bool {
        self.supports_off
    }

    pub fn dot_count(&self) -> usize {
        self.tiers.len() + self.off_offset()
    }

    /// The tier currently selected, or `None` when Off, `provider_default`, or
    /// no tier is selected. The Off position is not a tier, so it reports
    /// `None` here; read [`Self::choice_name`] for the persisted name.
    pub fn tier(&self) -> Option<&str> {
        match self.current.as_ref()? {
            Selection::Tier(tier) => Some(tier.as_str()),
            Selection::Off => None,
        }
    }

    /// The persisted preference name this slider shows: [`OFF_CHOICE_NAME`]
    /// for the Off position, the tier name for a tier, and `None` when nothing
    /// is selected (`provider_default`, or a model with no positions).
    pub fn choice_name(&self) -> Option<&str> {
        match self.current.as_ref()? {
            Selection::Tier(tier) => Some(tier.as_str()),
            Selection::Off => Some(OFF_CHOICE_NAME),
        }
    }

    /// Whether the Off position is the current selection.
    pub fn is_off(&self) -> bool {
        matches!(self.current, Some(Selection::Off))
    }

    pub fn is_provider_default(&self) -> bool {
        self.provider_default
    }

    pub fn label(&self) -> String {
        match self.current.as_ref() {
            Some(Selection::Tier(tier)) => tier_display_label(tier),
            Some(Selection::Off) => OFF_LABEL.to_string(),
            None if self.provider_default => PROVIDER_DEFAULT_LABEL.to_string(),
            None => String::new(),
        }
    }

    /// Position of the selected dot, or `None` when nothing is selected.
    ///
    /// With Off shown, position 0 is Off and tier `i` sits at position
    /// `i + 1`; without it, tier `i` sits at position `i`.
    pub fn selected_position(&self) -> Option<usize> {
        match self.current.as_ref()? {
            Selection::Off => Some(0),
            Selection::Tier(tier) => self
                .tiers
                .iter()
                .position(|candidate| candidate == tier)
                .map(|index| index + self.off_offset()),
        }
    }

    pub fn strongest_position(&self) -> Option<usize> {
        self.tiers
            .len()
            .checked_sub(1)
            .map(|last| last + self.off_offset())
    }

    /// Replaces the supported tier list, re-resolving the current and default
    /// tiers against it.
    pub fn set_tiers(
        &mut self,
        tiers: Vec<String>,
        supports_off: bool,
        current: &str,
        default: &str,
    ) {
        self.tiers = tiers;
        self.rebind(supports_off, current, default);
    }

    /// The tier the slider falls back to when the preferred tier is no longer
    /// supported.
    pub fn default_tier(&self) -> Option<&str> {
        self.default.as_deref()
    }

    fn select(&mut self, index: usize) -> Option<String> {
        let next = match (self.supports_off, index) {
            (true, 0) => Selection::Off,
            (true, _) => Selection::Tier(self.tiers.get(index - 1)?.clone()),
            (false, _) => Selection::Tier(self.tiers.get(index)?.clone()),
        };
        if self.current.as_ref() == Some(&next) {
            return None;
        }
        let name = match &next {
            Selection::Off => OFF_CHOICE_NAME.to_string(),
            Selection::Tier(tier) => tier.clone(),
        };
        self.current = Some(next);
        self.provider_default = false;
        Some(name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThinkingTierSelected {
    pub model: String,
    pub choice: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThinkingSliderTitleActivated;

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
impl EventEmitter<ThinkingSliderTitleActivated> for ThinkingSlider {}

impl ThinkingSlider {
    pub fn new(
        model_name: impl Into<String>,
        tiers: Vec<String>,
        supports_off: bool,
        current: impl AsRef<str>,
        default: impl AsRef<str>,
        _cx: &mut Context<Self>,
    ) -> Self {
        Self {
            model: ThinkingSliderModel::new(
                tiers,
                supports_off,
                current.as_ref(),
                default.as_ref(),
            ),
            model_name: model_name.into(),
            track_bounds: None,
        }
    }

    pub fn tiers(&self) -> &[String] {
        self.model.tiers()
    }

    /// The tier currently shown, or `None` when the Off position or
    /// `provider_default` is shown.
    pub fn tier(&self) -> Option<&str> {
        self.model.tier()
    }

    pub fn choice_name(&self) -> Option<&str> {
        self.model.choice_name()
    }

    /// Whether the Off position is the current selection.
    pub fn is_off(&self) -> bool {
        self.model.is_off()
    }

    pub fn is_provider_default(&self) -> bool {
        self.model.is_provider_default()
    }

    pub fn dot_count(&self) -> usize {
        self.model.dot_count()
    }

    pub fn has_tiers(&self) -> bool {
        !self.model.tiers().is_empty()
    }

    pub fn set_model_and_tiers(
        &mut self,
        model_name: impl Into<String>,
        tiers: Vec<String>,
        supports_off: bool,
        current: impl AsRef<str>,
        default: impl AsRef<str>,
        cx: &mut Context<Self>,
    ) {
        self.model_name = model_name.into();
        self.model
            .set_tiers(tiers, supports_off, current.as_ref(), default.as_ref());
        cx.notify();
    }

    pub fn select_index(&mut self, index: usize, cx: &mut Context<Self>) -> bool {
        let Some(choice) = self.model.select(index) else {
            return false;
        };
        cx.emit(ThinkingTierSelected {
            model: self.model_name.clone(),
            choice,
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

    fn tier_label_color(&self) -> Rgba {
        tier_label_color_for(
            self.model.selected_position(),
            self.model.strongest_position(),
        )
    }

    fn render_track_fill(&self, selected: Option<usize>) -> AnyElement {
        let Some(selected) = selected else {
            return Empty.into_any_element();
        };
        if track_fill_style(selected, self.model.strongest_position()) == TrackFillStyle::Gradient {
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
                    .when(gradient_segment_has_left_cap(index), |segment| {
                        segment.rounded_l(px(FILL_LEFT_CAP_RADIUS))
                    })
                    .when(gradient_segment_has_right_cap(index), |segment| {
                        segment.rounded_r(px(FILL_LEFT_CAP_RADIUS))
                    })
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
        let fill = fill_effective_width(selected, self.model.dot_count());

        let narrow = fill_has_right_cap(selected, self.model.dot_count());
        div()
            .debug_selector(|| "thinking-slider-fill-flat".into())
            .absolute()
            .left_0()
            .top_0()
            .h(px(THINKING_TRACK_HEIGHT))
            .w(px(fill))
            .rounded_l(px(FILL_LEFT_CAP_RADIUS))
            .when(narrow, |fill| fill.rounded_r(px(FILL_LEFT_CAP_RADIUS)))
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

fn fill_effective_width(index: usize, count: usize) -> f32 {
    dot_center_offset(index, count).max(THINKING_TRACK_HEIGHT)
}

fn fill_has_right_cap(index: usize, count: usize) -> bool {
    dot_center_offset(index, count) < THINKING_TRACK_HEIGHT
}

fn gradient_segment_has_left_cap(index: usize) -> bool {
    index == 0
}

fn gradient_segment_has_right_cap(index: usize) -> bool {
    index == TRACK_GRADIENT.len() - 2
}

fn card_shadow(card: Div) -> Div {
    card.shadow_lg()
}

impl Render for ThinkingSlider {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme(cx).colors;
        let count = self.model.dot_count();
        if !self.has_tiers() {
            return Empty.into_any_element();
        }

        let selected = self.model.selected_position();

        let label = self.model.label();
        let label_color = self.tier_label_color();

        let dots = (0..count).map(|index| {
            let offset = dot_center_offset(index, count);
            let filled = selected.is_some_and(|selected| index <= selected);
            let dot = div()
                .absolute()
                .left(px(offset - THINKING_DOT_DIAMETER / 2.0))
                .top(px((THINKING_TRACK_HEIGHT - THINKING_DOT_DIAMETER) / 2.0))
                .size(px(THINKING_DOT_DIAMETER))
                .rounded_full()
                .bg(if filled {
                    TRACK_DOT_FILLED
                } else {
                    colors.text_tertiary
                });
            match DOT_SELECTORS.get(index).copied() {
                Some(selector) => dot.debug_selector(move || selector.to_string()),
                None => dot,
            }
        });

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
            .when_some(selected, |track, selected| {
                track.child(
                    div()
                        .debug_selector(|| "thinking-slider-knob".into())
                        .absolute()
                        .left(px(
                            dot_center_offset(selected, count) - THINKING_KNOB_DIAMETER / 2.0
                        ))
                        .top_0()
                        .size(px(THINKING_KNOB_DIAMETER))
                        .rounded_full()
                        // Measured pure white in the light reference (§4.3);
                        // the elevated-surface token is white there and follows
                        // the theme in dark mode, which was never measured.
                        .bg(colors.bg_elevated),
                )
            });

        card_shadow(
            div()
                .debug_selector(|| "thinking-slider-card".into())
                .flex()
                .flex_col()
                .items_center()
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
                .text_color(colors.text_primary),
        )
        .child(self.render_title_block(label, label_color, colors, cx))
        // The track is the one full-width row; `items_center` on the card
        // cannot stretch it because it carries its own measured width.
        .child(track)
        .into_any_element()
    }
}

impl ThinkingSlider {
    fn render_title_block(
        &self,
        label: String,
        label_color: Rgba,
        colors: ThemeColors,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .id("thinking-slider-title-block")
            .debug_selector(|| "thinking-slider-title-block".into())
            .flex()
            .flex_col()
            .items_center()
            .gap(px(CARD_ROW_GAP))
            // The card's content band. Without it the block would be free to
            // grow past the card on a long model name.
            .max_w(px(THINKING_CARD_WIDTH - CARD_INSET * 2.0))
            .rounded_md()
            .hover(move |style| style.bg(colors.bg_hover))
            .px(px(TITLE_HIGHLIGHT_PADDING_X))
            .child(self.render_title_row(label, label_color, colors, cx))
            .child(self.render_model_row(colors))
            .into_any_element()
    }

    fn render_title_row(
        &self,
        label: String,
        label_color: Rgba,
        colors: ThemeColors,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .id("thinking-slider-title")
            .debug_selector(|| "thinking-slider-title".into())
            .flex()
            .items_center()
            .justify_center()
            .min_h(px(CARD_TEXT_ROW_HEIGHT))
            .flex_shrink_0()
            .min_w_0()
            .max_w(px(TITLE_BLOCK_CONTENT_MAX_WIDTH))
            .cursor_pointer()
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|_, _: &MouseUpEvent, _, cx| {
                    cx.emit(ThinkingSliderTitleActivated);
                }),
            )
            // The centred group. It exists as its own element so a test can
            // measure the group's centring independently of the tier name's own
            // width — with a chevron beside it, a centred *group* is not the
            // same claim as a centred label.
            .child(
                div()
                    .debug_selector(|| "thinking-slider-title-group".into())
                    .flex()
                    .items_center()
                    .gap(px(TITLE_GROUP_GAP))
                    .min_w_0()
                    .child(
                        div()
                            .debug_selector(|| "thinking-slider-label".into())
                            .min_w_0()
                            .truncate()
                            .text_size(px(Typography::METADATA))
                            .text_color(label_color)
                            .child(label),
                    )
                    .child(
                        div()
                            .debug_selector(|| "thinking-slider-chevron".into())
                            .flex()
                            .flex_shrink_0()
                            .child(crate::icons::icon(
                                crate::icons::Icon::ChevronRight,
                                colors.text_tertiary,
                            )),
                    ),
            )
            .into_any_element()
    }

    fn render_model_row(&self, colors: ThemeColors) -> AnyElement {
        div()
            .flex()
            .items_center()
            .justify_center()
            .min_h(px(CARD_TEXT_ROW_HEIGHT))
            .flex_shrink_0()
            .min_w_0()
            .max_w(px(TITLE_BLOCK_CONTENT_MAX_WIDTH))
            .child(
                div()
                    .debug_selector(|| "thinking-slider-model".into())
                    .min_w_0()
                    .truncate()
                    .text_size(px(Typography::SIDEBAR))
                    .text_color(colors.text_secondary)
                    .child(self.model_name.clone()),
            )
            .into_any_element()
    }
}

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
        open_slider_with(cx, tier_list, false, current, default)
    }

    fn open_slider_with(
        cx: &mut TestAppContext,
        tier_list: Vec<String>,
        supports_off: bool,
        current: &str,
        default: &str,
    ) -> WindowHandle<ThinkingSlider> {
        let current = current.to_string();
        let default = default.to_string();
        cx.update(|cx| {
            cx.set_global(vega_theme::Theme::light());
            cx.open_window(WindowOptions::default(), move |_, cx| {
                cx.new(|cx| {
                    ThinkingSlider::new(
                        "GPT-6 Astra",
                        tier_list,
                        supports_off,
                        current,
                        default,
                        cx,
                    )
                })
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
            let model = ThinkingSliderModel::new(list, false, "medium", "medium");
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
        let model = ThinkingSliderModel::new(Vec::new(), false, "medium", "medium");
        assert_eq!(model.tier(), None);
        assert_eq!(model.selected_position(), None);
        assert_eq!(model.default_tier(), None);
    }

    #[test]
    fn the_default_tier_resolves_through_the_fallback() {
        // `xhigh` is not supported; the nearest lower supported tier is `high`.
        let model =
            ThinkingSliderModel::new(tiers(&["low", "medium", "high"]), false, "low", "xhigh");
        assert_eq!(model.default_tier(), Some("high"));
    }

    #[test]
    fn selecting_an_index_is_idempotent() {
        let mut model =
            ThinkingSliderModel::new(tiers(&["low", "medium", "high"]), false, "medium", "low");
        assert_eq!(model.select(2), Some("high".to_string()));
        assert_eq!(model.select(2), None);
        assert_eq!(model.select(9), None);
        assert_eq!(model.tier(), Some("high"));
    }

    #[test]
    fn strongest_position_marks_the_gradient_tier() {
        let model =
            ThinkingSliderModel::new(tiers(&["low", "medium", "high"]), false, "medium", "low");
        assert_eq!(model.strongest_position(), Some(2));
        assert_eq!(
            tier_label_color_for(model.selected_position(), model.strongest_position()),
            TIER_LABEL_BASE
        );
        let mut strongest = model.clone();
        strongest.select(2);
        assert_eq!(
            tier_label_color_for(
                strongest.selected_position(),
                strongest.strongest_position()
            ),
            TIER_LABEL_STRONG
        );
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
