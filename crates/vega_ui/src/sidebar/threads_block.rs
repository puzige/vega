use super::*;

/// Events emitted by the session block.
pub enum ThreadsBlockEvent {
    /// A thread row was opened; the content column switched to it.
    Opened,
}

impl EventEmitter<ThreadsBlockEvent> for ThreadsBlock {}

/// The 「会话」 block: the selected project's threads, pinned group first,
/// then `updated_at` desc (store ordering, ui-spec §4.1 置顶组优先). Rows =
/// truncated title + relative time ("2h" style); the selected row gets
/// `bg_active` + a 2px accent bar on the left; unread rows render medium
/// weight + a dot (the field stays 0 until S3 produces unread state).
///
/// T13 (A1-05) session management: the main list reads `status = active`
/// only; archived threads hide here and surface in the 「已归档 (N)」
/// collapsed section at the bottom of the block (展开可查看，行上有「恢复」).
/// Hovering a row reveals its action group (裁决①：置顶 / 归档或恢复 / 删除,
/// ≤3 small buttons); double-clicking a row enters inline renaming via the
/// shared [`TextInput`] (Enter submits, Esc cancels, empty title = cancel —
/// the keyboard path itself is manual-acceptance, see [`resolve_rename`]).
/// No project selected → guidance copy. Collapse state persists in config
/// (`ui.sessions_collapsed`); the archive expansion is in-memory only.
pub struct ThreadsBlock {
    /// Cached active rows for the project in [`Self::loaded_project`].
    pub(crate) threads: Vec<Thread>,
    /// Cached archived rows (shown in the 「已归档」 collapsed section).
    pub(crate) archived: Vec<Thread>,
    /// Project id the cache was loaded for (`None` → guidance copy).
    pub(crate) loaded_project: Option<String>,
    /// Thread id currently under the mouse; drives the hover action group.
    pub(crate) hovered: Option<String>,
    /// Whether the 「已归档 (N)」 section is expanded (in-memory, T13 卡允许
    /// 不做折叠记忆).
    pub(crate) archive_expanded: bool,
    /// Active inline rename session (`None` = not renaming).
    pub(crate) editing: Option<RenameSession>,
    /// Inline error message (ui-spec §4.6).
    pub(crate) error: Option<String>,
}

/// An inline rename in progress: the thread being renamed plus the shared
/// [`TextInput`] entity pre-filled with its current title.
pub(crate) struct RenameSession {
    pub(crate) thread_id: String,
    pub(crate) input: Entity<TextInput>,
}

impl ThreadsBlock {
    /// Creates the block and loads the selected project's thread list.
    pub fn new(cx: &mut Context<Self>) -> Self {
        let mut view = Self {
            threads: Vec::new(),
            archived: Vec::new(),
            loaded_project: None,
            hovered: None,
            archive_expanded: false,
            editing: None,
            error: None,
        };
        view.reload(cx);
        view
    }

