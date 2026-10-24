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

/// Converts an RGB hex literal (`0xRRGGBB`) plus an explicit alpha
/// (`0.0..=1.0`) to [`Rgba`].
///
/// Derived fills keep their alpha as a ratio, not as an 8-bit channel: the
/// reference rule is `5%` / `3%`, and `13/255` would bake in a rounding step
/// the surface composition then inherits.
const fn rgba_alpha(hex: u32, a: f32) -> Rgba {
    Rgba {
        r: ((hex >> 16) & 0xFF) as f32 / 255.0,
        g: ((hex >> 8) & 0xFF) as f32 / 255.0,
        b: (hex & 0xFF) as f32 / 255.0,
        a,
    }
}

/// The full set of UI color tokens (single appearance, light or dark).
///
/// Field values must come from the token table in the UI spec; the two
/// shipped palettes are [`LIGHT`] and [`DARK`].
#[derive(Debug, Clone, Copy)]
pub struct ThemeColors {
    pub conversation_marker: Rgba,
    pub conversation_clock: Rgba,
    pub conversation_stage: Rgba,
    pub conversation_tool: Rgba,
    pub conversation_divider: Rgba,
    pub conversation_code: Rgba,
    pub conversation_bubble: Rgba,
    pub conversation_user: Rgba,
    /// Main content area background.
    pub bg_base: Rgba,
    /// Sidebar background.
    pub bg_sidebar: Rgba,
    /// Cards and composer background.
    pub bg_elevated: Rgba,
    pub bg_subtle: Rgba,
    /// Hover state background.
    pub bg_hover: Rgba,
    /// Selected state background (current thread).
    ///
    /// This is a fill **already composited for an opaque sidebar surface**
    /// (light `#EDEDEDFF` = the 5% ink rule over `#f9f9f9`). Use it on the
    /// sidebar and other opaque surfaces; on a white surface use
    /// [`ThemeColors::bg_active_alpha`] so the same rule derives the correct
    /// value instead of reusing this flattened constant.
    pub bg_active: Rgba,
    pub bg_active_alpha: Rgba,
    pub bg_utility_chip_overlay: Rgba,
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
    pub brand_primary: Rgba,
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
    conversation_marker: rgba(0xD2D3D3FF),
    conversation_clock: rgba(0x8E8F90FF),
    conversation_stage: rgba(0xA7A8A9FF),
    conversation_tool: rgba(0x767778FF),
    conversation_divider: rgba(0xEDEDEDFF),
    conversation_code: rgba(0xE9EAEAFF),
    conversation_bubble: rgba(0xE8F3FEFF),
    conversation_user: rgba(0x0C264AFF),

    bg_base: rgba(0xFFFFFFFF),
    bg_sidebar: rgba(0xFAF9F9FF),
    bg_elevated: rgba(0xFFFFFFFF),
    bg_subtle: rgba(0xF6F6F6FF),
    bg_hover: rgba(0xF3F3F3FF),
    bg_active: rgba(0xEDEDEDFF),
    bg_active_alpha: rgba_alpha(0x1A1C1F, 0.05),
    bg_utility_chip_overlay: rgba_alpha(0xDBDBDB, 0.60),
    border_subtle: rgba(0xE8E8E8FF),
    text_primary: rgba(0x191C1FFF),
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
    conversation_marker: rgba(0x383838FF),
    conversation_clock: rgba(0xABABABFF),
    conversation_stage: rgba(0x828282FF),
    conversation_tool: rgba(0xABABABFF),
    conversation_divider: rgba(0x383838FF),
    conversation_code: rgba(0x262626FF),
    conversation_bubble: rgba(0x203247FF),
    conversation_user: rgba(0xEDEDEDFF),

