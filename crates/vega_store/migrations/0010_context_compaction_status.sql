-- Issue #76: durable, content-free compaction lifecycle and accounting state.
-- Summary Usage may be absent, so this table deliberately records an
-- explicit unknown state instead of inserting a fabricated zero-cost usage
-- row.  Raw transcript/summary text never enters this projection.
CREATE TABLE context_compaction_status (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
  model TEXT NOT NULL,
  operation_key TEXT NOT NULL,
  generation INTEGER NOT NULL CHECK (generation >= 0),
  phase TEXT NOT NULL CHECK (phase IN ('started', 'succeeded', 'failed', 'cancelled')),
  usage_state TEXT NOT NULL CHECK (
    usage_state IN ('pending', 'known_priced', 'known_unpriced', 'unknown')
  ),
  failure TEXT CHECK (
    failure IS NULL OR failure IN (
      'cancelled', 'source_changed', 'no_compactable_prefix', 'too_large',
      'invalid_summary', 'images_unsupported', 'over_limit', 'unavailable'
    )
  ),
  source_version INTEGER NOT NULL CHECK (source_version >= 0),
  estimated_tokens INTEGER NOT NULL CHECK (estimated_tokens >= 0),
  input_budget INTEGER NOT NULL CHECK (input_budget >= 0),
  target_tokens INTEGER NOT NULL CHECK (target_tokens >= 0),
  created_at INTEGER NOT NULL
);

CREATE INDEX context_compaction_status_latest
  ON context_compaction_status(thread_id, model, id DESC);
