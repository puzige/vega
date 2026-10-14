ALTER TABLE context_checkpoints ADD COLUMN covered_assistant_id TEXT;
ALTER TABLE context_checkpoints ADD COLUMN covered_assistant_text_bytes INTEGER NOT NULL DEFAULT 0 CHECK (covered_assistant_text_bytes >= 0);
ALTER TABLE context_checkpoints ADD COLUMN covered_assistant_tool_seq INTEGER NOT NULL DEFAULT 0 CHECK (covered_assistant_tool_seq >= 0);
