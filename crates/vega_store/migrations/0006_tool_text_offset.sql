-- R70: display-only position of a call within its owning assistant text.
-- Existing audit rows deliberately remain NULL: their chronology is unknown.
ALTER TABLE tool_calls ADD COLUMN text_offset_bytes INTEGER
  CHECK (text_offset_bytes IS NULL OR text_offset_bytes >= 0);
