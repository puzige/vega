# Continuous compaction within a user turn

## Authorization and scope

2026-10-14: user explicitly requests repeated compaction during a single turn, correct model/tool round grouping, and protection against discarding task goals and progress. Tracking: #362. This supersedes conflicting newest-user-whole-turn retention and oldest-group-deletion behavior in the Claude compaction parity specification. Existing budget, authority, cancellation, timeout, immutable audit and transactional checkpoint guarantees remain.

## Requirements

1. At completed model/tool boundaries, compact already completed history within the active user turn, and permit subsequent compactions in the same run. Keep pending tool calls/results paired and preserve current user intent and any uncompleted suffix.
2. Persist actual model response/round identity independently of visible text offsets. Multiple calls in one response remain one round; different responses with empty visible text remain distinct. Use an additive migration if needed. Legacy history must remain readable; never invent unsafe tool pairing or silently drop ambiguous historical data.
3. Checkpoint coverage must represent partial progress through a still-active assistant row. Live continuation and restart must use the same coverage semantics, avoiding both repeated old tool results and lost new results. CAS checks must cover relevant source mutations.
4. Never make an oversized summary fit by deleting unsummarized task history. Bounded staged summarization may carry previous checkpoint, user goals/corrections, decisions, work completed and pending work forward. If complete safe coverage cannot fit bounded limits, fail without installing a checkpoint. An empty failed assistant row is not meaningful task coverage.
5. Summary instructions and retained provenance preserve goals and progress, without elevating historical text into authority. Do not claim semantic perfection of model summaries; deterministic coverage guards must prevent implementation-induced omissions.
6. Preserve existing configured trigger/target/hard budgets, provider/model freeze, tool-disabled summarizer, timeouts, cancellation, credential redaction, image handling, immutable raw audit and usage accounting. No user runtime DB/config changes.

## Acceptance matrix

| ID | Scenario | Expected |
|---|---|---|
| CC1 | Long first user turn, completed tool rounds | Compacts and resumes without extra user message |
| CC2 | Same run grows again after checkpoint | Second compaction succeeds; no duplicate or missing covered rounds |
| CC3 | Consecutive model responses with empty text and parallel tools | Distinct rounds; calls within each response paired |
| CC4 | Oversized input containing early goal and recent progress | Every covered group reaches summarization; no oldest-group deletion; goal/progress represented in mock continuation |
| CC5 | Failed empty reply followed by continue | Cannot replace task history with empty-context checkpoint |
| CC6 | Reopen checkpoint inside assistant row, including legacy source | Same remaining projection as live continuation; no tool reexecution |
| CC7 | Source mutation/cancellation/invalid summary/timeout | Previous checkpoint and raw audit intact; no false success |
| CC8 | Budget, images, credentials and usage | Existing safety behavior retained |

## Implementation plan and verification

### Durable source comparison (#409, 2026-10-17)

The active owned assistant/tool group is fenced by its ordered durable fields, not by provider-only replay state. Compare role, content, images, tool_call_id and complete ordered tool_calls; exclude run-memory reasoning_content, response_reasoning, live argument sidecars and model-projection markers from this local source comparison. Do not change global ChatMessage equality, mutate provider replay data or persist reasoning. All unrelated-history, source fingerprint, owner/model, predecessor and checkpoint CAS guards remain mandatory. The executable acceptance matrix and limited implementation scope are frozen in [the #409 specification](vega-issue-409-compaction-durable-source.md).

Dedicated implementation subagent owns schema/storage coverage and round provenance, runtime events and loop integration, conversation projection and bounded summarization, and focused regression tests. Main agent reviews integration and drives PR/CI. Add regression cases before implementation where practical. No local compilation/build/test; only quick formatting and diff/static checks. Cloud PR checks run regression and workspace tests. User validates real app after merge; keep issue open/In review until then.

## Validation

Pending implementation and cloud CI.
