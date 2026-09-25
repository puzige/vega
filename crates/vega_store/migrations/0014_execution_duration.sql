CREATE TABLE assistant_run_durations (
  message_id TEXT PRIMARY KEY REFERENCES messages(id) ON DELETE CASCADE,
  execution_duration_ms INTEGER NOT NULL CHECK (execution_duration_ms >= 0)
);
