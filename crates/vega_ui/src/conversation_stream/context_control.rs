//! #76 R1/R7: IO-free, exact-owner context controls. Authority stays in the app.
use super::*;
use vega_conversation::types::{
    ContextCompactionFailureCode as Failure, ContextCompactionStatus as Status,
    ContextCompactionStatusRecord, ContextSettings,
};

const LOAD_ERROR: &str = "上下文信息读取失败，请稍后重试";

pub(crate) struct ContextControl {
    pub open: bool,
    pub trigger: FocusHandle,
    limit: Entity<TextInput>,
    reserve: Entity<TextInput>,
    automatic: bool,
    settings: Option<ContextSettings>,
    estimate: Option<u64>,
    compactable: bool,
    next_request: u64,
    retired_through: u64,
    save_pending: Option<u64>,
    saving_values: Option<(String, String, bool)>,
    compact_pending: Option<u64>,
    cancel_pending: bool,
    error: Option<&'static str>,
    records: Vec<ContextCompactionStatusRecord>,
}

impl ContextControl {
    pub fn new(cx: &mut Context<ConversationStream>) -> Self {
        Self {
            open: false,
            trigger: cx.focus_handle(),
            limit: cx.new(|cx| TextInput::new(cx, "未配置", false).with_tab_stop(true)),
            reserve: cx.new(|cx| TextInput::new(cx, "输出预留", false).with_tab_stop(true)),
            automatic: false,
            settings: None,
            estimate: None,
            compactable: false,
            next_request: 0,
            retired_through: 0,
            save_pending: None,
            saving_values: None,
            compact_pending: None,
            cancel_pending: false,
            error: None,
            records: Vec::new(),
        }
    }
    fn busy(&self) -> bool {
        self.save_pending.is_some()
            || self.compact_pending.is_some()
            || self
                .records
                .last()
                .is_some_and(|r| r.status == Status::Compacting)
    }
}

impl ConversationStream {
    pub(crate) fn reset_context_control(&mut self, cx: &mut Context<Self>) {
        // R7: model change invalidates all old settings, status and ACK owners.
        // Keep the counter monotonic so an ABA switch cannot accept an old ACK.
        let next = self.context_control.next_request;
        self.context_control = ContextControl::new(cx);
        self.context_control.next_request = next;
        self.context_control.retired_through = next;
    }

    /// Whether context preparation/save currently excludes a competing submit.
    pub fn context_operation_busy(&self) -> bool {
        self.context_control.busy()
    }

    /// Restore durable uncertainty without re-adding tokens or clearing a latch.
    pub fn restore_context_unknown_usage(
        &mut self,
        thread_id: &str,
        model: &str,
        unknown_usage: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        if thread_id != self.thread.id || model != self.thread.model {
            return false;
        }
        if unknown_usage {
            self.meter.restore_unknown_context_usage();
            cx.notify();
        }
        true
    }

    /// Metadata failures are not charged summary attempts and never feed the meter.
    pub fn apply_context_load_error(
        &mut self,
        thread_id: &str,
        model: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        if thread_id != self.thread.id || model != self.thread.model {
            return false;
        }
        self.context_control.error = Some(LOAD_ERROR);
        cx.notify();
        true
    }

    /// R7: app maps each auto operation into this same stream-local ID space.
    pub fn reserve_context_operation_id(&mut self, _cx: &mut Context<Self>) -> Option<u64> {
        let id = self.context_control.next_request.checked_add(1)?;
        self.context_control.next_request = id;
        Some(id)
    }

    /// Apply controller-owned metadata; never reads history or invents an estimate.
    /// Call only after the app's own async generation/route fence has accepted it.
    pub fn apply_context_projection(
        &mut self,
        thread_id: &str,
        model: &str,
        settings: Option<ContextSettings>,
        estimated_tokens: Option<u64>,
        compactable: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        if thread_id != self.thread.id
            || model != self.thread.model
            || settings
                .as_ref()
                .is_some_and(|s| s.thread_id != thread_id || s.model != model)
        {
            return false;
        }
        let untouched = self.context_control.settings.is_none()
            && self.context_control.limit.read(cx).text().is_empty()
            && self.context_control.reserve.read(cx).text().is_empty();
        self.context_control.settings = settings;
        self.context_control.estimate = estimated_tokens;
        self.context_control.compactable = compactable;
        if self.context_control.error == Some(LOAD_ERROR) {
            self.context_control.error = None;
        }
        if (!self.context_control.open || untouched) && self.context_control.save_pending.is_none()
        {
            self.sync_context_inputs(cx);
        }
        cx.notify();
        true
    }

