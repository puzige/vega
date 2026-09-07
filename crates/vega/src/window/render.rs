use super::*;

impl Render for VegaWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_navigation(cx);
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
        // T13 delete confirmation overlay: rendered above everything (window
        // root, absolute) while a delete is pending (裁决②).
        let pending_delete = cx.global::<PendingDeleteConfirm>().0.clone();

        // Settings opens inside the content area (T09 layout change of the
        // T08 view switching): the sidebar stays visible unless collapsed.
        // 路由收敛（T12 + T17）：内容区 = 设置 or 会话流 or §4.6 空态。
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
            self.cancel_active_agent(cx);
            self.start_model_catalog_load(cx);
            // 设置视图：缓存 Entity，避免主题刷新等重渲染时重建导致表单输入丢失。
            if self.settings_view.is_none() {
                let config_path = self.composer_config_path();
                let settings = cx.new(|cx| SettingsView::from_path(config_path, cx));
                crate::app_usage::bind(&settings, cx);
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
                cx.subscribe(&settings, |this, _, _: &SettingsSaved, cx| {
                    this.on_settings_saved(cx);
                })
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
                let projection = self.pricing_controller.projection();
                let reasoning_projection = self.reasoning_settings_projection();
                settings.update(cx, |settings, cx| {
                    settings.apply_pricing_projection(projection, cx);
                    settings.apply_reasoning_projection(reasoning_projection, cx);
                });
                let focus = settings.read(cx).focus_handle(cx);
                window.focus(&focus, cx);
                self.settings_view = Some(settings);
            }
            match &self.settings_view {
                Some(settings) => settings.clone().into_any_element(),
                None => div().size_full().bg(colors.bg_base).into_any_element(),
            }
        } else {
            // 设置已关闭：丢弃缓存，下次打开时重新构造并载入最新配置。
            let returning_from_settings = self.settings_view.take().is_some();
            match cx.global::<OpenedThread>().0.clone() {
                Some(thread) => {
                    // S3-T17：会话流视图（每线程一个实体，切换会话时重建；
                    // MarkdownStream 内存态构造，不落库）。
                    let cached = match &self.stream_view {
                        Some((thread_id, view)) if *thread_id == thread.id => Some(view.clone()),
                        _ => None,
                    };
                    let stream = match cached {
                        Some(view) => view,
                        None => {
                            if let Some((_, previous)) = self.stream_view.take() {
                                self.cancel_active_agent(cx);
                                previous.update(cx, |stream, cx| stream.timeout_permission(cx));
                            }
                            let view = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
                            if let Some(label) =
                                self.sidebar.read(cx).project_label(&thread.project_id, cx)
                            {
                                view.update(cx, |stream, cx| stream.set_project_label(label, cx));
                            }
                            // A2-14/R1: this frame only projects the durable
                            // thread and the already loaded in-memory model
                            // catalog. Config IO is scheduled below on a
                            // worker, so render never reads config.toml.
                            let model_options = self.model_options_for_pricing();
                            view.update(cx, |stream, cx| {
                                stream.apply_thread(thread.clone(), cx);
                                stream.apply_model_options(model_options, cx);
                            });
                            self.apply_reasoning_profile_to_stream(&view, &thread.model, cx);
                            cx.subscribe(&view, |this, stream, request, cx| {
                                this.apply_thread_model_selection(stream.clone(), request, cx);
                            })
                            .detach();
                            cx.subscribe(&view, |this, stream, request, cx| {
                                this.persist_composer_defaults(stream.clone(), request, cx);
                            })
                            .detach();
                            cx.subscribe(&view, |this, stream, request, cx| {
                                this.persist_thread_settings(stream.clone(), request, cx);
                            })
                            .detach();
                            cx.subscribe(&view, |this, stream, request, cx| {
                                this.review_plan(stream.clone(), request, cx);
                            })
                            .detach();
                            cx.subscribe(&view, |this, stream, request, cx| {
                                this.submit_composer(stream.clone(), request, cx);
                            })
                            .detach();
                            cx.subscribe(
                                &view,
                                |this, stream, request: &ComposerStopRequested, cx| {
                                    this.stop_composer(stream.clone(), request, cx);
                                },
                            )
                            .detach();
                            cx.subscribe(
                                &view,
                                |this, stream, request: &FileIndexRequested, cx| {
                                    this.request_file_index(stream.clone(), request, cx);
                                },
                            )
                            .detach();
                            cx.subscribe(
                                &view,
                                |this, stream, request: &FileIndexCancelled, cx| {
                                    this.cancel_file_index(stream.clone(), request, cx);
                                },
                            )
                            .detach();
                            cx.subscribe(
                                &view,
                                |this, stream, request: &FileIndexRetryRequested, cx| {
                                    this.retry_file_index(stream.clone(), request, cx);
                                },
                            )
                            .detach();
                            cx.subscribe(&view, |this, stream, request, cx| {
                                this.open_workspace_diff(stream.clone(), request, cx);
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
                            let initial = match &cx.global::<VegaStore>().0 {
                                Ok(store) => (|| {
                                    // S8-T45/C7: the controller is rebuilt first,
                                    // one repair pass normalizes rows the killed
                                    // process left incomplete, and only then is
                                    // the newest durable page projected.
                                    let hydration =
                                        vega_conversation::history::restart_history_page(
                                            store,
                                            &thread.id,
                                            vega_store::messages::PAGE_LIMIT,
                                        )?;
                                    let plans =
                                        vega_conversation::plans::list_plans(store, &thread.id)?;
                                    let history = vega_conversation::threads::composer_history(
                                        store, &thread.id,
                                    )?;
                                    let recovery =
                                        vega_conversation::plans::recoverable_approved_instruction(
                                            store, &thread.id,
                                        )?;
                                    // S7-T39/C4: the calibrated counter baseline
                                    // comes from the conversation checked aggregate
                                    // query exactly once per route open; the meter
                                    // itself never touches SQLite afterwards.
                                    let usage = vega_conversation::threads::thread_usage_seed(
                                        store, &thread.id,
                                    )?;
                                    // S7-T40 restart recovery: token/cost/cache/
                                    // tool count re-project from the durable
                                    // audits; duration stays `—` (no finished
                                    // timestamp in `messages`, C4). The hydrated
                                    // page carries the same summary reference and
                                    // first-wins dedup keeps exactly one card.
                                    let summary = vega_conversation::summary::latest_task_summary(
                                        store, &thread.id, None,
                                    )?;
                                    Ok((hydration, plans, history, recovery, usage, summary))
                                })(),
                                Err(error) => {
                                    Err(vega_conversation::types::ConversationError::Store(
                                        error.clone(),
                                    ))
                                }
                            };
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
                            self.restore_navigation_draft(&thread.id, &view, cx);
                            self.stream_view = Some((thread.id.clone(), view.clone()));
                            self.sync_navigation(cx);
                            self.start_model_catalog_load(cx);
                            view
                        }
                    };
                    // Settings close invalidates the catalog even when this
                    // thread entity remains cached; restart the worker on
                    // the next visible frame so newly saved providers/models
                    // become selectable without rebuilding the session.
                    self.start_model_catalog_load(cx);
                    self.ensure_artifact_route(&thread, stream.clone(), cx);
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
                        stream.update(cx, |stream, cx| stream.focus_composer(window, cx));
                    }
                    stream.into_any_element()
                }
                None => {
                    if let Some((_, previous)) = self.stream_view.take() {
                        self.cancel_active_agent(cx);
                        previous.update(cx, |stream, cx| stream.timeout_permission(cx));
                    }
                    self.render_empty_state(!sidebar_visible, colors, cx)
                }
            }
        };

        let content = if settings_open {
            content
        } else {
            self.render_workspace(content, window, cx)
        };

        if let Some(palette) = &self.palette.view {
            let focus = palette.read(cx).focus_handle(cx);
            if !focus.is_focused(window) {
                window.focus(&focus, cx);
            }
        }
        self.sync_navigation(cx);
        div()
            .key_context("VegaWindow")
            .on_action(cx.listener(Self::toggle_sidebar))
            .on_action(cx.listener(Self::open_palette))
            .on_action(cx.listener(Self::palette_open_workspace))
            .on_action(cx.listener(Self::palette_toggle_terminal))
            .size_full()
            .flex()
            .flex_row()
            .relative()
            .bg(colors.bg_sidebar)
            .text_color(colors.text_primary)
            .when(sidebar_visible && !cx.global::<SettingsOpen>().0, |row| {
                row.child(self.sidebar.clone())
            })
            .child(
                // Content column host: settings brings its own 820px column,
                // the empty state is centered by its own layout.
                div()
                    .flex_1()
                    .min_w_0()
                    .my(px(4.))
                    .mr(px(4.))
                    .rounded(px(vega_theme::Layout::PANEL_RADIUS))
                    .border_1()
                    .border_color(colors.border_subtle)
                    .bg(colors.bg_base)
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .when(!sidebar_visible || settings_open, |column| {
                        column.child(
                            div()
                                .pl(px(Layout::TITLEBAR_LEADING_INSET))
                                .child(vega_ui::navigation::controls(cx, sidebar_visible)),
                        )
                    })
                    .children(
                        cx.try_global::<vega_ui::navigation::NavigationState>()
                            .and_then(|state| state.error)
                            .map(|error| {
                                div()
                                    .px_3()
                                    .py_1()
                                    .text_size(px(Typography::METADATA))
                                    .text_color(colors.text_secondary)
                                    .child(error)
                            }),
                    )
                    .child(div().flex_1().min_h_0().child(content)),
            )
            .children(self.palette.view.clone())
            // T13 删除确认弹层：最后绘制以覆盖全窗口；遮罩点击 / Esc 取消。
            .children(
                pending_delete.map(|thread| {
                    render_delete_confirm_overlay(&thread, self.sidebar.clone(), colors)
                }),
            )
    }
}

