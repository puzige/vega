ALTER TABLE run_diagnostic_events ADD COLUMN protocol_reason TEXT
CHECK (protocol_reason IS NULL OR (
    protocol_reason IN ('invalid_json', 'invalid_frame', 'invalid_delta', 'after_terminal', 'reasoning_alias_conflict', 'reasoning_detail', 'tool_fragment', 'tool_identity_conflict', 'incomplete_tool', 'terminal_conflict', 'missing_terminal', 'sse_decode', 'frame_capacity', 'response_event', 'text_snapshot_conflict', 'response_output', 'usage_inconsistent')
    AND failure_source IS 'provider' AND failure_kind IS 'protocol'
));
