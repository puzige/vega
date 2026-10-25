ALTER TABLE threads ADD COLUMN forked_from_thread_id TEXT REFERENCES threads(id) ON DELETE SET NULL;
ALTER TABLE threads ADD COLUMN forked_from_message_id TEXT;
CREATE INDEX threads_fork_source ON threads(forked_from_thread_id);

CREATE TABLE historical_message_delivery (
    message_id TEXT PRIMARY KEY REFERENCES messages(id) ON DELETE CASCADE,
    thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
    model_content TEXT,
    previous_assistant_id TEXT REFERENCES messages(id) ON DELETE CASCADE
);
CREATE INDEX historical_delivery_thread ON historical_message_delivery(thread_id, previous_assistant_id);

CREATE TABLE historical_tool_receipts (
    call_id TEXT PRIMARY KEY REFERENCES tool_calls(id) ON DELETE CASCADE,
    thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK(kind IN ('delegation', 'screenshot')),
    source_thread_id TEXT NOT NULL,
    source_message_id TEXT NOT NULL,
    source_call_id TEXT NOT NULL,
    input_json TEXT NOT NULL,
    output_text TEXT,
    references_json TEXT NOT NULL
);
CREATE INDEX historical_receipts_thread ON historical_tool_receipts(thread_id);