    /// Exact-request save ACK. Failure preserves the edited values for retry.
    pub fn finish_context_settings(
        &mut self,
        thread_id: &str,
        model: &str,
        request_id: u64,
        saved: Option<ContextSettings>,
        cx: &mut Context<Self>,
    ) -> bool {
        if thread_id != self.thread.id
            || model != self.thread.model
            || self.context_control.save_pending != Some(request_id)
            || saved
                .as_ref()
                .is_some_and(|s| s.thread_id != thread_id || s.model != model)
        {
            return false;
        }
        self.context_control.save_pending = None;
        let unchanged =
            self.context_control
                .saving_values
                .take()
                .is_some_and(|(limit, reserve, automatic)| {
                    limit == self.context_control.limit.read(cx).text()
                        && reserve == self.context_control.reserve.read(cx).text()
                        && automatic == self.context_control.automatic
                });
        if let Some(settings) = saved {
            self.context_control.settings = Some(settings);
            // R7: late save ACK must not erase edits made while storage was busy.
            if unchanged {
                self.sync_context_inputs(cx);
            }
            self.context_control.error = None;
        } else {
            self.context_control.error = Some("设置保存失败，请重试");
        }
        cx.notify();
        true
    }

    /// Project an actual operation transition; no synthetic progress percentages.
    /// App owns one monotonic generation namespace for manual and auto operations.
    pub fn apply_context_status(
        &mut self,
        thread_id: &str,
        model: &str,
        record: ContextCompactionStatusRecord,
        cx: &mut Context<Self>,
    ) -> bool {
        if thread_id != self.thread.id
            || model != self.thread.model
            || record.generation <= self.context_control.retired_through
            || self
                .context_control
                .compact_pending
                .is_some_and(|id| record.generation < id)
            || self.context_control.records.last().is_some_and(|old| {
                record.generation < old.generation
                    || (record.generation == old.generation
                        && (record.updated_at < old.updated_at
                            || matches!(
                                old.status,
                                Status::Succeeded | Status::Failed | Status::Cancelled
                            )))
            })
        {
            return false;
        }
        self.context_control.next_request =
            self.context_control.next_request.max(record.generation);
        // R5: a failed/unpriced summary must remain visible in the cost meter.
        if self
            .meter
            .apply(&ConversationEvent::ContextCompactionStatus {
                record: record.clone(),
            })
        {
            cx.notify();
        }
        self.context_control.estimate = record.estimated_tokens.or(self.context_control.estimate);
        if record.status != Status::Compacting {
            self.context_control.compact_pending = None;
            self.context_control.cancel_pending = false;
        }
        if self.context_control.records.last() != Some(&record) {
            // A bounded chronological strip, not raw assistant/transcript data.
            if self.context_control.records.len() == 6 {
                self.context_control.records.remove(0);
            }
            self.context_control.records.push(record);
        }
        cx.notify();
        true
    }

    fn sync_context_inputs(&mut self, cx: &mut Context<Self>) {
        let settings = self.context_control.settings.as_ref();
        let limit = settings
            .and_then(|s| s.context_limit)
            .map(|v| v.to_string())
            .unwrap_or_default();
        let reserve = settings
            .map(|s| s.output_reserve.to_string())
            .unwrap_or_default();
        self.context_control.automatic = settings.is_some_and(|s| s.automatic_compaction);
        self.context_control
            .limit
            .update(cx, |input, cx| input.set_text(&limit, cx));
        self.context_control
            .reserve
            .update(cx, |input, cx| input.set_text(&reserve, cx));
    }

    fn toggle_context(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let open = !self.context_control.open;
        self.close_composer_popovers(cx);
        self.branch_selector.update(cx, |selector, cx| {
            selector.request_close(cx);
        });
        self.context_control.open = open;
        if open {
            self.sync_context_inputs(cx);
            self.context_control.trigger.focus(window, cx);
        }
        cx.notify();
    }

