CREATE TABLE credential_continuations (
    id TEXT PRIMARY KEY REFERENCES messages(id) ON DELETE CASCADE,
    thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
    user_message_id TEXT NOT NULL REFERENCES messages(id),
    source_version INTEGER NOT NULL,
    source_fingerprint TEXT NOT NULL,
    raw_user_fingerprint TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('ready', 'claimed', 'completed', 'unknown')),
    operation_id TEXT UNIQUE,
    continuation_message_id TEXT REFERENCES messages(id),
    reason TEXT NOT NULL,
    created_at INTEGER NOT NULL
);
CREATE INDEX credential_continuations_thread ON credential_continuations(thread_id, state);
