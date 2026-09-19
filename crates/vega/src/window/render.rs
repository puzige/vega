use super::*;
use vega_ui::icons::{Icon, shell_icon_button};

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
            // R69 R1/R4: the home route (no opened thread) installs — or
            // reuses — the window's single unpersisted draft thread and
            // renders the real ConversationStream over it. `OpenedThread`
            // becomes `Some(draft)`, so the route identity is a task route
            // everywhere downstream while the draft stays a home route in the
            // navigation history (`Projection::route`).
            let thread = self.resolve_route_thread(cx);
            let draft_route = self.is_draft_route(&thread.id);
            {
                {
                    // S3-T17：会话流视图（每线程一个实体，切换会话时重建；
                    // MarkdownStream 内存态构造，不落库）。
                    let cached = match &self.stream_view {
                        Some((thread_id, view)) if *thread_id == thread.id => Some(view.clone()),
                        _ => None,
                    };
                    let stream = match cached {
                        Some(view) => {
                            // A8-01: the same draft id intentionally keeps
                            // the same stream (and Composer input/focus), but
                            // project selection may have changed around it.
                            // Only the window-owned unpersisted draft may
                            // cross that project boundary in place.
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
                            if let Some((_, previous)) = self.stream_view.take() {
                                self.cancel_active_agent(cx);
                                previous.update(cx, |stream, cx| stream.timeout_permission(cx));
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
                                this.persist_composer_thinking(stream.clone(), request, cx);
                            })
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
                            // R69 R7: a draft has no durable history by
                            // definition, so the whole hydration block is
                            // skipped. Running it would surface a visible
                            // controller error — `recoverable_approved_instruction`
                            // returns `NotFound` for a row that does not exist
                            // and the `?` chain turns that into the error bar.
                            // The empty state is applied directly instead.
                            let initial = if draft_route {
                                None
                            } else {
                                Some(match &cx.global::<VegaStore>().0 {
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
                            self.restore_navigation_draft(&thread.id, &view, cx);
                            self.stream_view = Some((thread.id.clone(), view.clone()));
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
                    // R69/R6 correction: the draft still has no durable row,
                    // so artifact access remains excluded. Branch access is
                    // different: `artifact_project_root` resolves the
                    // selected project row, and the existing branch
                    // controller can therefore list/switch its repository
                    // without materializing the draft. Standalone drafts are
                    // rejected by `ensure_branch_route` itself.
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
                        stream.update(cx, |stream, cx| stream.focus_composer(window, cx));
                    }
                    stream.into_any_element()
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
            // R46 §2.1: the shell slots belong to the window, not to a column.
            // They paint after every sidebar/rail/pane so they stay on top,
            // and before the palette and the delete-confirm overlay so those
            // full-window surfaces keep their precedence. Maximizing a dock
            // keeps them mounted (its x must not change); that pane's header
            // reserves the cluster's trailing band per §2.1.1. Settings keeps
            // its own full-height chrome and hides the shell header entirely.
            .children((!settings_open).then(|| self.render_shell_slot_cluster(window, cx)))
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

    /// R46 §2.1: the three shell slots are a window-level trailing cluster.
    /// They pin to the window's top-right corner, outside every column, rail
    /// and pane, so no panel state can move their x coordinate. The cluster
    /// keeps R45's frozen internals: three 28×28 controls, 6px gaps, fixed
    /// Environment → Terminal(⌘J) → Right order, and disabled slots keeping
    /// their grid. Vertical placement centers it in the 46px main header band,
    /// on the Codex `--padding-toolbar` trailing inset (8px) that the
    /// `SHELL_SLOT_CLUSTER_RESERVE` token also builds on.
    fn render_shell_slot_cluster(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        let project_route = self.shell_project_id(cx).is_some();
        let right_visible = self.persistent_right_workspace_visible(window, cx);
        let environment_visible = if self.environment_is_wide(window, cx) {
            !self.environment_collapsed
        } else {
            self.environment_overlay_open
        };
        div()
            .debug_selector(|| "main-header-shell-slots".into())
            .absolute()
            .top_0()
            .right_0()
            .h(px(Layout::MAIN_HEADER_HEIGHT))
            .pr(px(Layout::TOOLBAR_TRAILING_INSET))
            .flex()
            .items_center()
            .gap(px(6.))
            // The cluster is window chrome and owns its pixels. Anchoring it to
            // the window's top-right corner puts it over the right pane's own
            // trailing controls, and without this a press on a *disabled* slot
            // would fall through and activate whatever sits underneath instead
            // of staying inert (R45: activation ignored, grid kept). It also
            // keeps the narrow Environment overlay's full-window dismissal
            // backdrop from racing slot 1's own toggle.
            .occlude()
            .child(
                shell_icon_button(
                    Icon::Summary,
                    "切换环境",
                    None,
                    environment_visible,
                    project_route && !right_visible,
                    colors,
                    cx.listener(|this, _, window, cx| this.toggle_environment(window, cx)),
                )
                .debug_selector(|| "main-header-environment".into()),
            )
            .child(
                shell_icon_button(
                    Icon::DockBottom,
                    "切换终端",
                    Some("⌘J".into()),
                    self.workspace_recent_terminal_is_rendered(window, cx),
                    project_route,
                    colors,
                    cx.listener(|this, _, window, cx| this.workspace_toggle_bottom(window, cx)),
                )
                .debug_selector(|| "main-header-terminal".into()),
            )
            .child(
                shell_icon_button(
                    Icon::DockRight,
                    "切换右侧面板",
                    None,
                    self.right_workspace_rendered_non_terminal(window, cx),
                    self.right_workspace_slot_available(window, cx),
                    colors,
                    cx.listener(|this, _, window, cx| this.workspace_toggle_right(window, cx)),
                )
                .debug_selector(|| "main-header-workspace-right".into()),
            )
            .into_any_element()
    }

    /// R12/R69: the main header's title for the current route.
    ///
    /// The draft route keeps the pre-R69 home title. A draft has an empty
    /// title by construction, so without this branch the header would read
    /// `未命名任务` on the page the user has not started yet.
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
            .debug_selector(|| "main-header".into())
            .h(px(Layout::MAIN_HEADER_HEIGHT))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap_2()
            .border_b_1()
            .border_color(colors.border_subtle)
            // R46: the window-anchored slot cluster no longer renders inside
            // this element, so reserve its exact trailing width (3×28px slots
            // on 2×6px gaps plus the shared 8px toolbar trailing inset) to
            // keep the title clear of it. No negative margin and no coordinate
            // special case. The main header hosts no local actions, so it does
            // not add the pane-header ownership gutter (R47 §2.1).
            .pr(px(Layout::SHELL_SLOT_CLUSTER_RESERVE))
            .when(!sidebar_visible, |header| {
                header
                    .pl(px(Layout::TITLEBAR_LEADING_INSET))
                    .child(vega_ui::navigation::controls(cx, false))
            })
            .when(sidebar_visible, |header| header.pl_3())
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
