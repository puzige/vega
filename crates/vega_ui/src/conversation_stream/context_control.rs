use super::context_usage::{NextRequestEstimate, NextRequestSource};
use super::*;
use vega_conversation::types::{
    ContextAccountingRecord, ContextAccountingStage, ContextCompactionFailureCode as Failure,
    ContextCompactionStatus as Status, ContextCompactionStatusRecord, ContextSettings, TokenUsage,
};

const LOAD_ERROR: &str = "ui.could_not_load_context_information_try_again_later";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ContextUsageProjection {
    model_input_limit: Option<u64>,
    estimated_tokens: Option<u64>,
    last_call_total: Option<u64>,
}

impl ContextUsageProjection {
    pub fn new(
        model_input_limit: Option<u64>,
        estimated_tokens: Option<u64>,
        last_call_total: Option<u64>,
    ) -> Self {
        Self {
            model_input_limit,
            estimated_tokens,
            last_call_total,
        }
    }
}

struct PrimaryContextCall {
    response_id: String,
    total: Option<u64>,
}

pub(crate) struct ContextControl {
    pub open: bool,
    limit: Entity<TextInput>,
    reserve: Entity<TextInput>,
    automatic: bool,
    settings: Option<ContextSettings>,
    model_input_limit: Option<u64>,
    last_call_total: Option<u64>,
    next_request: Option<NextRequestEstimate>,
    live_call: Option<PrimaryContextCall>,
    live_accounting: Option<(String, ContextAccountingRecord)>,
    compactable: bool,
    source_fingerprint: Option<String>,
    next_operation_id: u64,
    retired_through: u64,
    save_pending: Option<u64>,
    saving_values: Option<(String, String, bool)>,
    compact_pending: Option<u64>,
    command_draft: Option<(u64, String, u64)>,
    cancel_pending: bool,
    error: Option<&'static str>,
    records: Vec<ContextCompactionStatusRecord>,
}