impl VegaWindow {
    fn toggle_sidebar(&mut self, _: &ToggleSidebar, window: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        if self.auto_collapsed(window, cx) {
            vega_ui::sidebar::show_persisted(cx);
        } else {
            vega_ui::sidebar::toggle_persisted(cx);
        }
    }

    fn empty_new_thread_clicked(
        &mut self,
        _: &MouseUpEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_new_thread(window, cx);
    }

    fn empty_add_project_clicked(
        &mut self,
        _: &MouseUpEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sidebar.update(cx, Sidebar::open_project_picker);
    }

    fn empty_show_sidebar_clicked(
        &mut self,
        _: &MouseUpEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.dispatch_action(Box::new(ToggleSidebar), cx);
    }

    /// The content-area empty state (ui-spec §4.6): a quiet prompt with the
    /// real route actions needed to start. Existing-project and no-project
    /// states intentionally use different copy, while the action handlers
    /// stay on Sidebar/VegaWindow's production paths.
    fn render_empty_state(
        &mut self,
        sidebar_hidden: bool,
        colors: ThemeColors,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let has_project = cx.global::<SelectedProject>().0.is_some();
        let title = if has_project {
            "今天想做些什么？"
        } else {
            "先添加一个项目"
        };
        let description = if has_project {
            "选择新建任务即可开始；也可以从侧栏切换项目。"
        } else {
            "添加一个文件夹后，就可以创建任务并开始工作。"
        };
        let start_action = if has_project {
            div()
                .w_full()
                .h(px(106.))
                .p_3()
                .rounded(px(Layout::COMPOSER_RADIUS))
                .border_1()
                .border_color(colors.border_subtle)
                .shadow_sm()
                .bg(colors.bg_elevated)
                .text_color(colors.text_secondary)
                .text_size(px(Typography::SIDEBAR))
                .cursor_pointer()
                .hover(move |style| style.bg(colors.bg_hover))
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(Self::empty_new_thread_clicked),
                )
                .child("新建任务并开始输入…")
                .into_any_element()
        } else {
            div()
                .w_full()
                .h(px(106.))
                .p_3()
                .rounded(px(Layout::COMPOSER_RADIUS))
                .border_1()
                .border_color(colors.border_subtle)
                .shadow_sm()
                .bg(colors.bg_elevated)
                .text_color(colors.text_secondary)
                .text_size(px(Typography::SIDEBAR))
                .cursor_pointer()
                .hover(move |style| style.bg(colors.bg_hover))
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(Self::empty_add_project_clicked),
                )
                .child("添加项目文件夹以开始…")
                .into_any_element()
        };
        let show_sidebar = (!has_project && sidebar_hidden).then(|| {
            div()
                .px_3()
                .py_1()
                .rounded_md()
                .text_size(px(Typography::SIDEBAR))
                .text_color(colors.text_secondary)
                .cursor_pointer()
                .hover(move |style| style.bg(colors.bg_hover))
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(Self::empty_show_sidebar_clicked),
                )
                .child("显示侧栏")
                .into_any_element()
        });
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .child(
                div()
                    .w_full()
                    .max_w(px(Layout::CONTENT_MAX_WIDTH))
                    .px(px(Layout::CONTENT_PADDING))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(32.))
                    .child(
                        div()
                            .text_size(px(Typography::EMPTY_STATE_TITLE))
                            .font_weight(Typography::EMPTY_STATE_TITLE_WEIGHT)
                            .text_color(colors.text_primary)
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(px(Typography::METADATA))
                            .text_color(colors.text_secondary)
                            .child(description),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .w_full()
                            .gap_2()
                            .child(start_action)
                            .children(show_sidebar),
                    ),
            )
            .into_any_element()
    }
}
