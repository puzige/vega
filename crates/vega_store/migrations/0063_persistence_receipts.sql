CREATE TABLE persistence_command_receipts (
    operation_id TEXT PRIMARY KEY NOT NULL,
    thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
    run_id TEXT NOT NULL,
    kind TEXT NOT NULL CHECK(kind IN ('snapshot','event')),
    state TEXT NOT NULL CHECK(state IN ('pending','committed')),
    reply_json TEXT,
    next_tool_seq INTEGER,
    active_message_id TEXT,
    active_response_id TEXT,
    created_at INTEGER NOT NULL
) STRICT;
CREATE INDEX persistence_command_receipts_run ON persistence_command_receipts(thread_id,run_id);
