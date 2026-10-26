ALTER TABLE threads ADD COLUMN permission_revision INTEGER NOT NULL DEFAULT 0 CHECK (permission_revision >= 0);
ALTER TABLE threads ADD COLUMN full_access_grant TEXT CHECK (full_access_grant IS NULL OR json_valid(full_access_grant));
ALTER TABLE credential_continuations ADD COLUMN permission_fence TEXT CHECK (permission_fence IS NULL OR json_valid(permission_fence));

CREATE TABLE permission_contract_state (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    authority_epoch TEXT NOT NULL CHECK (length(authority_epoch) > 0),
    reset_state TEXT NOT NULL CHECK (reset_state IN ('ready', 'pending'))
);

CREATE TABLE run_permission_fences (
    message_id TEXT PRIMARY KEY REFERENCES messages(id) ON DELETE CASCADE,
    thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
    permission_fence TEXT NOT NULL CHECK (json_valid(permission_fence))
);

CREATE TABLE delegated_permission_fences (
    child_message_id TEXT PRIMARY KEY REFERENCES delegated_executions(child_message_id) ON DELETE CASCADE,
    child_thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
    permission_fence TEXT NOT NULL CHECK (json_valid(permission_fence))
);

CREATE TABLE command_scope_audits (
    request_id TEXT PRIMARY KEY,
    thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
    run_id TEXT NOT NULL,
    call_id TEXT NOT NULL,
    request TEXT NOT NULL CHECK (json_valid(request)),
    state TEXT NOT NULL CHECK (state IN ('pending', 'approved', 'denied', 'timeout', 'cancelled', 'consumed', 'failed', 'unknown')),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(run_id, call_id)
);
CREATE INDEX command_scope_audits_thread ON command_scope_audits(thread_id, created_at);
