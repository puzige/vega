CREATE TABLE assistant_response_starts (
    message_id TEXT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    response_id TEXT NOT NULL,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    text_start_bytes INTEGER NOT NULL CHECK (text_start_bytes >= 0),
    started_at_ms INTEGER NOT NULL,
    PRIMARY KEY (message_id, response_id),
    UNIQUE (message_id, ordinal)
);

CREATE TABLE message_timestamp_boundaries (
    message_id TEXT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    operation_key TEXT NOT NULL,
    text_start_bytes INTEGER NOT NULL CHECK (text_start_bytes >= 0),
    PRIMARY KEY (message_id, operation_key)
);

CREATE TABLE thread_timestamp_fork_prefixes (
    thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
    prefix_seq INTEGER NOT NULL CHECK (prefix_seq >= 1),
    PRIMARY KEY (thread_id, prefix_seq)
);