    fn context_manual_reason(&self) -> Option<&'static str> {
        if self.draft_route {
            Some("先发送一条消息以创建会话")
        } else if self.actions.running
            || self.composer_submit_pending
            || self.active_agent_message.is_some()
        {
            Some("运行期间不能手动压缩；自动压缩由当前运行处理")
        } else if self.context_operation_busy() {
            Some("上下文操作正在进行")
        } else if self.trusted_action_busy
            || self.model_selection_pending.is_some()
            || self.approved_not_started
        {
            Some("请先完成当前操作")
        } else if self
            .context_control
            .settings
            .as_ref()
            .and_then(|s| s.context_limit)
            .is_none()
        {
            Some("请先配置并保存上下文容量")
        } else if !self.context_control.compactable {
            Some("暂无可压缩的完整历史；保留最新一轮")
        } else {
            None
        }
    }

    fn context_settings_blocked(&self) -> bool {
        self.draft_route
            || self.context_operation_busy()
            || self.actions.running
            || self.active_agent_message.is_some()
            || self.approved_not_started
            || self.composer_submit_pending
            || self.trusted_action_busy
            || self.model_selection_pending.is_some()
    }

    fn save_context(&mut self, cx: &mut Context<Self>) {
        if self.context_settings_blocked() {
            return;
        }
        let parse = |s: &str| -> Option<u64> {
            if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            s.parse::<u64>()
                .ok()
                .filter(|v| *v > 0 && *v <= u32::MAX as u64)
        };
        let limit = parse(self.context_control.limit.read(cx).text());
        let reserve = parse(self.context_control.reserve.read(cx).text());
        let (Some(limit), Some(reserve)) = (limit, reserve) else {
            self.context_control.error = Some("请输入 1 至 4294967295 的整数");
            cx.notify();
            return;
        };
        if reserve >= limit {
            self.context_control.error = Some("输出预留必须小于上下文总容量");
            cx.notify();
            return;
        }
        let Some(id) = self.context_control.next_request.checked_add(1) else {
            return;
        };
        self.context_control.next_request = id;
        self.context_control.save_pending = Some(id);
        self.context_control.saving_values = Some((
            self.context_control.limit.read(cx).text().to_owned(),
            self.context_control.reserve.read(cx).text().to_owned(),
            self.context_control.automatic,
        ));
        self.context_control.error = None;
        cx.emit(ContextSettingsRequested {
            request_id: id,
            settings: ContextSettings {
                thread_id: self.thread.id.clone(),
                model: self.thread.model.clone(),
                context_limit: Some(limit),
                output_reserve: reserve,
                automatic_compaction: self.context_control.automatic,
                updated_at: 0,
            },
        });
        cx.notify();
    }

    fn compact_context(&mut self, cx: &mut Context<Self>) {
        if self.context_manual_reason().is_some() {
            return;
        }
        let Some(id) = self.context_control.next_request.checked_add(1) else {
            return;
        };
        self.context_control.next_request = id;
        self.context_control.compact_pending = Some(id);
        self.context_control.error = None;
        cx.emit(ContextCompactionRequested {
            thread_id: self.thread.id.clone(),
            model: self.thread.model.clone(),
            request_id: id,
        });
        cx.notify();
    }

    fn cancel_context(&mut self, cx: &mut Context<Self>) {
        if self.context_control.cancel_pending {
            return;
        }
        let owner = self.context_control.compact_pending.or_else(|| {
            self.context_control
                .records
                .last()
                .filter(|r| r.status == Status::Compacting)
                .map(|r| r.generation)
        });
        if let Some(request_id) = owner {
            self.context_control.cancel_pending = true;
            cx.emit(ContextCompactionCancelRequested {
                thread_id: self.thread.id.clone(),
                model: self.thread.model.clone(),
                request_id,
            });
            cx.notify();
        }
    }

    pub(crate) fn render_context_control(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        let state = &self.context_control;
        let label = match (
            state.estimate,
            state.settings.as_ref().and_then(|s| s.context_limit),
        ) {
            (Some(usage), Some(limit)) => format!("上下文 ≈{usage}/{limit}"),
            (_, Some(limit)) => format!("上下文 —/{limit}"),
            _ => "上下文 · 未配置".into(),
        };
        div()
            .relative()
            // C13: Escape is globally bound to an action. A real app ancestor
            // handles that action before raw key listeners can run; the trigger
            // is also a sibling, not a descendant, of the popup key listener.
            // Own the action at their common ancestor while this layer is open.
            .on_action(
                cx.listener(|this, _: &crate::DismissEnvironmentOverlay, window, cx| {
                    if this.context_control.open {
                        this.context_control.open = false;
                        this.context_control.trigger.focus(window, cx);
                        cx.stop_propagation();
                        cx.notify();
                    } else {
                        cx.propagate();
                    }
                }),
            )
            .child(
                div()
                    .id("composer-context")
                    .debug_selector(|| "composer-context".into())
                    .aria_label("上下文设置")
                    .track_focus(&state.trigger)
                    .tab_stop(true)
                    .cursor_pointer()
                    .h(px(Layout::COMPOSER_UTILITY_CHIP_HEIGHT))
                    .px_2()
                    .rounded_full()
                    .flex()
                    .items_center()
                    .text_size(px(Typography::METADATA))
                    .text_color(colors.text_secondary)
                    .hover(|s| s.bg(colors.bg_hover))
                    .focus_visible(|s| s.border_1().border_color(colors.accent))
                    .when(state.open, |s| s.bg(colors.bg_hover))
                    .capture_any_mouse_down(|_, _, cx| cx.stop_propagation())
                    // R68: capture declares the trigger inside to sibling popups;
                    // mouse-up activation survives that deliberate down capture.
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| this.toggle_context(window, cx)),
                    )
                    .on_key_down(
                        cx.listener(|this, event: &gpui_kit::KeyDownEvent, window, cx| {
                            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                this.toggle_context(window, cx);
                                cx.stop_propagation();
                            }
                        }),
                    )
                    .child(label),
            )
            .when(state.open, |el| {
                el.child(
                    div().absolute().top_0().left_0().size_full().child(
                        gpui_kit::anchored()
                            .anchor(gpui_kit::Anchor::BottomLeft)
                            .position_mode(gpui_kit::AnchoredPositionMode::Local)
                            .position(gpui_kit::point(
                                px(0.0),
                                -px(Layout::COMPOSER_UTILITY_CHIP_PADDING_X),
                            ))
                            .snap_to_window_with_margin(px(Layout::COMPOSER_UTILITY_CHIP_PADDING_X))
                            .child(
                                gpui_kit::deferred(self.render_context_popup(window, cx))
                                    .with_priority(2),
                            ),
                    ),
                )
            })
            .into_any_element()
    }

    fn render_context_popup(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let can_save = !self.context_settings_blocked();
        let reason = self.context_manual_reason();
        let cancel = self.context_control.compact_pending.is_some()
            || self
                .context_control
                .records
                .last()
                .is_some_and(|r| r.status == Status::Compacting);
        let retry = self
            .context_control
            .records
            .last()
            .is_some_and(|r| matches!(r.status, Status::Failed | Status::Cancelled));
        div()
            .id("context-popup")
            .debug_selector(|| "context-popup".into())
            .w(px(Layout::MENU_MAX_WIDTH).min(
                (window.viewport_size().width - px(Layout::CONTENT_PADDING * 2.0)).max(px(1.0)),
            ))
            .max_h(px(Layout::COMPOSER_PICKER_MAX_HEIGHT).min(
                (window.viewport_size().height - px(Layout::CONTENT_PADDING * 2.0)).max(px(1.0)),
            ))
            .overflow_y_scroll()
            .p_3()
            .flex()
            .flex_col()
            .gap_2()
            .rounded(px(Layout::MENU_RADIUS))
            .bg(colors.bg_elevated)
            .border_1()
            .border_color(colors.border_subtle)
            .shadow_sm()
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                this.context_control.open = false;
                cx.notify();
            }))
            .on_key_down(
                cx.listener(|this, event: &gpui_kit::KeyDownEvent, window, cx| {
                    if event.keystroke.key == "escape" {
                        this.context_control.open = false;
                        this.context_control.trigger.focus(window, cx);
                        cx.stop_propagation();
                        cx.notify();
                    }
                }),
            )
            .text_size(px(Typography::BODY))
            .text_color(colors.text_primary)
            .child("上下文（近似估算，非服务方精确用量）")
            // R1/R7: primary actions precede the scrollable settings details.
            // Debug bounds exist even for clipped descendants; cancellation
            // must remain inside the actual popup hit-test clip while busy.
            .when(cancel, |el| {
                el.child(self.context_button(
                    "context-cancel",
                    if self.context_control.cancel_pending {
                        "正在取消…"
                    } else {
                        "取消压缩"
                    },
                    !self.context_control.cancel_pending,
                    cx,
                ))
            })
            .child(self.context_button(
                "context-compact",
                if retry {
                    "重试压缩"
                } else {
                    "立即压缩"
                },
                reason.is_none(),
                cx,
            ))
            .child("总容量 tokens")
            .child(
                div()
                    .debug_selector(|| "context-limit-input".into())
                    .child(self.context_control.limit.clone()),
            )
            .child("输出预留 tokens")
            .child(
                div()
                    .debug_selector(|| "context-reserve-input".into())
                    .child(self.context_control.reserve.clone()),
            )
            .child(self.context_button(
                "context-auto",
                if self.context_control.automatic {
                    "自动压缩：开启"
                } else {
                    "自动压缩：关闭"
                },
                can_save,
                cx,
            ))
            .child(self.context_button(
                "context-save",
                if self.context_control.save_pending.is_some() {
                    "正在保存…"
                } else {
                    "保存设置"
                },
                can_save,
                cx,
            ))
            .children(
                self.context_control
                    .error
                    .map(|error| div().text_color(colors.danger).child(error)),
            )
            .children(reason.map(|reason| {
                div()
                    .text_size(px(Typography::METADATA))
                    .text_color(colors.text_secondary)
                    .child(reason)
            }))
            .into_any_element()
    }

    fn context_button(
        &self,
        id: &'static str,
        label: &'static str,
        enabled: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        div()
            .id(id)
            .debug_selector(move || id.into())
            .aria_label(label)
            .focusable()
            .tab_stop(enabled)
            .rounded_md()
            .px_2()
            .py_1()
            .text_color(if enabled {
                colors.text_primary
            } else {
                colors.text_tertiary
            })
            .when(enabled, |el| {
                el.cursor_pointer()
                    .hover(|s| s.bg(colors.bg_hover))
                    .focus_visible(|s| s.border_1().border_color(colors.accent))
                    .on_click(cx.listener(move |this, _, _, cx| this.context_action(id, cx)))
                    .on_key_down(
                        cx.listener(move |this, event: &gpui_kit::KeyDownEvent, _, cx| {
                            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                this.context_action(id, cx);
                                cx.stop_propagation();
                            }
                        }),
                    )
            })
            .child(label)
            .into_any_element()
    }

    fn context_action(&mut self, id: &str, cx: &mut Context<Self>) {
        match id {
            "context-auto" => {
                if self.context_settings_blocked() {
                    return;
                }
                self.context_control.automatic = !self.context_control.automatic;
                cx.notify();
            }
            "context-save" => self.save_context(cx),
            "context-compact" => self.compact_context(cx),
            "context-cancel" => self.cancel_context(cx),
            _ => {}
        }
    }

    pub(crate) fn render_context_status(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.context_control.records.is_empty() && self.context_control.compact_pending.is_none()
        {
            return None;
        }
        let colors = theme(cx).colors;
        Some(
            div()
                .id("context-status")
                .debug_selector(|| "context-status".into())
                .max_w(px(Layout::COMPOSER_MAX_WIDTH))
                .w_full()
                .mx_auto()
                .text_size(px(Typography::METADATA))
                .text_color(colors.text_secondary)
                .flex()
                .flex_col()
                .children(
                    self.context_control
                        .records
                        .iter()
                        .map(|r| div().child(status_label(r))),
                )
                .when(
                    self.context_control.compact_pending.is_some()
                        && self
                            .context_control
                            .records
                            .last()
                            .is_none_or(|r| r.status != Status::Compacting),
                    |el| el.child("正在准备上下文压缩…"),
                )
                .into_any_element(),
        )
    }
}

fn status_label(record: &ContextCompactionStatusRecord) -> &'static str {
    match record.status {
        Status::Unknown => "上下文容量未配置",
        Status::Ready => "上下文已就绪",
        Status::Compacting => "正在压缩上下文…",
        Status::Succeeded => "上下文压缩完成，原始对话已保留",
        Status::Cancelled => "上下文压缩已取消，原始对话未更改",
        Status::Failed => match record.failure {
            Some(Failure::SourceChanged) => "历史已变化，请重试压缩",
            Some(Failure::NoCompactablePrefix) => "暂无可压缩的完整历史",
            Some(Failure::TooLarge | Failure::OverLimit) => {
                "上下文超出可处理容量，请调整容量或缩短输入后重试"
            }
            Some(Failure::ImagesUnsupported) => "历史图片无法安全压缩，请使用新的会话",
            Some(Failure::InvalidSummary) => "压缩结果无效，请重试",
            _ => "上下文压缩失败，请重试",
        },
    }
}

#[cfg(test)]
#[path = "tests/context_control.rs"]
mod tests;
