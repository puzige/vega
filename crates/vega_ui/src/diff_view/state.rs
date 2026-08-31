use super::*;

impl DiffView {
    pub fn new(thread_id: String, project_id: String, cx: &mut Context<Self>) -> Self {
        Self {
            thread_id,
            project_id,
            layout: DiffLayout::Unified,
            snapshot: None,
            expanded_file: None,
            prepared_projection: None,
            pending_projection: None,
            refresh_error: None,
            projection_error: None,
            refreshing: false,
            rows: Vec::new(),
            hunk_indexes: Vec::new(),
            current_hunk: None,
            focus: cx.focus_handle(),
            scroll: UniformListScrollHandle::new(),
        }
    }

    pub fn thread_id(&self) -> &str {
        &self.thread_id
    }

    pub fn project_id(&self) -> &str {
        &self.project_id
    }

    pub fn generation(&self) -> Option<u64> {
        self.snapshot.as_ref().map(|snapshot| snapshot.generation)
    }

    pub fn layout(&self) -> DiffLayout {
        self.layout
    }

    pub fn expanded_file(&self) -> Option<WorkspaceFileId> {
        self.expanded_file
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    pub fn is_refreshing(&self) -> bool {
        self.refreshing
    }

    pub fn refresh_error(&self) -> Option<GitWorkspaceErrorCode> {
        self.refresh_error
    }

    pub fn projection_error(&self) -> Option<GitWorkspaceErrorCode> {
        self.projection_error.map(|(_, code)| code)
    }

    /// Reconciles a fresh safe snapshot and returns at most one lazy request.
    pub fn apply_snapshot(&mut self, snapshot: WorkspaceSnapshot, cx: &mut Context<Self>) {
        let old_generation = self.generation();
        let old_expanded = self.expanded_file;
        let ids: Vec<_> = snapshot.files.iter().map(|file| file.id).collect();
        let expanded = reconcile_expanded(old_expanded, &ids);
        let preserve =
            should_preserve_projection(old_generation, snapshot.generation, old_expanded, expanded);

        if !preserve {
            self.prepared_projection = None;
            self.pending_projection = None;
            self.projection_error = None;
            self.current_hunk = None;
        }
        self.snapshot = Some(snapshot);
        self.expanded_file = expanded;
        self.refresh_error = None;
        self.rebuild_rows();
        if let Some(request) = self.request_missing_projection() {
            cx.emit(request);
        }
        cx.notify();
    }

    /// Applies only the exact current expanded file projection.
    pub fn apply_projection(
        &mut self,
        projection: DiffTextProjection,
        cx: &mut Context<Self>,
    ) -> bool {
        let file_id = projection.file_id();
        let is_current = self.snapshot.as_ref().is_some_and(|snapshot| {
            exact_current_file(
                self.expanded_file,
                snapshot.files.iter().map(|file| file.id),
                file_id,
            )
        });
        if !is_current {
            return false;
        }
        self.prepared_projection = Some(prepare_projection(&projection));
        self.pending_projection = None;
        self.projection_error = None;
        self.current_hunk = None;
        self.rebuild_rows();
        cx.notify();
        true
    }

    /// Invalidates every capability after a latest refresh failure.
    pub fn apply_refresh_error(&mut self, code: GitWorkspaceErrorCode, cx: &mut Context<Self>) {
        self.snapshot = None;
        self.expanded_file = None;
        self.prepared_projection = None;
        self.pending_projection = None;
        self.projection_error = None;
        self.refresh_error = Some(code);
        self.rows.clear();
        self.hunk_indexes.clear();
        self.current_hunk = None;
        cx.notify();
    }

    /// Applies an inline error only to the exact current accordion body.
    pub fn apply_projection_error(
        &mut self,
        file_id: WorkspaceFileId,
        code: GitWorkspaceErrorCode,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.expanded_file != Some(file_id) {
            return false;
        }
        self.prepared_projection = None;
        self.pending_projection = None;
        self.projection_error = Some((file_id, code));
        self.rebuild_rows();
        cx.notify();
        true
    }

    pub fn set_refreshing(&mut self, refreshing: bool, cx: &mut Context<Self>) {
        self.refreshing = refreshing;
        cx.notify();
    }

    /// Enforces the single-open accordion invariant.
    pub(crate) fn toggle_file(&mut self, file_id: WorkspaceFileId, cx: &mut Context<Self>) {
        let current = self
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.files.iter().any(|file| file.id == file_id));
        if !current {
            return;
        }
        self.expanded_file = (self.expanded_file != Some(file_id)).then_some(file_id);
        self.prepared_projection = None;
        self.pending_projection = None;
        self.projection_error = None;
        self.current_hunk = None;
        self.rebuild_rows();
        if let Some(request) = self.request_missing_projection() {
            cx.emit(request);
        }
        cx.notify();
    }

