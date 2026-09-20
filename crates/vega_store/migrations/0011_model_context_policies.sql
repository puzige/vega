-- Issue #76 correction: capacity and automatic compaction belong to the
-- configured provider/model, not to an individual conversation. Older
-- context_settings rows and all append-only checkpoints remain untouched.
CREATE TABLE model_context_policies (
  provider TEXT NOT NULL,
  model TEXT NOT NULL,
  input_limit INTEGER,
  output_reserve INTEGER,
  automatic_compaction INTEGER NOT NULL DEFAULT 1
    CHECK (automatic_compaction IN (0, 1)),
  updated_at INTEGER NOT NULL,
  PRIMARY KEY (provider, model),
  CHECK (
    (input_limit IS NULL AND output_reserve IS NULL) OR
    (input_limit BETWEEN 1 AND 4294967295 AND
     output_reserve BETWEEN 1 AND 4294967295 AND
     input_limit + output_reserve <= 4294967295)
  )
);