    bg_base: rgba(0x202020FF),
    bg_sidebar: rgba(0x191919FF),
    bg_elevated: rgba(0x2A2A2AFF),
    bg_subtle: rgba(0x262626FF),
    bg_hover: rgba(0x282828FF),
    bg_active: rgba(0x303030FF),
    bg_active_alpha: rgba_alpha(0xFFFFFF, 0.03),
    bg_utility_chip_overlay: rgba_alpha(0xFFFFFF, 0.10),
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
    pub const CONVERSATION: f32 = 13.5;
    pub const CONVERSATION_LINE_HEIGHT: f32 = 23.0 / Self::CONVERSATION;
    pub const SETTINGS_TITLE: f32 = 20.0;
    /// Body text: 13px (§3 "正文字体 …13px/1.55 行高").
    pub const BODY: f32 = 13.0;
    /// Body line height: 1.55× font size (ratio, §3).
    pub const BODY_LINE_HEIGHT: f32 = 1.55;
    pub const MESSAGE: f32 = 15.0;
    pub const MARKDOWN_H1: f32 = 22.0;
    pub const MARKDOWN_H2: f32 = 19.0;
    pub const MARKDOWN_H3: f32 = 17.0;
    pub const MESSAGE_LINE_HEIGHT: f32 = 1.65;
    /// Code font size: 12.5px, monospace (§3 "代码字体 SF Mono / JetBrains Mono，12.5px").
    pub const CODE: f32 = 12.5;
    pub const CODE_LINE_HEIGHT: f32 = 1.6;
    /// Sidebar primary navigation and task/project title size: 13px.
    pub const SIDEBAR: f32 = 13.0;
    pub const SIDEBAR_LINE_HEIGHT: f32 = 32.0;
    pub const METADATA: f32 = 12.0;
    pub const EMPTY_STATE_TITLE: f32 = 28.0;
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

pub struct Motion;

impl Motion {
    pub const INTERACTION_DURATION_MS: u64 = 120;
    pub const DISCLOSURE_TURN_MS: u64 = 120;
    pub const TAB_SELECTION_MS: u64 = 120;
    pub const THINKING_SHIMMER_MS: u64 = 1800;
}

/// Shared layout tokens for the native client.
///
/// Keeping radii and content geometry here makes the visual contract explicit
/// at call sites and prevents a component from silently falling back to a
/// named GPUI radius whose value can change between revisions.
pub struct Layout;

impl Layout {
    pub const MARKDOWN_LINK_LEADING_WIDTH: f32 = 20.;
    pub const MARKDOWN_LINK_ICON_GAP: f32 = 4.;
    pub const MARKDOWN_LINK_DASH: f32 = 2.;
    pub const MARKDOWN_LINK_DASH_GAP: f32 = 2.;
    pub const MARKDOWN_LINK_DASH_THICKNESS: f32 = 1.;
    pub const MARKDOWN_LINK_DASH_OFFSET: f32 = 2.;
    pub const LINK_PREVIEW_WIDTH: f32 = 360.;
    pub const LINK_PREVIEW_MARGIN: f32 = 12.;
    pub const LINK_PREVIEW_PADDING: f32 = 12.;
    pub const LINK_PREVIEW_MAX_HEIGHT: f32 = 420.;
    pub const LINK_PREVIEW_RADIUS: f32 = 8.;
    pub const LINK_PREVIEW_ACTION_HEIGHT: f32 = 28.;

