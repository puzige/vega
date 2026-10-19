CREATE TABLE delegated_executions (
  child_message_id TEXT PRIMARY KEY NOT NULL,
  child_thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
  parent_thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
  parent_message_id TEXT NOT NULL,
  request_call_id TEXT NOT NULL,
  ordinal INTEGER NOT NULL CHECK(ordinal >= 1),
  previous_child_message_id TEXT,
  kind TEXT NOT NULL CHECK(kind IN ('spawn','followup')),
  instruction_message_id TEXT,
  name TEXT NOT NULL CHECK(length(CAST(name AS BLOB)) BETWEEN 1 AND 256),
  task TEXT NOT NULL CHECK(length(CAST(task AS BLOB)) BETWEEN 1 AND 32768),
  state TEXT NOT NULL CHECK(state IN ('running','cancelling','completed','failed','interrupted')),
  result TEXT NOT NULL DEFAULT '',
  truncated INTEGER NOT NULL DEFAULT 0 CHECK(truncated IN (0,1)),
  usage_complete INTEGER NOT NULL DEFAULT 0 CHECK(usage_complete IN (0,1)),
  created_at INTEGER NOT NULL,
  UNIQUE(child_thread_id,ordinal),
  UNIQUE(parent_thread_id,parent_message_id,request_call_id),
  CHECK(child_thread_id <> parent_thread_id),
  CHECK((ordinal=1 AND kind='spawn' AND previous_child_message_id IS NULL) OR
        (ordinal>1 AND kind='followup' AND previous_child_message_id IS NOT NULL AND instruction_message_id IS NOT NULL))
);
CREATE UNIQUE INDEX delegated_executions_active ON delegated_executions(child_thread_id) WHERE state IN ('running','cancelling');
CREATE INDEX delegated_executions_parent ON delegated_executions(parent_thread_id,parent_message_id,created_at);
CREATE INDEX delegated_executions_child ON delegated_executions(child_thread_id,ordinal);
INSERT INTO delegated_executions(child_thread_id,child_message_id,parent_thread_id,parent_message_id,request_call_id,ordinal,previous_child_message_id,kind,instruction_message_id,name,task,state,result,truncated,usage_complete,created_at)
SELECT child_thread_id,child_message_id,parent_thread_id,parent_message_id,spawn_call_id,1,NULL,'spawn',
       (SELECT id FROM messages WHERE thread_id=delegated_runs.child_thread_id AND role='user' ORDER BY seq LIMIT 1),
       name,task,state,result,truncated,usage_complete,created_at FROM delegated_runs;
