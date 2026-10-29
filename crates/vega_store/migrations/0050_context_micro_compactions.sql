CREATE TABLE context_micro_compactions (
    call_id TEXT PRIMARY KEY REFERENCES tool_calls(id) ON DELETE CASCADE,
    thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
    source_digest TEXT NOT NULL,
    created_at INTEGER NOT NULL
);
CREATE INDEX context_micro_compactions_thread ON context_micro_compactions(thread_id);
