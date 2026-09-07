use super::*;

use gpui::Focusable;

impl ConversationStream {
    /// Applies a bounded `@file` worker result only for the current request
    /// generation. A result never reopens a selector that the user closed.
    pub fn apply_file_index_result(
        &mut self,
        generation: u64,
        result: Result<FileIndexSnapshot, FileIndexFailureCode>,
        cx: &mut Context<Self>,
    ) -> bool {
        if generation != self.file_index_generation || !self.file_index_loading {
            return false;
        }
        self.file_index_loading = false;
        match result {
            Ok(snapshot) => {
                self.file_snapshot = snapshot;
                self.file_index_loaded = true;
                self.file_index_failure = None;
                let query = self
                    .input
                    .read(cx)
                    .trailing_at_query()
                    .map(|(_, query)| query);
                if self.file_selector_wanted
                    && let Some(query) = query
                {
                    self.file_selector.open_for(&self.file_snapshot, &query);
                }
            }
            Err(code) => {
                self.file_index_loaded = false;
                self.file_index_failure = Some(code);
                self.file_selector.close();
            }
        }
        cx.notify();
        true
    }

    /// Compatibility seam for headless callers that already own the current
    /// generation. Production worker delivery uses the result/fence method.
    pub fn apply_file_index(&mut self, snapshot: FileIndexSnapshot, cx: &mut Context<Self>) {
        self.file_index_loading = true;
        self.apply_file_index_result(self.file_index_generation, Ok(snapshot), cx);
    }

    pub(crate) fn next_file_index_generation(&mut self) -> Option<u64> {
        let generation = self.file_index_generation.checked_add(1)?;
        self.file_index_generation = generation;
        Some(generation)
    }

    /// Invalidates a hidden stream's index owner when its route is replaced.
    /// The app controller cancels the worker separately; this method only
    /// fences UI state and clears route-scoped cache.
    pub fn invalidate_file_index(&mut self, cx: &mut Context<Self>) {
        let _ = self.next_file_index_generation();
        self.file_index_loading = false;
        self.file_index_loaded = false;
        self.file_index_failure = None;
        self.file_snapshot = FileIndexSnapshot::default();
        self.file_selector_wanted = false;
        self.file_selector.close();
        cx.notify();
    }

    /// Content-free state seam used by the application integration harness;
    /// the selector remains the sole owner of candidate rows.
    #[doc(hidden)]
    pub fn file_index_loaded(&self) -> bool {
        self.file_index_loaded
    }

    #[doc(hidden)]
    pub fn file_index_loading(&self) -> bool {
        self.file_index_loading
    }

    #[doc(hidden)]
    pub fn file_index_candidates(&self) -> Vec<String> {
        self.file_selector.candidates().to_vec()
    }

    #[doc(hidden)]
    pub fn composer_input(&self) -> Entity<TextInput> {
        self.input.clone()
    }

    #[doc(hidden)]
    pub fn controller_error_message(&self) -> Option<String> {
        self.controller_error.clone()
    }

    #[doc(hidden)]
    pub fn composer_submission_pending(&self) -> bool {
        self.composer_submit_pending
    }

    #[doc(hidden)]
    pub fn accept_file_candidate(&mut self, index: usize, cx: &mut Context<Self>) {
        self.on_selector_click(index, cx);
    }

    pub(crate) fn close_file_selector_and_cancel(&mut self, cx: &mut Context<Self>) {
        let should_cancel = self.file_index_loading;
        self.file_selector_wanted = false;
        self.file_selector.close();
        // Failed is also a visible selector state. Clear it on every close so
        // the hidden composer cannot retain a FileSelect key scope.
        self.file_index_failure = None;
        if should_cancel {
            let generation = self.file_index_generation;
            let _ = self.next_file_index_generation();
            self.file_index_loading = false;
            self.file_index_loaded = false;
            self.file_snapshot = FileIndexSnapshot::default();
            cx.emit(FileIndexCancelled {
                thread_id: self.thread.id.clone(),
                project_id: self.thread.project_id.clone(),
                generation,
            });
        }
        cx.notify();
    }

