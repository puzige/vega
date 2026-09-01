use super::*;

/// Quick-template placeholder labels for the empty state (ui-spec §4.6);
/// intentionally inert until the template feature lands (A7-02).
pub(crate) const EMPTY_STATE_TEMPLATES: [&str; 3] = ["快捷模板 1", "快捷模板 2", "快捷模板 3"];

impl Render for VegaWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Palette comes from the global theme so Cmd+Shift+L repaints instantly.
        let colors = theme(cx).colors;
        // Effective visibility: the user preference (Cmd+B, persisted) AND the
        // viewport auto-collapse rule (ui-spec §1).
        let sidebar_visible = !cx.global::<SidebarCollapsed>().0 && !self.auto_collapsed(window);
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
        let content: AnyElement = if settings_open {
            self.cancel_active_agent(cx);
            // 设置视图：缓存 Entity，避免主题刷新等重渲染时重建导致表单输入丢失。
            if self.settings_view.is_none() {
                let settings = cx.new(SettingsView::new);
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
                let projection = self.pricing_controller.projection();
                settings.update(cx, |settings, cx| {
                    settings.apply_pricing_projection(projection, cx);
                });
                self.settings_view = Some(settings);
            }
            match &self.settings_view {
                Some(settings) => settings.clone().into_any_element(),
                None => div().size_full().bg(colors.bg_base).into_any_element(),
            }
        } else {
            // 设置已关闭：丢弃缓存，下次打开时重新构造并载入最新配置。
            self.settings_view = None;
            match cx.global::<OpenedThread>().0.clone() {
                Some(thread) => {
                    if let Some(diff_view) = self.diff_controller.visible_view(&thread) {
                        let should_focus = self
                            .diff_controller
                            .active
                            .as_ref()
                            .is_some_and(|active| active.focus_pending);
                        if should_focus {
                            let focus = diff_view.read(cx).focus_handle(cx);
                            window.focus(&focus, cx);
                            if let Some(active) = self.diff_controller.active.as_mut() {
                                active.focus_pending = false;
                            }
                        }
                        return div()
                            .size_full()
                            .flex()
                            .flex_row()
                            .relative()
                            .bg(colors.bg_base)
                            .text_color(colors.text_primary)
                            .when(sidebar_visible, |row| row.child(self.sidebar.clone()))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .h_full()
                                    .overflow_hidden()
                                    .child(diff_view),
                            )
                            .children(pending_delete.map(|thread| {
                                render_delete_confirm_overlay(&thread, self.sidebar.clone(), colors)
                            }));
                    }
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
                            // A2-14: seed the composer model selector from the
                            // configured providers (zero IO from the view) and
                            // reflect the thread's current model if present.
                            {
                                let model_catalog = vega_store::config::load()
                                    .map_or(Vec::new(), |config| all_models(&config.providers));
                                let thread_model = thread.model.clone();
                                view.update(cx, |stream, cx| {
                                    if !thread_model.is_empty() {
                                        stream.apply_composer_defaults(
                                            vega_conversation::types::ComposerDefaults {
                                                model: thread_model,
                                                thinking: String::new(),
                                            },
                                            cx,
                                        );
                                    }
                                    stream.apply_model_options(model_catalog, cx);
                                });
                            }
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
                            self.stream_view = Some((thread.id.clone(), view.clone()));
                            view
                        }
                    };
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
                    stream.into_any_element()
                }
                None => {
                    if let Some((_, previous)) = self.stream_view.take() {
                        self.cancel_active_agent(cx);
                        previous.update(cx, |stream, cx| stream.timeout_permission(cx));
                    }
                    render_empty_state(colors)
                }
            }
        };

        div()
            .size_full()
            .flex()
            .flex_row()
            .relative()
            .bg(colors.bg_base)
            .text_color(colors.text_primary)
            .when(sidebar_visible, |row| row.child(self.sidebar.clone()))
            .child(
                // Content column host: settings brings its own 820px column,
                // the empty state is centered by its own layout.
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_hidden()
                    .child(content),
            )
            // T13 删除确认弹层：最后绘制以覆盖全窗口；遮罩点击 / Esc 取消。
            .children(
                pending_delete.map(|thread| {
                    render_delete_confirm_overlay(&thread, self.sidebar.clone(), colors)
                }),
            )
    }
}

/// The content-area empty state (ui-spec §4.6): centered guidance with inert
/// quick-template placeholder buttons, inside the 820px content column —
/// no large logo illustration. The temporary T10/T11 entry buttons were
/// retired in T12 (projects/sessions now live in the sidebar).
pub(crate) fn render_empty_state(colors: ThemeColors) -> AnyElement {
    div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .child(
            div()
                .w_full()
                .max_w(px(CONTENT_MAX_WIDTH))
                .px(px(CONTENT_MIN_PADDING))
                .flex()
                .flex_col()
                .items_center()
                .gap_3()
                .child(
                    div()
                        .text_size(px(Typography::HEADING_PAGE))
                        .font_weight(Typography::HEADING_PAGE_WEIGHT)
                        .child("✦ Vega"),
                )
                .child(
                    div()
                        .text_size(px(Typography::BODY))
                        .text_color(colors.text_secondary)
                        .child("开始一个新会话"),
                )
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .children(EMPTY_STATE_TEMPLATES.map(|label| {
                            div()
                                .px_3()
                                .py_1()
                                .rounded_md()
                                .border_1()
                                .border_color(colors.border_subtle)
                                .bg(colors.bg_elevated)
                                .text_size(px(Typography::SIDEBAR))
                                .text_color(colors.text_secondary)
                                .child(label)
                        })),
                ),
        )
        .into_any_element()
}