    /// Re-reads the selected project's thread lists: active rows for the
    /// main list, archived rows for the 「已归档」 section (both pinned
    /// first, then updated_at desc — store ordering).
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        let selected = cx.global::<SelectedProject>().0.clone();
        self.loaded_project = selected.clone();
        let result = match selected {
            None => Ok((Vec::new(), Vec::new())),
            Some(project_id) => with_store(cx, |store| {
                let active =
                    conversation::list_threads(store, &project_id, Some(ThreadStatus::Active))
                        .map_err(|error| error.to_string())?;
                let archived =
                    conversation::list_threads(store, &project_id, Some(ThreadStatus::Archived))
                        .map_err(|error| error.to_string())?;
                Ok((active, archived))
            }),
        };
        match result {
            Ok((threads, archived)) => {
                self.threads = threads;
                self.archived = archived;
                self.error = None;
            }
            Err(message) => self.error = Some(message),
        }
        // 编辑中的线程被删除后不再渲染其编辑器（输入实体随之释放）。
        if let Some(session) = &self.editing {
            let exists = self
                .threads
                .iter()
                .chain(self.archived.iter())
                .any(|thread| thread.id == session.thread_id);
            if !exists {
                self.editing = None;
            }
        }
        cx.notify();
    }

    /// Click = open: bumps `threads.updated_at` + the owning project's
    /// `last_opened_at` (single transaction) and switches the content column
    /// via the [`OpenedThread`] global.
    fn open_thread(&mut self, thread_id: &str, cx: &mut Context<Self>) {
        let result = with_store(cx, |store| {
            conversation::open_thread(store, thread_id).map_err(|error| error.to_string())
        });
        match result {
            Ok(opened) => {
                self.error = None;
                cx.set_global(OpenedThread(Some(opened)));
                self.reload(cx);
                cx.emit(ThreadsBlockEvent::Opened);
            }
            Err(message) => {
                self.error = Some(message);
                cx.notify();
            }
        }
    }

    /// Runs one thread mutation, reloads the cached rows, and surfaces a
    /// failure as the block's inline error bar (ui-spec §4.6).
    fn mutate_thread(
        &mut self,
        operation: impl FnOnce(&Store) -> Result<(), String>,
        cx: &mut Context<Self>,
    ) {
        let result = with_store(cx, operation);
        self.reload(cx);
        if let Err(message) = result {
            self.error = Some(message);
        }
        cx.notify();
    }

    /// Hover pin toggle (裁决①：hover 操作组里的置顶切换，再点取消).
    fn toggle_pin(&mut self, thread_id: &str, pinned: bool, cx: &mut Context<Self>) {
        self.mutate_thread(
            |store| {
                conversation::set_thread_pinned(store, thread_id, !pinned)
                    .map_err(|error| error.to_string())
            },
            cx,
        );
    }

    /// 归档 (active → archived) or 恢复 (archived → active).
    fn set_thread_status(&mut self, thread_id: &str, status: ThreadStatus, cx: &mut Context<Self>) {
        self.mutate_thread(
            |store| {
                conversation::set_thread_status(store, thread_id, status)
                    .map_err(|error| error.to_string())
            },
            cx,
        );
    }

    /// 「删除」 hover entry: parks the thread in [`PendingDeleteConfirm`];
    /// the window root renders the confirmation overlay (ui-spec §4.6: no
    /// system modal). Any inline rename is folded first so the overlay's Esc
    /// semantics stay unambiguous.
    fn request_delete(&mut self, thread: &Thread, cx: &mut Context<Self>) {
        self.editing = None;
        cx.set_global(PendingDeleteConfirm(Some(thread.clone())));
        cx.refresh_windows();
    }

    /// Double-click = rename: builds the inline editor pre-filled with the
    /// current title and moves focus into it. (合成键盘事件送不进 GPUI：键入
    /// 与 Enter/Esc 提交路径为人工验收；提交/取消的纯逻辑见 [`resolve_rename`]。)
    fn start_rename(&mut self, thread: &Thread, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .editing
            .as_ref()
            .is_some_and(|session| session.thread_id == thread.id)
        {
            return;
        }
        let input = cx.new(|cx| TextInput::new(cx, "会话标题", false));
        input.update(cx, |input, cx| input.set_text(&thread.title, cx));
        self.editing = Some(RenameSession {
            thread_id: thread.id.clone(),
            input: input.clone(),
        });
        let focus_handle = input.read(cx).focus_handle(cx);
        window.focus(&focus_handle, cx);
        cx.notify();
    }

    /// Enter on the rename editor: an empty title cancels (不写库); otherwise
    /// the trimmed title is persisted and the opened thread's cached copy is
    /// resynced so the content header shows the new title.
    fn commit_rename(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.editing.take() else {
            return;
        };
        let raw = session.input.read(cx).text().to_string();
        match resolve_rename(&raw) {
            RenameResolution::Cancel => {
                // 空标题提交视为取消：直接退出编辑态，不写库。
                cx.notify();
            }
            RenameResolution::Commit(title) => {
                let result = with_store(cx, |store| {
                    conversation::rename_thread(store, &session.thread_id, &title)
                        .map_err(|error| error.to_string())
                });
                // reload 先行：失败信息在其后写入，避免被 reload 清空。
                self.reload(cx);
                match result {
                    Ok(renamed) => {
                        let mut opened = cx.global::<OpenedThread>().0.clone();
                        if let Some(opened) =
                            opened.as_mut().filter(|opened| opened.id == renamed.id)
                        {
                            *opened = renamed;
                        }
                        cx.set_global(OpenedThread(opened));
                    }
                    Err(message) => self.error = Some(message),
                }
                cx.refresh_windows();
            }
        }
    }

    /// Esc on the rename editor (intercepted from the global `CloseSettings`
    /// action while the editor is mounted): exits editing without writing.
    fn cancel_rename(&mut self, cx: &mut Context<Self>) {
        if self.editing.take().is_some() {
            cx.notify();
        }
    }

    /// Hover bookkeeping for one row; only real changes notify.
    fn set_hovered(&mut self, thread_id: &str, hovered: bool, cx: &mut Context<Self>) {
        let changed = if hovered {
            if self.hovered.as_deref() != Some(thread_id) {
                self.hovered = Some(thread_id.to_string());
                true
            } else {
                false
            }
        } else if self.hovered.as_deref() == Some(thread_id) {
            self.hovered = None;
            true
        } else {
            false
        };
        if changed {
            cx.notify();
        }
    }

    /// Block header: collapsible title (chevron shows the state).
    fn render_header(&self, cx: &mut Context<Self>, colors: &ThemeColors) -> AnyElement {
        let collapsed = cx.global::<SessionsCollapsed>().0;
        div()
            .flex()
            .items_center()
            .h(px(Typography::SIDEBAR_LINE_HEIGHT))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .px_1()
                    .rounded_md()
                    .cursor_pointer()
                    .hover(move |s| s.bg(colors.bg_hover))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|_, _: &MouseUpEvent, _, cx| toggle_sessions_block(cx)),
                    )
                    .child(
                        div()
                            .text_size(px(Typography::HEADING_BLOCK))
                            .font_weight(Typography::HEADING_BLOCK_WEIGHT)
                            .text_color(colors.text_primary)
                            .child("会话"),
                    )
                    .child(div().text_color(colors.text_tertiary).child(if collapsed {
                        "▸"
                    } else {
                        "▾"
                    })),
            )
            .into_any_element()
    }

    /// The session rows (active only), or the guidance copy when no project
    /// is selected / no thread exists. The 「已归档 (N)」 collapsed section
    /// trails the active rows.
    fn render_body(&self, cx: &mut Context<Self>, colors: &ThemeColors) -> AnyElement {
        let opened_id = cx
            .global::<OpenedThread>()
            .0
            .as_ref()
            .map(|thread| thread.id.clone());
        let guidance = |message: &'static str| {
            div()
                .h(px(Typography::SIDEBAR_LINE_HEIGHT))
                .flex()
                .items_center()
                .px_1()
                .text_size(px(Typography::SIDEBAR))
                .text_color(colors.text_tertiary)
                .child(message)
                .into_any_element()
        };
        if self.loaded_project.is_none() {
            return guidance("暂无项目：先在「项目」区选择").into_any_element();
        }
        if self.threads.is_empty() && self.archived.is_empty() {
            return guidance("暂无会话：点顶部「新建任务」开始").into_any_element();
        }
        let mut body = div().flex().flex_col();
        if self.threads.is_empty() {
            // 仅剩归档线程：主列表给一行引导，归档折叠区照常可见可展开。
            body = body.child(guidance("暂无活跃会话"));
        } else {
            body = body.children(
                self.threads
                    .iter()
                    .map(|thread| self.render_row(thread, &opened_id, false, cx)),
            );
        }
        body.children(
            archive_section_visible(self.archived.len())
                .then(|| self.render_archive_header(cx, colors)),
        )
        .when(
            self.archive_expanded && archive_section_visible(self.archived.len()),
            |column| {
                column.children(
                    self.archived
                        .iter()
                        .map(|thread| self.render_row(thread, &opened_id, true, cx)),
                )
            },
        )
        .into_any_element()
    }

    /// 「已归档 (N)」折叠区入口：chevron 显示展开态，点击切换（本卡不持久化
    /// 该折叠状态）。
    fn render_archive_header(&self, cx: &mut Context<Self>, colors: &ThemeColors) -> AnyElement {
        div()
            .h(px(Typography::SIDEBAR_LINE_HEIGHT))
            .flex()
            .items_center()
            .gap_1()
            .px_1()
            .rounded_md()
            .cursor_pointer()
            .hover(move |s| s.bg(colors.bg_hover))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, cx| {
                    this.archive_expanded = !this.archive_expanded;
                    cx.notify();
                }),
            )
            .child(
                div()
                    .text_size(px(Typography::SIDEBAR))
                    .text_color(colors.text_secondary)
                    .child(format!("已归档 ({})", self.archived.len())),
            )
            .child(
                div()
                    .text_size(px(Typography::SIDEBAR))
                    .text_color(colors.text_tertiary)
                    .child(if self.archive_expanded { "▾" } else { "▸" }),
            )
            .into_any_element()
    }

    /// One session row per ui-spec §4.1: [2px accent bar][pin mark][title…]
    /// [dot] [relative time | hover action group]. The selected row gets
    /// `bg_active`; hovering a non-editing row swaps the time label for its
    /// action group (裁决①：置顶 / 归档或恢复 / 删除；行内编辑行除外)。
    ///
    /// The clickable body and the right side are sibling nodes — clicks on
    /// the action buttons must not re-trigger open (T10 经验：兄弟节点避免
    /// 嵌套命中).
    fn render_row(
        &self,
        thread: &Thread,
        opened_id: &Option<String>,
        archived: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        let selected = opened_id.as_deref() == Some(thread.id.as_str());
        let hovered = self.hovered.as_deref() == Some(thread.id.as_str());
        let editing_session = self
            .editing
            .as_ref()
            .filter(|session| session.thread_id == thread.id);
        let editing_this_row = editing_session.is_some();
        let actions_visible = row_shows_actions(hovered, editing_this_row);
        let thread_id = thread.id.clone();
        let row_thread = thread.clone();
        let mut row = div()
            .id(ElementId::Name(format!("thread-row-{thread_id}").into()))
            .h(px(Typography::SIDEBAR_LINE_HEIGHT))
            .flex()
            .items_center()
            .rounded_md()
            .overflow_hidden()
            .text_size(px(Typography::SIDEBAR))
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                this.set_hovered(&thread_id, *hovered, cx);
            }))
            .when(selected || (hovered && !editing_this_row), move |row| {
                row.bg(if selected {
                    colors.bg_active
                } else {
                    colors.bg_hover
                })
            })
            // 左侧 2px 强调条（选中态着 accent 色，未选中占位保持对齐）。
            .child(
                div()
                    .w(px(2.))
                    .h_full()
                    .flex_shrink_0()
                    .when(selected, move |bar| bar.bg(colors.accent)),
            );
        if let Some(session) = editing_session {
            row = row.child(self.render_rename_editor(session, cx));
        } else {
            row = row
                // 可点击主体与右侧操作是兄弟节点：双击进入行内编辑，单击
                // 打开会话（双击序列的第一次单击仍会先打开，属预期）。
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .pl_2()
                        .cursor_pointer()
                        .when(!selected, move |main| {
                            main.hover(move |s| s.bg(colors.bg_hover))
                        })
                        .on_mouse_up(
                            MouseButton::Left,
                            cx.listener(move |this, event: &MouseUpEvent, window, cx| {
                                if event.click_count >= 2 {
                                    this.start_rename(&row_thread, window, cx);
                                } else {
                                    this.open_thread(&row_thread.id, cx);
                                }
                            }),
                        )
                        .children(thread.pinned.then(|| {
                            // 置顶小标记：token 色着色（裁决③）。
                            div().flex_shrink_0().text_color(colors.accent).child("▲")
                        }))
                        .child(
                            div()
                                .truncate()
                                .text_color(colors.text_primary)
                                .when(thread.unread, |title| {
                                    title.font_weight(Typography::HEADING_CARD_WEIGHT)
                                })
                                .child(thread_title(thread)),
                        ),
                )
                // 未读圆点（数据恒 0 至 S3；显示逻辑本卡落地）。
                .children(
                    thread
                        .unread
                        .then(|| div().size(px(6.)).rounded_full().bg(colors.accent)),
                );
            if actions_visible {
                row = row.child(self.render_row_actions(thread, archived, cx));
            } else {
                row = row.child(
                    div()
                        .flex_shrink_0()
                        .px_2()
                        .text_color(colors.text_secondary)
                        .child(relative_time(thread.updated_at)),
                );
            }
        }
        row.into_any_element()
    }

    /// The hover action group (裁决①：每组最多 3 个小按钮). Active rows get
    /// [置顶/取消置顶][归档][删除]; archived rows get [恢复][删除].
    fn render_row_actions(
        &self,
        thread: &Thread,
        archived: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        let thread_id = thread.id.clone();
        let mut group = div().flex().items_center().gap_0p5().flex_shrink_0().pr_1();
        if archived {
            group = group.child(row_action_button(
                "恢复",
                colors.text_secondary,
                colors.bg_active,
                move |this, _: &MouseUpEvent, _, cx| {
                    this.set_thread_status(&thread_id, ThreadStatus::Active, cx);
                },
                cx,
            ));
        } else {
            let (pin_label, now_pinned) = if thread.pinned {
                ("取消置顶", true)
            } else {
                ("置顶", false)
            };
            group = group.child(row_action_button(
                pin_label,
                colors.text_secondary,
                colors.bg_active,
                move |this, _: &MouseUpEvent, _, cx| {
                    this.toggle_pin(&thread_id, now_pinned, cx);
                },
                cx,
            ));
            let thread_id = thread.id.clone();
            group = group.child(row_action_button(
                "归档",
                colors.text_secondary,
                colors.bg_active,
                move |this, _: &MouseUpEvent, _, cx| {
                    this.set_thread_status(&thread_id, ThreadStatus::Archived, cx);
                },
                cx,
            ));
        }
        let thread_for_delete = thread.clone();
        group
            .child(row_action_button(
                "删除",
                colors.danger,
                colors.bg_active,
                move |this, _: &MouseUpEvent, _, cx| {
                    let thread = thread_for_delete.clone();
                    this.request_delete(&thread, cx);
                },
                cx,
            ))
            .into_any_element()
    }

    /// The inline rename editor mounted in place of the row body: the shared
    /// [`TextInput`] under a `ThreadRename` key context. Enter dispatches
    /// [`ConfirmRename`] (bound to this context in `vega_ui::init`); Esc is
    /// intercepted from the global [`CloseSettings`] action, which the
    /// editor consumes so settings never closes mid-rename.
    fn render_rename_editor(&self, session: &RenameSession, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex_1()
            .min_w_0()
            .pr_1()
            .key_context("ThreadRename")
            .track_focus(&session.input.read(cx).focus_handle(cx))
            .on_action(cx.listener(|this, _: &ConfirmRename, _, cx| this.commit_rename(cx)))
            .on_action(cx.listener(|this, _: &CloseSettings, _, cx| this.cancel_rename(cx)))
            .child(session.input.clone())
            .into_any_element()
    }
}

impl Render for ThreadsBlock {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme(cx).colors;
        let collapsed = cx.global::<SessionsCollapsed>().0;
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(self.render_header(cx, &colors))
            .children(
                self.error
                    .clone()
                    .map(|message| error_bar(message, &colors)),
            )
            .when(!collapsed, |block| {
                block.child(self.render_body(cx, &colors))
            })
            .into_any_element()
    }
}
