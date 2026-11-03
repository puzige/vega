ALTER TABLE run_diagnostic_events ADD COLUMN credential_stage TEXT
CHECK (credential_stage IS NULL OR (failure_kind IS 'credential_check' AND credential_stage IN ('provider', 'bindings', 'mcp', 'privacy', 'authority', 'worker')));
ALTER TABLE run_diagnostic_events ADD COLUMN credential_reason TEXT
CHECK (credential_reason IS NULL OR (credential_stage IS NOT NULL AND credential_reason IN ('missing', 'unsafe', 'malformed', 'limit', 'credential_io', 'unsupported', 'database', 'configuration', 'state', 'worker')));
