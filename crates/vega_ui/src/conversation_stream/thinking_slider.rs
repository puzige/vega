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
//!
//! ## The Off position (R58 §2)
//!
//! R58 adds an **Off** position at the far left (tier index 0) for models that
//! declare a true disabled operation. Vega models "thinking disabled" as a
//! *separate* [`ReasoningChoice::Disabled`](vega_conversation::types::ReasoningChoice)
//! reached through the profile's `disabled_wire`, **not** as one entry of the
//! `efforts` sequence the way the reference implementation does. The Off
//! position is therefore **visual only**:
//!
//! - Off is index 0 and occupies a dot like any tier; the dot count becomes
//!   `efforts.len() + 1` whenever it is shown (R1).
//! - It is shown only when the profile declares `supports_disabled` **and** a
//!   matching `disabled_wire`, the same pairing `reasoning.rs` and
//!   `FrozenReasoning::validate` enforce (R2). Otherwise the ladder is exactly
//!   `efforts` and there is no Off.
//! - Selecting Off emits [`OFF_CHOICE_NAME`] (`"disabled"`), never an effort
//!   string, and `"off"`/`"none"`/`"disabled"` are never appended to
//!   [`ThinkingSliderModel::tiers`] (R3).
//! - The Off label is `关闭`, the wording Vega's removed thinking chip already
//!   used (R5). Its colour is **unmeasured** and deliberately reuses the
//!   lowest tier's visual; see [`tier_label_color_for`].

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

/// Radius of the fill layer's two **left** corners (R65 R1).
///
/// The track is a capsule, so the fill's left end has to be a half-circle cap
/// of radius `THINKING_TRACK_HEIGHT / 2.0` — the same shape the track's own
/// right cap draws. A single value covers every tier because GPUI clamps each
/// corner radius per quad to `min(width, height) / 2` (spec §4b): at the
/// narrowest fill (`dot_center_offset(0, n) = 12.0`) the radius clamps to 6.0,
/// which on a 12px-wide, 24px-tall box is exactly the left semicircle. No
/// per-tier branching is needed, and none is wanted — see spec §4b.
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

/// Minimum height of each of the card's two text rows (R62 R2). **Chosen, not
/// measured**: the reference layout fixes the rows' *order* and alignment, not
/// their height, so §6 M1/M4 stay open on it.
///
/// It is a **floor**, applied with `min_h`, not a fixed height: the model name
/// renders a 21px line box (13px `SIDEBAR` at the default line height), which
/// overflows a hard 20px and makes the row taller than the constant claims. As
/// a floor the row takes whichever is larger, so the constant still keeps the
/// two rows visually equal for short text while a larger line box can never be
/// clipped.
const CARD_TEXT_ROW_HEIGHT: f32 = 20.0;

/// Gap between the tier name and its chevron inside the title group (R62 R2).
/// **Chosen, not measured**: it is the 4px `gap_1` the pre-R62 title row used
/// between its two text runs, kept so the group's rhythm is unchanged.
const TITLE_GROUP_GAP: f32 = 4.0;

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

/// The persisted name of the Off position (R58 R3).
///
/// This is the string the composer already maps to
/// `ReasoningChoice::Disabled` (`core.rs`'s `reasoning_choice_for_name`) and
/// the one `reasoning.toml` stores in `ReasoningProfile.preference`. It is a
/// *choice* name, never an effort: it must not be appended to
/// [`ThinkingSliderModel::tiers`] or sent as `Effort`.
pub const OFF_CHOICE_NAME: &str = "disabled";

/// The Off position's tier-name text (R58 R5). Vega's existing wording for
/// `"disabled"`, taken from the thinking chip R57 replaced
/// (`docs/vega-r57-composer-alignment.md` §2.2).
pub const OFF_LABEL: &str = "关闭";

/// The no-selection label, shown when the persisted preference is
/// `provider_default` (R58 R6). Vega's existing wording for that state.
pub const PROVIDER_DEFAULT_LABEL: &str = "提供方默认";

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

/// What the slider currently shows. R58 R3/R6: the Off position and the
/// `provider_default` state are deliberately **not** entries of
/// [`ThinkingSliderModel::tiers`] — the tier list stays exactly the profile's
/// `efforts`, and only the *position* mapping knows about Off.
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
    /// Whether the profile declares a true disabled operation, i.e.
    /// `supports_disabled` **and** a matching `disabled_wire` (R2). When
    /// `false` there is no Off position at all.
    supports_off: bool,
    current: Option<Selection>,
    /// Whether the persisted preference is `provider_default` (R6). That is a
    /// third state — "let the provider decide" — and is not a position, so the
    /// track renders with nothing selected.
    provider_default: bool,
    default: Option<String>,
}

