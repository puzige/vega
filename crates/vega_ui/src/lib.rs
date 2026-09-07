//! GPUI views: sidebar with the projects/sessions blocks (T09 shell +
//! T12 content), settings skeleton (A1-10), shared input components, and the
//! S3-T17 virtualized conversation stream.
//!
//! The T10/T11 temporary full-page projects/threads views were retired in
//! T12; their data functions live on in `vega_store` / `vega_conversation`
//! and the sidebar blocks render the lists now.

pub mod artifact_card;
pub mod branch_selector;
pub mod command_palette;
pub mod commit_panel;
pub mod conversation_stream;
pub mod diff_view;
pub mod file_preview;
pub mod file_selector;
pub mod permission_card;
pub mod plan_card;
pub mod settings;
pub mod sidebar;
pub mod summary_card;
pub mod text_input;
pub mod tool_card;

use gpui_kit::{App, KeyBinding};

/// Registers the key bindings required by the vega_ui input components
/// (editing keys for [`text_input::TextInput`]), the T13 inline-rename
/// submit key (scoped to the `ThreadRename` key context so it cannot clash
/// with other views), and the T18 Composer keys (Enter = newline,
/// Cmd+Enter = send, scoped to `Composer`). Call once at app startup; the
/// settings actions are bound by the `vega` binary itself.
pub fn init(cx: &mut App) {
    navigation::init(cx);
    cx.on_app_quit(|_| vega_conversation::terminal::shutdown_all())
        .detach();
    cx.bind_keys([
        KeyBinding::new("cmd-k", command_palette::OpenPalette, None),
        KeyBinding::new("cmd-o", command_palette::OpenWorkspacePicker, None),
        KeyBinding::new("cmd-j", command_palette::ToggleWorkspaceTerminal, None),
        KeyBinding::new("down", command_palette::PaletteNext, Some("CommandPalette")),
        KeyBinding::new(
            "up",
            command_palette::PalettePrevious,
            Some("CommandPalette"),
        ),
        KeyBinding::new(
            "enter",
            command_palette::PaletteAccept,
            Some("CommandPalette"),
        ),
        KeyBinding::new(
            "escape",
            command_palette::PaletteDismiss,
            Some("CommandPalette"),
        ),
        KeyBinding::new(
            "tab",
            command_palette::PaletteNextScope,
            Some("CommandPalette"),
        ),
        KeyBinding::new(
            "tab",
            conversation_stream::NextComposerControl,
            Some("Composer"),
        ),
        KeyBinding::new(
            "shift-tab",
            conversation_stream::PreviousComposerControl,
            Some("Composer"),
        ),
        KeyBinding::new(
            "up",
            conversation_stream::PreviousComposerAction,
            Some("ComposerActions"),
        ),
        KeyBinding::new(
            "down",
            conversation_stream::NextComposerAction,
            Some("ComposerActions"),
        ),
        KeyBinding::new(
            "enter",
            conversation_stream::AcceptComposerAction,
            Some("ComposerActions"),
        ),
        KeyBinding::new(
            "tab",
            conversation_stream::AcceptComposerAction,
            Some("ComposerActions"),
        ),
        KeyBinding::new(
            "escape",
            conversation_stream::CloseComposerActions,
            Some("ComposerActions"),
        ),
        KeyBinding::new(
            "enter",
            conversation_stream::StopComposer,
            Some("ComposerStop"),
        ),
        KeyBinding::new(
            "space",
            conversation_stream::StopComposer,
            Some("ComposerStop"),
        ),
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
        KeyBinding::new(
            "enter",
            settings::ActivatePricingAction,
            Some("PricingSettings"),
        ),
        KeyBinding::new(
            "space",
            settings::ActivatePricingAction,
            Some("PricingSettings"),
        ),
        KeyBinding::new("tab", settings::NextPricingAction, Some("PricingSettings")),
        KeyBinding::new(
            "shift-tab",
            settings::PreviousPricingAction,
            Some("PricingSettings"),
        ),
        // Provider form: multiline models use Enter for a new line; the
        // focused Save/Edit controls accept Enter/Space, and Cmd+Enter saves
        // from any provider input without changing the current thread model.
        KeyBinding::new("enter", text_input::InsertNewline, Some("ProviderSettings")),
        KeyBinding::new(
            "cmd-enter",
            settings::ActivateProviderAction,
            Some("ProviderSettings"),
        ),
        KeyBinding::new(
            "enter",
            settings::ActivateProviderAction,
            Some("ProviderAction"),
        ),
        KeyBinding::new(
            "space",
            settings::ActivateProviderAction,
            Some("ProviderAction"),
        ),
        KeyBinding::new(
            "tab",
            settings::NextProviderAction,
            Some("ProviderSettings"),
        ),
        KeyBinding::new(
            "shift-tab",
            settings::PreviousProviderAction,
            Some("ProviderSettings"),
        ),
        // T13 行内重命名：Enter 提交（作用域 ThreadRename；Esc 取消通过
        // 重命名编辑器拦截全局 CloseSettings 动作实现，见 sidebar.rs）。
        KeyBinding::new("enter", sidebar::ConfirmRename, Some("ThreadRename")),
        // Session rows expose low-frequency actions through one compact menu
        // trigger. The row keeps its own key context while the menu is open,
        // so arrows, Enter/Space, and Esc remain mouse-independent.
        KeyBinding::new(
            "enter",
            sidebar::OpenThreadActions,
            Some("ThreadActionTrigger"),
        ),
        KeyBinding::new(
            "space",
            sidebar::OpenThreadActions,
            Some("ThreadActionTrigger"),
        ),
        KeyBinding::new(
            "up",
            sidebar::PreviousThreadAction,
            Some("ThreadActionsMenu"),
        ),
        KeyBinding::new("down", sidebar::NextThreadAction, Some("ThreadActionsMenu")),
        KeyBinding::new(
            "enter",
            sidebar::ActivateThreadAction,
            Some("ThreadActionsMenu"),
        ),
        KeyBinding::new(
            "space",
            sidebar::ActivateThreadAction,
            Some("ThreadActionsMenu"),
        ),
        KeyBinding::new(
            "escape",
            sidebar::CloseThreadActions,
            Some("ThreadActionsMenu"),
        ),
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
        // A2-12 `@file` 选择器（S8-T47）：作用域 FileSelect，仅当下拉打开
        // 时由 Composer 输入行携带该上下文，Enter/Tab=接受（first-wins）、
        // Up/Down=移动高亮、Esc=取消；关闭时这些键回落到既有绑定。
        KeyBinding::new("enter", file_selector::AcceptFile, Some("FileSelect")),
        KeyBinding::new("tab", file_selector::AcceptFile, Some("FileSelect")),
        KeyBinding::new("up", file_selector::PreviousFile, Some("FileSelect")),
        KeyBinding::new("down", file_selector::NextFile, Some("FileSelect")),
        KeyBinding::new("escape", file_selector::CancelFile, Some("FileSelect")),
        // A failed index gets its own focusable Retry stop. Enter activates it
        // from the failed panel; Space is limited to the focused button.
        KeyBinding::new("enter", file_selector::RetryFile, Some("FileSelectRetry")),
        KeyBinding::new(
            "enter",
            file_selector::RetryFile,
            Some("FileSelectRetryButton"),
        ),
        KeyBinding::new(
            "space",
            file_selector::RetryFile,
            Some("FileSelectRetryButton"),
        ),
        KeyBinding::new("tab", file_selector::FocusRetry, Some("FileSelectRetry")),
        KeyBinding::new(
            "shift-tab",
            file_selector::FocusPreviousRetry,
            Some("FileSelectRetry"),
        ),
        KeyBinding::new("escape", file_selector::CancelFile, Some("FileSelectRetry")),
        // A2-14 模型选择器（S8-T47）：Enter/Space 开合与接受、Up/Down 移动、
        // Esc 关闭；thinking 档位 chip 用 Enter/Space 循环。
        KeyBinding::new(
            "enter",
            conversation_stream::ActivateModel,
            Some("ModelSelector"),
        ),
        KeyBinding::new(
            "space",
            conversation_stream::ActivateModel,
            Some("ModelSelector"),
        ),
        KeyBinding::new(
            "up",
            conversation_stream::PreviousModel,
            Some("ModelSelector"),
        ),
        KeyBinding::new(
            "down",
            conversation_stream::NextModel,
            Some("ModelSelector"),
        ),
        KeyBinding::new(
            "escape",
            conversation_stream::CloseCompactSettings,
            Some("CompactComposerSettings"),
        ),
        KeyBinding::new(
            "escape",
            conversation_stream::CloseModel,
            Some("ModelSelector"),
        ),
        KeyBinding::new(
            "enter",
            conversation_stream::CycleThinking,
            Some("ThinkingLevel"),
        ),
        KeyBinding::new(
            "space",
            conversation_stream::CycleThinking,
            Some("ThinkingLevel"),
        ),
        KeyBinding::new(
            "enter",
            artifact_card::ArtifactActivate,
            Some("ArtifactCard"),
        ),
        KeyBinding::new(
            "space",
            artifact_card::ArtifactActivate,
            Some("ArtifactCard"),
        ),
        KeyBinding::new("escape", artifact_card::ArtifactClear, Some("ArtifactCard")),
        KeyBinding::new(
            "enter",
            branch_selector::ActivateBranch,
            Some("BranchSelector"),
        ),
        KeyBinding::new(
            "space",
            branch_selector::ActivateBranch,
            Some("BranchSelector"),
        ),
        KeyBinding::new(
            "up",
            branch_selector::PreviousBranch,
            Some("BranchSelector"),
        ),
        KeyBinding::new("down", branch_selector::NextBranch, Some("BranchSelector")),
        KeyBinding::new(
            "escape",
            branch_selector::CloseBranchSelector,
            Some("BranchSelector"),
        ),
        KeyBinding::new(
            "enter",
            commit_panel::ActivateCommitEnter,
            Some("CommitPanel"),
        ),
        KeyBinding::new(
            "cmd-enter",
            commit_panel::ConfirmCommitStage,
            Some("CommitPanel"),
        ),
        KeyBinding::new(
            "escape",
            commit_panel::CloseCommitPanel,
            Some("CommitPanel"),
        ),
        KeyBinding::new(
            "space",
            commit_panel::ActivateCommitSpace,
            Some("CommitPanel"),
        ),
        KeyBinding::new("tab", commit_panel::NextCommitFocus, Some("CommitPanel")),
        KeyBinding::new(
            "shift-tab",
            commit_panel::PreviousCommitFocus,
            Some("CommitPanel"),
        ),
        KeyBinding::new(
            "cmd-shift-d",
            conversation_stream::OpenWorkspaceDiff,
            Some("ConversationStream"),
        ),
        KeyBinding::new(
            "cmd-down",
            conversation_stream::ResumeTail,
            Some("ConversationStream"),
        ),
        KeyBinding::new(
            "enter",
            conversation_stream::ResumeTail,
            Some("ResumeTailButton"),
        ),
        KeyBinding::new(
            "space",
            conversation_stream::ResumeTail,
            Some("ResumeTailButton"),
        ),
        KeyBinding::new("escape", diff_view::CloseDiff, Some("DiffView")),
        KeyBinding::new("[", diff_view::PreviousDiffHunk, Some("DiffView")),
        KeyBinding::new("]", diff_view::NextDiffHunk, Some("DiffView")),
    ]);
}

pub mod icons;

pub mod navigation;
pub mod terminal;
