use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, Context, Entity, EventEmitter, FocusHandle, Focusable, Subscription, Window,
    actions,
};
use std::collections::{HashMap, HashSet};
use vega_conversation::types::{
    ArtifactCard as ArtifactProjection, ArtifactCardId, ArtifactPreviewProjection,
    GitWorkspaceErrorCode, OpenInOutcome, OpenInTarget, WorkspaceFileId,
};
use vega_theme::Layout;

use crate::conversation_stream::OpenWorkspaceDiffRequested;
use crate::nsh::ProjectionId;

mod host;
mod nsh_view;
#[cfg(test)]
mod tests;
actions!(vega_artifact_card, [ArtifactActivate, ArtifactClear]);

const OPEN_TARGETS: [(OpenInTarget, &str); 6] = [
    (OpenInTarget::VisualStudioCode, "VS Code"),
    (OpenInTarget::Cursor, "Cursor"),
    (OpenInTarget::Zed, "Zed"),
    (OpenInTarget::Terminal, "Terminal"),
    (OpenInTarget::DefaultApplication, "Default"),
    (OpenInTarget::RevealInFinder, "Finder"),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactPreviewRequested {
    pub thread_id: String,
    pub project_id: String,
    pub card_id: ArtifactCardId,
    pub file_id: WorkspaceFileId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactOpenRequested {
    pub thread_id: String,
    pub project_id: String,
    pub card_id: ArtifactCardId,
    pub file_id: WorkspaceFileId,
    pub target: OpenInTarget,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactCleared {
    pub thread_id: String,
    pub project_id: String,
    pub card_id: ArtifactCardId,
}

pub struct ArtifactCard {
    reading_preview: bool,
    thread_id: String,
    project_id: String,
    projection: ArtifactProjection,
    preview_lines: Vec<String>,
    inline_error: Option<GitWorkspaceErrorCode>,
    opening: Option<OpenInTarget>,
    focus: FocusHandle,
    revision: ProjectionId,
    presentations: HashMap<nsh_view::Mode, host::Presentation>,
    menu_open: bool,
}

impl EventEmitter<ArtifactPreviewRequested> for ArtifactCard {}
impl EventEmitter<ArtifactOpenRequested> for ArtifactCard {}
impl EventEmitter<ArtifactCleared> for ArtifactCard {}

impl Focusable for ArtifactCard {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ArtifactCard {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        host::render(self, nsh_view::Mode::Compact, window, cx)
    }
}

impl ArtifactCard {
    pub(crate) fn set_reading_preview(&mut self, reading: bool, cx: &mut Context<Self>) {
        if self.reading_preview != reading {
            self.reading_preview = reading;
            self.menu_open = false;
            self.revision = ProjectionId::new();
            cx.notify();
        }
    }

    pub fn new(
        thread_id: String,
        project_id: String,
        projection: ArtifactProjection,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            reading_preview: false,
            thread_id,
            project_id,
            projection,
            preview_lines: Vec::new(),
            inline_error: None,
            opening: None,
            revision: ProjectionId::new(),
            presentations: HashMap::new(),
            menu_open: false,
            focus: cx.focus_handle().tab_stop(true),
        }
    }

    pub fn id(&self) -> ArtifactCardId {
        self.projection.id
    }

    pub fn projection(&self) -> &ArtifactProjection {
        &self.projection
    }

    pub fn row_count(&self) -> usize {
        2 + self.preview_lines.len() + usize::from(self.inline_error.is_some())
    }

    pub fn apply_metadata(
        &mut self,
        projection: ArtifactProjection,
        cx: &mut Context<Self>,
    ) -> bool {
        if projection.id != self.projection.id {
            return false;
        }
        let capability_changed = projection.current_file_id != self.projection.current_file_id
            || !projection.preview_available;
        self.projection = projection;
        if capability_changed {
            self.preview_lines.clear();
            self.inline_error = None;
            self.opening = None;
        }
        self.revision = ProjectionId::new();
        cx.notify();
        true
    }

    pub fn apply_preview(
        &mut self,
        preview: ArtifactPreviewProjection,
        cx: &mut Context<Self>,
    ) -> bool {
        if preview.card_id() != self.projection.id
            || self.projection.current_file_id != Some(preview.file_id())
            || !self.projection.preview_available
        {
            return false;
        }
        self.preview_lines = preview_rows(preview.text());
        self.inline_error = None;
        self.revision = ProjectionId::new();
        cx.notify();
        true
    }

    pub fn apply_preview_error(
        &mut self,
        card_id: ArtifactCardId,
        file_id: WorkspaceFileId,
        code: GitWorkspaceErrorCode,
        cx: &mut Context<Self>,
    ) -> bool {
        if card_id != self.projection.id || self.projection.current_file_id != Some(file_id) {
            return false;
        }
        self.preview_lines.clear();
        self.inline_error = Some(code);
        self.revision = ProjectionId::new();
        cx.notify();
        true
    }

    pub fn set_opening(&mut self, target: Option<OpenInTarget>, cx: &mut Context<Self>) {
        self.opening = target;
        if target.is_some() {
            self.inline_error = None;
        }
        self.revision = ProjectionId::new();
        cx.notify();
    }

    pub fn apply_open_outcome(&mut self, outcome: OpenInOutcome, cx: &mut Context<Self>) -> bool {
        if outcome.card_id != self.projection.id || self.opening != Some(outcome.target) {
            return false;
        }
        self.opening = None;
        self.inline_error = None;
        self.revision = ProjectionId::new();
        cx.notify();
        true
    }

    pub fn apply_open_error(
        &mut self,
        card_id: ArtifactCardId,
        target: OpenInTarget,
        code: GitWorkspaceErrorCode,
        cx: &mut Context<Self>,
    ) -> bool {
        if card_id != self.projection.id || self.opening != Some(target) {
            return false;
        }
        self.opening = None;
        self.inline_error = Some(code);
        self.revision = ProjectionId::new();
        cx.notify();
        true
    }

    pub fn fail_corrupt(&mut self, cx: &mut Context<Self>) {
        self.preview_lines.clear();
        self.opening = None;
        self.inline_error = Some(GitWorkspaceErrorCode::ArtifactConflict);
        self.revision = ProjectionId::new();
        cx.notify();
    }

    pub fn fail_request(&mut self, code: GitWorkspaceErrorCode, cx: &mut Context<Self>) {
        self.preview_lines.clear();
        self.opening = None;
        self.inline_error = Some(code);
        self.revision = ProjectionId::new();
        cx.notify();
    }

    pub fn invalidate(&mut self, code: GitWorkspaceErrorCode, cx: &mut Context<Self>) {
        self.projection.current_file_id = None;
        self.projection.preview_available = false;
        self.fail_request(code, cx);
    }

    pub fn preview(&mut self, cx: &mut Context<Self>) {
        if !self.projection.preview_available || self.opening.is_some() {
            return;
        }
        let Some(file_id) = self.projection.current_file_id else {
            return;
        };
        self.inline_error = None;
        cx.emit(ArtifactPreviewRequested {
            thread_id: self.thread_id.clone(),
            project_id: self.project_id.clone(),
            card_id: self.projection.id,
            file_id,
        });
        self.revision = ProjectionId::new();
        cx.notify();
    }

    fn open(&mut self, target: OpenInTarget, cx: &mut Context<Self>) {
        if self.reading_preview || self.opening.is_some() {
            return;
        }
        let Some(file_id) = self.projection.current_file_id else {
            return;
        };
        self.menu_open = false;
        self.opening = Some(target);
        self.inline_error = None;
        cx.emit(ArtifactOpenRequested {
            thread_id: self.thread_id.clone(),
            project_id: self.project_id.clone(),
            card_id: self.projection.id,
            file_id,
            target,
        });
        self.revision = ProjectionId::new();
        cx.notify();
    }

    fn clear(&mut self, cx: &mut Context<Self>) {
        self.preview_lines.clear();
        self.inline_error = None;
        self.opening = None;
        cx.emit(ArtifactCleared {
            thread_id: self.thread_id.clone(),
            project_id: self.project_id.clone(),
            card_id: self.projection.id,
        });
        self.revision = ProjectionId::new();
        cx.notify();
    }

    pub fn render_row(
        card: Entity<Self>,
        row: usize,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        card.update(cx, |card, cx| {
            host::render(card, nsh_view::Mode::Row(row), window, cx)
        })
    }
}

struct ArtifactGroupFile {
    card: Entity<ArtifactCard>,
    capture_order: u64,
    _subscription: Subscription,
}

pub struct ArtifactGroup {
    message_id: String,
    files: Vec<ArtifactGroupFile>,
    capture_order: u64,
    expanded: bool,
    capture_errors: Vec<(String, GitWorkspaceErrorCode)>,
    file_errors: Vec<(Entity<ArtifactCard>, GitWorkspaceErrorCode)>,
    review_request: OpenWorkspaceDiffRequested,
    review_disabled_reason_key: Option<&'static str>,
    revision: ProjectionId,
    presentation: Option<host::Presentation>,
}

impl EventEmitter<OpenWorkspaceDiffRequested> for ArtifactGroup {}

impl ArtifactGroup {
    pub(crate) fn new(
        message_id: String,
        review_request: OpenWorkspaceDiffRequested,
        reading_preview: bool,
        standalone: bool,
    ) -> Self {
        Self {
            message_id,
            files: Vec::new(),
            capture_order: 0,
            expanded: false,
            capture_errors: Vec::new(),
            file_errors: Vec::new(),
            review_request,
            review_disabled_reason_key: review_disabled_reason_key(reading_preview, standalone),
            revision: ProjectionId::new(),
            presentation: None,
        }
    }

    pub fn message_id(&self) -> &str {
        &self.message_id
    }

    fn visible_files(&self) -> usize {
        if self.expanded {
            self.files.len()
        } else {
            self.files.len().min(Layout::ARTIFACT_VISIBLE_FILES)
        }
    }

    pub fn row_count(&self) -> usize {
        self.visible_files()
            + usize::from(self.files.len() > Layout::ARTIFACT_VISIBLE_FILES)
            + self.capture_errors.len()
            + self.file_errors.len()
    }

    pub(crate) fn estimated_height(&self) -> f32 {
        let rows = self.row_count();
        if rows == 0 {
            0.
        } else {
            Layout::ARTIFACT_BODY_GAP
                + Layout::ARTIFACT_HEADER_HEIGHT
                + rows as f32 * Layout::ARTIFACT_ROW_HEIGHT
                + self.visible_files().saturating_sub(1) as f32 * Layout::ARTIFACT_FILE_GAP
                + 2. * Layout::ARTIFACT_BORDER
        }
    }

    pub(crate) fn set_review_availability(
        &mut self,
        reading_preview: bool,
        standalone: bool,
        cx: &mut Context<Self>,
    ) {
        let reason = review_disabled_reason_key(reading_preview, standalone);
        if self.review_disabled_reason_key != reason {
            self.review_disabled_reason_key = reason;
            self.revision = ProjectionId::new();
            cx.notify();
        }
    }

    fn open_review(&mut self, cx: &mut Context<Self>) {
        if self.review_disabled_reason_key.is_none() && !self.files.is_empty() {
            cx.emit(self.review_request.clone());
        }
    }

    fn toggle_expanded(&mut self, cx: &mut Context<Self>) {
        self.expanded = !self.expanded;
        self.revision = ProjectionId::new();
        cx.notify();
    }

    fn sync_file(&mut self, card: Entity<ArtifactCard>, cx: &mut Context<Self>) {
        let mut indices = HashMap::<String, usize>::with_capacity(self.files.len());
        let mut files = Vec::<ArtifactGroupFile>::with_capacity(self.files.len());
        for file in self.files.drain(..) {
            let label = file.card.read(cx).projection.label.clone();
            if let Some(&index) = indices.get(&label) {
                if file.capture_order > files[index].capture_order {
                    files[index] = file;
                }
            } else {
                indices.insert(label, files.len());
                files.push(file);
            }
        }
        self.files = files;
        let live = self
            .files
            .iter()
            .map(|file| file.card.entity_id())
            .collect::<HashSet<_>>();
        self.file_errors
            .retain(|(card, _)| live.contains(&card.entity_id()));
        if !live.contains(&card.entity_id()) {
            self.revision = ProjectionId::new();
            cx.notify();
            return;
        }
        let error = card.read(cx).inline_error;
        if let Some(index) = self
            .file_errors
            .iter()
            .position(|(owned, _)| owned == &card)
        {
            if let Some(error) = error {
                self.file_errors[index].1 = error;
            } else {
                self.file_errors.remove(index);
            }
        } else if let Some(error) = error {
            self.file_errors.push((card, error));
        }
        self.revision = ProjectionId::new();
        cx.notify();
    }

    pub(crate) fn capture(
        &mut self,
        call_id: &str,
        card: Entity<ArtifactCard>,
        cx: &mut Context<Self>,
    ) {
        self.capture_order += 1;
        let subscription = cx.observe(&card, |this, card, cx| this.sync_file(card, cx));
        self.files.push(ArtifactGroupFile {
            card: card.clone(),
            capture_order: self.capture_order,
            _subscription: subscription,
        });
        self.capture_errors.retain(|(owner, _)| owner != call_id);
        self.sync_file(card, cx);
    }

    pub(crate) fn resolve_capture_error(&mut self, call_id: &str, cx: &mut Context<Self>) {
        let before = self.capture_errors.len();
        self.capture_errors.retain(|(owner, _)| owner != call_id);
        if before != self.capture_errors.len() {
            self.revision = ProjectionId::new();
            cx.notify();
        }
    }

    pub(crate) fn capture_error(
        &mut self,
        call_id: &str,
        code: GitWorkspaceErrorCode,
        cx: &mut Context<Self>,
    ) {
        if let Some((_, error)) = self
            .capture_errors
            .iter_mut()
            .find(|(owner, _)| owner == call_id)
        {
            *error = code;
        } else {
            self.capture_errors.push((call_id.to_owned(), code));
        }
        self.revision = ProjectionId::new();
        cx.notify();
    }

    pub(crate) fn render(group: Entity<Self>, window: &mut Window, cx: &mut App) -> AnyElement {
        group.update(cx, |group, cx| {
            host::render(group, nsh_view::Mode::Group, window, cx)
        })
    }
}

fn review_disabled_reason_key(reading_preview: bool, standalone: bool) -> Option<&'static str> {
    if reading_preview {
        Some("ui.artifact.review_reading_preview")
    } else if standalone {
        Some("ui.artifact.review_project_required")
    } else {
        None
    }
}

fn artifact_path(label: &str) -> (&str, &str) {
    label
        .rfind('/')
        .map_or(("", label), |index| label.split_at(index + 1))
}

fn error_label(code: Option<GitWorkspaceErrorCode>) -> String {
    code.map_or_else(String::new, |code| match code {
        GitWorkspaceErrorCode::GitUnavailable => {
            vega_i18n::t("ui.git_2_40_was_not_found_install_homebrew").to_owned()
        }
        GitWorkspaceErrorCode::GitUnsupported => {
            vega_i18n::t("ui.git_2_40_is_required_upgrade_git_and").to_owned()
        }
        GitWorkspaceErrorCode::GitExecutableChanged => {
            vega_i18n::t("ui.git_changed_while_vega_was_running_restart_vega").to_owned()
        }
        code => vega_i18n::t_args("ui.artifact_unavailable_code", &[("code", code.as_str())]),
    })
}

fn preview_rows(text: &str) -> Vec<String> {
    text.split_inclusive('\n')
        .map(|line| line.strip_suffix('\n').unwrap_or(line).to_owned())
        .collect()
}
