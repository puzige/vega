# Streaming credential boundaries

## Evidence and scope

The reported `getTaskPlanSnapshot` response was confirmed by an authorized,
read-only inspection of the affected installation: the persisted HTTP path ends
in `ta[REDACTED:provider_credential]`. No private conversation, credential,
database identifier or remote source content is included here.

The common-format scanner correctly rejects `sk-` inside a complete `task-`
word. The streaming projection discards emitted context, however, so a provider
split at `ta | sk-…` makes the suffix look like a standalone credential. It also
redacts an unfinished candidate before deciding what is safe to emit, which can
replace a partial secret and subsequently release its remaining bytes.

## Contract

- Retain the complete trailing token run, including ASCII letters/digits and
  `_-.+/=~`, before draining a stream buffer. This preserves the left boundary
  of an identifier or route across arbitrary provider splits.
- Compute the safe boundary on raw pending text. Retain unfinished credential
  prefixes, assignments, Bearer values, JWTs and known exact values. Do not
  restrict candidate-prefix inspection to a fixed 256-byte suffix: a longer
  credential must not lose its introducing context.
- Do not split any detected finding across the safe boundary, including known
  exact values containing whitespace or punctuation. Redact only the complete
  safe prefix; keep the raw remainder private until a later delta or final flush.
- Keep TextDelta, ThinkingDelta and SummaryDelta buffers independent. Existing
  policy exceptions and unconditional known-exact protection remain unchanged.
- No route allowlist, relaxed prefix matching, new dependency, database rewrite,
  user configuration change or automatic restoration of already-redacted text.

## Verification

Development-only Rust regressions run exclusively in GitHub Actions, covering
all two-way splits of a task route, embedded prefix, independent credential,
Bearer/assignment/JWT, long tokens and a known exact value with whitespace.
Include bytewise streaming, a threshold-length split and all three projection
channels. Preserve development source and result identities, then remove Rust
tests before the final required CI gate, as required by repository policy.
Local verification is limited to formatting and diff/static inspection. Real
provider/UI acceptance remains the user's manual check on the merged build.
