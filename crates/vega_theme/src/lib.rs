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
    /// Main content area background.
    pub bg_base: Rgba,
    /// Sidebar background.
    pub bg_sidebar: Rgba,
    /// Cards and composer background.
    pub bg_elevated: Rgba,
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
    /// Selected/active fill as **5% foreground ink over the current surface**
    /// (R50 contract A).
    ///
    /// Reference semantics: `--color-background-primary-ghost-hover`
    ///   = `color-mix(in oklab, var(--color-text-foreground) 5%, transparent)`
    /// with `--color-text-foreground: #1a1c1f` in the light theme. The alpha is
    /// kept so GPUI composites it against whatever surface the element paints
    /// on: `#f9f9f9` yields 238 (`#EEEEEE`), `#ffffff` yields 244 (`#F4F4F4`).
    /// Flattening it to a single opaque constant is exactly the defect this
    /// token exists to avoid. Dark mode uses 3% white (`#ffffff08`) instead of
    /// the light ink, and must never fall back to the light value.
    pub bg_active_alpha: Rgba,
    /// Transient fill for Composer utility chips while hovered or open.
    ///
    /// This is deliberately a translucent shared token rather than a
    /// pre-composited surface: the same chip is mounted on the light utility
    /// bar in both states, and GPUI must retain the alpha so the fill remains
    /// correct if that surface changes. Light uses neutral `#DBDBDB` at 60%
    /// alpha; dark uses white at 10% alpha (R1/R2).
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
    bg_sidebar: rgba(0xFAF9F9FF),
    bg_elevated: rgba(0xFFFFFFFF),
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
    bg_base: rgba(0x202020FF),
    bg_sidebar: rgba(0x191919FF),
    bg_elevated: rgba(0x2A2A2AFF),
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
    /// Sidebar primary navigation and task/project title size: 13px.
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
    /// Maximum body height for expanded tool details and live reasoning.
    pub const DISCLOSURE_CONTENT_MAX_HEIGHT: f32 = 240.0;
    /// Maximum child-list height for an expanded adjacent tool group.
    pub const TOOL_GROUP_MAX_HEIGHT: f32 = 320.0;
    /// Minimum readable Markdown table column: eight message ems.
    pub const MARKDOWN_TABLE_COLUMN_MIN_WIDTH: f32 = 8.0 * Typography::MESSAGE;
    /// Leading space reserved for native macOS titlebar controls.
    pub const TITLEBAR_LEADING_INSET: f32 = 96.0;
    /// Exact interactive surface used by shared titlebar controls.
    pub const TITLEBAR_CONTROL_SIZE: f32 = 28.0;
    /// Gap between adjacent shared titlebar control surfaces.
    pub const TITLEBAR_CONTROL_GAP: f32 = 4.0;
    /// Trailing inset of toolbar chrome pinned to the window's right edge
    /// (R47): the Codex WebView `--padding-toolbar` token (`spacing * 2` = 8px,
    /// AX-verified against the real app). The shell slot cluster paints its
    /// slots with this inset instead of a generic 12px padding.
    pub const TOOLBAR_TRAILING_INSET: f32 = 8.0;
    /// Trailing width `main-header` must reserve for the R46 window-anchored
    /// shell slot cluster: three 28px slots on the frozen 6px gaps (96px) plus
    /// the shared [`Layout::TOOLBAR_TRAILING_INSET`] the cluster itself also
    /// uses. The cluster pins to the window's top-right corner with that
    /// inset, so no column, rail or pane can move it.
    pub const SHELL_SLOT_CLUSTER_RESERVE: f32 = 104.0;
    /// Ownership gutter between a top-band pane's own trailing actions and the
    /// window-anchored shell slot band (R47): the pane header reserves
    /// [`Layout::SHELL_SLOT_CLUSTER_RESERVE`] plus this gutter, so the two
    /// layers (pane content vs window layout) never read as one button row.
    /// Codex native measurement puts the gap at ≈32px.
    pub const SHELL_SLOT_GUTTER: f32 = 32.0;
    /// Maximum readable width for conversation, settings, and diff content.
    ///
    /// Issue #100: this is the **same** width the Composer uses
    /// ([`Layout::COMPOSER_MAX_WIDTH`]). The reference implementation drives
    /// both the thread content column and the Composer container from a single
    /// token (`--thread-content-max-width: 48rem` = 768px), so the two surfaces
    /// share one edge. Vega previously froze two independently measured values
    /// (820 / 736) and the Composer sat 42px narrower on each side. The two
    /// constants stay separate literals so each remains independently pinned,
    /// and the frozen test in this file asserts they are equal.
    pub const CONTENT_MAX_WIDTH: f32 = 768.0;
    /// Minimum horizontal page padding around a readable content column.
    pub const CONTENT_PADDING: f32 = 16.0;
    /// Radius for ordinary panels and cards.
    pub const PANEL_RADIUS: f32 = 12.0;
    /// Radius for the primary composer surface.
    pub const COMPOSER_RADIUS: f32 = 20.0;
    /// Minimum height of the primary composer shell.
    pub const COMPOSER_MIN_HEIGHT: f32 = 100.0;
    /// Issue 63: bounded attachment preview, shared by draft and user turn.
    pub const ATTACHMENT_THUMBNAIL: f32 = 72.0;
    /// R59 R5: maximum height of a composer model-picker floating layer.
    ///
    /// R57 shipped no bound at all, so appending the tier slider to the model
    /// menu produced a 1409px-tall popup that covered the whole transcript and
    /// the R49 utility bar (R59 §1 D2). Every picker layer is now capped here
    /// and scrolls inside itself instead.
    pub const COMPOSER_PICKER_MAX_HEIGHT: f32 = 320.0;
    /// R61 R1: visible clearance between a picker layer's bottom edge and the
    /// top edge of the **model trigger** it hangs from.
    ///
    /// R59 anchored the layers to the composer stack's top edge and called the
    /// same 8px `COMPOSER_PICKER_ANCHOR_GAP`. R61 moved the anchor to the
    /// trigger itself (see `render_model_selector`) so the card hugs the
    /// model button (R61 §1 D1/D2); the value is unchanged, and R61 §6 M1
    /// leaves the final number to visual review. R61 A1 judges the result as
    /// "within 12px", which is what a test pins this token against.
    pub const COMPOSER_PICKER_TRIGGER_GAP: f32 = 8.0;
    /// Top padding of the Composer wrapper column inside the conversation.
    pub const COMPOSER_PADDING_TOP: f32 = 12.0;
    /// Bottom padding of the Composer wrapper column inside the conversation.
    pub const COMPOSER_PADDING_BOTTOM: f32 = 16.0;
    /// R49 utility bar height, from the Codex native 2x capture of the new-task
    /// composer (`bar top 707 .. bottom 744`). The bar sits above the composer
    /// card with zero overlap, so its bottom edge equals the card top.
    pub const COMPOSER_UTILITY_BAR_HEIGHT: f32 = 37.0;
    /// R49 utility bar horizontal inset relative to the composer card: the bar
    /// is 19px narrower on each side (`bar 510.0 .. 1219.5` vs
    /// `card 497.0 .. 1232.5`), which is what makes it read as a tab strip
    /// tucked under the card.
    pub const COMPOSER_UTILITY_BAR_INSET: f32 = 19.0;
    /// R49 utility bar top corner radius. Codex's captured transition measures
    /// 8..12; 12 is taken so the bar reads as one layer below the 20px card.
    /// The bottom corners stay square — the card continues the surface.
    pub const COMPOSER_UTILITY_BAR_RADIUS: f32 = 12.0;
    /// R3 gap between adjacent utility-bar chips after the R49 chip-state
    /// revision. The chip capsules themselves own 8px horizontal padding, so
    /// the text-to-icon visual distance remains 24px (8 + 8 + 8).
    pub const COMPOSER_UTILITY_CHIP_GAP: f32 = 8.0;
    /// R49 leading inset of the first utility-bar chip from the bar's left
    /// edge (Codex `chip1 left 524.5 - bar left 510.0`).
    pub const COMPOSER_UTILITY_CHIP_INSET: f32 = 14.5;
    /// R1 utility chip interactive height. This is smaller than the legacy
    /// R19 branch trigger, whose 32px geometry remains unchanged when the
    /// selector is mounted outside the Composer chip chrome.
    pub const COMPOSER_UTILITY_CHIP_HEIGHT: f32 = 28.0;
    /// R1 utility chip horizontal content padding on both sides.
    pub const COMPOSER_UTILITY_CHIP_PADDING_X: f32 = 8.0;
    /// R1 utility chip capsule radius (half of the 28px chip height).
    pub const COMPOSER_UTILITY_CHIP_RADIUS: f32 = 14.0;
    /// Issue #98: the floating "back to bottom" control's circular hit face.
    ///
    /// The reference implementation (Codex) shows a 32px circle above the
    /// composer when the transcript is detached from the live tail; the Issue
    /// #98 screenshot measured 63–64px at 2x, i.e. 32px logical.
    pub const SCROLL_TO_BOTTOM_SIZE: f32 = 32.0;
    /// Issue #98: clearance between the floating "back to bottom" control's
    /// bottom edge and the transcript viewport's bottom edge (measured ≈24px
    /// above the composer card in the reference).
    pub const SCROLL_TO_BOTTOM_GAP: f32 = 24.0;
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
    /// Shared leading origin for Sidebar navigation row titles.
    pub const SIDEBAR_NAV_CONTENT_INSET: f32 = 32.0;
    /// R48 indent ladder, label column: extra leading inset a Sidebar section
    /// label adds on top of the navigation content origin. Zero keeps
    /// `Pinned / Projects / Recents` on the content origin itself, which is the
    /// column the project folder icon shares (Codex AX: label 17.5, folder icon
    /// 16.5 in the same coordinate system).
    pub const SIDEBAR_LABEL_INSET: f32 = 0.0;
    /// R48 indent ladder, text column: leading inset that lands Sidebar row
    /// **text** on the shared text column, measured from the navigation content
    /// origin.
    ///
    /// This is a **derived value**, not an independent parameter: it is the
    /// project row's folder icon (16px) plus its `gap_2` (8px), i.e.
    /// `16.0 + 8.0`. Rows without an icon (project child tasks, Show More /
    /// Show Less controls) use it directly so they land on the same column the
    /// project row reaches by laying out icon + gap. Changing the icon size or
    /// the row gap without updating this token re-splits the two columns, which
    /// is exactly what the frozen test below guards.
    pub const SIDEBAR_ROW_INSET: f32 = 24.0;
    /// Stable width reserved for project metadata in pinned task rows.
    pub const SIDEBAR_PROJECT_METADATA_WIDTH: f32 = 85.0;
    /// R50 section pitch: the vertical gap **between two Sidebar sections**
    /// (`Pinned` → `Projects` → `Recents`), and nothing else.
    ///
    /// This is a section pitch, not a generic gap: it is the single source of
    /// section-to-section spacing in `render_organization`, replacing the
    /// shared `body.gap_2()` (8px) that left the Pinned section carrying an
    /// extra `mb_1()` (4px). Those two produced 12px at Pinned→Projects and
    /// 8px at Projects→Recents for the same relationship — the defect R50
    /// removes by giving every boundary one value.
    ///
    /// Value derivation. The reference (Codex 2x capture, logical px) measures
    /// a text band gap of ≈19.6px between rows inside a list and 31px / 43px
    /// from a section's last row to the next section label (the two differ by
    /// whether the previous section ends in a scrollable list). An exact match
    /// is impossible this round: Vega's frozen row height is 32px against the
    /// reference's 30px, so Vega's own intra-list band gap is 11px, not 19.6.
    /// What transfers is the *relationship*: the section boundary should read
    /// as a break, not as another row. At 12px the measured boundary is a
    /// 21.5px band gap, i.e. 1.95× the 11px row band gap — between the
    /// reference's two boundary ratios (31/19.6 = 1.58, 43/19.6 = 2.19).
    ///
    /// 12 is also the value already frozen by user-verified R35 acceptance and
    /// written into the design guidelines (`Pinned` → `Projects` 12px), so
    /// unifying on it corrects the 8px boundary instead of regressing the
    /// documented one. Any change here must keep both boundaries equal; see the
    /// structural invariant test in
    /// `sidebar/threads_block/organization/tests.rs`.
    pub const SIDEBAR_SECTION_GAP: f32 = 12.0;
    /// Composer width cap.
    ///
    /// Issue #100: equal to [`Layout::CONTENT_MAX_WIDTH`] by contract — the
    /// Composer and the body column share one edge, because the reference
    /// implementation drives both from a single `--thread-content-max-width`
    /// (48rem = 768px). The frozen test in this file fails if the two diverge.
    pub const COMPOSER_MAX_WIDTH: f32 = 768.0;
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
    pub const SETTINGS_CONTENT_MAX_WIDTH: f32 = 744.0;
    /// Width of a Settings boolean switch.
    pub const SETTINGS_SWITCH_WIDTH: f32 = 32.0;
    /// Height of a Settings boolean switch.
    pub const SETTINGS_SWITCH_HEIGHT: f32 = 20.0;
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
    /// Workspace tab row height (R51).
    pub const TAB_HEIGHT: f32 = 28.0;
    /// Workspace tab corner radius (R51).
    pub const TAB_RADIUS: f32 = 10.0;
    /// Horizontal inset inside a workspace tab (R51).
    pub const TAB_HORIZONTAL_INSET: f32 = 8.0;
    /// Gap between a workspace tab's type icon, label, and close control (R51).
    pub const TAB_CONTENT_GAP: f32 = 8.0;
    /// Terminal content toolbar height below the workspace tab header.
    pub const TERMINAL_TOOLBAR_HEIGHT: f32 = 32.0;
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
        assert_eq!(u32::from(DARK.bg_hover), 0x282828FF);
        assert_eq!(u32::from(DARK.bg_active), 0x303030FF);
    }

    #[test]
    fn light_tokens_match_ui_spec_table() {
        // Spot-check a few Light column entries from the UI spec §2 table.
        assert_eq!(u32::from(LIGHT.bg_base), 0xFFFFFFFF);
        // Preserve the original semantic-color guards while checking the R4
        // palette refresh around the neutral tokens.
        assert_eq!(u32::from(LIGHT.success), 0x1A7F37FF);
        assert_eq!(u32::from(LIGHT.danger), 0xCF222EFF);
        assert_eq!(u32::from(LIGHT.bg_sidebar), 0xFAF9F9FF);
        assert_eq!(u32::from(LIGHT.text_primary), 0x191C1FFF);
        assert_eq!(u32::from(LIGHT.code_bg), 0xF6F6F6FF);
        assert_eq!(u32::from(LIGHT.accent), 0x3478D8FF);
        assert_eq!(u32::from(LIGHT.brand_primary_strong), 0x245AAFFF);
        assert_eq!(u32::from(LIGHT.bg_hover), 0xF3F3F3FF);
        assert_eq!(u32::from(LIGHT.bg_active), 0xEDEDEDFF);
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
    fn r21_phase_two_geometry_is_frozen() {
        assert_eq!(Typography::SIDEBAR_LINE_HEIGHT, 32.0);
        assert_eq!(Layout::SIDEBAR_WIDTH, 304.0);
        assert_eq!(Layout::SIDEBAR_MIN_WIDTH, 240.0);
        assert_eq!(Layout::SIDEBAR_MAX_WIDTH, 365.0);
        assert_eq!(Layout::SIDEBAR_RESIZE_HIT_AREA, 5.0);
        assert_eq!(Layout::MAIN_CONTENT_GAP, 0.0);
        assert_eq!(Layout::MAIN_HEADER_HEIGHT, 46.0);
        // Issue #100: the body column and the Composer share one width. The
        // reference implementation drives both from a single
        // `--thread-content-max-width` (48rem = 768px); Vega previously froze
        // two independently measured values (820 / 736) and the Composer sat
        // 42px narrower on each side.
        assert_eq!(Layout::CONTENT_MAX_WIDTH, 768.0);
        assert_eq!(Layout::COMPOSER_MAX_WIDTH, 768.0);
        // R2: the two must never drift apart. Checked at compile time (like the
        // R61 relation) so a one-sided edit fails the build rather than merely
        // changing one token and leaving a token-to-token assertion to catch it.
        const {
            assert!(Layout::CONTENT_MAX_WIDTH == Layout::COMPOSER_MAX_WIDTH);
        }
        assert_eq!(
            Layout::CONTENT_MAX_WIDTH,
            Layout::COMPOSER_MAX_WIDTH,
            "the body column and the Composer must share one width (issue #100)"
        );
        assert_eq!(Layout::COMPOSER_RADIUS, 20.0);
        assert_eq!(Layout::COMPOSER_MIN_HEIGHT, 100.0);
        assert_eq!(Layout::ENVIRONMENT_RAIL_WIDTH, 320.0);
        assert_eq!(Layout::ENVIRONMENT_CARD_WIDTH, 304.0);
        assert_eq!(Layout::ENVIRONMENT_CARD_INSET, 16.0);
        assert_eq!(Layout::ENVIRONMENT_CARD_RADIUS, 18.0);
        assert_eq!(Layout::ENVIRONMENT_BREAKPOINT, 1230.0);
        assert_eq!(Layout::SETTINGS_CONTENT_MAX_WIDTH, 744.0);
        assert_eq!(Layout::SETTINGS_SWITCH_WIDTH, 32.0);
        assert_eq!(Layout::SETTINGS_SWITCH_HEIGHT, 20.0);
        assert_eq!(Layout::MENU_MAX_WIDTH, 350.0);
        assert_eq!(Layout::MENU_RADIUS, 18.0);
        assert_eq!(Layout::COMPOSER_SEND_SIZE, 28.0);
        assert_eq!(Layout::WORKSPACE_HEADER_HEIGHT, 40.0);
        assert_eq!(Layout::BOTTOM_WORKSPACE_HEIGHT, 272.0);
    }

    #[test]
    fn r51_workspace_tab_geometry_is_frozen() {
        assert_eq!(Layout::TAB_HEIGHT, 28.0);
        assert_eq!(Layout::TAB_RADIUS, 10.0);
        assert_eq!(Layout::TAB_HORIZONTAL_INSET, 8.0);
        assert_eq!(Layout::TAB_CONTENT_GAP, 8.0);
    }

    #[test]
    fn issue98_scroll_to_bottom_tokens_are_frozen() {
        // Issue #98: the floating "back to bottom" circle is 32px (63–64px at
        // 2x in the reference screenshot) and floats 24px above the transcript
        // viewport's bottom edge.
        assert_eq!(Layout::SCROLL_TO_BOTTOM_SIZE, 32.0);
        assert_eq!(Layout::SCROLL_TO_BOTTOM_GAP, 24.0);
    }

    #[test]
    fn r45_composer_padding_tokens_are_frozen() {
        assert_eq!(Layout::COMPOSER_PADDING_TOP, 12.0);
        assert_eq!(Layout::COMPOSER_PADDING_BOTTOM, 16.0);
    }

    #[test]
    fn r49_composer_utility_bar_tokens_are_frozen() {
        // Codex native 2x capture: bar 510.0..1219.5 (h 37) over card
        // 497.0..1232.5, chips at 524.5 / 601.5 / 683.0.
        assert_eq!(Layout::COMPOSER_UTILITY_BAR_HEIGHT, 37.0);
        assert_eq!(Layout::COMPOSER_UTILITY_BAR_INSET, 19.0);
        assert_eq!(Layout::COMPOSER_UTILITY_BAR_RADIUS, 12.0);
        assert_eq!(Layout::COMPOSER_UTILITY_CHIP_GAP, 8.0);
        assert_eq!(Layout::COMPOSER_UTILITY_CHIP_INSET, 14.5);
        // The bar is a narrower, tighter layer above the card, never a
        // replacement: its inset is non-zero and its top radius stays below
        // the card's 20px so the two surfaces read as separate layers.
        assert_ne!(Layout::COMPOSER_UTILITY_BAR_INSET, 0.0);
        assert_ne!(
            Layout::COMPOSER_UTILITY_BAR_RADIUS,
            Layout::COMPOSER_RADIUS,
            "the utility bar must not reuse the composer card radius"
        );
    }

    #[test]
    fn r1_composer_utility_chip_geometry_is_literal_and_shared() {
        // R1 freezes these values independently of the production call sites:
        // a mutation to a component must fail this contract rather than merely
        // changing the token and making a token-to-token assertion pass.
        assert_eq!(Layout::COMPOSER_UTILITY_CHIP_HEIGHT, 28.0);
        assert_eq!(Layout::COMPOSER_UTILITY_CHIP_PADDING_X, 8.0);
        assert_eq!(Layout::COMPOSER_UTILITY_CHIP_RADIUS, 14.0);
        assert_eq!(Layout::COMPOSER_UTILITY_CHIP_RADIUS * 2.0, 28.0);
        assert_eq!(Layout::COMPOSER_UTILITY_CHIP_GAP, 8.0);
    }

    #[test]
    fn r2_composer_utility_chip_overlay_tokens_keep_the_requested_alpha() {
        assert_eq!(u32::from(LIGHT.bg_utility_chip_overlay) >> 8, 0xDBDBDB);
        assert_eq!(LIGHT.bg_utility_chip_overlay.a, 0.60);
        assert_eq!(u32::from(DARK.bg_utility_chip_overlay) >> 8, 0xFFFFFF);
        assert_eq!(DARK.bg_utility_chip_overlay.a, 0.10);
    }

    #[test]
    fn r2_composer_utility_chip_overlay_composites_to_the_requested_samples() {
        // Source-over calculation from the task contract:
        // light: .6 * #DBDBDB + .4 * #FAF9F9 = (231, 231, 231)
        // dark:  .1 * #FFFFFF + .9 * #191919 = (48, 48, 48)
        let light = composite(LIGHT.bg_utility_chip_overlay, 0xFAF9F9);
        let dark = composite(DARK.bg_utility_chip_overlay, 0x191919);
        assert_eq!(light, [231, 231, 231]);
        assert_eq!(dark, [48, 48, 48]);
    }

    #[test]
    fn issue103_disclosure_viewport_tokens_are_frozen() {
        assert_eq!(Layout::DISCLOSURE_CONTENT_MAX_HEIGHT, 240.0);
        assert_eq!(Layout::TOOL_GROUP_MAX_HEIGHT, 320.0);
    }

    /// R61 R1: the trigger-anchoring tokens are frozen.
    ///
    /// R61 renamed `COMPOSER_PICKER_ANCHOR_GAP` (R59's column-top gap) to
    /// `COMPOSER_PICKER_TRIGGER_GAP` because the anchor moved from the composer
    /// column's top edge to the model trigger's top edge. The value is
    /// deliberately unchanged at 8px — R61 §6 M1 leaves the final number to
    /// visual review, and the spec's acceptance threshold (A1, ≤ 12px) is a
    /// tolerance around it rather than a second token.
    #[test]
    fn r61_picker_trigger_anchoring_tokens_are_frozen() {
        assert_eq!(Layout::COMPOSER_PICKER_TRIGGER_GAP, 8.0);
        assert_eq!(Layout::COMPOSER_PICKER_MAX_HEIGHT, 320.0);
        // The hug must be visibly tighter than the bar it overlaps: a gap at or
        // above the bar's own height would put the card back on the column's
        // top edge, which is the R61 §1 D1 defect. The relation is between two
        // constants, so it is checked at compile time rather than as a runtime
        // assertion.
        const {
            assert!(Layout::COMPOSER_PICKER_TRIGGER_GAP < Layout::COMPOSER_UTILITY_BAR_HEIGHT);
        }
    }

    #[test]
    fn r40_command_palette_geometry_is_frozen() {
        assert_eq!(Layout::COMMAND_PALETTE_WIDTH, 520.0);
        assert_eq!(Layout::COMMAND_PALETTE_MAX_HEIGHT, 480.0);
        assert_eq!(Layout::COMMAND_PALETTE_SIDE_INSET, 16.0);
        assert_eq!(Layout::COMMAND_PALETTE_TOP_OFFSET, 76.0);
        assert_eq!(Layout::COMMAND_PALETTE_VERTICAL_RESERVE, 108.0);
        assert_eq!(Layout::COMMAND_PALETTE_MIN_HEIGHT, 160.0);
        assert_ne!(
            Layout::COMMAND_PALETTE_WIDTH,
            Layout::MENU_MAX_WIDTH,
            "the global search palette must not inherit compact menu geometry"
        );
    }

    #[test]
    fn r43_titlebar_control_geometry_is_frozen() {
        assert_eq!(Layout::TITLEBAR_CONTROL_SIZE, 28.0);
        assert_eq!(Layout::TITLEBAR_CONTROL_GAP, 4.0);
        assert_eq!(
            Layout::TITLEBAR_CONTROL_SIZE + Layout::TITLEBAR_CONTROL_GAP,
            32.0
        );
    }

    #[test]
    fn r22_terminal_toolbar_height_is_frozen() {
        assert_eq!(Layout::TERMINAL_TOOLBAR_HEIGHT, 32.0);
    }

    #[test]
    fn r47_panel_alignment_tokens_are_frozen() {
        // Codex `--padding-toolbar` (spacing * 2), AX-verified at 8px.
        assert_eq!(Layout::TOOLBAR_TRAILING_INSET, 8.0);
        // 3×28 slots + 2×6 gaps + the 8px trailing inset.
        assert_eq!(Layout::SHELL_SLOT_CLUSTER_RESERVE, 104.0);
        assert_eq!(
            Layout::SHELL_SLOT_CLUSTER_RESERVE,
            3.0 * Layout::TITLEBAR_CONTROL_SIZE + 2.0 * 6.0 + Layout::TOOLBAR_TRAILING_INSET
        );
        // Pane actions vs. window slot band ownership gutter (Codex ≈32px);
        // top-band pane headers reserve both together.
        assert_eq!(Layout::SHELL_SLOT_GUTTER, 32.0);
        assert_eq!(
            Layout::SHELL_SLOT_CLUSTER_RESERVE + Layout::SHELL_SLOT_GUTTER,
            136.0
        );
    }

    #[test]
    fn r27_sidebar_row_geometry_is_frozen() {
        assert_eq!(Layout::SIDEBAR_NAV_CONTENT_INSET, 32.0);
        assert_eq!(Layout::SIDEBAR_PROJECT_METADATA_WIDTH, 85.0);
    }

    #[test]
    fn r48_sidebar_indent_ladder_is_frozen() {
        // The label column adds nothing on top of the navigation content
        // origin, so section labels share the column with the project folder
        // icon.
        assert_eq!(Layout::SIDEBAR_LABEL_INSET, 0.0);
        // The text column is a derived value: folder icon (16) + row gap (8).
        // Rows without an icon use the token directly, so the two columns stay
        // merged only while this identity holds.
        assert_eq!(Layout::SIDEBAR_ROW_INSET, 16.0 + 8.0);
        assert_eq!(Layout::SIDEBAR_ROW_INSET, 24.0);
        assert_ne!(
            Layout::SIDEBAR_ROW_INSET,
            Layout::SIDEBAR_NAV_CONTENT_INSET,
            "the R48 text column must not silently reuse the legacy 32px session inset"
        );
    }

    #[test]
    fn r50_sidebar_section_pitch_is_frozen() {
        // One explicit pitch for every section boundary (Pinned→Projects→
        // Recents). It replaces the shared 8px body gap plus the Pinned-only
        // 4px margin that made one boundary 12px and the other 8px.
        //
        // 12 keeps the user-verified R35 Pinned→Projects value and corrects the
        // 8px boundary to match, so the two boundaries read as one rhythm
        // instead of one loose and one tight. Measured in the mounted sidebar:
        // 12px section box gap, 21.5px visible text band at both boundaries
        // (1.95× the 11px intra-list band gap), and 42.5px row-to-label
        // text-to-text.
        assert_eq!(Layout::SIDEBAR_SECTION_GAP, 12.0);
        assert_eq!(
            Layout::SIDEBAR_SECTION_GAP % 4.0,
            0.0,
            "the section pitch must stay on the 4px rhythm"
        );
    }

    /// Composites a translucent fill over an opaque `0xRRGGBB` surface,
    /// returning the three 8-bit channels of the result.
    ///
    /// Source-over in straight (non-premultiplied) alpha:
    /// `out = fill * a + surface * (1 - a)`. This mirrors what GPUI does when
    /// it paints a translucent fill onto an opaque parent surface, and it is
    /// the pure-function form of the R50 selected-fill rule.
    fn composite(fill: Rgba, surface: u32) -> [i32; 3] {
        let surface_rgb = [
            ((surface >> 16) & 0xFF) as f32,
            ((surface >> 8) & 0xFF) as f32,
            (surface & 0xFF) as f32,
        ];
        let fill_rgb = [fill.r * 255.0, fill.g * 255.0, fill.b * 255.0];
        let mut out = [0i32; 3];
        for (index, channel) in out.iter_mut().enumerate() {
            *channel =
                (fill_rgb[index] * fill.a + surface_rgb[index] * (1.0 - fill.a)).round() as i32;
        }
        out
    }

    #[test]
    fn r50_selected_fill_alpha_tokens_are_frozen() {
        // The derived fill keeps the reference ratio as a float alpha; the
        // 8-bit channel would bake in a 13/255 rounding step.
        assert_eq!(LIGHT.bg_active_alpha.a, 0.05);
        assert_eq!(DARK.bg_active_alpha.a, 0.03);
        // Light ink is the reference `--color-text-foreground` (#1a1c1f).
        assert_eq!(u32::from(LIGHT.bg_active_alpha) >> 8, 0x1A1C1F);
        // Dark is 3% white, never the light ink.
        assert_eq!(u32::from(DARK.bg_active_alpha) >> 8, 0xFFFFFF);
        assert_ne!(
            u32::from(LIGHT.bg_active_alpha),
            u32::from(DARK.bg_active_alpha),
            "dark must not fall back to the light selected fill"
        );
        // The flattened sidebar token keeps its own value: it is the rule
        // already composited over the `#f9f9f9` sidebar, and 49 call sites
        // depend on it.
        assert_eq!(u32::from(LIGHT.bg_active), 0xEDEDEDFF);
        assert_eq!(u32::from(DARK.bg_active), 0x303030FF);
    }

    #[test]
    fn r50_selected_fill_derives_from_the_current_surface() {
        // Executable proof of the rule: 5% ink over an opaque surface.
        //   #fff    : 0.05*26 + 0.95*255 = 243.55 -> 244 (#f4f4f4)
        //   #f9f9f9 : 0.05*26 + 0.95*249 = 237.85 -> 238 (#eeeeee, measured 237)
        // ±1 tolerance covers the 237/238 truncation difference between the
        // two rounding conventions the reference and Vega use.
        let over_white = composite(LIGHT.bg_active_alpha, 0xFFFFFF);
        let over_sidebar = composite(LIGHT.bg_active_alpha, 0xF9F9F9);
        for channel in over_white {
            assert!(
                (channel - 244).abs() <= 1,
                "5% ink over #ffffff must derive 244, got {channel}"
            );
        }
        for channel in over_sidebar {
            assert!(
                (channel - 238).abs() <= 1,
                "5% ink over #f9f9f9 must derive 238, got {channel}"
            );
        }
        // The whole point: one rule, two surfaces, two results. A flattened
        // constant could not satisfy both.
        assert_ne!(over_white, over_sidebar);
    }

    #[test]
    fn r31_sidebar_typography_restores_compact_sizes() {
        assert_eq!(Typography::SIDEBAR, 13.0);
        assert_eq!(Typography::METADATA, 12.0);
        assert_eq!(Typography::SIDEBAR_LINE_HEIGHT, 32.0);
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
