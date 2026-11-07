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
    command_result TEXT,
    exit_signal INTEGER,
    cleanup_state TEXT NOT NULL DEFAULT 'pending' CHECK (cleanup_state IN ('pending','best_effort_completed','unresolved')),
    output_incomplete INTEGER NOT NULL DEFAULT 0 CHECK (output_incomplete IN (0,1)),
    resources_closed INTEGER NOT NULL DEFAULT 0 CHECK (resources_closed IN (0,1)),
    process_resolved INTEGER NOT NULL DEFAULT 0 CHECK (process_resolved IN (0,1)),
    cleanup_reasons TEXT NOT NULL DEFAULT '[]',
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX shell_operations_workspace_state ON shell_operations(workspace,state);
CREATE INDEX shell_operations_run ON shell_operations(run_id);
CREATE TABLE shell_resources (
    operation_id TEXT PRIMARY KEY NOT NULL REFERENCES shell_operations(operation_id),
    identity TEXT NOT NULL
);
CREATE TABLE shell_workspace_epochs (
    workspace TEXT PRIMARY KEY NOT NULL,
    epoch INTEGER NOT NULL DEFAULT 0 CHECK (epoch >= 0)
);
CREATE TABLE shell_blockers (
    operation_id TEXT PRIMARY KEY NOT NULL REFERENCES shell_operations(operation_id),
    workspace TEXT NOT NULL,
    epoch INTEGER NOT NULL,
    reason TEXT NOT NULL,
    created_at INTEGER NOT NULL
);
CREATE INDEX shell_blockers_workspace ON shell_blockers(workspace);
CREATE TABLE shell_cleanup_observations (
    observation_id TEXT PRIMARY KEY NOT NULL,
    operation_id TEXT NOT NULL REFERENCES shell_operations(operation_id),
    workspace TEXT NOT NULL,
    epoch INTEGER NOT NULL,
    facts TEXT NOT NULL,
    created_at INTEGER NOT NULL
);
CREATE TABLE shell_dispositions (
    disposition_id TEXT PRIMARY KEY NOT NULL,
    workspace TEXT NOT NULL,
    operation_ids TEXT NOT NULL,
    expected_epoch INTEGER NOT NULL,
    committed_epoch INTEGER NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('automatic','retry_cleanup','user_risk')),
    content TEXT NOT NULL,
    created_at INTEGER NOT NULL
);
