//! GPUI views: sidebar with the projects/sessions blocks (T09 shell +
//! T12 content), settings skeleton (A1-10), shared input components, and the
//! S3-T17 virtualized conversation stream.
//!
//! The T10/T11 temporary full-page projects/threads views were retired in
//! T12; their data functions live on in `vega_store` / `vega_conversation`
//! and the sidebar blocks render the lists now.

pub mod conversation_stream;
pub mod permission_card;
pub mod plan_card;
pub mod settings;
pub mod sidebar;
pub mod text_input;
pub mod tool_card;

use gpui::{App, KeyBinding};

/// Registers the key bindings required by the vega_ui input components
/// (editing keys for [`text_input::TextInput`]), the T13 inline-rename
/// submit key (scoped to the `ThreadRename` key context so it cannot clash
/// with other views), and the T18 Composer keys (Enter = newline,
/// Cmd+Enter = send, scoped to `Composer`). Call once at app startup; the
/// settings actions are bound by the `vega` binary itself.
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
        // T13 行内重命名：Enter 提交（作用域 ThreadRename；Esc 取消通过
        // 重命名编辑器拦截全局 CloseSettings 动作实现，见 sidebar.rs）。
        KeyBinding::new("enter", sidebar::ConfirmRename, Some("ThreadRename")),
        // T18 Composer：Enter=换行、Cmd+Enter=发送（架构师裁定，ui-spec
        // §4.4 未定项）。作用域 Composer——仅在 Composer 输入聚焦时生效，
        // 不影响设置表单与行内重命名。
        KeyBinding::new("enter", text_input::InsertNewline, Some("Composer")),
        KeyBinding::new(
            "cmd-enter",
            conversation_stream::SendMessage,
            Some("Composer"),
        ),
        KeyBinding::new("up", conversation_stream::PreviousMessage, Some("Composer")),
        KeyBinding::new(
            "enter",
            permission_card::PermissionEnter,
            Some("PermissionCard"),
        ),
        KeyBinding::new(
            "cmd-enter",
            permission_card::PermissionAlways,
            Some("PermissionCard"),
        ),
        KeyBinding::new(
            "escape",
            permission_card::PermissionDeny,
            Some("PermissionCard"),
        ),
        KeyBinding::new(
            "tab",
            permission_card::PermissionNextFocus,
            Some("PermissionCard"),
        ),
        KeyBinding::new(
            "shift-tab",
            permission_card::PermissionPreviousFocus,
            Some("PermissionCard"),
        ),
        KeyBinding::new(
            "space",
            permission_card::PermissionActivate,
            Some("PermissionCard"),
        ),
        KeyBinding::new("enter", plan_card::PlanActivate, Some("PlanCard")),
        KeyBinding::new("space", plan_card::PlanActivate, Some("PlanCard")),
        KeyBinding::new("tab", plan_card::PlanNext, Some("PlanCard")),
        KeyBinding::new("shift-tab", plan_card::PlanPrevious, Some("PlanCard")),
        KeyBinding::new(
            "enter",
            conversation_stream::ActivateThreadSetting,
            Some("ThreadSettings"),
        ),
        KeyBinding::new(
            "space",
            conversation_stream::ActivateThreadSetting,
            Some("ThreadSettings"),
        ),
    ]);
}
