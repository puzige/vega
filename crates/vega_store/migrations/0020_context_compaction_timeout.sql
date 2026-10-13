ALTER TABLE context_compaction_status ADD COLUMN failure_detail TEXT CHECK (
  failure_detail IS NULL OR (
    failure_detail = 'summary_timeout'
    AND failure IS NOT NULL
    AND failure = 'invalid_summary'
  )
);