impl ThinkingSliderModel {
    /// Builds the model, resolving both the current and the default tier
    /// against `tiers` straight away.
    ///
    /// `supports_off` is the R2 pairing (`supports_disabled` with a declared
    /// `disabled_wire`); `current` is the persisted preference name, where
    /// [`OFF_CHOICE_NAME`] selects the Off position and `"provider_default"`
    /// selects nothing.
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
        // R4: `"disabled"` maps to the Off position. It is only representable
        // when the profile declares the disabled operation; a profile that
        // somehow persists it without the capability falls through to the
        // ordinary unsupported-preference fallback instead of showing an Off
        // position the provider would reject (R2).
        if current == OFF_CHOICE_NAME && supports_off {
            self.current = Some(Selection::Off);
            self.provider_default = false;
            return;
        }
        // R6: `provider_default` is not a position. The track renders no
        // selection and the label names the state itself.
        if current == "provider_default" || current.is_empty() {
            self.current = None;
            self.provider_default = true;
            return;
        }
        self.current = resolve_tier(&self.tiers, current, &fallback)
            .map(|tier| Selection::Tier(tier.to_string()));
        self.provider_default = false;
    }

    /// The model's supported tiers, in the order the caller declared them.
    /// This is the profile's `efforts` verbatim — the Off position is not a
    /// member (R3).
    pub fn tiers(&self) -> &[String] {
        &self.tiers
    }

    /// Whether this model declares a true disabled operation, i.e. whether the
    /// Off position is shown at all (R2).
    pub fn shows_off(&self) -> bool {
        self.supports_off
    }

    /// Number of dots the track must render — one per supported tier, plus one
    /// for the Off position when it is shown (R1). This is the single source of
    /// truth the renderer reads.
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

    /// Whether the persisted preference is `provider_default` (R6).
    pub fn is_provider_default(&self) -> bool {
        self.provider_default
    }

    /// The tier-name text for the current state.
    pub fn label(&self) -> &str {
        match self.current.as_ref() {
            Some(Selection::Tier(tier)) => tier.as_str(),
            Some(Selection::Off) => OFF_LABEL,
            None if self.provider_default => PROVIDER_DEFAULT_LABEL,
            None => "",
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

    /// Position of the strongest supported tier. The gradient fill and the
    /// purple label both key off this, which is what makes the two measured
    /// states (`Medium` flat blue, `Ultra` gradient purple) both correct.
    ///
    /// The Off position is never the strongest tier, so an Off-only track has
    /// no strongest position and therefore no gradient (R5: Off takes the
    /// lowest tier's visual, which is the flat fill).
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

    /// Selects the position at `index`. Returns the persisted choice name when
    /// the selection changed.
    ///
    /// Position 0 is the Off position when the model shows one, and maps to
    /// [`OFF_CHOICE_NAME`] — never to an effort (R3).
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

/// Emitted when the user picks a different position. The composer turns this
/// into the existing `ComposerDefaults` save path; the component itself
/// performs no IO.
///
/// `choice` is a persisted *choice name*, not necessarily an effort: the Off
/// position emits [`OFF_CHOICE_NAME`] (`"disabled"`), which the composer maps
/// to `ReasoningChoice::Disabled` (R3). It is never `"off"`/`"none"`, and it
/// is never appended to the tier list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThinkingTierSelected {
    pub model: String,
    pub choice: String,
}

/// Emitted when the user activates the card's title row — the
/// `model name + tier name + >` block (R59 R2).
///
/// This is the **only** way level two is reached: the host answers it by
/// swapping this card out for the model list (R59 R3), so the two levels are
/// never on screen together. The reference implementation's
/// `composer.modelPicker.modelList.open.ariaLabel` ("Accessible label for the
/// selected-model action **above the model-picker slider**, which opens the
/// list of available models") is the same control.
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
    /// Builds the slider for one model.
    ///
    /// `tiers` is the model's supported tier list (`ReasoningProfile.efforts`),
    /// `supports_off` whether the Off position is shown (`supports_disabled`
    /// with a declared `disabled_wire`, R2), and `current` the persisted
    /// preference name ([`OFF_CHOICE_NAME`], `"provider_default"`, or a tier).
    /// An unsupported `current` resolves through [`resolve_tier`].
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

    /// The model's supported tiers, exactly the profile's `efforts`.
    ///
    /// Exposed so acceptance tests can prove the Off position never becomes a
    /// member of this list (R3).
    pub fn tiers(&self) -> &[String] {
        self.model.tiers()
    }

    /// The tier currently shown, or `None` when the Off position or
    /// `provider_default` is shown.
    pub fn tier(&self) -> Option<&str> {
        self.model.tier()
    }

    /// The persisted preference name this slider shows
    /// ([`OFF_CHOICE_NAME`] for Off, else a tier name), or `None` when nothing
    /// is selected (R3/R6).
    pub fn choice_name(&self) -> Option<&str> {
        self.model.choice_name()
    }

    /// Whether the Off position is the current selection.
    pub fn is_off(&self) -> bool {
        self.model.is_off()
    }

    /// Whether the persisted preference is `provider_default` (R6).
    pub fn is_provider_default(&self) -> bool {
        self.model.is_provider_default()
    }

    /// Dots the track renders — one per supported tier, plus one for the Off
    /// position when the model declares a disabled operation (R1). This is the
    /// count the composer's acceptance tests assert against, so it stays a
    /// view accessor rather than a test-only reach into the model.
    pub fn dot_count(&self) -> usize {
        self.model.dot_count()
    }

    /// Whether this model declares any tier at all.
    ///
    /// R12: a model with no declared tiers renders nothing, and the host must
    /// not leave an empty padded slot behind either. Note that Off alone is
    /// *not* a tier: a profile with `supports_disabled` but no `efforts` still
    /// renders nothing, because the ladder the slider moves along would be
    /// empty (R2's pairing is necessary but not sufficient on its own).
    pub fn has_tiers(&self) -> bool {
        !self.model.tiers().is_empty()
    }

    /// Replaces the model name and the supported tier list together (R57 P3).
    /// The composer re-projects both whenever the displayed model or its
    /// capability changes; keeping them in one call prevents a frame where the
    /// card names one model while showing another model's tiers.
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

    /// Selects the position at `index`, where index 0 is the Off position
    /// whenever the model shows one (R1). Returns whether the selection
    /// changed.
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

    /// Tier-name colour. Only the two measured endpoints are modelled (§4.5
    /// M2 leaves the ramp between them unmeasured): the strongest tier takes
    /// the measured purple, every other position the measured blue.
    ///
    /// R58 R5: the Off position is **unmeasured**. It deliberately takes the
    /// same visual as the lowest tier — the measured blue, flat fill — rather
    /// than a new colour, and is flagged as needing calibration.
    fn tier_label_color(&self) -> Rgba {
        tier_label_color_for(
            self.model.selected_position(),
            self.model.strongest_position(),
        )
    }

    /// The fill layer. The strongest tier gets the measured multi-stop
    /// gradient; every other position the measured flat fill (§4.2).
    ///
    /// With nothing selected (R6's `provider_default`) the track renders no
    /// fill at all, so the unselected state is visibly distinct from any
    /// position.
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
                    // R65 R1: only the segment flush with the track's left edge
                    // carries the capsule's left cap; the inner segment keeps
                    // its square corners so the two halves abut seamlessly.
                    .when(index == 0, |segment| {
                        segment.rounded_l(px(FILL_LEFT_CAP_RADIUS))
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
        let fill = dot_center_offset(selected, self.model.dot_count());
        div()
            .debug_selector(|| "thinking-slider-fill-flat".into())
            .absolute()
            .left_0()
            .top_0()
            .h(px(THINKING_TRACK_HEIGHT))
            .w(px(fill))
            // R65 R1: the fill's left end must be a capsule cap, not a square
            // corner. `overflow_hidden` on the track is a pure rectangle clip
            // (spec §4), so the track's own `rounded` never reaches this child
            // and the fill has to carry the radius itself.
            //
            // GPUI clamps each corner to `min(width, height) / 2` per quad
            // (spec §4b), so this one value is correct at every tier: at the
            // narrowest fill (12.0px) it clamps to 6.0, which on a 12px-wide
            // box is exactly the left semicircle.
            .rounded_l(px(FILL_LEFT_CAP_RADIUS))
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

/// R62 R4: the card's own elevation, one tier above the `shadow_sm()` the rest
/// of Vega's popovers use.
///
/// R61 anchored this card to the model trigger, which makes it overlap the
/// composer card (R61 §2 accepted that). At `shadow_sm()` the composer card's
/// top border and rounded corner read as a line crossing the slider card — the
/// user's D1 "looks translucent" / D2 "left side is cut off" reports, whose
/// real cause is an elevation too weak to separate the layers (R62 §1).
///
/// `shadow_lg` rather than `shadow_md`: `shadow_md`'s 6px blur at 10% opacity
/// is only one step above `shadow_sm` and still reads as a hairline where the
/// two cards cross, while `shadow_lg`'s 15px blur with a 3px negative spread
/// produces a clear dark transition on the card's left edge and bottom without
/// bleeding far past the card. This is a GPUI framework tier, not a Vega token
/// — Vega has no `shadow_md`/`shadow_lg` wrapper (R62 R4 note).
///
/// The card's background stays **opaque** `bg_elevated`; R4 forbids solving the
/// separation with translucency.
fn card_shadow(card: Div) -> Div {
    card.shadow_lg()
}

impl Render for ThinkingSlider {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme(cx).colors;
        let count = self.model.dot_count();
        if !self.has_tiers() {
            // Zero-tier fallback (R12, plan P2a): a model that declares no
            // reasoning tiers renders no slider at all. The reference app's
            // UI for this case is UNMEASURED (spec §4.5 M3) — this is the
            // simplest sensible reading and still needs calibration. The Off
            // position alone does not make a ladder, so it does not rescue
            // this case (R58 R2).
            return Empty.into_any_element();
        }
        // R6: `provider_default` selects no position. The track still renders
        // (the model does declare tiers) but carries no knob and no fill, and
        // the label names the state so the user can tell it apart from Off.
        let selected = self.model.selected_position();

        let label = self.model.label().to_string();
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

        // R62 R2: three stacked rows — tier name + chevron, model name, track.
        //
        // The tier row is the clickable control that opens the model list (R59
        // R2/R3); it is built by `render_title_row` so the event it emits stays
        // in one place. The model row is *not* clickable — see that function's
        // docs for why the two are kept apart.
        //
        // R6: both text rows carry `min_w_0` + `truncate`, so a long tier or
        // model name ellipsises inside the measured 254.5px card instead of
        // spilling past its edge. `min_w_0` is what lets a flex child shrink
        // below its content width; without it `truncate` never engages.
        //
        // R62 R4: `card_shadow` supplies the stronger elevation that makes the
        // card read as floating above the composer card it overlaps.
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
                // R62 R4: the surface stays fully opaque. The card's separation
                // from the composer card comes from `card_shadow` alone.
                .bg(colors.bg_elevated)
                .text_color(colors.text_primary),
        )
        .child(self.render_title_row(label, label_color, colors, cx))
        .child(self.render_model_row(colors))
        // The track is the one full-width row; `items_center` on the card
        // cannot stretch it because it carries its own measured width.
        .child(track)
        .into_any_element()
    }
}

impl ThinkingSlider {
    /// R62 R2 row 1: the tier name and its `>` chevron, centred, and the only
    /// control that opens the model list.
    ///
    /// R59 R2/R3 kept the title row as the level-two entry point; R62 R3 keeps
    /// that meaning exactly, so the `ThinkingSliderTitleActivated` emit below is
    /// the same event on the same condition. Only the row's contents changed:
    /// the bolt icon is gone (R1) and the model name moved to its own row.
    ///
    /// The row spans the card's content width so the whole band stays clickable
    /// — the hover fill and the hit area are the full row, not just the text —
    /// while `justify_center` centres the `tier name + chevron` group inside it
    /// (R2 row 1).
    ///
    /// **Row 2 is deliberately not clickable** (R3's first option). Reasons:
    /// the reference implementation's own two rows are separate elements and
    /// only the tier row carries the chevron, which is the affordance that
    /// says "this opens something"; and folding the model row into the same hit
    /// area would put a click target *below* the control the user reads as the
    /// button, so a click aimed at the model name would silently drill down.
    /// Leaving row 2 inert keeps the hit area identical to the affordance.
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
            .w_full()
            .min_h(px(CARD_TEXT_ROW_HEIGHT))
            .flex_shrink_0()
            .min_w_0()
            .cursor_pointer()
            .rounded_md()
            .hover(move |style| style.bg(colors.bg_hover))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|_, _: &MouseUpEvent, _, cx| {
                    // R59 R3: the host answers this by swapping this card
                    // out for the model list, so the two levels are never
                    // on screen together.
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
                    // R62 R2: the tier name keeps R57's **measured** strength
                    // ramp (`tier_label_color`: purple at the strongest tier,
                    // blue below it, spec §4.1) — R57 §3.4 R8 froze that and
                    // R62 does not change it.
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

    /// R62 R2 row 2: the model name, centred, in `text_secondary`.
    ///
    /// R62 R3 keeps this row **inert**: no id, no cursor, no hover, no mouse
    /// handler. It stays inside the card's own hit area (the layer's
    /// `occlude()`), so a click on it is swallowed by the card rather than
    /// falling through to the composer behind it — it simply does nothing.
    ///
    /// The `debug_selector` sits on the *inner* truncated box rather than on the
    /// full-width row, so the measured bounds are the model name's own text run.
    /// That is what makes the centring assertion (A3) meaningful: a full-width
    /// row would be centred by construction.
    fn render_model_row(&self, colors: ThemeColors) -> AnyElement {
        div()
            .flex()
            .items_center()
            .justify_center()
            .w_full()
            .min_h(px(CARD_TEXT_ROW_HEIGHT))
            .flex_shrink_0()
            .min_w_0()
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

/// Tier-name colour for one model state. Split out of the view so the
/// measured §4.1 rule is directly testable.
///
/// R58 R5: the Off position is **unmeasured**. It is not a tier and never the
/// strongest, so it falls to [`TIER_LABEL_BASE`] — the same visual as the
/// lowest tier. That is a deliberate placeholder, not a measurement; see
/// spec §5 M1/M2.
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
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

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

    /// Opens the slider with an explicit Off capability (R58 R2).
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

    /// Reads the mounted slider's persisted choice name (R58 R3).
    fn choice_of(window: &WindowHandle<ThinkingSlider>, cx: &TestAppContext) -> Option<String> {
        window
            .read_with(cx, |slider, _| slider.choice_name().map(str::to_string))
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

    /// R58 R1: with Off shown the dot count is `efforts.len() + 1` and Off
    /// occupies position 0. The tier list itself is untouched, which is what
    /// keeps Off out of `efforts` (R3).
    #[test]
    fn off_position_adds_one_dot_and_leaves_the_tier_list_alone() {
        let model = ThinkingSliderModel::new(
            tiers(&["low", "medium", "high"]),
            true,
            OFF_CHOICE_NAME,
            "low",
        );
        assert!(model.shows_off());
        assert_eq!(model.dot_count(), 4, "three efforts plus the Off position");
        assert_eq!(
            model.tiers(),
            tiers(&["low", "medium", "high"]).as_slice(),
            "Off must never be appended to the effort list"
        );
        assert!(
            model
                .tiers()
                .iter()
                .all(|tier| !matches!(tier.as_str(), "off" | "none" | "disabled"))
        );
        assert_eq!(model.selected_position(), Some(0), "Off is the far left");
        assert!(model.is_off());
        assert_eq!(model.choice_name(), Some(OFF_CHOICE_NAME));
        assert_eq!(model.label(), OFF_LABEL);
        assert_eq!(
            model.tier(),
            None,
            "Off is not a tier, so it reports no tier name"
        );
    }

    /// R58 R2: without a declared disabled operation there is no Off position,
    /// and the dot count is exactly `efforts.len()`.
    #[test]
    fn no_off_position_without_the_disabled_capability() {
        let model = ThinkingSliderModel::new(
            tiers(&["low", "medium", "high"]),
            false,
            // Even a persisted `"disabled"` cannot conjure the position: the
            // provider would reject the request, so the slider falls back.
            OFF_CHOICE_NAME,
            "low",
        );
        assert!(!model.shows_off());
        assert_eq!(model.dot_count(), 3);
        assert!(!model.is_off());
        assert_ne!(model.choice_name(), Some(OFF_CHOICE_NAME));
        assert_eq!(
            model.choice_name(),
            Some("low"),
            "an unusable disabled preference falls back to the default tier"
        );
    }

    /// R58 R3: selecting position `i` yields the tier `efforts[i - 1]` when Off
    /// is shown, and `efforts[i]` when it is not. Selecting position 0 yields
    /// `"disabled"` — a choice name, never an effort.
    #[test]
    fn selecting_positions_maps_off_to_disabled_and_tiers_to_efforts() {
        let mut with_off =
            ThinkingSliderModel::new(tiers(&["low", "medium", "high"]), true, "medium", "medium");
        assert_eq!(
            with_off.selected_position(),
            Some(2),
            "medium is position 2"
        );
        assert_eq!(with_off.select(0), Some(OFF_CHOICE_NAME.to_string()));
        assert!(with_off.is_off());
        assert_eq!(with_off.tier(), None);
        assert_eq!(with_off.select(1), Some("low".to_string()));
        assert_eq!(with_off.select(2), Some("medium".to_string()));
        assert_eq!(with_off.select(3), Some("high".to_string()));
        // Out of range changes nothing.
        assert_eq!(with_off.select(4), None);
        assert_eq!(with_off.select(9), None);
        assert_eq!(with_off.tier(), Some("high"));

        let mut without_off =
            ThinkingSliderModel::new(tiers(&["low", "medium", "high"]), false, "low", "low");
        assert_eq!(without_off.selected_position(), Some(0));
        assert_eq!(without_off.select(0), None, "already selected");
        assert_eq!(without_off.select(2), Some("high".to_string()));
    }

    /// R58 R4: `"disabled"` in the persisted preference maps to the Off
    /// position, and `"provider_default"` maps to no position at all (R6).
    #[test]
    fn persisted_preference_maps_onto_the_off_position() {
        let off = ThinkingSliderModel::new(tiers(&["low", "high"]), true, "disabled", "low");
        assert!(off.is_off());
        assert_eq!(off.selected_position(), Some(0));
        assert_eq!(off.choice_name(), Some(OFF_CHOICE_NAME));
        assert!(!off.is_provider_default());

        // R6: provider default is a third state, not a position.
        let default =
            ThinkingSliderModel::new(tiers(&["low", "high"]), true, "provider_default", "low");
        assert!(!default.is_off());
        assert!(default.is_provider_default());
        assert_eq!(default.selected_position(), None, "nothing is selected");
        assert_eq!(default.choice_name(), None);
        assert_eq!(default.label(), PROVIDER_DEFAULT_LABEL);
        assert_eq!(default.dot_count(), 3, "the ladder still renders");
    }

    /// R58 R1/R5: the Off position never takes the strongest tier's gradient —
    /// it is the lowest visual, so its label stays the base colour.
    #[test]
    fn off_takes_the_lowest_tier_visual() {
        let off = ThinkingSliderModel::new(tiers(&["low", "high"]), true, "disabled", "low");
        assert_eq!(
            tier_label_color_for(off.selected_position(), off.strongest_position()),
            TIER_LABEL_BASE
        );
        // The strongest position with Off shown is the last tier, not Off.
        assert_eq!(off.strongest_position(), Some(2));
        assert_eq!(
            track_fill_style(
                off.selected_position().unwrap_or_default(),
                off.strongest_position()
            ),
            TrackFillStyle::Flat
        );
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

    /// R58 R2: Off alone is not a ladder. A profile that declares the disabled
    /// operation but no efforts still renders no slider (R12), because there is
    /// nothing to slide between.
    #[test]
    fn off_alone_does_not_make_a_slider() {
        let model = ThinkingSliderModel::new(Vec::new(), true, OFF_CHOICE_NAME, "");
        assert!(model.shows_off());
        assert_eq!(model.dot_count(), 1, "the Off position is the only dot");
        assert!(model.tiers().is_empty());
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

    // ---- R65: the fill layer's left cap ----------------------------------

    /// `dot_center_offset` for a six-tier ladder, read off the formula before
    /// R65 gave the fill layer a radius. R65 is a paint-only change and must
    /// not move a single dot (spec §5 R2/R3, §6 A4).
    const R65_SIX_TIER_BASELINE: [f32; 6] = [12.0, 47.8, 83.6, 119.4, 155.2, 191.0];

    /// The same baseline with Off shown — seven positions, matching spec §4b's
    /// per-tier table.
    const R65_SEVEN_TIER_BASELINE: [f32; 7] = [12.0, 41.83, 71.67, 101.5, 131.33, 161.17, 191.0];

    /// R65 A4: every tier's dot centre is identical to the pre-change value,
    /// with and without the Off position.
    ///
    /// The literals above are the *pre-change* outputs, not a re-evaluation of
    /// the formula, so this test can actually fail if `dot_center_offset` or
    /// the frozen constants move. Recomputing them here would make it
    /// tautological.
    #[test]
    fn r65_a4_dot_centres_match_the_pre_change_baseline() {
        for (index, expected) in R65_SIX_TIER_BASELINE.iter().enumerate() {
            let actual = dot_center_offset(index, 6);
            assert!(
                (actual - expected).abs() <= 0.01,
                "R65 A4: six-tier dot {index} moved to {actual} from the \
                 pre-change baseline {expected}"
            );
        }
        for (index, expected) in R65_SEVEN_TIER_BASELINE.iter().enumerate() {
            let actual = dot_center_offset(index, 7);
            assert!(
                (actual - expected).abs() <= 0.01,
                "R65 A4: seven-tier dot {index} moved to {actual} from the \
                 pre-change baseline {expected}"
            );
        }
        // A one-dot ladder centres its single dot, and the strongest dot's
        // knob stays flush with the measured right edge (R3: 203px is a hard
        // bound).
        assert!((dot_center_offset(0, 1) - 101.5).abs() <= 0.01);
        assert_eq!(
            dot_center_offset(5, 6) + THINKING_KNOB_DIAMETER / 2.0,
            THINKING_TRACK_WIDTH
        );
    }

    /// R65 §4b: **one** radius value covers every tier, because GPUI clamps
    /// each corner per quad to `min(width, height) / 2`. At the narrowest fill
    /// (`dot_center_offset(0, n) = 12.0`) that clamp lands on 6.0 — the left
    /// semicircle of a 12px-wide, 24px-tall box; at every wider tier the full
    /// 12.0 cap draws.
    ///
    /// This pins the **arithmetic the spec determined** (the constant and the
    /// clamp), NOT the painted cap. The test platform has no headless
    /// renderer, so the rendered shape is not observable here — spec §6 says
    /// exactly that, and the shape proof is a native pixel scan. Do not read
    /// this test as covering A1/A2.
    #[test]
    fn r65_the_left_cap_radius_clamps_to_the_semicircle_at_the_narrowest_fill() {
        assert_eq!(FILL_LEFT_CAP_RADIUS, 12.0);
        assert_eq!(FILL_LEFT_CAP_RADIUS, THINKING_TRACK_HEIGHT / 2.0);

        let narrowest = dot_center_offset(0, 6);
        assert_eq!(narrowest, 12.0, "the lowest tier's fill is 12px wide");
        let clamped = FILL_LEFT_CAP_RADIUS.min(narrowest.min(THINKING_TRACK_HEIGHT) / 2.0);
        assert_eq!(clamped, 6.0, "a 12px-wide fill clamps to a 6px radius");

        // Every other tier is at least two radii wide, so its cap is a full
        // semicircle rather than a clamp.
        for index in 1..6 {
            let width = dot_center_offset(index, 6);
            assert!(
                width >= FILL_LEFT_CAP_RADIUS * 2.0,
                "tier {index}'s fill ({width}px) is narrower than the full cap"
            );
        }
    }

    /// R65 A4, rendered: the fill layer still begins flush with the track's
    /// left edge and still spans exactly `dot_center_offset` for every tier.
    ///
    /// A corner radius is paint-only, so it must not move or resize the fill.
    /// This is the rendered half of A4 — it says nothing about the painted
    /// cap, which this harness cannot observe.
    #[gpui_kit::test]
    async fn r65_a4_the_fill_keeps_its_left_edge_and_width_at_every_tier(cx: &mut TestAppContext) {
        let ladder = tiers(&["minimal", "low", "medium", "high", "xhigh", "max"]);
        let strongest = ladder.len() - 1;
        for (selected, tier_name) in ladder.iter().enumerate() {
            let window = open_slider(cx, ladder.clone(), tier_name, "medium");
            cx.run_until_parked();
            let mut visual = VisualTestContext::from_window(window.into(), cx);
            let track = visual
                .debug_bounds("thinking-slider-track")
                .expect("mounted track");
            let expected = dot_center_offset(selected, ladder.len());

            if selected == strongest {
                // The strongest tier takes the gradient: its first segment
                // must still start at the track's left edge, and the segment
                // widths must still sum to the measured track width. The
                // measured stops and their positions are unchanged (R2).
                let first = visual
                    .debug_bounds(GRADIENT_SEGMENT_SELECTORS[0])
                    .expect("mounted first gradient segment");
                let second = visual
                    .debug_bounds(GRADIENT_SEGMENT_SELECTORS[1])
                    .expect("mounted second gradient segment");
                assert!(
                    (f32::from(first.left() - track.left())).abs() <= 0.5,
                    "R65 A4: the gradient's first segment must begin at the \
                     track's left edge: segment {:?} vs track {:?}",
                    first.left(),
                    track.left()
                );
                let spanned = f32::from(first.size.width) + f32::from(second.size.width);
                assert!(
                    (spanned - THINKING_TRACK_WIDTH).abs() <= 1.0,
                    "R65 A4/R2: the gradient must still span the measured \
                     track: {spanned}px != {THINKING_TRACK_WIDTH}px"
                );
                continue;
            }

            let fill = visual
                .debug_bounds("thinking-slider-fill-flat")
                .expect("a non-strongest tier renders the flat fill");
            assert!(
                (f32::from(fill.left() - track.left())).abs() <= 0.5,
                "R65 A4: the fill must stay flush with the track's left edge \
                 at tier {selected}: fill {:?} vs track {:?}",
                fill.left(),
                track.left()
            );
            assert!(
                (f32::from(fill.size.width) - expected).abs() <= 0.5,
                "R65 A4: tier {selected}'s fill must still span \
                 dot_center_offset = {expected}px, got {}",
                f32::from(fill.size.width)
            );
        }
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
        // R58 R7: the reset control is gone from every state.
        assert!(visual.debug_bounds("thinking-slider-reset").is_none());
    }

    /// R58 R7: the reset control no longer exists, and its removal is a
    /// structural change rather than a hidden element.
    #[gpui_kit::test]
    async fn the_reset_control_is_removed(cx: &mut TestAppContext) {
        let window = open_slider(
            cx,
            tiers(&["minimal", "low", "medium", "high", "xhigh", "max"]),
            "medium",
            "high",
        );
        cx.run_until_parked();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        assert!(visual.debug_bounds("thinking-slider-card").is_some());
        assert!(
            visual.debug_bounds("thinking-slider-reset").is_none(),
            "R58 R7 removes the reset control entirely"
        );
        // The header still renders its tier label (R62 R1 removed only the bolt).
        assert!(visual.debug_bounds("thinking-slider-label").is_some());
    }

    /// R58 R1/A1: with Off shown the track renders `efforts.len() + 1` dots and
    /// the leftmost dot is the Off position.
    #[gpui_kit::test]
    async fn off_adds_a_leftmost_dot_when_the_model_supports_it(cx: &mut TestAppContext) {
        let window = open_slider_with(
            cx,
            tiers(&["minimal", "low", "medium", "high", "xhigh", "max"]),
            true,
            OFF_CHOICE_NAME,
            "medium",
        );
        cx.run_until_parked();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        assert_eq!(
            rendered_dot_count(&mut visual),
            7,
            "six efforts plus the Off position"
        );
        // The knob sits on the leftmost dot, i.e. Off.
        let track = visual
            .debug_bounds("thinking-slider-track")
            .expect("mounted track");
        let knob = visual
            .debug_bounds("thinking-slider-knob")
            .expect("mounted knob");
        let off_x = dot_x(track, 0, 7);
        assert!(
            (f32::from(knob.center().x) - f32::from(off_x)).abs() <= 1.0,
            "the knob must rest on the Off position at {off_x:?}, not {:?}",
            knob.center()
        );
        assert!(
            window
                .read_with(cx, |slider, _| slider.is_off())
                .unwrap_or(false)
        );
        assert_eq!(choice_of(&window, cx), Some(OFF_CHOICE_NAME.to_string()));
    }

    /// R58 R2/A2: without the disabled capability the Off position is absent
    /// and the dot count is exactly `efforts.len()`.
    #[gpui_kit::test]
    async fn no_off_dot_without_the_disabled_capability(cx: &mut TestAppContext) {
        let window = open_slider_with(
            cx,
            tiers(&["minimal", "low", "medium", "high", "xhigh", "max"]),
            false,
            "minimal",
            "medium",
        );
        cx.run_until_parked();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        assert_eq!(rendered_dot_count(&mut visual), 6);
        let track = visual
            .debug_bounds("thinking-slider-track")
            .expect("mounted track");
        let knob = visual
            .debug_bounds("thinking-slider-knob")
            .expect("mounted knob");
        let first_x = dot_x(track, 0, 6);
        assert!(
            (f32::from(knob.center().x) - f32::from(first_x)).abs() <= 1.0,
            "without Off, the leftmost dot is the first effort"
        );
        assert!(
            !window
                .read_with(cx, |slider, _| slider.is_off())
                .unwrap_or(true)
        );
        assert_eq!(choice_of(&window, cx), Some("minimal".to_string()));
    }

    /// R58 R6: `provider_default` renders the ladder with nothing selected —
    /// no knob and no fill — rather than pretending Off is selected.
    #[gpui_kit::test]
    async fn provider_default_renders_with_no_selection(cx: &mut TestAppContext) {
        let window = open_slider_with(
            cx,
            tiers(&["minimal", "low", "medium", "high", "xhigh", "max"]),
            true,
            "provider_default",
            "medium",
        );
        cx.run_until_parked();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        assert_eq!(rendered_dot_count(&mut visual), 7);
        assert!(
            visual.debug_bounds("thinking-slider-knob").is_none(),
            "provider_default selects no position"
        );
        assert!(
            visual.debug_bounds("thinking-slider-fill-flat").is_none(),
            "provider_default fills no track"
        );
        assert!(
            window
                .read_with(cx, |slider, _| slider.is_provider_default())
                .unwrap_or(false)
        );
        assert_eq!(choice_of(&window, cx), None);
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

    // ---- R62: the three-row card ----------------------------------------

    /// Opens the card for a six-tier model at `medium`.
    fn open_three_row_card(cx: &mut TestAppContext) -> WindowHandle<ThinkingSlider> {
        open_slider(
            cx,
            tiers(&["minimal", "low", "medium", "high", "xhigh", "max"]),
            "medium",
            "medium",
        )
    }

    /// R62 A1: the card no longer mounts the bolt icon.
    ///
    /// A bare "the bolt's selector is absent" assertion would be vacuous — the
    /// pre-R62 bolt never carried a `debug_selector`, so that check would pass
    /// on the old layout too. The proof is therefore **geometric**: the title
    /// group is exactly its two known children (`tier name + TITLE_GROUP_GAP +
    /// chevron`) wide. The removed bolt was a 16px icon preceded by the group's
    /// 4px gap, so a re-added bolt would widen the group by 20px and fail the
    /// width identity below.
    ///
    /// The chevron keeps an explicit selector so the identity has two measured
    /// terms rather than one measured and one assumed.
    #[gpui_kit::test]
    async fn r62_a1_the_title_group_has_no_bolt_icon(cx: &mut TestAppContext) {
        let window = open_three_row_card(cx);
        cx.run_until_parked();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        assert!(
            visual.debug_bounds("thinking-slider-card").is_some(),
            "the card itself must still mount, or this test proves nothing"
        );
        let group = visual
            .debug_bounds("thinking-slider-title-group")
            .expect("mounted title group");
        let label = visual
            .debug_bounds("thinking-slider-label")
            .expect("mounted tier name");
        let chevron = visual
            .debug_bounds("thinking-slider-chevron")
            .expect("mounted chevron");
        let children_width =
            f32::from(label.size.width) + TITLE_GROUP_GAP + f32::from(chevron.size.width);
        let group_width = f32::from(group.size.width);
        assert!(
            (group_width - children_width).abs() <= 1.0,
            "R62 A1: the title group must contain only the tier name and the \
             chevron. Group width {group_width}px vs its two children \
             {children_width}px (label {} + gap {TITLE_GROUP_GAP} + chevron \
             {}). A difference of ~20px means a 16px bolt icon and its gap are \
             back in the row.",
            f32::from(label.size.width),
            f32::from(chevron.size.width)
        );
        // R62 R1: `BOLT_SVG` and its helper are gone from the module, so no
        // selector for a bolt can exist.
        assert!(
            visual.debug_bounds("thinking-slider-bolt").is_none(),
            "R62 R1: the bolt icon must not be mounted in the card"
        );
        // The other title-row parts survive the removal.
        assert!(visual.debug_bounds("thinking-slider-title").is_some());
        assert!(
            visual.debug_bounds("thinking-slider-model").is_some(),
            "R62 R2 row 2 renders the model name"
        );
    }

    /// R62 A2: the tier name and the model name occupy two vertically
    /// separated rows.
    ///
    /// This is the structural claim R62 R2 makes — before R62 the two runs
    /// shared one row, so this assertion is exactly the one that could not have
    /// held on the old layout.
    #[gpui_kit::test]
    async fn r62_a2_tier_and_model_names_sit_on_separate_rows(cx: &mut TestAppContext) {
        let window = open_three_row_card(cx);
        cx.run_until_parked();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let tier_name = visual
            .debug_bounds("thinking-slider-label")
            .expect("mounted tier name");
        let model_name = visual
            .debug_bounds("thinking-slider-model")
            .expect("mounted model name");
        assert!(
            tier_name.bottom() <= model_name.top(),
            "R62 A2: the tier name (bottom {:?}) must sit entirely above the \
             model name (top {:?})",
            tier_name.bottom(),
            model_name.top()
        );
        // R62 R2 row order: the tier row is first, the model row second.
        assert!(
            tier_name.top() < model_name.top(),
            "R62 R2: the tier name is row 1 and the model name row 2: \
             tier={tier_name:?} model={model_name:?}"
        );
        // R62 R5: the model row is still inside the card and above the track.
        let card = visual
            .debug_bounds("thinking-slider-card")
            .expect("mounted card");
        let track = visual
            .debug_bounds("thinking-slider-track")
            .expect("mounted track");
        assert!(
            model_name.bottom() <= track.top() && track.bottom() <= card.bottom(),
            "R62 R2: the row order is tier, model, track: card={card:?} \
             model={model_name:?} track={track:?}"
        );
    }

    /// R62 A3: both text rows are horizontally centred inside the card.
    ///
    /// "Centred" is measured against the **card's** centre, not against the
    /// row's own bounds: the rows span the card's content width by design, so
    /// asserting a full-width box is centred would be vacuous. The tier claim is
    /// made on the `tier name + chevron` group, which is the unit R2 centres.
    #[gpui_kit::test]
    async fn r62_a3_both_text_rows_are_centred(cx: &mut TestAppContext) {
        let window = open_three_row_card(cx);
        cx.run_until_parked();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let card = visual
            .debug_bounds("thinking-slider-card")
            .expect("mounted card");
        let card_center = card.center().x;
        // Anti-vacuity guard: a box as wide as the card's content band would be
        // centred by construction, so each measured box must be genuinely
        // narrower than that band for the centring claim to mean anything.
        let content_band = f32::from(card.size.width) - CARD_INSET * 2.0;
        let half_tolerance = 1.0;
        for (label, selector) in [
            ("tier name + chevron group", "thinking-slider-title-group"),
            ("model name", "thinking-slider-model"),
        ] {
            let row = visual
                .debug_bounds(selector)
                .unwrap_or_else(|| panic!("mounted {label}"));
            assert!(
                f32::from(row.size.width) < content_band,
                "R62 A3: the {label} must be shrink-to-fit ({:.1}px) inside the \
                 {content_band}px content band, or its centring is vacuous",
                f32::from(row.size.width)
            );
            let offset = f32::from(row.center().x) - f32::from(card_center);
            assert!(
                offset.abs() <= half_tolerance,
                "R62 A3: the {label} must be centred in the card: its centre \
                 {:?} is {offset}px from the card's centre {card_center:?} \
                 (card={card:?} row={row:?})",
                row.center().x
            );
        }
    }

    /// R62 A4: row 1 still emits `ThinkingSliderTitleActivated` on click.
    ///
    /// R62 R3 requires the two-level drill-down semantics to be unchanged, so
    /// this asserts the same event the pre-R62 title row emitted. The event is
    /// observed on the mounted entity itself, which is the production path.
    #[gpui_kit::test]
    async fn r62_a4_the_title_row_still_emits_title_activated(cx: &mut TestAppContext) {
        let window = open_three_row_card(cx);
        cx.run_until_parked();
        let activated = Arc::new(AtomicUsize::new(0));
        let captured = activated.clone();
        let slider = window.entity(cx).expect("slider entity");
        cx.update(|cx| {
            cx.subscribe(&slider, move |_, _: &ThinkingSliderTitleActivated, _| {
                captured.fetch_add(1, Ordering::SeqCst);
            })
            .detach();
        });
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let title = visual
            .debug_bounds("thinking-slider-title")
            .expect("mounted title row");
        visual.simulate_click(title.center(), Modifiers::default());
        visual.run_until_parked();

        assert_eq!(
            activated.load(Ordering::SeqCst),
            1,
            "R62 A4: clicking row 1 must emit exactly one \
             ThinkingSliderTitleActivated"
        );
    }

    /// R62 R3: the model row does **not** emit the drill-down event.
    ///
    /// This is the other half of the R3 decision (row 2 is inert). Without it,
    /// "row 1 emits" would still hold if both rows emitted.
    #[gpui_kit::test]
    async fn r62_r3_the_model_row_is_inert(cx: &mut TestAppContext) {
        let window = open_three_row_card(cx);
        cx.run_until_parked();
        let activated = Arc::new(AtomicUsize::new(0));
        let captured = activated.clone();
        let slider = window.entity(cx).expect("slider entity");
        cx.update(|cx| {
            cx.subscribe(&slider, move |_, _: &ThinkingSliderTitleActivated, _| {
                captured.fetch_add(1, Ordering::SeqCst);
            })
            .detach();
        });
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let model_name = visual
            .debug_bounds("thinking-slider-model")
            .expect("mounted model name");
        let tier_before = tier_of(&window, cx);
        visual.simulate_click(model_name.center(), Modifiers::default());
        visual.run_until_parked();

        assert_eq!(
            activated.load(Ordering::SeqCst),
            0,
            "R62 R3: the model row must not open the model list"
        );
        assert_eq!(
            tier_of(&window, cx),
            tier_before,
            "R62 R3: the model row must not change the selected tier either"
        );
    }

    /// R62 A5: the card keeps the measured `THINKING_CARD_WIDTH`.
    ///
    /// R6 freezes the width and §4 forbids shrinking the card, so the taller
    /// three-row layout must not have traded width for height.
    #[gpui_kit::test]
    async fn r62_a5_the_card_keeps_its_width(cx: &mut TestAppContext) {
        let window = open_three_row_card(cx);
        cx.run_until_parked();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let card = visual
            .debug_bounds("thinking-slider-card")
            .expect("mounted card");
        assert!(
            (f32::from(card.size.width) - THINKING_CARD_WIDTH).abs() <= 1.0,
            "R62 A5/R6: card width {} != frozen {THINKING_CARD_WIDTH}",
            f32::from(card.size.width)
        );
        assert_eq!(THINKING_CARD_WIDTH, 254.5);
        // The track is unchanged too (R57 froze the bottom row).
        let track = visual
            .debug_bounds("thinking-slider-track")
            .expect("mounted track");
        assert!(
            (f32::from(track.size.width) - THINKING_TRACK_WIDTH).abs() <= 1.0,
            "R57's bottom row is unchanged: track width {} != {THINKING_TRACK_WIDTH}",
            f32::from(track.size.width)
        );
    }

    /// R62 A6: the card's shadow is no longer `shadow_sm`.
    ///
    /// `shadow_sm` is a GPUI framework method that writes a `Vec<BoxShadow>`
    /// straight into the element's style, so the value is asserted on the same
    /// builder the card uses: `card_shadow` must produce `shadow_lg`'s two
    /// layers, and must differ from `shadow_sm`'s. `StyleRefinement`'s fields
    /// are `Option`-wrapped by the derive, which is why the shadow is read
    /// through the style accessor rather than compared as a whole.
    #[test]
    fn r62_a6_the_card_shadow_is_stronger_than_shadow_sm() {
        let large = card_shadow(div()).style().box_shadow.clone();
        let small = div().shadow_sm().style().box_shadow.clone();
        let large = large.expect("card_shadow sets a box shadow");
        let small = small.expect("shadow_sm sets a box shadow");
        assert_ne!(
            large, small,
            "R62 A6/R4: the card's shadow must no longer be shadow_sm"
        );
        // `shadow_lg` is Tailwind's two-layer 10px/4px offset pair; the larger
        // blur is the value that distinguishes it from `shadow_sm`'s 3px/2px.
        assert_eq!(
            large.len(),
            2,
            "shadow_lg is a two-layer shadow, got {large:?}"
        );
        let widest_blur = large
            .iter()
            .map(|shadow| f32::from(shadow.blur_radius))
            .fold(0.0_f32, f32::max);
        assert!(
            widest_blur >= 15.0,
            "R62 R4: the card's shadow must be visibly wider than shadow_sm's \
             3px blur; got a widest blur of {widest_blur}px"
        );
        // The stronger tier is a strict escalation: the largest blur of the
        // chosen shadow exceeds the largest blur `shadow_sm` offers.
        let widest_small = small
            .iter()
            .map(|shadow| f32::from(shadow.blur_radius))
            .fold(0.0_f32, f32::max);
        assert!(
            widest_blur > widest_small,
            "R62 R4: {widest_blur}px blur must exceed shadow_sm's {widest_small}px"
        );
    }

    /// R62 R4: the surface stays **opaque**. The separation comes from the
    /// shadow alone; R4 explicitly forbids a translucent background.
    #[test]
    fn r62_r4_the_card_background_stays_opaque() {
        let colors = vega_theme::Theme::light().colors;
        assert_eq!(
            colors.bg_elevated.a, 1.0,
            "R62 R4: the card's surface token must stay fully opaque"
        );
        assert_eq!(colors.bg_elevated, measured_rgba(0xFFFFFFFF));
        // Dark mode too, so the rule is not appearance-dependent.
        assert_eq!(vega_theme::Theme::dark().colors.bg_elevated.a, 1.0);
    }

    /// R62 M1: the three-row card's content height, reported as an assertion so
    /// a future change to any row's height is visible rather than silent.
    ///
    /// The height is measured from the card's top edge to the track's bottom
    /// edge plus the bottom padding, rather than read from `card.size.height`.
    /// In this harness the card is the window's **root** view, so a fixed-width
    /// flex column stretches to the window's height; the content-driven height
    /// is the number M1 is about, and it is what the production mount (where
    /// the card sits in a shrink-to-fit absolute layer) renders.
    ///
    /// **Measured on the rendered frame: 106px** (the production mount in
    /// `model_picker_levels.rs` reads 108px; see the note below). The sum is
    /// `CARD_PADDING_TOP + 2 text rows + track + 2 × CARD_ROW_GAP +
    /// CARD_PADDING_BOTTOM` = 12 + 21 + 21 + 24 + 16 + 14 = 108. The text rows
    /// render at 21px rather than the 20px [`CARD_TEXT_ROW_HEIGHT`] floor
    /// because the 13px `SIDEBAR` line box is 21px; the floor is a minimum, not
    /// a fixed height, which is exactly what keeps the text from being clipped.
    ///
    /// The pre-R62 card was 12 + 24 + 8 + 24 + 14 = 82px, so the card grows
    /// **26px** taller and covers that much more of the 37px utility bar
    /// (R62 R5, M1 — measured as 55px into the bar on the production mount).
    #[gpui_kit::test]
    async fn r62_m1_the_card_height_is_the_three_row_sum(cx: &mut TestAppContext) {
        let window = open_three_row_card(cx);
        cx.run_until_parked();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let card = visual
            .debug_bounds("thinking-slider-card")
            .expect("mounted card");
        let title = visual
            .debug_bounds("thinking-slider-title")
            .expect("mounted title row");
        let model_name = visual
            .debug_bounds("thinking-slider-model")
            .expect("mounted model row");
        let track = visual
            .debug_bounds("thinking-slider-track")
            .expect("mounted track");

        // The paddings and gaps, measured on the rendered frame.
        assert!(
            (f32::from(title.top() - card.top()) - CARD_PADDING_TOP).abs() <= 1.0,
            "R62 M1: the top padding is CARD_PADDING_TOP ({CARD_PADDING_TOP}px), \
             got {}",
            f32::from(title.top() - card.top())
        );
        let gap_above_model = f32::from(model_name.top() - title.bottom());
        let gap_above_track = f32::from(track.top() - model_name.bottom());
        assert!(
            (gap_above_model - CARD_ROW_GAP).abs() <= 1.0
                && (gap_above_track - CARD_ROW_GAP).abs() <= 1.0,
            "R62 R2: the row gap is CARD_ROW_GAP ({CARD_ROW_GAP}px) in both \
             gaps, got {gap_above_model}px and {gap_above_track}px"
        );

        // The content-driven height: card top to track bottom, plus the bottom
        // padding the track's bounds cannot include.
        let content_height = f32::from(track.bottom() - card.top()) + CARD_PADDING_BOTTOM;
        // The two text rows are read from the frame, not assumed to be the
        // floor: `min_h` means the rendered height is the larger of the floor
        // and the line box, and M1 is about what actually renders.
        let text_rows = f32::from(title.size.height) + f32::from(model_name.size.height);
        let expected = CARD_PADDING_TOP
            + text_rows
            + THINKING_TRACK_HEIGHT
            + CARD_ROW_GAP * 2.0
            + CARD_PADDING_BOTTOM;
        assert!(
            (content_height - expected).abs() <= 1.0,
            "R62 M1: the card's content height is {content_height}px; its three \
             rows ({text_rows}px of text) plus padding sum to {expected}px"
        );
        // The floor holds: neither text row collapsed below it.
        for (label, row) in [("title", title), ("model", model_name)] {
            assert!(
                f32::from(row.size.height) >= CARD_TEXT_ROW_HEIGHT - 1.0,
                "R62 R2: the {label} row ({:?}) must be at least \
                 CARD_TEXT_ROW_HEIGHT ({CARD_TEXT_ROW_HEIGHT}px)",
                row.size.height
            );
        }

        // The pre-R62 two-row card, for the R5 growth claim: one title row that
        // shared its line with the model name, then the track.
        let two_row = CARD_PADDING_TOP
            + THINKING_TRACK_HEIGHT
            + CARD_ROW_GAP
            + THINKING_TRACK_HEIGHT
            + CARD_PADDING_BOTTOM;
        assert_eq!(two_row, 82.0, "the pre-R62 card measured 82px");
        assert_eq!(
            content_height - two_row,
            26.0,
            "R62 R5/M1: the card grows 26px taller (two text rows plus the \
             second gap, minus the row height the old title row shared), so it \
             covers 26px more of the utility bar — accepted by R62 R5"
        );
    }

    /// R62 R6: a long model name ellipsises inside the card instead of spilling
    /// past its edge.
    ///
    /// The card's width is frozen, so an unbounded model name would either
    /// stretch the card or paint outside it. `min_w_0` + `truncate` is what
    /// keeps it inside; this asserts the rendered text run stays within the
    /// card's content band.
    #[gpui_kit::test]
    async fn r62_r6_a_long_model_name_stays_inside_the_card(cx: &mut TestAppContext) {
        let long_name = "gpt-6-astra-ultra-extended-preview-2026-09-14-build-0001";
        let long_name_owned = long_name.to_string();
        let window = cx.update(|cx| {
            cx.set_global(vega_theme::Theme::light());
            cx.open_window(WindowOptions::default(), move |_, cx| {
                cx.new(|cx| {
                    ThinkingSlider::new(
                        long_name_owned,
                        tiers(&["minimal", "low", "medium", "high", "xhigh", "max"]),
                        false,
                        "medium",
                        "medium",
                        cx,
                    )
                })
            })
            .expect("long-name slider window")
        });
        cx.run_until_parked();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let card = visual
            .debug_bounds("thinking-slider-card")
            .expect("mounted card");
        let model_name = visual
            .debug_bounds("thinking-slider-model")
            .expect("mounted model name");
        assert!(
            (f32::from(card.size.width) - THINKING_CARD_WIDTH).abs() <= 1.0,
            "R62 R6: a long model name must not widen the card: {} != \
             {THINKING_CARD_WIDTH}",
            f32::from(card.size.width)
        );
        assert!(
            model_name.left() >= card.left() && model_name.right() <= card.right(),
            "R62 R6: the model name ({model_name:?}) must stay inside the card \
             ({card:?}) and ellipsise rather than spill"
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

    /// R58 R3: clicking the leftmost dot with Off shown emits `"disabled"` —
    /// the choice name that maps to `ReasoningChoice::Disabled` — and clicking
    /// the next dot emits the first effort. The Off position must never
    /// surface as an effort.
    #[gpui_kit::test]
    async fn clicking_the_off_dot_selects_disabled(cx: &mut TestAppContext) {
        let window = open_slider_with(
            cx,
            tiers(&["minimal", "low", "medium", "high", "xhigh", "max"]),
            true,
            "medium",
            "medium",
        );
        cx.run_until_parked();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let track = visual
            .debug_bounds("thinking-slider-track")
            .expect("mounted track");
        let count = 7;

        // `medium` is position 3 with Off at 0.
        assert_eq!(choice_of(&window, cx), Some("medium".to_string()));
        visual.simulate_click(
            gpui_kit::point(dot_x(track, 0, count), track.center().y),
            Modifiers::default(),
        );
        visual.run_until_parked();
        assert_eq!(
            choice_of(&window, cx),
            Some(OFF_CHOICE_NAME.to_string()),
            "the leftmost dot is the Off position"
        );
        assert!(
            window
                .read_with(cx, |slider, _| slider.is_off())
                .unwrap_or(false),
            "Off is not a tier"
        );
        assert_eq!(tier_of(&window, cx), None);

        // The first effort sits one dot to the right of Off.
        visual.simulate_click(
            gpui_kit::point(dot_x(track, 1, count), track.center().y),
            Modifiers::default(),
        );
        visual.run_until_parked();
        assert_eq!(choice_of(&window, cx), Some("minimal".to_string()));
        assert!(
            !window
                .read_with(cx, |slider, _| slider.is_off())
                .unwrap_or(true)
        );
    }

    /// R58 R3/A4: without Off, clicking dot `i` still selects `efforts[i]` —
    /// the R57 behaviour is unchanged.
    #[gpui_kit::test]
    async fn clicking_a_dot_without_off_still_selects_the_effort(cx: &mut TestAppContext) {
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
        visual.simulate_click(
            gpui_kit::point(dot_x(track, 0, 6), track.center().y),
            Modifiers::default(),
        );
        visual.run_until_parked();
        assert_eq!(tier_of(&window, cx), Some("minimal".to_string()));
        assert_eq!(choice_of(&window, cx), Some("minimal".to_string()));
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

    /// R58 R1/R3: a drag onto the Off dot selects `"disabled"`, the same as a
    /// click. The position is part of the same gesture as the tiers.
    #[gpui_kit::test]
    async fn dragging_onto_off_selects_disabled(cx: &mut TestAppContext) {
        let window = open_slider_with(
            cx,
            tiers(&["minimal", "low", "medium", "high", "xhigh", "max"]),
            true,
            "minimal",
            "medium",
        );
        cx.run_until_parked();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let track = visual
            .debug_bounds("thinking-slider-track")
            .expect("mounted track");
        let count = 7;

        // Position 1 is the first effort (`minimal`).
        visual.simulate_mouse_down(
            gpui_kit::point(dot_x(track, 1, count), track.center().y),
            MouseButton::Left,
            Modifiers::default(),
        );
        visual.run_until_parked();
        assert_eq!(choice_of(&window, cx), Some("minimal".to_string()));
        // GPUI consumes the first move past its 2 px threshold to create the
        // drag, so that move carries no `drag_move`; the next one does.
        visual.simulate_mouse_move(
            gpui_kit::point(dot_x(track, 2, count), track.center().y),
            Some(MouseButton::Left),
            Modifiers::default(),
        );
        visual.run_until_parked();
        visual.simulate_mouse_move(
            gpui_kit::point(dot_x(track, 0, count), track.center().y),
            Some(MouseButton::Left),
            Modifiers::default(),
        );
        visual.run_until_parked();
        assert_eq!(
            choice_of(&window, cx),
            Some(OFF_CHOICE_NAME.to_string()),
            "dragging onto the leftmost dot selects Off"
        );
        assert!(
            window
                .read_with(cx, |slider, _| slider.is_off())
                .unwrap_or(false)
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
