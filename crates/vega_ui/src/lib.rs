//! GPUI views: sidebar shell (T09), projects (A1-03, temporary mount),
//! settings skeleton (A1-10), and shared input components.

pub mod projects;
pub mod settings;
pub mod sidebar;
pub mod text_input;
// T11（A1-02）：临时会话视图，T12 集成时归位侧边栏。
pub mod threads;

use gpui::{App, KeyBinding};

/// Registers the key bindings required by the vega_ui input components
/// (editing keys for [`text_input::TextInput`]). Call once at app startup;
/// the settings actions are bound by the `vega` binary itself.
pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("backspace", text_input::Backspace, None),
        KeyBinding::new("delete", text_input::Delete, None),
        KeyBinding::new("left", text_input::Left, None),
        KeyBinding::new("right", text_input::Right, None),
        KeyBinding::new("shift-left", text_input::SelectLeft, None),
        KeyBinding::new("shift-right", text_input::SelectRight, None),
        KeyBinding::new("cmd-a", text_input::SelectAll, None),
        KeyBinding::new("home", text_input::Home, None),
        KeyBinding::new("end", text_input::End, None),
        KeyBinding::new("ctrl-cmd-space", text_input::ShowCharacterPalette, None),
        KeyBinding::new("cmd-v", text_input::Paste, None),
        KeyBinding::new("cmd-c", text_input::Copy, None),
        KeyBinding::new("cmd-x", text_input::Cut, None),
    ]);
}
