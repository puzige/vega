use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
use vega_runtime::{
    RuntimeDiagnosticPhase, RuntimeDiagnosticState, RuntimeEvent, RuntimeFinishReason,
    RuntimeToolCall, RuntimeToolResult, RuntimeToolStatus, VegaError,
};

use super::protocol::{digest, json_digest};

pub(super) struct Recorder {
    pub(super) trace: Vec<Value>,
    pub(super) terminal: Option<&'static str>,
    pub(super) failure: Option<&'static str>,
    pub(super) budget_exhausted: bool,
    calls: BTreeMap<String, String>,
    proposals: BTreeSet<String>,
    primary: BTreeSet<String>,
    running: BTreeSet<String>,
    finished: BTreeSet<String>,
    tool_proposals: u64,
    rejected: u64,
    repeated: u64,
    usage_count: usize,
    input: u64,
    output: u64,
    cost: Option<i64>,
    retry_seen: bool,
    pricing: Vec<Value>,
    token_limit: u64,
    cancel: CancellationToken,
    known_credentials: Vec<String>,
}

impl Recorder {
    pub(super) fn new(
        token_limit: u64,
        cancel: CancellationToken,
        known_credentials: Vec<String>,
    ) -> Self {
        Self {
            trace: Vec::new(),
            terminal: None,
            failure: None,
            budget_exhausted: false,
            calls: BTreeMap::new(),
            proposals: BTreeSet::new(),
            primary: BTreeSet::new(),
            running: BTreeSet::new(),
            finished: BTreeSet::new(),
            tool_proposals: 0,
            rejected: 0,
            repeated: 0,
            usage_count: 0,
            input: 0,
            output: 0,
            cost: Some(0),
            retry_seen: false,
            pricing: Vec::new(),
            token_limit,
            cancel,
            known_credentials,
        }
    }

    fn proposed(&mut self, call: &RuntimeToolCall) -> Result<(), VegaError> {
        if vega_runtime::inspect_credential_text(&call.input_json, &self.known_credentials)
            .iter()
            .any(|finding| finding.confidence == vega_runtime::CredentialConfidence::KnownExact)
        {
            self.failure = Some("credential_exposure_blocked");
            return Err(VegaError::CredentialExposureBlocked);
        }
        let name = match call.name.as_str() {
            "Read" => "read",
            "Write" => "write",
            "Edit" => "edit",
            other => other,
        };
        self.calls.insert(call.id.clone(), name.to_owned());
        self.tool_proposals += 1;
        let input = serde_json::from_str::<Value>(&call.input_json)
            .unwrap_or_else(|_| json!({"invalid_json_sha256": digest(call.input_json.as_bytes())}));
        let signature =
            json_digest(&json!({"tool": name, "input": input})).map_err(|_| VegaError::Tool {
                tool: "harness_eval".into(),
                message: "trace encoding failed".into(),
            })?;
        if !self.proposals.insert(signature) {
            self.repeated += 1;
        }
        self.trace.push(json!({
            "event": "tool_proposed", "tool": name, "call_id": call.id, "input": input,
        }));
        Ok(())
    }

    fn tool_finished(&mut self, result: &RuntimeToolResult) {
        if !self.finished.insert(result.call_id.clone()) {
            return;
        }
        let name = self.calls.get(&result.call_id).map(String::as_str);
        if result.status == RuntimeToolStatus::Rejected {
            self.rejected += 1;
        }
        if result.status == RuntimeToolStatus::Success
            && !result.reused
            && matches!(name, Some("write" | "edit"))
        {
            self.proposals.clear();
        }
        self.trace.push(json!({
            "event": "tool_finished", "tool": name, "call_id": result.call_id,
            "status": match result.status {
                RuntimeToolStatus::Rejected => "rejected",
                RuntimeToolStatus::Success => "success",
                RuntimeToolStatus::Failed => "failed",
                RuntimeToolStatus::Cancelled => "cancelled",
            },
            "output": result.output,
            "reused": result.reused,
            "exit_code": result.exit_code,
            "duration_ms": result.duration_ms,
            "truncated": result.truncated,
        }));
    }

