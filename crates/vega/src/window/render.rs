use super::*;
use vega_ui::icons::{Icon, shell_icon_button};

impl Render for VegaWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_navigation(cx);
        self.sync_preparation(cx);
        self.sync_completion_attention(window, cx);
        #[cfg(test)]
        if cx
            .try_global::<super::question_wait_tests::QuestionTestShell>()
            .is_some()
            && cx.global::<SettingsOpen>().0
        {
            return div();
        }
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
        let content: AnyElement = if settings_open {
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
                let settings = cx.new(|cx| SettingsView::from_path(config_path, cx));
                settings.update(cx, |view, cx| {
                    view.install_mcp_service(self.mcp_settings.clone(), cx)
                });
                crate::app_usage::bind(&settings, cx);
                settings.update(cx, |view, cx| {
                    view.apply_update_projection(self.updater.state.clone(), cx)
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
                None => div().size_full().bg(colors.bg_base).into_any_element(),
            }
        } else {
            // 设置已关闭：丢弃缓存，下次打开时重新构造并载入最新配置。
            let returning_from_settings = self.settings_view.take().is_some();

            let thread = self.resolve_route_thread(cx);
            self.update_message_location_route(Some(&thread.id));
            let draft_route = self.is_draft_route(&thread.id);
            {
                {
                    let cached = self
                        .agent_controller
                        .active_stream_for_thread(&thread.id)
                        .or_else(|| self.automation_controller.streams.get(&thread.id).cloned())
                        .or_else(|| match &self.stream_view {
                            Some((thread_id, view)) if *thread_id == thread.id => {
                                Some(view.clone())
                            }
                            _ => None,
                        });
                    let stream = match cached {
                        Some(view) => {
                            if self.stream_view.as_ref().is_none_or(|(thread_id, cached)| {
                                thread_id != &thread.id || cached != &view
                            }) {
                                if let Some((previous_id, previous)) = self.stream_view.take() {
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
                                self.stream_view = Some((thread.id.clone(), view.clone()));
                                if !draft_route {
                                    self.load_pending_queue(view.clone(), thread.id.clone(), cx);
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
                                self.apply_reasoning_profile_to_stream(&view, &thread.model, cx);
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
                                    previous.update(cx, |stream, cx| stream.timeout_permission(cx));
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
                            view.update(cx, |stream, cx| {
                                stream.apply_thread(thread.clone(), cx);
                                stream.apply_model_options(model_options, cx);
                            });
                            self.apply_reasoning_profile_to_stream(&view, &thread.model, cx);
                            self.agent_controller.delegated_unbound.remove(&thread.id);
                            self.bind_conversation_stream(&view, window, cx);

                            let initial = if draft_route {
                                None
                            } else {
                                Some(match &cx.global::<VegaStore>().0 {
                                    Ok(store) => (|| {
                                        let hydration =
                                            vega_conversation::history::restart_history_page(
                                                store,
                                                &thread.id,
                                                vega_store::messages::PAGE_LIMIT,
                                            )?;
                                        let plans = vega_conversation::plans::list_plans(
                                            store, &thread.id,
                                        )?;
                                        let history = vega_conversation::threads::composer_history(
                                            store, &thread.id,
                                        )?;
                                        let recovery =
                                            vega_conversation::plans::recoverable_approved_instruction(
                                                store, &thread.id,
                                            )?;

                                        let usage = vega_conversation::threads::thread_usage_seed(
                                            store, &thread.id,
                                        )?;

                                        let summary =
                                            vega_conversation::summary::latest_task_summary(
                                                store, &thread.id, None,
                                            )?;
                                        Ok((hydration, plans, history, recovery, usage, summary))
                                    })(),
                                    Err(error) => {
                                        Err(vega_conversation::types::ConversationError::Store(
                                            error.clone(),
                                        ))
                                    }
                                })
                            };
                            if let Some(initial) = initial {
                                view.update(cx, |stream, cx| match initial {
                                    Ok((hydration, plans, history, recovery, usage, summary)) => {
                                        // Hydrated history lands first so route-open
                                        // plan cards keep their position after it.
                                        stream.apply_history_page(hydration, cx);
                                        for plan in plans {
                                            stream.apply_plan(plan, cx);
                                        }
                                        stream.apply_composer_history(&thread.id, history, cx);
                                        if let Some(summary) = summary {
                                            stream.apply_task_summary(summary, cx);
                                        }
                                        if recovery.is_some() {
                                            stream.apply_approved_not_started(cx);
                                        }
                                        stream.restore_meter(usage, cx);
                                    }
                                    Err(_) => stream.apply_controller_error(cx),
                                });
                            }
                            self.stream_view = Some((thread.id.clone(), view.clone()));
                            if !draft_route {
                                self.load_pending_queue(view.clone(), thread.id.clone(), cx);
                            }
                            if let Some(anchor) = self.thread_scroll_states.get(&thread.id) {
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
                        .filter(|active| active.focus_pending && active.identity.stream == stream)
                        .map(|active| active.identity.panel.read(cx).focus_handle(cx));
                    if let Some(focus) = commit_focus {
                        window.focus(&focus, cx);
                        if let Some(active) = self.commit_controller.active.as_mut() {
                            active.focus_pending = false;
                        }
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
        };

        self.sync_subagent_panel_route(cx);
        let content = if settings_open {
            content
        } else {
            let navigation_error = cx
                .try_global::<vega_ui::navigation::NavigationState>()
                .and_then(|state| state.error);
            let main = div()
                .size_full()
                .min_h_0()
                .flex()
                .flex_col()
                .child(self.render_main_header(sidebar_visible, cx))
                .children(navigation_error.map(|error| {
                    div()
                        .px_3()
                        .py_1()
                        .text_size(px(Typography::METADATA))
                        .text_color(colors.text_secondary)
                        .child(error)
                }))
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
            .bg(colors.bg_base)
            .text_color(colors.text_primary)
            .child(gpui_kit::base::TextSelectionLayer)
            .when(sidebar_visible && !cx.global::<SettingsOpen>().0, |row| {
                row.child(self.sidebar.clone())
                    .child(self.render_sidebar_resizer(colors, cx))
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .bg(colors.bg_base)
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .children(
                        settings_open
                            .then(|| {
                                cx.try_global::<vega_ui::navigation::NavigationState>()
                                    .and_then(|state| state.error)
                                    .map(|error| {
                                        div()
                                            .px_3()
                                            .py_1()
                                            .text_size(px(Typography::METADATA))
                                            .text_color(colors.text_secondary)
                                            .child(error)
                                    })
                            })
                            .flatten(),
                    )
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
    fn render_sidebar_resizer(&self, colors: ThemeColors, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("sidebar-resize-handle")
            .w(px(Layout::SIDEBAR_RESIZE_HIT_AREA))
            .h_full()
            .flex_shrink_0()
            .flex()
            .justify_end()
            .bg(colors.bg_sidebar)
            .cursor_col_resize()
            .child(div().w(px(1.)).h_full().bg(colors.border_subtle))
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
        let colors = theme(cx).colors;
        let project_route = self.shell_project_id(cx).is_some();
        let environment_available = self.environment_available(cx);
        let right_visible = self.persistent_right_workspace_visible(window, cx);
        let environment_visible = if self.environment_is_wide(window, cx) {
            !self.environment_collapsed
        } else {
            self.environment_overlay_open
        };
        div()
            .absolute()
            .top_0()
            .right_0()
            .h(px(Layout::MAIN_HEADER_HEIGHT))
            .pr(px(Layout::TOOLBAR_TRAILING_INSET))
            .flex()
            .items_center()
            .gap(px(6.))
            .occlude()
            .child(shell_icon_button(
                Icon::Summary,
                "切换环境",
                None,
                environment_available
                    && !right_visible
                    && self.workspace_fullscreen_index().is_none()
                    && environment_visible,
                environment_available && !right_visible,
                colors,
                cx.listener(|this, _, window, cx| this.toggle_environment(window, cx)),
            ))
            .child(shell_icon_button(
                Icon::DockBottom,
                "切换终端",
                Some("⌘J".into()),
                self.workspace_recent_terminal_is_rendered(window, cx),
                project_route,
                colors,
                cx.listener(|this, _, window, cx| this.workspace_toggle_bottom(window, cx)),
            ))
            .child(
                div()
                    .capture_any_mouse_down(|_, window, _| window.prevent_default())
                    .child(shell_icon_button(
                        Icon::DockRight,
                        "切换右侧面板",
                        None,
                        self.right_workspace_rendered_non_terminal(window, cx),
                        self.right_workspace_slot_available(window, cx),
                        colors,
                        cx.listener(|this, _, window, cx| this.workspace_toggle_right(window, cx)),
                    )),
            )
            .into_any_element()
    }

    pub(crate) fn main_header_title(&self, cx: &App) -> String {
        let thread = cx.global::<OpenedThread>().0.clone();
        let Some(thread) = thread else {
            return "新建任务".to_string();
        };
        if self.is_draft_route(&thread.id) {
            return "新建任务".to_string();
        }
        if thread.title.is_empty() {
            "未命名任务".to_string()
        } else {
            thread.title.clone()
        }
    }

    fn render_main_header(&mut self, sidebar_visible: bool, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let title = self.main_header_title(cx);
        div()
            .id("main-header")
            .h(px(Layout::MAIN_HEADER_HEIGHT))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap_2()
            .border_b_1()
            .border_color(colors.border_subtle)
            .pr(px(Layout::SHELL_SLOT_CLUSTER_RESERVE))
            .when(!sidebar_visible, |header| {
                header
                    .pl(px(Layout::TITLEBAR_LEADING_INSET))
                    .child(vega_ui::navigation::controls(cx, false))
            })
            .when(sidebar_visible, |header| header.pl_3())
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .truncate()
                    .text_size(px(Typography::HEADING_PAGE))
                    .font_weight(Typography::HEADING_PAGE_WEIGHT)
                    .text_color(colors.text_primary)
                    .child(title),
            )
            .when(
                !self.update_notice_dismissed
                    && matches!(
                        self.updater.state.phase,
                        UpdatePhase::Ready | UpdatePhase::Available
                    ),
                |header| {
                    header.child(
                        div()
                            .id("vega-update-notice")
                            .focusable()
                            .tab_stop(true)
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .cursor_pointer()
                            .text_size(px(Typography::BODY))
                            .text_color(colors.brand_primary)
                            .hover(move |style| style.bg(colors.bg_hover))
                            .child(if self.updater.state.phase == UpdatePhase::Ready {
                                "更新已就绪"
                            } else {
                                "发现新版本"
                            })
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| {
                                    cx.set_global(SettingsOpen(true));
                                    cx.set_global(PricingSettingsRequested(false));
                                    if let Some(view) = &this.settings_view {
                                        view.update(cx, |view, cx| view.show_general(cx));
                                    }
                                    cx.notify();
                                }),
                            )
                            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                    cx.set_global(SettingsOpen(true));
                                    cx.set_global(PricingSettingsRequested(false));
                                    if let Some(view) = &this.settings_view {
                                        view.update(cx, |view, cx| view.show_general(cx));
                                    }
                                    cx.stop_propagation();
                                    cx.notify();
                                }
                            })),
                    )
                },
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
