-- Widens the persisted provider-failure vocabulary so an upstream capacity
-- notice delivered inside an established response is no longer recorded as an
-- invalid-data failure. SQLite cannot drop a CHECK constraint, so the table is
-- rebuilt with the same columns, constraints and indexes plus the new
-- allowlisted values `provider_overloaded` (failure_code) and `overloaded`
-- (failure_kind).
CREATE TABLE run_diagnostic_events_new (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
  run_id TEXT NOT NULL CHECK (length(run_id) BETWEEN 1 AND 128),
  attempt_id TEXT NOT NULL CHECK (length(attempt_id) BETWEEN 1 AND 128),
  parent_attempt_id TEXT CHECK (parent_attempt_id IS NULL OR length(parent_attempt_id) BETWEEN 1 AND 128),
  tool_call_id TEXT CHECK (tool_call_id IS NULL OR (length(tool_call_id) BETWEEN 1 AND 128 AND tool_call_id NOT GLOB '*[^A-Za-z0-9_-]*')),
  phase TEXT NOT NULL CHECK (phase IN ('run', 'primary_model', 'context_summary', 'tool')),
  state TEXT NOT NULL CHECK (state IN ('started', 'succeeded', 'failed', 'cancelled', 'interrupted')),
  failure_code TEXT CHECK (failure_code IS NULL OR failure_code IN ('cancelled', 'interrupted', 'context_over_limit', 'reasoning_limit', 'summary_timeout', 'summary_truncated', 'summary_empty', 'summary_format_invalid', 'summary_projection_invalid', 'summary_source_changed', 'summary_source_too_large', 'summary_aggregate_too_large', 'summary_images_unsupported', 'summary_no_compactable_prefix', 'summary_already_attempted', 'provider_http', 'provider_transport', 'provider_stream_interrupted', 'provider_timeout', 'provider_overloaded', 'provider_transport_or_stream', 'provider_protocol', 'provider_rejected', 'tool_failed', 'tool_rejected', 'unknown_safe_failure')),
  occurred_at INTEGER NOT NULL CHECK (occurred_at >= 0),
  duration_ms INTEGER CHECK (duration_ms IS NULL OR duration_ms >= 0),
  stop_reason TEXT CHECK (stop_reason IS NULL OR stop_reason IN ('end', 'tool_use', 'length')),
  input_tokens INTEGER CHECK (input_tokens IS NULL OR input_tokens >= 0),
  output_tokens INTEGER CHECK (output_tokens IS NULL OR output_tokens >= 0),
  cache_read_tokens INTEGER CHECK (cache_read_tokens IS NULL OR cache_read_tokens >= 0),
  cache_write_tokens INTEGER CHECK (cache_write_tokens IS NULL OR cache_write_tokens >= 0),
  visible_output_bytes INTEGER CHECK (visible_output_bytes IS NULL OR visible_output_bytes >= 0),
  tool_output_bytes INTEGER CHECK (tool_output_bytes IS NULL OR tool_output_bytes >= 0),
  tool_truncated INTEGER CHECK (tool_truncated IS NULL OR tool_truncated IN (0, 1)),
  http_status INTEGER CHECK (http_status IS NULL OR http_status BETWEEN 100 AND 599),
  request_id TEXT CHECK (request_id IS NULL OR (length(request_id) BETWEEN 1 AND 128 AND request_id NOT GLOB '*[^A-Za-z0-9._:-]*')),
  retry_count INTEGER CHECK (retry_count IS NULL OR retry_count >= 0),
  failure_source TEXT CHECK (failure_source IS NULL OR failure_source IN ('runtime', 'provider')),
  failure_kind TEXT CHECK (failure_kind IS NULL OR failure_kind IN ('unknown', 'http', 'transport', 'stream_interrupted', 'timeout', 'overloaded', 'protocol', 'rejected', 'output_limit', 'reasoning_limit', 'context_over_limit', 'cancelled', 'quota', 'authorization', 'rate_limited', 'credential_exposure', 'credential_check', 'credential_continuation', 'credential_projection')),
  failure_scope TEXT CHECK (failure_scope IS NULL OR failure_scope IN ('delta', 'response', 'run', 'request_input', 'request_context', 'summary_input', 'compaction_result', 'sse_frame')),
  failure_observed INTEGER CHECK (failure_observed IS NULL OR failure_observed >= 0),
  failure_limit INTEGER CHECK (failure_limit IS NULL OR failure_limit >= 0),
  failure_units TEXT CHECK (failure_units IS NULL OR failure_units IN ('bytes', 'tokens')),
  failure_observed_items INTEGER CHECK (failure_observed_items IS NULL OR failure_observed_items >= 0),
  failure_limit_items INTEGER CHECK (failure_limit_items IS NULL OR failure_limit_items >= 0),
  failure_tool_index INTEGER CHECK (failure_tool_index IS NULL OR failure_tool_index >= 0),
  runtime_stage TEXT CHECK (runtime_stage IS NULL OR runtime_stage IN ('run_preparation', 'snapshot', 'tool_proposal', 'tool_approval', 'tool_running', 'tool_terminal', 'event_pipeline')),
  runtime_reason TEXT CHECK (runtime_reason IS NULL OR runtime_reason IN ('database', 'approval_audit', 'state', 'io', 'pipeline')),
  sqlite_code INTEGER CHECK (sqlite_code IS NULL OR (typeof(sqlite_code) = 'integer' AND sqlite_code BETWEEN 1 AND 255)),
  sqlite_extended_code INTEGER CHECK (sqlite_extended_code IS NULL OR (typeof(sqlite_extended_code) = 'integer' AND sqlite_extended_code BETWEEN 1 AND 2147483647)),
  io_kind TEXT CHECK (io_kind IS NULL OR io_kind IN ('not_found', 'permission_denied', 'already_exists', 'invalid_input', 'invalid_data', 'timed_out', 'interrupted', 'unsupported', 'out_of_memory', 'storage_full', 'other')) CHECK (
    (runtime_stage IS NULL AND runtime_reason IS NULL AND sqlite_code IS NULL AND sqlite_extended_code IS NULL AND io_kind IS NULL)
    OR (
        runtime_stage IS NOT NULL AND runtime_reason IS NOT NULL
        AND (
            (sqlite_code IS NULL AND sqlite_extended_code IS NULL)
            OR (runtime_reason = 'database' AND sqlite_code IS NOT NULL AND sqlite_extended_code IS NOT NULL AND (sqlite_extended_code & 255) = sqlite_code)
        )
        AND (
            (runtime_reason = 'io' AND io_kind IS NOT NULL)
            OR (runtime_reason <> 'io' AND io_kind IS NULL)
        )
    )
  ),
  CHECK ((state = 'started' AND duration_ms IS NULL) OR (state <> 'started' AND duration_ms IS NOT NULL)),
  CHECK (
    (state IN ('started', 'succeeded') AND failure_code IS NULL)
    OR (state = 'failed' AND failure_code IS NOT NULL)
    OR (state = 'cancelled' AND failure_code = 'cancelled')
    OR (state = 'interrupted' AND failure_code = 'interrupted')
  )
);