    pub const IMAGE_PREVIEW_HEADER: f32 = 40.0;
    pub const IMAGE_PREVIEW_PADDING: f32 = 16.0;
    pub const IMAGE_PREVIEW_HEADER_PADDING: f32 = 12.0;
    pub const IMAGE_PREVIEW_CONTROL: f32 = 28.0;
    pub const IMAGE_PREVIEW_GAP: f32 = 8.0;
    pub const CONVERSATION_MAX_WIDTH: f32 = 736.0;
    pub const CONVERSATION_CLOCK_HEIGHT: f32 = 23.0;
    pub const CONVERSATION_DIVIDER_GAP: f32 = 4.5;
    pub const CONVERSATION_DIVIDER_HEIGHT: f32 = 1.0;
    pub const CONVERSATION_BODY_TOP: f32 = 16.0;
    pub const CONVERSATION_PARAGRAPH_GAP: f32 = 14.0;
    pub const CONVERSATION_STAGE_TOP: f32 = 24.0;
    pub const CONVERSATION_TOOL_HEIGHT: f32 = 25.0;
    pub const CONVERSATION_MARKER_WIDTH: f32 = 6.0;
    pub const CONVERSATION_MARKER_PITCH: f32 = 10.0;
    pub const CONVERSATION_CHEVRON_SIZE: f32 = 12.0;
    pub const CONVERSATION_INLINE_GAP: f32 = 6.0;
    pub const CONVERSATION_USER_RATIO: f32 = 0.7;
    pub const CONVERSATION_USER_PADDING: f32 = 16.0;
    pub const CONVERSATION_USER_BOTTOM: f32 = 48.0;
    pub const COPY_ACTION_SIZE: f32 = 24.0;
    pub const COPY_ACTION_GAP: f32 = 4.0;
    pub const MESSAGE_COPY_FOOTER: f32 = Self::COPY_ACTION_SIZE + Self::COPY_ACTION_GAP;
    pub const SCREENSHOT_MAX_WIDTH: f32 = 360.0;
    pub const SCREENSHOT_MAX_HEIGHT: f32 = 240.0;
    pub const SCREENSHOT_MIN_WIDTH: f32 = 64.0;
    pub const SCREENSHOT_MIN_HEIGHT: f32 = 32.0;
    pub const SCREENSHOT_BORDER: f32 = 1.0;
    pub const SCREENSHOT_RADIUS: f32 = 8.0;
    pub const SCREENSHOT_PREVIEW_MAX_WIDTH: f32 = 960.0;
    pub const SCREENSHOT_PREVIEW_MAX_HEIGHT: f32 = 720.0;
    pub const SCREENSHOT_PREVIEW_MARGIN: f32 = 24.0;
    pub const ICON_SIZE: f32 = 16.0;
    pub const CONTEXT_USAGE_SIZE: f32 = 28.0;
    pub const CONTEXT_USAGE_WIDTH: f32 = 320.0;
    pub const CONTEXT_USAGE_PADDING: f32 = 16.0;
    pub const CONVERSATION_NAVIGATION_GUTTER: f32 = 24.0;
    pub const CONVERSATION_PADDING: f32 = 32.0;
    pub const MESSAGE_ANCHOR_TRACK_WIDTH: f32 = 2.0;
    pub const MESSAGE_ANCHOR_MARKER_SIZE: f32 = 8.0;
    pub const MESSAGE_ANCHOR_ACTIVE_SIZE: f32 = 16.0;
    pub const MESSAGE_ANCHOR_HIT_HEIGHT: f32 = 16.0;
    pub const MESSAGE_ANCHOR_VISIBLE_COUNT: usize = 9;
    pub const MESSAGE_ANCHOR_LEFT_INSET: f32 = 4.0;
    pub const MESSAGE_ANCHOR_PREVIEW_WIDTH: f32 = 280.0;
    pub const MESSAGE_ANCHOR_PREVIEW_HEIGHT: f32 = 72.0;
    pub const READING_NAVIGATION_HEIGHT: f32 = 40.0;
    pub const CONVERSATION_BOTTOM_FADE_HEIGHT: f32 = 48.0;
    pub const SCROLL_EDGE_FADE_COMPACT: f32 = 16.0;
    pub const SCROLL_EDGE_FADE_READING: f32 = 24.0;
    pub const SCROLL_EDGE_FADE_EPSILON: f32 = 1.0;
    pub const READING_NAVIGATION_PADDING: f32 = 4.0;
    pub const READING_NAVIGATION_PADDING_X: f32 = 12.0;
    pub const READING_NAVIGATION_GAP: f32 = 8.0;
    pub const DISCLOSURE_HEADER_HEIGHT: f32 = 28.0;
    pub const DISCLOSURE_TRAILING_SLOT: f32 = 24.0;
    pub const DISCLOSURE_PADDING_X: f32 = 8.0;
    pub const DISCLOSURE_RADIUS: f32 = 8.0;
    pub const USER_MESSAGE_PADDING_TOP: f32 = 24.0;
    pub const ASSISTANT_MESSAGE_PADDING_TOP: f32 = 8.0;
    pub const MESSAGE_PADDING_BOTTOM: f32 = 16.0;
    pub const STATUS_PADDING: f32 = 8.0;
    pub const STATUS_GAP: f32 = 8.0;
    pub const STATUS_RADIUS: f32 = 8.0;
    pub const SETTINGS_SECTION_GAP: f32 = 24.0;
    pub const SETTINGS_CONTENT_GAP: f32 = 12.0;
    pub const SETTINGS_TITLE_GAP: f32 = 16.0;
    pub const SETTINGS_CONTROL_MIN_HEIGHT: f32 = 28.0;
    pub const SETTINGS_CONTROL_PADDING_X: f32 = 12.0;
    pub const SETTINGS_CONTROL_RADIUS: f32 = 8.0;
    pub const SETTINGS_NAV_WIDTH: f32 = 224.0;
    pub const SETTINGS_PROVIDER_LIST_WIDTH: f32 = 180.0;
    pub const SETTINGS_PROVIDER_COLUMN_GAP: f32 = 24.0;
    pub const SETTINGS_PROVIDER_DETAIL_MIN_WIDTH: f32 = 440.0;
    pub const SETTINGS_LABEL_WIDTH: f32 = 208.0;
    pub const SETTINGS_FIELD_GAP: f32 = 16.0;
    pub const SETTINGS_GROUP_PADDING: f32 = 16.0;
    pub const SETTINGS_ROW_PADDING_Y: f32 = 12.0;
    pub const SETTINGS_NOTES_MAX_HEIGHT: f32 = 240.0;
    pub const PROCESS_PADDING_X: f32 = 0.0;
    pub const TOOL_DETAIL_PADDING: f32 = 12.0;
    pub const TOOL_DETAIL_RADIUS: f32 = 12.0;
    pub const DISCLOSURE_SCROLLBAR_GUTTER: f32 = 12.0;
    pub const MARKDOWN_PARAGRAPH_GAP: f32 = 12.0;
    pub const MARKDOWN_HEADING_BEFORE: f32 = 24.0;
    pub const MARKDOWN_HEADING_AFTER: f32 = 12.0;
    pub const MARKDOWN_LIST_ITEM_GAP: f32 = 0.0;
    pub const MARKDOWN_LIST_INDENT: f32 = 24.0;
    pub const MARKDOWN_LIST_PARAGRAPH_GAP: f32 = 8.0;
    pub const MARKDOWN_CODE_PADDING: f32 = 12.0;
    pub const MARKDOWN_INLINE_CODE_PADDING_X: f32 = 4.0;
    pub const MARKDOWN_INLINE_CODE_EXTRA_HEIGHT: f32 = 4.0;
    pub const MARKDOWN_INLINE_CODE_RADIUS: f32 = 4.0;
    pub const MARKDOWN_TABLE_COLUMN_DEMAND_CAP: f32 = 4.0 * Self::MARKDOWN_TABLE_COLUMN_MIN_WIDTH;
    pub const MARKDOWN_TABLE_PADDING_X: f32 = 12.0;
    pub const MARKDOWN_TABLE_PADDING_Y: f32 = 8.0;
    pub const MARKDOWN_BLOCK_GAP: f32 = 16.0;
    pub const PROCESS_ITEM_GAP: f32 = 4.0;
    pub const ASSISTANT_ROLE_HEIGHT: f32 = 20.0;

