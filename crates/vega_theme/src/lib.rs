//! Light and dark themes.
//!
//! Color tokens defined in [vega-ui-spec.md §2](../../docs/vega-ui-spec.md).
//! All hex color literals in the workspace are confined to this crate;
//! components must reference these tokens instead of hardcoding colors.

use gpui::{App, FontWeight, Global, Rgba, WindowAppearance};

/// Converts an RGBA hex literal (`0xRRGGBBAA`) to [`Rgba`].
///
/// Mirrors `gpui::rgba`, which is not `const` and therefore cannot be used
/// in the token constants below.
const fn rgba(hex: u32) -> Rgba {
    let [r, g, b, a] = hex.to_be_bytes();
    Rgba {
        r: r as f32 / 255.0,
        g: g as f32 / 255.0,
        b: b as f32 / 255.0,
        a: a as f32 / 255.0,
    }
}

/// The full set of UI color tokens (single appearance, light or dark).
///
/// Field values must come from the token table in the UI spec; the two
/// shipped palettes are [`LIGHT`] and [`DARK`].
#[derive(Debug, Clone, Copy)]
pub struct ThemeColors {
    /// Main content area background.
    pub bg_base: Rgba,
    /// Sidebar background.
    pub bg_sidebar: Rgba,
    /// Cards and composer background.
    pub bg_elevated: Rgba,
    /// Hover state background.
    pub bg_hover: Rgba,
    /// Selected state background (current thread).
    pub bg_active: Rgba,
    /// 1px separators and card borders.
    pub border_subtle: Rgba,
    /// Primary text.
    pub text_primary: Rgba,
    /// Secondary information and timestamps.
    pub text_secondary: Rgba,
    /// Placeholders.
    pub text_tertiary: Rgba,
    /// Primary button (black on white / inverted).
    pub accent: Rgba,
    /// Tool success state and diff additions.
    pub success: Rgba,
    /// Error state, diff deletions, dangerous actions.
    pub danger: Rgba,
    /// Permission confirmation and budget warnings.
    pub warning: Rgba,
    /// Code block background.
    pub code_bg: Rgba,
}

/// Light palette (UI spec §2, "Light" column).
pub const LIGHT: ThemeColors = ThemeColors {
    bg_base: rgba(0xFFFFFFFF),
    bg_sidebar: rgba(0xF7F7F5FF),
    bg_elevated: rgba(0xFFFFFFFF),
    bg_hover: rgba(0xEFEFEDFF),
    bg_active: rgba(0xE9E9E7FF),
    border_subtle: rgba(0xE5E5E3FF),
    text_primary: rgba(0x1A1A1AFF),
    text_secondary: rgba(0x6B6B6BFF),
    text_tertiary: rgba(0x9E9E9EFF),
    accent: rgba(0x1A1A1AFF),
    success: rgba(0x1A7F37FF),
    danger: rgba(0xCF222EFF),
    warning: rgba(0x9A6700FF),
    code_bg: rgba(0xF6F8FAFF),
};

/// Dark palette (UI spec §2, "Dark" column).
pub const DARK: ThemeColors = ThemeColors {
    bg_base: rgba(0x1E1E1EFF),
    bg_sidebar: rgba(0x252525FF),
    bg_elevated: rgba(0x2D2D2DFF),
    bg_hover: rgba(0x383838FF),
    bg_active: rgba(0x404040FF),
    border_subtle: rgba(0x3A3A3AFF),
    text_primary: rgba(0xECECECFF),
    text_secondary: rgba(0x9C9C9CFF),
    text_tertiary: rgba(0x6B6B6BFF),
    accent: rgba(0xECECECFF),
    success: rgba(0x3FB950FF),
    danger: rgba(0xF85149FF),
    warning: rgba(0xD29922FF),
    code_bg: rgba(0x282C34FF),
};

/// Which palette the theme currently applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Appearance {
    /// Light palette ([`LIGHT`]).
    Light,
    /// Dark palette ([`DARK`]).
    Dark,
}

impl Appearance {
    /// Returns the opposite appearance.
    pub fn toggle(self) -> Self {
        match self {
            Appearance::Light => Appearance::Dark,
            Appearance::Dark => Appearance::Light,
        }
    }
}

/// The active UI theme: an [`Appearance`] plus the matching [`ThemeColors`].
///
/// Registered as a GPUI global at startup ([`Theme::system`]); components read
/// it through [`theme`].
#[derive(Debug, Clone, Copy)]
pub struct Theme {
    /// Color tokens for the active appearance.
    pub colors: ThemeColors,
    /// Which palette is currently active.
    pub appearance: Appearance,
}

impl Global for Theme {}

impl Theme {
    /// Theme with the light palette (UI spec §2, "Light" column).
    pub fn light() -> Self {
        Theme {
            colors: LIGHT,
            appearance: Appearance::Light,
        }
    }

    /// Theme with the dark palette (UI spec §2, "Dark" column).
    pub fn dark() -> Self {
        Theme {
            colors: DARK,
            appearance: Appearance::Dark,
        }
    }

    /// Theme matching the OS appearance at call time.
    ///
    /// Reads the real macOS appearance via `App::window_appearance` (gpui
    /// exposes it on this rev); `VibrantLight`/`VibrantDark` map onto
    /// light/dark respectively.
    pub fn system(cx: &App) -> Self {
        match cx.window_appearance() {
            WindowAppearance::Dark | WindowAppearance::VibrantDark => Self::dark(),
            WindowAppearance::Light | WindowAppearance::VibrantLight => Self::light(),
        }
    }