INSERT INTO run_diagnostic_events_new (
  id, thread_id, run_id, attempt_id, parent_attempt_id, tool_call_id, phase, state,
  failure_code, occurred_at, duration_ms, stop_reason, input_tokens, output_tokens,
  cache_read_tokens, cache_write_tokens, visible_output_bytes, tool_output_bytes,
  tool_truncated, http_status, request_id, retry_count, failure_source, failure_kind,
  failure_scope, failure_observed, failure_limit, failure_units, failure_observed_items,
  failure_limit_items, failure_tool_index, runtime_stage, runtime_reason, sqlite_code,
  sqlite_extended_code, io_kind
)
SELECT
  id, thread_id, run_id, attempt_id, parent_attempt_id, tool_call_id, phase, state,
  failure_code, occurred_at, duration_ms, stop_reason, input_tokens, output_tokens,
  cache_read_tokens, cache_write_tokens, visible_output_bytes, tool_output_bytes,
  tool_truncated, http_status, request_id, retry_count, failure_source, failure_kind,
  failure_scope, failure_observed, failure_limit, failure_units, failure_observed_items,
  failure_limit_items, failure_tool_index, runtime_stage, runtime_reason, sqlite_code,
  sqlite_extended_code, io_kind
FROM run_diagnostic_events;

DROP TABLE run_diagnostic_events;

ALTER TABLE run_diagnostic_events_new RENAME TO run_diagnostic_events;

CREATE INDEX idx_run_diagnostic_thread_run_id
  ON run_diagnostic_events(thread_id, run_id, id);
CREATE INDEX idx_run_diagnostic_thread_id
  ON run_diagnostic_events(thread_id, id);
CREATE UNIQUE INDEX idx_run_diagnostic_one_start
  ON run_diagnostic_events(thread_id, run_id, attempt_id)
  WHERE state = 'started';
CREATE UNIQUE INDEX idx_run_diagnostic_one_terminal
  ON run_diagnostic_events(thread_id, run_id, attempt_id)
  WHERE state <> 'started';
