use std::ops::Range;

use gpui_kit::prelude::*;
use gpui_kit::{
    App, Bounds, ClipboardItem, Context, CursorStyle, Element, ElementId, ElementInputHandler,
    Entity, EntityInputHandler, FocusHandle, Focusable, GlobalElementId, InspectorElementId,
    LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point,
    ShapedLine, SharedString, Style, TextAlign, TextRun, UTF16Selection, UnderlineStyle, Window,
    actions, div, fill, point, px, relative, size,
};
use vega_theme::{Typography, theme};

actions!(
    vega_text_input,
    [
        Backspace,
        Delete,
        Left,
        Right,
        Up,
        Down,
        SelectUp,
        SelectDown,
        SelectLeft,
        SelectRight,
        SelectAll,
        Home,
        End,
        ShowCharacterPalette,
        Paste,
        Cut,
        Copy,
        InsertNewline,
        AcceptPrediction,
        DismissPrediction
    ]
);

/// Character displayed for every content character in masked (key) fields.
const MASK_CHAR: char = '•';

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mention {
    range: Range<usize>,
    path: SharedString,
}

impl Mention {
    pub fn new(range: Range<usize>, path: impl Into<SharedString>) -> Self {
        Self {
            range,
            path: path.into(),
        }
    }

    pub fn range(&self) -> Range<usize> {
        self.range.clone()
    }

    pub fn path(&self) -> &str {
        &self.path
    }
}

/// One laid-out display line: its start offset in the content, the segment
/// length (excluding the separating `\n`), and the shaped line. Single-line
/// inputs keep exactly one entry (start 0).
#[derive(Clone)]
pub(crate) struct LineLayout {
    start: usize,
    len: usize,
    line: ShapedLine,
}

/// A text input (single-line, or fixed-row multi-line for the Composer).
///
/// Editing state (content, selection, marked/IME range) lives on the entity;
/// the platform talks to it through [`EntityInputHandler`]. When `masked` is
/// set the field renders [`MASK_CHAR`] per character and refuses cut/copy, so
/// the real value is never shown or extracted through the UI.
pub struct TextInput {
    image_paste: bool,
    edit_menu: bool,
    focus_handle: FocusHandle,
    content: SharedString,
    content_revision: u64,
    prediction: Option<SharedString>,
    placeholder: SharedString,
    placeholder_key: Option<&'static str>,
    masked: bool,
    multiline: bool,
    /// Visible row count, dynamically clamped to the configured row range.
    rows: usize,
    /// Minimum visible rows for this input's layout viewport.
    min_rows: usize,
    /// Maximum visible rows for this input's layout viewport.
    max_rows: usize,
    /// First painted visual row when wrapped content exceeds the viewport.
    first_visible_row: usize,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    mentions: Vec<Mention>,
    /// Layout cache from the last paint (one entry per display line).
    last_lines: Vec<LineLayout>,
    navigation_lines: Vec<LineLayout>,
    navigation_revision: u64,
    vertical_goal: Option<(Range<usize>, bool, u64, Pixels)>,
    /// Line height used at the last paint (for y → line mapping).
    last_line_height: Pixels,
    last_bounds: Option<Bounds<Pixels>>,
    mouse_selection: Option<mouse_selection::MouseSelection>,
    bare_chrome: bool,
    editor_scroll: Option<(Range<usize>, u64, usize)>,
    scroll_handle: element::TextScrollHandle,
    scrollable_editor: bool,
    scroll_fade_on_base: bool,
    total_visual_rows: usize,
}

mod element;
mod input_handler;
mod mouse_selection;
mod state;

/// Explicit Composer-only clipboard intent; never log clipboard payloads.
pub struct ImagePaste(pub std::sync::Arc<ClipboardItem>);
impl gpui_kit::EventEmitter<ImagePaste> for TextInput {}

impl TextInput {
    pub fn with_placeholder_key(mut self, key: &'static str) -> Self {
        self.placeholder_key = Some(key);
        self
    }

    pub fn set_placeholder_key(&mut self, key: &'static str, cx: &mut Context<Self>) {
        if self.placeholder_key != Some(key) {
            self.placeholder_key = Some(key);
            cx.notify();
        }
    }

    pub fn with_edit_menu(mut self) -> Self {
        self.edit_menu = true;
        self
    }

    fn localized_placeholder(&self) -> SharedString {
        self.placeholder_key
            .map_or_else(|| self.placeholder.clone(), |key| vega_i18n::t(key).into())
    }

    /// Enables attachment paste only for the conversation Composer.
    pub fn with_image_paste(mut self) -> Self {
        self.image_paste = true;
        self
    }
}
