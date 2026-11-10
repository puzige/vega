use super::workspace::nsh_workspace;
use super::*;

fn clear_confirmed_reasoning_default_ledger(
    failures: &mut std::collections::HashMap<(String, String), ReasoningDefaultFailure>,
    provider: &str,
    model: &str,
    choice: &str,
) {
    failures.retain(|_, failure| {
        failure.request.defaults.model != model
            || failure.request.defaults.thinking != choice
            || failure
                .request
                .defaults
                .reasoning
                .as_ref()
                .is_none_or(|profile| profile.provider != provider)
    });
}

fn begin_reasoning_default_ledger(
    failures: &mut std::collections::HashMap<(String, String), ReasoningDefaultFailure>,
    latest: &mut std::collections::HashMap<(String, String), ComposerDefaultsRequested>,
    request: &ComposerDefaultsRequested,
) {
    let key = (request.thread_id.clone(), request.defaults.model.clone());
    latest.insert(key.clone(), request.clone());
    if let Some(failure) = failures.get_mut(&key) {
        failure.retry_pending = true;
    }
}

fn finish_reasoning_default_ledger(
    failures: &mut std::collections::HashMap<(String, String), ReasoningDefaultFailure>,
    latest: &mut std::collections::HashMap<(String, String), ComposerDefaultsRequested>,
    request: &ComposerDefaultsRequested,
    defaults_failed: Option<bool>,
) -> bool {
    let key = (request.thread_id.clone(), request.defaults.model.clone());
    match latest.get(&key) {
        None => return false,
        Some(current) if current != request => {
            match defaults_failed {
                Some(true) => {
                    failures.insert(
                        key,
                        ReasoningDefaultFailure {
                            request: request.clone(),
                            retry_pending: true,
                        },
                    );
                }
                Some(false) => {
                    failures.remove(&key);
                }
                None => {}
            }
            return false;
        }
        Some(_) => {}
    }
    latest.remove(&key);
    match defaults_failed {
        Some(true) => {
            failures.insert(
                key,
                ReasoningDefaultFailure {
                    request: request.clone(),
                    retry_pending: false,
                },
            );
        }
        Some(false) => {
            failures.remove(&key);
        }
        None => {
            if let Some(failure) = failures.get_mut(&key) {
                failure.retry_pending = false;
            }
        }
    }
    defaults_failed == Some(false)
}

impl VegaWindow {
    fn bind_reasoning_default_retry(
        &mut self,
        stream: &Entity<ConversationStream>,
        cx: &mut Context<Self>,
    ) {
        self.reasoning_default_retry_subscriptions
            .retain(|_, (stream, _)| stream.upgrade().is_some());
        if self
            .reasoning_default_retry_subscriptions
            .contains_key(&stream.entity_id())
        {
            return;
        }
        let subscription = cx.subscribe(
            stream,
            |this, stream, event: &ReasoningDefaultRetryRequested, cx| {
                this.retry_reasoning_default(stream.clone(), event, cx);
            },
        );
        self.reasoning_default_retry_subscriptions
            .insert(stream.entity_id(), (stream.downgrade(), subscription));
    }

    pub(crate) fn project_reasoning_default_failures(
        &self,
        stream: &Entity<ConversationStream>,
        thread_id: &str,
        cx: &mut Context<Self>,
    ) {
        let failures = self
            .reasoning_default_failures
            .iter()
            .filter(|((id, _), _)| id == thread_id)
            .map(|(_, failure)| failure.clone())
            .collect();
        stream.update(cx, |stream, cx| {
            stream.apply_reasoning_default_failures(failures, cx);
        });
    }

    pub(crate) fn begin_reasoning_default_save(
        &mut self,
        request: &ComposerDefaultsRequested,
        cx: &mut Context<Self>,
    ) {
        begin_reasoning_default_ledger(
            &mut self.reasoning_default_failures,
            &mut self.reasoning_default_latest,
            request,
        );
        self.refresh_reasoning_default_failures(&request.thread_id, cx);
    }

