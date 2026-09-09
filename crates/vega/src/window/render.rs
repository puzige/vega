use super::*;
use vega_ui::icons::{Icon, icon_button};

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
            let navigation_error = cx
                .try_global::<vega_ui::navigation::NavigationState>()
                .and_then(|state| state.error);
            let main = div()
                .size_full()
                .min_h_0()
                .flex()
                .flex_col()
                .child(self.render_main_header(sidebar_visible, window, cx))
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
            .bg(colors.bg_base)
            .text_color(colors.text_primary)
            .when(sidebar_visible && !cx.global::<SettingsOpen>().0, |row| {
                row.child(self.sidebar.clone())
                    .child(self.render_sidebar_resizer(colors, cx))
            })
            .child(
                // R21 uses a flat split shell. Settings brings its own full-height
                // navigation rail and content column; conversation/empty routes
                // share this flush white surface.
                div()
                    .debug_selector(|| "main-content-panel".into())
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
                    .child(div().flex_1().min_h_0().child(content)),
            )
            .on_mouse_move(cx.listener(Self::resize_sidebar))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::finish_sidebar_resize))
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
    fn render_sidebar_resizer(&self, colors: ThemeColors, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("sidebar-resize-handle")
            .debug_selector(|| "sidebar-resize-handle".into())
            .w(px(Layout::SIDEBAR_RESIZE_HIT_AREA))
            .h_full()
            .flex_shrink_0()
            .flex()
            .justify_center()
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

    fn render_main_header(
        &mut self,
        sidebar_visible: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        let thread = cx.global::<OpenedThread>().0.clone();
        let title = thread.as_ref().map_or_else(
            || "新建任务".to_string(),
            |thread| {
                if thread.title.is_empty() {
                    "未命名任务".to_string()
                } else {
                    thread.title.clone()
                }
            },
        );
        let project_label = self.shell_project_label(cx);
        let has_project_label = project_label.is_some();
        let project_route = self.shell_project_id(cx).is_some();
        let review_available = self.shell_project_thread(cx).is_some();
        let right_visible = self.persistent_right_workspace_visible(window, cx);
        let environment_visible = if self.environment_is_wide(window, cx) {
            !self.environment_collapsed
        } else {
            self.environment_overlay_open
        };

        let mut actions = div().flex_shrink_0().flex().items_center().gap_1();
        if review_available {
            actions = actions.child(header_action(
                "main-header-review",
                "Review",
                Icon::Split,
                true,
                colors,
                cx.listener(|this, _, _, cx| {
                    this.environment_overlay_open = false;
                    this.workspace_open_diff(cx);
                    cx.notify();
                }),
            ));
        }
        if project_route {
            actions = actions.child(
                icon_button(
                    Icon::Terminal,
                    "切换终端",
                    colors,
                    cx.listener(|this, _, window, cx| {
                        this.environment_overlay_open = false;
                        this.workspace_toggle_terminal(window, cx)
                    }),
                )
                .debug_selector(|| "main-header-terminal".into()),
            );
        }
        for (index, label, icon, selector) in [
            (
                0,
                "显示右侧面板",
                Icon::DockRight,
                "main-header-restore-right",
            ),
            (
                1,
                "显示底部面板",
                Icon::DockBottom,
                "main-header-restore-bottom",
            ),
        ] {
            if self.hidden_workspace_available(index) {
                actions = actions.child(
                    icon_button(
                        icon,
                        label,
                        colors,
                        cx.listener(move |this, _, window, cx| {
                            this.restore_hidden_workspace(index, window, cx)
                        }),
                    )
                    .debug_selector(move || selector.into()),
                );
            }
        }
        if project_route && !right_visible {
            actions = actions.child(
                icon_button(
                    Icon::DockRight,
                    if environment_visible {
                        "隐藏 Environment"
                    } else {
                        "显示 Environment"
                    },
                    colors,
                    cx.listener(|this, _, window, cx| this.toggle_environment(window, cx)),
                )
                .debug_selector(|| "main-header-environment".into())
                .when(environment_visible, |button| button.bg(colors.bg_active)),
            );
        }

        div()
            .id("main-header")
            .debug_selector(|| "main-header".into())
            .h(px(Layout::MAIN_HEADER_HEIGHT))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap_2()
            .border_b_1()
            .border_color(colors.border_subtle)
            .pr_3()
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
                    .flex()
                    .items_center()
                    .gap_2()
                    .children(project_label.map(|label| {
                        div()
                            .debug_selector(|| "main-header-project".into())
                            .min_w_0()
                            .max_w(px(180.))
                            .flex()
                            .items_center()
                            .gap_1()
                            .text_size(px(Typography::SIDEBAR))
                            .text_color(colors.text_secondary)
                            .child(vega_ui::icons::icon(Icon::Folder, colors.text_secondary))
                            .child(div().min_w_0().truncate().child(label))
                    }))
                    .when(has_project_label, |labels| {
                        labels.child(
                            div()
                                .flex_shrink_0()
                                .text_size(px(Typography::METADATA))
                                .text_color(colors.text_tertiary)
                                .child("/"),
                        )
                    })
                    .child(
                        div()
                            .debug_selector(|| "main-header-title".into())
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .text_size(px(Typography::HEADING_PAGE))
                            .font_weight(Typography::HEADING_PAGE_WEIGHT)
                            .text_color(colors.text_primary)
                            .child(title),
                    ),
            )
            .child(actions)
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
                .min_h(px(Layout::COMPOSER_MIN_HEIGHT))
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
                .min_h(px(Layout::COMPOSER_MIN_HEIGHT))
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
            .child(
                div()
                    .flex_1()
                    .w_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .px(px(Layout::CONTENT_PADDING))
                    .child(
                        div()
                            .w_full()
                            .max_w(px(Layout::CONTENT_MAX_WIDTH))
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap_3()
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
                            ),
                    ),
            )
            .child(
                div()
                    .w_full()
                    .px(px(Layout::CONTENT_PADDING))
                    .pt(px(12.))
                    .pb(px(16.))
                    .child(
                        div()
                            .w_full()
                            .max_w(px(Layout::COMPOSER_MAX_WIDTH))
                            .mx_auto()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(start_action)
                            .children(show_sidebar),
                    ),
            )
            .into_any_element()
    }
}

fn header_action(
    id: &'static str,
    label: &'static str,
    icon: Icon,
    show_label: bool,
    colors: ThemeColors,
    activate: impl Fn(&(), &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let activate = std::rc::Rc::new(activate);
    let keyboard = activate.clone();
    div()
        .id(id)
        .debug_selector(move || id.into())
        .aria_label(label)
        .focusable()
        .tab_stop(true)
        .h(px(28.))
        .px_2()
        .rounded_md()
        .flex()
        .items_center()
        .gap_1()
        .text_size(px(Typography::METADATA))
        .text_color(colors.text_secondary)
        .cursor_pointer()
        .hover(move |style| style.bg(colors.bg_hover))
        .focus(move |style| style.bg(colors.bg_active))
        .on_mouse_up(MouseButton::Left, move |_, window, cx| {
            cx.stop_propagation();
            activate(&(), window, cx);
        })
        .on_key_down(move |event, window, cx| {
            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                cx.stop_propagation();
                keyboard(&(), window, cx);
            }
        })
        .child(vega_ui::icons::icon(icon, colors.text_secondary))
        .when(show_label, |button| button.child(label))
}
