mod presentation;

#[cfg(feature = "nsh-reading-integration")]
pub use presentation::mounted_regressions::{
    marked_palette_keys_leave_controller_and_focus_unchanged,
    ordinary_palette_keys_repeat_only_navigation_and_emit_exact_events,
    retired_palette_requests_cannot_mutate_mounted_controller,
};

use crate::nsh::ProjectionId;
use crate::text_input::TextInput;
use gpui_kit::{prelude::*, *};
use vega_conversation::types::*;

actions!(
    vega_palette,
    [
        OpenPalette,
        OpenWorkspacePicker,
        ToggleWorkspaceTerminal,
        PaletteNext,
        PalettePrevious,
        PaletteAccept,
        PaletteDismiss,
        PaletteNextScope
    ]
);
pub struct PaletteQueryChanged(pub String);
pub struct PaletteActivated(pub PaletteTarget);
pub struct PaletteClosed;
impl EventEmitter<PaletteQueryChanged> for CommandPalette {}
impl EventEmitter<PaletteActivated> for CommandPalette {}
impl EventEmitter<PaletteClosed> for CommandPalette {}

pub struct CommandPalette {
    input: Entity<TextInput>,
    query: String,
    query_revision: u64,
    scope: PaletteScope,
    actions: Vec<PaletteAction>,
    results: PaletteSearch,
    selected: usize,
    loading: bool,
    error: Option<PaletteError>,
    draft: presentation::SharedDraft,
    projection: ProjectionId,
    presentation: Option<presentation::Presentation>,
}

impl CommandPalette {
    pub fn new(actions: Vec<PaletteAction>, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| {
            TextInput::new(cx, "", false).with_placeholder_key("ui.search_actions_tasks_or_files")
        });
        Self {
            input,
            query: String::new(),
            query_revision: 0,
            scope: PaletteScope::All,
            actions,
            results: PaletteSearch::default(),
            selected: 0,
            loading: true,
            error: None,
            draft: presentation::SharedDraft::new(),
            projection: ProjectionId::new(),
            presentation: None,
        }
    }

    pub fn apply(&mut self, result: Result<PaletteSearch, PaletteError>, cx: &mut Context<Self>) {
        let shared = self.draft.clone();
        let Some(draft) = shared.locked() else {
            return;
        };
        if !draft.committed(&self.query, self.query_revision) {
            drop(draft);
            self.sync_query(cx);
            return;
        }
        self.loading = false;
        self.error = result.as_ref().err().copied();
        if let Ok(results) = result {
            self.results = results;
        }
        self.selected = 0;
        self.projection = ProjectionId::new();
        cx.notify();
    }

    pub fn begin_activation(&mut self, cx: &mut Context<Self>) {
        self.loading = true;
        self.error = None;
        self.projection = ProjectionId::new();
        cx.notify();
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    fn sync_query(&mut self, cx: &mut Context<Self>) {
        let shared = self.draft.clone();
        let Some(draft) = shared.locked() else {
            return;
        };
        if draft.composing {
            cx.notify();
            return;
        }
        if self.input.read(cx).text() != draft.text {
            self.input
                .update(cx, |input, cx| input.set_text(&draft.text, cx));
        }
        if self.query != draft.text || self.query_revision != draft.revision {
            self.query = draft.text.clone();
            self.query_revision = draft.revision;
            self.results = PaletteSearch::default();
            self.loading = true;
            self.selected = 0;
            self.projection = ProjectionId::new();
            cx.emit(PaletteQueryChanged(draft.text.clone()));
        }
        cx.notify();
    }

    fn entries(&self) -> Vec<(String, String, PaletteTarget)> {
        let mut result = Vec::new();
        if matches!(self.scope, PaletteScope::All | PaletteScope::Actions) {
            let needle = self.query.to_lowercase();
            for action in &self.actions {
                let (label, shortcut, aliases) = action_label(*action);
                if needle.is_empty()
                    || format!("{label} {aliases}")
                        .to_lowercase()
                        .contains(&needle)
                {
                    result.push((
                        label.into(),
                        shortcut.into(),
                        PaletteTarget::Action(*action),
                    ));
                }
            }
        }
        if matches!(self.scope, PaletteScope::All | PaletteScope::Tasks) {
            result.extend(self.results.tasks.iter().take(30).map(|task| {
                (
                    if task.title.is_empty() {
                        vega_i18n::t("ui.untitled_task").into()
                    } else {
                        task.title.clone()
                    },
                    task.project_name.clone(),
                    PaletteTarget::Task(task.clone()),
                )
            }));
        }
        if matches!(self.scope, PaletteScope::All | PaletteScope::Files) {
            result.extend(self.results.files.iter().take(30).map(|path| {
                (
                    path.clone(),
                    vega_i18n::t("ui.file").into(),
                    PaletteTarget::File(path.clone()),
                )
            }));
        }
        result
    }
}

pub fn action_label(action: PaletteAction) -> (&'static str, &'static str, &'static str) {
    match action {
        PaletteAction::NewTask => (vega_i18n::t("ui.new_task"), "⌘N", "new task"),
        PaletteAction::OpenWorkspace => (
            vega_i18n::t("ui.open_workspace"),
            "⌘O",
            "open workspace project",
        ),
        PaletteAction::Settings => (vega_i18n::t("ui.open_settings"), "⌘,", "settings"),
        PaletteAction::ToggleSidebar => (vega_i18n::t("ui.toggle_sidebar"), "⌘B", "sidebar"),
        PaletteAction::Terminal => (vega_i18n::t("ui.toggle_terminal"), "⌘J", "terminal"),
        PaletteAction::Preview => (vega_i18n::t("ui.open_preview"), "", "preview"),
        PaletteAction::Review => (vega_i18n::t("ui.open_review"), "", "review diff"),
    }
}
impl Focusable for CommandPalette {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.presentation
            .as_ref()
            .map(|presentation| presentation.focus_handle(cx))
            .unwrap_or_else(|| self.input.read(cx).focus_handle(cx))
    }
}

impl Render for CommandPalette {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.render_nsh(window, cx)
    }
}