    pub(crate) fn finish_reasoning_default_save(
        &mut self,
        request: &ComposerDefaultsRequested,
        defaults_failed: Option<bool>,
        cx: &mut Context<Self>,
    ) {
        let confirmed = finish_reasoning_default_ledger(
            &mut self.reasoning_default_failures,
            &mut self.reasoning_default_latest,
            request,
            defaults_failed,
        );
        if confirmed && let Some(profile) = &request.defaults.reasoning {
            self.clear_reasoning_default_failures_matching(
                &profile.provider,
                &request.defaults.model,
                &request.defaults.thinking,
                cx,
            );
        }
        self.refresh_reasoning_default_failures(&request.thread_id, cx);
    }

    pub(crate) fn discard_reasoning_default_failure(
        &mut self,
        request: &ComposerDefaultsRequested,
        cx: &mut Context<Self>,
    ) {
        let key = (request.thread_id.clone(), request.defaults.model.clone());
        if self
            .reasoning_default_failures
            .get(&key)
            .is_some_and(|failure| failure.request == *request)
        {
            self.reasoning_default_failures.remove(&key);
            self.refresh_reasoning_default_failures(&request.thread_id, cx);
        }
    }

    pub(crate) fn clear_reasoning_default_failures_matching(
        &mut self,
        provider: &str,
        model: &str,
        choice: &str,
        cx: &mut Context<Self>,
    ) {
        clear_confirmed_reasoning_default_ledger(
            &mut self.reasoning_default_failures,
            provider,
            model,
            choice,
        );
        if let Some((id, stream)) = &self.stream_view {
            self.project_reasoning_default_failures(stream, id, cx);
        }
        cx.notify();
    }

    fn refresh_reasoning_default_failures(&self, thread_id: &str, cx: &mut Context<Self>) {
        if let Some((id, stream)) = &self.stream_view
            && id == thread_id
        {
            self.project_reasoning_default_failures(stream, thread_id, cx);
        }
        cx.notify();
    }

    fn retry_reasoning_default(
        &mut self,
        stream: Entity<ConversationStream>,
        event: &ReasoningDefaultRetryRequested,
        cx: &mut Context<Self>,
    ) {
        let request = &event.request;
        let key = (request.thread_id.clone(), request.defaults.model.clone());
        if !self.owns_stream_request(&stream, &request.thread_id, cx)
            || !self
                .reasoning_default_failures
                .get(&key)
                .is_some_and(|failure| !failure.retry_pending && failure.request == *request)
        {
            return;
        }
        self.begin_reasoning_default_save(request, cx);
        self.retry_composer_thinking(stream, request, cx);
    }
}

