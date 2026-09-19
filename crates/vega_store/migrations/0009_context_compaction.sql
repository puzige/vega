-- Issue #76: settings are scoped by the exact (thread, model) identity.
-- Checkpoints are append-only projections; the original messages and tool
-- audit remain the source of truth and are never rewritten by this migration.
CREATE TABLE context_settings (
  thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
  model TEXT NOT NULL,
  context_limit INTEGER,
  output_reserve INTEGER NOT NULL,
  automatic_compaction INTEGER NOT NULL DEFAULT 1
    CHECK (automatic_compaction IN (0, 1)),
  updated_at INTEGER NOT NULL,
  PRIMARY KEY (thread_id, model),
  CHECK (context_limit IS NULL OR context_limit > 0),
  CHECK (output_reserve > 0),
  CHECK (context_limit IS NULL OR output_reserve < context_limit)
);

CREATE TABLE context_checkpoints (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
  model TEXT NOT NULL,
  source_version INTEGER NOT NULL CHECK (source_version >= 0),
  covered_through_seq INTEGER NOT NULL CHECK (covered_through_seq >= 0),
  source_fingerprint TEXT NOT NULL,
  summary TEXT NOT NULL CHECK (length(trim(summary)) > 0),
  estimator_version TEXT NOT NULL,
  expected_previous_id INTEGER,
  created_at INTEGER NOT NULL
);

CREATE UNIQUE INDEX context_checkpoint_identity
  ON context_checkpoints(thread_id, model, source_version, source_fingerprint);
CREATE INDEX context_checkpoint_latest
  ON context_checkpoints(thread_id, model, id DESC);
