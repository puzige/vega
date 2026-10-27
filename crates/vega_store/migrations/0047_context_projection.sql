ALTER TABLE context_checkpoints ADD COLUMN projection_plan TEXT;

DROP INDEX context_checkpoint_identity;
CREATE UNIQUE INDEX context_checkpoint_identity
    ON context_checkpoints(thread_id, model, source_version, source_fingerprint,
        COALESCE(expected_previous_id, 0), COALESCE(projection_plan, ''));
