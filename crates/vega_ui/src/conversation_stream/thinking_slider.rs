use gpui_kit::prelude::*;
use gpui_kit::{EventEmitter, Render, Rgba, Window};

pub const THINKING_CARD_WIDTH: f32 = 254.5;
pub const THINKING_TRACK_WIDTH: f32 = 203.0;
pub const THINKING_TRACK_HEIGHT: f32 = 24.0;
pub const THINKING_KNOB_DIAMETER: f32 = 24.0;

pub const THINKING_DOT_DIAMETER: f32 = 4.0;

const fn measured_rgba(hex: u32) -> Rgba {
    let [r, g, b, a] = hex.to_be_bytes();
    Rgba {
        r: r as f32 / 255.0,
        g: g as f32 / 255.0,
        b: b as f32 / 255.0,
        a: a as f32 / 255.0,
    }
}

const TIER_LABEL_STRONG: Rgba = measured_rgba(0x924FF7FF);
const TIER_LABEL_BASE: Rgba = measured_rgba(0x3983F7FF);

const TRACK_FILL_FLAT: Rgba = measured_rgba(0x3983F7FF);

const TRACK_GRADIENT: [(f32, Rgba); 3] = [
    (0.0, measured_rgba(0x3346CDFF)),
    (0.64, measured_rgba(0xAE76FFFF)),
    (1.0, measured_rgba(0x7E5AF0FF)),
];

const TRACK_DOT_FILLED: Rgba = measured_rgba(0xFFFFFFFF);

const TRACK_UNFILLED: Rgba = measured_rgba(0xE9E8E8FF);

pub const OFF_CHOICE_NAME: &str = "disabled";

pub const OFF_LABEL: &str = "ui.off";

pub const PROVIDER_DEFAULT_LABEL: &str = "ui.default";

pub fn tier_display_label(effort: &str) -> String {
    if effort == "enabled" {
        return vega_i18n::t("ui.enable_thinking").into();
    }
    match effort {
        "none" => vega_i18n::t("ui.none").to_string(),
        "minimal" => vega_i18n::t("ui.minimal").to_string(),
        "low" => vega_i18n::t("ui.low").to_string(),
        "medium" => vega_i18n::t("ui.medium").to_string(),
        "high" => vega_i18n::t("ui.high").to_string(),
        "xhigh" => vega_i18n::t("ui.extra_high").to_string(),
        "max" => vega_i18n::t("ui.max").to_string(),
        "ultra" => vega_i18n::t("ui.ultra").to_string(),
        "persistent" => vega_i18n::t("ui.persistent").to_string(),
        other => {
            let mut characters = other.chars();
            match characters.next() {
                Some(first) => first.to_uppercase().collect::<String>() + characters.as_str(),
                None => String::new(),
            }
        }
    }
}

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
    Off,
    Tier(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThinkingSliderModel {
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

    fn off_offset(&self) -> usize {
        usize::from(self.supports_off)
    }

    fn rebind(&mut self, supports_off: bool, current: &str, default: &str) {
        self.supports_off = supports_off;
        self.default =
            resolve_tier(&self.tiers, default, default).map(std::string::ToString::to_string);

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
        self.current =
            find_supported(&self.tiers, current).map(|tier| Selection::Tier(tier.to_string()));
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

    pub fn tier(&self) -> Option<&str> {
        match self.current.as_ref()? {
            Selection::Tier(tier) => Some(tier.as_str()),
            Selection::Off => None,
        }
    }

    pub fn choice_name(&self) -> Option<&str> {
        match self.current.as_ref()? {
            Selection::Tier(tier) => Some(tier.as_str()),
            Selection::Off => Some(OFF_CHOICE_NAME),
        }
    }

    pub fn is_off(&self) -> bool {
        matches!(self.current, Some(Selection::Off))
    }

    pub fn is_provider_default(&self) -> bool {
        self.provider_default
    }

    pub fn label(&self) -> String {
        match self.current.as_ref() {
            Some(Selection::Tier(tier)) => tier_display_label(tier),
            Some(Selection::Off) => vega_i18n::t(OFF_LABEL).to_string(),
            None if self.provider_default => vega_i18n::t(PROVIDER_DEFAULT_LABEL).to_string(),
            None => String::new(),
        }
    }

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

pub struct ThinkingSlider {
    model: ThinkingSliderModel,
    model_name: String,
}

pub(super) mod native;

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
        }
    }

    pub fn tiers(&self) -> &[String] {
        self.model.tiers()
    }

    pub fn tier(&self) -> Option<&str> {
        self.model.tier()
    }

    pub fn choice_name(&self) -> Option<&str> {
        self.model.choice_name()
    }

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

    pub(super) fn native_snapshot(&self) -> native::Snapshot {
        native::Snapshot {
            model: self.model.clone(),
            model_name: self.model_name.clone(),
        }
    }

    pub(super) fn select_provider_default(&mut self, cx: &mut Context<Self>) {
        if self.model.is_provider_default() {
            return;
        }
        self.model.current = None;
        self.model.provider_default = true;
        cx.emit(ThinkingTierSelected {
            model: self.model_name.clone(),
            choice: "provider_default".to_owned(),
        });
        cx.notify();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackFillStyle {
    Flat,
    Gradient,
}

pub fn track_fill_style(selected: usize, strongest: Option<usize>) -> TrackFillStyle {
    if strongest == Some(selected) {
        TrackFillStyle::Gradient
    } else {
        TrackFillStyle::Flat
    }
}

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

pub(super) fn native_label_color(model: &ThinkingSliderModel) -> Rgba {
    if model.selected_position().is_some()
        && model.selected_position() == model.strongest_position()
    {
        TIER_LABEL_STRONG
    } else {
        TIER_LABEL_BASE
    }
}

impl Render for ThinkingSlider {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui_kit::Empty
    }
}
