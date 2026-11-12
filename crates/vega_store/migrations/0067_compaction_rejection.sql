ALTER TABLE run_diagnostic_events ADD COLUMN compaction_rejection TEXT
CHECK (compaction_rejection IS NULL OR compaction_rejection IN (
  'compaction_ownership', 'compaction_source_fence', 'compaction_history_projection',
  'compaction_history_owned', 'compaction_history_messages', 'compaction_history_group',
  'compaction_history_call', 'compaction_history_offset', 'compaction_history_result',
  'compaction_micro_prepare', 'compaction_micro_commit', 'compaction_writer_admission',
  'compaction_worker_join'
));
