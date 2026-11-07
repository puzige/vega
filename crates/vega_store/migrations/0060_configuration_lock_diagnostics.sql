ALTER TABLE run_diagnostic_events ADD COLUMN configuration_lock TEXT
CHECK (configuration_lock IS NULL OR configuration_lock IN ('file', 'edit', 'registry'));
ALTER TABLE run_diagnostic_events ADD COLUMN configuration_lock_attempts INTEGER
CHECK (configuration_lock_attempts IS NULL OR configuration_lock_attempts BETWEEN 1 AND 6);
ALTER TABLE run_diagnostic_events ADD COLUMN configuration_lock_wait_ms INTEGER
CHECK (configuration_lock_wait_ms IS NULL OR configuration_lock_wait_ms BETWEEN 0 AND 500);
CREATE TEMP TABLE configuration_reason_upgrade (id INTEGER PRIMARY KEY, reason TEXT);
INSERT INTO configuration_reason_upgrade SELECT id, credential_reason FROM run_diagnostic_events;
ALTER TABLE run_diagnostic_events DROP COLUMN credential_reason;
ALTER TABLE run_diagnostic_events ADD COLUMN credential_reason TEXT
CHECK (credential_reason IS NULL OR (credential_stage IS NOT NULL AND credential_reason IN ('missing', 'unsafe', 'malformed', 'limit', 'credential_io', 'unsupported', 'database', 'configuration', 'state', 'worker', 'configuration_poisoned', 'configuration_directory', 'configuration_lock_open', 'configuration_lock_validation', 'configuration_read', 'configuration_parse', 'configuration_lock_acquire')));
UPDATE run_diagnostic_events SET credential_reason = (SELECT reason FROM configuration_reason_upgrade WHERE configuration_reason_upgrade.id = run_diagnostic_events.id);
DROP TABLE configuration_reason_upgrade;
