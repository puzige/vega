//! Light and dark themes.
//!
//! Color tokens defined in [vega-ui-spec.md §2](../../docs/vega-ui-spec.md).
//! All hex color literals in the workspace are confined to this crate;
//! components must reference these tokens instead of hardcoding colors.

use gpui_kit::{App, FontWeight, Global, Rgba, WindowAppearance};

/// Converts an RGBA hex literal (`0xRRGGBBAA`) to [`Rgba`].
///
/// Mirrors `gpui_kit::rgba`, which is not `const` and therefore cannot be used
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
    /// Primary action and selected-state brand color.
    pub accent: Rgba,
    /// R18 primary brand color: sapphire in light mode, ice blue in dark.
    pub brand_primary: Rgba,
    /// R18 stronger brand color for high-contrast icon or hover emphasis.
    pub brand_primary_strong: Rgba,
    /// Low-contrast brand wash used for active/selected surfaces.
    pub brand_soft: Rgba,
    /// Foreground color for content rendered on the primary brand surface.
    pub brand_on_accent: Rgba,
    /// Tool success state and diff additions.
    pub success: Rgba,
    /// Error state, diff deletions, dangerous actions.
    pub danger: Rgba,
    /// Permission confirmation and budget warnings.
    pub warning: Rgba,
    /// Code block background.
    pub code_bg: Rgba,
    /// Sidebar custom group colors: gray, red, orange, yellow, green, blue, purple.
    pub sidebar_group_colors: [Rgba; 7],
}

/// Light palette (UI spec §2, "Light" column).
pub const LIGHT: ThemeColors = ThemeColors {
    bg_base: rgba(0xFFFFFFFF),
    bg_sidebar: rgba(0xF3F3F3FF),
    bg_elevated: rgba(0xFFFFFFFF),
    bg_hover: rgba(0xECECECFF),
    bg_active: rgba(0xEAF2FCFF),
    border_subtle: rgba(0xE8E8E8FF),
    text_primary: rgba(0x202020FF),
    text_secondary: rgba(0x676767FF),
    text_tertiary: rgba(0x8A8A8AFF),
    accent: rgba(0x3478D8FF),
    brand_primary: rgba(0x3478D8FF),
    brand_primary_strong: rgba(0x245AAFFF),
    brand_soft: rgba(0xEAF2FCFF),
    brand_on_accent: rgba(0xFFFFFFFF),
    success: rgba(0x1A7F37FF),
    danger: rgba(0xCF222EFF),
    warning: rgba(0x9A6700FF),
    code_bg: rgba(0xF6F6F6FF),
    sidebar_group_colors: [
        rgba(0x8A8A8AFF),
        rgba(0xCF222EFF),
        rgba(0xBC4C00FF),
        rgba(0x9A6700FF),
        rgba(0x1A7F37FF),
        rgba(0x0969DAFF),
        rgba(0x8250DFFF),
    ],
};

/// Dark palette (UI spec §2, "Dark" column).
pub const DARK: ThemeColors = ThemeColors {
    bg_base: rgba(0x202020FF),
    bg_sidebar: rgba(0x191919FF),
    bg_elevated: rgba(0x2A2A2AFF),
    bg_hover: rgba(0x323232FF),
    bg_active: rgba(0x203247FF),
    border_subtle: rgba(0x383838FF),
    text_primary: rgba(0xEDEDEDFF),
    text_secondary: rgba(0xABABABFF),
    text_tertiary: rgba(0x828282FF),
    accent: rgba(0x8FC7FFFF),
    brand_primary: rgba(0x8FC7FFFF),
    brand_primary_strong: rgba(0x609DE1FF),
    brand_soft: rgba(0x203247FF),
    brand_on_accent: rgba(0x13233AFF),
    success: rgba(0x3FB950FF),
    danger: rgba(0xF85149FF),
    warning: rgba(0xD29922FF),
    code_bg: rgba(0x262626FF),
    sidebar_group_colors: [
        rgba(0xABABABFF),
        rgba(0xF85149FF),
        rgba(0xF0883EFF),
        rgba(0xD29922FF),
        rgba(0x3FB950FF),
        rgba(0x58A6FFFF),
        rgba(0xBC8CFFFF),
    ],
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
    /// Whether native appearance changes should update this palette.
    pub follow_system: bool,
}

impl Global for Theme {}

impl Theme {
    /// Theme with the light palette (UI spec §2, "Light" column).
    pub fn light() -> Self {
        Theme {
            colors: LIGHT,
            appearance: Appearance::Light,
            follow_system: false,
        }
    }

    /// Theme with the dark palette (UI spec §2, "Dark" column).
    pub fn dark() -> Self {
        Theme {
            colors: DARK,
            appearance: Appearance::Dark,
            follow_system: false,
        }
    }

