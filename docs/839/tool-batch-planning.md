# Same-response tool-call planning

Issue #839. Baseline: `cd742e9680641b6dfc14ff65a2718e56de3d1803`.

## Evidence and scope

The preceding read-only audit matched 53 implementation tool calls between
Vega's stored response groups and CPA's upstream/downstream logs. Each upstream
response contained one function call, even though `parallel_tool_calls` was
true. Earlier requests in the same conversation carried two to four calls with
the same system-prompt and tool-definition hashes as the first implementation
request. Thirty-three implementation calls were shell calls, some already
combining several operations. These historical facts do not establish that a
prompt change will improve latency, or that every single-call turn was wasteful.

Current source accepts multiple calls per response, executes the prepared batch
in order, and only then requests another model response. The change targets
planning, not parser loss or scheduler concurrency. Codex's explicit tool-use
instructions are a reference for clarity, not authority to expose tools Vega
does not provide. Reference snapshot: Codex
`d583e73c4d1204f1e9f654ef87065e9f0dd07ac7`,
`codex-rs/core/gpt_5_2_prompt.md`, Shell commands section.

Only the existing planning paragraph in
`crates/vega/src/agent_context.rs::SYSTEM_PROMPT` and this specification change.
No tool schema, provider flag, executor, permission policy, persistence, UI,
dependency or CI workflow changes. #840 tracks separate native-chain regression
work; #841, #412 and #842 remain follow-up evaluations, not this implementation.

## Acceptance scenarios

| Preconditions / operation | Required observable behavior | Verification |
|---|---|---|
| Several known, independent file/range reads; contracts allow batching | Request them in one model response, with focused output and distinct results; do not require concurrent execution | Prompt review; user comparison |
| Two known, non-overlapping changes to one file after the needed reads | Prefer one `edit` call with multiple `edits`, preserving exact matching and atomic file-edit semantics | Prompt review; user comparison |
| Search results determine which file to read | Wait for search output before choosing the read; do not invent its path | Prompt review; user counterexample |
| Read target changes before editing | Re-read the changed file before preparing the edit; batching does not override identity checks | Prompt review; user counterexample |
| A tool requires a standalone call, approval, or ordered observations | Keep that boundary, including user questions, skill loading and desktop operations | Prompt review; user counterexamples |
| A batch partly fails or has an unknown side-effect outcome | Preserve individual outcomes and existing recovery rules; never replay successful side effects or retry an unknown effect merely to replay a batch | Prompt review; unchanged runtime contracts |
| No other necessary independent operation is known | Use a single call; do not broaden investigation to fill a batch | Prompt review; user comparison |

## Minimal implementation

Replace the general suggestion to batch independent reads with explicit
same-model-response guidance and a known-file-read example. State that batching
does not promise parallel execution. Collect known prerequisite reads before
editing and use the existing multi-edit array for one file. Preserve dependent
steps, fresh-read requirements, per-tool boundaries and partial-failure recovery.
Keep focused reads, verified conclusions and omitted-output retrieval guidance.

This is a model-behavior hypothesis, not a guaranteed batching rate or time
saving. Revert the paragraph if it induces unsafe grouping or reduces quality.

## Verification and user comparison

Local validation is limited to format, diff and static inspection. Independent
review checks the changed instruction against the existing tool contracts.
Existing required Actions must pass for the exact final head before merge.
No new Rust tests are justified for a prompt-only edit: source-string assertions
would not demonstrate model behavior, and scripted model responses would not
measure the prompt effect. Do not claim the empty Rust suite as a test pass.

After merge, user-led comparison uses baseline and candidate app/prompt versions
with the same model, reasoning settings, tool definitions, permission mode and
sanitized conversation prefix. Verify the actual loaded build/prompt, not just a
Git branch. Use disposable equivalent workspaces or reset only task-owned test
files between user-run trials; never reuse a mutated workspace as the control.
Do not send live model requests or run Vega automatically for this comparison.

Use the acceptance rows as paired tasks: known reads, same-file edits, dependent
search, stale file, standalone/approval boundary, partial failure and single-call
control. Repeat paired samples in alternating order and retain failures as well
as successes; one successful batch is not proof of a general speedup. Record:

- Exact build/prompt identity, model, reasoning and sanitized task identity.
- Model response count, individual tool-call count, and operations already
  combined within shell calls; do not confuse the three.
- Wall time, model-request time, tool time and external waits separately.
- Task correctness, erroneous grouping, permission/ordering violations,
  unexpected operations and recovery attempts.

Accept the behavioral hypothesis only if comparable samples reduce unnecessary
model turns without a correctness or safety regression. Mixed or insufficient
results remain inconclusive, not a reason to weaken boundaries or silently add a
batch tool. Live comparisons and user acceptance remain NOT RUN until the user
provides results. CI passing and merge do not establish either.
