CREATE TABLE composer_prediction_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
    completed_message_id TEXT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    event TEXT NOT NULL,
    attempt INTEGER NOT NULL CHECK (attempt BETWEEN 0 AND 3),
    occurred_at INTEGER NOT NULL
);
CREATE INDEX composer_prediction_events_message ON composer_prediction_events(thread_id, completed_message_id, id);
