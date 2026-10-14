# Model switching preserves conversation checkpoints

2026-10-14 user authorized #369: switch GPT to DeepSeek while retaining task goal/progress and continuing same task. Supersedes exact-model-only checkpoint lookup; budgets, model/provider settings and frozen authority remain model-specific.

## Requirements

- Select latest valid checkpoint for the thread regardless of producing model. Producer model remains provenance, not access partition. Switching back must not select a stale checkpoint.
- Apply identical coverage/projection semantics to initial send, UI context estimate, manual/automatic compaction, staged summary, restart and source/CAS validation. Preserve partial assistant cursors and images. A target-model checkpoint install must compare against the actual prior thread checkpoint, even if produced by another model.
- Recompute estimates with current model/tool/system configuration. Never carry old usage anchor, reasoning replay or model/provider permission/config across switch. Honor target hard input budget, target/reserve, automatic setting and summary timeouts. Recompact existing checkpoint plus uncovered suffix safely when necessary; never replay all covered raw history merely because model changed.
- Keep task goal, corrections, progress and pending work in retained historical summary plus uncovered messages. No unsummarized deletion, tool reexecution or elevation of summary to system authority. Preserve immutable raw audit; no direct user DB/config edits.
- Handle legacy checkpoints and exact-model historical API callers consistently. Fail safely on invalid source or target constraints. No new dependency or migration unless required and explained.

## Tests and plan

Implementation delegated to dedicated subagent, main owns review and CI. Add production tests for A checkpoint -> switch B -> primary request includes goal/progress and only uncovered tail (zero replay of covered large outputs); B new checkpoint -> switch A uses newest B summary; reopen; partial-active-row checkpoint; smaller target budget triggers safe compaction; source mutation/CAS; wrong-thread isolation. Include regression with a huge raw history safely covered by checkpoint and target budget far below raw size.

Local formatting/static/diff only, no compilation/tests. Cloud PR gate compiles and tests. Keep issue In review until user real-model acceptance. This cannot reconstruct semantic information absent from an already bad legacy summary; preserve accessible newer explicit recovery messages and valid checkpoints, never fabricate lost progress.

Validation pending.
