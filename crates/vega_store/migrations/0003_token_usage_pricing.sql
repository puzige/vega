-- S7-T38 (C5 · A10-04): price-aware usage audit columns for token_usage.
--
-- Columns are appended only (no drop, no rebuild, no new table). Legacy rows
-- written before this migration keep their S4/S5 `cost_microcents=0` placeholder
-- and read back as NULL in every new column.
ALTER TABLE token_usage ADD COLUMN pricing_version TEXT;   -- NULL = legacy/unpriced; 'pricing_v1' = priced (0 allowed as priced-zero)
ALTER TABLE token_usage ADD COLUMN pricing_profile TEXT;   -- 'base' | 'peak_utc_weekly' for priced rows; NULL for legacy
ALTER TABLE token_usage ADD COLUMN call_started_at INTEGER; -- Unix UTC seconds of the logical provider call start used for the quote; NULL for legacy