impl ContextControl {
    pub fn new(cx: &mut Context<ConversationStream>) -> Self {
        Self {
            open: false,
            limit: cx.new(|cx| {
                TextInput::new(cx, "", false)
                    .with_placeholder_key("ui.not_configured")
                    .with_tab_stop(true)
            }),
            reserve: cx.new(|cx| {
                TextInput::new(cx, "", false)
                    .with_placeholder_key("ui.output_reserve")
                    .with_tab_stop(true)
            }),
            automatic: false,
            settings: None,
            model_input_limit: None,
            last_call_total: None,
            next_request: None,
            live_call: None,
            live_accounting: None,
            compactable: false,
            source_fingerprint: None,
            next_operation_id: 0,
            retired_through: 0,
            save_pending: None,
            saving_values: None,
            compact_pending: None,
            command_draft: None,
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
    pub(crate) fn submit_compact_command(&mut self, cx: &mut Context<Self>) -> bool {
        if self.thread.backend != TaskBackend::Native {
            return false;
        }
        let original = self.input.read(cx).text().to_owned();
        let Some(requirements) = compact_requirements(&original) else {
            return false;
        };
        if self.input.read(cx).is_composing() {
            return true;
        }
        let error = if requirements.len() > 8192 {
            Some("ui.compact_requirements_too_long")
        } else if !self.attachments.is_empty()
            || !self.input.read(cx).reference_mentions().is_empty()
            || self.skill_intent.is_some()
        {
            Some("ui.compact_remove_attachments")
        } else if !self.can_submit_message(cx)
            || self.actions.running
            || self.permissions_revoked
            || !self.context_control.compactable
            || self.context_control.source_fingerprint.is_none()
        {
            Some("ui.compact_unavailable")
        } else {
            None
        };
        if let Some(error) = error {
            self.controller_error = Some(crate::localized_copy::LocalizedCopy::new(error));
            cx.notify();
            return true;
        }
        let Some(request_id) = self.reserve_context_operation_id(cx) else {
            return true;
        };
        let summary_requirements = requirements.to_owned();
        self.invalidate_composer_prediction(cx);
        self.close_composer_popovers(cx);
        self.controller_error = None;
        self.input.update(cx, |input, cx| input.set_text("", cx));
        self.context_control.command_draft =
            Some((request_id, original, self.input.read(cx).content_revision()));
        self.context_control.compact_pending = Some(request_id);
        cx.emit(ContextCompactionRequested {
            thread_id: self.thread.id.clone(),
            model: self.thread.model.clone(),
            request_id,
            selection: None,
            summary_requirements,
        });
        cx.notify();
        true
    }

    fn finish_compact_command(&mut self, request_id: u64, restore: bool, cx: &mut Context<Self>) {
        if self
            .context_control
            .command_draft
            .as_ref()
            .is_some_and(|(id, _, _)| *id == request_id)
            && let Some((_, text, revision)) = self.context_control.command_draft.take()
            && restore
            && self.input.read(cx).content_revision() == revision
            && self.input.read(cx).text().is_empty()
        {
            self.input.update(cx, |input, cx| input.set_text(&text, cx));
        }
    }

    pub(crate) fn context_usage_visible(&self) -> bool {
        !self.entries.is_empty() && self.context_control.last_call_total.is_some()
    }

    pub fn set_compaction_source(&mut self, fingerprint: String) {
        self.context_control.source_fingerprint = Some(fingerprint);
    }

    pub(crate) fn prepare_partial_compaction_menu(&self, index: usize, cx: &mut Context<Self>) {
        use gpui_kit::component::menu::PopupMenuItem;
        let Some(copy) = self.copy_source_for_entry(index) else {
            return;
        };
        *copy.compaction_menu.borrow_mut() = None;
        use vega_conversation::types::{PartialCompactionDirection, PartialCompactionSelection};
        let Some(message_id) = self
            .entry_identities
            .get(index)
            .and_then(|identity| identity.message_id.clone())
        else {
            return;
        };
        let Some(fingerprint) = self.context_control.source_fingerprint.clone() else {
            return;
        };
        if !self.message_actions_enabled()
            || self.context_control.busy()
            || self.reading_preview
            || self.permissions_revoked
        {
            return;
        }
        if !matches!(
            self.entries.get(index),
            Some(StreamEntry::User { .. } | StreamEntry::Assistant { .. })
        ) {
            return;
        }
        let weak = cx.entity().downgrade();
        *copy.compaction_menu.borrow_mut() = Some(std::rc::Rc::new(move |mut menu, _, _| {
            for (direction, label) in [
                (
                    PartialCompactionDirection::UpTo,
                    "ui.compact_before_message",
                ),
                (PartialCompactionDirection::From, "ui.compact_from_message"),
            ] {
                let weak = weak.clone();
                let selection = PartialCompactionSelection {
                    message_id: message_id.clone(),
                    source_fingerprint: fingerprint.clone(),
                    direction,
                };
                menu = menu.item(PopupMenuItem::new(vega_i18n::t(label)).on_click(
                    move |_, _, cx| {
                        let _ = weak.update(cx, |stream, cx| {
                            if !stream.message_actions_enabled() || stream.context_control.busy() {
                                return;
                            }
                            let Some(request_id) = stream.reserve_context_operation_id(cx) else {
                                return;
                            };
                            stream.context_control.compact_pending = Some(request_id);
                            cx.emit(ContextCompactionRequested {
                                thread_id: stream.thread.id.clone(),
                                model: stream.thread.model.clone(),
                                request_id,
                                selection: Some(selection.clone()),
                                summary_requirements: String::new(),
                            });
                        });
                    },
                ));
            }
            menu
        }));
    }

    pub(crate) fn context_usage_display(&self) -> context_usage::ContextUsageDisplay {
        context_usage::ContextUsageDisplay::new(
            self.context_control.last_call_total,
            self.context_control.model_input_limit,
            self.context_control.next_request,
        )
    }

    pub(crate) fn reset_context_control(&mut self, cx: &mut Context<Self>) {
        let next = self.context_control.next_operation_id;
        self.context_control = ContextControl::new(cx);
        self.context_control.next_operation_id = next;
        self.context_control.retired_through = next;
        self.context_usage_trigger_hovered = false;
    }

    /// Whether context preparation/save currently excludes a competing submit.
    pub fn context_operation_busy(&self) -> bool {
        self.context_control.busy()
    }

    pub fn retire_context_operation(
        &mut self,
        thread_id: &str,
        model: &str,
        request_id: u64,
        cx: &mut Context<Self>,
    ) {
        if thread_id != self.thread.id {
            return;
        }
        let owns_record = model == self.thread.model
            && self.context_control.records.last().is_some_and(|record| {
                record.generation == request_id && record.status == Status::Compacting
            });
        let owns_pending =
            model == self.thread.model && self.context_control.compact_pending == Some(request_id);
        let index = self.entries.iter().position(|entry| {
            matches!(entry, StreamEntry::ContextCompaction { model: owner, record, restored: false }
                if owner == model && record.generation == request_id
                    && record.status == Status::Compacting)
        });
        if !owns_record && !owns_pending && index.is_none() {
            return;
        }
        if owns_record {
            self.context_control.records.clear();
        }
        if owns_pending {
            self.finish_compact_command(request_id, true, cx);
            self.context_control.compact_pending = None;
        }
        if owns_pending || (owns_record && self.context_control.compact_pending.is_none()) {
            self.context_control.cancel_pending = false;
        }
        self.context_control.retired_through = self.context_control.retired_through.max(request_id);
        for group in self.run_activity_groups.values() {
            group.update(cx, |group, cx| {
                group.remove_compactions(model, request_id..=request_id, cx);
            });
        }
        if let Some(index) = index {
            self.entries.remove(index);
            self.list_remove(index);
            self.shift_entry_indices(index, 1);
            self.invalidate_item(Some(index));
        }
        self.refresh_run_notices(cx);
        cx.notify();
    }

    #[cfg(feature = "context-test-support")]
    pub fn context_status_snapshot(&self) -> Option<ContextCompactionStatusRecord> {
        self.context_control.records.last().cloned()
    }

    #[cfg(feature = "context-test-support")]
    pub fn context_status_rows(&self) -> Vec<(String, ContextCompactionStatusRecord)> {
        self.entries
            .iter()
            .filter_map(|entry| match entry {
                StreamEntry::ContextCompaction { model, record, .. } => {
                    Some((model.clone(), record.clone()))
                }
                _ => None,
            })
            .collect()
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
        let id = self.context_control.next_operation_id.checked_add(1)?;
        self.context_control.next_operation_id = id;
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
        if self.context_control.last_call_total.is_none() {
            self.context_control.last_call_total = usage.last_call_total;
        }
        if self.context_control.live_accounting.is_none() {
            self.context_control.next_request =
                usage.estimated_tokens.map(|tokens| NextRequestEstimate {
                    tokens,
                    source: NextRequestSource::Estimated,
                });
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
            self.context_control.error = Some("ui.could_not_save_settings_try_again");
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
        self.context_control.next_operation_id = self
            .context_control
            .next_operation_id
            .max(record.generation);
        let through = if record.status == Status::Compacting {
            record.generation.saturating_sub(1)
        } else {
            record.generation
        };
        for group in self.run_activity_groups.values() {
            group.update(cx, |group, cx| {
                group.remove_compactions(model, 0..=through, cx)
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
        if self.context_control.live_accounting.is_none()
            && let Some(estimated) = record.estimated_tokens
        {
            self.context_control.next_request = Some(NextRequestEstimate {
                tokens: estimated,
                source: NextRequestSource::Estimated,
            });
        }
        if record.status != Status::Compacting {
            self.finish_compact_command(record.generation, record.status != Status::Succeeded, cx);
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
        if record.status == Status::Compacting
            && let Some(message_id) =
                self.entries
                    .iter()
                    .zip(&self.entry_identities)
                    .find_map(|(entry, identity)| match entry {
                        StreamEntry::ContextCompaction {
                            model: owner,
                            record: owned,
                            ..
                        } if owner == model && owned.generation == record.generation => {
                            identity.message_id.clone()
                        }
                        _ => None,
                    })
            && self
                .active_run_activity
                .as_ref()
                .is_some_and(|(active, group)| {
                    active == &message_id
                        && group
                            .read(cx)
                            .compaction_segment(model, record.generation)
                            .is_none()
                })
            && let Some((group, segment)) = self.ensure_live_run_activity_segment(&message_id, cx)
        {
            group.update(cx, |group, cx| {
                group.append_compaction(segment, model, record.generation, cx)
            });
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
                .is_some_and(|(old_id, old)| {
                    old_id != message_id
                        || record.revision < old.revision
                        || (record.revision == old.revision
                            && old.stage == ContextAccountingStage::PostSummaryTarget
                            && record.stage != ContextAccountingStage::PostSummaryTarget)
                })
        {
            return false;
        }
        let new_boundary = self
            .context_control
            .live_accounting
            .as_ref()
            .is_none_or(|(_, old)| {
                record.revision > old.revision
                    || (record.stage == ContextAccountingStage::PostSummaryTarget
                        && old.stage != ContextAccountingStage::PostSummaryTarget)
            });
        if new_boundary {
            self.context_control.live_call = None;
        }
        self.context_control.next_request = Some(NextRequestEstimate {
            tokens: record.predicted_input,
            source: match record.source {
                vega_conversation::types::ContextAccountingSource::UsageAnchored => {
                    NextRequestSource::UsageAnchored
                }
                vega_conversation::types::ContextAccountingSource::Estimated => {
                    NextRequestSource::Estimated
                }
            },
        });
        // The record carries the runtime's own per-run input budget, so the
        // ring has a real denominator during the run instead of only after the
        // post-run projection refresh.
        self.context_control.model_input_limit = Some(record.input_budget);
        self.context_control.live_accounting = Some((message_id.to_string(), record));
        cx.notify();
        true
    }

    pub(crate) fn begin_context_response(&mut self, response_id: &str) {
        if self.thread.backend != TaskBackend::Native || !self.actions.running {
            return;
        }
        if self
            .context_control
            .live_call
            .as_ref()
            .is_none_or(|call| call.response_id != response_id)
        {
            self.context_control.live_call = Some(PrimaryContextCall {
                response_id: response_id.to_owned(),
                total: None,
            });
        }
    }

    pub fn apply_context_usage(
        &mut self,
        thread_id: &str,
        model: &str,
        message_id: &str,
        usage: TokenUsage,
        cx: &mut Context<Self>,
    ) -> bool {
        if thread_id != self.thread.id
            || model != self.thread.model
            || self.thread.backend != TaskBackend::Native
            || !self.actions.running
            || !self
                .active_agent_message
                .as_ref()
                .is_some_and(|(active, _)| active == message_id)
            || usage.input == 0
        {
            return false;
        }
        let Some(total) = usage.input.checked_add(usage.output) else {
            return false;
        };
        let Some(call) = self.context_control.live_call.as_mut() else {
            return false;
        };
        if self.active_response_id.as_deref() != Some(call.response_id.as_str()) {
            return false;
        }
        let total = call.total.map_or(total, |old| old.max(total));
        call.total = Some(total);
        self.context_control.last_call_total = Some(total);
        cx.notify();
        true
    }

    pub(crate) fn reset_live_context_usage(&mut self) {
        self.context_control.live_call = None;
    }

    pub(crate) fn clear_live_context_accounting(&mut self) {
        self.context_control.live_call = None;
        if self.context_control.live_accounting.take().is_some()
            && self.context_control.last_call_total.is_none()
        {
            self.context_usage_trigger_hovered = false;
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
        Status::Unknown => vega_i18n::t("ui.context_capacity_is_not_configured"),
        Status::Ready => vega_i18n::t("ui.context_ready"),
        Status::Compacting => vega_i18n::t("ui.compacting_context"),
        Status::Succeeded => vega_i18n::t("ui.context_compacted"),
        Status::Cancelled => vega_i18n::t("ui.context_compaction_cancelled"),
        Status::Failed => failure.unwrap_or(Failure::Unavailable).message(),
    }
}

fn compact_requirements(text: &str) -> Option<&str> {
    let rest = text.strip_prefix("/compact")?;
    (rest.is_empty() || rest.starts_with(char::is_whitespace)).then(|| rest.trim())
}

#[cfg(test)]
mod compact_command_tests;
