CREATE TABLE tool_call_rounds (
    message_id TEXT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    call_id TEXT NOT NULL,
    response_id TEXT NOT NULL,
    PRIMARY KEY (message_id, call_id)
);
