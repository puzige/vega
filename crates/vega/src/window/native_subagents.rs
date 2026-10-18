use super::*;
use crate::native_subagents::NativeSubagentUpdate;
use vega_conversation::types::{
    SubagentAdmission, SubagentOpenRequested, SubagentState, SubagentStopRequested,
};

impl VegaWindow {
    pub(crate) fn apply_subagent_update(
        &mut self,
        parent_id: &str,
        parent_stream: &Entity<ConversationStream>,
        update: NativeSubagentUpdate,
        cx: &mut Context<Self>,
    ) {
        match update {
            NativeSubagentUpdate::Admit { admission, reply } => {
                let valid = !admission.cancel.is_cancelled()
                    && admission.snapshot.parent_thread_id == parent_id
                    && self
                        .agent_controller
                        .active
                        .get(parent_id)
                        .is_some_and(|active| {
                            active.stream == *parent_stream
                                && !active.cancel.is_cancelled()
                                && active.root_message_id.as_deref()
                                    == Some(&admission.snapshot.parent_message_id)
                        });
                if !valid
                    || self
                        .agent_controller
                        .active
                        .contains_key(&admission.thread.id)
                    || thread_is_running(&admission.thread.id, cx)
                {
                    admission.cancel.cancel();
                    let _ = reply.send(Err(vega_runtime::VegaError::Cancelled));
                    return;
                }
                let SubagentAdmission {
                    thread,
                    snapshot,
                    cancel,
                    permission_queue,
                    user_questions,
                    task,
                    queue_generation,
                } = *admission;
                let thread_id = thread.id.clone();
                let view = cx.new(move |cx| {
                    ConversationStream::new_with_request_queues(
                        thread,
                        permission_queue,
                        user_questions,
                        cx,
                    )
                });
                let Some((generation, _)) =
                    self.agent_controller
                        .begin(thread_id.clone(), view.clone(), Some(task), None)
                else {
                    cancel.cancel();
                    let _ = reply.send(Err(vega_runtime::VegaError::Cancelled));
                    return;
                };
                if let Some(active) = self.agent_controller.active.get_mut(&thread_id) {
                    active.cancel = cancel.clone();
                    active.delegated_message_id = Some(snapshot.message_id.clone());
                    active.delegated_parent_thread = Some(parent_id.into());
                    active.queue_generation = queue_generation;
                }
                self.agent_controller
                    .delegated_unbound
                    .insert(thread_id.clone());
                self.agent_controller
                    .queue_streams
                    .insert(thread_id.clone(), view.clone());
                vega_ui::sidebar::set_thread_running(&thread_id, true, cx);
                crate::automation_controller::ensure_global(cx);
                cx.update_global::<crate::automation_controller::AutomationGlobal, _>(
                    |global, _| {
                        global.runs.insert(
                            thread_id.clone(),
                            crate::automation_controller::AutomationRunOwner {
                                started_at: None,
                                generation,
                                owner: Arc::downgrade(&self.window_alive),
                                claim: None,
                                cancel: cancel.clone(),
                            },
                        );
                    },
                );
                view.update(cx, ConversationStream::begin_composer_run);
                parent_stream.update(cx, |parent, cx| {
                    parent.apply_subagent(snapshot.clone(), cx);
                    parent.bind_subagent_stream(&snapshot.parent_message_id, &thread_id, view, cx);
                });
                self.apply_subagent_panel_update(snapshot, cx);
                if reply.send(Ok(())).is_err() {
                    cancel.cancel();
                }
            }
            NativeSubagentUpdate::Event {
                thread_id,
                message_id,
                event,
            } => {
                let Some(active) = self
                    .agent_controller
                    .active
                    .get(&thread_id)
                    .filter(|active| {
                        active.delegated_message_id.as_deref() == Some(&message_id)
                            && active.delegated_parent_thread.as_deref() == Some(parent_id)
                    })
                else {
                    return;
                };
                let stream = active.stream.clone();
                let generation = active.generation;
                parent_stream.update(cx, |parent, cx| parent.apply_subagent_usage(&event, cx));
                let batch = AgentBatch {
                    composer_prediction: None,
                    subagents: Vec::new(),
                    events: vec![event],
                    codex_mode_confirmed: false,
                    computer_use_ready: None,
                    mcp_unavailable: Vec::new(),
                    reference_failure: None,
                    credential_failure: false,
                    finished: None,
                };
                self.apply_agent_batch_ingress(generation, &thread_id, &stream, batch, cx);
            }
            NativeSubagentUpdate::Updated(snapshot) => {
                if snapshot.parent_thread_id == parent_id {
                    self.apply_subagent_panel_update(snapshot.clone(), cx);
                    parent_stream.update(cx, |stream, cx| stream.apply_subagent(snapshot, cx));
                }
            }
            NativeSubagentUpdate::Finished {
                snapshot,
                usage_complete,
            } => {
                if snapshot.parent_thread_id != parent_id {
                    return;
                }
                self.apply_subagent_panel_update(snapshot.clone(), cx);
                parent_stream.update(cx, |stream, cx| {
                    stream.apply_subagent(snapshot.clone(), cx);
                    stream.finish_subagent_usage(usage_complete, cx);
                });
                let Some(active) =
                    self.agent_controller
                        .active
                        .get(&snapshot.agent_id)
                        .filter(|active| {
                            active.delegated_message_id.as_deref() == Some(&snapshot.message_id)
                                && active.delegated_parent_thread.as_deref() == Some(parent_id)
                        })
                else {
                    return;
                };
                let stream = active.stream.clone();
                let generation = active.generation;
                let batch = AgentBatch {
                    composer_prediction: None,
                    subagents: Vec::new(),
                    events: Vec::new(),
                    codex_mode_confirmed: false,
                    computer_use_ready: None,
                    mcp_unavailable: Vec::new(),
                    reference_failure: None,
                    credential_failure: false,
                    finished: Some(snapshot.state == SubagentState::Completed),
                };
                if let AgentBatchIngress::Finished { run, .. } = self.apply_agent_batch_ingress(
                    generation,
                    &snapshot.agent_id,
                    &stream,
                    batch,
                    cx,
                ) {
                    let cancelled =
                        snapshot.state == SubagentState::Interrupted || run.cancel.is_cancelled();
                    stream.update(cx, |stream, cx| {
                        stream.finish_composer_run(cancelled, cx);
                        stream.timeout_permission(cx);
                    });
                    cx.update_global::<crate::automation_controller::AutomationGlobal, _>(
                        |global, _| {
                            global.finish_run(&snapshot.agent_id, generation, &self.window_alive)
                        },
                    );
                    self.finish_pending_run(
                        &snapshot.agent_id,
                        &stream,
                        snapshot.state == SubagentState::Completed && !cancelled,
                        cx,
                    );
                    self.resume_deferred_message_location(&snapshot.agent_id, cx);
                }
            }
        }
        cx.notify();
    }

