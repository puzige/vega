CREATE TABLE fork_mutation_origins (
    call_id TEXT PRIMARY KEY REFERENCES tool_calls(id) ON DELETE CASCADE,
    project_id TEXT NOT NULL CHECK(length(project_id) > 0),
    source_thread_id TEXT NOT NULL CHECK(length(source_thread_id) > 0),
    source_call_id TEXT NOT NULL CHECK(length(source_call_id) > 0)
);
