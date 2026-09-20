//! #76: IO-free, exact-owner compaction lifecycle projection.
use super::*;
use vega_conversation::types::{
    ContextCompactionFailureCode as Failure, ContextCompactionStatus as Status,
    ContextCompactionStatusRecord, ContextSettings,
};

const LOAD_ERROR: &str = "上下文信息读取失败，请稍后重试";

pub(crate) struct ContextControl {
    pub open: bool,
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

    pub(crate) fn render_context_status(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        // Ready/Unknown are not lifecycle events and the old per-thread
        // settings must not leak back into the Composer as a status chip.
        let visible_records = self.context_control.records.iter().filter(|record| {
            matches!(
                record.status,
                Status::Compacting | Status::Succeeded | Status::Failed | Status::Cancelled
            )
        });
        visible_records.clone().next()?;
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
                .children(visible_records.map(|record| div().child(status_label(record))))
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
            Some(Failure::TooLarge) => "历史内容已达到安全分段上限，原始对话已保留；请在新会话继续",
            Some(Failure::OverLimit) => "模型上下文预算不足，请调整模型容量或缩短当前输入后重试",
            Some(Failure::ImagesUnsupported) => "历史图片无法安全压缩，请使用新的会话",
            Some(Failure::InvalidSummary) => "压缩结果无效，请重试",
            _ => "上下文压缩失败，请重试",
        },
    }
}

#[cfg(test)]
#[path = "tests/context_control.rs"]
mod tests;