    pub const DISCLOSURE_CONTENT_MAX_HEIGHT: f32 = 240.0;
    pub const TOOL_GROUP_MAX_HEIGHT: f32 = 320.0;
    /// Minimum readable Markdown table column: eight message ems.
    pub const MARKDOWN_TABLE_COLUMN_MIN_WIDTH: f32 = 8.0 * Typography::MESSAGE;
    /// Leading space reserved for native macOS titlebar controls.
    pub const TITLEBAR_LEADING_INSET: f32 = 96.0;
    /// Exact interactive surface used by shared titlebar controls.
    pub const TITLEBAR_CONTROL_SIZE: f32 = 28.0;
    /// Gap between adjacent shared titlebar control surfaces.
    pub const TITLEBAR_CONTROL_GAP: f32 = 4.0;
    pub const TOOLBAR_TRAILING_INSET: f32 = 8.0;
    pub const SHELL_SLOT_CLUSTER_RESERVE: f32 = 104.0;
    pub const SHELL_SLOT_GUTTER: f32 = 32.0;
    pub const CONTENT_MAX_WIDTH: f32 = 768.0;
    /// Maximum user-message bubble width as a fraction of the message column.
    pub const USER_MESSAGE_MAX_WIDTH_RATIO: f32 = 0.8;
    pub const USER_MESSAGE_RADIUS: f32 = 16.0;
    /// Minimum horizontal page padding around a readable content column.
    pub const CONTENT_PADDING: f32 = 16.0;
    /// Radius for ordinary panels and cards.
    pub const PANEL_RADIUS: f32 = 12.0;
    /// Radius for the primary composer surface.
    pub const COMPOSER_RADIUS: f32 = 20.0;
    /// Minimum height of the primary composer shell.
    pub const COMPOSER_MIN_HEIGHT: f32 = 100.0;
    pub const PENDING_MESSAGE_INSET: f32 = 16.0;
    pub const PENDING_MESSAGE_ROW_HEIGHT: f32 = 36.0;
    pub const PENDING_MESSAGE_OVERLAP: f32 = 8.0;
    pub const PENDING_MESSAGE_RADIUS: f32 = 16.0;
    pub const PENDING_MESSAGE_THUMBNAIL: f32 = 24.0;
    pub const PENDING_MESSAGE_VISIBLE_ROWS: usize = 3;
    pub const ATTACHMENT_THUMBNAIL: f32 = 72.0;
    pub const COMPOSER_PICKER_MAX_HEIGHT: f32 = 320.0;
    pub const COMPOSER_PICKER_TRIGGER_GAP: f32 = 8.0;
    pub const COMPOSER_TEXT_TRIGGER_HEIGHT: f32 = 28.0;
    pub const COMPOSER_TEXT_TRIGGER_RADIUS: f32 = 14.0;
    pub const COMPOSER_TEXT_TRIGGER_PADDING_X: f32 = 12.0;
    /// Top padding of the Composer wrapper column inside the conversation.
    pub const COMPOSER_PADDING_TOP: f32 = 12.0;
    /// Bottom padding of the Composer wrapper column inside the conversation.
    pub const COMPOSER_PADDING_BOTTOM: f32 = 16.0;
    pub const COMPOSER_UTILITY_BAR_HEIGHT: f32 = 37.0;
    pub const COMPOSER_UTILITY_BAR_INSET: f32 = 19.0;
    pub const COMPOSER_UTILITY_BAR_RADIUS: f32 = 12.0;
    pub const COMPOSER_UTILITY_CHIP_GAP: f32 = 8.0;
    pub const COMPOSER_UTILITY_CHIP_INSET: f32 = 14.5;
    pub const COMPOSER_UTILITY_CHIP_HEIGHT: f32 = 28.0;
    pub const COMPOSER_UTILITY_CHIP_PADDING_X: f32 = 8.0;
    pub const COMPOSER_UTILITY_CHIP_RADIUS: f32 = 14.0;
    pub const SCROLL_TO_BOTTOM_SIZE: f32 = 32.0;
    /// Height reserved by every non-Settings main route.
    pub const MAIN_HEADER_HEIGHT: f32 = 46.0;
    /// Gap between the main content panel and the native window edges/sidebar.
    pub const MAIN_CONTENT_GAP: f32 = 0.0;
    /// Provider selector column inside Settings.
    pub const PROVIDER_LIST_WIDTH: f32 = 180.0;
    /// Default Sidebar width for a new or legacy configuration.
    pub const SIDEBAR_WIDTH: f32 = 304.0;
    /// Minimum user-resizable Sidebar width.
    pub const SIDEBAR_MIN_WIDTH: f32 = 240.0;
    /// Maximum user-resizable Sidebar width.
    pub const SIDEBAR_MAX_WIDTH: f32 = 365.0;
    /// Pointer hit area at the trailing edge of the Sidebar.
    pub const SIDEBAR_RESIZE_HIT_AREA: f32 = 5.0;
    /// Sidebar horizontal and vertical padding.
    pub const SIDEBAR_PADDING: f32 = 12.0;
    pub const SIDEBAR_TOOLBAR_CONTENT_GAP: f32 = 18.0;
    /// Shared leading origin for Sidebar navigation row titles.
    pub const SIDEBAR_NAV_CONTENT_INSET: f32 = 32.0;
    pub const SIDEBAR_LABEL_INSET: f32 = 0.0;
    pub const SIDEBAR_ROW_INSET: f32 = 24.0;
    /// Stable width reserved for project metadata in pinned task rows.
    pub const SIDEBAR_PROJECT_METADATA_WIDTH: f32 = 85.0;
    pub const SIDEBAR_SECTION_GAP: f32 = 12.0;
    pub const SIDEBAR_REORDER_LINE_HEIGHT: f32 = 2.0;
    pub const COMPOSER_MAX_WIDTH: f32 = 768.0;
    pub const AUTOMATION_HEIGHT: f32 = 36.0;
    pub const AUTOMATION_INSET: f32 = 16.0;
    pub const AUTOMATION_PADDING: f32 = 12.0;
    pub const AUTOMATION_ICON_SIZE: f32 = 16.0;
    pub const AUTOMATION_ACTION_SIZE: f32 = 28.0;
    pub const AUTOMATION_RADIUS: f32 = 12.0;
    pub const AUTOMATION_GAP: f32 = 4.0;
    pub const AUTOMATION_COMPACT_WIDTH: f32 = 360.0;
    pub const AUTOMATION_COMPACT_INSET: f32 = 8.0;
    pub const AUTOMATION_COMPACT_PADDING: f32 = 6.0;
    /// Fixed wide-screen Environment rail width (304px card + 16px inset).
    pub const ENVIRONMENT_RAIL_WIDTH: f32 = 320.0;
    /// Width of the Environment card inside its rail.
    pub const ENVIRONMENT_CARD_WIDTH: f32 = 304.0;
    /// Top/right inset for the floating Environment card.
    pub const ENVIRONMENT_CARD_INSET: f32 = 16.0;
    /// Environment card radius.
    pub const ENVIRONMENT_CARD_RADIUS: f32 = 18.0;
    /// Width at which the persistent Environment rail becomes available.
    pub const ENVIRONMENT_BREAKPOINT: f32 = 1230.0;
    /// Maximum width of the Settings content column.
    pub const SETTINGS_CONTENT_MAX_WIDTH: f32 = 720.0;
    /// Width of a Settings boolean switch.
    pub const SETTINGS_SWITCH_WIDTH: f32 = 32.0;
    /// Height of a Settings boolean switch.
    pub const SETTINGS_SWITCH_HEIGHT: f32 = 20.0;
    pub const PREPARATION_CARD_WIDTH: f32 = 560.0;
    pub const PREPARATION_CARD_HEIGHT: f32 = 120.0;
    pub const PREPARATION_CARD_SHADOW_PADDING: f32 = 24.0;
    pub const PREPARATION_OVERVIEW_WIDTH: f32 = 600.0;
    pub const PREPARATION_OVERVIEW_HEIGHT: f32 = 440.0;
    pub const PREPARATION_OVERVIEW_PADDING: f32 = 32.0;
    pub const PREPARATION_OVERVIEW_ICON: f32 = 56.0;
    pub const PREPARATION_OVERVIEW_TITLE_LINE: f32 = 34.0;
    pub const PREPARATION_OVERVIEW_DESCRIPTION_WIDTH: f32 = 420.0;
    pub const PREPARATION_OVERVIEW_DESCRIPTION_LINE: f32 = 21.0;
    pub const PREPARATION_PERMISSION_HEIGHT: f32 = 84.0;
    pub const PREPARATION_PERMISSION_ICON: f32 = 40.0;
    pub const PREPARATION_APP_ICON_SIZE: f32 = 28.0;
    pub const PREPARATION_APP_TARGET_HEIGHT: f32 = 42.0;
    pub const PREPARATION_GUIDE_CONTROL: f32 = 32.0;
    pub const PREPARATION_GUIDE_GAP: f32 = 12.0;
    pub const PREPARATION_CLOSE_SIZE: f32 = 24.0;
    /// Maximum width of a multi-section floating menu.
    pub const MENU_MAX_WIDTH: f32 = 350.0;
    /// Preferred width of the global command/search palette.
    pub const COMMAND_PALETTE_WIDTH: f32 = 520.0;
    /// Maximum height of the global command/search palette.
    pub const COMMAND_PALETTE_MAX_HEIGHT: f32 = 480.0;
    /// Horizontal clearance retained between the palette and each viewport edge.
    pub const COMMAND_PALETTE_SIDE_INSET: f32 = 16.0;
    /// Existing top offset for the global command/search palette.
    pub const COMMAND_PALETTE_TOP_OFFSET: f32 = 76.0;
    /// Combined vertical space reserved above and below the palette.
    pub const COMMAND_PALETTE_VERTICAL_RESERVE: f32 = 108.0;
    /// Defensive minimum palette height in unusually short viewports.
    pub const COMMAND_PALETTE_MIN_HEIGHT: f32 = 160.0;
    /// Radius shared by large floating menus and popovers.
    pub const MENU_RADIUS: f32 = 18.0;
    /// Exact circular send/stop control size.
    pub const COMPOSER_SEND_SIZE: f32 = 28.0;
    /// Default bottom workspace height.
    pub const BOTTOM_WORKSPACE_HEIGHT: f32 = 272.0;
    /// Workspace tab/header height.
    pub const WORKSPACE_HEADER_HEIGHT: f32 = 40.0;
    pub const TAB_HEIGHT: f32 = 28.0;
    pub const TAB_RADIUS: f32 = 10.0;
    pub const TAB_HORIZONTAL_INSET: f32 = 8.0;
    pub const TAB_CONTENT_GAP: f32 = 8.0;
    /// Terminal content toolbar height below the workspace tab header.
    pub const TERMINAL_TOOLBAR_HEIGHT: f32 = 32.0;
    pub const SIDEBAR_ACTIONS_WIDTH: f32 = 52.0;
    pub const SIDEBAR_ACTIONS_INSET: f32 = 8.0;
    pub const SIDEBAR_ACTION_SIZE: f32 = 24.0;
    pub const SIDEBAR_ACTION_GAP: f32 = 4.0;
    pub const SIDEBAR_FOOTER_TOP: f32 = 4.0;
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
