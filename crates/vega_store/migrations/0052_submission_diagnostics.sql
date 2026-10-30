CREATE TABLE submission_diagnostic_events (
  submission_id TEXT NOT NULL CHECK (length(submission_id) = 26),
  attempt INTEGER NOT NULL CHECK (attempt > 0),
  terminal INTEGER NOT NULL CHECK (terminal IN (0, 1)),
  payload TEXT NOT NULL,
  PRIMARY KEY (submission_id, attempt, terminal)
);