    /// Follows the caret into/out of an `@token` (A2-12). The first `@` in a
    /// session triggers exactly one bounded index request; later opens
    /// re-filter a complete accepted snapshot locally. No token closes and
    /// fences the selector without allowing a late result to reopen it.
    pub(crate) fn sync_at_query(&mut self, input: &Entity<TextInput>, cx: &mut Context<Self>) {
        let query = input.read(cx).trailing_at_query().map(|(_, query)| query);
        match query {
            None => {
                let should_cancel = self.file_index_loading;
                self.file_selector_wanted = false;
                self.file_selector.close();
                self.file_index_failure = None;
                if should_cancel {
                    let generation = self.file_index_generation;
                    let _ = self.next_file_index_generation();
                    self.file_index_loading = false;
                    self.file_index_loaded = false;
                    self.file_snapshot = FileIndexSnapshot::default();
                    cx.emit(FileIndexCancelled {
                        thread_id: self.thread.id.clone(),
                        project_id: self.thread.project_id.clone(),
                        generation,
                    });
                }
                cx.notify();
            }
            Some(query) => {
                self.file_selector_wanted = true;
                if !self.file_index_loaded
                    && !self.file_index_loading
                    && self.file_index_failure.is_none()
                {
                    let Some(generation) = self.next_file_index_generation() else {
                        self.file_index_loading = false;
                        self.file_index_loaded = false;
                        self.file_index_failure = Some(FileIndexFailureCode::Traversal);
                        self.file_selector.close();
                        cx.notify();
                        return;
                    };
                    self.file_index_loading = true;
                    self.file_selector.close();
                    cx.emit(FileIndexRequested {
                        thread_id: self.thread.id.clone(),
                        project_id: self.thread.project_id.clone(),
                        generation,
                    });
                }
                if self.file_index_loaded && self.file_index_failure.is_none() {
                    self.file_selector.open_for(&self.file_snapshot, &query);
                } else {
                    self.file_selector.close();
                }
                cx.notify();
            }
        }
    }

    /// `up` in an open selector moves the highlight instead of history
    /// recall (ui-spec §6 键盘可达；选择器打开时按键先到选择器).
    pub(crate) fn on_selector_previous(
        &mut self,
        _: &PreviousFile,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.file_selector.move_highlight(-1);
        cx.notify();
    }

    pub(crate) fn on_selector_next(
        &mut self,
        _: &NextFile,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.file_selector.move_highlight(1);
        cx.notify();
    }

    pub(crate) fn on_selector_cancel(
        &mut self,
        _: &CancelFile,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_file_selector_and_cancel(cx);
        self.focus_composer(window, cx);
    }

    /// Retry button for a failed bounded index. The input token remains
    /// editable while the previous generation is fenced and replaced.
    pub(crate) fn on_selector_retry(
        &mut self,
        _: &MouseUpEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.retry_file_index(window, cx);
    }

    pub(crate) fn on_selector_retry_action(
        &mut self,
        _: &RetryFile,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.retry_file_index(window, cx);
    }

    pub(crate) fn on_selector_retry_focus(
        &mut self,
        _: &FocusRetry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.file_selector_wanted && self.file_index_failure.is_some() {
            if self.file_retry_focus.is_focused(window) {
                window.focus_next(cx);
            } else {
                window.focus(&self.file_retry_focus, cx);
            }
        }
    }

    pub(crate) fn on_selector_retry_focus_previous(
        &mut self,
        _: &FocusPreviousRetry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.file_selector_wanted && self.file_index_failure.is_some() {
            window.focus_prev(cx);
        }
    }

    fn retry_file_index(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((_, _query)) = self.input.read(cx).trailing_at_query() else {
            self.close_file_selector_and_cancel(cx);
            return;
        };
        self.file_selector_wanted = true;
        self.file_selector.close();
        self.file_index_loaded = false;
        self.file_index_loading = true;
        self.file_index_failure = None;
        self.file_snapshot = FileIndexSnapshot::default();
        let Some(generation) = self.next_file_index_generation() else {
            self.file_index_loading = false;
            self.file_index_failure = Some(FileIndexFailureCode::Traversal);
            cx.notify();
            return;
        };
        cx.emit(FileIndexRetryRequested {
            thread_id: self.thread.id.clone(),
            project_id: self.thread.project_id.clone(),
            generation,
        });
        self.focus_composer(window, cx);
        cx.notify();
    }

    /// Restores composer focus after returning from another application route.
    pub fn focus_composer(&self, window: &mut Window, cx: &mut Context<Self>) {
        let input_focus = self.input.read(cx).focus_handle(cx);
        window.focus(&input_focus, cx);
    }

    /// Enter/Tab in an open selector accepts first-wins and completes the
    /// `@token` in the composer input; the send binding never fires through
    /// this path (the selector context shadows it while open).
    pub(crate) fn on_selector_accept(
        &mut self,
        _: &AcceptFile,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(path) = self.file_selector.accept() {
            self.file_selector_wanted = false;
            self.input
                .update(cx, |input, cx| input.complete_at_query(&path, cx));
        }
        cx.notify();
    }

    pub(crate) fn on_selector_click(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(path) = self.file_selector.accept_at(index) {
            self.file_selector_wanted = false;
            self.input
                .update(cx, |input, cx| input.complete_at_query(&path, cx));
        }
        cx.notify();
    }
}
