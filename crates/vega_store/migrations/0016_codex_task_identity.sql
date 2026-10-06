ALTER TABLE threads
  ADD COLUMN backend TEXT NOT NULL DEFAULT 'native'
  CHECK (backend IN ('native', 'codex'));

CREATE TABLE codex_task_snapshots (
  thread_id TEXT PRIMARY KEY REFERENCES threads(id) ON DELETE CASCADE,
  profile_id TEXT NOT NULL CHECK (length(profile_id) BETWEEN 1 AND 128 AND profile_id NOT GLOB '*[^A-Za-z0-9_-]*'),
  profile_display_name TEXT NOT NULL CHECK (length(profile_display_name) BETWEEN 1 AND 128),
  adapter_kind TEXT NOT NULL CHECK (adapter_kind = 'codex_acp'),
  executable TEXT NOT NULL CHECK (length(executable) BETWEEN 1 AND 4096),
  arguments_json TEXT NOT NULL CHECK (json_valid(arguments_json) AND json_type(arguments_json) = 'array'),
  adapter_version TEXT NOT NULL CHECK (length(adapter_version) BETWEEN 1 AND 64 AND adapter_version NOT GLOB '*[^0-9A-Za-z.+_-]*'),
  codex_version TEXT NOT NULL CHECK (length(codex_version) BETWEEN 1 AND 64 AND codex_version NOT GLOB '*[^0-9A-Za-z.+_-]*'),
  model TEXT CHECK (model IS NULL OR (length(model) BETWEEN 1 AND 128 AND model NOT GLOB '*[^A-Za-z0-9._-]*')),
  model_provider TEXT CHECK (model_provider IS NULL OR (length(model_provider) BETWEEN 1 AND 128 AND model_provider NOT GLOB '*[^A-Za-z0-9._-]*')),
  reasoning_effort TEXT CHECK (reasoning_effort IS NULL OR reasoning_effort IN ('minimal', 'low', 'medium', 'high', 'xhigh')),
  sandbox_mode TEXT NOT NULL CHECK (sandbox_mode IN ('read_only', 'workspace_write')),
  approval_policy TEXT NOT NULL CHECK (approval_policy IN ('on_request', 'never')),
  selected_project_id TEXT REFERENCES projects(id) ON DELETE RESTRICT,
  worktree_id TEXT CHECK (worktree_id IS NULL OR (length(worktree_id) BETWEEN 1 AND 128 AND worktree_id NOT GLOB '*[^A-Za-z0-9_-]*')),
  canonical_working_directory TEXT NOT NULL CHECK (length(canonical_working_directory) BETWEEN 1 AND 4096),
  additional_directories_json TEXT NOT NULL CHECK (json_valid(additional_directories_json) AND json_type(additional_directories_json) = 'array'),
  created_at INTEGER NOT NULL CHECK (created_at >= 0)
);

CREATE INDEX idx_codex_task_profile ON codex_task_snapshots(profile_id, thread_id);

CREATE TRIGGER codex_task_snapshot_requires_codex_backend
BEFORE INSERT ON codex_task_snapshots
WHEN NOT EXISTS (
  SELECT 1 FROM threads WHERE id = NEW.thread_id AND backend = 'codex'
)
BEGIN
  SELECT RAISE(ABORT, 'codex snapshot requires codex backend');
END;

CREATE TRIGGER codex_task_snapshot_immutable
BEFORE UPDATE ON codex_task_snapshots
BEGIN
  SELECT RAISE(ABORT, 'codex task snapshot is immutable');
END;

CREATE TRIGGER thread_codex_backend_immutable
BEFORE UPDATE OF backend ON threads
WHEN OLD.backend <> NEW.backend AND EXISTS (
  SELECT 1 FROM codex_task_snapshots WHERE thread_id = OLD.id
)
BEGIN
  SELECT RAISE(ABORT, 'codex task backend is immutable');
END;

CREATE TABLE codex_session_creations (
  thread_id TEXT PRIMARY KEY REFERENCES codex_task_snapshots(thread_id) ON DELETE CASCADE,
  state TEXT NOT NULL CHECK (state IN ('absent', 'intent', 'confirmed', 'uncertain', 'definitively_failed')),
  intent_id TEXT UNIQUE CHECK (intent_id IS NULL OR (length(intent_id) BETWEEN 1 AND 128 AND intent_id NOT GLOB '*[^A-Za-z0-9_-]*')),
  session_id TEXT UNIQUE CHECK (session_id IS NULL OR (length(session_id) BETWEEN 1 AND 512 AND instr(session_id, char(0)) = 0 AND instr(session_id, char(10)) = 0 AND instr(session_id, char(13)) = 0)),
  uncertainty_code TEXT CHECK (uncertainty_code IS NULL OR uncertainty_code IN ('transport_closed', 'protocol_failure', 'outcome_unknown')),
  failure_code TEXT CHECK (failure_code IS NULL OR failure_code IN ('process_start_failed', 'authentication_required', 'adapter_rejected', 'invalid_configuration')),
  updated_at INTEGER NOT NULL CHECK (updated_at >= 0),
  CHECK (
    (state = 'absent' AND intent_id IS NULL AND session_id IS NULL AND uncertainty_code IS NULL AND failure_code IS NULL)
    OR (state = 'intent' AND intent_id IS NOT NULL AND session_id IS NULL AND uncertainty_code IS NULL AND failure_code IS NULL)
    OR (state = 'confirmed' AND intent_id IS NOT NULL AND session_id IS NOT NULL AND uncertainty_code IS NULL AND failure_code IS NULL)
    OR (state = 'uncertain' AND intent_id IS NOT NULL AND session_id IS NULL AND uncertainty_code IS NOT NULL AND failure_code IS NULL)
    OR (state = 'definitively_failed' AND intent_id IS NOT NULL AND session_id IS NULL AND uncertainty_code IS NULL AND failure_code IS NOT NULL)
  )
);

CREATE TRIGGER codex_session_creation_initial_state
AFTER INSERT ON codex_task_snapshots
BEGIN
  INSERT INTO codex_session_creations (thread_id, state, updated_at)
  VALUES (NEW.thread_id, 'absent', NEW.created_at);
END;

CREATE TRIGGER codex_session_creation_transition
BEFORE UPDATE ON codex_session_creations
WHEN NOT (
  OLD.state = 'absent' AND NEW.state = 'intent'
  AND NEW.intent_id IS NOT NULL AND NEW.session_id IS NULL
  AND NEW.uncertainty_code IS NULL AND NEW.failure_code IS NULL
  AND NEW.updated_at >= OLD.updated_at
  OR OLD.state = 'intent' AND NEW.intent_id = OLD.intent_id
  AND NEW.state IN ('confirmed', 'uncertain', 'definitively_failed')
  AND NEW.updated_at >= OLD.updated_at
)
BEGIN
  SELECT RAISE(ABORT, 'invalid codex session creation transition');
END;

PRAGMA user_version = 16;