    /// Flips between light and dark in place, swapping the palette to match.
    pub fn toggle(&mut self) {
        self.appearance = self.appearance.toggle();
        self.colors = match self.appearance {
            Appearance::Light => LIGHT,
            Appearance::Dark => DARK,
        };
    }
}

/// Convenience accessor for the theme global ("components take the theme via
/// `cx.theme()`" in the task card); requires [`Theme`] to have been registered
/// with `App::set_global` at startup.
pub fn theme(cx: &App) -> &Theme {
    cx.global::<Theme>()
}

/// Typography constants ([vega-ui-spec.md §3](../../docs/vega-ui-spec.md)).
///
/// Font sizes are logical pixels, meant to be fed to `gpui::px`. Line-height
/// values marked as ratios are unitless multipliers; `SIDEBAR_LINE_HEIGHT` is
/// an absolute pixel row height, matching the spec verbatim.
pub struct Typography;

impl Typography {
    /// Body text: 13px (§3 "正文字体 …13px/1.55 行高").
    pub const BODY: f32 = 13.0;
    /// Body line height: 1.55× font size (ratio, §3).
    pub const BODY_LINE_HEIGHT: f32 = 1.55;
    /// Conversation message body: 14px (§3 "会话消息正文 14px/1.6").
    pub const MESSAGE: f32 = 14.0;
    /// Message line height: 1.6× font size (ratio, §3).
    pub const MESSAGE_LINE_HEIGHT: f32 = 1.6;
    /// Code font size: 12.5px, monospace (§3 "代码字体 SF Mono / JetBrains Mono，12.5px").
    pub const CODE: f32 = 12.5;
    /// Sidebar entry font size: 13px (§3 "侧边栏条目 13px，行高 32px").
    pub const SIDEBAR: f32 = 13.0;
    /// Sidebar entry row height: 32px absolute (§3).
    pub const SIDEBAR_LINE_HEIGHT: f32 = 32.0;
    /// Page heading size: 16px (§3 "页面 16px 600").
    pub const HEADING_PAGE: f32 = 16.0;
    /// Page heading weight: 600 (§3).
    pub const HEADING_PAGE_WEIGHT: FontWeight = FontWeight::SEMIBOLD;
    /// Block heading size: 14px (§3 "区块 14px 600").
    pub const HEADING_BLOCK: f32 = 14.0;
    /// Block heading weight: 600 (§3).
    pub const HEADING_BLOCK_WEIGHT: FontWeight = FontWeight::SEMIBOLD;
    /// Card heading size: 13px (§3 "卡片 13px 500").
    pub const HEADING_CARD: f32 = 13.0;
    /// Card heading weight: 500 (§3).
    pub const HEADING_CARD_WEIGHT: FontWeight = FontWeight::MEDIUM;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_tokens_match_ui_spec_table() {
        // Spot-check a few Dark column entries from the UI spec §2 table.
        assert_eq!(u32::from(DARK.bg_base), 0x1E1E1EFF);
        assert_eq!(u32::from(DARK.text_primary), 0xECECECFF);
        assert_eq!(u32::from(DARK.code_bg), 0x282C34FF);
    }

    #[test]
    fn light_tokens_match_ui_spec_table() {
        // Spot-check a few Light column entries from the UI spec §2 table.
        assert_eq!(u32::from(LIGHT.bg_base), 0xFFFFFFFF);
        assert_eq!(u32::from(LIGHT.success), 0x1A7F37FF);
        assert_eq!(u32::from(LIGHT.danger), 0xCF222EFF);
    }

    #[test]
    fn light_and_dark_palettes_differ_on_key_tokens() {
        assert_ne!(u32::from(LIGHT.bg_base), u32::from(DARK.bg_base));
        assert_ne!(u32::from(LIGHT.text_primary), u32::from(DARK.text_primary));
    }

    #[test]
    fn appearance_toggle_round_trips() {
        assert_eq!(Appearance::Light.toggle(), Appearance::Dark);
        assert_eq!(Appearance::Dark.toggle(), Appearance::Light);
        assert_eq!(Appearance::Light.toggle().toggle(), Appearance::Light);
    }

    #[test]
    fn theme_toggle_swaps_palette_in_place() {
        let mut theme = Theme::light();
        theme.toggle();
        assert_eq!(theme.appearance, Appearance::Dark);
        assert_eq!(u32::from(theme.colors.bg_base), u32::from(DARK.bg_base));
        assert_eq!(
            u32::from(theme.colors.text_primary),
            u32::from(DARK.text_primary)
        );

        theme.toggle();
        assert_eq!(theme.appearance, Appearance::Light);
        assert_eq!(u32::from(theme.colors.bg_base), u32::from(LIGHT.bg_base));
        assert_eq!(
            u32::from(theme.colors.text_primary),
            u32::from(LIGHT.text_primary)
        );
    }

    #[test]
    fn theme_constructors_match_palettes() {
        let light = Theme::light();
        assert_eq!(light.appearance, Appearance::Light);
        assert_eq!(u32::from(light.colors.bg_base), u32::from(LIGHT.bg_base));

        let dark = Theme::dark();
        assert_eq!(dark.appearance, Appearance::Dark);
        assert_eq!(u32::from(dark.colors.bg_base), u32::from(DARK.bg_base));
    }
}