    pub(crate) fn open_subagent(
        &mut self,
        stream: Entity<ConversationStream>,
        request: &SubagentOpenRequested,
        cx: &mut Context<Self>,
    ) {
        if !self.owns_stream_request(&stream, &request.source_thread_id, cx) {
            return;
        }
        let Some(database) = self.context_database(cx) else {
            return;
        };
        let source = request.source_thread_id.clone();
        let target = request.thread_id.clone();
        let worker_source = source.clone();
        let work = cx.background_executor().spawn(async move {
            let store = vega_store::Store::open(database).map_err(|_| ())?;
            vega_conversation::subagents::related_thread(&store, &worker_source, &target)
                .map_err(|_| ())
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                if !this.owns_stream_request(&stream, &source, cx) {
                    return;
                }
                match result {
                    Ok(thread) => {
                        this.accept_palette_thread(thread, cx);
                    }
                    Err(()) => stream.update(cx, ConversationStream::apply_controller_error),
                }
            });
        })
        .detach();
    }

    pub(crate) fn stop_subagent(
        &mut self,
        stream: Entity<ConversationStream>,
        request: &SubagentStopRequested,
        cx: &mut Context<Self>,
    ) {
        if !self.owns_stream_request(&stream, &request.source_thread_id, cx) {
            return;
        }
        if let Some(active) =
            self.agent_controller
                .active
                .get(&request.thread_id)
                .filter(|active| {
                    active.delegated_message_id.as_deref() == Some(&request.message_id)
                        && active.delegated_parent_thread.as_deref()
                            == Some(&request.source_thread_id)
                })
        {
            active.cancel.cancel();
            stream.update(cx, |stream, cx| {
                stream.mark_subagent_cancelling(&request.thread_id, &request.message_id, cx)
            });
        }
    }
}

