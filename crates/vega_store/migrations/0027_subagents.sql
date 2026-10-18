ALTER TABLE threads ADD COLUMN parent_thread_id TEXT REFERENCES threads(id) ON DELETE SET NULL;
ALTER TABLE threads ADD COLUMN workspace_owner_thread_id TEXT;
CREATE INDEX threads_parent ON threads(parent_thread_id);

CREATE TABLE delegated_runs (
  child_thread_id TEXT PRIMARY KEY REFERENCES threads(id) ON DELETE CASCADE,
  parent_thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
  parent_message_id TEXT NOT NULL,
  spawn_call_id TEXT NOT NULL,
  child_message_id TEXT NOT NULL UNIQUE,
  name TEXT NOT NULL CHECK(length(CAST(name AS BLOB)) BETWEEN 1 AND 256),
  task TEXT NOT NULL CHECK(length(CAST(task AS BLOB)) BETWEEN 1 AND 32768),
  state TEXT NOT NULL CHECK(state IN ('running','cancelling','completed','failed','interrupted')),
  result TEXT NOT NULL DEFAULT '',
  truncated INTEGER NOT NULL DEFAULT 0 CHECK(truncated IN (0,1)),
  usage_complete INTEGER NOT NULL DEFAULT 0 CHECK(usage_complete IN (0,1)),
  created_at INTEGER NOT NULL,
  UNIQUE(parent_thread_id,parent_message_id,spawn_call_id),
  UNIQUE(parent_thread_id,parent_message_id,name),
  CHECK(child_thread_id <> parent_thread_id)
);
CREATE INDEX delegated_runs_parent ON delegated_runs(parent_thread_id,parent_message_id,created_at);

ALTER TABLE token_usage ADD COLUMN delegated_parent_thread_id TEXT;
ALTER TABLE token_usage ADD COLUMN delegated_parent_message_id TEXT;
ALTER TABLE token_usage ADD COLUMN delegated_cache_known INTEGER CHECK(delegated_cache_known IN (0,1));
CREATE INDEX token_usage_delegated_parent ON token_usage(delegated_parent_thread_id,delegated_parent_message_id);
