use super::*;

#[derive(Clone, Copy)]
pub(super) enum Rejection {
    Ownership,
    SourceFence,
    HistoryProjection,
    HistoryOwned,
    HistoryMessages,
    HistoryGroup,
    HistoryCall,
    HistoryOffset,
    HistoryResult,
    MicroPrepare,
    MicroCommit,
    WriterAdmission,
    WorkerJoin,
}

impl Rejection {
    fn reason(self) -> &'static str {
        match self {
            Self::Ownership => "compaction_ownership",
            Self::SourceFence => "compaction_source_fence",
            Self::HistoryProjection => "compaction_history_projection",
            Self::HistoryOwned => "compaction_history_owned",
            Self::HistoryMessages => "compaction_history_messages",
            Self::HistoryGroup => "compaction_history_group",
            Self::HistoryCall => "compaction_history_call",
            Self::HistoryOffset => "compaction_history_offset",
            Self::HistoryResult => "compaction_history_result",
            Self::MicroPrepare => "compaction_micro_prepare",
            Self::MicroCommit => "compaction_micro_commit",
            Self::WriterAdmission => "compaction_writer_admission",
            Self::WorkerJoin => "compaction_worker_join",
        }
    }

    pub(super) fn record(
        self,
        diagnostics: Option<&super::super::diagnostics::DiagnosticsContext>,
    ) {
        if let Some(diagnostics) = diagnostics {
            let attempt_id = ulid::Ulid::generate().to_string();
            let started_at = std::time::Instant::now();
            diagnostics.stage_event(
                &attempt_id,
                vega_store::run_diagnostics::DiagnosticPhase::ContextSummary,
                vega_store::run_diagnostics::DiagnosticState::Started,
                None,
                started_at,
                vega_store::run_diagnostics::DiagnosticMetrics::default(),
            );
            diagnostics.stage_event(
                &attempt_id,
                vega_store::run_diagnostics::DiagnosticPhase::ContextSummary,
                vega_store::run_diagnostics::DiagnosticState::Failed,
                Some(vega_store::run_diagnostics::DiagnosticFailureCode::SummarySourceChanged),
                started_at,
                vega_store::run_diagnostics::DiagnosticMetrics {
                    compaction_rejection: Some(self.reason().into()),
                    ..Default::default()
                },
            );
        }
    }

    pub(super) fn error(
        self,
        diagnostics: Option<&super::super::diagnostics::DiagnosticsContext>,
    ) -> VegaError {
        self.record(diagnostics);
        context_error(ContextRuntimeError::SourceChanged)
    }
}
