CREATE TABLE pending_messages (
    id TEXT PRIMARY KEY,
    thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL,
    behavior TEXT NOT NULL CHECK (behavior IN ('queue', 'steer')),
    generation TEXT NOT NULL,
    payload TEXT NOT NULL,
    model_content TEXT,
    state TEXT NOT NULL DEFAULT 'pending' CHECK (state IN ('pending', 'claimed', 'delivered')),
    created_at INTEGER NOT NULL,
    UNIQUE(thread_id, ordinal)
);
CREATE INDEX pending_messages_thread ON pending_messages(thread_id, ordinal);
