//! Light and dark themes.
//!
//! Color tokens defined in [vega-ui-spec.md §2](../../docs/vega-ui-spec.md).
//! All hex color literals in the workspace are confined to this crate;
//! components must reference these tokens instead of hardcoding colors.

use gpui::Rgba;

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
}
