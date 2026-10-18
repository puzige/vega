use super::*;

#[derive(Default)]
pub(super) struct CompletionAttention {
    window: Option<AnyWindowHandle>,
    visible_thread: Option<String>,
    activation_subscription: Option<Subscription>,
}

impl VegaWindow {
    pub(super) fn sync_completion_attention(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.completion_attention.window = Some(window.window_handle());
        if self.completion_attention.activation_subscription.is_none() {
            self.completion_attention.activation_subscription =
                Some(cx.observe_window_activation(window, |this, window, cx| {
                    this.sync_completion_attention(window, cx);
                    cx.notify();
                }));
        }
        let visible = if window.is_window_active() && !cx.global::<SettingsOpen>().0 {
            cx.global::<OpenedThread>().0.clone().filter(|thread| {
                self.draft
                    .as_ref()
                    .is_none_or(|draft| draft.id != thread.id)
            })
        } else {
            None
        };
        let next = visible.as_ref().map(|thread| thread.id.clone());
        if self.completion_attention.visible_thread != next {
            self.completion_attention.visible_thread = next;
            if let Some(thread) = visible
                && thread.unread
            {
                self.record_navigation_visit(thread, cx);
            }
        }
    }

    fn completion_is_foreground(&self, thread_id: &str, cx: &App) -> bool {
        self.completion_attention.window.is_some()
            && self.completion_attention.window == cx.active_window()
            && !cx.global::<SettingsOpen>().0
            && cx
                .global::<OpenedThread>()
                .0
                .as_ref()
                .is_some_and(|thread| thread.id == thread_id)
            && self
                .stream_view
                .as_ref()
                .is_some_and(|(id, _)| id == thread_id)
    }

    pub(super) fn finish_completion_attention(
        &mut self,
        run: &ActiveAgentRun,
        success: bool,
        cx: &mut Context<Self>,
    ) {
        if run.delegated_parent_thread.is_some()
            || run.cancellation_reason == Some(RunCancellationReason::ReviewHandoff)
            || self.completion_is_foreground(&run.thread_id, cx)
        {
            return;
        }
        vega_ui::navigation::begin_task_mutation(cx);
        let result = (|| {
            let store = cx.global::<VegaStore>().0.as_ref().map_err(Clone::clone)?;
            vega_conversation::threads::update_thread(
                store,
                &run.thread_id,
                &ThreadUpdate {
                    unread: Some(true),
                    ..Default::default()
                },
            )
            .map_err(|error| error.to_string())?;
            vega_conversation::threads::get_thread(store, &run.thread_id)
                .map_err(|error| error.to_string())
        })();
        vega_ui::navigation::finish_task_mutation(cx);
        let thread = match result {
            Ok(thread) => thread,
            Err(_) => {
                Self::completion_navigation_error("未读状态保存失败，请重新打开任务后重试。", cx);
                return;
            }
        };
        if let Some(mut current) = cx.global::<OpenedThread>().0.clone()
            && current.id == thread.id
            && current.project_id == thread.project_id
        {
            current.unread = true;
            cx.set_global(OpenedThread(Some(current)));
        }
        self.sidebar.update(cx, Sidebar::reload_sessions);
        if run.cancellation_reason == Some(RunCancellationReason::User) {
            return;
        }
        let Some(output) = run
            .stream
            .read(cx)
            .completion_response(&run.message_ids, cx)
        else {
            return;
        };
        let outcome = if success {
            run.completion_outcome
                .unwrap_or(RunCompletionOutcome::Completed)
        } else {
            match run.completion_outcome {
                Some(RunCompletionOutcome::Interrupted) => RunCompletionOutcome::Interrupted,
                _ => RunCompletionOutcome::Failed,
            }
        };
        crate::app_notifications::show(
            NavigationRoute::Task {
                project: thread.project_binding().map(str::to_owned),
                task: thread.id.clone(),
            },
            &thread.title,
            run.generation,
            outcome,
            &output,
            cx,
        );
    }

    pub(crate) fn open_completion_notification(
        &mut self,
        tag: &str,
        route: &NavigationRoute,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if cx
            .try_global::<vega_ui::navigation::TaskMutationState>()
            .is_some_and(|state| state.pending > 0)
        {
            Self::completion_navigation_error("任务操作尚未完成，请稍后重试导航。", cx);
            return;
        }
        let Some(database) = self.file_backed_store_path(cx) else {
            Self::completion_navigation_error("任务暂时无法打开，请稍后重试。", cx);
            return;
        };
        let resolved =
            vega_conversation::navigation::NavigationService::new(database).resolve(route);
        match resolved {
            Ok(Some(thread)) => {
                if self.accept_palette_thread(thread, cx) {
                    cx.activate(true);
                    window.activate_window();
                    crate::app_notifications::dismiss(tag, cx);
                }
            }
            Err(NavigationError::Unavailable) => {
                Self::completion_navigation_error("任务暂时无法打开，请稍后重试。", cx);
            }
            Ok(None) | Err(NavigationError::InvalidRoute) => {}
        }
    }

    fn completion_navigation_error(message: &'static str, cx: &mut Context<Self>) {
        let mut state = cx
            .try_global::<vega_ui::navigation::NavigationState>()
            .cloned()
            .unwrap_or_default();
        state.error = Some(message);
        cx.set_global(state);
        cx.refresh_windows();
    }
}