impl VegaWindow {
    pub(crate) fn bind_conversation_stream(
        &mut self,
        view: &Entity<ConversationStream>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view = view.clone();
        cx.subscribe_in(
            &view,
            window,
            |this, stream, request: &ImagePreviewRequested, window, cx| {
                if this.panel_image_requested(stream, cx) {
                    return;
                }
                this.handle_image_preview(stream.clone(), request, window, cx);
            },
        )
        .detach();
        cx.observe_in(&view, window, |this, _, window, cx| {
            this.sync_image_preview(window, cx);
        })
        .detach();
        cx.subscribe(&view, |this, stream, request, cx| {
            this.apply_thread_model_selection(stream.clone(), request, cx);
        })
        .detach();
        cx.subscribe(
            &view,
            |this, stream, request: &TaskBackendSelectionRequested, cx| {
                this.select_draft_task_backend(stream.clone(), request, cx);
            },
        )
        .detach();
        cx.subscribe(&view, |this, stream, request, cx| {
            this.persist_composer_thinking(stream.clone(), request, cx);
        })
        .detach();
        cx.subscribe(
            &view,
            |this, stream, request: &vega_ui::conversation_stream::PrivacyRequested, cx| {
                this.request_privacy(stream.clone(), request, cx);
            },
        )
        .detach();
        cx.subscribe(
            &view,
            |this,
             stream,
             request: &vega_ui::conversation_stream::CredentialContinuationRequested,
             cx| {
                this.resume_credential_continuation(stream.clone(), request, cx);
            },
        )
        .detach();
        cx.subscribe(&view, |this, stream, request, cx| {
            this.persist_thread_settings(stream.clone(), request, cx);
        })
        .detach();
        cx.subscribe(
            &view,
            |this, stream, request: &ContextSettingsRequested, cx| {
                this.persist_context_settings(stream.clone(), request, cx);
            },
        )
        .detach();
        cx.subscribe(
            &view,
            |this, stream, request: &ContextCompactionRequested, cx| {
                this.start_context_compaction(stream.clone(), request, cx);
            },
        )
        .detach();
        cx.subscribe(
            &view,
            |this, stream, request: &ContextCompactionCancelRequested, cx| {
                this.cancel_context_operation(stream.clone(), request, cx);
            },
        )
        .detach();
        cx.subscribe(&view, |this, stream, request, cx| {
            this.review_plan(stream.clone(), request, cx);
        })
        .detach();
        cx.subscribe(&view, |this, stream, request, cx| {
            this.submit_composer(stream.clone(), request, cx);
        })
        .detach();
        cx.subscribe_in(
            &view,
            window,
            |this, stream, request: &AutomationEditorRequested, window, cx| {
                this.open_automation_editor(stream.clone(), request, window, cx);
            },
        )
        .detach();
        cx.subscribe(&view, |this, stream, request: &AutomationRequested, cx| {
            this.automation_requested(stream.clone(), request, cx)
        })
        .detach();
        cx.subscribe(
            &view,
            |this, stream, request: &PendingQueueRequested, cx| {
                this.pending_queue_requested(stream.clone(), request, cx)
            },
        )
        .detach();
        cx.subscribe(
            &view,
            |this, stream, request: &SkillComposerProjectionRequested, cx| {
                this.request_skill_composer_projection(stream.clone(), request, cx);
            },
        )
        .detach();
        cx.subscribe(
            &view,
            |this, stream, request: &SkillComposerMutationRequested, cx| {
                this.request_skill_composer_mutation(stream.clone(), request, cx);
            },
        )
        .detach();
        view.update(cx, ConversationStream::request_skill_projection);
        cx.subscribe(
            &view,
            |this, stream, request: &ComposerStopRequested, cx| {
                this.stop_composer(stream.clone(), request, cx);
            },
        )
        .detach();
        cx.subscribe(&view, |this, stream, request: &FileIndexRequested, cx| {
            this.request_file_index(stream.clone(), request, cx);
        })
        .detach();
        cx.subscribe(&view, |this, stream, request: &FileIndexCancelled, cx| {
            this.cancel_file_index(stream.clone(), request, cx);
        })
        .detach();
        cx.subscribe(
            &view,
            |this, stream, request: &FileIndexRetryRequested, cx| {
                this.retry_file_index(stream.clone(), request, cx);
            },
        )
        .detach();
        cx.subscribe(&view, |this, stream, request, cx| {
            this.open_workspace_diff(
                stream.clone(),
                request,
                DiffFocusIntent::PreserveCurrent,
                cx,
            );
        })
        .detach();
        cx.subscribe(&view, |this, stream, request, cx| {
            this.open_commit_panel(stream.clone(), request, cx);
        })
        .detach();
        cx.subscribe(&view, |this, stream, request, cx| {
            this.workspace_tool_terminal(stream.clone(), request, cx);
        })
        .detach();
        cx.subscribe(&view, |this, stream, request, cx| {
            this.request_history_page(stream.clone(), request, cx);
        })
        .detach();
        cx.subscribe(
            &view,
            |this, stream, request: &NewerHistoryPageRequested, cx| {
                this.request_newer_history_page(stream.clone(), request, cx);
            },
        )
        .detach();
        cx.subscribe(
            &view,
            |this, stream, request: &MessageLocationRequested, cx| {
                this.request_message_location(stream.clone(), request, cx);
            },
        )
        .detach();
        let branch_selector = view.read(cx).branch_selector();
        cx.subscribe(&branch_selector, |this, selector, request, cx| {
            this.request_branch_list(selector.clone(), request, cx);
        })
        .detach();
        cx.subscribe(&branch_selector, |this, selector, request, cx| {
            this.request_branch_switch(selector.clone(), request, cx);
        })
        .detach();
        cx.subscribe(&branch_selector, |this, selector, request, cx| {
            this.branch_selector_closed(selector.clone(), request, cx);
        })
        .detach();
        let commit_panel = view.read(cx).commit_panel();
        cx.subscribe(&commit_panel, |this, panel, request, cx| {
            this.request_commit_prepare(panel.clone(), request, cx);
        })
        .detach();
        cx.subscribe(&commit_panel, |this, panel, request, cx| {
            this.request_commit_draft(panel.clone(), request, cx);
        })
        .detach();
        cx.subscribe(&commit_panel, |this, panel, request, cx| {
            this.request_commit_execute(panel.clone(), request, cx);
        })
        .detach();
        cx.subscribe(&commit_panel, |this, panel, request, cx| {
            this.commit_panel_closed(panel.clone(), request, cx);
        })
        .detach();
        cx.subscribe_in(
            &view,
            window,
            |this, stream, request: &SubagentOpenRequested, window, cx| {
                this.open_subagent_preview(stream.clone(), request, window, cx)
            },
        )
        .detach();
        cx.subscribe(
            &view,
            |this, stream, request: &SubagentStopRequested, cx| {
                this.stop_subagent(stream.clone(), request, cx)
            },
        )
        .detach();
    }
}
