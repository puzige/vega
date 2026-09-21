# Claude Code local compaction parity

## Authorization and baseline

2026-09-22: owner explicitly approved implementing the Claude Code logic after review of the supplied 2.1.88 package source map. Baseline is current origin/master a756559. This specification supersedes the strategy in `vega-compaction-codex-parity.md` and conflicting #76/#88/#91 multi-stage/immutable-summary requirements. Preserve the previous prototype and evidence outside the worktree; do not mix the two strategies. Existing #76 is the tracking issue; #117 timeline presentation is out of scope.

Reference modules: `services/compact/compact.ts`, `autoCompact.ts`, `grouping.ts`, `prompt.ts`, `microCompact.ts`, `sessionMemoryCompact.ts` from the owner-supplied distribution. Port observable behavior using Vega-native code; no wholesale source copying. Main owns this specification and review; dedicated implementation agent owns code/tests; separate acceptance agent owns a new external owned-fixture harness.

## Selected upstream path and explicit adaptations

Port the ordinary streaming full-compaction path, its prompt-too-long recovery, and automatic failure control. Do not implement the cache-sharing fork, provider-specific cache edits, feature-gated session-memory extraction or hooks infrastructure absent from Vega. These are explicit scope differences, not silently claimed parity.

Vega retains its configured 80% trigger/60% result target and provider/model policy in this patch. Replacing these settings with upstream model-capacity buffers requires separately validated model capacities; do not rewrite user settings. Preserve SQLite transactional snapshot/predecessor checks, immutable raw audit and below-system historical authority. Existing current system/tools/skills/MCP context remains runtime-owned and must still be present after compaction. Vega has no equivalent read-file-cache restoration registry, so this patch does not manufacture/re-read arbitrary files or introduce an unguarded file path. Current live suffix and tool groups are retained exactly.

## Behavioral contract

### C1 Direct summary, bounded output

- Construct one direct summary request from the current checkpoint-aware chronological historical messages, preserving roles, complete assistant tool-call/result groups and previous checkpoint as source. Append the summary instruction as a user request. Current newest user/live suffix remains exact outside the replaced prefix; its intent may be supplied as clearly labelled continuation context if needed.
- Remove the fixed 32 KiB staging, 128 KiB per-stage ceiling, recursive splitting on Length, map/reduce loops and append-only prior-summary accumulation. The source remains bounded by existing store limits and configured request token budget. A previous checkpoint is summarized again as part of the history rather than appended unchanged.
- Use continuation-focused structured summary requirements reflecting the upstream prompt: intent/corrections, decisions, relevant exact paths, errors, pending work, current task and next step. No separate analysis draft is required in Vega; accept valid legacy summary wrappers. Output ceiling is min(20000, configured output reserve), with a bounded128KiB collected-text ceiling replacing the previous32KiB cap; request-only reasoning disable is used only when legal in the frozen provider profile. Unknown profiles are not sent unsupported options. Preserve genuine completion/Length/malformed/empty/tool-output validation; never accept truncated output as success. No arbitrary 2048 visible cap remains.
- Tool execution is disabled for compaction. Do not run historic tools or widen tool permission. Each logical summary request has the existing60-second acquisition+stream deadline and a child cancellation token. One operation retains the180-second safety deadline from the prototype; identify this as a Vega safeguard, not an upstream constant.

### C2 Prompt-too-long recovery

- Only classified context-input overflow permits trimming/retry. Prefer structured provider error code; when providers expose text only, use a narrow bounded recognizer anchored to known context-limit/too-long phrases and appropriate HTTP status. Generic400,429,5xx,auth,cancel,empty summaries and output Length must not trigger history deletion/retry. Classification must not print raw diagnostics or user content.
- Group by complete API round (assistant response plus its matching tool results; preceding user context belongs to its proper chronological position), not fixed bytes or only whole human turns. Preserve tool-pair validity, including multiple tool calls per round. Do not split tool inputs/results into unrelated fragments.
- If the configured local input estimate already exceeds the summary request budget, use the same group-trimming algorithm before sending an oversized request. Otherwise make the initial request directly. On typed provider input overflow, drop enough oldest groups to cover a trustworthy available gap; with no usable gap, drop max(1,floor(group_count*0.2)), retaining at least one summarizable group. Insert one explicit historical-truncation marker and exclude that synthetic marker from subsequent grouping so retries always progress.
- At most3 trimming retries after the initial plan/request, including local preflight trimming. Never retry an unchanged input. If no legal smaller history fits, fail explicitly and preserve raw history/checkpoint. Retain known usage from all requests exactly once; incomplete usage remains unknown.
- Commit exactly one valid final checkpoint only when the rebuilt full projection fits the existing target and source/predecessor guards still hold. No follow-on reduction request to force a fit.

