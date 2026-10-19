ALTER TABLE run_diagnostic_events ADD COLUMN runtime_stage TEXT CHECK (runtime_stage IS NULL OR runtime_stage IN ('run_preparation', 'snapshot', 'tool_proposal', 'tool_approval', 'tool_running', 'tool_terminal', 'event_pipeline'));
ALTER TABLE run_diagnostic_events ADD COLUMN runtime_reason TEXT CHECK (runtime_reason IS NULL OR runtime_reason IN ('database', 'approval_audit', 'state', 'io', 'pipeline'));
ALTER TABLE run_diagnostic_events ADD COLUMN sqlite_code INTEGER CHECK (sqlite_code IS NULL OR (typeof(sqlite_code) = 'integer' AND sqlite_code BETWEEN 1 AND 255));
ALTER TABLE run_diagnostic_events ADD COLUMN sqlite_extended_code INTEGER CHECK (sqlite_extended_code IS NULL OR (typeof(sqlite_extended_code) = 'integer' AND sqlite_extended_code BETWEEN 1 AND 2147483647));
ALTER TABLE run_diagnostic_events ADD COLUMN io_kind TEXT CHECK (io_kind IS NULL OR io_kind IN ('not_found', 'permission_denied', 'already_exists', 'invalid_input', 'invalid_data', 'timed_out', 'interrupted', 'unsupported', 'out_of_memory', 'storage_full', 'other')) CHECK (
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
);
