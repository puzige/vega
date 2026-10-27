CREATE TABLE permission_reset_journal (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    reset_id TEXT NOT NULL,
    authority_epoch TEXT NOT NULL,
    configuration_id TEXT NOT NULL,
    source_mode TEXT NOT NULL,
    source_revision INTEGER NOT NULL CHECK (source_revision >= 0),
    source_grant TEXT,
    target_revision INTEGER NOT NULL CHECK (target_revision > source_revision),
    completed INTEGER NOT NULL DEFAULT 0 CHECK (completed IN (0, 1)),
    created_at INTEGER NOT NULL
);

ALTER TABLE pending_messages ADD COLUMN permission_invalidated INTEGER NOT NULL DEFAULT 0 CHECK (permission_invalidated IN (0, 1));
ALTER TABLE credential_continuations ADD COLUMN permission_invalidated INTEGER NOT NULL DEFAULT 0 CHECK (permission_invalidated IN (0, 1));

CREATE TABLE permission_replay_cutoffs (
    thread_id TEXT PRIMARY KEY REFERENCES threads(id) ON DELETE CASCADE,
    through_seq INTEGER NOT NULL,
    reset_id TEXT NOT NULL
);
