CREATE TABLE run_diagnostic_events (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
  run_id TEXT NOT NULL CHECK (length(run_id) BETWEEN 1 AND 128),
  attempt_id TEXT NOT NULL CHECK (length(attempt_id) BETWEEN 1 AND 128),
  parent_attempt_id TEXT CHECK (parent_attempt_id IS NULL OR length(parent_attempt_id) BETWEEN 1 AND 128),
  tool_call_id TEXT CHECK (tool_call_id IS NULL OR (length(tool_call_id) BETWEEN 1 AND 128 AND tool_call_id NOT GLOB '*[^A-Za-z0-9_-]*')),
  phase TEXT NOT NULL CHECK (phase IN ('run', 'primary_model', 'context_summary', 'tool')),
  state TEXT NOT NULL CHECK (state IN ('started', 'succeeded', 'failed', 'cancelled', 'interrupted')),
  failure_code TEXT CHECK (failure_code IS NULL OR failure_code IN ('cancelled', 'interrupted', 'context_over_limit', 'reasoning_limit', 'summary_timeout', 'summary_truncated', 'summary_empty', 'summary_format_invalid', 'summary_projection_invalid', 'summary_source_changed', 'summary_source_too_large', 'summary_aggregate_too_large', 'summary_images_unsupported', 'summary_no_compactable_prefix', 'summary_already_attempted', 'provider_http', 'provider_transport_or_stream', 'provider_protocol', 'provider_rejected', 'tool_failed', 'tool_rejected', 'unknown_safe_failure')),
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
  CHECK ((state = 'started' AND duration_ms IS NULL) OR (state <> 'started' AND duration_ms IS NOT NULL)),
  CHECK (
    (state IN ('started', 'succeeded') AND failure_code IS NULL)
    OR (state = 'failed' AND failure_code IS NOT NULL)
    OR (state = 'cancelled' AND failure_code = 'cancelled')
    OR (state = 'interrupted' AND failure_code = 'interrupted')
  )
);

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