impl Render for VegaWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_language(cx);
        crate::nsh_preview::sync_menus(cx);
        self.sync_navigation(cx);
        self.sync_preparation(cx);
        self.sync_completion_attention(window, cx);
        if self.appearance_subscription.is_none() {
            self.appearance_subscription =
                Some(cx.observe_window_appearance(window, |_, _, cx| {
                    if theme(cx).follow_system {
                        cx.set_global(Theme::system(cx));
                        cx.notify();
                    }
                }));
        }
        // Palette comes from the global theme so Cmd+Shift+L repaints instantly.
        let colors = theme(cx).colors;
        // Effective visibility: the user preference (Cmd+B, persisted) AND the
        // viewport auto-collapse rule (ui-spec §1).
        let sidebar_visible =
            !cx.global::<SidebarCollapsed>().0 && !self.auto_collapsed(window, cx);

        let pending_delete = cx.global::<PendingDeleteConfirm>().0.clone();

        let settings_open = cx.global::<SettingsOpen>().0;
        if settings_open {
            self.set_workspace_presentation_visible(false, cx);
            self.hide_shell_chrome(cx);
        }
        if self.diff_controller.active.as_ref().is_some_and(|active| {
            settings_open || !Self::diff_route_is_current(&active.identity, cx)
        }) {
            self.diff_controller.close();
        }
        if self
            .artifact_controller
            .active
            .as_ref()
            .is_some_and(|active| {
                settings_open || !Self::artifact_route_is_current(&active.identity, cx)
            })
        {
            self.close_artifact_route(GitWorkspaceErrorCode::StaleGeneration, cx);
        }
        if self
            .branch_controller
            .active
            .as_ref()
            .is_some_and(|active| {
                settings_open || !Self::branch_route_is_current(&active.identity, cx)
            })
        {
            self.close_branch_route(GitWorkspaceErrorCode::StaleGeneration, cx);
        }
        self.sync_workspace_route(cx);
        self.sync_automation_workspace_size(window, cx);
        self.sync_subagent_workspace_size(window, cx);
        self.sync_automation_editor_visibility(window, cx);
        if !settings_open
            && self.diff_controller.active.is_none()
            && self
                .workspace
                .tabs
                .iter()
                .any(|(key, _)| *key == workspace::TabKey::Diff)
        {
            self.workspace_restore_review(cx);
        }
        self.sync_palette(window, cx);
        self.sync_navigation(cx);
        let settings_open = cx.global::<SettingsOpen>().0;
        let opening_thread = self.sidebar.read(cx).opening_thread(cx).is_some();
        if (settings_open || opening_thread)
            && let Some((_, stream)) = &self.stream_view
        {
            stream.update(cx, ConversationStream::suspend_ui_presentations);
        }
        let content: AnyElement = if settings_open {
            self.conversation_loading.clear();
            if self.settings_view.is_none()
                && let Some((_, stream)) = &self.stream_view
            {
                stream.update(cx, ConversationStream::suspend_skill_projection);
            }
            self.start_model_catalog_load(cx);
            // 设置视图：缓存 Entity，避免主题刷新等重渲染时重建导致表单输入丢失。
            if self.settings_view.is_none() {
                let config_path = self.composer_config_path();
                let skills_service = self
                    .context_database(cx)
                    .zip(
                        config_path
                            .as_deref()
                            .and_then(std::path::Path::parent)
                            .map(std::path::Path::to_path_buf),
                    )
                    .map(|(database, config_root)| {
                        SkillSettingsService::new(database, config_root, self.shell_project_id(cx))
                    });
                let permission_service =
                    self.context_database(cx)
                        .zip(config_path.clone())
                        .map(|(database, config)| {
                            vega_conversation::permission_contracts::DefaultPermissionService::new(
                                database, config,
                            )
                        });
                let settings = cx.new(|cx| SettingsView::from_path(config_path, cx));
                settings.update(cx, |view, cx| {
                    view.install_mcp_service(self.mcp_settings.clone(), cx);
                    view.install_default_permission_service(permission_service, cx);
                    view.apply_permission_reset_projection(
                        self.permission_reset.projection.clone(),
                        cx,
                    );
                });
                cx.subscribe(
                    &settings,
                    |this, view, request: &PermissionResetRequested, cx| {
                        this.permission_reset_requested(view.clone(), request.0, cx);
                    },
                )
                .detach();
                crate::app_usage::bind(&settings, cx);
                settings.update(cx, |view, cx| {
                    view.apply_update_projection(self.localized_update_projection(), cx)
                });
                cx.subscribe(&settings, |this, _, request: &UpdateRequest, cx| {
                    this.request_update(*request, cx);
                })
                .detach();
                cx.subscribe(
                    &settings,
                    |this, view, request: &PricingMutationRequested, cx| {
                        this.request_pricing_mutation(view.clone(), request, cx);
                    },
                )
                .detach();
                cx.subscribe(&settings, |this, view, _: &PricingReloadRequested, cx| {
                    this.request_pricing_reload(view.clone(), cx);
                })
                .detach();
                cx.subscribe(
                    &settings,
                    |this, view, request: &PricingRetryRequested, cx| {
                        this.request_pricing_retry(view.clone(), request, cx);
                    },
                )
                .detach();
                cx.subscribe(
                    &settings,
                    |this, view, request: &PricingDiscardRequested, cx| {
                        this.request_pricing_discard(view.clone(), request, cx);
                    },
                )
                .detach();
                cx.subscribe(
                    &settings,
                    |this, view, request: &vega_conversation::types::ModelCatalogRequested, cx| {
                        this.request_model_catalog(view.clone(), request, cx);
                    },
                )
                .detach();
                cx.subscribe(&settings, |this, _, event: &SettingsSaved, cx| {
                    this.on_settings_saved(event.replay_identity_changed, cx);
                })
                .detach();
                cx.subscribe(
                    &settings,
                    |this, view, request: &DesktopPreparationRequested, cx| {
                        if this.settings_view.as_ref() == Some(&view)
                            && cx.global::<SettingsOpen>().0
                        {
                            this.request_settings_preparation(request.pane, cx);
                        }
                    },
                )
                .detach();
                cx.subscribe(
                    &settings,
                    |this, view, request: &ModelContextLoadRequested, cx| {
                        this.request_model_context_load(view.clone(), request, cx);
                    },
                )
                .detach();
                cx.subscribe(
                    &settings,
                    |this, view, request: &ModelContextSaveRequested, cx| {
                        this.request_model_context_save(view.clone(), request, cx);
                    },
                )
                .detach();
                cx.subscribe(
                    &settings,
                    |this, view, request: &ReasoningProfileSaveRequested, cx| {
                        this.request_reasoning_profile_save(view.clone(), request, cx);
                    },
                )
                .detach();
                cx.subscribe(&settings, |this, view, _: &ReasoningReloadRequested, cx| {
                    this.request_reasoning_reload(view.clone(), cx);
                })
                .detach();
                cx.subscribe(
                    &settings,
                    |this, view, event: &ModelCapabilitySaveRequested, cx| {
                        this.request_model_capability_save(view.clone(), event, cx)
                    },
                )
                .detach();
                cx.subscribe(
                    &settings,
                    |this, view, _: &ModelCapabilityReloadRequested, cx| {
                        this.request_model_capability_reload(view.clone(), cx)
                    },
                )
                .detach();
                let projection = self.pricing_controller.projection();
                let reasoning_projection = self.reasoning_settings_projection();
                settings.update(cx, |settings, cx| {
                    settings.apply_pricing_projection(projection, cx);
                    settings.apply_reasoning_projection(reasoning_projection, cx);
                    settings.set_skills_service(skills_service, cx);
                });
                let focus = settings.read(cx).focus_handle(cx);
                window.focus(&focus, cx);
                self.settings_view = Some(settings.clone());
                self.request_model_capability_reload(settings, cx);
            }
            match &self.settings_view {
                Some(settings) => settings.clone().into_any_element(),
                None => div().size_full().into_any_element(),
            }
        } else if let Some((_, started)) = self.sidebar.read(cx).opening_thread(cx) {
            self.render_conversation_loading(Some(started), cx)
        } else {
            // 设置已关闭：丢弃缓存，下次打开时重新构造并载入最新配置。
            let departing_settings = self.settings_view.take();
            let returning_from_settings = departing_settings.is_some();
            if let Some(settings) = departing_settings {
                settings.update(cx, |settings, _| {
                    settings.close_default_permission_session()
                });
            }

            let thread = self.resolve_route_thread(cx);
            self.update_message_location_route(Some(&thread.id));
            let notification_tail = self.completion_attention.pending_tail_thread.as_deref()
                == Some(thread.id.as_str());
            let draft_route = self.is_draft_route(&thread.id);
            {
                {
                    let cached = self
                        .agent_controller
                        .active_stream_for_thread(&thread.id)
                        .or_else(|| {
                            self.agent_controller
                                .preparation_stream
                                .as_ref()
                                .filter(|stream| {
                                    self.agent_controller
                                        .ordinary_preparing
                                        .contains_key(&thread.id)
                                        && stream.read(cx).thread_snapshot().id == thread.id
                                })
                                .cloned()
                        })
                        .or_else(|| self.composer_prediction_stream(&thread.id))
                        .or_else(|| self.automation_controller.streams.get(&thread.id).cloned())
                        .or_else(|| match &self.stream_view {
                            Some((thread_id, view)) if *thread_id == thread.id => {
                                Some(view.clone())
                            }
                            _ => None,
                        });
                    let waiting = cached.is_none()
                        && !draft_route
                        && !self.conversation_ready(&thread.id, cx);
                    if waiting {
                        if let Some((_, stream)) = &self.stream_view {
                            stream.update(cx, ConversationStream::suspend_ui_presentations);
                        }
                        self.render_conversation_loading(None, cx)
                    } else {
                        if cached.is_some() || draft_route {
                            self.conversation_loading.clear();
                        }
                        let stream = match cached {
                            Some(view) => {
                                if self.stream_view.as_ref().is_none_or(|(thread_id, cached)| {
                                    thread_id != &thread.id || cached != &view
                                }) {
                                    if let Some((previous_id, previous)) = self.stream_view.take() {
                                        previous.update(
                                            cx,
                                            ConversationStream::suspend_ui_presentations,
                                        );
                                        if previous_id != thread.id {
                                            let anchor = previous.read(cx).scroll_anchor_snapshot();
                                            self.thread_scroll_states
                                                .remember(previous_id.clone(), anchor);
                                        }
                                        if self
                                            .agent_controller
                                            .active_stream_for_thread(&previous_id)
                                            .as_ref()
                                            != Some(&previous)
                                        {
                                            previous.update(cx, |stream, cx| {
                                                stream.timeout_permission(cx)
                                            });
                                        }
                                    }
                                    self.stream_view = Some((thread.id.clone(), view.clone()));
                                    let model = view.read(cx).displayed_model().to_owned();
                                    self.apply_reasoning_profile_to_stream(&view, &model, cx);
                                    if !draft_route
                                        && self.composer_prediction_stream(&thread.id).as_ref()
                                            != Some(&view)
                                    {
                                        self.load_pending_queue(
                                            view.clone(),
                                            thread.id.clone(),
                                            cx,
                                        );
                                    }
                                    self.load_running_message_default(cx);
                                }
                                if self.agent_controller.delegated_unbound.remove(&thread.id) {
                                    let models = self.model_options_for_pricing();
                                    let label = self
                                        .sidebar
                                        .read(cx)
                                        .project_label(&thread.project_id, cx)
                                        .unwrap_or_default();
                                    view.update(cx, |stream, cx| {
                                        stream.apply_model_options(models, cx);
                                        stream.set_project_label(label, cx);
                                    });
                                    self.apply_reasoning_profile_to_stream(
                                        &view,
                                        &thread.model,
                                        cx,
                                    );
                                    self.bind_conversation_stream(&view, window, cx);
                                }

                                let needs_rebind = draft_route
                                    && view.read(cx).route_project_id() != thread.project_id;
                                let label = if needs_rebind {
                                    self.sidebar
                                        .read(cx)
                                        .project_label(&thread.project_id, cx)
                                        .unwrap_or_default()
                                } else {
                                    String::new()
                                };
                                view.update(cx, |stream, cx| {
                                    stream.set_draft_route(draft_route, cx);
                                    if needs_rebind {
                                        stream.rebind_draft_project(thread.clone(), label, cx);
                                    }
                                });
                                view
                            }
                            None => {
                                if let Some((previous_id, previous)) = self.stream_view.take() {
                                    previous
                                        .update(cx, ConversationStream::suspend_ui_presentations);
                                    if previous_id != thread.id {
                                        let anchor = previous.read(cx).scroll_anchor_snapshot();
                                        self.thread_scroll_states
                                            .remember(previous_id.clone(), anchor);
                                    }
                                    if self
                                        .agent_controller
                                        .active_stream_for_thread(&previous_id)
                                        .as_ref()
                                        != Some(&previous)
                                    {
                                        previous
                                            .update(cx, |stream, cx| stream.timeout_permission(cx));
                                    }
                                }
                                let view = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
                                let label = self
                                    .sidebar
                                    .read(cx)
                                    .project_label(&thread.project_id, cx)
                                    .unwrap_or_default();
                                view.update(cx, |stream, cx| {
                                    stream.set_draft_route(draft_route, cx);
                                    stream.set_project_label(label, cx);
                                });

                                let model_options = self.model_options_for_pricing();
                                let initial = self.conversation_loading.take_ready();
                                let pending_model = initial
                                    .as_ref()
                                    .and_then(|loaded| loaded.pending_model.clone());
                                view.update(cx, |stream, cx| {
                                    stream.apply_thread(thread.clone(), cx);
                                    stream.restore_next_turn_model(pending_model, cx);
                                    stream.apply_model_options(model_options, cx);
                                });
                                if let Some(initial) = initial {
                                    view.update(cx, |stream, cx| (initial.apply)(stream, cx));
                                }
                                self.apply_reasoning_profile_to_stream(&view, &thread.model, cx);
                                self.agent_controller.delegated_unbound.remove(&thread.id);
                                self.bind_conversation_stream(&view, window, cx);

                                self.conversation_loading.clear();
                                self.rehydrate_ordinary(&view, &thread.id, cx);
                                self.stream_view = Some((thread.id.clone(), view.clone()));
                                if !draft_route {
                                    self.load_pending_queue(view.clone(), thread.id.clone(), cx);
                                }
                                if !notification_tail
                                    && let Some(anchor) = self.thread_scroll_states.get(&thread.id)
                                {
                                    view.update(cx, |stream, cx| {
                                        if !stream.restore_scroll_anchor(&anchor)
                                            && let Some(message_id) = anchor.message_id.as_deref()
                                        {
                                            stream.request_message_location_with_anchor(
                                                message_id,
                                                Some(anchor.clone()),
                                                cx,
                                            );
                                        }
                                    });
                                }
                                self.restore_navigation_draft(&thread.id, &view, cx);
                                self.sync_navigation(cx);
                                self.start_model_catalog_load(cx);
                                view
                            }
                        };
                        self.bind_reasoning_default_retry(&stream, cx);
                        self.project_reasoning_default_failures(&stream, &thread.id, cx);
                        self.project_pending_reasoning_intents(&stream, &thread.id, cx);
                        if notification_tail {
                            self.completion_attention.pending_tail_thread = None;
                            stream.update(cx, ConversationStream::resume_tail);
                        }
                        stream.update(cx, ConversationStream::refresh_ordinary_submissions);
                        self.sync_context_route(&stream, cx);
                        // Settings close invalidates the catalog even when this
                        // thread entity remains cached; restart the worker on
                        // the next visible frame so newly saved providers/models
                        // become selectable without rebuilding the session.
                        self.start_model_catalog_load(cx);

                        if !draft_route {
                            self.ensure_artifact_route(&thread, stream.clone(), cx);
                        }
                        self.ensure_branch_route(&thread, stream.clone(), cx);
                        let commit_focus = self
                            .commit_controller
                            .active
                            .as_ref()
                            .filter(|active| {
                                active.focus_pending && active.identity.stream == stream
                            })
                            .map(|active| active.identity.panel.read(cx).focus_handle(cx));
                        if let Some(focus) = commit_focus {
                            window.focus(&focus, cx);
                            if let Some(active) = self.commit_controller.active.as_mut() {
                                active.focus_pending = false;
                            }
                        }
                        if self.fork_focus_thread.as_deref() == Some(thread.id.as_str()) {
                            self.fork_focus_thread = None;
                            stream.update(cx, |stream, cx| stream.focus_composer(window, cx));
                        }
                        if returning_from_settings {
                            stream.update(cx, |stream, cx| {
                                stream.request_skill_projection(cx);
                                stream.focus_composer(window, cx);
                            });
                        }
                        stream.update(cx, |stream, cx| stream.set_reading_preview(false, cx));
                        stream.into_any_element()
                    }
                }
            }
        };

        self.sync_subagent_panel_route(cx);
        let content = if settings_open {
            content
        } else {
            let navigation_error = cx
                .try_global::<vega_ui::navigation::NavigationState>()
                .and_then(|state| state.error_message());
            let main = div()
                .size_full()
                .min_h_0()
                .flex()
                .flex_col()
                .child(self.render_main_header(sidebar_visible, window, cx))
                .children(self.render_permission_reset_notice(window, cx))
                .children(
                    navigation_error
                        .map(|_| self.render_shell_chrome(shell_chrome::Kind::Error, window, cx)),
                )
                .child(div().flex_1().min_h_0().overflow_hidden().child(content))
                .into_any_element();
            self.render_workspace(main, window, cx)
        };

        if let Some(palette) = &self.palette.view {
            let focus = palette.read(cx).focus_handle(cx);
            if !focus.is_focused(window) {
                window.focus(&focus, cx);
            }
        }
        self.sync_navigation(cx);
        self.sync_preparation(cx);
        let reading_blocked = settings_open
            || self.palette.view.is_some()
            || pending_delete.is_some()
            || self.sidebar.read(cx).opening_thread(cx).is_some();
        if let Some((_, stream)) = &self.stream_view {
            ConversationStream::set_reading_navigation_blocked(stream, reading_blocked, cx);
        }
        self.set_subagent_reading_navigation_blocked(reading_blocked, cx);
        let preparation_error = self.render_preparation_error(cx);
        div()
            .key_context("VegaWindow")
            .on_action(cx.listener(Self::toggle_sidebar))
            .on_action(cx.listener(Self::open_palette))
            .on_action(cx.listener(Self::palette_open_workspace))
            .on_action(cx.listener(Self::palette_toggle_terminal))
            .on_action(cx.listener(Self::dismiss_environment_overlay))
            .size_full()
            .flex()
            .flex_row()
            .relative()
            .text_color(colors.text_primary)
            .child(nsh_workspace::background(
                "nsh-window-base",
                colors.bg_base,
                window,
                cx,
            ))
            .child(gpui_kit::base::TextSelectionLayer)
            .when(sidebar_visible && !cx.global::<SettingsOpen>().0, |row| {
                row.child(self.sidebar.clone())
                    .child(self.render_sidebar_resizer(colors, window, cx))
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .relative()
                    .child(nsh_workspace::background(
                        "nsh-window-main-base",
                        colors.bg_base,
                        window,
                        cx,
                    ))
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .children(
                        settings_open
                            .then(|| {
                                cx.try_global::<vega_ui::navigation::NavigationState>()
                                    .and_then(|state| state.error_message())
                                    .map(|_| {
                                        self.render_shell_chrome(
                                            shell_chrome::Kind::Error,
                                            window,
                                            cx,
                                        )
                                    })
                            })
                            .flatten(),
                    )
                    .children(vega_ui::sidebar::preference_save_notice(cx))
                    .children(preparation_error)
                    .child(div().flex_1().min_h_0().child(content)),
            )
            .children((!settings_open).then(|| self.render_shell_slot_cluster(window, cx)))
            .on_mouse_move(cx.listener(Self::resize_sidebar))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::finish_sidebar_resize))
            .children(self.palette.view.clone())
            .children(
                pending_delete.map(|thread| {
                    render_delete_confirm_overlay(&thread, self.sidebar.clone(), colors)
                }),
            )
            .child(vega_ui::link_preview::MarkdownLinkPreviewLayer)
    }
}

