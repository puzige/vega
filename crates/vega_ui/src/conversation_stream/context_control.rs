//! #76: IO-free, exact-owner compaction lifecycle projection.
use super::*;
use vega_conversation::types::{
    ContextAccountingRecord, ContextCompactionFailureCode as Failure,
    ContextCompactionStatus as Status, ContextCompactionStatusRecord, ContextSettings,
};

const LOAD_ERROR: &str = "上下文信息读取失败，请稍后重试";

pub(crate) struct ContextControl {
    pub open: bool,
    limit: Entity<TextInput>,
    reserve: Entity<TextInput>,
    automatic: bool,
    settings: Option<ContextSettings>,
    estimate: Option<u64>,
    live_accounting: Option<(String, ContextAccountingRecord)>,
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
            live_accounting: None,
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
    pub(crate) fn context_usage_source(&self) -> (Option<u64>, Option<u64>) {
        (
            self.context_control.estimate,
            self.context_control
                .settings
                .as_ref()
                .and_then(|settings| settings.context_limit),
        )
    }

    pub(crate) fn reset_context_control(&mut self, cx: &mut Context<Self>) {
        // R7: model change invalidates all old settings, status and ACK owners.
        // Keep the counter monotonic so an ABA switch cannot accept an old ACK.
        let next = self.context_control.next_request;
        self.context_control = ContextControl::new(cx);
        self.context_control.next_request = next;
        self.context_control.retired_through = next;
        self.context_usage_trigger_hovered = false;
        self.context_usage_tooltip_hovered = false;
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
        if self.context_control.live_accounting.is_none() {
            self.context_control.estimate = estimated_tokens;
        }
        if self.context_control.estimate.is_none() {
            self.context_usage_trigger_hovered = false;
            self.context_usage_tooltip_hovered = false;
        }
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
        if self.context_control.live_accounting.is_none() {
            self.context_control.estimate =
                record.estimated_tokens.or(self.context_control.estimate);
        }
        if record.status != Status::Compacting {
            self.context_control.compact_pending = None;
            self.context_control.cancel_pending = false;
        }
        if matches!(
            record.status,
            Status::Compacting | Status::Succeeded | Status::Failed | Status::Cancelled
        ) {
            let index = self.entries.iter().position(|entry| {
                matches!(entry,
                StreamEntry::ContextCompaction { model: owner, record: old, .. }
                if owner == model && old.generation == record.generation)
            });
            if let Some(index) = index {
                if let StreamEntry::ContextCompaction { record: old, .. } = &mut self.entries[index]
                {
                    *old = record.clone();
                }
                self.invalidate_item(Some(index));
            } else {
                // Just like a tool boundary: future text must follow this row,
                // even when compaction happens in the middle of an agent turn.
                self.close_active_segment_before_tool();
                let index = self.entries.len();
                self.entries.push(StreamEntry::ContextCompaction {
                    model: model.to_owned(),
                    record: record.clone(),
                    restored: false,
                });
                self.list_append(index);
            }
        }
        // Keep only the newest transition for fencing/busy state. The list
        // owns every operation's single visible row, without a six-row cap.
        self.context_control.records.clear();
        self.context_control.records.push(record);
        cx.notify();
        true
    }

    /// Accept only the active run's content-free budget decision.
    pub fn apply_context_accounting(
        &mut self,
        thread_id: &str,
        model: &str,
        message_id: &str,
        record: ContextAccountingRecord,
        cx: &mut Context<Self>,
    ) -> bool {
        if thread_id != self.thread.id
            || model != self.thread.model
            || !self.actions.running
            || !self
                .active_agent_message
                .as_ref()
                .is_some_and(|(active, _)| active == message_id)
            || self
                .context_control
                .live_accounting
                .as_ref()
                .is_some_and(|(old_id, old)| old_id != message_id || record.revision < old.revision)
        {
            return false;
        }
        self.context_control.estimate = Some(record.predicted_input);
        self.context_control.live_accounting = Some((message_id.to_string(), record));
        cx.notify();
        true
    }

    pub(crate) fn clear_live_context_accounting(&mut self) {
        if self.context_control.live_accounting.take().is_some() {
            self.context_control.estimate = None;
            self.context_usage_trigger_hovered = false;
            self.context_usage_tooltip_hovered = false;
        }
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

    /// Restore the latest durable result at the loaded history tail. Legacy
    /// records have no transcript anchor, so the row is explicitly historical.
    /// The controller must also accept the original owner/load-sequence fence.
    pub fn restore_context_status(
        &mut self,
        thread_id: &str,
        model: &str,
        mut record: ContextCompactionStatusRecord,
        cx: &mut Context<Self>,
    ) -> bool {
        if thread_id != self.thread.id
            || model != self.thread.model
            || self.actions.running
            || self.active_agent_message.is_some()
            || !self.context_control.records.is_empty()
            || self.context_control.busy()
            || self.entries.iter().any(|entry| {
                matches!(entry,
                StreamEntry::ContextCompaction { model: owner, .. } if owner == model)
            })
        {
            return false;
        }
        let Some(id) = self.reserve_context_operation_id(cx) else {
            return false;
        };
        record.generation = id;
        if !self.apply_context_status(thread_id, model, record, cx) {
            return false;
        }
        if let Some(StreamEntry::ContextCompaction { restored, .. }) = self.entries.last_mut() {
            *restored = true;
        }
        true
    }
}

pub(super) fn status_label_for(status: Status, failure: Option<Failure>) -> &'static str {
    match status {
        Status::Unknown => "上下文容量未配置",
        Status::Ready => "上下文已就绪",
        Status::Compacting => "正在压缩上下文",
        Status::Succeeded => "上下文已压缩",
        Status::Cancelled => "上下文压缩已取消，原始对话未更改",
        Status::Failed => match failure {
            Some(Failure::SourceChanged) => "历史已变化，请重试压缩",
            Some(Failure::NoCompactablePrefix) => "暂无可压缩的完整历史",
            Some(Failure::TooLarge) => "历史内容已达到安全分段上限，原始对话已保留；请在新会话继续",
            Some(Failure::OverLimit) => {
                "本地上下文预算检查未通过，原始对话已保留；请调整容量或缩短当前输入"
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
