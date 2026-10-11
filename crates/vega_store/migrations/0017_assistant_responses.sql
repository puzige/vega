CREATE TABLE assistant_responses (
    message_id TEXT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    response_id TEXT NOT NULL,
    text_start_bytes INTEGER NOT NULL CHECK (text_start_bytes >= 0),
    text_end_bytes INTEGER NOT NULL CHECK (text_end_bytes > text_start_bytes),
    kind TEXT NOT NULL CHECK (kind IN ('commentary', 'final')),
    PRIMARY KEY (message_id, response_id)
);
