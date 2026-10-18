use super::*;
use vega_conversation::types::{
    ContextAccountingRecord, ContextCompactionFailureCode as Failure,
    ContextCompactionStatus as Status, ContextCompactionStatusRecord, ContextSettings,
};

const LOAD_ERROR: &str = "上下文信息读取失败，请稍后重试";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ContextUsageProjection {
    model_input_limit: Option<u64>,
    estimated_tokens: Option<u64>,
}

impl ContextUsageProjection {
    pub fn new(model_input_limit: Option<u64>, estimated_tokens: Option<u64>) -> Self {
        Self {
            model_input_limit,
            estimated_tokens,
        }
    }
}

pub(crate) struct ContextControl {
    pub open: bool,
    limit: Entity<TextInput>,
    reserve: Entity<TextInput>,
    automatic: bool,
    settings: Option<ContextSettings>,
    model_input_limit: Option<u64>,
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
            model_input_limit: None,
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
            self.context_control.model_input_limit,
        )
    }

    pub(crate) fn reset_context_control(&mut self, cx: &mut Context<Self>) {
        let next = self.context_control.next_request;
        self.context_control = ContextControl::new(cx);
        self.context_control.next_request = next;
        self.context_control.retired_through = next;
        self.context_usage_trigger_hovered = false;
        self.context_usage_tooltip_hovered = false;
        self.context_usage_open = false;
        self.context_usage_dismissed = false;
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
        usage: ContextUsageProjection,
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
        self.context_control.model_input_limit = usage.model_input_limit;
        if self.context_control.live_accounting.is_none() {
            self.context_control.estimate = usage.estimated_tokens;
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
        if let Some((_, group)) = &self.active_run_activity {
            group.update(cx, |group, cx| {
                group.set_compacting(record.status == Status::Compacting, cx)
            });
        }

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
                if let Some((message_id, _)) = &self.active_agent_message {
                    let identity = self.durable_entry_identity(
                        message_id,
                        &format!("compaction-{}", record.generation),
                        None,
                    );
                    self.set_entry_identity(index, identity);
                }
            }
        }
        // Keep only the newest transition for fencing/busy state. The list
        // owns every operation's single visible row, without a six-row cap.
        self.context_control.records.clear();
        self.context_control.records.push(record);
        self.refresh_run_notices(cx);
        let changed = self
            .entries
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| {
                matches!(entry, StreamEntry::ContextCompaction { .. }).then_some(index)
            })
            .collect::<Vec<_>>();
        for index in changed {
            self.invalidate_item(Some(index));
            self.invalidate_item(Some(index + 1));
        }
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

    pub(crate) fn compaction_failure_resolved(&self, index: usize) -> bool {
        let Some(StreamEntry::ContextCompaction { model, record, .. }) = self.entries.get(index)
        else {
            return false;
        };
        let message_id = self
            .entry_identities
            .get(index)
            .and_then(|identity| identity.message_id.as_deref());
        record.status == Status::Failed
            && self.entries.iter().zip(&self.entry_identities).any(
                |(entry, identity)| match entry {
                    StreamEntry::ContextCompaction {
                        model: owner,
                        record: newer,
                        ..
                    } => {
                        owner == model
                            && identity.message_id.as_deref() == message_id
                            && newer.generation > record.generation
                            && newer.status == Status::Succeeded
                    }
                    _ => false,
                },
            )
    }

    pub(crate) fn compaction_failure_resolved_for_owner(
        &self,
        message_id: &str,
        failure: vega_conversation::types::ContextCompactionFailureCode,
    ) -> bool {
        let owned_statuses = self
            .entries
            .iter()
            .zip(&self.entry_identities)
            .filter_map(|(entry, identity)| match entry {
                StreamEntry::ContextCompaction { model, record, .. }
                    if identity.message_id.as_deref() == Some(message_id) =>
                {
                    Some((model, record))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        let has_owned_failure = owned_statuses
            .iter()
            .any(|(_, record)| record.status == Status::Failed);
        owned_statuses.iter().any(|(model, record)| {
            record.status == Status::Succeeded
                && *model == &self.thread.model
                && (!has_owned_failure
                    || owned_statuses.iter().any(|(failed_model, failed)| {
                        failed.status == Status::Failed
                            && failed.failure == Some(failure)
                            && failed_model == model
                            && failed.generation < record.generation
                    }))
        })
    }

    /// Restore the latest durable result at the loaded history tail. Legacy
    /// records have no transcript anchor, so the row is explicitly historical.
    /// The controller must also accept the original owner/load-sequence fence.
    pub fn restore_context_run_failures(
        &mut self,
        failures: &[(String, RunFailureKind)],
        cx: &mut Context<Self>,
    ) {
        if self.actions.running || self.active_agent_message.is_some() {
            return;
        }
        for (message_id, failure) in failures {
            if self
                .entry_identities
                .iter()
                .any(|identity| identity.message_id.as_ref() == Some(message_id))
            {
                self.remember_run_failure(message_id, *failure);
            }
        }
        self.refresh_run_notices(cx);
        cx.notify();
    }

    pub fn restore_owned_context_status(
        &mut self,
        thread_id: &str,
        model: &str,
        record: ContextCompactionStatusRecord,
        message_id: Option<&str>,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.restore_context_status(thread_id, model, record, cx) {
            return false;
        }
        if let Some(message_id) = message_id.filter(|id| {
            self.entry_identities
                .iter()
                .any(|identity| identity.message_id.as_deref() == Some(id))
        }) {
            let index = self.entries.len() - 1;
            let identity = self.durable_entry_identity(message_id, "restored-compaction", None);
            self.set_entry_identity(index, identity);
        }
        self.refresh_run_notices(cx);
        cx.notify();
        true
    }

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
        Status::Unknown => "Context capacity is not configured",
        Status::Ready => "Context ready",
        Status::Compacting => "Compacting context",
        Status::Succeeded => "Context compacted",
        Status::Cancelled => "Context compaction cancelled",
        Status::Failed => failure.unwrap_or(Failure::Unavailable).message(),
    }
}
