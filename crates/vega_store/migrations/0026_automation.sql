CREATE TABLE automation_tasks (
    thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('goal','loop')),
    identity TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK (revision > 0),
    data TEXT NOT NULL CHECK (json_valid(data)),
    PRIMARY KEY (thread_id, kind)
);
CREATE TABLE automation_runs (
    id TEXT PRIMARY KEY,
    thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
    finished INTEGER NOT NULL DEFAULT 0 CHECK (finished IN (0,1)),
    data TEXT NOT NULL CHECK (json_valid(data))
);
CREATE UNIQUE INDEX automation_run_owner ON automation_runs(thread_id) WHERE finished = 0;
