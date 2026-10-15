ALTER TABLE run_diagnostic_events ADD COLUMN failure_source TEXT CHECK (failure_source IS NULL OR failure_source IN ('runtime', 'provider'));
ALTER TABLE run_diagnostic_events ADD COLUMN failure_kind TEXT CHECK (failure_kind IS NULL OR failure_kind IN ('unknown', 'http', 'transport', 'protocol', 'rejected', 'output_limit', 'reasoning_limit', 'context_over_limit', 'cancelled', 'quota', 'authorization', 'rate_limited', 'credential_exposure', 'credential_check', 'credential_continuation', 'credential_projection'));
ALTER TABLE run_diagnostic_events ADD COLUMN failure_scope TEXT CHECK (failure_scope IS NULL OR failure_scope IN ('delta', 'response', 'run', 'request_input', 'request_context', 'summary_input', 'compaction_result', 'sse_frame'));
ALTER TABLE run_diagnostic_events ADD COLUMN failure_observed INTEGER CHECK (failure_observed IS NULL OR failure_observed >= 0);
ALTER TABLE run_diagnostic_events ADD COLUMN failure_limit INTEGER CHECK (failure_limit IS NULL OR failure_limit >= 0);
ALTER TABLE run_diagnostic_events ADD COLUMN failure_units TEXT CHECK (failure_units IS NULL OR failure_units IN ('bytes', 'tokens'));
ALTER TABLE run_diagnostic_events ADD COLUMN failure_observed_items INTEGER CHECK (failure_observed_items IS NULL OR failure_observed_items >= 0);
ALTER TABLE run_diagnostic_events ADD COLUMN failure_limit_items INTEGER CHECK (failure_limit_items IS NULL OR failure_limit_items >= 0);
ALTER TABLE run_diagnostic_events ADD COLUMN failure_tool_index INTEGER CHECK (failure_tool_index IS NULL OR failure_tool_index >= 0);
