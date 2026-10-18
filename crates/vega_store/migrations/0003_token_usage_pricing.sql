--
-- Columns are appended only (no drop, no rebuild, no new table). Legacy rows
-- and read back as NULL in every new column.
ALTER TABLE token_usage ADD COLUMN pricing_version TEXT;   -- NULL = legacy/unpriced; 'pricing_v1' = priced (0 allowed as priced-zero)
ALTER TABLE token_usage ADD COLUMN pricing_profile TEXT;   -- 'base' | 'peak_utc_weekly' for priced rows; NULL for legacy
ALTER TABLE token_usage ADD COLUMN call_started_at INTEGER; -- Unix UTC seconds of the logical provider call start used for the quote; NULL for legacy
