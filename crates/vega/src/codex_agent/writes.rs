use std::sync::atomic::{AtomicBool, Ordering};

use vega_conversation::codex_persistence::*;

use super::*;

pub(super) struct CodexWrites {
    service: CodexPersistence,
    failed: AtomicBool,
}

impl CodexWrites {
    pub(super) fn from_startup(service: CodexPersistence) -> Self {
        Self {
            service,
            failed: AtomicBool::new(false),
        }
    }

    pub(super) fn finish_worker(self, success: bool) -> bool {
        let settled = self.record(self.service.settle_abandoned_startup()).is_ok();
        success && settled && self.healthy()
    }

    pub(super) fn validate_external_start(&self) -> Result<(), String> {
        self.record(self.service.validate_external_start())
    }

    pub(super) fn mark_external_started(&self) {
        self.service.mark_external_started();
    }

    pub(super) fn healthy(&self) -> bool {
        !self.failed.load(Ordering::Acquire)
    }

    fn record<T>(&self, result: Result<T, String>) -> Result<T, String> {
        if let Err(error) = &result {
            self.failed.store(true, Ordering::Release);
            tracing::error!(error = %error, "Codex persistence not confirmed; no automatic replay");
        }
        result
    }

    fn clock(&self) -> Result<i64, String> {
        self.record(unix_millis().ok_or_else(|| "Codex clock unavailable".into()))
    }

    pub(super) fn fail(&self, error: &str) {
        let _ = self.record::<()>(Err(error.into()));
    }

    fn available(&self) -> Result<(), String> {
        if self.healthy() {
            Ok(())
        } else {
            Err("Codex persistence requires reconciliation".into())
        }
    }

    pub(super) fn session(&self, outcome: CodexSessionOutcome) -> Result<(), String> {
        self.available()?;
        self.record(self.service.session_outcome(outcome, self.clock()?))
    }

    pub(super) fn messages(&self, prompt: &str) -> Result<CodexMessageReceipt, String> {
        self.available()?;
        self.record(self.service.create_messages(
            prompt.to_owned(),
            ulid::Ulid::generate().to_string(),
            ulid::Ulid::generate().to_string(),
            self.clock()?,
        ))
    }

    pub(super) fn stream(&self, message: &str, content: &str) -> Result<(), String> {
        self.available()?;
        self.record(self.service.stream(message.into(), content.into()))
    }

    pub(super) fn finish(
        &self,
        message: &str,
        content: &str,
        status: &str,
        duration: i64,
    ) -> Result<(), String> {
        let status = match status {
            "done" => CodexMessageStatus::Done,
            "failed" => CodexMessageStatus::Failed,
            "interrupted" => CodexMessageStatus::Interrupted,
            _ => return Err("Codex message status invalid".into()),
        };
        self.record(
            self.service
                .finish_message(message.into(), content.into(), status, duration),
        )
    }

    pub(super) fn propose(&self, proposal: CodexToolProposal) -> Result<(), String> {
        self.available()?;
        self.record(self.service.propose_tool(proposal))
    }

    pub(super) fn tool(
        &self,
        activity: &CodexToolActivity,
        change: CodexToolChange,
    ) -> Result<(), String> {
        if matches!(&change, CodexToolChange::Start { .. }) {
            self.available()?;
        }
        self.record(self.service.change_tool(
            CodexToolSource {
                call_id: activity.call_id.clone(),
                message_id: activity.message_id.clone(),
                identity: activity.identity.clone(),
                preview: activity.preview,
            },
            change,
            self.clock()?,
        ))
    }
}
