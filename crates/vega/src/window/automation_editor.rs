use super::*;
use vega_ui::automation_editor::{
    AutomationEditSubmission, AutomationEditor, AutomationEditorEvent, AutomationEditorKey,
};

impl VegaWindow {
    pub(super) fn open_automation_editor(
        &mut self,
        stream: Entity<ConversationStream>,
        request: &AutomationEditorRequested,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.owns_stream_request(&stream, &request.thread_id, cx) {
            return;
        }
        let Some(task) = stream
            .read(cx)
            .automation_projection()
            .tasks
            .iter()
            .find(|task| task.identity == request.identity)
            .cloned()
        else {
            return;
        };
        self.open_automation_task(task, window, cx);
    }

    fn open_automation_task(
        &mut self,
        task: AutomationTask,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = AutomationEditorKey::for_task(&task);
        if !self.automation_editors.contains_key(&key) {
            let view = cx.new(|cx| AutomationEditor::new(task, cx));
            cx.subscribe_in(
                &view,
                window,
                |this, view, event: &AutomationEditorEvent, window, cx| {
                    match event {
                        AutomationEditorEvent::Save(submitted) => {
                            let stream = this
                                .automation_controller
                                .streams
                                .get(&submitted.key.thread_id)
                                .cloned();
                            if let Some(stream) = stream {
                                this.change_automation(
                                    stream,
                                    &submitted.key.thread_id,
                                    submitted.command(),
                                    None,
                                    Some((view.clone(), submitted.clone())),
                                    cx,
                                );
                            } else {
                                view.update(cx, |view, cx| {
                                    view.finish_save_localized(
                                        submitted,
                                        Err(vega_ui::localized_copy::LocalizedCopy::new(
                                            "app.error.session_unavailable",
                                        )),
                                        cx,
                                    )
                                });
                            }
                        }
                        AutomationEditorEvent::Changed => {
                            this.sync_automation_editor_visibility(window, cx)
                        }
                        AutomationEditorEvent::Close => {
                            this.close_automation_workspace(&view.read(cx).key(), window, cx)
                        }
                        AutomationEditorEvent::OpenLatest(task) => {
                            this.close_automation_workspace(&view.read(cx).key(), window, cx);
                            this.open_automation_task(task.clone(), window, cx)
                        }
                    }
                    cx.notify();
                },
            )
            .detach();
            self.automation_editors.insert(key.clone(), view);
        }
        self.show_automation_workspace(key, window, cx);
    }

    pub(super) fn sync_automation_editor_visibility(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let keys = self.visible_automation_editors(window, cx);
        for (thread, stream) in &self.automation_controller.streams {
            let dirty = keys.iter().any(|key| {
                key.thread_id == *thread
                    && self
                        .automation_editors
                        .get(*key)
                        .is_some_and(|view| view.read(cx).is_dirty(cx))
            });
            stream.update(cx, |stream, cx| {
                stream.set_automation_editor_dirty(dirty, cx)
            });
        }
    }

    pub(super) fn refresh_automation_editors(
        &self,
        thread: &str,
        tasks: &[AutomationTask],
        timings: &std::collections::HashMap<String, AutomationTiming>,
        cx: &mut Context<Self>,
    ) {
        for (key, view) in &self.automation_editors {
            if key.thread_id == thread {
                view.update(cx, |view, cx| {
                    view.refresh(
                        tasks,
                        timings.get(&key.identity).map(|time| time.elapsed_ms),
                        cx,
                    )
                });
            }
        }
    }

    pub(super) fn automation_edit_error(
        &self,
        stream: &Entity<ConversationStream>,
        editor: Option<&(Entity<AutomationEditor>, AutomationEditSubmission)>,
        error: String,
        cx: &mut Context<Self>,
    ) {
        if let Some((view, submitted)) = editor {
            view.update(cx, |view, cx| {
                view.finish_save_localized(
                    submitted,
                    Err(crate::app_localization::localized_error(&error)),
                    cx,
                )
            });
        } else {
            self.automation_error(stream, error, cx);
        }
    }
}
