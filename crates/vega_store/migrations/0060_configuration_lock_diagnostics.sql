ALTER TABLE run_diagnostic_events ADD COLUMN configuration_lock TEXT
CHECK (configuration_lock IS NULL OR configuration_lock IN ('file', 'edit', 'registry'));
ALTER TABLE run_diagnostic_events ADD COLUMN configuration_lock_attempts INTEGER
CHECK (configuration_lock_attempts IS NULL OR configuration_lock_attempts BETWEEN 1 AND 6);
ALTER TABLE run_diagnostic_events ADD COLUMN configuration_lock_wait_ms INTEGER
CHECK (configuration_lock_wait_ms IS NULL OR configuration_lock_wait_ms BETWEEN 0 AND 500);
