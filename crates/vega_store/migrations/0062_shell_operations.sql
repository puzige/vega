CREATE TABLE shell_operations (
    operation_id TEXT PRIMARY KEY NOT NULL,
    thread_id TEXT NOT NULL REFERENCES threads(id),
    run_id TEXT NOT NULL REFERENCES messages(id),
    call_id TEXT NOT NULL REFERENCES tool_calls(id),
    generation TEXT NOT NULL,
    workspace TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('prepared','running','finishing','stopping','succeeded','failed','timed_out','cancelled','outcome_unknown')),
    exit_code INTEGER,
    error TEXT,
    duration_ms INTEGER,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX shell_operations_workspace_state ON shell_operations(workspace,state);
CREATE INDEX shell_operations_run ON shell_operations(run_id);