    pub(crate) fn toggle_layout(&mut self, cx: &mut Context<Self>) {
        self.layout = match self.layout {
            DiffLayout::Unified => DiffLayout::SideBySide,
            DiffLayout::SideBySide => DiffLayout::Unified,
        };
        self.current_hunk = None;
        self.rebuild_rows();
        cx.notify();
    }

    pub(crate) fn next_hunk(&mut self, cx: &mut Context<Self>) -> Option<usize> {
        let row = navigate_hunk(&self.hunk_indexes, &mut self.current_hunk, true);
        if let Some(row) = row {
            self.scroll.scroll_to_item(row, ScrollStrategy::Nearest);
            cx.notify();
        }
        row
    }

    pub(crate) fn previous_hunk(&mut self, cx: &mut Context<Self>) -> Option<usize> {
        let row = navigate_hunk(&self.hunk_indexes, &mut self.current_hunk, false);
        if let Some(row) = row {
            self.scroll.scroll_to_item(row, ScrollStrategy::Nearest);
            cx.notify();
        }
        row
    }

    pub(crate) fn request_missing_projection(&mut self) -> Option<DiffProjectionRequested> {
        let snapshot = self.snapshot.as_ref()?;
        let file_id = self.expanded_file?;
        if self.prepared_projection.is_some()
            || self.projection_error.is_some()
            || self.pending_projection == Some(file_id)
        {
            return None;
        }
        self.pending_projection = Some(file_id);
        Some(DiffProjectionRequested {
            thread_id: self.thread_id.clone(),
            project_id: self.project_id.clone(),
            generation: snapshot.generation,
            file_id,
        })
    }

    pub(crate) fn rebuild_rows(&mut self) {
        self.rows.clear();
        self.hunk_indexes.clear();
        let Some(snapshot) = self.snapshot.as_ref() else {
            return;
        };
        for file in &snapshot.files {
            let expanded = self.expanded_file == Some(file.id);
            self.rows.push(PreparedRow::File {
                id: file.id,
                label: file.label.clone(),
                summary: file_summary(file),
                expanded,
            });
            if !expanded {
                continue;
            }
            if let Some((error_file, code)) = self.projection_error
                && error_file == file.id
            {
                self.rows.push(PreparedRow::ProjectionError {
                    id: file.id,
                    text: error_label(code).to_owned(),
                });
                continue;
            }
            let Some(projection) = self
                .prepared_projection
                .as_ref()
                .filter(|item| item.file_id == file.id)
            else {
                self.rows.push(PreparedRow::Message {
                    text: "Loading diff…".to_owned(),
                    danger: false,
                });
                continue;
            };
            let base = self.rows.len();
            let prepared = layout_projection_rows(projection, self.layout);
            self.hunk_indexes
                .extend(prepared.hunk_indexes.into_iter().map(|index| base + index));
            self.rows.extend(prepared.rows);
        }
    }

    pub(crate) fn retry_clicked(
        &mut self,
        _: &MouseUpEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.refreshing {
            return;
        }
        cx.emit(DiffRetryRequested {
            thread_id: self.thread_id.clone(),
            project_id: self.project_id.clone(),
        });
    }

    pub(crate) fn file_clicked(&mut self, file_id: WorkspaceFileId, cx: &mut Context<Self>) {
        self.toggle_file(file_id, cx);
    }

    pub(crate) fn retry_projection(&mut self, file_id: WorkspaceFileId, cx: &mut Context<Self>) {
        if self.projection_error.map(|(id, _)| id) != Some(file_id)
            || self.expanded_file != Some(file_id)
        {
            return;
        }
        self.projection_error = None;
        self.rebuild_rows();
        if let Some(request) = self.request_missing_projection() {
            cx.emit(request);
        }
        cx.notify();
    }

    pub(crate) fn back_clicked(
        &mut self,
        _: &MouseUpEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.emit(DiffClosed {
            thread_id: self.thread_id.clone(),
            project_id: self.project_id.clone(),
        });
    }

    pub(crate) fn close_action(&mut self, _: &CloseDiff, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(DiffClosed {
            thread_id: self.thread_id.clone(),
            project_id: self.project_id.clone(),
        });
    }

    pub(crate) fn previous_action(
        &mut self,
        _: &PreviousDiffHunk,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.previous_hunk(cx);
    }

    pub(crate) fn next_action(&mut self, _: &NextDiffHunk, _: &mut Window, cx: &mut Context<Self>) {
        self.next_hunk(cx);
    }

    pub(crate) fn toggle_layout_action(
        &mut self,
        _: &ToggleDiffLayout,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_layout(cx);
    }
}