### C3 Images and history reconstruction

- Summarizer input replaces images with textual attachment markers/provenance rather than rejecting all image-containing history or serializing base64 as text. Images in any supported nested tool content must receive equivalent treatment. Do not invent visual descriptions.
- Retain the already-tested Vega image adaptation: newest covered user image groups, complete bytes and identifying historical/untrusted text, within deterministic64000 estimated tokens; chronological replay, active newest user images exact and outside that allowance. Explicit omission marker for excluded old images; raw database images remain intact. This is a Vega extension beyond upstream text placeholders, required to address the owner's image problem.
- Use one reconstruction helper across candidate install, normal next turn, read_context_projection and restart, avoiding image/summary duplication. Validate even omitted source images; malformed source still fails.
- Runtime continues with the current system prompt, skills/MCP/tool definitions, permissions and newest live suffix. They are not reconstructed from untrusted summary text. Verify these existing paths rather than add unrelated restoration subsystems.

### C4 Automatic continuation and failure circuit

- Match upstream's distinction between failed compaction and unusable original context. On a recoverable summary/provider failure while the original request still fits the hard input budget, record failure/usage and let the ordinary primary call continue using untouched history. No success checkpoint/status may be fabricated. At or above the hard overflow boundary, fail safely rather than send an oversized original request.
- Keep a per-primary-run consecutive automatic-compaction failure count. After3 failures, skip further automatic compaction in that run; successful compaction resets it. Repeated unchanged-source suppression must skip a futile soft-threshold attempt rather than abort an otherwise sendable primary request. Fresh user run/manual retry starts fresh. Do not persist a permanent lockout.
- Cancellation, source/CAS change, store failure, invalid authority/projection and credential/permission guard failures remain terminal; they must not be disguised as recoverable compaction failures. Skill prospective activation must not bypass its budget/authorization gates.

## Acceptance matrix — regressions before implementation

| ID | Case | Required evidence |
|---|---|---|
| A1 | Long complete tool history, primary automatic compaction | One normal summary request, no segment/reducer; one checkpoint, primary continuation, required early constraint/later correction retained |
| A2 | Direct provider input overflow, repeated overflow, local preflight overflow | Oldest complete groups trimmed; one marker; strict progress; maximum3 retries; no orphan calls or raw audit changes |
| A3 | Generic400/auth/429, Length, timeout, malformed/empty output | No PTL trimming; no partial checkpoint; exact usage/cancellation semantics |
| A4 | Soft-threshold failure vs hard overflow, then >3 tool rounds | Original primary continues only when sendable; circuit stops retries; no repeated-source abort; next run retry remains available |
| A5 | Old checkpoint and new turn, successful second compact, reopen | Prior checkpoint consolidated; current constraints/tools/skills remain; same restored projection and unchanged raw audit |
| A6 | Approximately214KB old and current images, repeat compact/reopen | Retained image bytes once, budget omission explicit, no images_unsupported, corruption rejected |
| A7 | Source/model/CAS change, cancellation, permission guard | Existing terminal safety behavior, no stale install or unauthorized primary continuation |
| A8 | Owned real configured provider long fixture, image, primary reply and reopen | Actual real-provider completion/usage, one checkpoint, retained correction and image; preserve failures truthfully |
| A9 | Signed candidate in fixed Documents path, native continuation | Actual build identity, application result, persistent screenshot/evidence; no claim based on mocks alone |

## Execution and evidence

Implementation scope includes directly affected conversation compaction/pipeline/tests and runtime loop/error/provider classification/tests as necessary. No new dependency/schema, unrelated UI or user config/database mutation. Add only specific regression tests needed for changed contracts; update old strategy assertions only where expressly superseded. Preserve original failed evidence and explain test changes.

Run `python3 scripts/verify.py --plan` then `python3 scripts/verify.py`; all Cargo work through `scripts/cargo-lock.sh`, worktree-isolated target. Acceptance harness must remove temporary external module registration before final gate. Main reviews code, evidence and source identity, then packages/installs when app idle; do not leave installation inconsistent with tested tree. Report actual limits and unrun native cases. Delivery report: `docs/vega-claude-compaction-parity-delivery.md`.

### Implementation adaptation notes

Vega ChatMessage does not expose upstream assistant API response IDs. Existing durable assistant text offsets and paired projected assistant/tool batches supply the corresponding safe round boundaries. Use these, never split a tool group. The configured reserve is the only available model-output bound in the current interface; report this rather than claiming automatic discovery of model limits. Provider diagnostic bodies are already redacted/bounded by transport; classify only structured known error codes or narrow unambiguous context-input-overflow phrases with400/413, never arbitrary errors.
