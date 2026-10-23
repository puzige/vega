CREATE TABLE delegated_stage_checkpoints (
 id TEXT PRIMARY KEY,
 child_thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
 child_message_id TEXT NOT NULL,
 payload TEXT NOT NULL,
 authority TEXT NOT NULL,
 created_at INTEGER NOT NULL
);
CREATE INDEX delegated_stage_checkpoint_execution ON delegated_stage_checkpoints(child_thread_id,child_message_id,created_at);
CREATE TABLE delegated_execution_recovery (
 child_thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
 child_message_id TEXT NOT NULL,
 release_state TEXT NOT NULL CHECK(release_state IN ('held','released','recovered_abandoned','quarantined')),
 authority TEXT NOT NULL,
 handoff_checkpoint_id TEXT,
 resume_checkpoint_id TEXT,
 reconciliations TEXT NOT NULL DEFAULT '[]',
 PRIMARY KEY(child_thread_id,child_message_id)
);
CREATE TABLE delegated_read_receipts (
 call_id TEXT PRIMARY KEY REFERENCES tool_calls(id) ON DELETE CASCADE,
 child_thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
 child_message_id TEXT NOT NULL,
 canonical_path TEXT NOT NULL,
 sha256 TEXT NOT NULL,
 authority TEXT NOT NULL
);
CREATE TABLE delegated_execution_messages (
 message_id TEXT PRIMARY KEY,
 child_thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
 execution_message_id TEXT NOT NULL,
 FOREIGN KEY(child_thread_id,execution_message_id) REFERENCES delegated_execution_recovery(child_thread_id,child_message_id) ON DELETE CASCADE
);