    /// Theme matching the OS appearance at call time.
    ///
    /// Reads the real macOS appearance via `App::window_appearance` (gpui
    /// exposes it on this rev); `VibrantLight`/`VibrantDark` map onto
    /// light/dark respectively.
    pub fn system(cx: &App) -> Self {
        let mut theme = match cx.window_appearance() {
            WindowAppearance::Dark | WindowAppearance::VibrantDark => Self::dark(),
            WindowAppearance::Light | WindowAppearance::VibrantLight => Self::light(),
        };
        theme.follow_system = true;
        theme
    }

    /// Flips between light and dark in place, swapping the palette to match.
    pub fn toggle(&mut self) {
        self.follow_system = false;
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
/// Font sizes are logical pixels, meant to be fed to `gpui_kit::px`. Line-height
/// values marked as ratios are unitless multipliers; `SIDEBAR_LINE_HEIGHT` is
/// an absolute pixel row height, matching the spec verbatim.
pub struct Typography;

impl Typography {
    /// Settings page title from R8 reference.
    pub const SETTINGS_TITLE: f32 = 24.0;
    /// Body text: 13px (§3 "正文字体 …13px/1.55 行高").
    pub const BODY: f32 = 13.0;
    /// Body line height: 1.55× font size (ratio, §3).
    pub const BODY_LINE_HEIGHT: f32 = 1.55;
    /// Conversation message body: 15px (R4 visual revision §2).
    pub const MESSAGE: f32 = 15.0;
    /// Message line height: 1.65× font size (ratio, R4 visual revision §2).
    pub const MESSAGE_LINE_HEIGHT: f32 = 1.65;
    /// Code font size: 12.5px, monospace (§3 "代码字体 SF Mono / JetBrains Mono，12.5px").
    pub const CODE: f32 = 12.5;
    /// Sidebar entry font size: 13px (§3 "侧边栏条目 13px，行高 34px").
    pub const SIDEBAR: f32 = 13.0;
    /// Sidebar entry row height: 32px absolute (R19 phase-1 shell freeze).
    pub const SIDEBAR_LINE_HEIGHT: f32 = 32.0;
    /// Compact metadata and status labels: 12px (R4 visual revision §2).
    pub const METADATA: f32 = 12.0;
    /// Empty-state title: 28px semibold (R8 visual parity).
    pub const EMPTY_STATE_TITLE: f32 = 28.0;
    /// Empty-state title weight: semibold (R8 visual parity).
    pub const EMPTY_STATE_TITLE_WEIGHT: FontWeight = FontWeight::SEMIBOLD;
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

/// Shared layout tokens for the native client.
///
/// Keeping radii and content geometry here makes the visual contract explicit
/// at call sites and prevents a component from silently falling back to a
/// named GPUI radius whose value can change between revisions.
pub struct Layout;

impl Layout {
    /// Leading space reserved for native macOS titlebar controls.
    pub const TITLEBAR_LEADING_INSET: f32 = 96.0;
    /// Maximum readable width for conversation, settings, and diff content.
    pub const CONTENT_MAX_WIDTH: f32 = 820.0;
    /// Minimum horizontal page padding around a readable content column.
    pub const CONTENT_PADDING: f32 = 16.0;
    /// Radius for ordinary panels and cards.
    pub const PANEL_RADIUS: f32 = 12.0;
    /// Radius for the primary composer surface.
    pub const COMPOSER_RADIUS: f32 = 20.0;
    /// Minimum height of the primary composer shell.
    pub const COMPOSER_MIN_HEIGHT: f32 = 100.0;
    /// Height reserved by every non-Settings main route.
    pub const MAIN_HEADER_HEIGHT: f32 = 46.0;
    /// Gap between the main content panel and the native window edges/sidebar.
    pub const MAIN_CONTENT_GAP: f32 = 4.0;
    /// Provider selector column inside Settings.
    pub const PROVIDER_LIST_WIDTH: f32 = 180.0;
    /// Sidebar width from the R19 phase-1 shell freeze.
    pub const SIDEBAR_WIDTH: f32 = 260.0;
    /// Sidebar horizontal and vertical padding.
    pub const SIDEBAR_PADDING: f32 = 12.0;
    /// Composer width cap; the thread column remains wider for readable output.
    pub const COMPOSER_MAX_WIDTH: f32 = 736.0;
    /// Fixed wide-screen Environment rail width.
    pub const ENVIRONMENT_RAIL_WIDTH: f32 = 292.0;
    /// Top/right inset for the floating Environment card.
    pub const ENVIRONMENT_CARD_INSET: f32 = 16.0;
    /// Environment card radius.
    pub const ENVIRONMENT_CARD_RADIUS: f32 = 18.0;
    /// Width at which the persistent Environment rail becomes available.
    pub const ENVIRONMENT_BREAKPOINT: f32 = 1180.0;
    /// Default bottom workspace height.
    pub const BOTTOM_WORKSPACE_HEIGHT: f32 = 272.0;
    /// Workspace tab/header height.
    pub const WORKSPACE_HEADER_HEIGHT: f32 = 40.0;
    /// Reserved trailing width for session timestamps and the compact action
    /// menu trigger. Low-frequency actions live in the popover so long
    /// session titles keep the main width of the rail.
    pub const SIDEBAR_ACTIONS_WIDTH: f32 = 72.0;
    /// Compact width for the sidebar task action popup.
    pub const TASK_MENU_WIDTH: f32 = 240.0;
}

/// Standard xterm ANSI palette, used only for colors explicitly emitted by a terminal.
pub fn terminal_indexed_color(index: u8) -> Rgba {
    const BASE: [u32; 16] = [
        0x000000, 0xcd0000, 0x00cd00, 0xcdcd00, 0x0000ee, 0xcd00cd, 0x00cdcd, 0xe5e5e5, 0x7f7f7f,
        0xff0000, 0x00ff00, 0xffff00, 0x5c5cff, 0xff00ff, 0x00ffff, 0xffffff,
    ];
    let value = match index {
        0..=15 => BASE[index as usize],
        16..=231 => {
            let index = index - 16;
            let level = |value: u8| {
                if value == 0 {
                    0
                } else {
                    55 + u32::from(value) * 40
                }
            };
            (level(index / 36) << 16) | (level((index / 6) % 6) << 8) | level(index % 6)
        }
        _ => {
            let value = 8 + u32::from(index - 232) * 10;
            (value << 16) | (value << 8) | value
        }
    };
    rgba((value << 8) | 255)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_tokens_match_ui_spec_table() {
        // Spot-check a few Dark column entries from the UI spec §2 table.
        assert_eq!(u32::from(DARK.bg_base), 0x202020FF);
        assert_eq!(u32::from(DARK.text_primary), 0xEDEDEDFF);
        assert_eq!(u32::from(DARK.code_bg), 0x262626FF);
    }

    #[test]
    fn light_tokens_match_ui_spec_table() {
        // Spot-check a few Light column entries from the UI spec §2 table.
        assert_eq!(u32::from(LIGHT.bg_base), 0xFFFFFFFF);
        // Preserve the original semantic-color guards while checking the R4
        // palette refresh around the neutral tokens.
        assert_eq!(u32::from(LIGHT.success), 0x1A7F37FF);
        assert_eq!(u32::from(LIGHT.danger), 0xCF222EFF);
        assert_eq!(u32::from(LIGHT.bg_sidebar), 0xF3F3F3FF);
        assert_eq!(u32::from(LIGHT.code_bg), 0xF6F6F6FF);
        assert_eq!(u32::from(LIGHT.accent), 0x3478D8FF);
        assert_eq!(u32::from(LIGHT.brand_primary_strong), 0x245AAFFF);
        assert_eq!(u32::from(LIGHT.bg_active), 0xEAF2FCFF);
    }

    #[test]
    fn light_and_dark_palettes_differ_on_key_tokens() {
        assert_ne!(u32::from(LIGHT.bg_base), u32::from(DARK.bg_base));
        assert_ne!(u32::from(LIGHT.text_primary), u32::from(DARK.text_primary));
        assert_ne!(
            u32::from(LIGHT.brand_primary),
            u32::from(DARK.brand_primary)
        );
        assert_ne!(u32::from(LIGHT.bg_active), u32::from(DARK.bg_active));
    }

    #[test]
    fn brand_tokens_match_r17_logo_palette() {
        assert_eq!(u32::from(LIGHT.brand_primary), 0x3478D8FF);
        assert_eq!(u32::from(DARK.brand_primary), 0x8FC7FFFF);
        assert_eq!(u32::from(DARK.brand_primary_strong), 0x609DE1FF);
        assert_eq!(u32::from(LIGHT.brand_on_accent), 0xFFFFFFFF);
        assert_eq!(u32::from(DARK.brand_on_accent), 0x13233AFF);
    }

    #[test]
    fn r19_phase_one_geometry_is_frozen() {
        assert_eq!(Typography::SIDEBAR_LINE_HEIGHT, 32.0);
        assert_eq!(Layout::SIDEBAR_WIDTH, 260.0);
        assert_eq!(Layout::MAIN_CONTENT_GAP, 4.0);
        assert_eq!(Layout::MAIN_HEADER_HEIGHT, 46.0);
        assert_eq!(Layout::CONTENT_MAX_WIDTH, 820.0);
        assert_eq!(Layout::COMPOSER_MAX_WIDTH, 736.0);
        assert_eq!(Layout::COMPOSER_MIN_HEIGHT, 100.0);
        assert_eq!(Layout::ENVIRONMENT_RAIL_WIDTH, 292.0);
        assert_eq!(Layout::ENVIRONMENT_CARD_INSET, 16.0);
        assert_eq!(Layout::ENVIRONMENT_CARD_RADIUS, 18.0);
        assert_eq!(Layout::ENVIRONMENT_BREAKPOINT, 1180.0);
        assert_eq!(Layout::WORKSPACE_HEADER_HEIGHT, 40.0);
        assert_eq!(Layout::BOTTOM_WORKSPACE_HEIGHT, 272.0);
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
