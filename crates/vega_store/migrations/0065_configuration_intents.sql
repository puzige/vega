CREATE TABLE default_permission_intent (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    generation TEXT NOT NULL,
    pending_id TEXT,
    source_json TEXT,
    target_json TEXT
) STRICT;
INSERT INTO default_permission_intent(singleton,generation) VALUES(1,lower(hex(randomblob(16))));
CREATE TABLE draft_creation_intents (
    operation_id TEXT PRIMARY KEY NOT NULL,
    thread_id TEXT NOT NULL,
    authority_epoch TEXT NOT NULL,
    configuration_generation TEXT NOT NULL,
    source_json TEXT,
    state TEXT NOT NULL CHECK(state IN ('prepared','committed')),
    created_at INTEGER NOT NULL
) STRICT;
CREATE UNIQUE INDEX draft_creation_intents_pending_thread ON draft_creation_intents(thread_id) WHERE state='prepared';