impl VegaWindow {
    fn render_sidebar_resizer(
        &self,
        colors: ThemeColors,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .id("sidebar-resize-handle")
            .w(px(Layout::SIDEBAR_RESIZE_HIT_AREA))
            .h_full()
            .flex_shrink_0()
            .flex()
            .justify_end()
            .relative()
            .child(nsh_workspace::background(
                "nsh-sidebar-resizer-base",
                colors.bg_sidebar,
                window,
                cx,
            ))
            .cursor_col_resize()
            .child(nsh_workspace::separator(
                "nsh-sidebar-separator",
                true,
                window,
                cx,
            ))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::begin_sidebar_resize))
            .into_any_element()
    }

    fn begin_sidebar_resize(
        &mut self,
        _: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.prevent_default();
        cx.stop_propagation();
        self.sidebar_resize_dragging = true;
        cx.notify();
    }

    fn resize_sidebar(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if !self.sidebar_resize_dragging {
            return;
        }
        if event.pressed_button != Some(MouseButton::Left) {
            self.sidebar_resize_dragging = false;
            vega_ui::sidebar::persist_width(cx);
            return;
        }
        vega_ui::sidebar::set_width(f32::from(event.position.x), cx);
    }

    fn finish_sidebar_resize(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.sidebar_resize_dragging {
            self.sidebar_resize_dragging = false;
            vega_ui::sidebar::persist_width(cx);
            cx.notify();
        }
    }

    fn render_shell_slot_cluster(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .absolute()
            .top_0()
            .right_0()
            .h(px(Layout::MAIN_HEADER_HEIGHT))
            .pr(px(Layout::TOOLBAR_TRAILING_INSET))
            .flex()
            .items_center()
            .occlude()
            .child(
                div()
                    .w(px(
                        3. * Layout::TITLEBAR_CONTROL_SIZE + 2. * Layout::TITLEBAR_CONTROL_GAP
                    ))
                    .h(px(Layout::TITLEBAR_CONTROL_SIZE))
                    .child(self.render_shell_chrome(shell_chrome::Kind::Slots, window, cx)),
            )
            .into_any_element()
    }

    pub(crate) fn main_header_title(&self, cx: &App) -> String {
        let thread = cx.global::<OpenedThread>().0.clone();
        let Some(thread) = thread else {
            return vega_i18n::t("app.task.create").to_string();
        };
        if self.is_draft_route(&thread.id) {
            return vega_i18n::t("app.task.create").to_string();
        }
        if thread.title.is_empty() {
            vega_i18n::t("app.task.untitled").to_string()
        } else {
            thread.title.clone()
        }
    }

    fn render_main_header(
        &mut self,
        sidebar_visible: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .id("main-header")
            .h(px(Layout::MAIN_HEADER_HEIGHT))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap_2()
            .relative()
            .border_b_1()
            .border_color(gpui_kit::transparent_black())
            .child(
                div()
                    .absolute()
                    .bottom_0()
                    .left_0()
                    .w_full()
                    .child(nsh_workspace::separator(
                        "nsh-main-header-rule",
                        false,
                        window,
                        cx,
                    )),
            )
            .pr(px(Layout::SHELL_SLOT_CLUSTER_RESERVE))
            .when(!sidebar_visible, |header| {
                header
                    .pl(px(Layout::TITLEBAR_LEADING_INSET))
                    .child(vega_ui::navigation::controls(window, cx, false))
            })
            .when(sidebar_visible, |header| header.pl_3())
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(self.render_shell_chrome(shell_chrome::Kind::Header, window, cx)),
            )
            .into_any_element()
    }

    fn toggle_sidebar(&mut self, _: &ToggleSidebar, window: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        if self.auto_collapsed(window, cx) {
            vega_ui::sidebar::show_persisted(cx);
        } else {
            vega_ui::sidebar::toggle_persisted(cx);
        }
    }
}