    pub(super) fn observe(&mut self, event: RuntimeEvent) -> Result<(), VegaError> {
        match event {
            RuntimeEvent::ToolCallProposed(call) => self.proposed(&call)?,
            RuntimeEvent::ToolCallValidationRejected { call, result }
            | RuntimeEvent::ToolCallConflict { call, result } => {
                self.proposed(&call)?;
                self.tool_finished(&result);
            }
            RuntimeEvent::ToolCallRunning { call_id, .. } => {
                self.running.insert(call_id.clone());
                self.trace
                    .push(json!({"event": "tool_running", "call_id": call_id}));
            }
            RuntimeEvent::ToolCallFinished(result)
            | RuntimeEvent::ToolReadFinished { result, .. } => self.tool_finished(&result),
            RuntimeEvent::StageHandoff { checkpoint_id } => {
                self.terminal = Some("stage_handoff");
                self.trace.push(json!({"event":"stage_handoff","origin":"runtime","checkpoint_id":checkpoint_id}));
            }
            RuntimeEvent::ToolCallApproved { call_id, audit, .. } => {
                self.trace.push(json!({
                    "event": "tool_approved", "call_id": call_id,
                    "decision": format!("{:?}", audit.decision),
                    "source": format!("{:?}", audit.source),
                }));
            }
            RuntimeEvent::DiagnosticAttempt(attempt) => {
                if attempt.phase == RuntimeDiagnosticPhase::PrimaryModel
                    && attempt.state == RuntimeDiagnosticState::Started
                {
                    self.primary.insert(attempt.attempt_id.clone());
                }
                self.trace.push(json!({
                    "event": "diagnostic", "attempt_id": attempt.attempt_id,
                    "phase": format!("{:?}", attempt.phase),
                    "state": format!("{:?}", attempt.state),
                    "duration_ms": attempt.duration_ms,
                    "failure": attempt.failure.map(|value| format!("{value:?}")),
                    "http_status": attempt.metrics.provider.http_status,
                    "retry_count": attempt.metrics.provider.retry_count,
                }));
            }
            RuntimeEvent::ResponseRetrying {
                attempt,
                max_retries,
                ..
            } => {
                self.retry_seen = true;
                self.trace.push(json!({
                    "event": "response_retrying", "attempt": attempt, "max_retries": max_retries,
                }));
            }
            RuntimeEvent::UsageUpdated {
                usage,
                cost_microcents,
                pricing,
            } => {
                self.usage_count += 1;
                self.input = self.input.saturating_add(usage.input);
                self.output = self.output.saturating_add(usage.output);
                let price = pricing.map(|source| {
                    json!({
                        "version": source.version, "profile": source.profile,
                        "call_started_at": source.call_started_at,
                    })
                });
                self.cost = if price.is_some() {
                    self.cost.and_then(|cost| cost.checked_add(cost_microcents))
                } else {
                    None
                };
                if let Some(source) = &price {
                    self.pricing.push(source.clone());
                }
                self.trace.push(json!({
                    "event": "usage", "input_tokens": usage.input, "output_tokens": usage.output,
                    "cache_read_tokens": usage.cache_read, "cache_write_tokens": usage.cache_write,
                    "cost_microcents": price.as_ref().map(|_| cost_microcents), "pricing": price,
                }));
                if self.input.saturating_add(self.output) >= self.token_limit {
                    self.budget_exhausted = true;
                    self.cancel.cancel();
                }
            }
            RuntimeEvent::SkillActivation { audit, .. } => {
                self.trace.push(json!({
                    "event": "skill_activation", "name": audit.name, "status": audit.status,
                    "content_sha256": audit.content_sha256,
                    "origin": match audit.origin {
                        vega_runtime::skills::ActivationOrigin::Model => "model",
                        vega_runtime::skills::ActivationOrigin::ExplicitUser => "explicit_user",
                    },
                }));
            }
            RuntimeEvent::GeneratedNotice {
                kind,
                text_start_bytes,
                text_end_bytes,
            } => {
                let (kind, turn_limit, delivered_images) = match kind {
                    vega_runtime::RuntimeGeneratedNoticeKind::TurnLimit { limit } => {
                        ("turn_limit", Some(limit), None)
                    }
                    vega_runtime::RuntimeGeneratedNoticeKind::ComputerStopped {
                        delivered_images,
                    } => ("computer_stopped", None, Some(delivered_images)),
                };
                self.trace.push(json!({
                    "event": "generated_notice",
                    "kind": kind,
                    "turn_limit": turn_limit,
                    "delivered_images": delivered_images,
                    "text_start_bytes": text_start_bytes,
                    "text_end_bytes": text_end_bytes,
                }));
            }
            RuntimeEvent::Finished(reason) => {
                let terminal = match reason {
                    RuntimeFinishReason::End => "end",
                    RuntimeFinishReason::Length => "length",
                    RuntimeFinishReason::TurnLimit => "turn_limit",
                };
                self.terminal = Some(terminal);
                self.trace
                    .push(json!({"event": "finished", "reason": terminal}));
            }
            RuntimeEvent::Interrupted => {
                self.terminal = Some("cancelled");
                self.trace.push(json!({"event": "interrupted"}));
            }
            RuntimeEvent::Error(error) => {
                let category = failure_category(&error);
                self.failure = Some(category);
                self.terminal = Some("failed");
                self.trace
                    .push(json!({"event": "error", "category": category}));
            }
            _ => {}
        }
        Ok(())
    }

    pub(super) fn metrics(&self, actual_requests: usize) -> Value {
        let usage_complete =
            actual_requests > 0 && self.usage_count == actual_requests && !self.retry_seen;
        let known = usage_complete && self.cost.is_some();
        json!({
            "primary_model_requests": actual_requests,
            "primary_model_attempts": self.primary.len(),
            "tool_proposals": self.tool_proposals,
            "tool_executions": self.running.len(),
            "rejected_tool_calls": self.rejected,
            "repeated_tool_proposals": self.repeated,
            "input_tokens": (self.usage_count > 0).then_some(self.input),
            "output_tokens": (self.usage_count > 0).then_some(self.output),
            "cost_microcents": if known { self.cost } else { None },
            "cost_units_per_usd": 1_000_000,
            "pricing_known": known,
            "usage_complete": usage_complete,
            "pricing_provenance": self.pricing,
        })
    }
}

pub(super) fn failure_category(error: &VegaError) -> &'static str {
    match error {
        VegaError::Provider { .. } | VegaError::ProviderDiagnostic { .. } => "provider_failure",
        VegaError::Cancelled => "cancelled",
        VegaError::CredentialExposureBlocked => "credential_exposure_blocked",
        VegaError::CredentialCheckUnavailable => "credential_check_unavailable",
        VegaError::Io(_) => "io_failure",
        VegaError::Tool { .. } => "tool_failure",
        _ => "runtime_failure",
    }
}
