CREATE TRIGGER source_validation_credential_continuations_insert BEFORE INSERT ON credential_continuations
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision) SELECT owner,1 FROM (SELECT NEW.thread_id AS owner) WHERE owner IS NOT NULL ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision) SELECT owner,1 FROM (SELECT victim.thread_id AS owner FROM credential_continuations victim WHERE victim.id IS NEW.id OR victim.operation_id IS NEW.operation_id OR victim.rowid IS NEW.rowid) WHERE owner IS NOT NULL ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER source_validation_credential_continuations_update BEFORE UPDATE ON credential_continuations
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision) SELECT owner,1 FROM (SELECT OLD.thread_id AS owner) WHERE owner IS NOT NULL ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision) SELECT owner,1 FROM (SELECT NEW.thread_id AS owner) WHERE owner IS NOT NULL ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision) SELECT owner,1 FROM (SELECT victim.thread_id AS owner FROM credential_continuations victim WHERE victim.id IS NEW.id OR victim.operation_id IS NEW.operation_id OR victim.rowid IS NEW.rowid) WHERE owner IS NOT NULL ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER source_validation_credential_continuations_delete BEFORE DELETE ON credential_continuations
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision) SELECT owner,1 FROM (SELECT OLD.thread_id AS owner) WHERE owner IS NOT NULL ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER source_validation_delegated_read_receipts_insert BEFORE INSERT ON delegated_read_receipts
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision) SELECT owner,1 FROM (SELECT NEW.child_thread_id AS owner) WHERE owner IS NOT NULL ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision) SELECT owner,1 FROM (SELECT victim.child_thread_id AS owner FROM delegated_read_receipts victim WHERE victim.call_id IS NEW.call_id OR victim.rowid IS NEW.rowid) WHERE owner IS NOT NULL ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER source_validation_delegated_read_receipts_update BEFORE UPDATE ON delegated_read_receipts
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision) SELECT owner,1 FROM (SELECT OLD.child_thread_id AS owner) WHERE owner IS NOT NULL ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision) SELECT owner,1 FROM (SELECT NEW.child_thread_id AS owner) WHERE owner IS NOT NULL ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision) SELECT owner,1 FROM (SELECT victim.child_thread_id AS owner FROM delegated_read_receipts victim WHERE victim.call_id IS NEW.call_id OR victim.rowid IS NEW.rowid) WHERE owner IS NOT NULL ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER source_validation_delegated_read_receipts_delete BEFORE DELETE ON delegated_read_receipts
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision) SELECT owner,1 FROM (SELECT OLD.child_thread_id AS owner) WHERE owner IS NOT NULL ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER source_validation_delegated_permission_fences_insert BEFORE INSERT ON delegated_permission_fences
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision) SELECT owner,1 FROM (SELECT NEW.child_thread_id AS owner) WHERE owner IS NOT NULL ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision) SELECT owner,1 FROM (SELECT victim.child_thread_id AS owner FROM delegated_permission_fences victim WHERE victim.child_message_id IS NEW.child_message_id OR victim.rowid IS NEW.rowid) WHERE owner IS NOT NULL ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER source_validation_delegated_permission_fences_update BEFORE UPDATE ON delegated_permission_fences
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision) SELECT owner,1 FROM (SELECT OLD.child_thread_id AS owner) WHERE owner IS NOT NULL ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision) SELECT owner,1 FROM (SELECT NEW.child_thread_id AS owner) WHERE owner IS NOT NULL ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision) SELECT owner,1 FROM (SELECT victim.child_thread_id AS owner FROM delegated_permission_fences victim WHERE victim.child_message_id IS NEW.child_message_id OR victim.rowid IS NEW.rowid) WHERE owner IS NOT NULL ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER source_validation_delegated_permission_fences_delete BEFORE DELETE ON delegated_permission_fences
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision) SELECT owner,1 FROM (SELECT OLD.child_thread_id AS owner) WHERE owner IS NOT NULL ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER source_validation_run_permission_fences_insert BEFORE INSERT ON run_permission_fences
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision) SELECT owner,1 FROM (SELECT NEW.thread_id AS owner) WHERE owner IS NOT NULL ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision) SELECT owner,1 FROM (SELECT victim.thread_id AS owner FROM run_permission_fences victim WHERE victim.message_id IS NEW.message_id OR victim.rowid IS NEW.rowid) WHERE owner IS NOT NULL ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER source_validation_run_permission_fences_update BEFORE UPDATE ON run_permission_fences
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision) SELECT owner,1 FROM (SELECT OLD.thread_id AS owner) WHERE owner IS NOT NULL ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision) SELECT owner,1 FROM (SELECT NEW.thread_id AS owner) WHERE owner IS NOT NULL ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision) SELECT owner,1 FROM (SELECT victim.thread_id AS owner FROM run_permission_fences victim WHERE victim.message_id IS NEW.message_id OR victim.rowid IS NEW.rowid) WHERE owner IS NOT NULL ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER source_validation_run_permission_fences_delete BEFORE DELETE ON run_permission_fences
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision) SELECT owner,1 FROM (SELECT OLD.thread_id AS owner) WHERE owner IS NOT NULL ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER source_validation_context_precompute_accounting_insert BEFORE INSERT ON context_precompute_accounting
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision) SELECT owner,1 FROM (SELECT NEW.thread_id AS owner) WHERE owner IS NOT NULL ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision) SELECT owner,1 FROM (SELECT victim.thread_id AS owner FROM context_precompute_accounting victim WHERE victim.generation IS NEW.generation OR victim.rowid IS NEW.rowid) WHERE owner IS NOT NULL ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER source_validation_context_precompute_accounting_update BEFORE UPDATE ON context_precompute_accounting
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision) SELECT owner,1 FROM (SELECT OLD.thread_id AS owner) WHERE owner IS NOT NULL ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision) SELECT owner,1 FROM (SELECT NEW.thread_id AS owner) WHERE owner IS NOT NULL ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision) SELECT owner,1 FROM (SELECT victim.thread_id AS owner FROM context_precompute_accounting victim WHERE victim.generation IS NEW.generation OR victim.rowid IS NEW.rowid) WHERE owner IS NOT NULL ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER source_validation_context_precompute_accounting_delete BEFORE DELETE ON context_precompute_accounting
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision) SELECT owner,1 FROM (SELECT OLD.thread_id AS owner) WHERE owner IS NOT NULL ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER source_validation_context_precompute_slots_insert BEFORE INSERT ON context_precompute_slots
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision) SELECT owner,1 FROM (SELECT NEW.thread_id AS owner) WHERE owner IS NOT NULL ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision) SELECT owner,1 FROM (SELECT victim.thread_id AS owner FROM context_precompute_slots victim WHERE victim.thread_id IS NEW.thread_id OR victim.generation IS NEW.generation OR victim.rowid IS NEW.rowid) WHERE owner IS NOT NULL ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER source_validation_context_precompute_slots_update BEFORE UPDATE ON context_precompute_slots
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision) SELECT owner,1 FROM (SELECT OLD.thread_id AS owner) WHERE owner IS NOT NULL ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision) SELECT owner,1 FROM (SELECT NEW.thread_id AS owner) WHERE owner IS NOT NULL ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision) SELECT owner,1 FROM (SELECT victim.thread_id AS owner FROM context_precompute_slots victim WHERE victim.thread_id IS NEW.thread_id OR victim.generation IS NEW.generation OR victim.rowid IS NEW.rowid) WHERE owner IS NOT NULL ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER source_validation_context_precompute_slots_delete BEFORE DELETE ON context_precompute_slots
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision) SELECT owner,1 FROM (SELECT OLD.thread_id AS owner) WHERE owner IS NOT NULL ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER source_validation_context_precompute_ready_insert BEFORE INSERT ON context_precompute_ready
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision) SELECT owner,1 FROM (SELECT NEW.thread_id AS owner) WHERE owner IS NOT NULL ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision) SELECT owner,1 FROM (SELECT victim.thread_id AS owner FROM context_precompute_ready victim WHERE victim.thread_id IS NEW.thread_id OR victim.generation IS NEW.generation OR victim.rowid IS NEW.rowid) WHERE owner IS NOT NULL ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER source_validation_context_precompute_ready_update BEFORE UPDATE ON context_precompute_ready
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision) SELECT owner,1 FROM (SELECT OLD.thread_id AS owner) WHERE owner IS NOT NULL ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision) SELECT owner,1 FROM (SELECT NEW.thread_id AS owner) WHERE owner IS NOT NULL ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision) SELECT owner,1 FROM (SELECT victim.thread_id AS owner FROM context_precompute_ready victim WHERE victim.thread_id IS NEW.thread_id OR victim.generation IS NEW.generation OR victim.rowid IS NEW.rowid) WHERE owner IS NOT NULL ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER source_validation_context_precompute_ready_delete BEFORE DELETE ON context_precompute_ready
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision) SELECT owner,1 FROM (SELECT OLD.thread_id AS owner) WHERE owner IS NOT NULL ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER source_validation_context_precompute_stages_insert BEFORE INSERT ON context_precompute_stages
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision) SELECT owner,1 FROM (SELECT (SELECT thread_id FROM context_precompute_accounting WHERE generation=NEW.generation) AS owner) WHERE owner IS NOT NULL ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision) SELECT owner,1 FROM (SELECT (SELECT thread_id FROM context_precompute_accounting WHERE generation=victim.generation) AS owner FROM context_precompute_stages victim WHERE victim.rowid IS NEW.rowid OR (victim.generation IS NEW.generation AND victim.stage IS NEW.stage)) WHERE owner IS NOT NULL ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER source_validation_context_precompute_stages_update BEFORE UPDATE ON context_precompute_stages
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision) SELECT owner,1 FROM (SELECT (SELECT thread_id FROM context_precompute_accounting WHERE generation=OLD.generation) AS owner) WHERE owner IS NOT NULL ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision) SELECT owner,1 FROM (SELECT (SELECT thread_id FROM context_precompute_accounting WHERE generation=NEW.generation) AS owner) WHERE owner IS NOT NULL ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision) SELECT owner,1 FROM (SELECT (SELECT thread_id FROM context_precompute_accounting WHERE generation=victim.generation) AS owner FROM context_precompute_stages victim WHERE victim.rowid IS NEW.rowid OR (victim.generation IS NEW.generation AND victim.stage IS NEW.stage)) WHERE owner IS NOT NULL ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER source_validation_context_precompute_stages_delete BEFORE DELETE ON context_precompute_stages
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision) SELECT owner,1 FROM (SELECT (SELECT thread_id FROM context_precompute_accounting WHERE generation=OLD.generation) AS owner) WHERE owner IS NOT NULL ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER source_validation_token_usage_insert BEFORE INSERT ON token_usage
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision) SELECT owner,1 FROM (SELECT NEW.thread_id AS owner) WHERE owner IS NOT NULL ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision) SELECT owner,1 FROM (SELECT victim.thread_id AS owner FROM token_usage victim WHERE victim.id IS NEW.id OR victim.rowid IS NEW.rowid) WHERE owner IS NOT NULL ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER source_validation_token_usage_update BEFORE UPDATE ON token_usage
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision) SELECT owner,1 FROM (SELECT OLD.thread_id AS owner) WHERE owner IS NOT NULL ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision) SELECT owner,1 FROM (SELECT NEW.thread_id AS owner) WHERE owner IS NOT NULL ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision) SELECT owner,1 FROM (SELECT victim.thread_id AS owner FROM token_usage victim WHERE victim.id IS NEW.id OR victim.rowid IS NEW.rowid) WHERE owner IS NOT NULL ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER source_validation_token_usage_delete BEFORE DELETE ON token_usage
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision) SELECT owner,1 FROM (SELECT OLD.thread_id AS owner) WHERE owner IS NOT NULL ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;
